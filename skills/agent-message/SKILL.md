---
name: agent-message
description: Message an agent's parent, siblings, or direct children, or send a fixed context advisory to one exact resident session ID. Sender identity is daemon-derived.
---

# Agent Message

`send` delivers direct messages within the current agent's nuclear family
(parent, siblings, and direct children; roots are siblings).
`remind_context` is a separate narrow janitor surface for a fixed advisory to
one exact live resident session ID, including an unrelated subagent. The daemon
derives your sender identity from the current session; do not try to include a
`from` field.

Call directly from the kernel:

```python
children = await rlm.list_subagents()
child = next((item for item in children if item.active_session_id), None)
if child is not None:
    receipt = await agent_message.send(
        "Please inspect the latest result.",
        receiver_role="child",
        receiver_name=child.session_name,
        priority="medium",
    )
    # Keep the child until this follow-up finishes so its result remains observable.
```

## API

- `await agent_observe.list_agents()` (agent-observe skill) is the roster: it
  lists the parent, siblings, and children this skill can reach, active or not,
  with the `relationship` and `sessionName` that `send` takes as
  `receiver_role` and `receiver_name`.
- `await agent_message.send(message, receiver_role="parent" | "sibling" | "child", receiver_name=None, priority="default")` — sends one direct
  text message to one family member. `priority` is `"low"`, `"default"`,
  `"medium"`, or `"high"`; it is preserved in the receiver's inbox metadata.
  Low priority is silent, default uses the normal bounded notice, medium includes
  a bounded preview, and high forces the full message into the wake notice.
  `await agent_message.reply(message, priority="default")` is the parent-reply
  convenience form and accepts the same priorities.
  Sending to an inactive or idle completed subagent normally wakes it and starts
  an ordinary follow-up turn in that same session and context.
  The child remains available only until its parent session closes. The daemon
  resolves `receiver_role` within the current agent family; `receiver_name` is
  required for siblings and children and omitted for the unique parent.
  `send("all", message, priority=...)` broadcasts only to the family roster and
  returns `{receipts: [...]}` in roster order; successful entries are ordinary
  receipts and failed entries contain the target id and a short `error`. One
  failed delivery does not reject successful deliveries. Messages use steering
  delivery so a busy target sees them during its active run. Returns a receipt
  with a `deliveryStatus` field: `"delivered"` means the message reached an idle
  target's context; `"queued"` means a steering message was accepted and will
  deliver when the target's current work allows (`send` does not block waiting
  for that). Delivered receipts carry `deliveredAt`, queued receipts carry
  `queuedAt`.
- `await agent_message.remind_context(active_session_id, reason="compact" | "release")`
  sends one fixed, low-priority context-maintenance advisory to the exact live
  resident ID returned by `agent_observe.context_health`; it does not accept a
  name, suffix, durable ID, custom text, broadcast, or priority. `"compact"`
  suggests choosing a useful checkpoint and fully compacting when safe.
  `"release"` suggests reviewing expendable tool outputs: consume the
  immediately preceding result in the next useful cell, then take a fresh
  checkpoint and call `rlm.release()`; use `rlm.release("tail")` when the
  whole exchange is spent, and reserve `rlm.release("all")` for occasional
  cleanup. The target decides whether and when to act. Delivery uses the
  ordinary attributed inbox path and returns its regular receipt. A stale or
  passivated target, or one whose inbox is disabled or full, is rejected rather
  than woken; a
  low-priority reminder waits silently in an idle resident's inbox until
  unrelated work wakes it.

## Safety

- Do not delete a child immediately after `send`: delivered follow-ups may still
  be running and queued receipts have not run yet. Wait until observation shows
  the child is idle and its context is no longer needed before calling
  `await rlm.delete_subagent(child)`.
- Reach is limited to parent, siblings, and direct children; relay through an
  intermediate child instead of messaging grandchildren or cousins directly.
- Sender identity is daemon-derived and cannot be spoofed from Python.
- The daemon enforces message size, rate, and pending-queue limits before
  accepting delivery.
- Context reminders are canned advisories, not arbitrary cross-family
  messaging or an instruction to compact. They cannot force compaction,
  escalate priority, wake a saved session, or spoof their sender.
