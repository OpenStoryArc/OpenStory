#!/usr/bin/env python3
"""Prototype: translate pi-mono JSONL session files to CloudEvents.

Validates the mapping from pi-mono's session format to Open Story's
CloudEvent 1.0 + hierarchical subtype system before implementing in Rust.

Usage:
    uv run python scripts/translate_pi_mono.py <path-to-session.jsonl>
    uv run python scripts/translate_pi_mono.py --test
"""

import argparse
import json
import sys
import uuid
from pathlib import Path
from typing import Any


IO_ARC_EVENT = "io.arc.event"


def detect_format(line: dict) -> str:
    """Detect whether a JSONL line is pi-mono or Claude Code format.

    Pi-mono signals:
      - type: "session" with provider/modelId (header)
      - type: "message" with nested message.role
      - type: "model_change", "compaction", etc.

    Claude Code signals:
      - type: "assistant", "user", "progress", "system"
    """
    entry_type = line.get("type", "")
    if entry_type == "session" and "cwd" in line:
        return "pi_mono"
    if entry_type == "message" and isinstance(line.get("message"), dict):
        return "pi_mono"
    if entry_type in ("model_change", "compaction", "thinking_level_change",
                       "branch_summary", "label", "custom", "custom_message",
                       "session_info"):
        return "pi_mono"
    return "claude_code"


def determine_assistant_subtype(content: list[dict]) -> str:
    """Determine assistant subtype from content blocks."""
    has_thinking = False
    has_tool_call = False
    for block in content:
        bt = block.get("type", "")
        if bt == "toolCall":
            has_tool_call = True
        elif bt == "thinking":
            has_thinking = True
    if has_tool_call:
        return "message.assistant.tool_use"
    if has_thinking:
        return "message.assistant.thinking"
    return "message.assistant.text"


def normalize_content_blocks(content: list[dict]) -> list[dict]:
    """Normalize pi-mono content blocks to Open Story format.

    toolCall -> tool_use, arguments -> input
    """
    normalized = []
    for block in content:
        if block.get("type") == "toolCall":
            normalized.append({
                "type": "tool_use",
                "id": block.get("id", ""),
                "name": block.get("name", "unknown"),
                "input": block.get("arguments", {}),
            })
        else:
            normalized.append(block)
    return normalized


def extract_text(content) -> str | None:
    """Extract text from message content (string or array of blocks)."""
    if isinstance(content, str) and content:
        return content
    if isinstance(content, list):
        for block in content:
            if block.get("type") == "text" and block.get("text"):
                return block["text"]
    return None


def extract_first_tool(content: list[dict]) -> tuple[str, dict] | None:
    """Extract first tool name and input from content blocks."""
    for block in content:
        if block.get("type") == "toolCall":
            return block.get("name", "unknown"), block.get("arguments", {})
    return None


