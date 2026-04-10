"""Prototype: Event graph navigation model.

Builds inverted indexes from real session data and validates the
faceted navigation model before implementing in TypeScript.

Usage:
    uv run python scripts/prototype_event_graph.py [--url URL] [--session ID]
    uv run python scripts/prototype_event_graph.py --interactive [--session ID]
    uv run python scripts/prototype_event_graph.py --test
"""

import argparse
import json
import urllib.request
from collections import Counter, defaultdict
from dataclasses import dataclass, field


# ---------------------------------------------------------------------------
# Data model
# ---------------------------------------------------------------------------

@dataclass
class Turn:
    index: int
    prompt_text: str | None
    prompt_timestamp: str | None
    response_text: str | None
    event_ids: list[str] = field(default_factory=list)
    tool_counts: dict[str, int] = field(default_factory=dict)
    files: list[str] = field(default_factory=list)
    has_error: bool = False


@dataclass
class EventGraph:
    turns: list[Turn]
    file_index: dict[str, list[str]]    # file path -> event IDs
    tool_index: dict[str, list[str]]    # tool name -> event IDs
    agent_index: dict[str, list[str]]   # agent ID -> event IDs
    error_ids: list[str]
    records_by_id: dict[str, dict]      # event ID -> record


# ---------------------------------------------------------------------------
# Extraction
# ---------------------------------------------------------------------------

def extract_file_path(record: dict) -> str | None:
    """Extract file path from a tool_call record's payload."""
    if record.get("record_type") != "tool_call":
        return None

    payload = record.get("payload", {})

    # Try typed_input first
    ti = payload.get("typed_input")
    if ti and isinstance(ti, dict):
        fp = ti.get("file_path")
        if fp:
            return fp

    # Try raw_input
    ri = payload.get("raw_input")
    if ri and isinstance(ri, dict):
        fp = ri.get("file_path") or ri.get("file") or ri.get("path")
        if fp:
            return fp

    return None


def extract_tool_name(record: dict) -> str | None:
    """Extract tool name from a tool_call record."""
    if record.get("record_type") != "tool_call":
        return None
    return record.get("payload", {}).get("name")


def is_error(record: dict) -> bool:
    """Check if a record is an error."""
    if record.get("record_type") == "error":
        return True
    if record.get("record_type") == "tool_result":
        payload = record.get("payload", {})
        if payload.get("is_error"):
            return True
    return False


# ---------------------------------------------------------------------------
# Graph building
# ---------------------------------------------------------------------------

def split_into_turns(records: list[dict]) -> list[Turn]:
    """Split records into turns at user_message boundaries."""
    turns: list[Turn] = []
    current_events: list[dict] = []
    current_prompt: str | None = None
    current_prompt_ts: str | None = None
    turn_idx = 0

    def flush():
        nonlocal turn_idx
        if not current_events:
            return
        tool_counts: dict[str, int] = {}
        files_set: set[str] = set()
        has_err = False
        response = None

        for e in current_events:
            tn = extract_tool_name(e)
            if tn:
                tool_counts[tn] = tool_counts.get(tn, 0) + 1
            fp = extract_file_path(e)
            if fp:
                files_set.add(fp)
            if is_error(e):
                has_err = True
            if e.get("record_type") == "assistant_message":
                text = e.get("payload", {}).get("text", "")
                if text:
                    response = text

        turns.append(Turn(
            index=turn_idx,
            prompt_text=current_prompt,
            prompt_timestamp=current_prompt_ts,
            response_text=response,
            event_ids=[e.get("id", "") for e in current_events],
            tool_counts=tool_counts,
            files=sorted(files_set),
            has_error=has_err,
        ))
        turn_idx += 1

    for r in records:
        if r.get("record_type") == "user_message":
            flush()
            current_events = [r]
            text = r.get("payload", {}).get("text", "")
            current_prompt = text[:100] if text else None
            current_prompt_ts = r.get("timestamp")
        else:
            current_events.append(r)

    flush()
    return turns


