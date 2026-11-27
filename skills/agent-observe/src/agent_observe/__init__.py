"""Read-only Beehive session observation skill.

All session lookup and data access live in the daemon. These functions only
call the host bridge exposed inside the Beehive Python kernel.
"""

from __future__ import annotations

from math import isfinite
from typing import Any

from rlm import host_request


async def list_agents() -> dict[str, Any]:
    """List the full nuclear family: parent, siblings, children, active or not."""
    return await host_request("agent_observe.list")


async def context_health(
    *,
    min_tool_percent: float | None = None,
    min_active_seconds: int | None = None,
) -> list[dict[str, Any]]:
    """Return content-free maintenance health for other resident sessions.

    Thresholds are inclusive and combine with OR. With neither threshold,
    every other resident session is returned, including idle sessions.
    """
    payload: dict[str, Any] = {}
    if min_tool_percent is not None:
        if isinstance(min_tool_percent, bool) or not isinstance(
            min_tool_percent, (int, float)
        ):
            raise TypeError(
                "min_tool_percent must be int, float, or None, "
                f"got {type(min_tool_percent).__name__}"
            )
        try:
            finite = isfinite(min_tool_percent)
        except OverflowError:
            finite = False
        if not finite or min_tool_percent < 0:
            raise ValueError("min_tool_percent must be finite and nonnegative")
        payload["min_tool_percent"] = min_tool_percent
    if min_active_seconds is not None:
        if isinstance(min_active_seconds, bool) or not isinstance(
            min_active_seconds, int
        ):
            raise TypeError(
                "min_active_seconds must be int or None, "
                f"got {type(min_active_seconds).__name__}"
            )
        if min_active_seconds < 0:
            raise ValueError("min_active_seconds must be nonnegative")
        payload["min_active_seconds"] = min_active_seconds
    result = await host_request("agent_observe.context_health", payload)
    agents = result.get("agents") if isinstance(result, dict) else None
    if not isinstance(agents, list):
        raise RuntimeError(
            "agent_observe.context_health is unavailable; update or restart Beehive"
        )
    return agents


async def get_agent(target: str) -> dict[str, Any]:
    """Read one family member's summary by active id, session id/name, or id suffix."""
    if not isinstance(target, str):
        raise TypeError(f"target must be str, got {type(target).__name__}")
    return await host_request("agent_observe.get", {"target": target})


async def recent_messages(
    target: str,
    limit: int = 8,
    max_chars: int = 800,
) -> dict[str, Any]:
    """Read bounded recent message previews from a family member's session.

    Args:
        target: Active session id, session id/name, or unambiguous id suffix.
        limit: Number of recent messages to return. Host validates 1-50.
        max_chars: Per-message preview size. Host validates 80-2000.
    """
    if not isinstance(target, str):
        raise TypeError(f"target must be str, got {type(target).__name__}")
    if not isinstance(limit, int):
        raise TypeError(f"limit must be int, got {type(limit).__name__}")
    if not isinstance(max_chars, int):
        raise TypeError(f"max_chars must be int, got {type(max_chars).__name__}")
    return await host_request(
        "agent_observe.recent",
        {
            "target": target,
            "limit": limit,
            "max_chars": max_chars,
        },
    )
