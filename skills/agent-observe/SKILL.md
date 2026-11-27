---
name: agent-observe
description: Read-only family observation and content-free context health for other resident daemon sessions. Use to inspect family status, bounded message previews, and context-maintenance pressure without mutating sessions.
---

# Agent Observe

Observe the current agent's nuclear family through the local daemon: parent,
siblings, direct children, and self. `list_agents` is the one family roster and
covers every member `agent_message.send` can reach. `get_agent` also returns an
inactive child's roster row. `recent_messages` also reads a member no worker
holds (a passivated or finished child) from its session file, read-only,
without waking it. They cannot read a root sibling that has no live session in
this worker.

`context_health` is a separate, content-free daemon-wide view of every other
resident session, including unrelated sessions and idle sessions. It exposes
only identifiers, family relationship when one exists, live state, and numeric
context-maintenance counters. It never returns prompts, transcript text,
progress notes, paths, or other session content. Observation itself cannot
compact, release, prompt, steer, clear, kill, rename, or otherwise mutate a
session.

Call directly from the kernel:

```python
children = await rlm.list_subagents()
child = next((item for item in children if item.active_session_id), None)
if child is not None:
    worker = await agent_observe.get_agent(child.session_name)
    recent = await agent_observe.recent_messages(child.session_name, limit=6)
    # Deletion is a parent-owned RLM operation, not an observe mutation:
    await rlm.delete_subagent(child)
```

For an explicitly requested advisory heartbeat, dedupe once per target and
successful full-compaction epoch:

```python
_context_advised = globals().setdefault("_context_advised", set())

async def context_janitor():
    for row in await agent_observe.context_health(min_active_seconds=3 * 3600):
        key = (row["activeSessionId"], row["compactionEpoch"])
        if not row["isCompacting"] and key not in _context_advised:
            await agent_message.remind_context(row["activeSessionId"], reason="compact")
            _context_advised.add(key)  # only after the advisory was accepted

await rlm_heartbeat.create(
    "Run `await context_janitor()` once; do not compact or release another session.",
    interval="15m",
    label="context-janitor",
)
```

The reminder is advisory only. `context_health` never sends it automatically,
and it never runs a process in the target session. A caller may instead filter
on a configurable `min_tool_percent` and use `reason="release"`.

## API

- `await agent_observe.list_agents(include_descendants=False)` returns `current` (the calling agent's
  own row) and `agents` (the family: parent, siblings, and direct children,
  active or not; the caller is excluded from `agents`). Each row carries
  `sessionId`, optional `sessionName`, `relationship`
  (`parent`/`sibling`/`child`), `status` (`running`/`idle` for live sessions,
  `inactive` without one), the live `activity` (`tool`/`model`/`compacting`/
  `busy`/`user`/`idle`), `isSessionActive`, the counts known
  for it (`messageCount`, `queuedCount`, `attachedClients`), `cwd` when the
  roster row carries it, the parent edges (`parentActiveSessionId`,
  `parentSessionId`) and `rlmChildId` for subagents, and `firstMessage`
  (capped at 240 chars) for an inactive child. A member with no live session
  has no `activeSessionId` and no live detail; with `include_descendants=True`
  `agents` also holds every descendant of the caller, each with `depth` (a child
  is 1; grandchildren and deeper have `relationship` null; follow
  `parentActiveSessionId`/`parentSessionId` to rebuild the tree); address it with
  `agent_message.send` using its `relationship` plus its `sessionName`, or its
  `sessionId` when the member has no name. For direct children,
  `await rlm.list_subagents()` also exposes parent-owned lifecycle handles.
- `await agent_observe.context_health(*, min_tool_percent=None,
  min_active_seconds=None)` returns a Python list of health rows for every
  *other* live resident daemon session. Either threshold is optional,
  inclusive, and the two filters combine with OR; a null measurement cannot
  satisfy its threshold. With neither threshold, all other residents are
  returned, including idle sessions. The fields are
  `activeSessionId`, nullable `sessionId`, nullable `sessionName`, nullable
  `parentActiveSessionId`, nullable `relationship`
  (`parent`/`sibling`/`child`), `isStreaming`, `isCompacting`, nullable
  `toolOutputTokens`, nullable `contextWindow`, nullable
  `toolOutputPercent`, nullable `lastReleaseKind`, nullable
  `toolResultsSinceRelease`, nullable `activeSecondsSinceCompaction`, and
  nullable `compactionEpoch`.
  `toolOutputPercent` is retained tool RESULT output as a percentage of the
  model's full context window. `lastReleaseKind` is the latest observed
  successful release directive (`last`, `current`, `tail`, `all`, or
  `explicit`), and `toolResultsSinceRelease` counts subsequent results still
  in the active context. A low percentage after `all`, or after releasing a
  small last result, is not proof that the agent cleaned as it went.
  Working time counts executing turns, including tool execution and the
  current open segment, but not idle time. Tool-result releases do not reset
  that clock; only a successful full compaction does. `compactionEpoch`
  identifies that compaction and is null before the first one. Settled turn
  time survives worker restarts. Existing sessions begin accruing measurable
  working time only after upgrading because historical work versus idle time
  cannot be reconstructed; their last compaction ID can still populate the
  epoch. Fresh or mixed-version workers can report null metrics until their
  first health snapshot reaches the supervisor.
- `await agent_observe.get_agent(target)` returns `agent`, where `agent`
  contains one agent summary (the same shape as the roster rows, including a
  passive child's row). `target` is resolved like other live-session
  selectors: active id, session id or name, or an unambiguous id suffix.
- `await agent_observe.recent_messages(target, limit=8, max_chars=800)`
  returns `agent` (the target's summary), `messages` (up to `limit` recent
  bounded previews), the clamped `limit` and `maxChars`, and `truncated`
  (true when older messages exist beyond the returned window). `limit` must be
  1-50, and `max_chars` must be 80-2000. Each preview's `index` is the
  message's position in the full transcript.

## Safety

- This skill's observation calls expose no mutation commands. The heartbeat
  example opts into the separate advisory `agent_message.remind_context` API.
- Family transcript reads reject targets outside the nuclear family and remain
  bounded by count and per-message character limit.
- Context-health rows contain no transcript or prompt content and grant no
  additional transcript or generic family permissions.
