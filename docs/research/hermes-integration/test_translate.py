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


if __name__ == "__main__":
    print("translate_hermes.py — example translation tests")
    print()
    sys.exit(run_all())
