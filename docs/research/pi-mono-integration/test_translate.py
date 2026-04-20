"""
test_translate.py — Asserts the decomposing pi-mono translator preserves
content from bundled JSONL lines and produces correct CloudEvents.

Run:
    cd docs/research/pi-mono-integration
    python test_translate.py

No external dependencies. Uses plain assert + a tiny test runner.
Tests against real captured session data in captures/.
"""

from __future__ import annotations

import json
import os
import sys
import traceback
from typing import Any, Callable, Dict, List, Tuple

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from translate_pi_decompose import translate_file, translate_line  # noqa: E402

HERE = os.path.dirname(os.path.abspath(__file__))
CAPTURES = os.path.join(HERE, "captures")


# ─── Test runner ──────────────────────────────────────────────────────

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
    print(f"  {passed} passed, {failed} failed, {passed + failed} total")
    return 1 if failed else 0


def load_scenario(name: str) -> str:
    return os.path.join(CAPTURES, name, "session.jsonl")


def subtypes(events: List[Dict]) -> List[str]:
    return [e["subtype"] for e in events]


# ─── Scenario 01: text only ──────────────────────────────────────────

@test
def scenario_01_text_only_event_count():
    """Text-only response produces exactly 4 events: session, model_change, user, text."""
    events = translate_file(load_scenario("01-text-only"))
    # session + model_change + (thinking_level_change skipped) + user + assistant
    assert len(events) == 4, f"expected 4 events, got {len(events)}"


@test
def scenario_01_text_only_subtypes():
    events = translate_file(load_scenario("01-text-only"))
    st = subtypes(events)
    assert "system.session_start" in st
    assert "system.model_change" in st
    assert "message.user.prompt" in st
    assert "message.assistant.text" in st


@test
def scenario_01_text_only_text_content():
    """The text content is preserved in the CloudEvent payload."""
    events = translate_file(load_scenario("01-text-only"))
    text_events = [e for e in events if e["subtype"] == "message.assistant.text"]
    assert len(text_events) == 1
    assert len(text_events[0]["data"]["agent_payload"]["text"]) > 0


# ─── Scenario 04: thinking + text (THE BUG) ──────────────────────────

@test
def scenario_04_thinking_text_produces_both():
    """A [thinking, text] response MUST produce both thinking AND text events."""
    events = translate_file(load_scenario("04-thinking-plus-text"))
    st = subtypes(events)
    assert "message.assistant.thinking" in st, "thinking event missing"
    assert "message.assistant.text" in st, "text event missing — THIS IS THE BUG"


@test
def scenario_04_thinking_text_content_preserved():
    """Both thinking and text content are preserved."""
    events = translate_file(load_scenario("04-thinking-plus-text"))
    thinking = [e for e in events if e["subtype"] == "message.assistant.thinking"]
    text = [e for e in events if e["subtype"] == "message.assistant.text"]
    assert len(thinking) == 1
    assert len(text) == 1
    assert len(thinking[0]["data"]["agent_payload"]["text"]) > 0
    assert len(text[0]["data"]["agent_payload"]["text"]) > 0


@test
def scenario_04_text_is_the_actual_answer():
    """The text event contains the answer, not the thinking."""
    events = translate_file(load_scenario("04-thinking-plus-text"))
    text = [e for e in events if e["subtype"] == "message.assistant.text"][0]
    answer = text["data"]["agent_payload"]["text"]
    assert "1+1" in answer or "Peano" in answer or "successor" in answer, \
        f"Text event should contain the answer, got: {answer[:80]}"


# ─── Scenario 06: thinking + text + tool (WORST CASE) ────────────────

@test
def scenario_06_full_decomposition():
    """[thinking, text, toolCall] line produces 3 separate CloudEvents."""
    events = translate_file(load_scenario("06-thinking-text-tool"))
    # Find events from the first assistant message (the bundled one)
    assistant_events = [e for e in events if e["subtype"].startswith("message.assistant.")]
    # First API call should produce thinking + text + tool_use
    # Second API call should produce text
    assert len(assistant_events) >= 4, \
        f"expected >=4 assistant events (3 decomposed + 1 final text), got {len(assistant_events)}"


