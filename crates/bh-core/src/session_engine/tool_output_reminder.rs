//! Account for unreleased tool results and advise at the model-request
//! boundary, without adding reminders to durable history.

use std::sync::{atomic::AtomicU8, Arc, RwLock};

use bh_agent::agent_loop::ToolResultLandingFn;
use bh_agent::stream::StreamFn;
use bh_agent::types::{AgentMessage, ToolResultContent, ToolResultMessage};

const NOTICE: &str =
    "[tool-output-pressure] Tool results are taking up a significant part of the context window. Use rlm.release() for results you no longer need.";

pub(super) fn is_legacy_notice(text: &str) -> bool {
    text.starts_with("[tool-output-pressure] Tool results use about ")
        && text.ends_with("for transcript-only output.")
}

#[derive(Debug)]
struct TrackedResult {
    tool_call_id: String,
    timestamp: Option<u64>,
    tokens: u64,
    released: bool,
}

#[derive(Debug, Default)]
struct PressureState {
    results: Vec<TrackedResult>,
    live_tokens: u64,
    results_since_release: u64,
    last_release: Option<&'static str>,
}

impl PressureState {
    fn land(
        &mut self,
        message: &mut ToolResultMessage,
        prefix: &[AgentMessage],
        batch_prefix: &[ToolResultMessage],
    ) -> super::ToolOutputStats {
        if !self.prefix_matches(prefix, batch_prefix) {
            self.rebuild(prefix);
            for result in batch_prefix {
                self.track_existing(result);
            }
        }
        self.record_result(message);
        let released = self.apply_release(message);
        let tokens = if released { 0 } else { result_tokens(message) };
        self.live_tokens = self.live_tokens.saturating_add(tokens);
        self.results.push(TrackedResult {
            tool_call_id: message.tool_call_id.clone(),
            timestamp: result_timestamp(message),
            tokens,
            released,
        });

        self.stats()
    }

    fn stats(&self) -> super::ToolOutputStats {
        super::ToolOutputStats {
            tokens: self.live_tokens,
            results_since_release: self.results_since_release,
            last_release: self.last_release,
        }
    }

    fn record_result(&mut self, result: &ToolResultMessage) {
        if let Some(kind) = release_kind(result) {
            self.results_since_release = 0;
            self.last_release = Some(kind);
        } else {
            self.results_since_release = self.results_since_release.saturating_add(1);
        }
    }

    fn prefix_matches(
        &self,
        prefix: &[AgentMessage],
        batch_prefix: &[ToolResultMessage],
    ) -> bool {
        let mut expected = self.results.iter();
        for result in prefix
            .iter()
            .filter_map(|message| match message {
                AgentMessage::Standard(bh_agent::types::Message::ToolResult(result)) => {
                    Some(result)
                }
                _ => None,
            })
            .chain(batch_prefix)
        {
            let Some(tracked) = expected.next() else {
                return false;
            };
            if tracked.tool_call_id.as_str() != result.tool_call_id.as_str()
                || tracked.timestamp != result_timestamp(result)
            {
                return false;
            }
        }
        expected.next().is_none()
    }

    fn rebuild(&mut self, prefix: &[AgentMessage]) {
        let release_mask = super::tool_output_release::loop_release_mask(prefix);
        let mut results = Vec::new();
        let mut live_tokens = 0u64;
        self.results_since_release = 0;
        self.last_release = None;
        for (index, message) in prefix.iter().enumerate() {
            let AgentMessage::Standard(bh_agent::types::Message::ToolResult(result)) = message else {
                continue;
            };
            let effect = super::tool_output_release::effect(&release_mask, index);
            let released = effect != super::tool_output_release::ReleaseEffect::Keep;
            if effect != super::tool_output_release::ReleaseEffect::Omit {
                self.record_result(result);
            }
            let tokens = if released { 0 } else { result_tokens(result) };
            if !released {
                live_tokens = live_tokens.saturating_add(tokens);
            }
            results.push(TrackedResult {
                timestamp: result_timestamp(result),
                tool_call_id: result.tool_call_id.clone(),
                tokens,
                released,
            });
        }
        self.results = results;
        self.live_tokens = live_tokens;
    }

    fn track_existing(&mut self, result: &ToolResultMessage) {
        let released = self.apply_release(result);
        let tokens = if released { 0 } else { result_tokens(result) };
        self.record_result(result);
        self.live_tokens = self.live_tokens.saturating_add(tokens);
        self.results.push(TrackedResult {
            tool_call_id: result.tool_call_id.clone(),
            timestamp: result_timestamp(result),
            tokens,
            released,
        });
    }

