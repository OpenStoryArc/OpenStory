"""
test_translate.py — Asserts the Hermes → CloudEvent translator preserves
the structural shape OpenStory's pipeline downstream relies on.

Run:
    cd docs/research/hermes-integration
    python test_translate.py

No external dependencies. Uses plain assert + a tiny test runner so the
prototype directory stays self-contained.
"""

from __future__ import annotations

import json
import os
import sys
import traceback
from typing import Any, Callable, Dict, List, Tuple

# Make `translate_hermes` importable when run from this directory.
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from translate_hermes import translate_file, translate_event  # noqa: E402


HERE = os.path.dirname(os.path.abspath(__file__))
EXAMPLE = os.path.join(HERE, "example_hermes_events.jsonl")


# ─────────────────────────────────────────────────────────────────────────────
# Tiny test runner
# ─────────────────────────────────────────────────────────────────────────────


_TESTS: List[Tuple[str, Callable[[], None]]] = []


def test(fn: Callable[[], None]) -> Callable[[], None]:
    _TESTS.append((fn.__name__, fn))
    return fn


def run_all() -> int:
    passed = 0
    failed = 0
    for name, fn in _TESTS:
        try:
            fn()
            print(f"  PASS  {name}")
            passed += 1
        except AssertionError as e:
            print(f"  FAIL  {name}: {e}")
            traceback.print_exc()
            failed += 1
        except Exception as e:
            print(f"  ERROR {name}: {e}")
            traceback.print_exc()
            failed += 1
    print()
    print(f"  {passed} passed, {failed} failed")
    return 0 if failed == 0 else 1


# ─────────────────────────────────────────────────────────────────────────────
# Tests against the example file
# ─────────────────────────────────────────────────────────────────────────────


@test
def test_example_file_translates_without_error():
    events = translate_file(EXAMPLE)
    assert len(events) > 0, "expected at least one CloudEvent from the example"


@test
def test_event_count_matches_expected_shape():
    events = translate_file(EXAMPLE)
    # The example session has:
    #   1 session_start
    #   1 user prompt
    #   1 assistant turn with thinking + 1 tool call → 2 events (thinking + tool_use)
    #   1 tool result
    #   1 final assistant text-only message
    #   1 session_end (turn complete)
    # That's 7 CloudEvents from 6 input lines.
    assert len(events) == 7, f"expected 7 CloudEvents, got {len(events)}"


@test
def test_subtypes_are_in_expected_order():
    events = translate_file(EXAMPLE)
    subtypes = [ev["subtype"] for ev in events]
    expected = [
        "system.session.start",
        "message.user.prompt",
        "message.assistant.thinking",
        "message.assistant.tool_use",
        "message.user.tool_result",
        "message.assistant.text",
        "system.turn.complete",
    ]
    assert subtypes == expected, f"subtype sequence mismatch:\n  got: {subtypes}\n  exp: {expected}"


@test
def test_cloudevent_envelope_fields_present():
    events = translate_file(EXAMPLE)
    for ev in events:
        for required in ("specversion", "id", "source", "type", "subtype", "time", "data"):
            assert required in ev, f"missing required field {required} in {ev}"
        assert ev["specversion"] == "1.0"
        assert ev["source"].startswith("hermes://")


@test
def test_session_id_is_threaded_through_data():
    events = translate_file(EXAMPLE)
    for ev in events:
        assert ev["data"]["session_id"] == "hermes-test-session-001"


@test
def test_event_ids_are_deterministic():
    events1 = translate_file(EXAMPLE)
    events2 = translate_file(EXAMPLE)
    ids1 = [ev["id"] for ev in events1]
    ids2 = [ev["id"] for ev in events2]
    assert ids1 == ids2, "event IDs are not stable across translation passes"


@test
def test_event_ids_are_unique():
    events = translate_file(EXAMPLE)
    ids = [ev["id"] for ev in events]
    assert len(ids) == len(set(ids)), f"duplicate event IDs: {ids}"


@test
def test_assistant_tool_use_payload_carries_args():
    events = translate_file(EXAMPLE)
    tool_use_events = [ev for ev in events if ev["subtype"] == "message.assistant.tool_use"]
    assert len(tool_use_events) == 1
    payload = tool_use_events[0]["data"]["agent_payload"]
    assert payload["tool"] == "Read"
    assert payload["args"] == {"file_path": "/repo/README.md"}
    assert payload["tool_use_id"] == "toolu_01abc"


@test
def test_tool_result_links_to_tool_use_id():
    events = translate_file(EXAMPLE)
    tool_use = next(ev for ev in events if ev["subtype"] == "message.assistant.tool_use")
    tool_result = next(ev for ev in events if ev["subtype"] == "message.user.tool_result")
    assert (
        tool_result["data"]["agent_payload"]["tool_use_id"]
        == tool_use["data"]["agent_payload"]["tool_use_id"]
    ), "tool result should reference the tool_use_id"


