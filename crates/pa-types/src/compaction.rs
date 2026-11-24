//! Shared defaults and validation for context settings.

/// Remind on every request when unreleased tool results occupy this much of the model window.
pub const DEFAULT_TOOL_OUTPUT_REMINDER_PERCENT: u8 = 20;

/// Whether a configured automatic compaction percentage is a finite value in 1–100.
#[must_use]
pub fn is_valid_threshold_percent(percent: f64) -> bool {
    percent.is_finite() && (1.0..=100.0).contains(&percent)
}
