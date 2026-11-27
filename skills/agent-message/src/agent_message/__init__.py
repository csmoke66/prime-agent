"""Beehive session-to-session messaging skill.

All routing and sender identity live in the TypeScript daemon. These functions
only call the host bridge exposed inside the Beehive kernel.
"""

from __future__ import annotations

from typing import Any, Literal

from rlm import host_request

ReceiverRole = Literal["parent", "sibling", "child"]
MessagePriority = Literal["low", "default", "medium", "high"]
ContextReminderReason = Literal["compact", "release"]


async def remind_context(
    target: str,
    *,
    reason: ContextReminderReason | str = "compact",
) -> dict[str, Any]:
    """Send a fixed context-maintenance advisory to one exact resident session."""
    if not isinstance(target, str):
        raise TypeError(f"target must be str, got {type(target).__name__}")
    if not target or target != target.strip():
        raise ValueError("target must be an exact active session id")
    if reason not in ("compact", "release"):
        raise ValueError('reason must be "compact" or "release"')
    receipt = await host_request(
        "agent_message.remind_context",
        {"target": target, "reason": reason},
    )
    _emit_sent_message(receipt)
    return receipt


_MESSAGE_DISPLAY_MIME = "application/vnd.beehive.agent-message+json"


async def send(
    message: str,
    broadcast_message: str | None = None,
    *,
    receiver_role: ReceiverRole | str | None = None,
    receiver_name: str | None = None,
    priority: MessagePriority | str = "default",
) -> dict[str, Any]:
    """Send one direct role-addressed message or broadcast with sender priority."""
    roles = ("parent", "sibling", "child")
    priorities = ("low", "default", "medium", "high")
    if priority not in priorities:
        raise ValueError('priority must be "low", "default", "medium", or "high"')
    if broadcast_message is not None:
        if message != "all":
            raise TypeError(
                "positional agent_message.send targets are not supported; "
                "use receiver_role and receiver_name"
            )
        if receiver_role is not None or receiver_name is not None:
            raise TypeError("broadcast cannot be combined with receiver_role/receiver_name")
        payload: dict[str, Any] = {
            "target": "all",
            "message": broadcast_message,
            "priority": priority,
        }
    else:
        if receiver_role not in roles:
            raise ValueError('receiver_role must be "parent", "sibling", or "child"')
        if not isinstance(message, str):
            raise TypeError(f"message must be str, got {type(message).__name__}")
        if receiver_role == "parent":
            if receiver_name is not None:
                raise ValueError("receiver_name must be omitted for parent messages")
        elif not isinstance(receiver_name, str) or not receiver_name.strip():
            raise ValueError("receiver_name is required for sibling and child messages")
        payload = {
            "message": message,
            "receiver_role": receiver_role,
            "receiver_name": receiver_name,
            "priority": priority,
        }
    receipt = await host_request("agent_message.send", payload)
    receipts = receipt.get("receipts") if isinstance(receipt, dict) else None
    if isinstance(receipts, list):
        for item in receipts:
            if isinstance(item, dict) and "deliveryStatus" in item:
                _emit_sent_message(item)
    else:
        _emit_sent_message(receipt, receiver_role)
    return receipt


async def reply(
    message: str,
    *,
    priority: MessagePriority | str = "default",
) -> dict[str, Any]:
    """Reply to this agent's parent with sender-controlled priority."""
    return await send(message, receiver_role="parent", priority=priority)


def _emit_sent_message(receipt: dict[str, Any], receiver_role: str | None = None) -> None:
    try:
        from rlm import emit

        label = (
            "Agent message queued"
            if receipt.get("deliveryStatus") == "queued"
            else "Agent message sent"
        )
        display_receipt = dict(receipt)
        if receiver_role in ("parent", "sibling", "child"):
            display_receipt["receiverRole"] = receiver_role
        emit(
            {
                _MESSAGE_DISPLAY_MIME: display_receipt,
                "text/plain": label,
            }
        )
    except Exception:
        pass
