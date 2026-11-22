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
    ) {
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
        if !self.armed
            || !at_or_above_pressure(
                self.live_tokens,
                model.context_window,
                threshold_percent,
            )
        {
            return;
        }
        if released {
            // `current` hides this body on the imminent request. Keep the
            // episode armed so the next visible result can carry the note.
            return;
        }

        let percent = approximate_percent(self.live_tokens, model.context_window);
        let note = format!(
            "{NOTICE_PREFIX} Tool results use about {percent}% of this model's window. If finished with the latest exchange, keep a checkpoint and use rlm.release(\"tail\"). For new transcript-only output use rlm.release(\"current\"). Batch older releases at milestones to preserve cache hits."
        );
        let note_tokens = text_tokens(&note);
        message.content.push(ToolResultContent::text(note));
        self.live_tokens = self.live_tokens.saturating_add(note_tokens);
        if let Some(tracked) = self.results.last_mut() {
            tracked.tokens = tracked.tokens.saturating_add(note_tokens);
        }
        self.armed = false;
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
        for (index, message) in prefix.iter().enumerate() {
            let AgentMessage::Standard(pa_agent::types::Message::ToolResult(result)) = message else {
                continue;
            };
            let released = release_mask.as_ref().is_some_and(|mask| {
                mask.get(index).is_some_and(|effect| {
                    effect != &super::tool_output_release::ReleaseEffect::Keep
                })
            });
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

pub(super) fn landing(percent: std::sync::Arc<std::sync::atomic::AtomicU8>) -> ToolResultLandingFn {
    let state = std::sync::Mutex::new(PressureState::default());
    let last_percent = std::sync::atomic::AtomicU8::new(u8::MAX);
    std::sync::Arc::new(move |message, prefix, batch_prefix, model| {
        super::tool_output_release::resolve_loop_tail_release(message, prefix);
        let threshold_percent = percent.load(std::sync::atomic::Ordering::Relaxed);
        let previous_percent = last_percent.swap(
            threshold_percent,
            std::sync::atomic::Ordering::Relaxed,
        );
        if threshold_percent == 0 || model.context_window == 0 {
            return;
        }
        state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .land(
                message,
                prefix,
                batch_prefix,
                model,
                threshold_percent,
                previous_percent == 0,
            );
    })
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

    fn release_all(id: &str) -> AgentMessage {
        AgentMessage::Standard(pa_agent::types::Message::ToolResult(ToolResultMessage {
            tool_call_id: id.to_string(),
            tool_name: pa_types::ai::PYTHON_TOOL_NAME.to_string(),
            content: vec![ToolResultContent::text(String::new())],
            details: Some(serde_json::json!({ "releaseToolOutputs": "all" })),
            is_error: false,
            timestamp: 0,
        }))
    }

    fn release_current(id: &str, chars: usize) -> AgentMessage {
        AgentMessage::Standard(pa_agent::types::Message::ToolResult(ToolResultMessage {
            tool_call_id: id.to_string(),
            tool_name: pa_types::ai::PYTHON_TOOL_NAME.to_string(),
            content: vec![ToolResultContent::text("x".repeat(chars))],
            details: Some(serde_json::json!({ "releaseToolOutputs": "current" })),
            is_error: false,
            timestamp: 0,
        }))
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

    #[test]
    fn reminds_once_and_rearms_after_release_compaction_or_reenable() {
        let threshold = std::sync::Arc::new(std::sync::atomic::AtomicU8::new(30));
        let hook = landing(std::sync::Arc::clone(&threshold));
        let mut prefix = Vec::new();

        assert!(land(&hook, &mut prefix, result("first", 120)));
        assert!(!land(&hook, &mut prefix, result("still-high", 40)));

        assert!(!land(&hook, &mut prefix, release_all("release")));
        assert!(!land(
            &hook,
            &mut prefix,
            release_current("hidden-current", 1_000),
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
            release_current("post-compaction-hidden", 1_000),
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
}