def translate_pi_line(
    line: dict,
    session_id: str,
    seq: int,
    seen_ids: set[str],
) -> list[dict]:
    """Pure function: translate one pi-mono JSONL line to CloudEvent(s).

    Returns empty list for unknown types or duplicate IDs.
    """
    entry_type = line.get("type", "")
    known_types = {
        "session", "message", "compaction", "model_change",
        "thinking_level_change", "branch_summary", "label",
        "custom", "custom_message", "session_info",
    }
    if entry_type not in known_types:
        return []

    # Dedup by entry id (if present)
    entry_id = line.get("id")
    if entry_id:
        if entry_id in seen_ids:
            return []
        seen_ids.add(entry_id)

    source = f"pi://session/{session_id}"
    timestamp = line.get("timestamp")

    # Common envelope
    envelope: dict[str, Any] = {}
    if entry_id:
        envelope["uuid"] = entry_id
    if line.get("parentId") is not None:
        envelope["parent_uuid"] = line["parentId"]
    envelope["session_id"] = session_id

    subtype: str | None = None
    extras: dict[str, Any] = {}

    if entry_type == "session":
        subtype = "system.session_start"
        extras["cwd"] = line.get("cwd", "")
        if line.get("provider"):
            extras["provider"] = line["provider"]
        if line.get("modelId"):
            extras["model"] = line["modelId"]
        if line.get("thinkingLevel"):
            extras["thinking_level"] = line["thinkingLevel"]
        if line.get("version"):
            extras["version"] = line["version"]

    elif entry_type == "message":
        message = line.get("message", {})
        role = message.get("role", "")
        content = message.get("content", [])
        if isinstance(content, str):
            content = [{"type": "text", "text": content}]

        if role == "user":
            subtype = "message.user.prompt"
            text = extract_text(content)
            if text:
                extras["text"] = text

        elif role == "assistant":
            subtype = determine_assistant_subtype(content)
            text = extract_text(content)
            if text:
                extras["text"] = text

            if subtype == "message.assistant.tool_use":
                tool_info = extract_first_tool(content)
                if tool_info:
                    extras["tool"] = tool_info[0]
                    extras["args"] = tool_info[1]

            if message.get("model"):
                extras["model"] = message["model"]
            if message.get("stopReason"):
                extras["stop_reason"] = message["stopReason"]
            if message.get("usage"):
                extras["token_usage"] = message["usage"]
            # Normalize content blocks for raw
            content = normalize_content_blocks(content)

            extras["content_types"] = [b.get("type") for b in content if b.get("type")]

        elif role == "toolResult":
            subtype = "message.user.tool_result"
            extras["tool_call_id"] = message.get("toolCallId", "")
            extras["tool_name"] = message.get("toolName", "")
            extras["is_error"] = message.get("isError", False)

        elif role == "bashExecution":
            subtype = "progress.bash"
            extras["command"] = message.get("command", "")
            extras["exit_code"] = message.get("exitCode")
            extras["output"] = message.get("output", "")

        elif role == "compactionSummary":
            subtype = "system.compact"
            extras["summary"] = message.get("summary", "")

        elif role == "branchSummary":
            # Skip for spike
            return []

        elif role == "custom":
            # Skip for spike
            return []

        else:
            # Unknown role
            return []

        # Normalize message content in raw
        if role == "assistant":
            raw_message = dict(message)
            raw_message["content"] = normalize_content_blocks(
                raw_message.get("content", []) if isinstance(raw_message.get("content"), list) else []
            )
            raw_line = dict(line)
            raw_line["message"] = raw_message
        else:
            raw_line = line

    elif entry_type == "compaction":
        subtype = "system.compact"
        extras["summary"] = line.get("summary", "")
        extras["tokens_before"] = line.get("tokensBefore", 0)
        extras["first_kept_entry_id"] = line.get("firstKeptEntryId", "")
        raw_line = line

    elif entry_type == "model_change":
        subtype = "system.model_change"
        extras["provider"] = line.get("provider", "")
        extras["model"] = line.get("modelId", "")
        raw_line = line

    elif entry_type == "thinking_level_change":
        # Skip for spike
        return []

    elif entry_type in ("branch_summary", "label", "custom",
                         "custom_message", "session_info"):
        # Skip for spike
        return []

    else:
        return []

    if subtype is None:
        return []

    # Build data payload
    data: dict[str, Any] = {}
    if entry_type == "session":
        data["raw"] = line
    elif entry_type == "message":
        data["raw"] = raw_line  # type: ignore[possibly-undefined]
    else:
        data["raw"] = raw_line  # type: ignore[possibly-undefined]
    data["seq"] = seq
    data.update(envelope)
    data.update(extras)

    event_id = entry_id or str(uuid.uuid4())

    cloud_event = {
        "specversion": "1.0",
        "id": event_id,
        "source": source,
        "type": IO_ARC_EVENT,
        "time": timestamp or "",
        "datacontenttype": "application/json",
        "data": data,
        "subtype": subtype,
    }

    return [cloud_event]


def translate_session(filepath: Path) -> list[dict]:
    """Translate a full pi-mono session JSONL file to CloudEvents."""
    session_id = filepath.stem  # filename without extension
    events = []
    seen_ids: set[str] = set()
    seq = 0

    with open(filepath) as f:
        for line_str in f:
            line_str = line_str.strip()
            if not line_str:
                continue
            try:
                line = json.loads(line_str)
            except json.JSONDecodeError:
                continue

            seq += 1
            new_events = translate_pi_line(line, session_id, seq, seen_ids)
            events.extend(new_events)

    return events


# --- Tests ---