def build_event_graph(records: list[dict]) -> EventGraph:
    """Build all indexes in a single pass."""
    file_index: dict[str, list[str]] = defaultdict(list)
    tool_index: dict[str, list[str]] = defaultdict(list)
    agent_index: dict[str, list[str]] = defaultdict(list)
    error_ids: list[str] = []
    records_by_id: dict[str, dict] = {}

    for r in records:
        rid = r.get("id", "")
        records_by_id[rid] = r

        fp = extract_file_path(r)
        if fp:
            file_index[fp].append(rid)

        tn = extract_tool_name(r)
        if tn:
            tool_index[tn].append(rid)

        aid = r.get("agent_id")
        if aid:
            agent_index[aid].append(rid)

        if is_error(r):
            error_ids.append(rid)

    turns = split_into_turns(records)

    return EventGraph(
        turns=turns,
        file_index=dict(file_index),
        tool_index=dict(tool_index),
        agent_index=dict(agent_index),
        error_ids=error_ids,
        records_by_id=records_by_id,
    )


# ---------------------------------------------------------------------------
# Faceted queries
# ---------------------------------------------------------------------------

def apply_facets(
    graph: EventGraph,
    turn: int | None = None,
    file: str | None = None,
    tool: str | None = None,
    agent: str | None = None,
) -> list[str]:
    """Return event IDs matching all active facets (intersection)."""
    sets: list[set[str]] = []

    if turn is not None:
        if 0 <= turn < len(graph.turns):
            sets.append(set(graph.turns[turn].event_ids))
        else:
            return []

    if file is not None:
        sets.append(set(graph.file_index.get(file, [])))

    if tool is not None:
        sets.append(set(graph.tool_index.get(tool, [])))

    if agent is not None:
        sets.append(set(graph.agent_index.get(agent, [])))

    if not sets:
        return list(graph.records_by_id.keys())

    result = sets[0]
    for s in sets[1:]:
        result = result & s

    # Preserve original order
    all_ids = list(graph.records_by_id.keys())
    return [rid for rid in all_ids if rid in result]


# ---------------------------------------------------------------------------
# Printing
# ---------------------------------------------------------------------------

def print_graph_summary(graph: EventGraph) -> None:
    """Print the graph summary."""
    print(f"Total records: {len(graph.records_by_id)}")
    print(f"Turns: {len(graph.turns)}")
    print(f"Unique files: {len(graph.file_index)}")
    print(f"Tool types: {len(graph.tool_index)}")
    print(f"Agents: {len(graph.agent_index)}")
    print(f"Errors: {len(graph.error_ids)}")
    print()

    print("=== Turn Outline ===")
    for t in graph.turns:
        tools_str = "  ".join(f"{k}({v})" for k, v in sorted(t.tool_counts.items(), key=lambda x: -x[1]))
        err = " [ERROR]" if t.has_error else ""
        prompt = (t.prompt_text or "(no prompt)")[:60]
        print(f"  Turn {t.index:2d}: {len(t.event_ids):3d} events  {len(t.files):2d} files{err}")
        print(f"          {prompt}")
        if tools_str:
            print(f"          {tools_str}")
    print()

    print("=== File Index (top 15) ===")
    sorted_files = sorted(graph.file_index.items(), key=lambda x: -len(x[1]))
    for fp, ids in sorted_files[:15]:
        # Count reads vs writes
        reads = 0
        writes = 0
        for rid in ids:
            r = graph.records_by_id[rid]
            tn = extract_tool_name(r)
            if tn in ("Read", "Grep", "Glob"):
                reads += 1
            elif tn in ("Edit", "Write"):
                writes += 1
        # Shorten path
        short = fp.replace("\\", "/")
        if len(short) > 50:
            short = "..." + short[-47:]
        print(f"  {short:50s}  {len(ids):3d} events  {reads}R {writes}W")
    print()

    print("=== Tool Index ===")
    sorted_tools = sorted(graph.tool_index.items(), key=lambda x: -len(x[1]))
    for tn, ids in sorted_tools:
        # Which turns use this tool?
        turn_set = set()
        for t in graph.turns:
            if tn in t.tool_counts:
                turn_set.add(t.index)
        print(f"  {tn:12s}  {len(ids):3d} events  across {len(turn_set)} turns")
    print()

    if graph.agent_index:
        print("=== Agent Index ===")
        for aid, ids in sorted(graph.agent_index.items(), key=lambda x: -len(x[1])):
            print(f"  {aid[:20]:20s}  {len(ids):3d} events")
        print()

    if graph.error_ids:
        print(f"=== Errors ({len(graph.error_ids)}) ===")
        for eid in graph.error_ids[:5]:
            r = graph.records_by_id[eid]
            text = r.get("payload", {}).get("text", r.get("payload", {}).get("output", ""))
            if isinstance(text, str):
                text = text[:80]
            print(f"  {r.get('timestamp', '?')[:19]}  {text}")
        print()