    fn apply_release(&mut self, result: &ToolResultMessage) -> bool {
        match super::tool_output_release::loop_release_directive(result) {
            Some(super::tool_output_release::ReleaseToolOutputs::Last) => {
                if let Some(index) = self.results.len().checked_sub(1) {
                    self.release_index(index);
                }
            }
            Some(super::tool_output_release::ReleaseToolOutputs::All) => {
                for index in 0..self.results.len() {
                    self.release_index(index);
                }
            }
            Some(super::tool_output_release::ReleaseToolOutputs::Explicit(ids)) => {
                for index in 0..self.results.len() {
                    if ids.iter().any(|id| {
                        id.as_str() == Some(self.results[index].tool_call_id.as_str())
                    }) {
                        self.release_index(index);
                    }
                }
            }
            Some(super::tool_output_release::ReleaseToolOutputs::Exchanges(records)) => {
                for (tool_call_id, _assistant_timestamp, result_timestamp) in
                    super::tool_output_release::released_exchange_keys(records)
                {
                    for index in 0..self.results.len() {
                        if self.results[index].tool_call_id.as_str() == tool_call_id
                            && self.results[index].timestamp == Some(result_timestamp)
                        {
                            self.release_index(index);
                        }
                    }
                }
            }
            Some(super::tool_output_release::ReleaseToolOutputs::Current) => return true,
            None => {}
        }
        false
    }

    fn release_index(&mut self, index: usize) {
        let result = &mut self.results[index];
        if result.released {
            return;
        }
        result.released = true;
        self.live_tokens = self.live_tokens.saturating_sub(result.tokens);
        result.tokens = 0;
    }
}

fn release_kind(result: &ToolResultMessage) -> Option<&'static str> {
    match super::tool_output_release::loop_release_directive(result)? {
        super::tool_output_release::ReleaseToolOutputs::Last => Some("last"),
        super::tool_output_release::ReleaseToolOutputs::All => Some("all"),
        super::tool_output_release::ReleaseToolOutputs::Current => Some("current"),
        super::tool_output_release::ReleaseToolOutputs::Explicit(_) => Some("explicit"),
        super::tool_output_release::ReleaseToolOutputs::Exchanges(_) => Some("tail"),
    }
}


pub(super) struct ToolOutputAccounting {
    pub(super) landing: ToolResultLandingFn,
    pub(super) history_changed: bh_agent::agent::MessageHistoryChangedFn,
    pub(super) stats: std::sync::Arc<std::sync::RwLock<super::ToolOutputStats>>,
}

pub(super) fn accounting() -> ToolOutputAccounting {
    let state = std::sync::Arc::new(std::sync::Mutex::new(PressureState::default()));
    let stats = std::sync::Arc::new(std::sync::RwLock::new(super::ToolOutputStats::default()));
    let history_state = std::sync::Arc::clone(&state);
    let history_stats = std::sync::Arc::clone(&stats);
    let history_changed: bh_agent::agent::MessageHistoryChangedFn =
        std::sync::Arc::new(move |messages: &[AgentMessage]| {
            let mut state = history_state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            state.rebuild(messages);
            *history_stats
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = state.stats();
        });
    let landing_state = state;
    let landing_stats = std::sync::Arc::clone(&stats);
    let landing: ToolResultLandingFn =
        std::sync::Arc::new(move |message, prefix, batch_prefix, _model| {
            super::tool_output_release::resolve_loop_tail_release(message, prefix);
            let mut state = landing_state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let snapshot = state.land(message, prefix, batch_prefix);
            *landing_stats
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = snapshot;
        });
    ToolOutputAccounting {
        landing,
        history_changed,
        stats,
    }
}

pub(super) fn remind_on_request(
    stream: StreamFn,
    percent: Arc<AtomicU8>,
    stats: Arc<RwLock<super::ToolOutputStats>>,
) -> StreamFn {
    Arc::new(move |model, mut context, options| {
        let threshold = percent.load(std::sync::atomic::Ordering::Relaxed);
        let tokens = stats
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .tokens;
        if at_or_above_pressure(tokens, model.context_window, threshold) {
            match &mut context.tail_context {
                Some(note) => {
                    note.push_str("\n\n");
                    note.push_str(NOTICE);
                }
                None => context.tail_context = Some(NOTICE.into()),
            }
        }
        stream(model, context, options)
    })
}

fn result_tokens(result: &ToolResultMessage) -> u64 {
    let chars: u64 = result
        .content
        .iter()
        .map(|content| match content {
            ToolResultContent::Text(text) if is_legacy_notice(&text.text) => 0,
            ToolResultContent::Text(text) => text.text.chars().count() as u64,
            ToolResultContent::Image(_) => 4_800,
            ToolResultContent::Video(_) => bh_types::ai::VIDEO_ESTIMATED_TOKENS.saturating_mul(4),
        })
        .sum();
    chars.div_ceil(4)
}

