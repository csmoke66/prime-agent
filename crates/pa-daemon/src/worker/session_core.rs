//! The live session's state block.
use super::QueuedItem;

use std::collections::VecDeque;

use serde_json::Value;

use crate::session_store::SessionFile;
use crate::types::SessionActionSnapshot;

/// The live session: store, queue, sequencing; shared by the connection
/// tasks, the runner, and the compaction manager via the core mutex.
pub(crate) struct SessionCore {
    pub(crate) active_session_id: String,
    pub(crate) generation: String,
    pub(crate) last_event_sequence: u64,
    pub(crate) store: Option<SessionFile>,
    pub(crate) cwd: String,
    pub(crate) steering: VecDeque<QueuedItem>,
    pub(crate) follow_up: VecDeque<QueuedItem>,
    pub(crate) busy: bool,
    pub(crate) created: bool,
    pub(crate) attached_client_ids: Vec<String>,
    pub(crate) abort_requested: bool,
    /// Explicit cancellation of this picked delivery; graceful shutdown is excluded.
    pub(crate) turn_cancelled: bool,
    /// A successful terminal Done was observed for this picked delivery.
    pub(crate) turn_completed: bool,
    /// A flow that detaches from the interrupted turn's events (a manual
    /// `compact`, a branch navigation) swallowed the aborted row in TS, so the
    /// gate's aborted-row exception stays closed while it settles its turn.
    pub(crate) suppress_aborted_row: bool,
    pub(crate) shutdown_requested: bool,
    /// True while a compaction run is in flight (TS `isCompacting`).
    pub(crate) compacting: bool,
    /// The turn's tool calls in flight, keyed by tool-call id: the
    /// roster summary derives `isRunningTools` from its size.
    pub(crate) running_tool_calls: std::collections::HashSet<String>,
    /// The session's tool executions so far (the summary's `toolUseCount`):
    /// bumped per `tool_execution_start` under the same core lock the frames
    /// sequence under, and seeded from the session file when a worker opens
    /// it, so a revived session never resets the count.
    pub(crate) tool_use_count: u64,
    /// Admission ids belonging to the current in-flight turn. The queue
    /// handoff and owned cancellation both inspect this under the core lock.
    pub(crate) running_admission_ids: std::collections::HashSet<String>,
    /// TS `autoCompactionEnabled` (settings default: on).
    pub(crate) auto_compaction_enabled: bool,
    /// The last broadcast queue snapshot (TS `_lastSessionActionSnapshot`):
    /// `session_action_update` fires only when the projection changed.
    pub(crate) last_action_snapshot: Option<SessionActionSnapshot>,
    /// This session's RLM recursion depth (children run at depth + 1).
    pub(crate) rlm_depth: u32,
    /// The wall-clock ms of this session's last activity: stamped every time
    /// the runner parks after work, so the idle-eviction clock measures from
    /// the true end of the last activity. Zero means "no activity yet".
    pub(crate) last_activity_ms: u64,
    /// `top-level` | `subagent` (summary `runtimeKind`).
    pub(crate) runtime_kind: String,
    /// The subagent runtime identity (create `runtimeMetadata`): the child id
    /// and the parent's ids, so the roster keys children `parentPath#childId`.
    pub(crate) rlm_child_id: Option<String>,
    pub(crate) parent_active_session_id: Option<String>,
    pub(crate) parent_session_id: Option<String>,
    /// The create command's harness `childScript` (kept across the runtime swap
    /// so a replacement's children stay scripted); `None` for product sessions.
    pub(crate) child_script: Option<String>,
    /// The session's service-tier preference (TS `_serviceTierPreference`,
    /// `None` = "auto"): clamps `priority` to `default` without fast mode.
    pub(crate) service_tier: Option<pa_types::ai::ServiceTier>,
    /// The ACTIVE tier the engine's request slot carries: the preference clamped
    /// to the current model. Diverges from `service_tier` only while the current
    /// model does not support the requested tier.
    pub(crate) active_service_tier: Option<pa_types::ai::ServiceTier>,
    /// The queue delivery modes: "all" or "one-at-a-time". The steering default
    /// is "all" (queued steers co-deliver as ONE turn); the follow-up default is
    /// "one-at-a-time" (drain one per turn when idle).
    pub(crate) steering_mode: String,
    pub(crate) follow_up_mode: String,
    /// The one-shot forced steering batch (armed by `abort_and_send_queued`):
    /// armed items co-deliver as ONE batched turn at the next boundary, even
    /// under "one-at-a-time". Disarms when no armed item remains queued.
    pub(crate) forced_all_steering: bool,
    /// The scoped model list (TS `_scopedModels`): wire entries
    /// `{ model, thinkingLevel? }` the model cycler cycles within.
    pub(crate) scoped_models: Vec<Value>,
    /// A retry in flight was aborted (`abort_retry`); the turn's abort
    /// probe reads it and the next turn start clears it.
    pub(crate) retry_abort_requested: bool,
    /// Queued-input admission is suspended (`requestAbort` and manual `compact()`
    /// set it): the runner drains nothing and a plain prompt is rejected. Cleared
    /// by the resume sites: `steer`/`follow_up`, `streamingBehavior`, `resume_queue`,
    /// a queued-message mutation, a cron/heartbeat fire, a compact with an active goal.
    pub(crate) queued_input_suspended: bool,
    /// Restored next-turn rows (TS `_pendingNextTurnMessages`,
    /// `restore_next_turn`): delivered as prefix rows with the next turn.
    pub(crate) pending_next_turn: Vec<Value>,
    /// The queue projection's active action: the runner sets the phase transitions
    /// (`preparing` at pickup, `committing` at the first row, `running` at the
    /// first assistant frame) and clears it once the turn settles.
    pub(crate) active_action: Option<crate::types::SessionActionActive>,
    /// Settled executing-turn milliseconds since the active branch's latest
    /// successful full compaction. The optional instant adds the resident
    /// turn's live segment without writing progress checkpoints.
    pub(super) compaction_active_ms: u64,
    pub(super) compaction_active_since: Option<std::time::Instant>,
    /// Durable id of the latest full-compaction row on the active branch.
    pub(super) compaction_epoch: Option<String>,
}

