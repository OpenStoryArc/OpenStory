"""
translate_pi_decompose.py — Decomposing translator for pi-mono JSONL.

Takes pi-mono's bundled JSONL (one line per API response, multiple content
blocks per line) and produces one CloudEvent per content block.

This is the Python prototype / executable spec. The Rust port goes into
rs/core/src/translate_pi.rs as an update to the existing translator.

Pure functions, no side effects, no external dependencies.

Usage:
    from translate_pi_decompose import translate_file, translate_line
    events = translate_file("path/to/session.jsonl")
"""

from __future__ import annotations

import json
import uuid
from typing import Any, Dict, List, Optional


# ─── CloudEvent construction ──────────────────────────────────────────


def _derive_event_id(session_id: str, entry_id: str, block_index: int, subtype: str) -> str:
    """Deterministic event ID from source line + block position.

    Uses UUID5 so the same input always produces the same ID.
    The block_index differentiates decomposed events from the same line.
    """
    base = f"pi-mono:{session_id}:{entry_id}:{block_index}:{subtype}"
    return str(uuid.uuid5(uuid.NAMESPACE_URL, base))


def _make_event(
    *,
    subtype: str,
    session_id: str,
    entry_id: str,
    block_index: int,
    timestamp: str,
    source_line: Dict,
    payload: Dict,
) -> Dict:
    """Construct a CloudEvent 1.0 envelope."""
    return {
        "specversion": "1.0",
        "id": _derive_event_id(session_id, entry_id, block_index, subtype),
        "source": f"pi://session/{session_id}",
        "type": "io.arc.event",
        "subtype": subtype,
        "time": timestamp,
        "datacontenttype": "application/json",
        "agent": "pi-mono",
        "data": {
            "session_id": session_id,
            "seq": None,  # filled by caller
            "raw": source_line,  # preserve the original bundled line
            "agent_payload": {
                "_variant": "pi-mono",
                **payload,
            },
        },
    }


# ─── Content block decomposition ──────────────────────────────────────


def _decompose_assistant(
    line: Dict,
    session_id: str,
    entry_id: str,
    timestamp: str,
) -> List[Dict]:
    """Decompose an assistant message into one CloudEvent per content block.

    This is the core of the fix. A pi-mono assistant line with
    content: [thinking, text, toolCall, toolCall] becomes 4 CloudEvents.
    """
    message = line.get("message", {})
    content = message.get("content", [])
    stop_reason = message.get("stopReason")
    usage = message.get("usage")
    model = message.get("model")
    provider = message.get("provider")
    response_id = message.get("responseId")

    if not content:
        return []

    events = []
    for i, block in enumerate(content):
        block_type = block.get("type", "")

        if block_type == "thinking":
            events.append(_make_event(
                subtype="message.assistant.thinking",
                session_id=session_id,
                entry_id=entry_id,
                block_index=i,
                timestamp=timestamp,
                source_line=line,
                payload={
                    "text": block.get("thinking", ""),
                    "thinking_signature": block.get("thinkingSignature"),
                    "model": model,
                    "provider": provider,
                },
            ))

        elif block_type == "text":
            events.append(_make_event(
                subtype="message.assistant.text",
                session_id=session_id,
                entry_id=entry_id,
                block_index=i,
                timestamp=timestamp,
                source_line=line,
                payload={
                    "text": block.get("text", ""),
                    "model": model,
                    "provider": provider,
                    "stop_reason": stop_reason if i == len(content) - 1 else None,
                },
            ))

        elif block_type == "toolCall":
            events.append(_make_event(
                subtype="message.assistant.tool_use",
                session_id=session_id,
                entry_id=entry_id,
                block_index=i,
                timestamp=timestamp,
                source_line=line,
                payload={
                    "tool": block.get("name"),
                    "tool_call_id": block.get("id"),
                    "args": block.get("arguments"),
                    "model": model,
                    "provider": provider,
                    "stop_reason": stop_reason if i == len(content) - 1 else None,
                },
            ))

    # Attach token usage to the LAST event (it belongs to the whole API call)
    if events and usage:
        events[-1]["data"]["agent_payload"]["token_usage"] = usage

    return events


