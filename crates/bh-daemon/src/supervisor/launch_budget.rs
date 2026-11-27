//! The worker launch budget: connect probes, the connect deadline,
//! the auth floor, and the env override.

/// Worker connect budget: probes, connect, and the auth handshake all share this deadline
/// (TS `WORKER_CONNECT_TIMEOUT_MS`: 30s Unix, 90s Windows). Env-overridable for slow
/// environments; the default keeps the TS wire behavior.
pub(super) const DEFAULT_WORKER_CONNECT_TIMEOUT_MS: u64 = 30_000;
/// The auth handshake's minimum budget: a worker that just proved life gets at least
/// this long to answer, so the launch fails with the connect-budget error only when the
/// worker is genuinely wedged.
pub(super) const WORKER_AUTH_FLOOR_MS: u64 = 10_000;
/// Overrides [`DEFAULT_WORKER_CONNECT_TIMEOUT_MS`] when set to a positive
/// number of milliseconds (tests under parallel load use this seam).
pub(super) const WORKER_CONNECT_TIMEOUT_ENV: &str = "PA_DAEMON_WORKER_CONNECT_TIMEOUT_MS";
/// One socket probe attempt (TS `WORKER_CONNECT_PROBE_MS`).
pub(super) const WORKER_CONNECT_PROBE_MS: u64 = 500;
/// Pause between probe attempts. TS `WORKER_PROBE_BACKOFF_MIN_MS` (25ms)
/// doubles per retry up to `WORKER_PROBE_BACKOFF_MAX_MS` (win32 2s, unix
/// 25 - TS's min equals its max there, so the doubling is a flat grid).
/// Unix keeps a flat pause (the doubling adds nothing when min==max) at
/// 1ms: a session worker binds its socket ~1-3ms after the fork, the
/// probe loop starts once the descriptor's durable persist has landed,
/// and the pause quantizes the bind only in the windows where that
/// persist finishes before the worker binds - a 1ms pause caps the
/// overshoot at <=1ms for the cost of ~2-4 failed connects per spawn
/// (each ~us). Windows doubles exactly like TS (a flat 2s first retry
/// would sleep through the sub-second boots the doubling exists to
/// catch). Overshoot tables: probe-grid record 20261001-034500 on the
/// bench repo. Timing-only: the probe, the connect budget, the auth
/// floor, and the launch-failure error are unchanged.
pub(super) const WORKER_CONNECT_BACKOFF_MS: u64 = 1;

