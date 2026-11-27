// large_futures: stack-resident futures on hot paths by design.
// too_many_lines: style gate, not correctness. Casts: 64-bit targets;
// narrowing sits at bounded OS/protocol boundaries.
#![allow(
    clippy::large_futures,
    clippy::too_many_lines,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]

//! Release workflow gates: the Windows build job shares the packaging action
//! with the standalone Windows workflow, links the static VC runtime, and the
//! promote job waits for it. Releases publish to GitHub Releases only.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

/// The repo root (crates/bh-cli -> crates -> root): the workflows live there.
fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .map(Path::to_path_buf)
        .expect("worktree root")
}

#[derive(Clone, Deserialize)]
struct Workflow {
    jobs: std::collections::BTreeMap<String, Job>,
}

#[derive(Clone, Deserialize)]
struct Job {
    #[serde(default)]
    steps: Vec<Step>,
}

#[derive(Deserialize)]
struct CompositeAction {
    runs: Job,
}

#[derive(Clone, Deserialize)]
struct Step {
    name: Option<String>,
    uses: Option<String>,
    run: Option<String>,
    #[serde(default)]
    with: Option<serde_yaml::Value>,
    #[serde(default)]
    env: Option<serde_yaml::Value>,
}

const WINDOWS_RELEASE_ACTION: &str = "./.github/actions/assemble-windows-release";

fn windows_release_steps() -> Vec<Step> {
    let action_path = repo_root().join(WINDOWS_RELEASE_ACTION).join("action.yml");
    let text = fs::read_to_string(action_path).expect("read Windows release action");
    let action: CompositeAction = serde_yaml::from_str(&text).expect("parse Windows release action");
    action.runs.steps
}

#[test]
fn windows_build_job_contract() {
    let text = fs::read_to_string(repo_root().join(".github/workflows/release.yml"))
        .expect("read release.yml");
    let workflow: Workflow = serde_yaml::from_str(&text).expect("release.yml parses as YAML");
    let windows = workflow
        .jobs
        .get("build-windows")
        .expect("the build-windows job exists");
    let fork = fs::read_to_string(repo_root().join(".github/workflows/windows-release.yml"))
        .expect("read the fork's Windows release workflow");
    let fork: Workflow = serde_yaml::from_str(&fork).expect("parse the fork's Windows workflow");
    for job in [windows, fork.jobs.get("build").expect("the fork's build job")] {
        assert!(
            job.steps
                .iter()
                .any(|step| step.uses.as_deref() == Some(WINDOWS_RELEASE_ACTION)),
            "both Windows releases must use the shared packaging action"
        );
    }
    let steps = windows_release_steps();
    assert!(
        steps.iter().any(|step| {
            step.name.as_deref() == Some("Livecheck gate: --version prints the workspace version")
                && step
                    .run
                    .as_deref()
                    .is_some_and(|run| run.contains("release/beehive.exe"))
        }),
        "the Windows livecheck must name the .exe binary Cargo's MSVC linker emits"
    );
    assert!(
        steps.iter().any(|step| {
            step.run.as_deref().is_some_and(|run| {
                run.contains("cargo build --release --locked --target")
                    && !run.contains("dist/beehive")
            })
        }),
        "the Windows build compiles the MSVC target (the split-debug step is linux-only)"
    );
    assert!(
        text.contains("needs: [build-gnu, build-darwin, build-windows]")
            && text.contains("needs.build-windows.result == 'success'"),
        "promote must wait for the Windows build"
    );
}

/// The shipped Windows binary must link the STATIC VC runtime: the
/// MSVC target links vcruntime dynamically by default, the windows-2022
/// runner image carries vcruntime140.dll, and a stock Windows machine does
/// not - the dynamically linked launcher dies at process start
/// (0xC0000135, `STATUS_DLL_NOT_FOUND`, exit -1073741515) before main runs.
/// The operator hit exactly that on a real box (2026-10-06), so the
/// crt-static pin is the build step's own env: only the SHIPPED binary is
/// static, and never through Cargo.toml, whose dev builds stay dynamic.
#[test]
fn the_windows_build_links_the_static_vc_runtime() {
    let text = fs::read_to_string(repo_root().join(".github/workflows/release.yml"))
        .expect("read release.yml");
    let workflow: Workflow = serde_yaml::from_str(&text).expect("release.yml parses as YAML");
    let steps = windows_release_steps();
    let build = steps
        .iter()
        .find(|step| {
            step.name.as_deref() == Some("Build (release, locked)")
                && step
                    .run
                    .as_deref()
                    .is_some_and(|run| run.contains("cargo build --release --locked"))
        })
        .expect("the shared Windows release build step exists");
    let rustflags = build
        .env
        .as_ref()
        .and_then(|env| env.get("RUSTFLAGS"))
        .and_then(serde_yaml::Value::as_str)
        .expect("the Windows build step must set RUSTFLAGS");
    assert!(
        rustflags.contains("target-feature=+crt-static"),
        "the Windows build must link the static VC runtime (RUSTFLAGS carries target-feature=+crt-static), got: {rustflags}"
    );
    // The pin is the Windows build step's alone: the gnu and darwin release
    // builds stay exactly as they are (crt-static would trade their libc
    // contract for nothing a shipped binary needs).
    for job in ["build-gnu", "build-darwin"] {
        for step in &workflow.jobs.get(job).expect("job exists").steps {
            if let Some(run) = step.run.as_deref() {
                if run.contains("cargo build --release --locked") {
                    assert!(
                        !step
                            .env
                            .as_ref()
                            .and_then(|env| env.get("RUSTFLAGS"))
                            .and_then(serde_yaml::Value::as_str)
                            .is_some_and(|flags| flags.contains("crt-static")),
                        "crt-static is the Windows launcher's fix, never a gnu/darwin flag"
                    );
                }
            }
        }
    }
    // Dev builds stay dynamic: the pin lives in the release workflow alone,
    // never in Cargo.toml (whose every target and profile would go static).
    let cargo_toml = fs::read_to_string(repo_root().join("Cargo.toml")).expect("read Cargo.toml");
    assert!(
        !cargo_toml.contains("crt-static"),
        "crt-static belongs to the release workflow's Windows build step, not Cargo.toml: dev builds link the runtime dynamically"
    );
}

#[test]
fn release_publishes_only_to_github_releases() {
    let text = fs::read_to_string(repo_root().join(".github/workflows/release.yml"))
        .expect("read release.yml");
    let workflow: Workflow = serde_yaml::from_str(&text).expect("release.yml parses as YAML");
    let promote = workflow.jobs.get("promote").expect("the promote job exists");
    assert!(
        promote
            .steps
            .iter()
            .any(|step| step.uses.as_deref().is_some_and(|uses| uses.starts_with("softprops/action-gh-release"))),
        "promote attaches the archives to the GitHub release"
    );
    assert!(
        !text.contains("aws s3"),
        "no step uploads to external object storage"
    );
}
