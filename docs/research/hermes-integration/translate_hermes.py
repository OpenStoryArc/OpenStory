"""
translate_hermes.py — Hermes-native event lines → CloudEvents.

Python sketch / executable spec for the eventual Rust translator at
`rs/core/src/translate_hermes.rs`. Parallel to `translate_pi.rs` (the pi-mono
translator referenced in BACKLOG.md). The point is the structure, not the
production fidelity — once this passes its tests against real Hermes data,
the Rust port is straightforward.

Inputs:
    A `.jsonl` file written by the hermes-openstory plugin. Each line is a
    Hermes-native event with a small envelope:

        {
          "envelope": {
            "session_id": str,
            "event_seq": int,        # monotonic per session
            "timestamp": str,        # ISO-8601
            "source": "hermes",
            ... # optional: hermes_version, model, platform
          },
          "event_type": "session_start" | "message" | "session_end",
          "data": { ... event-type-specific shape ... }
        }

    The `data` field of an `event_type: "message"` event carries Hermes's
    in-memory message dict, exactly as the agent loop sees it. That shape is
    where the # VERIFY: markers live — it depends on which provider is active
    in the Hermes session.

Outputs:
    A list of CloudEvents (as Python dicts), in the shape OpenStory's
    pipeline expects. The fields are CloudEvents 1.0 plus an OpenStory-
    specific `data` payload. The exact shape mirrors what
    `rs/core/src/translate.rs` (the Claude Code translator) produces.

This module is pure: line in, dict out, no I/O beyond the optional
`translate_file()` convenience function.
"""

from __future__ import annotations

import json
import uuid
from typing import Any, Dict, Iterable, List, Optional


# ─────────────────────────────────────────────────────────────────────────────
# Top-level entry points
# ─────────────────────────────────────────────────────────────────────────────


def translate_line(line: str) -> List[Dict[str, Any]]:
    """Translate one Hermes-native JSONL line into zero or more CloudEvents.

    Returns a list because some Hermes events fan out into multiple
    CloudEvents (e.g. an assistant message with N tool calls becomes one
    `message.assistant.tool_use` event per tool call, the same way the
    Claude Code translator handles multi-tool turns).

    Pure: same input, same output, always.
    """
    line = line.strip()
    if not line:
        return []
    raw = json.loads(line)
    return translate_event(raw)


def translate_event(raw: Dict[str, Any]) -> List[Dict[str, Any]]:
    """Dispatch on `event_type` and produce CloudEvents."""
    envelope = raw.get("envelope", {})
    data = raw.get("data", {})
    event_type = raw.get("event_type", "")

    session_id = envelope.get("session_id", "")
    timestamp = envelope.get("timestamp", "")
    seq = envelope.get("event_seq", 0)

    if event_type == "session_start":
        return [_make_session_start(session_id, timestamp, seq, envelope, data)]
    elif event_type == "session_end":
        return [_make_turn_complete(session_id, timestamp, seq, data)]
    elif event_type == "message":
        return _translate_message(session_id, timestamp, seq, data)
    else:
        return []  # unknown event types are dropped silently — forward-compat


def translate_file(path: str) -> List[Dict[str, Any]]:
    """Read a Hermes events JSONL file and return all CloudEvents.

    Convenience helper for tests and one-shot backfill scripts. Production
    OpenStory should stream lines through `translate_line()` from the file
    watcher, not buffer the whole file.
    """
    out: List[Dict[str, Any]] = []
    with open(path, "r", encoding="utf-8") as f:
        for line in f:
            out.extend(translate_line(line))
    return out


# ─────────────────────────────────────────────────────────────────────────────
# Message translation — the load-bearing core
# ─────────────────────────────────────────────────────────────────────────────