def run_tests():
    """Boundary table tests for the translate functions."""
    passed = 0
    failed = 0

    def assert_eq(desc: str, actual, expected):
        nonlocal passed, failed
        if actual == expected:
            passed += 1
        else:
            failed += 1
            print(f"  FAIL: {desc}")
            print(f"    expected: {expected}")
            print(f"    actual:   {actual}")

    seen: set[str] = set()

    # --- Format detection ---
    print("Format detection:")
    assert_eq("session header → pi_mono",
              detect_format({"type": "session", "cwd": "/foo"}), "pi_mono")
    assert_eq("message with role → pi_mono",
              detect_format({"type": "message", "message": {"role": "user"}}), "pi_mono")
    assert_eq("model_change → pi_mono",
              detect_format({"type": "model_change"}), "pi_mono")
    assert_eq("claude assistant → claude_code",
              detect_format({"type": "assistant", "message": {"role": "assistant"}}), "claude_code")
    assert_eq("claude user → claude_code",
              detect_format({"type": "user"}), "claude_code")
    assert_eq("empty → claude_code",
              detect_format({}), "claude_code")

    # --- Session header ---
    print("Session header:")
    header = {
        "type": "session", "id": "sess-1", "timestamp": "2025-01-01T00:00:00Z",
        "cwd": "/work", "provider": "anthropic", "modelId": "claude-sonnet-4-5",
        "thinkingLevel": "off", "version": 3,
    }
    evts = translate_pi_line(header, "test-session", 1, set())
    assert_eq("produces 1 event", len(evts), 1)
    assert_eq("subtype", evts[0]["subtype"], "system.session_start")
    assert_eq("source", evts[0]["source"], "pi://session/test-session")
    assert_eq("provider in data", evts[0]["data"]["provider"], "anthropic")
    assert_eq("model in data", evts[0]["data"]["model"], "claude-sonnet-4-5")

    # --- User message ---
    print("User message:")
    user_msg = {
        "type": "message", "timestamp": "2025-01-01T00:00:01Z",
        "message": {
            "role": "user",
            "content": [{"type": "text", "text": "hello world"}],
            "timestamp": 1234567890,
        },
    }
    evts = translate_pi_line(user_msg, "test-session", 2, set())
    assert_eq("produces 1 event", len(evts), 1)
    assert_eq("subtype", evts[0]["subtype"], "message.user.prompt")
    assert_eq("text extracted", evts[0]["data"]["text"], "hello world")

    # --- Assistant text ---
    print("Assistant text:")
    asst_text = {
        "type": "message", "timestamp": "2025-01-01T00:00:02Z",
        "message": {
            "role": "assistant",
            "content": [{"type": "text", "text": "I can help with that."}],
            "api": "anthropic-messages", "provider": "anthropic",
            "model": "claude-sonnet-4-5",
            "usage": {"input": 100, "output": 50},
            "stopReason": "stop", "timestamp": 1234567891,
        },
    }
    evts = translate_pi_line(asst_text, "test-session", 3, set())
    assert_eq("produces 1 event", len(evts), 1)
    assert_eq("subtype", evts[0]["subtype"], "message.assistant.text")
    assert_eq("text extracted", evts[0]["data"]["text"], "I can help with that.")
    assert_eq("model", evts[0]["data"]["model"], "claude-sonnet-4-5")

    # --- Assistant tool_use ---
    print("Assistant tool_use:")
    asst_tool = {
        "type": "message", "timestamp": "2025-01-01T00:00:03Z",
        "message": {
            "role": "assistant",
            "content": [
                {"type": "text", "text": "Let me read that file."},
                {"type": "toolCall", "id": "tc-1", "name": "read",
                 "arguments": {"path": "/foo/bar.rs"}},
            ],
            "api": "anthropic-messages", "provider": "anthropic",
            "model": "claude-sonnet-4-5",
            "usage": {"input": 100, "output": 50},
            "stopReason": "toolUse", "timestamp": 1234567892,
        },
    }
    evts = translate_pi_line(asst_tool, "test-session", 4, set())
    assert_eq("produces 1 event", len(evts), 1)
    assert_eq("subtype", evts[0]["subtype"], "message.assistant.tool_use")
    assert_eq("tool name", evts[0]["data"]["tool"], "read")
    assert_eq("tool args", evts[0]["data"]["args"], {"path": "/foo/bar.rs"})
    # Verify normalization in raw
    raw_content = evts[0]["data"]["raw"]["message"]["content"]
    assert_eq("toolCall normalized to tool_use in raw",
              raw_content[1]["type"], "tool_use")

    # --- Assistant thinking ---
    print("Assistant thinking:")
    asst_think = {
        "type": "message", "timestamp": "2025-01-01T00:00:04Z",
        "message": {
            "role": "assistant",
            "content": [{"type": "thinking", "thinking": "Let me consider..."}],
            "api": "anthropic-messages", "provider": "anthropic",
            "model": "claude-sonnet-4-5",
            "usage": {"input": 100, "output": 50},
            "stopReason": "stop", "timestamp": 1234567893,
        },
    }
    evts = translate_pi_line(asst_think, "test-session", 5, set())
    assert_eq("produces 1 event", len(evts), 1)
    assert_eq("subtype", evts[0]["subtype"], "message.assistant.thinking")

    # --- Tool result ---
    print("Tool result:")
    tool_result = {
        "type": "message", "timestamp": "2025-01-01T00:00:05Z",
        "message": {
            "role": "toolResult",
            "toolCallId": "tc-1", "toolName": "read",
            "content": [{"type": "text", "text": "file contents here"}],
            "isError": False, "timestamp": 1234567894,
        },
    }
    evts = translate_pi_line(tool_result, "test-session", 6, set())
    assert_eq("produces 1 event", len(evts), 1)
    assert_eq("subtype", evts[0]["subtype"], "message.user.tool_result")
    assert_eq("tool_name", evts[0]["data"]["tool_name"], "read")

    # --- Bash execution ---
    print("Bash execution:")
    bash_msg = {
        "type": "message", "timestamp": "2025-01-01T00:00:06Z",
        "message": {
            "role": "bashExecution",
            "command": "cargo test",
            "output": "test result: ok",
            "exitCode": 0,
            "cancelled": False,
            "truncated": False,
            "timestamp": 1234567895,
        },
    }
    evts = translate_pi_line(bash_msg, "test-session", 7, set())
    assert_eq("produces 1 event", len(evts), 1)
    assert_eq("subtype", evts[0]["subtype"], "progress.bash")
    assert_eq("command", evts[0]["data"]["command"], "cargo test")

    # --- Compaction ---
    print("Compaction:")
    compaction = {
        "type": "compaction", "id": "comp-1", "parentId": "msg-5",
        "timestamp": "2025-01-01T00:00:07Z",
        "summary": "Refactored auth module", "firstKeptEntryId": "msg-3",
        "tokensBefore": 50000,
    }
    evts = translate_pi_line(compaction, "test-session", 8, set())
    assert_eq("produces 1 event", len(evts), 1)
    assert_eq("subtype", evts[0]["subtype"], "system.compact")
    assert_eq("summary", evts[0]["data"]["summary"], "Refactored auth module")

    # --- Model change ---
    print("Model change:")
    model_change = {
        "type": "model_change", "timestamp": "2025-01-01T00:00:08Z",
        "provider": "openai", "modelId": "gpt-4o",
    }
    evts = translate_pi_line(model_change, "test-session", 9, set())
    assert_eq("produces 1 event", len(evts), 1)
    assert_eq("subtype", evts[0]["subtype"], "system.model_change")
    assert_eq("provider", evts[0]["data"]["provider"], "openai")
    assert_eq("model", evts[0]["data"]["model"], "gpt-4o")

    # --- Unknown type ---
    print("Unknown type:")
    evts = translate_pi_line({"type": "foobar"}, "test-session", 10, set())
    assert_eq("produces 0 events", len(evts), 0)

    # --- Duplicate id ---
    print("Duplicate id:")
    seen_dedup: set[str] = set()
    evts1 = translate_pi_line(
        {"type": "compaction", "id": "dup-1", "timestamp": "2025-01-01T00:00:09Z",
         "summary": "first", "firstKeptEntryId": "a", "tokensBefore": 100},
        "test-session", 11, seen_dedup,
    )
    evts2 = translate_pi_line(
        {"type": "compaction", "id": "dup-1", "timestamp": "2025-01-01T00:00:10Z",
         "summary": "duplicate", "firstKeptEntryId": "a", "tokensBefore": 100},
        "test-session", 12, seen_dedup,
    )
    assert_eq("first produces 1 event", len(evts1), 1)
    assert_eq("duplicate produces 0 events", len(evts2), 0)

    # --- Skipped types ---
    print("Skipped types:")
    for skip_type in ("thinking_level_change", "branch_summary", "label", "custom"):
        evts = translate_pi_line({"type": skip_type}, "test-session", 13, set())
        assert_eq(f"{skip_type} → 0 events", len(evts), 0)

    # --- Summary ---
    print(f"\n{'=' * 40}")
    print(f"  {passed} passed, {failed} failed")
    if failed:
        print("  SOME TESTS FAILED")
        sys.exit(1)
    else:
        print("  ALL TESTS PASSED")


def main():
    parser = argparse.ArgumentParser(description="Translate pi-mono JSONL to CloudEvents")
    parser.add_argument("filepath", nargs="?", help="Path to pi-mono session JSONL file")
    parser.add_argument("--test", action="store_true", help="Run boundary table tests")
    parser.add_argument("--stats", action="store_true", help="Print subtype distribution")
    args = parser.parse_args()

    if args.test:
        run_tests()
        return

    if not args.filepath:
        parser.error("filepath is required (or use --test)")

    path = Path(args.filepath)
    if not path.exists():
        print(f"File not found: {path}", file=sys.stderr)
        sys.exit(1)

    events = translate_session(path)

    if args.stats:
        from collections import Counter
        subtypes = Counter(e["subtype"] for e in events)
        print(f"Total events: {len(events)}")
        print(f"Subtypes:")
        for st, count in subtypes.most_common():
            print(f"  {st}: {count}")
    else:
        for event in events:
            print(json.dumps(event))


if __name__ == "__main__":
    main()
