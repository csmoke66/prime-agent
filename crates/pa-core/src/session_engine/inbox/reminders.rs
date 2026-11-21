//! Non-consuming previews of waiting messages in a periodic wake. The mode is
//! local to the recipient session, not the entire session family.
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::{reminder_notice, SessionInbox};
use crate::session_engine::harness_digest::session_artifact_dir_for_log;
use crate::settings::storage::{atomic_write_with, AtomicWriteOptions};
use anyhow::{bail, Context, Result};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum ReminderMode {
    Count,
    #[default]
    Preview,
    Full,
}

impl ReminderMode {
    pub fn parse(text: &str) -> Result<Self> {
        match text {
            "count" => Ok(Self::Count),
            "preview" => Ok(Self::Preview),
            "full" => Ok(Self::Full),
            _ => bail!("Usage: /inbox-reminders [count | preview | full]"),
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Count => "count",
            Self::Preview => "preview",
            Self::Full => "full",
        }
    }
    fn file(log: &Path) -> Result<PathBuf> {
        Ok(session_artifact_dir_for_log(log)
            .context("no artifact directory for inbox settings")?
            .join("inbox-reminder-mode"))
    }
    pub fn load(log: &Path) -> Result<Self> {
        let file = Self::file(log)?;
        match std::fs::read_to_string(&file) {
            Ok(text) => Self::parse(text.trim()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error).with_context(|| format!("read {}", file.display())),
        }
    }
    pub fn save(self, log: &Path) -> Result<()> {
        let file = Self::file(log)?;
        std::fs::create_dir_all(file.parent().context("no inbox settings directory")?)?;
        let _guard = crate::platform::lock_dir::LockDir::acquire_owned_retrying(
            &file,
            Duration::from_secs(30),
            50,
            Duration::from_millis(10),
        )?;
        atomic_write_with(&file, self.name(), AtomicWriteOptions { fsync: true })
    }
}

impl SessionInbox {
    pub fn unread_reminder(&self, mode: ReminderMode) -> String {
        let state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let messages = &state.messages;
        let mut text = reminder_notice(messages.len());
        if mode == ReminderMode::Count {
            return text;
        }
        let visible = if mode == ReminderMode::Preview {
            messages.len().min(5)
        } else {
            messages.len()
        };
        for message in &messages[..visible] {
            text.push_str("\n • ");
            // Escape terminal controls, even for full bodies; untrusted message text
            // must not alter the terminal or impersonate a second reminder row.
            append_field(
                &mut text,
                message.sender.as_deref().unwrap_or(&message.class),
                Some(64),
            );
            text.push_str(": ");
            append_field(
                &mut text,
                &message.text,
                (mode == ReminderMode::Preview).then_some(200),
            );
        }
        if visible < messages.len() {
            let _ = write!(text, "\n … {} more unread", messages.len() - visible);
        }
        text
    }
}

fn append_field(out: &mut String, field: &str, limit: Option<usize>) {
    for (i, c) in field.chars().enumerate() {
        if Some(i) == limit {
            out.push('…');
            break;
        }
        if c.is_control() || matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}') {
            out.extend(c.escape_default());
        } else {
            out.push(c);
        }
    }
}
