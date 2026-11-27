use std::fs as stdfs;
use std::path::{Path, PathBuf};

use serde_json::json;
use tempfile::tempdir;

use super::*;

fn workflow(instructions: &str) -> Workflow {
    Workflow {
        version: 1,
        description: Some("A review workflow".into()),
        model: "openai-codex/gpt-6.2-astra".into(),
        effort: Some("high".into()),
        subagents: Some(WorkflowSubagents {
            allowed_models: Some(vec!["openai-codex/gpt-6.2-sol".into()]),
            effort: Some("medium".into()),
        }),
        override_subagent_models: false,
        context: None,
        harness_memory: None,
        interactive_apply_context_field: None,
        prompt: instructions.into(),
        input_fields: Fields::new(),
        output_fields: Fields::new(),
    }
}

fn project_path(cwd: &Path) -> PathBuf {
    cwd.join(".beehive/workflows")
}

fn create_project(cwd: &Path, name: &str, definition: &Workflow) -> Result<()> {
    create_in(
        cwd,
        name,
        OsStr::new(&definition_name(name)?),
        definition,
        None,
    )
}

fn load_project(cwd: &Path, name: &str) -> Result<Workflow> {
    load_from(cwd, name, OsStr::new(&definition_name(name)?), None, None)
}

fn write_definition(dir: &Path, name: &str, definition: &serde_json::Value) {
    stdfs::create_dir_all(dir).unwrap();
    stdfs::write(dir.join(format!("{name}.json")), definition.to_string()).unwrap();
}

#[test]
fn creates_lists_and_loads_inline_without_overwriting() {
    let project = tempdir().unwrap();
    let cwd = project.path();
    let original = workflow("Review the change.");
    assert!(list_from(cwd, None, None).unwrap().is_empty());
    assert!(!project_path(cwd).exists());
    create_project(cwd, "review", &original).unwrap();
    assert!(create_project(cwd, "review", &workflow("replacement")).is_err());
    let file = project_path(cwd).join("review.json");
    let saved: serde_json::Value = serde_json::from_slice(&stdfs::read(file).unwrap()).unwrap();
    assert_eq!(
        saved["subagents"]["allowedModels"],
        json!(["openai-codex/gpt-6.2-sol"])
    );
    assert_eq!(load_project(cwd, "review").unwrap(), original);
    assert_eq!(list_from(cwd, None, None).unwrap(), ["review"]);
    assert_eq!(stdfs::read_dir(project_path(cwd)).unwrap().count(), 1);
}

#[test]
fn rejects_nonportable_names_before_creating_directories() {
    let project = tempdir().unwrap();
    let cwd = project.path();
    for name in [
        "",
        ".",
        "..",
        "../outside",
        "/tmp/name",
        "a/b",
        "a\\b",
        "C:stream",
        "Review",
        "x.json",
        "con",
        "lpt9",
    ] {
        assert!(
            create(cwd, name, &workflow("Instructions")).is_err(),
            "{name:?}"
        );
        assert!(load(cwd, cwd, name).is_err(), "{name:?}");
    }
    assert!(!project_path(cwd).exists());
}

#[test]
fn validates_create_and_disk_schema() {
    let project = tempdir().unwrap();
    let cwd = project.path();
    let mut invalid = workflow("Review.");
    invalid.version = 2;
    assert!(create_project(cwd, "review", &invalid).is_err());
    assert!(!project_path(cwd).exists());

    let mut valid = serde_json::to_value(workflow("Review.")).unwrap();
    let mut cases = vec![json!({"model":"provider/model", "instructions":"Go"})];
    for (field, value) in [
        ("version", json!(2)),
        ("locked", json!(true)),
        ("overrideSubagentModels", json!(true)),
        ("prompt", json!(" \t")),
        ("instructionsFile", json!("instructions.md")),
        ("model", json!("bare-model")),
        ("effort", json!("invalid")),
        ("subagents", json!({"effort":"invalid"})),
        ("subagents", json!({"allowedModels":["provider/*"]})),
        ("context", json!({"includeParentSession":true,"maxCharacters":1})),
        ("interactiveApplyContextField", json!("missing")),
    ] {
        let previous = valid.as_object_mut().unwrap().insert(field.into(), value);
        cases.push(valid.clone());
        match previous {
            Some(value) => valid[field] = value,
            None => {
                valid.as_object_mut().unwrap().remove(field);
            }
        }
    }
    for invalid in cases {
        write_definition(&project_path(cwd), "invalid", &invalid);
        assert!(load_project(cwd, "invalid").is_err(), "{invalid}");
    }
}

