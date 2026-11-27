//! Session-local edits to the named parts of a system prompt.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use bh_types::daemon::{PromptSegmentEdit, PromptSegmentState, SessionPromptSegment};
use serde::{Deserialize, Serialize};

use super::SystemPromptBreakdown;
use crate::settings::{atomic_write_with, AtomicWriteOptions};

#[derive(Clone, Default, Serialize, Deserialize)]
struct Edits {
    /// Absent entries keep their defaults; `null` suppresses a base segment.
    defaults: BTreeMap<String, Option<String>>,
    added: BTreeMap<String, String>,
}

pub(crate) struct SessionPrompt {
    base: SystemPromptBreakdown,
    path: Option<PathBuf>,
    edits: Edits,
}

impl SessionPrompt {
    pub(crate) async fn open(
        base: SystemPromptBreakdown,
        artifacts: Option<&Path>,
    ) -> Result<Self> {
        let path = artifacts.map(|dir| dir.join("prompt-segments.json"));
        let edits = if let Some(path) = path.clone() {
            tokio::task::spawn_blocking(move || -> Result<Edits> {
                match std::fs::read(&path) {
                    Ok(bytes) => serde_json::from_slice(&bytes)
                        .with_context(|| format!("read prompt segments from {}", path.display())),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        Ok(Edits::default())
                    }
                    Err(error) => Err(error).with_context(|| format!("open {}", path.display())),
                }
            })
            .await??
        } else {
            Edits::default()
        };
        Ok(Self { base, path, edits })
    }

    pub(crate) fn effective(&self) -> String {
        if self.edits.defaults.is_empty() && self.edits.added.is_empty() {
            return self.base.assembled.clone();
        }
        let mut rendered = Vec::new();
        for layer in &self.base.segments {
            let mut text = String::new();
            let mut after = "";
            for (name, initial, separator) in layer.named_parts() {
                let body = match self.edits.defaults.get(name) {
                    None => initial,
                    Some(Some(body)) => body,
                    Some(None) => continue,
                };
                text.push_str(after);
                text.push_str(body);
                after = separator;
            }
            if !text.is_empty() {
                rendered.push(text);
            }
        }
        rendered.extend(self.edits.added.values().cloned());
        rendered.join("\n\n")
    }

    pub(crate) fn list(&self) -> Vec<SessionPromptSegment> {
        let mut rows: Vec<_> = self
            .base
            .segments
            .iter()
            .flat_map(|layer| layer.named_parts())
            .map(|(name, initial, _)| {
                let (body, state) = match self.edits.defaults.get(name) {
                    None => (initial, PromptSegmentState::Default),
                    Some(Some(body)) => (body.as_str(), PromptSegmentState::Modified),
                    Some(None) => (initial, PromptSegmentState::Removed),
                };
                SessionPromptSegment {
                    name: name.to_owned(),
                    body: body.to_owned(),
                    state,
                }
            })
            .collect();
        rows.extend(
            self.edits
                .added
                .iter()
                .map(|(name, body)| SessionPromptSegment {
                    name: name.clone(),
                    body: body.clone(),
                    state: PromptSegmentState::Added,
                }),
        );
        rows
    }

    pub(crate) async fn edit(&mut self, edit: PromptSegmentEdit) -> Result<()> {
        let mut next = self.edits.clone();
        let exists = |name: &str| {
            self.base
                .segments
                .iter()
                .flat_map(|layer| layer.named_parts())
                .any(|(other, _, _)| name == other)
        };
        match edit {
            PromptSegmentEdit::Add { name, body } => {
                anyhow::ensure!(
                    !name.is_empty()
                        && name.len() <= 80
                        && name
                            .bytes()
                            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_'),
                    "Use 1–80 lowercase letters, digits, or underscores for a segment name"
                );
                anyhow::ensure!(
                    !exists(&name)
                        && !next.added.contains_key(&name)
                        && !next.defaults.contains_key(&name),
                    "Segment {name} already exists"
                );
                anyhow::ensure!(!body.trim().is_empty(), "The segment body is empty");
                next.added.insert(name, body);
            }
            PromptSegmentEdit::Replace { name, body } => {
                anyhow::ensure!(
                    !body.trim().is_empty(),
                    "The segment body is empty; use Remove instead"
                );
                if let Some(previous) = next.added.get_mut(&name) {
                    *previous = body;
                } else {
                    anyhow::ensure!(exists(&name), "Unknown prompt segment {name}");
                    next.defaults.insert(name, Some(body));
                }
            }
            PromptSegmentEdit::Remove { name } => {
                if next.added.remove(&name).is_none() {
                    anyhow::ensure!(exists(&name), "Unknown prompt segment {name}");
                    next.defaults.insert(name, None);
                }
            }
            PromptSegmentEdit::Reset { name } => {
                anyhow::ensure!(exists(&name), "Unknown built-in prompt segment {name}");
                next.defaults.remove(&name);
            }
        }
        if let Some(path) = self.path.clone() {
            let json = serde_json::to_string_pretty(&next)?;
            tokio::task::spawn_blocking(move || -> Result<()> {
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                atomic_write_with(&path, &json, AtomicWriteOptions { fsync: true })
                    .with_context(|| format!("save prompt segments to {}", path.display()))
            })
            .await??;
        }
        self.edits = next;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prompts::{system_prompt_breakdown, BuildSystemPromptOptions};

    #[tokio::test]
    async fn named_edits_survive_reopen_and_are_local_to_one_session() {
        let first = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        let base = system_prompt_breakdown(&BuildSystemPromptOptions::default());
        let original = base.assembled.clone();
        let mut prompt = SessionPrompt::open(base.clone(), Some(first.path()))
            .await
            .unwrap();
        let name = "releasing_tool_outputs".to_owned();
        let before = prompt
            .list()
            .into_iter()
            .find(|row| row.name == name)
            .unwrap()
            .body;
        let replacement = "First line.\nSecond line.".to_owned();
        prompt
            .edit(PromptSegmentEdit::Replace {
                name: name.clone(),
                body: replacement.clone(),
            })
            .await
            .unwrap();
        assert_eq!(
            prompt.effective(),
            original.replacen(&before, &replacement, 1)
        );
        prompt
            .edit(PromptSegmentEdit::Remove { name: name.clone() })
            .await
            .unwrap();
        let added = "additional_guidance".to_owned();
        prompt
            .edit(PromptSegmentEdit::Add {
                name: added.clone(),
                body: "Added guidance.".to_owned(),
            })
            .await
            .unwrap();

        let mut reopened = SessionPrompt::open(base.clone(), Some(first.path()))
            .await
            .unwrap();
        let unaffected = SessionPrompt::open(base, Some(other.path())).await.unwrap();
        assert_eq!(reopened.effective(), prompt.effective());
        assert_eq!(unaffected.effective(), original);
        assert!(!reopened.effective().contains(&before));
        assert!(reopened.effective().ends_with("\n\nAdded guidance."));
        reopened
            .edit(PromptSegmentEdit::Reset { name })
            .await
            .unwrap();
        reopened
            .edit(PromptSegmentEdit::Remove { name: added })
            .await
            .unwrap();
        assert_eq!(reopened.effective(), original);
    }
}