def _translate_message(
    session_id: str, timestamp: str, seq: int, msg: Dict[str, Any]
) -> List[Dict[str, Any]]:
    """Convert one Hermes message dict into one or more CloudEvents.

    The role determines the subtype prefix:
        user      → message.user.prompt
        assistant → message.assistant.{thinking | text | tool_use}
        tool      → message.user.tool_result
        system    → system.injected.{compression | todo | other}
    """
    role = msg.get("role", "")

    if role == "user":
        return [_make_user_prompt(session_id, timestamp, seq, msg)]

    if role == "tool":
        return [_make_tool_result(session_id, timestamp, seq, msg)]

    if role == "system":
        return [_make_system_injected(session_id, timestamp, seq, msg)]

    if role == "assistant":
        return _translate_assistant_message(session_id, timestamp, seq, msg)

    return []


def _translate_assistant_message(
    session_id: str, timestamp: str, seq: int, msg: Dict[str, Any]
) -> List[Dict[str, Any]]:
    """Assistant messages are the most complex.

    A single assistant turn from Hermes can carry:
      - reasoning / thinking (Anthropic-style or OpenAI o1-style)
      - text content
      - one or more tool calls

    We emit one CloudEvent per logical sub-event so OpenStory's pattern
    detector can recognize them as separate eval/apply phases.
    """
    out: List[Dict[str, Any]] = []

    # ── Thinking phase (if present) ──
    # VERIFY: Hermes may store reasoning as either:
    #   (a) a top-level `reasoning` string field on the assistant message
    #       (likely for OpenAI/OpenRouter providers and o1-style models)
    #   (b) a `content` array containing {"type": "thinking", "text": ...} blocks
    #       (likely for Anthropic-direct provider)
    # The translator currently checks (a) first and (b) second. Confirm by
    # reading one finalized session log per provider type.
    reasoning = msg.get("reasoning")
    if reasoning:
        out.append(
            _make_event(
                subtype="message.assistant.thinking",
                session_id=session_id,
                timestamp=timestamp,
                seq=seq,
                payload={"text": reasoning},
            )
        )
    else:
        # Check Anthropic-style content blocks
        content = msg.get("content")
        if isinstance(content, list):
            for block in content:
                if isinstance(block, dict) and block.get("type") == "thinking":
                    out.append(
                        _make_event(
                            subtype="message.assistant.thinking",
                            session_id=session_id,
                            timestamp=timestamp,
                            seq=seq,
                            payload={"text": block.get("text", "")},
                        )
                    )

    # ── Text content (if any) ──
    text = _extract_text(msg)
    has_tool_calls = bool(msg.get("tool_calls")) or _has_anthropic_tool_use(msg)

    # If the assistant produced text *and* tool calls, the text is part of the
    # eval phase that precedes the apply phase. Emit it as a separate event so
    # the pattern detector can attribute it correctly.
    if text and not has_tool_calls:
        out.append(
            _make_event(
                subtype="message.assistant.text",
                session_id=session_id,
                timestamp=timestamp,
                seq=seq,
                payload={
                    "text": text,
                    "stop_reason": msg.get("stop_reason") or msg.get("finish_reason"),
                },
            )
        )

    # ── Tool calls (if any) ──
    # VERIFY: Hermes message dicts use the OpenAI shape when the provider is
    # OpenRouter / Nous Portal / OpenAI:
    #   tool_calls: [{"id": ..., "type": "function",
    #                 "function": {"name": ..., "arguments": "<json string>"}}]
    # And the Anthropic shape when the provider is Anthropic direct:
    #   content: [..., {"type": "tool_use", "id": ..., "name": ..., "input": {...}}]
    # Both branches are handled below; verify the field names against a real
    # session log for each provider before relying on this in production.
    for tc in msg.get("tool_calls", []) or []:
        out.append(_translate_openai_tool_call(session_id, timestamp, seq, tc, text))

    if _has_anthropic_tool_use(msg):
        for block in msg.get("content", []) or []:
            if isinstance(block, dict) and block.get("type") == "tool_use":
                out.append(
                    _translate_anthropic_tool_use(session_id, timestamp, seq, block, text)
                )

    return out