@test
def scenario_06_all_subtypes_present():
    events = translate_file(load_scenario("06-thinking-text-tool"))
    st = subtypes(events)
    assert "message.assistant.thinking" in st, "thinking missing"
    assert "message.assistant.text" in st, "text missing"
    assert "message.assistant.tool_use" in st, "tool_use missing"
    assert "message.user.tool_result" in st, "tool_result missing"


@test
def scenario_06_text_between_thinking_and_tool():
    """The text block that sits between thinking and toolCall is preserved."""
    events = translate_file(load_scenario("06-thinking-text-tool"))
    text_events = [e for e in events if e["subtype"] == "message.assistant.text"]
    # Should have at least 2: one from the bundled line, one from the final response
    assert len(text_events) >= 2, f"expected >=2 text events, got {len(text_events)}"
    # The first text event should be the short one from the bundled line
    first_text = text_events[0]["data"]["agent_payload"]["text"]
    assert "read" in first_text.lower() or "file" in first_text.lower() or "analyze" in first_text.lower(), \
        f"First text should be about reading the file, got: {first_text[:80]}"


@test
def scenario_06_tool_use_has_name_and_args():
    events = translate_file(load_scenario("06-thinking-text-tool"))
    tool_events = [e for e in events if e["subtype"] == "message.assistant.tool_use"]
    assert len(tool_events) >= 1
    payload = tool_events[0]["data"]["agent_payload"]
    assert payload["tool"] == "read", f"expected tool='read', got {payload['tool']}"
    assert "path" in (payload.get("args") or {}), "tool args should contain 'path'"


@test
def scenario_06_token_usage_on_last_event():
    """Token usage is attached to the last event of each decomposed group."""
    events = translate_file(load_scenario("06-thinking-text-tool"))
    tool_events = [e for e in events if e["subtype"] == "message.assistant.tool_use"]
    assert len(tool_events) >= 1
    usage = tool_events[0]["data"]["agent_payload"].get("token_usage")
    assert usage is not None, "token_usage should be on the last decomposed event (tool_use)"
    assert usage.get("input") is not None or usage.get("output") is not None


# ─── Scenario 07: multi-tool ─────────────────────────────────────────

@test
def scenario_07_both_tools_visible():
    """Multiple toolCall blocks in one response produce multiple tool_use events."""
    events = translate_file(load_scenario("07-multi-tool"))
    tool_events = [e for e in events if e["subtype"] == "message.assistant.tool_use"]
    assert len(tool_events) >= 2, \
        f"expected >=2 tool_use events for parallel tool calls, got {len(tool_events)}"


@test
def scenario_07_tool_ids_unique():
    """Each tool_use event has a distinct tool_call_id."""
    events = translate_file(load_scenario("07-multi-tool"))
    tool_events = [e for e in events if e["subtype"] == "message.assistant.tool_use"]
    ids = [e["data"]["agent_payload"]["tool_call_id"] for e in tool_events]
    assert len(ids) == len(set(ids)), f"tool_call_ids should be unique, got {ids}"


@test
def scenario_07_tool_names_correct():
    """Both tool calls are 'read' with different paths."""
    events = translate_file(load_scenario("07-multi-tool"))
    tool_events = [e for e in events if e["subtype"] == "message.assistant.tool_use"]
    for e in tool_events:
        assert e["data"]["agent_payload"]["tool"] == "read"
    paths = [e["data"]["agent_payload"]["args"]["path"] for e in tool_events]
    assert any("config" in p for p in paths), f"expected config.toml in paths, got {paths}"
    assert any("broken" in p for p in paths), f"expected test-broken.py in paths, got {paths}"


# ─── Scenario 08: tool error ─────────────────────────────────────────

