//! Session-local tool-output pressure advisory.
//!
//! The landing hook accounts only model-visible, unreleased tool-result
//! bodies. It appends one advisory to the result that enters a high-pressure
//! episode; the durable placement keeps the note at a stable prompt position.

use pa_agent::agent_loop::ToolResultLandingFn;
use pa_agent::types::{AgentMessage, Model, ToolResultContent, ToolResultMessage};

const NOTICE_PREFIX: &str = "[tool-output-pressure]";
/// Ten percent of the configured threshold is hysteresis: at the default
/// 30% trigger, releases must take pressure below 27% before another episode.
const REARM_THRESHOLD_TENTHS: u128 = 9;

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
    armed: bool,
    initialized: bool,
    compaction_timestamp: Option<u64>,
    model_provider: String,
    model_id: String,
    context_window: u64,
}

impl PressureState {
    fn land(
        &mut self,
        message: &mut ToolResultMessage,
        prefix: &[AgentMessage],
        batch_prefix: &[ToolResultMessage],
        model: &Model,
        threshold_percent: u8,
        force_rearm: bool,
    ) -> super::ToolOutputStats {
        let compaction_timestamp = latest_compaction_timestamp(prefix);
        let compaction_changed = self.initialized
            && compaction_timestamp.is_some()
            && compaction_timestamp != self.compaction_timestamp;
        let model_changed = self.initialized
            && (self.model_provider != model.provider
                || self.model_id != model.id
                || self.context_window != model.context_window);
        if !self.initialized || compaction_changed || !self.prefix_matches(prefix, batch_prefix) {
            self.rebuild(prefix, compaction_changed);
            for result in batch_prefix {
                self.track_existing(result);
            }
        }
        if force_rearm || model_changed {
            self.armed = true;
        }
        self.model_provider.clone_from(&model.provider);
        self.model_id.clone_from(&model.id);
        self.context_window = model.context_window;
        self.compaction_timestamp = compaction_timestamp;
        self.initialized = true;

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

        if below_rearm_pressure(
            self.live_tokens,
            model.context_window,
            threshold_percent,
        ) {
            self.armed = true;
        }
        if self.armed
            && at_or_above_pressure(
                self.live_tokens,
                model.context_window,
                threshold_percent,
            )
            && !released
        {
            let percent = approximate_percent(self.live_tokens, model.context_window);
            let note = format!(
                "{NOTICE_PREFIX} Tool results use about {percent}% of this model's window. Even small results cost tokens on later requests. Keep evidence you still need; once a result is spent, save the finding and call rlm.release() in your next useful cell. Don't wait for a bulk clear. Use rlm.release(\"tail\") for a spent exchange or rlm.release(\"current\") for transcript-only output."
            );
            let note_tokens = text_tokens(&note);
            message.content.push(ToolResultContent::text(note));
            self.live_tokens = self.live_tokens.saturating_add(note_tokens);
            if let Some(tracked) = self.results.last_mut() {
                tracked.tokens = tracked.tokens.saturating_add(note_tokens);
            }
            self.armed = false;
        }
        self.stats()
    }

