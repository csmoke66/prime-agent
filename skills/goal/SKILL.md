---
name: goal
description: Manage the persistent thread goal from the Python REPL. Use to read goal status and budget usage, to start a goal when the user explicitly asks for one, or to mark the active goal complete once its objective is fully achieved.
---

# Goal

The thread goal is a persistent objective the harness keeps re-prompting you to
pursue across turns until it is complete. Goal state (status, token budget,
usage accounting) lives in the host; this skill is the kernel-side interface to
it. Call it directly from the Python REPL:

```python
await goal.get()
await goal.create("ship the release notes")
# Only pass token_budget when the user explicitly asks for one:
# await goal.create("ship the release notes", token_budget=200000)
await goal.complete()
```

## API

- `await goal.get(session=None)` — current goal as a dict: `goal` (or `None` when no goal
  is set), `remaining_tokens`, and `completion_budget_report`. The `goal` dict
  carries `objective`, `status`, `token_budget`, `tokens_used`,
  `time_used_seconds`, and timestamps.
- `await goal.create(objective, token_budget=None)` — start a new active goal.
  Fails while a goal is still pending (active, paused, or budget-limited); a
  completed or errored goal is replaced by the new one. Only create a goal when
  the user or system/developer instructions explicitly ask for a persistent
  long-running goal; do not infer goals from ordinary tasks. Set `token_budget`
  only when an explicit token budget is requested.
- `await goal.complete()` — mark the existing goal achieved. Use only when the
  objective has actually been achieved and no required work remains; do not
  call it merely because the budget is nearly exhausted or because you are
  stopping work. When the result includes a `completion_budget_report`, report
  that final usage to the user.

## Controlling another session's goal

Every call below takes `session=` (a resident session's active id); omitted, it
acts on this session. Any resident session is a valid target, like
`rlm.workflows.run(session=...)`. Use it only when the user or the task asks
you to manage that agent's goal.

- `await goal.get(session=...)` — read the goal.
- `await goal.pause(session=...)` — suspend an active goal; no continuation
  nudges run until resumed.
- `await goal.resume(session=...)` — restart a paused, budget-limited, or
  failed goal and its continuations.
- `await goal.update(objective=None, token_budget=None, session=...)` — edit
  the objective and/or budget; an active target gets a harness notice.
- `await goal.stop(session=...)` — clear the goal.

## Rules

- Budget-limiting is applied by the host. Pause, resume, update, and stop are
  for the user's explicit requests, mostly on other agents' goals.
- When an active goal is actually complete, call `await goal.complete()`; do
  not merely say it is done — the harness keeps continuing the goal until the
  completion call arrives.
