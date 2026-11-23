//! The live master switch and model-boundary filtering. Durable history is untouched.

use pa_types::session::AgentMessage;

/// The single live policy read for scheduling, planning, apply, and context delivery.
pub(crate) fn enabled(settings: Option<&crate::settings::SettingsManager>) -> bool {
    settings.is_none_or(|settings| settings.reopen().get_harness_refinements())
}

impl super::AgentSession {
    pub(crate) fn harness_refinements_enabled(&self) -> bool {
        enabled(self.refinement_settings.as_ref())
    }
}

/// Remove only typed harness rows; never interpret a user's quoted text as metadata.
pub(crate) fn without_refinements(messages: Vec<AgentMessage>) -> Vec<AgentMessage> {
    messages
        .into_iter()
        .filter_map(|message| match message {
            AgentMessage::Custom(custom)
                if matches!(
                    custom.custom_type.as_str(),
                    "harness_digest" | "refinement_notice" | "refinement_outcome"
                ) =>
            {
                None
            }
            AgentMessage::CompactionSummary(mut summary) => {
                summary.harness_digest = None;
                summary.harness_state_fingerprint = None;
                Some(AgentMessage::CompactionSummary(summary))
            }
            message @ (AgentMessage::User(_)
            | AgentMessage::Assistant(_)
            | AgentMessage::ToolResult(_)
            | AgentMessage::BashExecution(_)
            | AgentMessage::Custom(_)
            | AgentMessage::BranchSummary(_)) => Some(message),
        })
        .collect()
}