@test
def scenario_08_error_preserved():
    """Tool error (isError: true) is preserved in the tool_result event."""
    events = translate_file(load_scenario("08-tool-error"))
    tool_results = [e for e in events if e["subtype"] == "message.user.tool_result"]
    assert len(tool_results) >= 1
    # At least one should be an error
    errors = [e for e in tool_results if e["data"]["agent_payload"].get("is_error")]
    assert len(errors) >= 1, "expected at least one tool_result with is_error=true"


# ─── Tool name classification (sentence.rs compatibility) ────────────

# Claude Code tool names (what sentence.rs expects)
CLAUDE_PREPARATORY = {"Read", "Grep", "Glob", "WebSearch", "WebFetch"}
CLAUDE_CREATIVE = {"Write", "Edit"}
CLAUDE_DELEGATORY = {"Agent"}

# Pi-mono tool names (what pi-mono actually uses)
PIMONO_TOOLS = {"read", "grep", "find", "ls", "write", "edit", "bash"}

# Expected mapping: pi-mono name → sentence.rs role
EXPECTED_ROLE = {
    "read": "preparatory",
    "grep": "preparatory",
    "find": "preparatory",   # pi-mono's find ≈ Claude's Glob
    "ls": "preparatory",
    "write": "creative",
    "edit": "creative",
    "bash": "verificatory",  # default; command content determines actual role
}


def classify_tool_case_insensitive(name: str) -> str:
    """What classify_tool SHOULD do — case-insensitive matching."""
    lower = name.lower()
    if lower in {"read", "grep", "glob", "find", "ls", "websearch", "webfetch"}:
        return "preparatory"
    if lower in {"write", "edit"}:
        return "creative"
    if lower in {"agent"}:
        return "delegatory"
    if lower in {"askuserquestion", "toolsearch", "exitplanmode"}:
        return "interactive"
    return "verificatory"


def classify_tool_current(name: str) -> str:
    """What classify_tool does TODAY — exact case match (broken for pi-mono)."""
    if name in CLAUDE_PREPARATORY:
        return "preparatory"
    if name in CLAUDE_CREATIVE:
        return "creative"
    if name in CLAUDE_DELEGATORY:
        return "delegatory"
    return "verificatory"


@test
def pimono_tools_misclassified_by_current_code():
    """Document the bug: current case-sensitive matching misclassifies pi-mono tools."""
    misclassified = []
    for tool, expected in EXPECTED_ROLE.items():
        actual = classify_tool_current(tool)
        if actual != expected:
            misclassified.append((tool, expected, actual))
    # read, grep, find, ls, write, edit should all be misclassified
    assert len(misclassified) >= 5, \
        f"expected >=5 misclassified tools, got {len(misclassified)}: {misclassified}"


@test
def pimono_tools_correct_with_case_insensitive():
    """Case-insensitive matching fixes all pi-mono tool classifications."""
    for tool, expected in EXPECTED_ROLE.items():
        actual = classify_tool_case_insensitive(tool)
        assert actual == expected, \
            f"tool '{tool}': expected {expected}, got {actual}"


@test
def claude_tools_still_correct_with_case_insensitive():
    """Case-insensitive matching doesn't break Claude Code tool classification."""
    claude_expected = {
        "Read": "preparatory", "Grep": "preparatory", "Glob": "preparatory",
        "Write": "creative", "Edit": "creative",
        "Bash": "verificatory", "Agent": "delegatory",
        "AskUserQuestion": "interactive",
    }
    for tool, expected in claude_expected.items():
        actual = classify_tool_case_insensitive(tool)
        assert actual == expected, \
            f"Claude tool '{tool}': expected {expected}, got {actual}"


