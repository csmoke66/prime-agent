//! `/edit_context`: edit the model-visible context in `$VISUAL`/`$EDITOR`. The dump is the
//! `get_session_context` message array as pretty JSON; the saved text goes back through
//! `set_session_context`. A rejected save is kept as a draft the next `/edit_context` reopens.
use super::{
    AgentView, DaemonCommand, Duration, Map, Result, SessionUi, StatusKind, Value,
    UI_REQUEST_TIMEOUT_MS,
};
use crate::external_editor::EditorTarget;

impl SessionUi {
    /// Fetch the context and queue the editor handoff the interactive loop performs.
    pub(crate) async fn handle_edit_context_command(&mut self, view: &mut AgentView) -> Result<()> {
        self.track_feature_outcome("edit_context", "initiated", None);
        if self.turn_active || view.working.is_some() || self.work_in_flight() {
            self.note_as(
                "Wait for the current response to finish before editing the context.",
                StatusKind::Warning,
                view,
            );
            self.track_feature_outcome("edit_context", "unavailable", None);
            return Ok(());
        }
        let Some(command) = crate::external_editor::editor_command() else {
            self.note_as(
                "\u{26a0} No editor configured. Set $VISUAL or $EDITOR environment variable.",
                StatusKind::Warning,
                view,
            );
            self.track_feature_outcome("edit_context", "unavailable", None);
            return Ok(());
        };
        let data = self
            .bounded_request(
                Duration::from_millis(UI_REQUEST_TIMEOUT_MS),
                DaemonCommand::GetSessionContext {
                    id: None,
                    active_session_id: self.active_session_id.clone(),
                    rest: Map::default(),
                },
            )
            .await;
        let dump = data.and_then(|data| {
            Ok(serde_json::to_string_pretty(&data["context"]["messages"])?)
        });
        match dump {
            Ok(dump) => {
                let text = self.context_edit_draft.take().unwrap_or_else(|| dump.clone());
                self.context_edit_baseline = dump;
                self.external_editor_request = Some((command, EditorTarget::Context(text)));
            }
            Err(error) => {
                self.error_row(&format!("{error:#}"), view);
                self.track_feature_outcome("edit_context", "failed", None);
            }
        }
        Ok(())
    }

    /// The settled editor run: an unchanged, empty, or non-zero-exit save aborts; anything else
    /// is sent to the daemon, and a rejection keeps the text as the retry draft.
    pub(crate) async fn apply_context_edit_outcome(
        &mut self,
        outcome: anyhow::Result<Option<String>>,
        view: &mut AgentView,
    ) {
        self.dirty = true;
        let text = match outcome {
            Err(error) => {
                self.error_row(&format!("{error:#}"), view);
                self.track_feature_outcome("edit_context", "failed", None);
                return;
            }
            Ok(text) => text.filter(|text| !text.trim().is_empty()),
        };
        let Some(text) = text else {
            self.note("Context edit aborted.", view);
            self.track_feature_outcome("edit_context", "canceled", None);
            return;
        };
        if text == self.context_edit_baseline {
            self.note("Context unchanged.", view);
            self.track_feature_outcome("edit_context", "canceled", None);
            return;
        }
        let applied = match serde_json::from_str::<Vec<Value>>(&text) {
            Err(error) => Err(anyhow::anyhow!("The context is not a JSON array of messages: {error}")),
            Ok(messages) => {
                self.bounded_request(
                    Duration::from_millis(UI_REQUEST_TIMEOUT_MS),
                    DaemonCommand::SetSessionContext {
                        id: None,
                        active_session_id: self.active_session_id.clone(),
                        messages,
                        rest: Map::default(),
                    },
                )
                .await
            }
        };
        match applied {
            Ok(_) => {
                self.note("Context updated; the next turn uses it.", view);
                self.track_feature_outcome("edit_context", "completed", None);
            }
            Err(error) => {
                self.error_row(
                    &format!(
                        "{error:#}. Run /edit_context to fix the draft, or save an empty file to abort."
                    ),
                    view,
                );
                self.context_edit_draft = Some(text);
                self.track_feature_outcome("edit_context", "failed", None);
            }
        }
    }
}