fn result_timestamp(result: &ToolResultMessage) -> Option<u64> {
    u64::try_from(result.timestamp).ok()
}

fn at_or_above_pressure(tokens: u64, context_window: u64, threshold_percent: u8) -> bool {
    context_window != 0
        && threshold_percent != 0
        && u128::from(tokens) * 100 >= u128::from(context_window) * u128::from(threshold_percent)
}

#[cfg(test)]
mod tests {
    use super::*;

    use bh_agent::types::Model;

    fn model(context_window: u64) -> Model {
        Model {
            id: "test".to_string(),
            name: "Test".to_string(),
            api: "test".to_string(),
            provider: "test".to_string(),
            base_url: String::new(),
            reasoning: false,
            cost: bh_agent::types::UsageCost::default(),
            context_window,
            max_tokens: 0,
            max_tokens_explicit: false,
        }
    }

    fn result(id: &str, chars: usize) -> AgentMessage {
        AgentMessage::Standard(bh_agent::types::Message::ToolResult(ToolResultMessage {
            tool_call_id: id.to_string(),
            tool_name: "bash".to_string(),
            content: vec![ToolResultContent::text("x".repeat(chars))],
            details: None,
            is_error: false,
            timestamp: 0,
        }))
    }

    fn release_string(id: &str, directive: &str, chars: usize) -> AgentMessage {
        AgentMessage::Standard(bh_agent::types::Message::ToolResult(ToolResultMessage {
            tool_call_id: id.to_string(),
            tool_name: bh_types::ai::PYTHON_TOOL_NAME.to_string(),
            content: vec![ToolResultContent::text("x".repeat(chars))],
            details: Some(serde_json::json!({ "releaseToolOutputs": directive })),
            is_error: false,
            timestamp: 0,
        }))
    }

    fn release_explicit(id: &str, targets: &[&str]) -> AgentMessage {
        AgentMessage::Standard(bh_agent::types::Message::ToolResult(ToolResultMessage {
            tool_call_id: id.to_string(),
            tool_name: bh_types::ai::PYTHON_TOOL_NAME.to_string(),
            content: vec![ToolResultContent::text(String::new())],
            details: Some(serde_json::json!({ "releaseToolOutputs": targets })),
            is_error: false,
            timestamp: 0,
        }))
    }

    fn assistant(timestamp: i64, id: &str) -> AgentMessage {
        serde_json::from_value::<bh_agent::types::AssistantMessage>(serde_json::json!({
            "timestamp": timestamp,
            "content": [{
                "type": "toolCall",
                "id": id,
                "name": "python_repl",
                "arguments": { "code": "work()" }
            }],
            "usage": {},
            "stopReason": "toolUse"
        }))
        .unwrap()
        .into()
    }

    fn land(hook: &ToolResultLandingFn, prefix: &mut Vec<AgentMessage>, mut message: AgentMessage) {
        let AgentMessage::Standard(bh_agent::types::Message::ToolResult(result)) = &mut message
        else {
            panic!("test result");
        };
        hook(result, prefix, &[], &model(100));
        prefix.push(message);
    }

