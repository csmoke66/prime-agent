//! The goal mint's progress gate: the just-settled turn's examination,
//! provider retry windows, durable no-progress streak with bounded
//! exponential backoff, and order-safety rules.

use super::{now_millis, GoalDriver};
use crate::goals::GoalState;
use crate::session::manager::SessionManager;
use crate::session_engine::provider_retry::provider_stream_failure_kind;

/// Initial delay after a no-output turn.
const CONTINUATION_NO_PROGRESS_BACKOFF_BASE_MS: u64 = 10_000;

/// Maximum delay between retries after consecutive no-output turns.
const CONTINUATION_NO_PROGRESS_BACKOFF_MAX_MS: u64 = 5 * 60 * 1_000;

/// The durable one-shot wake's cron label: the daemon's boundary sites arm a
/// one-shot cron at [`GoalDriver::backoff_wake_at`], so the backoff retry actually runs.
pub const GOAL_BACKOFF_WAKE_CRON_LABEL: &str = "goal-backoff-wake";

/// The wake marker prompt: the scheduler fires it into the follow-up lane;
/// its turn re-probes the provider and re-consults the mint.
pub const GOAL_BACKOFF_WAKE_MARKER_TEXT: &str = "<goal_backoff_wake>\nThe goal continuation backoff window has elapsed; this wake is automatic. Continue the goal work from where it stopped.\n</goal_backoff_wake>";

/// Whether the turn produced any output: an EMPTY content block list OR one
/// whose every block is empty (the abort corpse's single empty text part).
#[must_use]
pub fn turn_produced_no_output(message: &bh_agent::types::AssistantMessage) -> bool {
    message.content.iter().all(|part| match part {
        bh_agent::types::AssistantContent::Text(text) => text.text.is_empty(),
        bh_agent::types::AssistantContent::Thinking(thinking) => thinking.thinking.is_empty(),
        bh_agent::types::AssistantContent::ToolCall(_) => false,
    })
}

/// The just-settled turn's provider-failure text on a terminal failure (stop
/// reason `error`, a stream failure outside the quota-park class).
#[must_use]
pub fn terminal_provider_failure(message: &bh_agent::types::AssistantMessage) -> Option<String> {
    if message.stop_reason != bh_agent::types::StopReason::Error {
        return None;
    }
    // The diagnostic is consulted ONLY to exclude the quota-park
    // class.
    if provider_stream_failure_kind(message).as_deref() == Some("rate_limit") {
        return None;
    }
    Some(
        message
            .error_message
            .clone()
            .filter(|error| !error.is_empty())
            .unwrap_or_else(|| "Assistant response failed".to_string()),
    )
}

impl GoalDriver {
    /// The mint's progress gate: examine the just-settled turn and report
    /// whether the mint may proceed. Every refusal arm persists its own
    /// state change before returning `false`.
    ///
    /// # Errors
    ///
    /// Returns the error when a state transition fails to persist.
    pub(super) fn progress_gate(
        &mut self,
        session: &mut SessionManager,
        turn: &bh_agent::types::AssistantMessage,
    ) -> anyhow::Result<bool> {
        // Only a turn THIS goal's lifetime produced can judge it: a leftover
        // corpse from BEFORE the goal began must not finish the fresh goal.
        let turn_is_this_goals = self
            .state
            .created_at
            .is_none_or(|created_at| turn.timestamp > created_at as i64);
        // The examined-turn gate: a row the driver has already judged never
        // re-enters the progress machinery — without it the failed pair's
        // removal would expose the earlier progress row and reset the streak.
        let turn_is_new = self
            .counted_no_progress_turn_ms
            .is_none_or(|examined| turn.timestamp > examined);
        // A failed turn must be recorded even when its timestamp collides
        // with an already-examined turn.
        if turn_is_this_goals {
            if let Some(error) = terminal_provider_failure(turn) {
                self.record_failed_turn(
                    session,
                    Some(&error),
                    provider_stream_failure_kind(turn).as_deref(),
                )?;
                return Ok(false);
            }
        }
        if turn_is_this_goals && turn_is_new {
            // A parked corpse never consumes the no-progress budget. The refusal
            // STICKS through the PARKED-REFUSAL window, NOT the backoff window — a
            // probe wake must never be scheduled into the parked session.
            if provider_stream_failure_kind(turn).as_deref() == Some("rate_limit") {
                self.counted_no_progress_turn_ms = Some(turn.timestamp);
                // An earlier strike's backoff window CLEARS here: the parked session
                // owns the retry cadence; the durable streak carries the strike.
                self.no_progress_backoff_until_ms = 0;
                self.parked_refusal_until_ms =
                    now_millis() + CONTINUATION_NO_PROGRESS_BACKOFF_BASE_MS;
                return Ok(false);
            }
            if turn_produced_no_output(turn) {
                // A no-output turn records the streak and delays the next
                // attempt. It never ends the goal, regardless of how many
                // consecutive attempts fail.
                self.counted_no_progress_turn_ms = Some(turn.timestamp);
                self.no_progress_streak = self.no_progress_streak.saturating_add(1);
                let multiplier =
                    2u64.saturating_pow(self.no_progress_streak.saturating_sub(1));
                let delay = CONTINUATION_NO_PROGRESS_BACKOFF_BASE_MS
                    .saturating_mul(multiplier)
                    .min(CONTINUATION_NO_PROGRESS_BACKOFF_MAX_MS);
                self.no_progress_backoff_until_ms = now_millis().saturating_add(delay);
                // The streak AND the examined-turn key persist with the goal
                // state: the same corpse never strikes twice.
                self.set_state(
                    session,
                    GoalState {
                        no_progress_streak: Some(self.no_progress_streak),
                        no_progress_turn_ms: Some(turn.timestamp),
                        ..self.state.clone()
                    },
                )?;
                return Ok(false);
            }
            // The turn produced output: the streak resets and any armed
            // backoff window clears — and the reset persists.
            if self.no_progress_streak != 0 {
                self.no_progress_streak = 0;
                self.no_progress_backoff_until_ms = 0;
                self.parked_refusal_until_ms = 0;
                self.counted_no_progress_turn_ms = Some(turn.timestamp);
                self.set_state(
                    session,
                    GoalState {
                        no_progress_streak: Some(0),
                        no_progress_turn_ms: Some(turn.timestamp),
                        ..self.state.clone()
                    },
                )?;
            }
        }
        Ok(true)
    }
}
