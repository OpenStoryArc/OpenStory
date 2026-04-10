"""Analyze event sequences in sessions to find natural groupings.

Questions:
- What patterns of tool usage repeat?
- Are there natural "phases" (read, then edit, then test)?
- How do turns structure the conversation?
- What groupings would help navigation?

Usage:
    uv run python scripts/analyze_event_groups.py [--url URL] [--session ID]
    uv run python scripts/analyze_event_groups.py --test
"""

import argparse
import json
import urllib.request
from collections import Counter


def fetch_records(base_url: str, session_id: str) -> list[dict]:
    """Fetch WireRecords for a session."""
    data = urllib.request.urlopen(f"{base_url}/api/sessions/{session_id}/records").read()
    return json.loads(data)


def fetch_sessions(base_url: str) -> list[dict]:
    data = urllib.request.urlopen(f"{base_url}/api/sessions").read()
    return json.loads(data)["sessions"]


def analyze_event_sequence(records: list[dict]) -> dict:
    """Analyze the event sequence for natural groupings."""
    results = {}

    # Record type sequence
    types = [r.get("record_type", "?") for r in records]
    results["total"] = len(records)
    results["type_counts"] = dict(Counter(types))

    # Tool name sequence (for tool_call records)
    tool_sequence = []
    for r in records:
        if r.get("record_type") == "tool_call":
            payload = r.get("payload", {})
            tool_sequence.append(payload.get("name", "?"))
    results["tool_sequence_length"] = len(tool_sequence)

    # Consecutive tool runs (same tool repeated)
    runs = []
    if tool_sequence:
        current_tool = tool_sequence[0]
        current_count = 1
        for t in tool_sequence[1:]:
            if t == current_tool:
                current_count += 1
            else:
                runs.append((current_tool, current_count))
                current_tool = t
                current_count = 1
        runs.append((current_tool, current_count))
    results["tool_runs"] = runs
    results["run_count"] = len(runs)

    # Turn boundaries (user_message records)
    turns = []
    current_turn_events = []
    turn_number = 0
    for r in records:
        if r.get("record_type") == "user_message":
            if current_turn_events:
                turns.append(summarize_turn(turn_number, current_turn_events))
                turn_number += 1
            current_turn_events = [r]
        else:
            current_turn_events.append(r)
    if current_turn_events:
        turns.append(summarize_turn(turn_number, current_turn_events))
    results["turns"] = turns
    results["turn_count"] = len(turns)

    # Common tool sequences (bigrams)
    if len(tool_sequence) >= 2:
        bigrams = Counter()
        for i in range(len(tool_sequence) - 1):
            bigrams[(tool_sequence[i], tool_sequence[i + 1])] += 1
        results["common_sequences"] = [
            {"from": a, "to": b, "count": c}
            for (a, b), c in bigrams.most_common(10)
        ]

    # Phases: group consecutive events by "activity type"
    phases = []
    current_phase = None
    current_phase_events = []
    for r in records:
        phase = classify_phase(r)
        if phase != current_phase:
            if current_phase and current_phase_events:
                phases.append({
                    "phase": current_phase,
                    "events": len(current_phase_events),
                    "tools": [e.get("payload", {}).get("name") for e in current_phase_events
                             if e.get("record_type") == "tool_call"],
                })
            current_phase = phase
            current_phase_events = [r]
        else:
            current_phase_events.append(r)
    if current_phase and current_phase_events:
        phases.append({
            "phase": current_phase,
            "events": len(current_phase_events),
            "tools": [e.get("payload", {}).get("name") for e in current_phase_events
                     if e.get("record_type") == "tool_call"],
        })
    results["phases"] = phases
    results["phase_count"] = len(phases)

    return results


def classify_phase(record: dict) -> str:
    """Classify a record into a high-level phase."""
    rt = record.get("record_type", "")
    if rt == "user_message":
        return "prompt"
    if rt == "assistant_message":
        return "response"
    if rt in ("reasoning",):
        return "thinking"
    if rt == "error":
        return "error"
    if rt in ("turn_start", "turn_end"):
        return "turn_boundary"
    if rt == "tool_call":
        tool = record.get("payload", {}).get("name", "")
        if tool in ("Read", "Glob", "Grep"):
            return "investigate"
        if tool in ("Edit", "Write"):
            return "modify"
        if tool == "Bash":
            return "execute"
        if tool == "Agent":
            return "delegate"
        return "tool_other"
    if rt == "tool_result":
        return "result"
    return "other"


