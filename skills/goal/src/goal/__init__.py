"""Beehive goal skill: manage the persistent thread goal from the kernel.

All goal state lives in the TypeScript host; these functions are thin typed
wrappers over the generic host bridge (`rlm.host_request`). They only work
inside the Beehive Python kernel.
"""

from __future__ import annotations

from typing import Any

from rlm import host_request


async def get(session: str | None = None) -> dict[str, Any]:
    """Read a thread goal: this session's, or another resident session's by id.

    Returns a dict with `goal` (None when no goal is set), `remaining_tokens`,
    and `completion_budget_report`. The `goal` dict carries the objective,
    status, token budget, and token/elapsed-time usage.
    """
    if session is None:
        return await host_request("goal.get")
    return await _control("get", session)


async def _control(op: str, session: str | None, **fields: Any) -> dict[str, Any]:
    if session is not None and not isinstance(session, str):
        raise TypeError(f"session must be str or None, got {type(session).__name__}")
    payload = {"op": op, **{k: v for k, v in fields.items() if v is not None}}
    if session is not None:
        payload["session"] = session
    return await host_request("goal.control", payload)


async def pause(session: str | None = None) -> dict[str, Any]:
    """Pause an active goal (this session's, or another resident session's by id).

    Queued continuation nudges are withdrawn and no new ones are minted until `resume`.
    """
    return await _control("pause", session)


async def resume(session: str | None = None) -> dict[str, Any]:
    """Resume a paused, budget-limited, or failed goal; its continuations restart."""
    return await _control("resume", session)


async def update(
    objective: str | None = None,
    token_budget: int | None = None,
    session: str | None = None,
) -> dict[str, Any]:
    """Edit a goal's objective and/or token budget, keeping its usage accounting.

    An active goal's session receives a harness notice with the new objective.
    """
    if objective is not None and not isinstance(objective, str):
        raise TypeError(f"objective must be str or None, got {type(objective).__name__}")
    if token_budget is not None and not isinstance(token_budget, int):
        raise TypeError(f"token_budget must be int or None, got {type(token_budget).__name__}")
    return await _control("update", session, objective=objective, token_budget=token_budget)


async def stop(session: str | None = None) -> dict[str, Any]:
    """Clear a goal entirely (this session's, or another resident session's by id)."""
    return await _control("stop", session)


async def create(objective: str, token_budget: int | None = None) -> dict[str, Any]:
    """Start a new active thread goal.

    Fails while a goal is still pending (active, paused, or budget-limited);
    a completed or errored goal is replaced. Only create a goal when the user
    or system/developer instructions explicitly ask for a persistent
    long-running goal. Set `token_budget` only when an explicit token budget is
    requested.
    """
    if not isinstance(objective, str):
        raise TypeError(f"objective must be str, got {type(objective).__name__}")
    if token_budget is not None and not isinstance(token_budget, int):
        raise TypeError(f"token_budget must be int or None, got {type(token_budget).__name__}")
    payload: dict[str, Any] = {"objective": objective}
    if token_budget is not None:
        payload["token_budget"] = token_budget
    return await host_request("goal.create", payload)


async def complete() -> dict[str, Any]:
    """Mark the existing thread goal achieved.

    Use only when the objective has actually been achieved and no required
    work remains — not because the budget is nearly exhausted or because you
    are stopping work. Pause, resume, and budget-limit transitions are
    controlled by the user and the host.
    """
    return await host_request("goal.complete")