def _translate_openai_tool_call(
    session_id: str, timestamp: str, seq: int, tc: Dict[str, Any], preceding_text: str
) -> Dict[str, Any]:
    fn = tc.get("function", {})
    name = fn.get("name", "")
    args_raw = fn.get("arguments", "{}")
    try:
        args = json.loads(args_raw) if isinstance(args_raw, str) else args_raw
    except json.JSONDecodeError:
        args = {"_raw": args_raw}
    return _make_event(
        subtype="message.assistant.tool_use",
        session_id=session_id,
        timestamp=timestamp,
        seq=seq,
        payload={
            "tool": name,
            "tool_use_id": tc.get("id", ""),
            "args": args,
            "preceding_text": preceding_text or None,
            "stop_reason": "tool_use",
        },
    )


def _translate_anthropic_tool_use(
    session_id: str, timestamp: str, seq: int, block: Dict[str, Any], preceding_text: str
) -> Dict[str, Any]:
    return _make_event(
        subtype="message.assistant.tool_use",
        session_id=session_id,
        timestamp=timestamp,
        seq=seq,
        payload={
            "tool": block.get("name", ""),
            "tool_use_id": block.get("id", ""),
            "args": block.get("input", {}),
            "preceding_text": preceding_text or None,
            "stop_reason": "tool_use",
        },
    )


# ─────────────────────────────────────────────────────────────────────────────
# Single-event constructors
# ─────────────────────────────────────────────────────────────────────────────


def _make_session_start(
    session_id: str,
    timestamp: str,
    seq: int,
    envelope: Dict[str, Any],
    data: Dict[str, Any],
) -> Dict[str, Any]:
    return _make_event(
        subtype="system.session.start",
        session_id=session_id,
        timestamp=timestamp,
        seq=seq,
        payload={
            "model": envelope.get("model", ""),
            "platform": envelope.get("platform", ""),
            "hermes_version": envelope.get("hermes_version", ""),
            "system_prompt_preview": data.get("system_prompt_preview", ""),
            "tools": data.get("tools", []),
        },
    )


def _make_turn_complete(
    session_id: str, timestamp: str, seq: int, data: Dict[str, Any]
) -> Dict[str, Any]:
    return _make_event(
        subtype="system.turn.complete",
        session_id=session_id,
        timestamp=timestamp,
        seq=seq,
        payload={
            "reason": data.get("reason", "end_turn"),
            "message_count": data.get("message_count", 0),
        },
    )


def _make_user_prompt(
    session_id: str, timestamp: str, seq: int, msg: Dict[str, Any]
) -> Dict[str, Any]:
    return _make_event(
        subtype="message.user.prompt",
        session_id=session_id,
        timestamp=timestamp,
        seq=seq,
        payload={"text": _extract_text(msg)},
    )


def _make_tool_result(
    session_id: str, timestamp: str, seq: int, msg: Dict[str, Any]
) -> Dict[str, Any]:
    # VERIFY: Hermes tool messages — does Hermes use `tool_call_id` or `id`,
    # and does it use `tool_name` or `name`? Both are checked here; trim
    # whichever is wrong once verified.
    return _make_event(
        subtype="message.user.tool_result",
        session_id=session_id,
        timestamp=timestamp,
        seq=seq,
        payload={
            "tool": msg.get("tool_name") or msg.get("name") or "",
            "tool_use_id": msg.get("tool_call_id") or msg.get("id") or "",
            "text": _extract_text(msg),
            "is_error": bool(msg.get("is_error", False)),
        },
    )


def _make_system_injected(
    session_id: str, timestamp: str, seq: int, msg: Dict[str, Any]
) -> Dict[str, Any]:
    # VERIFY: Hermes injects various synthetic system messages (compression
    # summaries, todo snapshots). What field distinguishes them? `subtype`?
    # `name`? Or just substring matching on content? For now we group them all
    # under `system.injected.other`.
    return _make_event(
        subtype="system.injected.other",
        session_id=session_id,
        timestamp=timestamp,
        seq=seq,
        payload={"text": _extract_text(msg)},
    )


