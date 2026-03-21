"""Analyze session data from the Open Story API to understand categorization patterns.

Usage:
    uv run python scripts/analyze_sessions.py [--url URL]
    uv run python scripts/analyze_sessions.py --test
"""

import argparse
import json
import urllib.request
from collections import Counter
from datetime import datetime


def fetch_sessions(base_url: str) -> list[dict]:
    """Fetch all sessions from the API."""
    data = urllib.request.urlopen(f"{base_url}/api/sessions").read()
    return json.loads(data)


def analyze(sessions: list[dict]) -> dict:
    """Analyze session data and return categorization insights."""
    results = {}

    results["total"] = len(sessions)

    # Status breakdown
    results["by_status"] = dict(Counter(s.get("status", "unknown") for s in sessions))

    # Project breakdown
    results["by_project"] = dict(Counter(s.get("project_name") or "(no project)" for s in sessions))

    # Date breakdown
    by_date: dict[str, int] = {}
    for s in sessions:
        ts = (s.get("start_time") or "")[:10]
        if ts:
            by_date[ts] = by_date.get(ts, 0) + 1
    results["by_date"] = dict(sorted(by_date.items(), reverse=True))

    # Agent vs main sessions
    agent_sessions = [s for s in sessions if s["session_id"].startswith("agent-")]
    main_sessions = [s for s in sessions if not s["session_id"].startswith("agent-")]
    results["main_count"] = len(main_sessions)
    results["agent_count"] = len(agent_sessions)

    # Event count distribution
    event_counts = [s.get("event_count", 0) for s in sessions]
    results["event_count_buckets"] = {
        "tiny (<50)": sum(1 for c in event_counts if c < 50),
        "small (50-200)": sum(1 for c in event_counts if 50 <= c < 200),
        "medium (200-500)": sum(1 for c in event_counts if 200 <= c < 500),
        "large (500+)": sum(1 for c in event_counts if c >= 500),
    }

    # Duration distribution
    durations = [s["duration_ms"] for s in sessions if s.get("duration_ms")]
    if durations:
        results["duration_buckets"] = {
            "< 1 min": sum(1 for d in durations if d < 60_000),
            "1-10 min": sum(1 for d in durations if 60_000 <= d < 600_000),
            "10-60 min": sum(1 for d in durations if 600_000 <= d < 3_600_000),
            "60+ min": sum(1 for d in durations if d >= 3_600_000),
        }

    # Sessions with/without prompts
    with_prompt = sum(1 for s in sessions if s.get("first_prompt"))
    results["with_prompt"] = with_prompt
    results["without_prompt"] = len(sessions) - with_prompt

    return results


def print_report(results: dict) -> None:
    """Print a human-readable report."""
    print(f"Total sessions: {results['total']}")
    print(f"  Main: {results['main_count']}, Agent: {results['agent_count']}")
    print()

    print("By status:")
    for status, count in sorted(results["by_status"].items(), key=lambda x: -x[1]):
        print(f"  {status:12s} {count}")
    print()

    print("By project:")
    for project, count in sorted(results["by_project"].items(), key=lambda x: -x[1]):
        print(f"  {project:30s} {count}")
    print()

    print("By date:")
    for date, count in results["by_date"].items():
        print(f"  {date}  {count}")
    print()

    print("Event count distribution:")
    for bucket, count in results["event_count_buckets"].items():
        print(f"  {bucket:20s} {count}")
    print()

    if "duration_buckets" in results:
        print("Duration distribution:")
        for bucket, count in results["duration_buckets"].items():
            print(f"  {bucket:20s} {count}")
        print()

    print(f"With prompt: {results['with_prompt']}, Without: {results['without_prompt']}")
    print()

    # Key insight
    agent_pct = results["agent_count"] / max(results["total"], 1) * 100
    print("--- Insights ---")
    print(f"Agent sessions are {agent_pct:.0f}% of total ({results['agent_count']}/{results['total']})")
    if agent_pct > 30:
        print("  -> Consider hiding agent sessions by default, or grouping under their parent")
    stale = results["by_status"].get("stale", 0)
    stale_pct = stale / max(results["total"], 1) * 100
    if stale_pct > 50:
        print(f"  -> {stale_pct:.0f}% of sessions are stale — consider defaulting to non-stale filter")


def _test():
    """Self-tests for the analyze function."""
    sessions = [
        {"session_id": "abc-123", "status": "completed", "start_time": "2025-01-16T10:00:00Z",
         "event_count": 100, "duration_ms": 300000, "first_prompt": "Fix bug", "project_name": "Alpha"},
        {"session_id": "agent-xyz", "status": "stale", "start_time": "2025-01-16T09:00:00Z",
         "event_count": 30, "duration_ms": 60000, "first_prompt": "Research", "project_name": "Alpha"},
        {"session_id": "def-456", "status": "stale", "start_time": "2025-01-15T08:00:00Z",
         "event_count": 500, "first_prompt": None, "project_name": None},
    ]

    r = analyze(sessions)
    assert r["total"] == 3
    assert r["main_count"] == 2
    assert r["agent_count"] == 1
    assert r["by_status"]["stale"] == 2
    assert r["by_status"]["completed"] == 1
    assert r["by_project"]["Alpha"] == 2
    assert r["with_prompt"] == 2
    assert r["without_prompt"] == 1
    assert r["event_count_buckets"]["tiny (<50)"] == 1
    assert r["event_count_buckets"]["small (50-200)"] == 1
    assert r["event_count_buckets"]["large (500+)"] == 1
    print("All tests passed.")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="Analyze Open Story session data")
    parser.add_argument("--url", default="http://localhost:3002", help="API base URL")
    parser.add_argument("--test", action="store_true", help="Run self-tests")
    args = parser.parse_args()

    if args.test:
        _test()
    else:
        sessions = fetch_sessions(args.url)
        results = analyze(sessions)
        print_report(results)