def summarize_turn(number: int, events: list[dict]) -> dict:
    """Summarize a turn's contents."""
    types = Counter(e.get("record_type", "?") for e in events)
    tools = [e.get("payload", {}).get("name", "?")
             for e in events if e.get("record_type") == "tool_call"]
    prompt = ""
    for e in events:
        if e.get("record_type") == "user_message":
            prompt = (e.get("payload", {}).get("text", "") or "")[:60]
            break
    return {
        "turn": number,
        "events": len(events),
        "types": dict(types),
        "tools": tools,
        "prompt": prompt,
    }


def print_analysis(results: dict) -> None:
    """Print human-readable analysis."""
    print(f"Total records: {results['total']}")
    print(f"Turns: {results['turn_count']}")
    print(f"Tool calls: {results['tool_sequence_length']}")
    print(f"Tool runs (consecutive same-tool): {results['run_count']}")
    print(f"Activity phases: {results['phase_count']}")
    print()

    print("=== Record types ===")
    for t, c in sorted(results["type_counts"].items(), key=lambda x: -x[1]):
        print(f"  {t:25s} {c}")
    print()

    print("=== Turns ===")
    for t in results["turns"]:
        tools_str = ", ".join(t["tools"][:8])
        if len(t["tools"]) > 8:
            tools_str += f" +{len(t['tools']) - 8} more"
        print(f"  Turn {t['turn']:2d}: {t['events']:3d} events  [{tools_str}]")
        if t["prompt"]:
            print(f"           {t['prompt']}")
    print()

    print("=== Activity phases ===")
    phase_summary = Counter(p["phase"] for p in results["phases"])
    print("  Phase distribution:")
    for p, c in phase_summary.most_common():
        print(f"    {p:15s} {c}x")
    print()
    print("  Phase sequence (first 30):")
    for p in results["phases"][:30]:
        tools_str = ", ".join(p["tools"][:5]) if p["tools"] else ""
        print(f"    {p['phase']:15s} {p['events']:3d} events  {tools_str}")
    print()

    if "common_sequences" in results:
        print("=== Common tool sequences ===")
        for s in results["common_sequences"][:8]:
            print(f"  {s['from']:10s} -> {s['to']:10s}  {s['count']}x")
    print()

    print("=== Tool runs (consecutive same-tool) ===")
    for tool, count in results["tool_runs"][:20]:
        bar = "#" * min(count, 20)
        print(f"  {tool:10s} x{count:2d}  {bar}")


def _test():
    records = [
        {"record_type": "user_message", "payload": {"text": "Fix the bug"}},
        {"record_type": "tool_call", "payload": {"name": "Read"}},
        {"record_type": "tool_result", "payload": {}},
        {"record_type": "tool_call", "payload": {"name": "Read"}},
        {"record_type": "tool_result", "payload": {}},
        {"record_type": "tool_call", "payload": {"name": "Edit"}},
        {"record_type": "tool_result", "payload": {}},
        {"record_type": "tool_call", "payload": {"name": "Bash"}},
        {"record_type": "tool_result", "payload": {}},
        {"record_type": "assistant_message", "payload": {"text": "Fixed!"}},
    ]

    r = analyze_event_sequence(records)
    assert r["total"] == 10
    assert r["turn_count"] == 1
    assert r["tool_sequence_length"] == 4
    assert len(r["tool_runs"]) == 3  # Read x2, Edit x1, Bash x1
    assert r["tool_runs"][0] == ("Read", 2)
    assert r["phase_count"] > 0
    print("All tests passed.")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="Analyze event groupings in sessions")
    parser.add_argument("--url", default="http://localhost:3002", help="API base URL")
    parser.add_argument("--session", default="", help="Session ID (default: largest main session)")
    parser.add_argument("--test", action="store_true", help="Run self-tests")
    args = parser.parse_args()

    if args.test:
        _test()
    else:
        if not args.session:
            sessions = fetch_sessions(args.url)
            main = [s for s in sessions if not s["session_id"].startswith("agent-")]
            main.sort(key=lambda s: s.get("event_count", 0), reverse=True)
            if not main:
                print("No main sessions found")
                exit(1)
            args.session = main[0]["session_id"]
            print(f"Using session: {args.session[:12]} ({main[0].get('event_count', 0)} events)")
            print()

        records = fetch_records(args.url, args.session)
        if not records:
            print(f"No records for session {args.session[:12]}")
            print("(Projection may be empty - try a session with recent activity)")
            exit(1)
        results = analyze_event_sequence(records)
        print_analysis(results)
