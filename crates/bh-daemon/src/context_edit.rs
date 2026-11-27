//! `set_session_context`: replace the model-visible context with an edited message list.
//!
//! The session file stays append-only. The edit appends a compaction entry that hides all
//! earlier history (the mechanism context-repair workflows use), then appends the edited
//! messages as `message` entries after it, and rebuilds the engine context from the branch.

use serde_json::{json, Value};

use bh_types::session::AgentMessage;

use crate::protocol::{response_failure, response_success, DaemonResponse};
use crate::worker::Worker;

const COMMAND: &str = "set_session_context";

impl Worker {
    /// Apply an edited context. Refused while a turn or compaction runs; a list equal to the
    /// current context is a no-op (`changed: false`).
    pub(crate) async fn handle_set_session_context(&self, payload: &Value) -> DaemonResponse {
        if let Err(response) = self.require_created(COMMAND) {
            return response;
        }
        let mut messages = Vec::new();
        for (index, raw) in payload["messages"]
            .as_array()
            .into_iter()
            .flatten()
            .enumerate()
        {
            match serde_json::from_value::<AgentMessage>(raw.clone()) {
                Ok(message) => messages.push(message),
                Err(error) => {
                    let position = index + 1;
                    return response_failure(
                        None,
                        COMMAND,
                        &format!("Invalid message {position}: {error}"),
                        None,
                    );
                }
            }
        }
        let branch_entries = {
            let mut core = self.core.lock().unwrap();
            if core.busy || core.compacting {
                return response_failure(
                    None,
                    COMMAND,
                    "Cannot edit the context while a turn or compaction is running",
                    None,
                );
            }
            let Some(store) = core.store.as_mut() else {
                return response_failure(None, COMMAND, "Session is still initializing", None);
            };
            let current =
                bh_core::session::build_session_context(&store.branch_file_entries(), store.leaf_id());
            if current.messages == messages {
                return response_success(None, COMMAND, Some(json!({ "changed": false })));
            }
            let (summary, rest) = match messages.split_first() {
                Some((AgentMessage::CompactionSummary(summary), rest)) => (Some(summary), rest),
                _ => (None, messages.as_slice()),
            };
            let mut compaction = json!({
                "summary": summary.map_or("", |summary| summary.summary.as_str()),
                "firstKeptEntryId": "",
                "tokensBefore": summary.map_or(0, |summary| summary.tokens_before),
                "details": { "readFiles": [], "modifiedFiles": [] },
                "fromHook": false,
                "customInstructions": summary.and_then(|summary| summary.custom_instructions.clone()),
                "harnessDigest": summary.and_then(|summary| summary.harness_digest.clone()),
                "harnessStateFingerprint": summary
                    .and_then(|summary| summary.harness_state_fingerprint.clone()),
            });
            compaction
                .as_object_mut()
                .expect("object literal")
                .retain(|_, value| !value.is_null());
            let compaction_time = summary.map_or_else(crate::util::now_iso, |summary| {
                crate::util::iso_from_unix_ms(summary.timestamp)
            });
            let persisted = store
                .persist_entry_at("compaction", compaction, &compaction_time)
                .and_then(|_| {
                    rest.iter().try_for_each(|message| {
                        store
                            .persist_entry("message", json!({ "message": message }))
                            .map(drop)
                    })
                });
            if let Err(error) = persisted {
                return response_failure(None, COMMAND, &format!("{error:#}"), None);
            }
            let branch_entries = store.branch_file_entries();
            core.compacting = true;
            branch_entries
        };
        let rebuilt = crate::branch_navigation::rebuild_engine_context(
            &self.engine,
            branch_entries,
            bh_core::session_engine::goal_driver::GoalBranchReload::SameTimeline,
        )
        .await;
        self.core.lock().unwrap().compacting = false;
        self.idle_notify.notify_waiters();
        match rebuilt {
            Ok(()) => response_success(None, COMMAND, Some(json!({ "changed": true }))),
            Err(error) => response_failure(
                None,
                COMMAND,
                &format!("Context saved, but the live session could not rebuild it: {error}"),
                None,
            ),
        }
    }
}
