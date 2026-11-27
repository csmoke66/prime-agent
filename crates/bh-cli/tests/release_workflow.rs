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

//! Release workflow gate: releases publish to GitHub Releases only.

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

#[derive(Deserialize)]
struct Workflow {
    jobs: std::collections::BTreeMap<String, Job>,
}

#[derive(Deserialize)]
struct Job {
    #[serde(default)]
    steps: Vec<Step>,
}

#[derive(Deserialize)]
struct Step {
    uses: Option<String>,
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