# ─────────────────────────────────────────────────────────────────────────────
# CloudEvent envelope
# ─────────────────────────────────────────────────────────────────────────────


def _make_event(
    *,
    subtype: str,
    session_id: str,
    timestamp: str,
    seq: int,
    payload: Dict[str, Any],
) -> Dict[str, Any]:
    """Build a CloudEvent in the shape OpenStory's pipeline expects.

    Mirrors the Claude Code translator's output. The `data` field carries
    OpenStory's internal `EventData` shape (session_id at top level for
    quick filtering, plus an `agent_payload` that's where the subtype-
    specific fields live).
    """
    return {
        "specversion": "1.0",
        "id": _derive_event_id(session_id, seq, subtype),
        "source": f"hermes://{session_id}",
        "type": "io.opentelemetry.observability.agent.event",
        "subtype": subtype,
        "time": timestamp,
        "datacontenttype": "application/json",
        "data": {
            "session_id": session_id,
            "agent_payload": {
                "_variant": _payload_variant_for(subtype),
                **payload,
            },
        },
    }


def _derive_event_id(session_id: str, seq: int, subtype: str) -> str:
    """Deterministic event ID derived from (session, seq, subtype).

    Stable across re-translation passes. Mirrors the property the
    "Synthetic Event ID Stability" backlog item is asking for in the
    Claude Code translator.
    """
    base = f"hermes:{session_id}:{seq}:{subtype}"
    return str(uuid.uuid5(uuid.NAMESPACE_URL, base))


def _payload_variant_for(subtype: str) -> str:
    """Map subtype to OpenStory's `AgentPayload` variant tag.

    The mapping mirrors the Claude Code translator. If OpenStory adds new
    payload variants, this function gets a new branch.
    """
    if subtype.startswith("message.assistant.tool_use"):
        return "AssistantToolUse"
    if subtype.startswith("message.assistant.thinking"):
        return "AssistantThinking"
    if subtype.startswith("message.assistant"):
        return "AssistantText"
    if subtype.startswith("message.user.tool_result"):
        return "UserToolResult"
    if subtype.startswith("message.user"):
        return "UserPrompt"
    if subtype == "system.session.start":
        return "SessionStart"
    if subtype == "system.turn.complete":
        return "TurnComplete"
    return "SystemInjected"


# ─────────────────────────────────────────────────────────────────────────────
# Helpers
# ─────────────────────────────────────────────────────────────────────────────


def _extract_text(msg: Dict[str, Any]) -> str:
    """Pull the textual content out of a Hermes message dict.

    Handles both string `content` and the Anthropic-style content-block list.
    """
    content = msg.get("content")
    if isinstance(content, str):
        return content
    if isinstance(content, list):
        parts = []
        for block in content:
            if isinstance(block, dict) and block.get("type") == "text":
                parts.append(block.get("text", ""))
            elif isinstance(block, str):
                parts.append(block)
        return "\n".join(parts)
    return ""


def _has_anthropic_tool_use(msg: Dict[str, Any]) -> bool:
    content = msg.get("content")
    if not isinstance(content, list):
        return False
    return any(
        isinstance(b, dict) and b.get("type") == "tool_use" for b in content
    )


# ─────────────────────────────────────────────────────────────────────────────
# CLI for ad-hoc inspection
# ─────────────────────────────────────────────────────────────────────────────

if __name__ == "__main__":
    import sys

    if len(sys.argv) != 2:
        print("usage: python translate_hermes.py <hermes_events.jsonl>")
        sys.exit(1)
    events = translate_file(sys.argv[1])
    for ev in events:
        print(json.dumps(ev, indent=2))