@test
def test_thinking_text_is_preserved():
    events = translate_file(EXAMPLE)
    thinking = next(ev for ev in events if ev["subtype"] == "message.assistant.thinking")
    text = thinking["data"]["agent_payload"]["text"]
    assert "user wants" in text or "README" in text


@test
def test_terminal_assistant_message_uses_text_subtype():
    events = translate_file(EXAMPLE)
    text_events = [ev for ev in events if ev["subtype"] == "message.assistant.text"]
    assert len(text_events) == 1
    payload = text_events[0]["data"]["agent_payload"]
    assert "Open Story" in payload["text"]
    assert payload["stop_reason"] in ("end_turn", "stop")


@test
def test_unknown_event_type_is_dropped_silently():
    raw = {
        "envelope": {"session_id": "x", "event_seq": 99, "timestamp": "2026-04-08T00:00:00Z", "source": "hermes"},
        "event_type": "future_unknown_event",
        "data": {},
    }
    out = translate_event(raw)
    assert out == [], "unknown event types should be dropped, not crash"


# ─────────────────────────────────────────────────────────────────────────────
# Tests added in Phase 2 from SOURCE_VERIFICATION.md findings.
#
# These exercise shapes that came directly from reading hermes-agent source
# (commit 6e3f7f36 on 2026-04-08). They are still synthetic — no real Hermes
# session has been captured — but they encode the verified ground truth from
# tests/agent/test_anthropic_adapter.py:575 and run_agent.py:2408-2472 so the
# translator's behavior matches what real Hermes data WOULD produce.
# ─────────────────────────────────────────────────────────────────────────────


def _hermes_message_event(seq, msg):
    """Helper to wrap a raw Hermes message dict in the input envelope."""
    return {
        "envelope": {
            "session_id": "verify-001",
            "event_seq": seq,
            "timestamp": "2026-04-08T14:00:00Z",
            "source": "hermes",
        },
        "event_type": "message",
        "data": msg,
    }


@test
def test_canonical_openai_tool_call_shape_from_hermes_test_fixture():
    """The exact assistant tool-call shape verified from
    tests/agent/test_anthropic_adapter.py:575 in hermes-agent."""
    raw = _hermes_message_event(
        1,
        {
            "role": "assistant",
            "content": "Let me search.",
            "tool_calls": [
                {
                    "id": "tc_1",
                    "function": {
                        "name": "search",
                        "arguments": '{"query": "test"}',  # JSON STRING
                    },
                }
            ],
        },
    )
    out = translate_event(raw)
    # Should produce ONE tool_use CloudEvent (text+tool together = preceding_text)
    tool_use = [e for e in out if e["subtype"] == "message.assistant.tool_use"]
    assert len(tool_use) == 1, f"expected 1 tool_use, got {len(tool_use)}"
    payload = tool_use[0]["data"]["agent_payload"]
    assert payload["tool"] == "search"
    assert payload["tool_use_id"] == "tc_1"
    # arguments string should be parsed into a dict
    assert payload["args"] == {"query": "test"}, "JSON string args must be parsed"
    assert payload["preceding_text"] == "Let me search."


@test
def test_canonical_tool_result_shape_from_hermes_test_fixture():
    """Tool result shape verified from tests/agent/test_anthropic_adapter.py:590.
    Note: no `tool_name`, no `is_error`, plain string content."""
    raw = _hermes_message_event(
        1,
        {
            "role": "tool",
            "tool_call_id": "tc_1",
            "content": "search results",
        },
    )
    out = translate_event(raw)
    assert len(out) == 1
    payload = out[0]["data"]["agent_payload"]
    assert out[0]["subtype"] == "message.user.tool_result"
    assert payload["tool_use_id"] == "tc_1"
    assert payload["text"] == "search results"
    # tool_name is optional and absent in the canonical fixture
    assert payload["tool"] == ""
    assert payload["is_error"] is False


@test
def test_tool_result_with_optional_tool_name_field():
    """tool_name is optional but legal — verified from
    gateway/session.py:957 which reads message.get('tool_name')."""
    raw = _hermes_message_event(
        1,
        {
            "role": "tool",
            "tool_call_id": "tc_2",
            "tool_name": "Read",
            "content": "file contents...",
        },
    )
    out = translate_event(raw)
    payload = out[0]["data"]["agent_payload"]
    assert payload["tool"] == "Read"