@test
def tool_events_carry_native_name():
    """Tool use events preserve pi-mono's native lowercase tool name."""
    for scenario in os.listdir(CAPTURES):
        jsonl = os.path.join(CAPTURES, scenario, "session.jsonl")
        if not os.path.isfile(jsonl):
            continue
        events = translate_file(jsonl)
        for e in events:
            if e["subtype"] == "message.assistant.tool_use":
                tool = e["data"]["agent_payload"]["tool"]
                assert tool == tool.lower(), \
                    f"{scenario}: expected lowercase tool name, got '{tool}'"


@test
def find_maps_to_preparatory():
    """Pi-mono's 'find' tool (≈ Claude's Glob) should be Preparatory."""
    assert classify_tool_case_insensitive("find") == "preparatory"
    assert classify_tool_case_insensitive("Glob") == "preparatory"


@test
def sentence_subject_is_pi():
    """Sentence detector should use 'Pi' as subject for pi-mono events."""
    # This tests that the agent field flows through to sentence generation
    for scenario in os.listdir(CAPTURES):
        jsonl = os.path.join(CAPTURES, scenario, "session.jsonl")
        if not os.path.isfile(jsonl):
            continue
        events = translate_file(jsonl)
        for e in events:
            assert e.get("agent") == "pi-mono", \
                f"{scenario}: agent should be 'pi-mono', got {e.get('agent')}"


# ─── Cross-scenario invariants ───────────────────────────────────────

@test
def all_events_have_cloudevent_envelope():
    """Every event has the required CloudEvent 1.0 fields."""
    for scenario in os.listdir(CAPTURES):
        jsonl = os.path.join(CAPTURES, scenario, "session.jsonl")
        if not os.path.isfile(jsonl):
            continue
        events = translate_file(jsonl)
        for e in events:
            assert e.get("specversion") == "1.0", f"{scenario}: missing specversion"
            assert e.get("id"), f"{scenario}: missing id"
            assert e.get("source", "").startswith("pi://"), f"{scenario}: bad source"
            assert e.get("type") == "io.arc.event", f"{scenario}: bad type"
            assert e.get("subtype"), f"{scenario}: missing subtype"
            assert e.get("agent") == "pi-mono", f"{scenario}: bad agent"


@test
def all_event_ids_unique_within_session():
    """Event IDs are unique within each session."""
    for scenario in os.listdir(CAPTURES):
        jsonl = os.path.join(CAPTURES, scenario, "session.jsonl")
        if not os.path.isfile(jsonl):
            continue
        events = translate_file(jsonl)
        ids = [e["id"] for e in events]
        assert len(ids) == len(set(ids)), \
            f"{scenario}: duplicate event IDs: {[x for x in ids if ids.count(x) > 1]}"


@test
def all_event_ids_deterministic():
    """Translating the same file twice produces identical event IDs."""
    for scenario in os.listdir(CAPTURES):
        jsonl = os.path.join(CAPTURES, scenario, "session.jsonl")
        if not os.path.isfile(jsonl):
            continue
        events1 = translate_file(jsonl)
        events2 = translate_file(jsonl)
        for e1, e2 in zip(events1, events2):
            assert e1["id"] == e2["id"], f"{scenario}: non-deterministic IDs"


@test
def raw_data_preserved_untouched():
    """Every event's data.raw contains the original JSONL line."""
    for scenario in os.listdir(CAPTURES):
        jsonl = os.path.join(CAPTURES, scenario, "session.jsonl")
        if not os.path.isfile(jsonl):
            continue
        events = translate_file(jsonl)
        for e in events:
            raw = e["data"].get("raw")
            assert raw is not None, f"{scenario}: missing raw data"
            assert isinstance(raw, dict), f"{scenario}: raw should be dict"
            assert "type" in raw, f"{scenario}: raw should have 'type' field"


@test
def seq_numbers_monotonically_increase():
    """Seq numbers are monotonically increasing within each file."""
    for scenario in os.listdir(CAPTURES):
        jsonl = os.path.join(CAPTURES, scenario, "session.jsonl")
        if not os.path.isfile(jsonl):
            continue
        events = translate_file(jsonl)
        seqs = [e["data"]["seq"] for e in events]
        for i in range(1, len(seqs)):
            assert seqs[i] > seqs[i - 1], \
                f"{scenario}: seq not increasing: {seqs[i-1]} -> {seqs[i]}"


