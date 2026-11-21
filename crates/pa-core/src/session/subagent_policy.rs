//! The durable child-model policy owned by one root session. Readers see the old or new
//! document; concurrent TUI edits to independent fields merge under the same lock.
use std::borrow::Cow;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::platform::lock_dir::LockDir;
use crate::session_engine::harness_digest::session_artifact_dir_for_log;
use crate::settings::storage::{atomic_write_with, AtomicWriteOptions};
use crate::settings::ThinkingLevelSetting;

#[derive(Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionSubagentPolicy {
    /// None inherits global allowedModels; an explicit empty list is unrestricted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allowed_models: Option<Vec<String>>,
    /// None leaves normal spawn/parent effort resolution unchanged.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effort: Option<ThinkingLevelSetting>,
}

impl SessionSubagentPolicy {
    /// Follow RLM child headers to the session that owns the policy. A normal
    /// depth-zero fork is independent even when its header records a parent.
    pub fn owner_log(session_log: &Path, depth: u32) -> Result<Cow<'_, Path>> {
        let mut log = Cow::Borrowed(session_log);
        for expected in (1..=depth).rev() {
            let header = crate::session::manager::read_session_header(&log)
                .with_context(|| format!("cannot read sub-agent ancestry at {}", log.display()))?;
            anyhow::ensure!(
                header.rlm_depth == Some(u64::from(expected)),
                "inconsistent sub-agent ancestry at {}",
                log.display()
            );
            log = Cow::Owned(PathBuf::from(
                header
                    .parent_session
                    .context("sub-agent has no parent session log")?,
            ));
        }
        Ok(log)
    }

    fn file(session_log: &Path) -> Result<PathBuf> {
        Ok(session_artifact_dir_for_log(session_log)
            .context("no artifact directory for this session log")?
            .join("subagent-policy.json"))
    }

    fn read(path: &Path) -> Result<Self> {
        match fs::read(path) {
            Ok(bytes) => {
                let policy: Self = serde_json::from_slice(&bytes)
                    .with_context(|| format!("invalid sub-agent policy at {}", path.display()))?;
                Self::validate_models(policy.allowed_models.as_deref())?;
                Ok(policy)
            }
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error).with_context(|| format!("read {}", path.display())),
        }
    }

    /// Load the current policy without creating any files. Only absence means inherit.
    pub fn load(session_log: &Path) -> Result<Self> {
        Self::read(&Self::file(session_log)?)
    }

    fn update(session_log: &Path, change: impl FnOnce(&mut Self)) -> Result<Self> {
        let path = Self::file(session_log)?;
        fs::create_dir_all(path.parent().context("missing policy directory")?)?;
        let _guard = LockDir::acquire_owned_retrying(
            &path,
            Duration::from_secs(30),
            50,
            Duration::from_millis(10),
        )?;
        let mut policy = Self::read(&path)?;
        change(&mut policy);
        atomic_write_with(
            &path,
            &serde_json::to_string_pretty(&policy)?,
            AtomicWriteOptions { fsync: true },
        )?;
        Ok(policy)
    }

    fn validate_models(models: Option<&[String]>) -> Result<()> {
        for pattern in models.unwrap_or_default() {
            anyhow::ensure!(!pattern.trim().is_empty(), "empty sub-agent model pattern");
            if pattern.contains(['*', '?', '[']) {
                globset::Glob::new(pattern)
                    .with_context(|| format!("invalid sub-agent model pattern '{pattern}'"))?;
            }
        }
        Ok(())
    }

    /// Change only the model override; an effort set by another client is preserved.
    pub fn set_models(session_log: &Path, models: Option<Vec<String>>) -> Result<Self> {
        Self::validate_models(models.as_deref())?;
        Self::update(session_log, |policy| policy.allowed_models = models)
    }

    /// Change only the effort override; an allowlist set by another client is preserved.
    pub fn set_effort(session_log: &Path, effort: Option<ThinkingLevelSetting>) -> Result<Self> {
        Self::update(session_log, |policy| policy.effort = effort)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_child_resolves_the_same_policy_after_a_root_edit() {
        let dir = tempfile::tempdir().unwrap();
        let logs = dir.path().join("sessions");
        fs::create_dir(&logs).unwrap();
        let root = logs.join("root.jsonl");
        let child = logs.join("child.jsonl");
        let nested = logs.join("nested.jsonl");
        for (file, depth, parent) in [(&child, 1, &root), (&nested, 2, &child)] {
            fs::write(
                file,
                serde_json::json!({
                    "type":"session", "id": "id", "cwd":"/", "timestamp":"2026-01-01",
                    "rlmDepth": depth, "parentSession": parent
                })
                .to_string(),
            )
            .unwrap();
        }
        let owner = SessionSubagentPolicy::owner_log(&nested, 2).unwrap();
        assert_eq!(owner.as_ref(), root.as_path());
        SessionSubagentPolicy::set_effort(&root, Some(ThinkingLevelSetting::Low)).unwrap();
        assert_eq!(
            SessionSubagentPolicy::load(&owner).unwrap().effort,
            Some(ThinkingLevelSetting::Low)
        );
        assert!(SessionSubagentPolicy::owner_log(&nested, 1).is_err());
    }

    #[test]
    fn independent_edits_survive_resume_and_do_not_leak_to_another_session() {
        let root = tempfile::tempdir().unwrap();
        let a = root.path().join("sessions/a.jsonl");
        let b = root.path().join("sessions/b.jsonl");
        std::thread::scope(|scope| {
            scope.spawn(|| SessionSubagentPolicy::set_models(&a, Some(vec![])).unwrap());
            scope.spawn(|| {
                SessionSubagentPolicy::set_effort(&a, Some(ThinkingLevelSetting::High)).unwrap()
            });
        });
        assert_eq!(
            SessionSubagentPolicy::load(&a).unwrap(),
            SessionSubagentPolicy {
                allowed_models: Some(vec![]),
                effort: Some(ThinkingLevelSetting::High),
            }
        );
        assert_eq!(
            SessionSubagentPolicy::load(&b).unwrap(),
            SessionSubagentPolicy::default()
        );
        SessionSubagentPolicy::set_models(&a, None).unwrap();
        assert_eq!(
            SessionSubagentPolicy::load(&a).unwrap(),
            SessionSubagentPolicy {
                allowed_models: None,
                effort: Some(ThinkingLevelSetting::High),
            }
        );
    }

    #[test]
    fn malformed_policy_is_not_ignored_or_replaced_by_an_edit() {
        let root = tempfile::tempdir().unwrap();
        let log = root.path().join("sessions/a.jsonl");
        let file = SessionSubagentPolicy::file(&log).unwrap();
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, "{broken").unwrap();
        assert!(SessionSubagentPolicy::set_effort(&log, None).is_err());
        assert_eq!(fs::read_to_string(file).unwrap(), "{broken");
    }
}