@test
def test_orphaned_tool_result_does_not_crash():
    """Context compression can leave a tool_result without its preceding
    tool_use (verified from tests/agent/test_anthropic_adapter.py:651).
    The translator must handle this gracefully — it just emits the
    tool_result CloudEvent on its own and OpenStory's pipeline handles
    the dangling reference."""
    raw = _hermes_message_event(
        1,
        {
            "role": "tool",
            "tool_call_id": "tc_orphan",
            "content": "stale result from compressed-out tool use",
        },
    )
    out = translate_event(raw)
    assert len(out) == 1
    assert out[0]["subtype"] == "message.user.tool_result"
    assert out[0]["data"]["agent_payload"]["tool_use_id"] == "tc_orphan"


@test
def test_assistant_message_with_empty_content_and_tool_calls():
    """When the assistant emits only tool calls, content is empty string
    (not None). Verified from tests/agent/test_anthropic_adapter.py:603."""
    raw = _hermes_message_event(
        1,
        {
            "role": "assistant",
            "content": "",
            "tool_calls": [
                {"id": "tc_a", "function": {"name": "tool_a", "arguments": "{}"}},
            ],
        },
    )
    out = translate_event(raw)
    tool_use = [e for e in out if e["subtype"] == "message.assistant.tool_use"]
    text = [e for e in out if e["subtype"] == "message.assistant.text"]
    assert len(tool_use) == 1
    # Empty content + tool calls = NO text event emitted (preceding_text is None)
    assert len(text) == 0
    assert tool_use[0]["data"]["agent_payload"]["preceding_text"] is None


@test
def test_multiple_tool_calls_in_one_assistant_message():
    """Verified from tests/agent/test_anthropic_adapter.py:621
    (test_merges_consecutive_tool_results) — Hermes can issue multiple
    tool calls in one turn. The translator should fan out one
    CloudEvent per tool call so OpenStory's pattern detector sees them
    as parallel apply phases."""
    raw = _hermes_message_event(
        1,
        {
            "role": "assistant",
            "content": "",
            "tool_calls": [
                {"id": "tc_1", "function": {"name": "tool_a", "arguments": "{}"}},
                {"id": "tc_2", "function": {"name": "tool_b", "arguments": "{}"}},
            ],
        },
    )
    out = translate_event(raw)
    tool_use = [e for e in out if e["subtype"] == "message.assistant.tool_use"]
    assert len(tool_use) == 2
    assert {tu["data"]["agent_payload"]["tool"] for tu in tool_use} == {"tool_a", "tool_b"}


@test
def test_assistant_message_with_reasoning_and_tool_call():
    """The full canonical shape: reasoning + content + tool_calls all in
    one assistant message. Should produce: thinking event + tool_use event.
    No standalone text event (the content is preceding_text on the tool_use)."""
    raw = _hermes_message_event(
        1,
        {
            "role": "assistant",
            "content": "I'll search for that.",
            "reasoning": "The user wants information; search is the right tool.",
            "tool_calls": [
                {"id": "tc_1", "function": {"name": "search", "arguments": '{"q": "x"}'}},
            ],
            "finish_reason": "tool_calls",
        },
    )
    out = translate_event(raw)
    subtypes = [e["subtype"] for e in out]
    assert subtypes == ["message.assistant.thinking", "message.assistant.tool_use"]
    thinking = out[0]["data"]["agent_payload"]
    assert "user wants information" in thinking["text"]
    tool_use = out[1]["data"]["agent_payload"]
    assert tool_use["preceding_text"] == "I'll search for that."


@test
def test_assistant_text_only_message_carries_finish_reason():
    """Pure text response (no tool calls). Should emit one text event
    with the finish_reason in the payload."""
    raw = _hermes_message_event(
        1,
        {
            "role": "assistant",
            "content": "Here's the answer.",
            "finish_reason": "stop",
        },
    )
    out = translate_event(raw)
    assert len(out) == 1
    assert out[0]["subtype"] == "message.assistant.text"
    assert out[0]["data"]["agent_payload"]["stop_reason"] == "stop"


@test
def test_system_injected_message_maps_to_system_injected_other():
    """Compression summaries / todo snapshots are plain system messages
    (verified §4.5 of SOURCE_VERIFICATION.md). They have no special
    field, so they all map to `system.injected.other`."""
    raw = _hermes_message_event(
        1,
        {
            "role": "system",
            "content": "[Compressed history follows] Earlier in the session...",
        },
    )
    out = translate_event(raw)
    assert len(out) == 1
    assert out[0]["subtype"] == "system.injected.other"


@test
def test_unknown_role_is_dropped_silently():
    """Forward-compat: a future role we don't recognize must not crash."""
    raw = _hermes_message_event(
        1,
        {"role": "future_role", "content": "..."},
    )
    out = translate_event(raw)
    assert out == []


# ─────────────────────────────────────────────────────────────────────────────


if __name__ == "__main__":
    print("translate_hermes.py — example translation tests")
    print()
    sys.exit(run_all())