# ─── Line-level translation ───────────────────────────────────────────


def translate_line(line: Dict, session_id: Optional[str] = None) -> List[Dict]:
    """Translate one pi-mono JSONL line into CloudEvent(s).

    Returns 0..N events. Most lines produce 1 event.
    Assistant messages with multiple content blocks produce N events.
    """
    line_type = line.get("type", "")
    entry_id = line.get("id", "")
    timestamp = line.get("timestamp", "")

    # Session header
    if line_type == "session":
        sid = line.get("id", session_id or "")
        return [_make_event(
            subtype="system.session_start",
            session_id=sid,
            entry_id=entry_id or sid,
            block_index=0,
            timestamp=timestamp,
            source_line=line,
            payload={
                "cwd": line.get("cwd"),
                "version": line.get("version"),
                "provider": line.get("provider"),
                "model": line.get("modelId"),
                "thinking_level": line.get("thinkingLevel"),
            },
        )]

    # Model change
    if line_type == "model_change":
        return [_make_event(
            subtype="system.model_change",
            session_id=session_id or "",
            entry_id=entry_id,
            block_index=0,
            timestamp=timestamp,
            source_line=line,
            payload={
                "provider": line.get("provider"),
                "model_id": line.get("modelId"),
            },
        )]

    # Compaction
    if line_type == "compaction":
        return [_make_event(
            subtype="system.compact",
            session_id=session_id or "",
            entry_id=entry_id,
            block_index=0,
            timestamp=timestamp,
            source_line=line,
            payload={
                "summary": line.get("summary"),
                "tokens_before": line.get("tokensBefore"),
                "first_kept_entry_id": line.get("firstKeptEntryId"),
            },
        )]

    # Skip metadata-only types
    if line_type in ("thinking_level_change", "branch_summary", "label",
                     "custom", "custom_message", "session_info"):
        return []

    # Messages
    if line_type == "message":
        message = line.get("message", {})
        role = message.get("role", "")

        if role == "user":
            text = ""
            content = message.get("content", [])
            if isinstance(content, list):
                for block in content:
                    if block.get("type") == "text":
                        text = block.get("text", "")
                        break
            return [_make_event(
                subtype="message.user.prompt",
                session_id=session_id or "",
                entry_id=entry_id,
                block_index=0,
                timestamp=timestamp,
                source_line=line,
                payload={"text": text},
            )]

        if role == "assistant":
            return _decompose_assistant(line, session_id or "", entry_id, timestamp)

        if role == "toolResult":
            text = ""
            content = message.get("content", [])
            if isinstance(content, list):
                for block in content:
                    if block.get("type") == "text":
                        text = block.get("text", "")
                        break
            return [_make_event(
                subtype="message.user.tool_result",
                session_id=session_id or "",
                entry_id=entry_id,
                block_index=0,
                timestamp=timestamp,
                source_line=line,
                payload={
                    "tool_call_id": message.get("toolCallId"),
                    "tool_name": message.get("toolName"),
                    "is_error": message.get("isError", False),
                    "text": text,
                },
            )]

        if role == "bashExecution":
            return [_make_event(
                subtype="progress.bash",
                session_id=session_id or "",
                entry_id=entry_id,
                block_index=0,
                timestamp=timestamp,
                source_line=line,
                payload={
                    "command": message.get("command"),
                    "exit_code": message.get("exitCode"),
                    "output": message.get("output"),
                },
            )]

    # Unknown type — skip
    return []


# ─── File-level translation ───────────────────────────────────────────


def translate_file(path: str) -> List[Dict]:
    """Translate an entire pi-mono JSONL file into CloudEvents."""
    events = []
    session_id = None
    seq = 0

    with open(path) as f:
        for line_str in f:
            line_str = line_str.strip()
            if not line_str:
                continue
            line = json.loads(line_str)

            # Extract session ID from header
            if line.get("type") == "session":
                session_id = line.get("id", "")

            for event in translate_line(line, session_id):
                seq += 1
                event["data"]["seq"] = seq
                events.append(event)

    return events
