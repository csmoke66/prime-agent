//! Compact-session tests.
use super::*;
use bh_types::ai::{AssistantMessage, UserContent};

fn session_with_turns(cwd: &std::path::Path, turns: usize) -> SessionManager {
    let mut session = SessionManager::in_memory(cwd);
    for i in 0..turns {
        session
            .append_message(AgentMessage::User(bh_types::ai::UserMessage {
                content: UserContent::Text(format!("turn {i} message with some words")),
                timestamp: 0,
                rest: serde_json::Map::default(),
            }))
            .unwrap();
        session
            .append_message(AgentMessage::Assistant(AssistantMessage {
                content: vec![bh_types::ai::AssistantContentBlock::Text(
                    bh_types::ai::TextContent {
                        text: format!("reply {i}"),
                        text_signature: None,
                        rest: serde_json::Map::default(),
                    },
                )],
                api: "openai-completions".to_string(),
                provider: "test".to_string(),
                model: "m".to_string(),
                response_model: None,
                response_id: None,
                diagnostics: None,
                usage: bh_types::ai::Usage {
                    input: 100,
                    output: 20,
                    cache_read: 0,
                    cache_write: 0,
                    total_tokens: 120,
                    cost: bh_types::ai::UsageCost::default(),
                },
                stop_reason: bh_types::ai::StopReason::Stop,
                stop_reason_raw: None,
                error_message: None,
                timestamp: 0,
                rest: serde_json::Map::default(),
            }))
            .unwrap();
    }
    session
}

/// The faux provider with one scripted summarizer response. The faux
/// seam is process-global, so every registration unregisters on drop.
fn faux_registration() -> bh_ai::faux::FauxProviderRegistration {
    let registration =
        bh_ai::faux::register_faux_provider(bh_ai::faux::RegisterFauxProviderOptions {
            models: Some(vec![bh_ai::faux::FauxModelDefinition {
                id: "compact-m".to_string(),
                name: Some("Compact Model".to_string()),
                reasoning: Some(false),
                input: Some(vec![bh_types::ai::ModelInput::Text]),
                cost: None,
                context_window: Some(1_000),
                max_tokens: Some(256),
            }]),
            ..Default::default()
        });
    registration.set_responses(vec![bh_ai::faux::FauxResponseStep::Message(
        bh_ai::faux::faux_assistant_text_message(
            "## Goal\nsummarized goal",
            bh_ai::faux::FauxAssistantMessageOptions::default(),
        ),
    )]);
    registration
}

mod auxiliary_model;
mod cut_math;
mod execute_compaction;
mod prepare_compaction;
mod second_compaction;
mod skip_guards;
mod split_turn;

async fn execute_compaction(
    session: &mut SessionManager,
    options: CompactOptions<'_>,
) -> anyhow::Result<CompactOutcome> {
    let started_at = std::time::Instant::now();
    let mut attempt = match prepare_attempt(session, &options) {
        Ok(attempt) => attempt,
        Err(skip) => return Ok(CompactOutcome::Skipped(skip.user_message())),
    };
    let prepared = summarize_attempt(&attempt, &options, &SummaryBill::default()).await?;
    assert!(
        commit_attempt(session, &mut attempt, &prepared, options.abort)?,
        "no concurrent writer in a unit test"
    );
    Ok(CompactOutcome::Ran(Box::new(CompactRun {
        result: prepared.result,
        entry: prepared.entry,
        duration_ms: started_at.elapsed().as_millis() as u64,
        ipython_state: None,
    })))
}