impl SessionCore {
    /// Whether a turn, compaction, or queued action is in flight (TS
    /// `hasOngoingSessionWork`); the supervisor-lost exit waits for it to settle.
    pub(crate) fn has_ongoing_work(&self) -> bool {
        self.busy || self.compacting || !self.steering.is_empty() || !self.follow_up.is_empty()
    }

    /// Snapshot the working-runtime clock. While a turn is executing, the
    /// wall sample lets a cached reader extrapolate until the next roster row.
    #[must_use]
    pub(crate) fn compaction_clock(&self) -> (u64, Option<u64>) {
        self.compaction_clock_at(std::time::Instant::now(), crate::util::now_ms())
    }

    /// The active branch's latest successful full-compaction entry id.
    #[must_use]
    pub(crate) fn compaction_epoch(&self) -> Option<&str> {
        self.compaction_epoch.as_deref()
    }

    pub(crate) fn start_compaction_clock(&mut self) {
        self.start_compaction_clock_at(std::time::Instant::now());
    }

    /// End the resident turn segment and durably checkpoint settled work.
    pub(crate) fn settle_compaction_clock(&mut self) -> anyhow::Result<()> {
        self.stop_compaction_clock_at(std::time::Instant::now());
        let active_ms = self.compaction_active_ms;
        let compaction_epoch = self.compaction_epoch.as_deref();
        if let Some(store) = self.store.as_mut() {
            store.persist_compaction_clock(active_ms, compaction_epoch)?;
        }
        Ok(())
    }

    /// A successful full compaction starts a fresh epoch. An executing turn
    /// continues accruing from the reset boundary rather than stopping.
    pub(crate) fn reset_compaction_clock(&mut self, epoch: Option<String>) {
        self.reset_compaction_clock_at(epoch, std::time::Instant::now());
    }

    /// Rebind to a store/branch. Resident elapsed time never crosses this
    /// boundary, so an overnight restore cannot be mistaken for active work.
    pub(crate) fn restore_compaction_clock(
        &mut self,
        active_ms: u64,
        epoch: Option<String>,
    ) {
        self.compaction_active_ms = active_ms;
        self.compaction_active_since = None;
        self.compaction_epoch = epoch;
    }

    fn start_compaction_clock_at(&mut self, now: std::time::Instant) {
        if self.compaction_active_since.is_none() {
            self.compaction_active_since = Some(now);
        }
    }

    fn stop_compaction_clock_at(&mut self, now: std::time::Instant) {
        if let Some(started) = self.compaction_active_since.take() {
            self.compaction_active_ms = self
                .compaction_active_ms
                .saturating_add(duration_ms(now.saturating_duration_since(started)));
        }
    }

