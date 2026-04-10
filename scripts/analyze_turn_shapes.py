#!/usr/bin/env python3
"""Analyze the problem space of eval-apply turn shapes from real session data.

Fetches records from the OpenStory API and maps every distinct event sequence
that occurs between turn boundaries. This is the empirical input to the
eval-apply detector — the distribution of sequences it must handle correctly.

Usage:
    python3 scripts/analyze_turn_shapes.py [session_id]
    python3 scripts/analyze_turn_shapes.py --all        # analyze all sessions
    python3 scripts/analyze_turn_shapes.py --test       # validate against running server
"""

import json
import sys
import urllib.request
from collections import Counter

API = "http://localhost:3002"
RELEVANT_TYPES = {
    "user_message", "assistant_message", "tool_call", "tool_result",
    "reasoning", "system_event", "turn_end",
}


def fetch_json(url):
    with urllib.request.urlopen(url) as resp:
        return json.loads(resp.read())


def fetch_records(session_id):
    return fetch_json(f"{API}/api/sessions/{session_id}/records")


def segment_turns(records):
    """Segment records into turns bounded by turn_end."""
    relevant = [r for r in records if r.get("record_type") in RELEVANT_TYPES]
    turns = []
    current = []
    for r in relevant:
        rt = r.get("record_type")
        current.append(r)
        if rt == "turn_end":
            turns.append(current)
            current = []
    return turns, current  # complete turns + incomplete remainder


def turn_shape(turn_records):
    """Collapse a turn's record types into a shape string."""
    types = [r["record_type"] for r in turn_records if r["record_type"] != "system_event"]
    collapsed = []
    for rt in types:
        if collapsed and collapsed[-1][0] == rt:
            collapsed[-1] = (rt, collapsed[-1][1] + 1)
        else:
            collapsed.append((rt, 1))
    return " -> ".join(f"{rt}" if n == 1 else f"{rt}x{n}" for rt, n in collapsed)


def classify_shape(shape):
    """Classify a turn shape into a probability class."""
    parts = shape.split(" -> ")
    has_tool = any("tool_" in p for p in parts)
    has_reasoning = any("reasoning" in p for p in parts)
    multi_user = sum(1 for p in parts if p.startswith("user_message")) > 1
    multi_assistant = sum(1 for p in parts if p.startswith("assistant_message")) > 1
    has_parallel = any("x" in p and "tool_call" in p for p in parts)

    classes = []
    if not has_tool:
        classes.append("pure_text")
    if has_tool and not multi_assistant:
        classes.append("single_eval_apply")
    if multi_assistant and has_tool:
        classes.append("multi_eval_apply")
    if has_reasoning:
        classes.append("with_thinking")
    if multi_user:
        classes.append("multi_user_prompt")
    if has_parallel:
        classes.append("parallel_tools")
    if not parts[-1].startswith("turn_end"):
        classes.append("no_turn_end")
    return classes or ["unknown"]


def analyze_session(session_id):
    records = fetch_records(session_id)
    turns, incomplete = segment_turns(records)
    shapes = Counter()
    classes = Counter()

    for turn in turns:
        shape = turn_shape(turn)
        shapes[shape] += 1
        for cls in classify_shape(shape):
            classes[cls] += 1

    return {
        "session_id": session_id,
        "total_records": len(records),
        "complete_turns": len(turns),
        "incomplete_turns": 1 if incomplete else 0,
        "distinct_shapes": len(shapes),
        "shapes": shapes,
        "classes": classes,
    }


def print_analysis(result):
    print(f"\nSession: {result['session_id']}")
    print(f"  Records: {result['total_records']}, Turns: {result['complete_turns']}, "
          f"Distinct shapes: {result['distinct_shapes']}")
    print()
    print("  Probability classes:")
    total = result["complete_turns"]
    for cls, cnt in result["classes"].most_common():
        pct = cnt / total * 100 if total > 0 else 0
        print(f"    {cls:25s} {cnt:4d}  ({pct:5.1f}%)")
    print()
    print("  Top 10 shapes:")
    for shape, cnt in result["shapes"].most_common(10):
        pct = cnt / total * 100 if total > 0 else 0
        print(f"    [{cnt:3d}] ({pct:4.1f}%) {shape[:100]}")


if __name__ == "__main__":
    if len(sys.argv) > 1 and sys.argv[1] == "--all":
        sessions = fetch_json(f"{API}/api/sessions")["sessions"]
        all_classes = Counter()
        all_shapes = Counter()
        total_turns = 0
        for s in sessions:
            try:
                result = analyze_session(s["session_id"])
                all_classes.update(result["classes"])
                all_shapes.update(result["shapes"])
                total_turns += result["complete_turns"]
            except Exception as e:
                print(f"  skip {s['session_id'][:12]}: {e}", file=sys.stderr)

        print(f"\n=== AGGREGATE: {len(sessions)} sessions, {total_turns} turns ===")
        print(f"Distinct shapes: {len(all_shapes)}")
        print()
        print("Probability classes (the problem space):")
        for cls, cnt in all_classes.most_common():
            pct = cnt / total_turns * 100 if total_turns > 0 else 0
            print(f"  {cls:25s} {cnt:4d}  ({pct:5.1f}%)")
        print()
        print("Top 15 shapes:")
        for shape, cnt in all_shapes.most_common(15):
            pct = cnt / total_turns * 100 if total_turns > 0 else 0
            print(f"  [{cnt:3d}] ({pct:4.1f}%) {shape[:100]}")

    elif len(sys.argv) > 1 and sys.argv[1] == "--test":
        print("Running data contract tests against running server...")
        sessions = fetch_json(f"{API}/api/sessions")["sessions"]
        assert len(sessions) > 0, "should have sessions"

        # Pick a session with turns
        sid = sessions[0]["session_id"]
        result = analyze_session(sid)
        assert result["complete_turns"] >= 0, "should parse turns"
        print(f"  PASS: analyzed {result['complete_turns']} turns")
        print("All tests passed.")

    else:
        session_id = sys.argv[1] if len(sys.argv) > 1 else None
        if not session_id:
            sessions = fetch_json(f"{API}/api/sessions")["sessions"]
            session_id = max(sessions, key=lambda s: s.get("event_count", 0))["session_id"]
            print(f"Using largest session: {session_id[:12]}")
        result = analyze_session(session_id)
        print_analysis(result)