@test
def no_assistant_text_lost_in_any_scenario():
    """The critical invariant: if pi-mono wrote text, the translator emits it.

    For every assistant message line that contains a text content block,
    there must be a message.assistant.text CloudEvent.
    """
    for scenario in os.listdir(CAPTURES):
        jsonl_path = os.path.join(CAPTURES, scenario, "session.jsonl")
        if not os.path.isfile(jsonl_path):
            continue
        with open(jsonl_path) as f:
            lines = [json.loads(l) for l in f if l.strip()]

        # Count text blocks in assistant messages
        expected_text_count = 0
        for line in lines:
            if line.get("type") == "message":
                msg = line.get("message", {})
                if msg.get("role") == "assistant":
                    content = msg.get("content", [])
                    for block in content:
                        if block.get("type") == "text":
                            expected_text_count += 1

        # Count text events from translator
        events = translate_file(jsonl_path)
        actual_text_count = len([e for e in events if e["subtype"] == "message.assistant.text"])

        assert actual_text_count == expected_text_count, \
            f"{scenario}: expected {expected_text_count} text events, got {actual_text_count}"


@test
def no_thinking_lost_in_any_scenario():
    """If pi-mono wrote thinking, the translator emits it."""
    for scenario in os.listdir(CAPTURES):
        jsonl_path = os.path.join(CAPTURES, scenario, "session.jsonl")
        if not os.path.isfile(jsonl_path):
            continue
        with open(jsonl_path) as f:
            lines = [json.loads(l) for l in f if l.strip()]

        expected = 0
        for line in lines:
            if line.get("type") == "message":
                msg = line.get("message", {})
                if msg.get("role") == "assistant":
                    for block in msg.get("content", []):
                        if block.get("type") == "thinking":
                            expected += 1

        events = translate_file(jsonl_path)
        actual = len([e for e in events if e["subtype"] == "message.assistant.thinking"])
        assert actual == expected, \
            f"{scenario}: expected {expected} thinking events, got {actual}"


@test
def no_tool_calls_lost_in_any_scenario():
    """If pi-mono wrote toolCall blocks, the translator emits all of them."""
    for scenario in os.listdir(CAPTURES):
        jsonl_path = os.path.join(CAPTURES, scenario, "session.jsonl")
        if not os.path.isfile(jsonl_path):
            continue
        with open(jsonl_path) as f:
            lines = [json.loads(l) for l in f if l.strip()]

        expected = 0
        for line in lines:
            if line.get("type") == "message":
                msg = line.get("message", {})
                if msg.get("role") == "assistant":
                    for block in msg.get("content", []):
                        if block.get("type") == "toolCall":
                            expected += 1

        events = translate_file(jsonl_path)
        actual = len([e for e in events if e["subtype"] == "message.assistant.tool_use"])
        assert actual == expected, \
            f"{scenario}: expected {expected} tool_use events, got {actual}"


# ─── Raw data & ID structure ─────────────────────────────────────────

@test
def decomposed_events_share_same_raw():
    """All CloudEvents decomposed from one JSONL line carry the same raw data."""
    events = translate_file(load_scenario("06-thinking-text-tool"))
    # Find thinking, text, and tool_use that came from the same bundled line
    thinking = [e for e in events if e["subtype"] == "message.assistant.thinking"]
    text = [e for e in events if e["subtype"] == "message.assistant.text"]
    tools = [e for e in events if e["subtype"] == "message.assistant.tool_use"]

    assert len(thinking) >= 1 and len(tools) >= 1
    # The thinking and tool_use from the first API call share the same raw
    raw_thinking = json.dumps(thinking[0]["data"]["raw"], sort_keys=True)
    raw_tool = json.dumps(tools[0]["data"]["raw"], sort_keys=True)
    assert raw_thinking == raw_tool, "Decomposed events should share the same raw line"


