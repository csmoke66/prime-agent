# bh-telemetry

Modular telemetry: the event schema, queueing/batching, sinks, and the
pseudonymous installation identity. Events never leave the machine.

## Scope

- `Properties`: primitive-only property maps (the privacy boundary — strings,
  numbers, booleans, null; structured values are rejected at insertion).
- `TelemetryEvent`: `{id, name, timestamp, properties}` records.
- `TelemetryClient`: non-blocking `track(event, properties)`, `flush`,
  `shutdown`. One background worker, capped queue (drop-oldest), batched
  flush (size + interval + explicit), fan-out to every sink. Best-effort by
  contract: never blocks, never panics, never fails the agent.
- `TelemetrySink` trait + shipped sinks:
  `FileSink` (local JSONL transparency mirror at `<agentDir>/telemetry.jsonl`),
  `NoopSink` (opt-out fast path), `MockSink` (tests, also re-exported for
  downstream crate tests).
- `install_id`: `<agentDir>/telemetry.json` `{version, installationId}`,
  exclusive 0600 create, validated on load (TS parity).
- `rename_onto`: the shared rename-onto-destination primitive for durable
  persist writes (TS `renameOntoSync`: bounded win32 destination-busy
  retry, `10ms * attempt` backoff, every failure immediate off Windows).
  Lives in this crate because every persist owner (bh-core's settings and
  session writes, bh-daemon's descriptors/journals/session store, this
  crate's install id) already depends on it while it depends on no other
  workspace crate; bh-core re-exports it as `platform::rename_onto` so its
  platform wall stays the engine's single platform entry.
- The TS boolean env parsing (`parse_bool_override`); the opt-out
  precedence (`BEEHIVE_OFFLINE`, `DO_NOT_TRACK`, `BEEHIVE_TELEMETRY`,
  settings) lives in bh-core's `telemetry_switch`.

## Non-goals

- No settings reading: the enabled flow is owned by bh-core
  settings + bh-cli wiring (this crate only answers "does the environment
  override?").
- No event emission from this crate: seams live in bh-core/bh-daemon/bh-cli,
  which call `track` with schema-documented events.
- No prompt/session content, tool arguments, file paths, or user identity —
  by construction (primitives-only properties).
- No retries or delivery guarantees: batches are best-effort and offline-safe
  (dropped on failure), matching the TS product.

## Public API

- `TelemetryClient` / `TelemetryClientConfig` (`track`, `flush`, `shutdown`,
  `dropped_count`, `install_id`, `spawn`)
- `TelemetrySink` trait, `SinkOutcome`
- `Properties`, `TelemetryEvent`
- `install_id(agent_dir)`
- `rename_onto(from, to)`
- `parse_bool_override(value)`, `existing_install_id(agent_dir)`
- Sinks: `FileSink`, `NoopSink`, `MockSink` (+ `RecordedBatch`)

## Placement

Depends on no workspace crate (sits beside bh-types at the bottom of the
dependency graph). Consumers: bh-core, bh-daemon, bh-cli.