/// Resolve at every provider request, including tool continuations and retries.
/// I/O runs off the async worker. Filtering precedes conversion while provenance is typed.
pub(crate) fn convert_to_llm(
    settings: crate::settings::SettingsManager,
    reminder_percent: std::sync::Arc<std::sync::atomic::AtomicU8>,
    compaction_notice: std::sync::Arc<std::sync::Mutex<super::compaction::CompactionNotice>>,
) -> pa_agent::agent_loop::ConvertToLlmFn {
    let settings = std::sync::Arc::new(settings);
    std::sync::Arc::new(move |messages| {
        let settings = std::sync::Arc::clone(&settings);
        let reminder_percent = std::sync::Arc::clone(&reminder_percent);
        let compaction_notice = std::sync::Arc::clone(&compaction_notice);
        Box::pin(async move {
            // Both policies use the same live settings snapshot. Landing a
            // parallel batch of results must not re-read files on the async worker.
            let enabled = tokio::task::spawn_blocking(move || {
                let live = settings.reopen();
                reminder_percent.store(
                    live.get_tool_output_reminder_percent(),
                    std::sync::atomic::Ordering::Relaxed,
                );
                live.get_harness_refinements()
            }).await?;
            let messages = if enabled {
                messages
            } else {
                messages
                    .into_iter()
                    .filter_map(|message| match message {
                        pa_agent::types::AgentMessage::Custom(custom) => {
                            let typed =
                                serde_json::from_value(serde_json::to_value(custom).ok()?).ok()?;
                            without_refinements(vec![typed])
                                .into_iter()
                                .next()
                                .and_then(|row| super::session_message_to_loop(&row))
                        }
                        message @ pa_agent::types::AgentMessage::Standard(_) => Some(message),
                    })
                    .collect()
            };
            let mut projection = super::messages::loop_convert_to_llm(messages);
            if compaction_notice
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take()
            {
                projection.messages.push(pa_agent::types::Message::User(pa_agent::types::UserMessage {
                    content: pa_agent::types::UserContent::Text(
                        "[pre-compaction] Automatic compaction is approaching. Save any findings you still need, then use rlm.release() in your next useful REPL cell to release spent tool results. Use rlm.release([tool_call_id, ...]) for specific older results. Keep raw output you still need.".into()
                    ),
                    timestamp: super::now_millis() as i64,
                }));
            }
            Ok(projection)
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::SettingsManager;
    use pa_types::ai::{UserContent, UserMessage};

    fn user(text: &str) -> AgentMessage {
        AgentMessage::User(UserMessage {
            content: UserContent::Text(text.to_string()),
            timestamp: 0,
            rest: serde_json::Map::default(),
        })
    }

    #[tokio::test]
    async fn pre_compaction_notice_reaches_the_next_request_once_at_the_effective_margin() {
        use crate::session_engine::compaction::{compaction_threshold, CompactionSettings};

        let dir = tempfile::tempdir().unwrap();
        let settings = SettingsManager::create(dir.path(), dir.path().join("agent"));
        let notice = std::sync::Arc::new(std::sync::Mutex::new(
            super::super::compaction::CompactionNotice::default(),
        ));
        let convert = convert_to_llm(settings, std::sync::Arc::default(), notice.clone());
        let window = 128_000;
        let threshold = compaction_threshold(window, 32_000, &CompactionSettings::default());
        let margin = threshold - window / 10;
        let request = vec![super::super::session_message_to_loop(&user("continue")).unwrap()];
        let check = |tokens| notice.lock().unwrap().check(Some(tokens), window, threshold);
        let has_notice = |messages: pa_agent::agent_loop::ProviderMessages| {
            serde_json::to_string(&messages.messages)
                .unwrap()
                .contains("[pre-compaction]")
        };

        assert!(!check(margin - 1));
        assert!(!has_notice(convert(request.clone()).await.unwrap()));
        assert!(!check(margin));
        let warned = convert(request.clone()).await.unwrap();
        assert!(serde_json::to_string(&warned.messages).unwrap().contains("rlm.release()"));
        assert!(!check(threshold));
        assert!(!has_notice(convert(request.clone()).await.unwrap()));
        assert!(!check(margin - 1));
        assert!(!check(margin));
        assert!(has_notice(convert(request.clone()).await.unwrap()));
        assert!(!check(margin - 1));
        assert!(check(threshold + 1));
        assert!(!has_notice(convert(request).await.unwrap()));
    }

    #[tokio::test]
    async fn live_request_gate_filters_saved_rows_and_preserves_quoted_user_content() {
        let tmp = tempfile::tempdir().unwrap();
        let mut settings = SettingsManager::create(tmp.path(), tmp.path().join("agent"));
        let convert = convert_to_llm(
            settings.reopen(),
            std::sync::Arc::default(),
            std::sync::Arc::default(),
        );
        let digest =
            super::super::harness_digest::harness_digest_prompt_row("saved memory", 1, "fp");
        let quote =
            super::super::session_message_to_loop(&user("[harness-digest] quoted user content"))
                .unwrap();
        let messages = vec![digest.clone(), quote.clone()];
        assert_eq!(convert(messages.clone()).await.unwrap().messages.len(), 2);
        settings.set_harness_refinements(false).unwrap();
        let expected = super::super::messages::loop_convert_to_llm(vec![quote]);
        assert_eq!(convert(messages.clone()).await.unwrap(), expected);
        settings.set_harness_refinements(true).unwrap();
        assert_eq!(convert(messages).await.unwrap().messages.len(), 2);
    }

    #[tokio::test]
    async fn cold_and_resumed_disabled_sessions_omit_saved_harness_context() {
        let tmp = tempfile::tempdir().unwrap();
        let agent_dir = tmp.path().join("agent");
        let mut settings = SettingsManager::create(tmp.path(), &agent_dir);
        settings.set_harness_refinements(false).unwrap();
        let model: pa_agent::types::Model = serde_json::from_value(serde_json::json!({
            "id":"m", "name":"m", "api":"test", "provider":"test", "baseUrl":"http://localhost",
            "reasoning":false, "cost":{"input":0,"output":0,"cacheRead":0,"cacheWrite":0},
            "contextWindow":10000, "maxTokens":100
        }))
        .unwrap();
        for resumed in [false, true] {
            let mut manager = crate::session::manager::SessionManager::in_memory(tmp.path());
            manager.materialize_session_file(Some(tmp.path().join(if resumed {
                "resumed"
            } else {
                "cold"
            })));
            if resumed {
                manager
                    .append_custom_message(
                        "harness_digest",
                        UserContent::Text("saved memory".to_string()),
                        false,
                        None,
                    )
                    .unwrap();
                manager.append_message(user("Continue my work")).unwrap();
            }
            let provider =
                std::sync::Arc::new(pa_agent::scripted::ScriptedProvider::new(model.clone()));
            let engine =
                super::super::engine::create_session(super::super::engine::SessionEngineConfig {
                    cwd: tmp.path().to_path_buf(),
                    agent_dir: agent_dir.clone(),
                    model: Some(model.clone()),
                    stream_fn: Some(provider.stream_fn()),
                    session_manager: Some(manager),
                    daemonless: true,
                    ..Default::default()
                })
                .await
                .unwrap();
            assert!(!engine.session.auto_refine_allowed());
            assert!(engine.session.harness_digest_inputs().await.is_none());
            assert!(engine
                .session
                .pending_digest_prompt_row()
                .await
                .unwrap()
                .is_none());
            let live = engine.session.agent().state().await.messages;
            assert_eq!(
                super::super::messages::loop_convert_to_llm(live).messages.len(),
                usize::from(resumed)
            );
            // Re-enable without a reload (the busy-session case): the next admission loads state.
            settings.set_harness_refinements(true).unwrap();
            assert!(engine.session.auto_refine_allowed());
            assert!(engine
                .session
                .pending_digest_prompt_row()
                .await
                .unwrap()
                .is_some());
            settings.set_harness_refinements(false).unwrap();
            assert!(engine
                .session
                .pending_digest_prompt_row()
                .await
                .unwrap()
                .is_none());
            settings.set_harness_refinements(true).unwrap();
            assert!(
                engine
                    .session
                    .pending_digest_prompt_row()
                    .await
                    .unwrap()
                    .is_none(),
                "a consumed digest must not be re-armed by disabling and enabling"
            );
            settings.set_harness_refinements(false).unwrap();
            // The history remains inspectable even though it is absent from model context.
            if resumed {
                assert!(engine.session.entries().await.iter().any(|entry| matches!(entry,
                    pa_types::session::FileEntry::CustomMessage { payload, .. } if payload.custom_type == "harness_digest")));
            }
        }
    }

    #[test]
    fn disabled_context_removes_compaction_digest_but_keeps_the_summary() {
        let summary = pa_types::session::CompactionSummaryMessage {
            summary: "Earlier decisions still apply".to_string(),
            tokens_before: 10,
            retained_message_count: None,
            custom_instructions: None,
            harness_digest: Some("saved memory".to_string()),
            harness_state_fingerprint: Some("fp".to_string()),
            timestamp: 0,
        };
        let mut expected = summary.clone();
        expected.harness_digest = None;
        expected.harness_state_fingerprint = None;
        assert_eq!(
            without_refinements(vec![AgentMessage::CompactionSummary(summary)]),
            vec![AgentMessage::CompactionSummary(expected)]
        );
    }
}