    fn replace_history(&mut self, messages: &[AgentMessage]) {
        let compaction_timestamp = latest_compaction_timestamp(messages);
        let compaction_changed = self.initialized
            && compaction_timestamp.is_some()
            && compaction_timestamp != self.compaction_timestamp;
        self.rebuild(messages, compaction_changed);
        self.compaction_timestamp = compaction_timestamp;
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
                AgentMessage::Standard(pa_agent::types::Message::ToolResult(result)) => {
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

    fn rebuild(&mut self, prefix: &[AgentMessage], force_rearm: bool) {
        let release_mask = super::tool_output_release::loop_release_mask(prefix);
        let mut results = Vec::new();
        let mut live_tokens = 0u64;
        let mut has_live_notice = false;
        self.results_since_release = 0;
        self.last_release = None;
        for (index, message) in prefix.iter().enumerate() {
            let AgentMessage::Standard(pa_agent::types::Message::ToolResult(result)) = message else {
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
                has_live_notice |= contains_notice(result);
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
        self.armed = force_rearm || !has_live_notice;
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
        if !released && contains_notice(result) {
            self.armed = false;
        }
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
    pub(super) history_changed: pa_agent::agent::MessageHistoryChangedFn,
    pub(super) stats: std::sync::Arc<std::sync::RwLock<super::ToolOutputStats>>,
}

pub(super) fn accounting(
    percent: std::sync::Arc<std::sync::atomic::AtomicU8>,
) -> ToolOutputAccounting {
    let state = std::sync::Arc::new(std::sync::Mutex::new(PressureState::default()));
    let stats = std::sync::Arc::new(std::sync::RwLock::new(super::ToolOutputStats::default()));
    let history_state = std::sync::Arc::clone(&state);
    let history_stats = std::sync::Arc::clone(&stats);
    let history_changed: pa_agent::agent::MessageHistoryChangedFn =
        std::sync::Arc::new(move |messages: &[AgentMessage]| {
            let mut state = history_state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            state.replace_history(messages);
            *history_stats
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = state.stats();
        });
    let landing_state = state;
    let landing_stats = std::sync::Arc::clone(&stats);
    let last_percent = std::sync::atomic::AtomicU8::new(u8::MAX);
    let landing: ToolResultLandingFn =
        std::sync::Arc::new(move |message, prefix, batch_prefix, model| {
            super::tool_output_release::resolve_loop_tail_release(message, prefix);
            let threshold_percent = percent.load(std::sync::atomic::Ordering::Relaxed);
            let previous_percent = last_percent.swap(
                threshold_percent,
                std::sync::atomic::Ordering::Relaxed,
            );
            let mut state = landing_state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let snapshot = state.land(
                message,
                prefix,
                batch_prefix,
                model,
                threshold_percent,
                previous_percent == 0,
            );
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

fn latest_compaction_timestamp(messages: &[AgentMessage]) -> Option<u64> {
    messages.iter().rev().find_map(|message| match message {
        AgentMessage::Custom(custom) if custom.role == "compactionSummary" => {
            custom.payload.get("timestamp").and_then(serde_json::Value::as_u64)
        }
        _ => None,
    })
}

fn result_tokens(result: &ToolResultMessage) -> u64 {
    let chars: u64 = result
        .content
        .iter()
        .map(|content| match content {
            ToolResultContent::Text(text) => text.text.chars().count() as u64,
            ToolResultContent::Image(_) => 4_800,
            ToolResultContent::Video(_) => pa_types::ai::VIDEO_ESTIMATED_TOKENS.saturating_mul(4),
        })
        .sum();
    chars.div_ceil(4)
}

fn result_timestamp(result: &ToolResultMessage) -> Option<u64> {
    u64::try_from(result.timestamp).ok()
}

fn text_tokens(text: &str) -> u64 {
    (text.chars().count() as u64).div_ceil(4)
}

fn contains_notice(result: &ToolResultMessage) -> bool {
    result.content.iter().any(|content| {
        matches!(content, ToolResultContent::Text(text) if text.text.starts_with(NOTICE_PREFIX))
    })
}

fn at_or_above_pressure(tokens: u64, context_window: u64, threshold_percent: u8) -> bool {
    threshold_percent > 0
        && u128::from(tokens).saturating_mul(100)
            >= u128::from(context_window).saturating_mul(u128::from(threshold_percent))
}

fn below_rearm_pressure(tokens: u64, context_window: u64, threshold_percent: u8) -> bool {
    context_window > 0
        && threshold_percent > 0
        && u128::from(tokens).saturating_mul(1_000)
            < u128::from(context_window)
                .saturating_mul(u128::from(threshold_percent))
                .saturating_mul(REARM_THRESHOLD_TENTHS)
}

fn approximate_percent(tokens: u64, context_window: u64) -> u128 {
    let window = u128::from(context_window);
    u128::from(tokens)
        .saturating_mul(100)
        .saturating_add(window / 2)
        / window
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model(context_window: u64) -> Model {
        Model {
            id: "test".to_string(),
            name: "Test".to_string(),
            api: "test".to_string(),
            provider: "test".to_string(),
            base_url: String::new(),
            reasoning: false,
            cost: pa_agent::types::UsageCost::default(),
            context_window,
            max_tokens: 0,
            max_tokens_explicit: false,
        }
    }

    fn result(id: &str, chars: usize) -> AgentMessage {
        AgentMessage::Standard(pa_agent::types::Message::ToolResult(ToolResultMessage {
            tool_call_id: id.to_string(),
            tool_name: "bash".to_string(),
            content: vec![ToolResultContent::text("x".repeat(chars))],
            details: None,
            is_error: false,
            timestamp: 0,
        }))
    }

    fn release_string(id: &str, directive: &str, chars: usize) -> AgentMessage {
        AgentMessage::Standard(pa_agent::types::Message::ToolResult(ToolResultMessage {
            tool_call_id: id.to_string(),
            tool_name: pa_types::ai::PYTHON_TOOL_NAME.to_string(),
            content: vec![ToolResultContent::text("x".repeat(chars))],
            details: Some(serde_json::json!({ "releaseToolOutputs": directive })),
            is_error: false,
            timestamp: 0,
        }))
    }

    fn release_explicit(id: &str, targets: &[&str]) -> AgentMessage {
        AgentMessage::Standard(pa_agent::types::Message::ToolResult(ToolResultMessage {
            tool_call_id: id.to_string(),
            tool_name: pa_types::ai::PYTHON_TOOL_NAME.to_string(),
            content: vec![ToolResultContent::text(String::new())],
            details: Some(serde_json::json!({ "releaseToolOutputs": targets })),
            is_error: false,
            timestamp: 0,
        }))
    }

    fn assistant(timestamp: i64, id: &str) -> AgentMessage {
        serde_json::from_value::<pa_agent::types::AssistantMessage>(serde_json::json!({
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

    fn has_notice(message: &AgentMessage) -> bool {
        matches!(message,
            AgentMessage::Standard(pa_agent::types::Message::ToolResult(result))
                if contains_notice(result))
    }

    fn land(
        hook: &ToolResultLandingFn,
        prefix: &mut Vec<AgentMessage>,
        mut message: AgentMessage,
    ) -> bool {
        let AgentMessage::Standard(pa_agent::types::Message::ToolResult(result)) = &mut message
        else {
            panic!("test result");
        };
        hook(result, prefix, &[], &model(100));
        let reminded = has_notice(&message);
        prefix.push(message);
        reminded
    }

    fn stats(accounting: &ToolOutputAccounting) -> super::super::ToolOutputStats {
        *accounting
            .stats
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    #[test]
    fn reminds_once_and_rearms_after_release_compaction_or_reenable() {
        let threshold = std::sync::Arc::new(std::sync::atomic::AtomicU8::new(30));
        let accounting = accounting(std::sync::Arc::clone(&threshold));
        let hook = std::sync::Arc::clone(&accounting.landing);
        let mut prefix = Vec::new();

        assert!(land(&hook, &mut prefix, result("first", 120)));
        assert!(!land(&hook, &mut prefix, result("still-high", 40)));

        assert!(!land(
            &hook,
            &mut prefix,
            release_string("release", "all", 0),
        ));
        assert!(!land(
            &hook,
            &mut prefix,
            release_string("hidden-current", "current", 1_000),
        ));
        assert!(land(&hook, &mut prefix, result("new-episode", 120)));
        assert!(!land(&hook, &mut prefix, result("same-episode", 4)));

        prefix.push(AgentMessage::Custom(pa_agent::types::CustomAgentMessage {
            role: "compactionSummary".to_string(),
            payload: serde_json::json!({ "timestamp": 1 }),
        }));
        assert!(!land(
            &hook,
            &mut prefix,
            release_string("post-compaction-hidden", "current", 1_000),
        ));
        assert!(land(
            &hook,
            &mut prefix,
            result("post-compaction-visible", 4),
        ));

        threshold.store(0, std::sync::atomic::Ordering::Relaxed);
        assert!(!land(&hook, &mut prefix, result("disabled", 120)));
        threshold.store(30, std::sync::atomic::Ordering::Relaxed);
        assert!(land(&hook, &mut prefix, result("re-enabled", 4)));
    }

    #[test]
    fn meter_tracks_disabled_reminders_and_release_receipts() {
        let accounting = accounting(std::sync::Arc::new(
            std::sync::atomic::AtomicU8::new(0),
        ));
        let hook = std::sync::Arc::clone(&accounting.landing);
        let mut prefix = Vec::new();

        assert!(!land(&hook, &mut prefix, result("first", 40)));
        assert_eq!(
            stats(&accounting),
            super::super::ToolOutputStats {
                tokens: 10,
                results_since_release: 1,
                last_release: None,
            }
        );
        assert!(!land(
            &hook,
            &mut prefix,
            release_string("hidden-current", "current", 400),
        ));
        assert_eq!(
            stats(&accounting),
            super::super::ToolOutputStats {
                tokens: 10,
                results_since_release: 0,
                last_release: Some("current"),
            }
        );
        assert!(!land(&hook, &mut prefix, result("second", 80)));
        assert_eq!(
            stats(&accounting),
            super::super::ToolOutputStats {
                tokens: 30,
                results_since_release: 1,
                last_release: Some("current"),
            }
        );
        assert!(!land(
            &hook,
            &mut prefix,
            release_explicit("explicit", &["first"]),
        ));
        assert_eq!(
            stats(&accounting),
            super::super::ToolOutputStats {
                tokens: 20,
                results_since_release: 0,
                last_release: Some("explicit"),
            }
        );
        assert!(!land(
            &hook,
            &mut prefix,
            release_string("last", "last", 0),
        ));
        assert_eq!(stats(&accounting).last_release, Some("last"));
        assert!(!land(
            &hook,
            &mut prefix,
            release_string("all", "all", 0),
        ));
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
        let accounting = accounting(std::sync::Arc::new(
            std::sync::atomic::AtomicU8::new(0),
        ));
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
            AgentMessage::Standard(pa_agent::types::Message::ToolResult(prior)),
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
        let release = AgentMessage::Standard(pa_agent::types::Message::ToolResult(
            ToolResultMessage {
                tool_call_id: "release".to_string(),
                tool_name: pa_types::ai::PYTHON_TOOL_NAME.to_string(),
                content: vec![ToolResultContent::text(String::new())],
                details: Some(
                    serde_json::json!({ "releaseToolOutputs": { "tail": 1 } }),
                ),
                is_error: false,
                timestamp: 4,
            },
        ));

        assert!(!land(&hook, &mut prefix, release));
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
        let accounting = accounting(std::sync::Arc::new(
            std::sync::atomic::AtomicU8::new(0),
        ));
        let stats_handle = std::sync::Arc::clone(&accounting.stats);
        let agent = pa_agent::agent::Agent::new(pa_agent::agent::AgentOptions {
            initial_state: pa_agent::agent::AgentInitialState {
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
                AgentMessage::Custom(pa_agent::types::CustomAgentMessage {
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
