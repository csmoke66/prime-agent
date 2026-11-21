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
            _ => bail!("Inbox reminder mode must be count, preview, or full"),
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
    /// Transfer a full mailbox under the same lock as kernel reads. The consumer
    /// must durably accept the batch before we acknowledge the inbox journal.
    pub fn deliver_full_mailbox(&self, accept: impl FnOnce(String) -> bool) -> bool {
        use super::{InboxRecord, INBOX_CLASS_AGENT_MESSAGE};
        use crate::session_engine::agent_messaging::DEFAULT_AGENT_MESSAGE_MAX_PENDING_PER_SESSION;
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state
            .messages
            .iter()
            .filter(|m| m.class == INBOX_CLASS_AGENT_MESSAGE)
            .nth(DEFAULT_AGENT_MESSAGE_MAX_PENDING_PER_SESSION - 1)
            .is_none()
        {
            return false;
        }
        let mut text = format!(
            "[inbox full] Delivering all {} waiting messages directly:",
            state.messages.len()
        );
        append_messages(&mut text, &state.messages, ReminderMode::Full);
        if !accept(text) {
            return false;
        }
        self.append_journal(
            &serde_json::to_value(InboxRecord::Drained).expect("unit journal record"),
        );
        state.messages.clear();
        true
    }

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
        append_messages(&mut text, &messages[..visible], mode);
        if visible < messages.len() {
            let _ = write!(text, "\n … {} more unread", messages.len() - visible);
        }
        text
    }
}

fn append_messages(text: &mut String, messages: &[super::InboxMessage], mode: ReminderMode) {
    for message in messages {
        text.push_str("\n • ");
        // Escape terminal controls, even for full bodies; untrusted message text
        // must not alter the terminal or impersonate a second reminder row.
        append_field(
            text,
            message.sender.as_deref().unwrap_or(&message.class),
            Some(64),
        );
        text.push_str(": ");
        append_field(
            text,
            &message.text,
            (mode == ReminderMode::Preview).then_some(200),
        );
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session_engine::agent_messaging::DEFAULT_AGENT_MESSAGE_MAX_PENDING_PER_SESSION;
    use crate::session_engine::inbox::{
        InboxNotice, INBOX_CLASS_AGENT_MESSAGE, INBOX_CLASS_HEARTBEAT,
    };

    #[test]
    fn full_handoff_keeps_unread_mail_until_the_complete_batch_is_accepted() {
        let dir = tempfile::tempdir().unwrap();
        let inbox = SessionInbox::open(Some(dir.path()));
        let long = "abc".repeat(200);
        for class in std::iter::once(INBOX_CLASS_HEARTBEAT).chain(std::iter::repeat_n(
            INBOX_CLASS_AGENT_MESSAGE,
            DEFAULT_AGENT_MESSAGE_MAX_PENDING_PER_SESSION,
        )) {
            inbox.push(
                InboxNotice {
                    class,
                    sender: None,
                    text: long.clone(),
                    meta: serde_json::Value::Null,
                },
                1,
            );
        }
        assert!(!inbox.deliver_full_mailbox(|_| false));
        drop(inbox);
        let inbox = SessionInbox::open(Some(dir.path()));
        assert!(inbox.deliver_full_mailbox(|text| {
            assert_eq!(
                text.matches(&long).count(),
                DEFAULT_AGENT_MESSAGE_MAX_PENDING_PER_SESSION + 1
            );
            assert!(text.contains("heartbeat: abc"));
            true
        }));
        drop(inbox);
        assert_eq!(SessionInbox::open(Some(dir.path())).count().0, 0);
    }
}