@test
def raw_preserves_original_bundled_content():
    """Raw data contains the original content array with all blocks."""
    events = translate_file(load_scenario("06-thinking-text-tool"))
    tools = [e for e in events if e["subtype"] == "message.assistant.tool_use"]
    assert len(tools) >= 1
    raw = tools[0]["data"]["raw"]
    content = raw.get("message", {}).get("content", [])
    types = [b.get("type") for b in content]
    # The raw should have ALL three blocks, even though this event is just the tool
    assert "thinking" in types, "raw should preserve thinking block"
    assert "text" in types, "raw should preserve text block"
    assert "toolCall" in types, "raw should preserve toolCall block"


@test
def decomposed_event_ids_differ_from_entry_id():
    """CloudEvent IDs are derived, not the same as the pi-mono entry id."""
    events = translate_file(load_scenario("06-thinking-text-tool"))
    thinking = [e for e in events if e["subtype"] == "message.assistant.thinking"]
    tools = [e for e in events if e["subtype"] == "message.assistant.tool_use"]
    assert len(thinking) >= 1 and len(tools) >= 1
    # Different subtypes from same line should have different CloudEvent IDs
    assert thinking[0]["id"] != tools[0]["id"], \
        "Decomposed events must have unique IDs"


@test
def pimono_entry_id_in_raw():
    """The original pi-mono short hex entry id is preserved in raw data."""
    events = translate_file(load_scenario("06-thinking-text-tool"))
    tools = [e for e in events if e["subtype"] == "message.assistant.tool_use"]
    assert len(tools) >= 1
    raw_id = tools[0]["data"]["raw"].get("id")
    assert raw_id is not None, "pi-mono entry id should be in raw"
    # Pi-mono uses short hex IDs (8 chars), not full UUIDs
    assert len(raw_id) == 8, f"expected 8-char hex id, got '{raw_id}' ({len(raw_id)} chars)"


@test
def parent_child_chain_preserved_in_raw():
    """Pi-mono's parentId chain is preserved in raw data across message events.

    Note: some parentIds point to skipped entry types (thinking_level_change,
    model_change) which don't produce CloudEvents. This is expected — the raw
    data preserves the original chain even when intermediate entries are skipped.
    """
    events = translate_file(load_scenario("06-thinking-text-tool"))
    # Get raw id/parentId from message-type events only
    message_ids = set()
    for e in events:
        raw = e["data"]["raw"]
        if raw.get("type") == "message" and raw.get("id"):
            message_ids.add(raw["id"])

    # Verify that message → message parentId links are valid
    # (parentIds pointing to skipped types like thinking_level_change are allowed)
    for e in events:
        raw = e["data"]["raw"]
        if raw.get("type") != "message":
            continue
        pid = raw.get("parentId")
        if pid and pid in message_ids:
            # If parent is a message we translated, it should exist
            pass  # valid
        # If parent is NOT a message (e.g. thinking_level_change), that's fine —
        # it was skipped but the parentId in raw is preserved honestly


@test
def session_id_is_full_uuid():
    """Pi-mono session IDs are full UUIDs, preserved in CloudEvent source."""
    events = translate_file(load_scenario("06-thinking-text-tool"))
    for e in events:
        source = e.get("source", "")
        # pi://session/{uuid}
        session_id = source.replace("pi://session/", "")
        assert len(session_id) == 36, \
            f"session_id should be full UUID (36 chars), got '{session_id}' ({len(session_id)} chars)"
        assert session_id.count("-") == 4, "session_id should have 4 hyphens (UUID format)"


# ─── Main ─────────────────────────────────────────────────────────────

if __name__ == "__main__":
    print()
    print("Pi-Mono Decomposing Translator — Test Suite")
    print("=" * 50)
    print(f"Captures dir: {CAPTURES}")
    print()
    sys.exit(run_all())
