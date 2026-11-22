//! Priority-derived inbox notices and the durable full-mailbox handoff.
use std::fmt::Write as _;

use super::{arrival_notice, reminder_notice, InboxMessage, InboxPriority, SessionInbox};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DetailMode {
    Preview,
    Full,
}

impl SessionInbox {
    /// Transfer an overflowing mailbox under the same lock as kernel reads.
    /// The consumer must durably accept the complete batch before we
    /// acknowledge the inbox journal.
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
            .filter(|message| message.class == INBOX_CLASS_AGENT_MESSAGE)
            .nth(DEFAULT_AGENT_MESSAGE_MAX_PENDING_PER_SESSION - 1)
            .is_none()
        {
            return false;
        }
        let mut text = format!(
            "[inbox full] Delivering all {} waiting messages directly:",
            state.messages.len()
        );
        append_messages(&mut text, &state.messages, DetailMode::Full);
        if !accept(text) {
            return false;
        }
        self.append_journal(
            &serde_json::to_value(InboxRecord::Drained).expect("unit journal record"),
        );
        state.messages.clear();
        true
    }

    /// Transfer only high-priority messages. Unrelated pending mail remains in
    /// both memory and the durable journal; a rejected queue checkpoint leaves
    /// the entire inbox untouched.
    pub fn deliver_high_messages(&self, accept: impl FnOnce(String) -> bool) -> bool {
        use super::InboxRecord;
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let count = state
            .messages
            .iter()
            .filter(|message| message.priority() == InboxPriority::High)
            .count();
        if count == 0 {
            return false;
        }
        let mut text =
            format!("[inbox high] Delivering {count} high-priority messages directly:");
        for message in state
            .messages
            .iter()
            .filter(|message| message.priority() == InboxPriority::High)
        {
            append_message(&mut text, message, DetailMode::Full);
        }
        if !accept(text) {
            return false;
        }
        let ids = state
            .messages
            .iter()
            .filter(|message| message.priority() == InboxPriority::High)
            .map(|message| message.id.clone())
            .collect();
        self.append_journal(
            &serde_json::to_value(InboxRecord::Read { ids }).expect("read journal record"),
        );
        state
            .messages
            .retain(|message| message.priority() != InboxPriority::High);
        true
    }

    /// Render one coalesced arrival batch per message: default and low
    /// contribute only to the count, medium contributes a bounded preview,
    /// and high contributes its complete body.
    #[must_use]
    pub fn arrival_notification(
        &self,
        batch: &[InboxMessage],
        total_waiting: usize,
    ) -> (String, String) {
        let (preview, mut text) = arrival_notice(batch, total_waiting);
        append_priority_details(&mut text, batch);
        (preview, text)
    }

    /// Render unread low-priority mail for an unrelated turn directly under
    /// the store lock. Low messages remain unread and therefore may accompany
    /// later unrelated wakes until the recipient reads them.
    #[must_use]
    pub fn held_low_notification(&self) -> Option<(String, String)> {
        let state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.messages.is_empty()
            || state
                .messages
                .iter()
                .any(|message| message.priority() != InboxPriority::Low)
        {
            return None;
        }
        Some(self.arrival_notification(&state.messages, state.messages.len()))
    }

    /// The periodic reminder selected by the strongest waiting message.
    /// A low-only mailbox is silent; machine notices and missing priorities
    /// retain the historical count reminder.
    #[must_use]
    pub fn unread_reminder(&self) -> Option<(InboxPriority, String)> {
        let state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let messages = &state.messages;
        let priority = effective_priority(messages);
        if messages.is_empty() || priority == InboxPriority::Low {
            return None;
        }
        let mut text = reminder_notice(messages.len());
        append_priority_details(&mut text, messages);
        Some((priority, text))
    }
}

fn effective_priority(messages: &[InboxMessage]) -> InboxPriority {
    messages
        .iter()
        .map(InboxMessage::priority)
        .max()
        .unwrap_or(InboxPriority::Low)
}

fn append_priority_details(text: &mut String, messages: &[InboxMessage]) {
    let mut shown_medium = 0usize;
    let mut omitted_medium = 0usize;
    for message in messages {
        match message.priority() {
            InboxPriority::Medium if shown_medium < 5 => {
                append_message(text, message, DetailMode::Preview);
                shown_medium += 1;
            }
            InboxPriority::Medium => omitted_medium += 1,
            InboxPriority::High => append_message(text, message, DetailMode::Full),
            InboxPriority::Low | InboxPriority::Default => {}
        }
    }
    if omitted_medium > 0 {
        let _ = write!(text, "\n … {omitted_medium} more medium-priority unread");
    }
}

fn append_messages(text: &mut String, messages: &[InboxMessage], mode: DetailMode) {
    for message in messages {
        append_message(text, message, mode);
    }
}

fn append_message(text: &mut String, message: &InboxMessage, mode: DetailMode) {
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
        (mode == DetailMode::Preview).then_some(200),
    );
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

    #[test]
    fn high_handoff_journals_a_selective_read_and_keeps_other_mail() {
        let dir = tempfile::tempdir().unwrap();
        let inbox = SessionInbox::open(Some(dir.path()));
        for (priority, text) in [("low", "background"), ("high", "urgent")] {
            inbox.push(
                InboxNotice {
                    class: INBOX_CLASS_AGENT_MESSAGE,
                    sender: Some("peer".to_string()),
                    text: text.to_string(),
                    meta: serde_json::json!({ "priority": priority }),
                },
                1,
            );
        }
        assert!(!inbox.deliver_high_messages(|_| false));
        assert_eq!(inbox.count().0, 2, "rejection changes nothing");
        assert!(inbox.deliver_high_messages(|text| {
            assert!(text.contains("urgent"));
            assert!(!text.contains("background"));
            true
        }));
        assert_eq!(inbox.count().0, 1);
        drop(inbox);
        let reopened = SessionInbox::open(Some(dir.path()));
        let waiting = reopened.waiting_messages();
        assert_eq!(waiting.len(), 1);
        assert_eq!(waiting[0].text, "background");
    }
}
