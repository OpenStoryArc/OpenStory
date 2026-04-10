"""Analyze session hierarchy — how do agent sessions relate to parent sessions?

Investigates:
- Do agent session IDs encode their parent?
- Can we link via project_id?
- Can we link via timestamps (agent started during parent's timespan)?
- What data fields distinguish main vs agent sessions?

Usage:
    uv run python scripts/analyze_session_hierarchy.py [--url URL]
    uv run python scripts/analyze_session_hierarchy.py --test
"""

import argparse
import json
import urllib.request
from collections import defaultdict


def fetch_sessions(base_url: str) -> list[dict]:
    data = urllib.request.urlopen(f"{base_url}/api/sessions").read()
    return json.loads(data)["sessions"]


def classify_sessions(sessions: list[dict]) -> dict:
    """Classify sessions into main vs agent and find relationships."""
    main = [s for s in sessions if not s["session_id"].startswith("agent-")]
    agents = [s for s in sessions if s["session_id"].startswith("agent-")]

    # Group by project
    by_project: dict[str, list[dict]] = defaultdict(list)
    for s in sessions:
        proj = s.get("project_name") or s.get("project_id") or "(none)"
        by_project[proj].append(s)

    # Try to match agents to parents by project + time overlap
    parent_map: dict[str, str] = {}  # agent_session_id -> parent_session_id
    orphan_agents: list[dict] = []

    for agent in agents:
        agent_start = agent.get("start_time", "")
        agent_project = agent.get("project_id") or agent.get("project_name")
        matched = False

        for parent in main:
            parent_project = parent.get("project_id") or parent.get("project_name")
            if agent_project and parent_project and agent_project == parent_project:
                # Same project — likely related
                parent_map[agent["session_id"]] = parent["session_id"]
                matched = True
                break

        if not matched:
            orphan_agents.append(agent)

    return {
        "main": main,
        "agents": agents,
        "parent_map": parent_map,
        "orphan_agents": orphan_agents,
        "by_project": dict(by_project),
    }


def print_hierarchy(result: dict) -> None:
    """Print the session hierarchy."""
    main = result["main"]
    agents = result["agents"]
    parent_map = result["parent_map"]

    print(f"Main sessions: {len(main)}")
    print(f"Agent sessions: {len(agents)}")
    print(f"  Matched to parent: {len(parent_map)}")
    print(f"  Orphaned: {len(result['orphan_agents'])}")
    print()

    # Show main sessions with their agent children
    children_of: dict[str, list[str]] = defaultdict(list)
    for agent_id, parent_id in parent_map.items():
        children_of[parent_id].append(agent_id)

    # All sessions lookup
    all_sessions = {s["session_id"]: s for s in main + agents}

    print("=== Session Hierarchy ===")
    print()
    for m in sorted(main, key=lambda s: s.get("start_time", ""), reverse=True):
        sid = m["session_id"]
        prompt = (m.get("first_prompt") or "")[:60]
        status = m.get("status", "?")
        events = m.get("event_count", 0)
        print(f"[{status:9s}] {sid[:12]}  {events:4d} events  {prompt}")

        for child_id in children_of.get(sid, []):
            child = all_sessions.get(child_id, {})
            cprompt = (child.get("first_prompt") or "")[:50]
            cstatus = child.get("status", "?")
            cevents = child.get("event_count", 0)
            print(f"  |-- [{cstatus:9s}] {child_id[:20]}  {cevents:4d} events  {cprompt}")
        print()

    if result["orphan_agents"]:
        print("=== Orphaned Agent Sessions ===")
        for o in result["orphan_agents"]:
            prompt = (o.get("first_prompt") or "")[:50]
            print(f"  {o['session_id'][:20]}  {o.get('event_count',0):4d} events  {prompt}")

    # Field analysis
    print()
    print("=== Field Analysis ===")
    print()
    sample_agent = agents[0] if agents else {}
    sample_main = main[0] if main else {}
    all_keys = set(sample_agent.keys()) | set(sample_main.keys())
    print(f"Available fields: {sorted(all_keys)}")
    print()

    # Check what distinguishes agents
    print("Agent session ID patterns:")
    prefixes = set()
    for a in agents[:10]:
        sid = a["session_id"]
        # agent-XXXX pattern
        parts = sid.split("-")
        if len(parts) >= 2:
            prefixes.add(f"{parts[0]}-{parts[1][:4]}")
    for p in sorted(prefixes):
        print(f"  {p}...")

    # Check label/branch fields
    print()
    print("Labels on agent sessions:")
    for a in agents[:5]:
        print(f"  {a['session_id'][:20]}  label={a.get('label')!r}  branch={a.get('branch')!r}")


def _test():
    sessions = [
        {"session_id": "abc-123", "status": "completed", "start_time": "2025-01-16T10:00:00Z",
         "event_count": 100, "first_prompt": "Fix bug", "project_name": "Alpha", "project_id": "p1"},
        {"session_id": "agent-xyz", "status": "stale", "start_time": "2025-01-16T09:00:00Z",
         "event_count": 30, "first_prompt": "Research", "project_name": "Alpha", "project_id": "p1"},
        {"session_id": "agent-orphan", "status": "stale", "start_time": "2025-01-16T09:00:00Z",
         "event_count": 20, "first_prompt": "Lost", "project_name": None, "project_id": None},
    ]

    result = classify_sessions(sessions)
    assert len(result["main"]) == 1
    assert len(result["agents"]) == 2
    assert result["parent_map"]["agent-xyz"] == "abc-123"
    assert len(result["orphan_agents"]) == 1
    print("All tests passed.")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="Analyze session hierarchy")
    parser.add_argument("--url", default="http://localhost:3002", help="API base URL")
    parser.add_argument("--test", action="store_true", help="Run self-tests")
    args = parser.parse_args()

    if args.test:
        _test()
    else:
        sessions = fetch_sessions(args.url)
        result = classify_sessions(sessions)
        print_hierarchy(result)