    fn reset_compaction_clock_at(
        &mut self,
        epoch: Option<String>,
        now: std::time::Instant,
    ) {
        self.compaction_active_ms = 0;
        if self.compaction_active_since.is_some() {
            self.compaction_active_since = Some(now);
        }
        self.compaction_epoch = epoch;
    }

    fn compaction_clock_at(
        &self,
        monotonic_now: std::time::Instant,
        wall_now_ms: u64,
    ) -> (u64, Option<u64>) {
        match self.compaction_active_since {
            Some(started) => (
                self.compaction_active_ms.saturating_add(duration_ms(
                    monotonic_now.saturating_duration_since(started),
                )),
                Some(wall_now_ms),
            ),
            None => (self.compaction_active_ms, None),
        }
    }

    /// A created session core for command modules' unit tests (the private
    /// bookkeeping fields stay owned here).
    #[cfg(test)]
    pub(crate) fn test_core(store: Option<SessionFile>, cwd: String) -> Self {
        let (compaction_active_ms, compaction_epoch) = store
            .as_ref()
            .map_or((0, None), SessionFile::compaction_clock_state);
        SessionCore {
            active_session_id: store.as_ref().map_or_else(
                || "test-session".to_string(),
                |store| store.session_id().to_string(),
            ),
            generation: String::new(),
            last_event_sequence: 0,
            store,
            cwd,
            steering: VecDeque::new(),
            follow_up: VecDeque::new(),
            busy: false,
            created: true,
            attached_client_ids: Vec::new(),
            abort_requested: false,
            turn_cancelled: false,
            turn_completed: false,
            suppress_aborted_row: false,
            shutdown_requested: false,
            last_activity_ms: 0,
            compacting: false,
            running_tool_calls: std::collections::HashSet::new(),
            running_admission_ids: std::collections::HashSet::new(),
            tool_use_count: 0,
            auto_compaction_enabled: true,
            last_action_snapshot: Some(SessionActionSnapshot::default()),
            rlm_depth: 0,
            runtime_kind: "top-level".to_string(),
            rlm_child_id: None,
            parent_active_session_id: None,
            parent_session_id: None,
            child_script: None,
            service_tier: None,
            active_service_tier: None,
            steering_mode: "all".to_string(),
            follow_up_mode: "one-at-a-time".to_string(),
            forced_all_steering: false,
            scoped_models: Vec::new(),
            retry_abort_requested: false,
            queued_input_suspended: false,
            pending_next_turn: Vec::new(),
            active_action: None,
            compaction_active_ms,
            compaction_active_since: None,
            compaction_epoch,
        }
    }
}

fn duration_ms(duration: std::time::Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn working_clock_counts_only_live_turn_segments_and_resets_in_place() {
        let base = std::time::Instant::now();
        let mut core = SessionCore::test_core(None, String::new());
        core.restore_compaction_clock(40, Some("old-compaction".to_string()));

        core.start_compaction_clock_at(base);
        assert_eq!(
            core.compaction_clock_at(base + std::time::Duration::from_millis(10), 1_000),
            (50, Some(1_000))
        );
        core.stop_compaction_clock_at(base + std::time::Duration::from_millis(15));
        assert_eq!(
            core.compaction_clock_at(base + std::time::Duration::from_secs(10), 11_000),
            (55, None),
        );

        let resumed = base + std::time::Duration::from_secs(20);
        core.start_compaction_clock_at(resumed);
        core.reset_compaction_clock_at(
            Some("new-compaction".to_string()),
            resumed + std::time::Duration::from_millis(7),
        );
        assert_eq!(core.compaction_epoch(), Some("new-compaction"));
        assert_eq!(
            core.compaction_clock_at(
                resumed + std::time::Duration::from_millis(12),
                21_000,
            ),
            (5, Some(21_000)),
        );
        core.stop_compaction_clock_at(resumed + std::time::Duration::from_millis(17));
        assert_eq!(core.compaction_clock_at(resumed, 0), (10, None));
    }

    #[test]
    fn restore_never_accrues_the_gap_while_the_worker_was_absent() {
        let base = std::time::Instant::now();
        let mut core = SessionCore::test_core(None, String::new());
        core.start_compaction_clock_at(base);
        core.stop_compaction_clock_at(base + std::time::Duration::from_millis(25));

        core.restore_compaction_clock(25, None);

        assert_eq!(
            core.compaction_clock_at(base + std::time::Duration::from_secs(86_400), 86_400_000),
            (25, None),
        );
    }
}