#[test]
fn user_registry_is_global_fallback_and_project_definitions_override_it() {
    let project = tempdir().unwrap();
    let user = tempdir().unwrap();
    let user_registry = user.path().join("workflows");
    write_definition(
        &user_registry,
        "review",
        &serde_json::to_value(workflow("user")).unwrap(),
    );
    write_definition(
        &user_registry,
        "global-only",
        &serde_json::to_value(workflow("global")).unwrap(),
    );
    write_definition(
        &project_path(project.path()),
        "review",
        &serde_json::to_value(workflow("project")).unwrap(),
    );
    let user = Directory::project_root(&user_registry).unwrap();
    let filename = OsStr::new("review.json");
    assert_eq!(
        load_from(project.path(), "review", filename, None, Some(&user))
            .unwrap()
            .prompt,
        "project"
    );
    assert_eq!(
        list_from(project.path(), None, Some(&user)).unwrap(),
        ["global-only", "review"]
    );
    stdfs::remove_file(project_path(project.path()).join(filename)).unwrap();
    assert_eq!(
        load_from(project.path(), "review", filename, None, Some(&user))
            .unwrap()
            .prompt,
        "user"
    );
}

#[test]
fn operator_precedence_and_reservation_never_depend_on_definition_data() {
    let project = tempdir().unwrap();
    let cwd = project.path();
    let admin = tempdir().unwrap();
    // The production boundary is checked before a Directory is returned.
    // Use a private fixture directory to test registry lookup, not a public
    // env/flag for redirecting the privileged location.
    let operator = Directory::project_root(admin.path()).unwrap();
    let registry = project_path(cwd);
    write_definition(
        &registry,
        "review",
        &serde_json::to_value(workflow("project")).unwrap(),
    );
    write_definition(
        admin.path(),
        "review",
        &serde_json::to_value(workflow("from operator")).unwrap(),
    );
    stdfs::create_dir(registry.join("directory.json")).unwrap();
    stdfs::write(registry.join("unrelated.txt"), "").unwrap();
    write_definition(
        &registry,
        "additional",
        &serde_json::to_value(workflow("other")).unwrap(),
    );
    let filename = OsStr::new("review.json");
    assert_eq!(
        load_from(cwd, "review", filename, Some(&operator), None)
            .unwrap()
            .prompt,
        "from operator"
    );
    assert_eq!(
        list_from(cwd, Some(&operator), None).unwrap(),
        ["additional", "review"]
    );
    stdfs::write(admin.path().join("review.json"), "not json").unwrap();
    assert!(load_from(cwd, "review", filename, Some(&operator), None).is_err());
    stdfs::remove_file(registry.join(filename)).unwrap();
    assert!(create_in(
        cwd,
        "review",
        filename,
        &workflow("shadow"),
        Some(&operator)
    )
    .is_err());
    assert!(!registry.join(filename).exists());
}

#[test]
fn rejects_symlink_directories_and_files() {
    use std::os::unix::fs::symlink;

    let project = tempdir().unwrap();
    let outside = tempdir().unwrap();
    let cwd = project.path();
    let beehive_dir = cwd.join(".beehive");
    let registry = project_path(cwd);
    let valid = workflow("Review.");
    let victim = outside.path().join("victim.md");
    stdfs::write(&victim, "untouched").unwrap();

    symlink(outside.path(), &beehive_dir).unwrap();
    assert!(create_project(cwd, "review", &valid).is_err());
    assert!(!outside.path().join("workflows").exists());
    stdfs::remove_file(&beehive_dir).unwrap();
    stdfs::create_dir(&beehive_dir).unwrap();
    symlink(outside.path(), &registry).unwrap();
    assert!(create_project(cwd, "review", &valid).is_err());
    assert!(!outside.path().join("review.json").exists());
    stdfs::remove_file(&registry).unwrap();
    stdfs::create_dir(&registry).unwrap();

    symlink(&victim, registry.join("review.json")).unwrap();
    assert!(create_project(cwd, "review", &valid).is_err());
    assert!(load_project(cwd, "review").is_err());
    symlink(
        outside.path().join("does-not-exist"),
        registry.join("dangling.json"),
    )
    .unwrap();
    assert!(create_project(cwd, "dangling", &valid).is_err());
    assert!(!outside.path().join("does-not-exist").exists());
    stdfs::hard_link(&victim, registry.join("hardlink.json")).unwrap();
    assert!(create_project(cwd, "hardlink", &valid).is_err());
    assert_eq!(stdfs::read_to_string(victim).unwrap(), "untouched");
}

#[test]
fn concurrent_creators_publish_exactly_one_complete_definition() {
    let project = tempdir().unwrap();
    let cwd = project.path();
    let results = std::thread::scope(|scope| {
        let first = scope.spawn(|| create_project(cwd, "review", &workflow("first")));
        let second = scope.spawn(|| create_project(cwd, "review", &workflow("second")));
        [first.join().unwrap(), second.join().unwrap()]
    });
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    let expected = if results[0].is_ok() {
        "first"
    } else {
        "second"
    };
    assert_eq!(load_project(cwd, "review").unwrap().prompt, expected);
}