def print_facet_query(graph: EventGraph, label: str, **facets) -> None:
    """Print results of a facet query."""
    ids = apply_facets(graph, **facets)
    print(f"--- {label} ({len(ids)} events) ---")
    for rid in ids[:10]:
        r = graph.records_by_id[rid]
        rt = r.get("record_type", "?")
        ts = r.get("timestamp", "?")[:19]
        payload = r.get("payload", {})
        tool = payload.get("name", "")
        fp = extract_file_path(r) or ""
        text = payload.get("text", "")
        if isinstance(text, str) and text:
            detail = text[:50]
        elif fp:
            detail = fp[-50:]
        elif tool:
            detail = tool
        else:
            detail = ""
        print(f"  {ts}  {rt:20s}  {detail}")
    if len(ids) > 10:
        print(f"  ... and {len(ids) - 10} more")
    print()


# ---------------------------------------------------------------------------
# Fetching
# ---------------------------------------------------------------------------

def fetch_records(base_url: str, session_id: str) -> list[dict]:
    data = urllib.request.urlopen(f"{base_url}/api/sessions/{session_id}/records").read()
    return json.loads(data)


def fetch_sessions(base_url: str) -> list[dict]:
    data = urllib.request.urlopen(f"{base_url}/api/sessions").read()
    return json.loads(data)["sessions"]


def pick_session(base_url: str) -> tuple[str, int]:
    """Pick the largest main session."""
    sessions = fetch_sessions(base_url)
    main = [s for s in sessions if not s["session_id"].startswith("agent-")]
    main.sort(key=lambda s: s.get("event_count", 0), reverse=True)
    if not main:
        raise SystemExit("No main sessions found")
    s = main[0]
    return s["session_id"], s.get("event_count", 0)


# ---------------------------------------------------------------------------
# Self-tests
# ---------------------------------------------------------------------------