    fn stats(accounting: &ToolOutputAccounting) -> super::super::ToolOutputStats {
        *accounting
            .stats
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    #[tokio::test]
    async fn reminds_on_every_request_at_threshold_until_results_are_released() {
        use bh_agent::stream::{LlmContext, StreamRequestOptions};

        let threshold = Arc::new(AtomicU8::new(
            bh_types::compaction::DEFAULT_TOOL_OUTPUT_REMINDER_PERCENT,
        ));
        let accounting = accounting();
        let sent = Arc::new(std::sync::Mutex::new(Vec::new()));
        let stream: StreamFn = {
            let sent = Arc::clone(&sent);
            Arc::new(move |_, context, _| {
                sent.lock().unwrap().push(context.tail_context);
                Box::pin(async { anyhow::bail!("captured request") })
            })
        };
        let stream = remind_on_request(stream, threshold, Arc::clone(&accounting.stats));
        let mut prefix = vec![result("below", 76)]; // 19 tokens of 100
        (accounting.history_changed)(&prefix);
        let request = || {
            stream(
                model(100),
                LlmContext { tail_context: Some("scratchpad".into()), ..Default::default() },
                StreamRequestOptions::default(),
            )
        };
        let _ = request().await;
        land(&accounting.landing, &mut prefix, result("threshold", 4));
        let _ = request().await;
        let _ = request().await; // no new tool call and still at 20%
        land(&accounting.landing, &mut prefix, release_string("release", "all", 0));
        let _ = request().await;
        assert_eq!(
            *sent.lock().unwrap(),
            [
                Some("scratchpad".into()),
                Some(format!("scratchpad\n\n{NOTICE}")),
                Some(format!("scratchpad\n\n{NOTICE}")),
                Some("scratchpad".into()),
            ]
        );
    }

    #[test]
    fn meter_tracks_release_receipts() {
        let accounting = accounting();
        let hook = std::sync::Arc::clone(&accounting.landing);
        let mut prefix = Vec::new();

        land(&hook, &mut prefix, result("first", 40));
        assert_eq!(
            stats(&accounting),
            super::super::ToolOutputStats {
                tokens: 10,
                results_since_release: 1,
                last_release: None,
            }
        );
        land(&hook, &mut prefix, release_string("hidden-current", "current", 400));
        assert_eq!(
            stats(&accounting),
            super::super::ToolOutputStats {
                tokens: 10,
                results_since_release: 0,
                last_release: Some("current"),
            }
        );
        land(&hook, &mut prefix, result("second", 80));
        assert_eq!(
            stats(&accounting),
            super::super::ToolOutputStats {
                tokens: 30,
                results_since_release: 1,
                last_release: Some("current"),
            }
        );
        land(&hook, &mut prefix, release_explicit("explicit", &["first"]));
        assert_eq!(
            stats(&accounting),
            super::super::ToolOutputStats {
                tokens: 20,
                results_since_release: 0,
                last_release: Some("explicit"),
            }
        );
        land(&hook, &mut prefix, release_string("last", "last", 0));
        assert_eq!(stats(&accounting).last_release, Some("last"));
        land(&hook, &mut prefix, release_string("all", "all", 0));
        assert_eq!(
            stats(&accounting),
            super::super::ToolOutputStats {
                tokens: 0,
                results_since_release: 0,
                last_release: Some("all"),
            }
        );
    }

    #[test]
    fn meter_applies_tail_release_on_the_landing_result() {
        let accounting = accounting();
        let hook = std::sync::Arc::clone(&accounting.landing);
        let prior = ToolResultMessage {
            tool_call_id: "prior".to_string(),
            tool_name: "bash".to_string(),
            content: vec![ToolResultContent::text("x".repeat(40))],
            details: None,
            is_error: false,
            timestamp: 2,
        };
        let mut prefix = vec![
            assistant(1, "prior"),
            AgentMessage::Standard(bh_agent::types::Message::ToolResult(prior)),
            assistant(3, "release"),
        ];
        (accounting.history_changed)(&prefix);
        assert_eq!(
            stats(&accounting),
            super::super::ToolOutputStats {
                tokens: 10,
                results_since_release: 1,
                last_release: None,
            }
        );
        let release = AgentMessage::Standard(bh_agent::types::Message::ToolResult(
            ToolResultMessage {
                tool_call_id: "release".to_string(),
                tool_name: bh_types::ai::PYTHON_TOOL_NAME.to_string(),
                content: vec![ToolResultContent::text(String::new())],
                details: Some(
                    serde_json::json!({ "releaseToolOutputs": { "tail": 1 } }),
                ),
                is_error: false,
                timestamp: 4,
            },
        ));

        land(&hook, &mut prefix, release);
        assert_eq!(
            stats(&accounting),
            super::super::ToolOutputStats {
                tokens: 0,
                results_since_release: 0,
                last_release: Some("tail"),
            }
        );
    }

    #[tokio::test]
    async fn restored_branch_and_compacted_tail_replace_meter_before_another_tool() {
        let accounting = accounting();
        let stats_handle = std::sync::Arc::clone(&accounting.stats);
        let agent = bh_agent::agent::Agent::new(bh_agent::agent::AgentOptions {
            initial_state: bh_agent::agent::AgentInitialState {
                messages: Some(vec![
                    result("released-before-restore", 40),
                    release_string("restored-release", "all", 0),
                    result("restored", 400),
                ]),
                ..Default::default()
            },
            tool_result_landing: Some(accounting.landing),
            message_history_changed: Some(accounting.history_changed),
            ..Default::default()
        });
        assert_eq!(
            *stats_handle.read().unwrap(),
            super::super::ToolOutputStats {
                tokens: 100,
                results_since_release: 1,
                last_release: Some("all"),
            }
        );

        agent.set_messages(vec![result("branch", 80)]).await;
        assert_eq!(
            *stats_handle.read().unwrap(),
            super::super::ToolOutputStats {
                tokens: 20,
                results_since_release: 1,
                last_release: None,
            }
        );

        agent
            .set_messages(vec![
                AgentMessage::Custom(bh_agent::types::CustomAgentMessage {
                    role: "compactionSummary".to_string(),
                    payload: serde_json::json!({ "timestamp": 9 }),
                }),
                result("retained-tail", 40),
            ])
            .await;
        assert_eq!(
            *stats_handle.read().unwrap(),
            super::super::ToolOutputStats {
                tokens: 10,
                results_since_release: 1,
                last_release: None,
            }
        );
    }
}