def _test():
    # --- extract_file_path ---
    assert extract_file_path({"record_type": "tool_call", "payload": {"name": "Read", "typed_input": {"tool": "read", "file_path": "/a.ts"}}}) == "/a.ts"
    assert extract_file_path({"record_type": "tool_call", "payload": {"name": "Edit", "raw_input": {"file_path": "/b.rs"}}}) == "/b.rs"
    assert extract_file_path({"record_type": "tool_call", "payload": {"name": "Grep", "raw_input": {"path": "src/"}}}) == "src/"
    assert extract_file_path({"record_type": "tool_call", "payload": {"name": "Bash", "raw_input": {"command": "test"}}}) is None
    assert extract_file_path({"record_type": "user_message", "payload": {"text": "hi"}}) is None
    assert extract_file_path({"record_type": "tool_call", "payload": {"name": "Agent"}}) is None

    # --- split_into_turns ---
    records = [
        {"id": "1", "record_type": "user_message", "payload": {"text": "Fix bug"}, "timestamp": "T1"},
        {"id": "2", "record_type": "tool_call", "payload": {"name": "Read", "typed_input": {"tool": "read", "file_path": "a.ts"}}, "timestamp": "T2"},
        {"id": "3", "record_type": "tool_result", "payload": {"output": "ok"}, "timestamp": "T3"},
        {"id": "4", "record_type": "assistant_message", "payload": {"text": "Fixed!"}, "timestamp": "T4"},
        {"id": "5", "record_type": "user_message", "payload": {"text": "Add tests"}, "timestamp": "T5"},
        {"id": "6", "record_type": "tool_call", "payload": {"name": "Bash", "raw_input": {"command": "npm test"}}, "timestamp": "T6"},
        {"id": "7", "record_type": "tool_result", "payload": {"output": "pass"}, "timestamp": "T7"},
        {"id": "8", "record_type": "assistant_message", "payload": {"text": "Done"}, "timestamp": "T8"},
    ]
    turns = split_into_turns(records)
    assert len(turns) == 2
    assert turns[0].index == 0
    assert turns[0].prompt_text == "Fix bug"
    assert len(turns[0].event_ids) == 4
    assert turns[0].tool_counts == {"Read": 1}
    assert turns[0].files == ["a.ts"]
    assert turns[0].response_text == "Fixed!"
    assert turns[1].index == 1
    assert turns[1].prompt_text == "Add tests"
    assert turns[1].tool_counts == {"Bash": 1}

    # --- events before first prompt ---
    records2 = [
        {"id": "0", "record_type": "tool_call", "payload": {"name": "Read", "raw_input": {"file_path": "x.ts"}}, "timestamp": "T0"},
        {"id": "1", "record_type": "user_message", "payload": {"text": "Go"}, "timestamp": "T1"},
    ]
    turns2 = split_into_turns(records2)
    assert len(turns2) == 2
    assert turns2[0].prompt_text is None
    assert turns2[1].prompt_text == "Go"

    # --- error detection ---
    records3 = [
        {"id": "1", "record_type": "user_message", "payload": {"text": "Try"}, "timestamp": "T1"},
        {"id": "2", "record_type": "error", "payload": {"text": "boom"}, "timestamp": "T2"},
    ]
    turns3 = split_into_turns(records3)
    assert turns3[0].has_error is True

    # --- build_event_graph ---
    graph = build_event_graph(records)
    assert len(graph.turns) == 2
    assert "a.ts" in graph.file_index
    assert len(graph.file_index["a.ts"]) == 1
    assert "Read" in graph.tool_index
    assert "Bash" in graph.tool_index
    assert len(graph.error_ids) == 0

    # --- apply_facets ---
    all_ids = apply_facets(graph)
    assert len(all_ids) == 8

    turn0 = apply_facets(graph, turn=0)
    assert len(turn0) == 4
    assert "1" in turn0

    file_a = apply_facets(graph, file="a.ts")
    assert len(file_a) == 1
    assert "2" in file_a

    bash = apply_facets(graph, tool="Bash")
    assert len(bash) == 1
    assert "6" in bash

    # Intersection: turn 1 + Bash
    t1_bash = apply_facets(graph, turn=1, tool="Bash")
    assert len(t1_bash) == 1
    assert "6" in t1_bash

    # No match
    empty = apply_facets(graph, file="nonexistent.ts")
    assert len(empty) == 0

    print("All tests passed.")


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="Prototype event graph navigation")
    parser.add_argument("--url", default="http://localhost:3002", help="API base URL")
    parser.add_argument("--session", default="", help="Session ID")
    parser.add_argument("--interactive", action="store_true", help="Simulate interactive queries")
    parser.add_argument("--test", action="store_true", help="Run self-tests")
    args = parser.parse_args()

    if args.test:
        _test()
        raise SystemExit(0)

    session_id = args.session
    if not session_id:
        session_id, count = pick_session(args.url)
        print(f"Using session: {session_id[:12]} ({count} events)")
        print()

    records = fetch_records(args.url, session_id)
    if not records:
        print(f"No records for {session_id[:12]} (projection may be empty)")
        raise SystemExit(1)

    graph = build_event_graph(records)
    print_graph_summary(graph)

    if args.interactive:
        print("=== Faceted Queries ===")
        print()

        # Query: first turn
        if graph.turns:
            print_facet_query(graph, f"Turn 0: '{graph.turns[0].prompt_text or '?'}'", turn=0)

        # Query: most-touched file
        if graph.file_index:
            top_file = max(graph.file_index, key=lambda f: len(graph.file_index[f]))
            print_facet_query(graph, f"File: {top_file}", file=top_file)

        # Query: Bash commands
        if "Bash" in graph.tool_index:
            print_facet_query(graph, "All Bash commands", tool="Bash")

        # Query: errors (direct ID listing, not a facet)
        if graph.error_ids:
            print(f"--- All errors ({len(graph.error_ids)} events) ---")
            for eid in graph.error_ids[:10]:
                r = graph.records_by_id[eid]
                ts = r.get("timestamp", "?")[:19]
                text = r.get("payload", {}).get("text", r.get("payload", {}).get("output", ""))
                if isinstance(text, str):
                    text = text[:80]
                print(f"  {ts}  {text}")
            print()

        # Cross-facet: largest turn + most used tool
        if graph.turns and graph.tool_index:
            largest_turn = max(range(len(graph.turns)), key=lambda i: len(graph.turns[i].event_ids))
            top_tool = max(graph.tool_index, key=lambda t: len(graph.tool_index[t]))
            print_facet_query(
                graph,
                f"Turn {largest_turn} + {top_tool}",
                turn=largest_turn,
                tool=top_tool,
            )
