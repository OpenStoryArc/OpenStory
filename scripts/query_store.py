"""Query the open-story SQLite event store for session characteristics and patterns.

Usage:
    uv run python scripts/query_store.py                    # default data dir
    uv run python scripts/query_store.py --data-dir ./data  # custom data dir
    uv run python scripts/query_store.py --test             # run self-tests
"""

import argparse
import json
import sqlite3
import sys
from dataclasses import dataclass, field
from pathlib import Path


# -- Data models ------------------------------------------------------

@dataclass
class SessionStats:
    id: str
    label: str | None
    branch: str | None
    event_count: int
    first_event: str | None
    last_event: str | None


@dataclass
class SubtypeCount:
    subtype: str
    count: int


@dataclass
class PatternCount:
    pattern_type: str
    count: int


@dataclass
class StoreReport:
    total_events: int = 0
    total_sessions: int = 0
    total_patterns: int = 0
    sessions: list[SessionStats] = field(default_factory=list)
    subtype_distribution: list[SubtypeCount] = field(default_factory=list)
    pattern_distribution: list[PatternCount] = field(default_factory=list)
    tool_distribution: list[SubtypeCount] = field(default_factory=list)
    events_per_hour: list[tuple[str, int]] = field(default_factory=list)


# -- Queries ----------------------------------------------------------

def connect(db_path: Path) -> sqlite3.Connection:
    if not db_path.exists():
        print(f"Database not found: {db_path}", file=sys.stderr)
        sys.exit(1)
    conn = sqlite3.connect(str(db_path))
    conn.row_factory = sqlite3.Row
    return conn


def query_report(conn: sqlite3.Connection) -> StoreReport:
    report = StoreReport()

    # Totals
    report.total_events = conn.execute("SELECT COUNT(*) FROM events").fetchone()[0]
    report.total_sessions = conn.execute("SELECT COUNT(*) FROM sessions").fetchone()[0]
    report.total_patterns = conn.execute("SELECT COUNT(*) FROM patterns").fetchone()[0]

    # Sessions
    rows = conn.execute(
        "SELECT id, label, branch, event_count, first_event, last_event "
        "FROM sessions ORDER BY last_event DESC"
    ).fetchall()
    report.sessions = [
        SessionStats(
            id=r["id"], label=r["label"], branch=r["branch"],
            event_count=r["event_count"],
            first_event=r["first_event"], last_event=r["last_event"],
        )
        for r in rows
    ]

    # Subtype distribution
    rows = conn.execute(
        "SELECT subtype, COUNT(*) as count FROM events "
        "GROUP BY subtype ORDER BY count DESC"
    ).fetchall()
    report.subtype_distribution = [
        SubtypeCount(subtype=r["subtype"], count=r["count"]) for r in rows
    ]

    # Pattern distribution
    rows = conn.execute(
        "SELECT type, COUNT(*) as count FROM patterns "
        "GROUP BY type ORDER BY count DESC"
    ).fetchall()
    report.pattern_distribution = [
        PatternCount(pattern_type=r["type"], count=r["count"]) for r in rows
    ]

    # Tool distribution (extract tool name from tool_use events)
    rows = conn.execute(
        "SELECT json_extract(payload, '$.data.tool') as tool, COUNT(*) as count "
        "FROM events WHERE subtype = 'message.assistant.tool_use' "
        "AND tool IS NOT NULL "
        "GROUP BY tool ORDER BY count DESC"
    ).fetchall()
    report.tool_distribution = [
        SubtypeCount(subtype=r["tool"], count=r["count"]) for r in rows
    ]

    # Events per hour
    rows = conn.execute(
        "SELECT substr(timestamp, 1, 13) as hour, COUNT(*) as count "
        "FROM events WHERE timestamp != '' "
        "GROUP BY hour ORDER BY hour DESC LIMIT 48"
    ).fetchall()
    report.events_per_hour = [(r["hour"], r["count"]) for r in rows]

    return report


# -- Cross-session queries -------------------------------------------

def query_session_synopsis(conn: sqlite3.Connection, session_id: str) -> dict | None:
    """Session synopsis: goal, journey, outcome."""
    row = conn.execute(
        "SELECT id, label, project_id, project_name, event_count, first_event, last_event "
        "FROM sessions WHERE id = ?", (session_id,)
    ).fetchone()
    if not row:
        return None

    tool_count = conn.execute(
        "SELECT COUNT(*) FROM events WHERE session_id = ? AND subtype = 'message.assistant.tool_use'",
        (session_id,)
    ).fetchone()[0]

    error_count = conn.execute(
        "SELECT COUNT(*) FROM events WHERE session_id = ? AND subtype = 'system.error'",
        (session_id,)
    ).fetchone()[0]

    top_tools = conn.execute(
        "SELECT json_extract(payload, '$.data.tool') as tool, COUNT(*) as cnt "
        "FROM events WHERE session_id = ? AND subtype = 'message.assistant.tool_use' "
        "AND tool IS NOT NULL GROUP BY tool ORDER BY cnt DESC LIMIT 5",
        (session_id,)
    ).fetchall()

    return {
        "session_id": row["id"],
        "label": row["label"],
        "project_id": row["project_id"],
        "project_name": row["project_name"],
        "event_count": row["event_count"],
        "tool_count": tool_count,
        "error_count": error_count,
        "first_event": row["first_event"],
        "last_event": row["last_event"],
        "top_tools": [{"tool": t["tool"], "count": t["cnt"]} for t in top_tools],
    }


def query_project_pulse(conn: sqlite3.Connection, days: int = 7) -> list[dict]:
    """Project activity pulse: events per project in the last N days."""
    from datetime import datetime, timedelta, timezone
    cutoff = (datetime.now(timezone.utc) - timedelta(days=days)).isoformat()
    rows = conn.execute(
        "SELECT project_id, project_name, COUNT(DISTINCT id) as session_count, "
        "SUM(event_count) as total_events, MAX(last_event) as last_activity "
        "FROM sessions WHERE project_id IS NOT NULL AND last_event >= ? "
        "GROUP BY project_id ORDER BY total_events DESC",
        (cutoff,)
    ).fetchall()
    return [
        {"project_id": r["project_id"], "project_name": r["project_name"],
         "session_count": r["session_count"], "event_count": r["total_events"],
         "last_activity": r["last_activity"]}
        for r in rows
    ]


def query_project_context(conn: sqlite3.Connection, project_id: str, limit: int = 5) -> list[dict]:
    """Project context: recent sessions for a project."""
    rows = conn.execute(
        "SELECT id, label, event_count, first_event, last_event "
        "FROM sessions WHERE project_id = ? ORDER BY last_event DESC LIMIT ?",
        (project_id, limit)
    ).fetchall()
    return [
        {"session_id": r["id"], "label": r["label"], "event_count": r["event_count"],
         "first_event": r["first_event"], "last_event": r["last_event"]}
        for r in rows
    ]


def query_file_impact(conn: sqlite3.Connection, session_id: str) -> list[dict]:
    """File impact: files read vs. written."""
    rows = conn.execute(
        "SELECT COALESCE("
        "  json_extract(payload, '$.data.args.file_path'),"
        "  json_extract(payload, '$.data.args.file'),"
        "  json_extract(payload, '$.data.args.path')"
        ") as target, json_extract(payload, '$.data.tool') as tool, COUNT(*) as cnt "
        "FROM events WHERE session_id = ? AND subtype = 'message.assistant.tool_use' "
        "AND target IS NOT NULL GROUP BY target, tool ORDER BY target",
        (session_id,)
    ).fetchall()

    impacts: dict[str, dict] = {}
    for r in rows:
        target = r["target"]
        if target not in impacts:
            impacts[target] = {"file": target, "reads": 0, "writes": 0}
        if r["tool"] in ("Read", "Glob", "Grep"):
            impacts[target]["reads"] += r["cnt"]
        elif r["tool"] in ("Edit", "Write", "NotebookEdit"):
            impacts[target]["writes"] += r["cnt"]

    return sorted(impacts.values(), key=lambda x: x["reads"] + x["writes"], reverse=True)


def query_productivity_by_hour(conn: sqlite3.Connection, days: int = 30) -> list[dict]:
    """Productivity by hour of day."""
    from datetime import datetime, timedelta, timezone
    cutoff = (datetime.now(timezone.utc) - timedelta(days=days)).isoformat()
    rows = conn.execute(
        "SELECT CAST(strftime('%H', timestamp) AS INTEGER) as hour, COUNT(*) as cnt "
        "FROM events WHERE timestamp >= ? GROUP BY hour ORDER BY hour",
        (cutoff,)
    ).fetchall()
    return [{"hour": r["hour"], "event_count": r["cnt"]} for r in rows]


# -- Display ----------------------------------------------------------

def print_report(report: StoreReport) -> None:
    print(f"\n{'=' * 60}")
    print(f"  open-story Event Store")
    print(f"{'=' * 60}")
    print(f"  Events:   {report.total_events:,}")
    print(f"  Sessions: {report.total_sessions}")
    print(f"  Patterns: {report.total_patterns}")
    print()

    # Sessions table
    print(f"{'-' * 60}")
    print("  Sessions (most recent first)")
    print(f"{'-' * 60}")
    print(f"  {'Events':>6}  {'Label':<40}  {'Branch'}")
    for s in report.sessions:
        label = (s.label or "(no label)")[:40]
        branch = s.branch or ""
        print(f"  {s.event_count:>6}  {label:<40}  {branch}")
    print()

    # Subtype distribution
    print(f"{'-' * 60}")
    print("  Event subtypes")
    print(f"{'-' * 60}")
    for st in report.subtype_distribution[:15]:
        bar = "#" * min(50, st.count // max(1, report.total_events // 50))
        print(f"  {st.count:>6}  {st.subtype:<40}  {bar}")
    print()

    # Tool distribution
    if report.tool_distribution:
        print(f"{'-' * 60}")
        print("  Tool calls")
        print(f"{'-' * 60}")
        total_tools = sum(t.count for t in report.tool_distribution)
        for t in report.tool_distribution[:15]:
            pct = t.count / total_tools * 100 if total_tools > 0 else 0
            bar = "#" * int(pct / 2)
            print(f"  {t.count:>6}  ({pct:4.1f}%)  {t.subtype:<20}  {bar}")
        print()

    # Pattern distribution
    if report.pattern_distribution:
        print(f"{'-' * 60}")
        print("  Detected patterns")
        print(f"{'-' * 60}")
        for p in report.pattern_distribution:
            print(f"  {p.count:>6}  {p.pattern_type}")
        print()

    # Activity timeline (last 24h)
    if report.events_per_hour:
        print(f"{'-' * 60}")
        print("  Activity (events per hour, last 48h)")
        print(f"{'-' * 60}")
        max_count = max(c for _, c in report.events_per_hour) if report.events_per_hour else 1
        for hour, count in report.events_per_hour[:24]:
            bar = "#" * int(count / max_count * 40) if max_count > 0 else ""
            print(f"  {hour}  {count:>5}  {bar}")
        print()


def print_json(report: StoreReport) -> None:
    data = {
        "total_events": report.total_events,
        "total_sessions": report.total_sessions,
        "total_patterns": report.total_patterns,
        "sessions": [
            {"id": s.id, "label": s.label, "branch": s.branch,
             "event_count": s.event_count, "first_event": s.first_event,
             "last_event": s.last_event}
            for s in report.sessions
        ],
        "subtype_distribution": [
            {"subtype": st.subtype, "count": st.count}
            for st in report.subtype_distribution
        ],
        "tool_distribution": [
            {"tool": t.subtype, "count": t.count}
            for t in report.tool_distribution
        ],
        "pattern_distribution": [
            {"type": p.pattern_type, "count": p.count}
            for p in report.pattern_distribution
        ],
    }
    print(json.dumps(data, indent=2))


# -- Tests ------------------------------------------------------------

def run_tests():
    """Self-tests using an in-memory SQLite database."""
    import tempfile
    from datetime import datetime, timezone
    # Use today's date for test data so time-windowed queries always match
    TODAY = datetime.now(timezone.utc).strftime("%Y-%m-%d")

    def sql(s: str) -> str:
        return s.replace("__TODAY__", TODAY)

    print("Running tests...")

    # Test 1: empty database
    conn = sqlite3.connect(":memory:")
    conn.row_factory = sqlite3.Row
    conn.executescript(sql("""
        CREATE TABLE events (id TEXT PRIMARY KEY, session_id TEXT, subtype TEXT, timestamp TEXT, payload TEXT);
        CREATE TABLE sessions (id TEXT PRIMARY KEY, label TEXT, branch TEXT, event_count INTEGER DEFAULT 0, first_event TEXT, last_event TEXT, project_id TEXT, project_name TEXT);
        CREATE TABLE patterns (id TEXT PRIMARY KEY, session_id TEXT, type TEXT, start_time TEXT, end_time TEXT, summary TEXT, event_ids TEXT DEFAULT '[]', metadata TEXT);
    """))
    report = query_report(conn)
    assert report.total_events == 0, f"expected 0 events, got {report.total_events}"
    assert report.total_sessions == 0
    assert report.total_patterns == 0
    assert report.sessions == []
    assert report.subtype_distribution == []
    print("  OK: empty database returns zeros")

    # Test 2: populated database
    conn.executescript(sql("""
        INSERT INTO events VALUES ('e1', 's1', 'message.user.prompt', '__TODAY__T10:00:00Z', '{"data":{"text":"hello"}}');
        INSERT INTO events VALUES ('e2', 's1', 'message.assistant.tool_use', '__TODAY__T10:00:01Z', '{"data":{"tool":"Bash"}}');
        INSERT INTO events VALUES ('e3', 's1', 'message.assistant.tool_use', '__TODAY__T10:00:02Z', '{"data":{"tool":"Edit"}}');
        INSERT INTO events VALUES ('e4', 's1', 'message.user.tool_result', '__TODAY__T10:00:03Z', '{"data":{}}');
        INSERT INTO sessions VALUES ('s1', 'fix auth bug', 'main', 4, '__TODAY__T10:00:00Z', '__TODAY__T10:00:03Z', NULL, NULL);
        INSERT INTO patterns VALUES ('p1', 's1', 'test.cycle', '__TODAY__T10:00:00Z', '__TODAY__T10:00:03Z', 'PASS', '[]', '{}');
        INSERT INTO patterns VALUES ('p2', 's1', 'turn.phase', '__TODAY__T10:00:00Z', '__TODAY__T10:00:03Z', 'impl', '[]', '{}');
    """))
    report = query_report(conn)
    assert report.total_events == 4, f"expected 4 events, got {report.total_events}"
    assert report.total_sessions == 1
    assert report.total_patterns == 2
    assert len(report.sessions) == 1
    assert report.sessions[0].label == "fix auth bug"
    assert report.sessions[0].event_count == 4
    print("  OK: populated database returns correct counts")

    # Test 3: subtype distribution
    subtypes = {st.subtype: st.count for st in report.subtype_distribution}
    assert subtypes.get("message.assistant.tool_use") == 2
    assert subtypes.get("message.user.prompt") == 1
    assert subtypes.get("message.user.tool_result") == 1
    print("  OK: subtype distribution correct")

    # Test 4: tool distribution
    tools = {t.subtype: t.count for t in report.tool_distribution}
    assert tools.get("Bash") == 1, f"expected Bash=1, got {tools}"
    assert tools.get("Edit") == 1, f"expected Edit=1, got {tools}"
    print("  OK: tool distribution correct")

    # Test 5: pattern distribution
    patterns = {p.pattern_type: p.count for p in report.pattern_distribution}
    assert patterns.get("test.cycle") == 1
    assert patterns.get("turn.phase") == 1
    print("  OK: pattern distribution correct")

    # Test 6: events per hour
    assert len(report.events_per_hour) == 1
    assert report.events_per_hour[0][0] == f"{TODAY}T10"
    assert report.events_per_hour[0][1] == 4
    print("  OK: events per hour correct")

    # Test 7: multiple sessions ordered by last_event desc
    conn.executescript(sql("""
        INSERT INTO events VALUES ('e5', 's2', 'message.user.prompt', '__TODAY__T12:00:00Z', '{"data":{"text":"newer"}}');
        INSERT INTO sessions VALUES ('s2', 'newer session', 'feature', 1, '__TODAY__T12:00:00Z', '__TODAY__T12:00:00Z', NULL, NULL);
    """))
    report = query_report(conn)
    assert report.sessions[0].label == "newer session", "most recent session should be first"
    assert report.sessions[1].label == "fix auth bug"
    print("  OK: sessions ordered by most recent")

    # Test 8: json output doesn't crash
    import io
    old_stdout = sys.stdout
    sys.stdout = io.StringIO()
    print_json(report)
    output = sys.stdout.getvalue()
    sys.stdout = old_stdout
    parsed = json.loads(output)
    assert parsed["total_events"] == 5
    assert len(parsed["sessions"]) == 2
    print("  OK: JSON output is valid")

    conn.close()
    print(f"  OK: 8 original tests passed")

    # Test 9: session synopsis
    conn = sqlite3.connect(":memory:")
    conn.row_factory = sqlite3.Row
    conn.executescript(sql("""
        CREATE TABLE events (id TEXT PRIMARY KEY, session_id TEXT, subtype TEXT, timestamp TEXT, payload TEXT);
        CREATE TABLE sessions (id TEXT PRIMARY KEY, label TEXT, branch TEXT, event_count INTEGER DEFAULT 0, first_event TEXT, last_event TEXT, project_id TEXT, project_name TEXT);
        CREATE TABLE patterns (id TEXT PRIMARY KEY, session_id TEXT, type TEXT, start_time TEXT, end_time TEXT, summary TEXT, event_ids TEXT DEFAULT '[]', metadata TEXT);
        INSERT INTO events VALUES ('e1', 's1', 'message.assistant.tool_use', '__TODAY__T10:00:00Z', '{"data":{"tool":"Read"}}');
        INSERT INTO events VALUES ('e2', 's1', 'message.assistant.tool_use', '__TODAY__T10:00:01Z', '{"data":{"tool":"Edit"}}');
        INSERT INTO events VALUES ('e3', 's1', 'system.error', '__TODAY__T10:00:02Z', '{"data":{"text":"oops"}}');
        INSERT INTO sessions VALUES ('s1', 'fix bug', 'main', 3, '__TODAY__T10:00:00Z', '__TODAY__T11:00:00Z', 'my-proj', 'My Project');
    """))
    synopsis = query_session_synopsis(conn, "s1")
    assert synopsis is not None
    assert synopsis["session_id"] == "s1"
    assert synopsis["tool_count"] == 2
    assert synopsis["error_count"] == 1
    assert synopsis["label"] == "fix bug"
    assert synopsis["project_id"] == "my-proj"
    assert len(synopsis["top_tools"]) == 2
    print("  OK: session synopsis query correct")

    # Test 10: synopsis returns None for missing session
    assert query_session_synopsis(conn, "nonexistent") is None
    print("  OK: synopsis returns None for missing session")

    # Test 11: project pulse
    pulse = query_project_pulse(conn, days=365)
    assert len(pulse) == 1
    assert pulse[0]["project_id"] == "my-proj"
    assert pulse[0]["session_count"] == 1
    print("  OK: project pulse query correct")

    # Test 12: project context
    ctx = query_project_context(conn, "my-proj")
    assert len(ctx) == 1
    assert ctx[0]["session_id"] == "s1"
    assert ctx[0]["label"] == "fix bug"
    print("  OK: project context query correct")

    # Test 13: file impact
    conn.executescript(sql("""
        INSERT INTO events VALUES ('fi1', 's1', 'message.assistant.tool_use', '__TODAY__T10:00:03Z', '{"data":{"tool":"Read","args":{"file_path":"src/lib.rs"}}}');
        INSERT INTO events VALUES ('fi2', 's1', 'message.assistant.tool_use', '__TODAY__T10:00:04Z', '{"data":{"tool":"Edit","args":{"file_path":"src/lib.rs"}}}');
    """))
    impact = query_file_impact(conn, "s1")
    lib_impact = [i for i in impact if i["file"] == "src/lib.rs"]
    assert len(lib_impact) == 1
    assert lib_impact[0]["reads"] == 1
    assert lib_impact[0]["writes"] == 1
    print("  OK: file impact query correct")

    conn.close()
    print(f"\nAll 13 tests passed.")


# -- Main -------------------------------------------------------------

if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="Query the open-story SQLite event store")
    parser.add_argument("--data-dir", default="./data", help="Path to data directory")
    parser.add_argument("--format", choices=["text", "json"], default="text", help="Output format")
    parser.add_argument("--test", action="store_true", help="Run self-tests")

    sub = parser.add_subparsers(dest="command")

    # Synopsis subcommand
    syn_parser = sub.add_parser("synopsis", help="Session synopsis")
    syn_parser.add_argument("session_id", help="Session ID")

    # Pulse subcommand
    pulse_parser = sub.add_parser("pulse", help="Project activity pulse")
    pulse_parser.add_argument("--days", type=int, default=7, help="Days to look back")

    # Context subcommand
    ctx_parser = sub.add_parser("context", help="Project context")
    ctx_parser.add_argument("project", help="Project ID")

    # Impact subcommand
    impact_parser = sub.add_parser("impact", help="File impact for a session")
    impact_parser.add_argument("session_id", help="Session ID")

    args = parser.parse_args()

    if args.test:
        run_tests()
        sys.exit(0)

    db_path = Path(args.data_dir) / "open-story.db"
    conn = connect(db_path)

    if args.command == "synopsis":
        result = query_session_synopsis(conn, args.session_id)
        if result is None:
            print(f"Session not found: {args.session_id}", file=sys.stderr)
            sys.exit(1)
        if args.format == "json":
            print(json.dumps(result, indent=2))
        else:
            print(f"Session: {result['session_id']}")
            if result["label"]:
                print(f"Label:   {result['label']}")
            if result["project_name"]:
                print(f"Project: {result['project_name']}")
            print(f"Events:  {result['event_count']}")
            print(f"Tools:   {result['tool_count']}")
            print(f"Errors:  {result['error_count']}")
            if result["top_tools"]:
                print("\nTop tools:")
                for t in result["top_tools"]:
                    print(f"  {t['tool']:<12} {t['count']}")

    elif args.command == "pulse":
        pulse = query_project_pulse(conn, args.days)
        if args.format == "json":
            print(json.dumps(pulse, indent=2))
        elif not pulse:
            print(f"No activity in the last {args.days} days.")
        else:
            print(f"{'Project':<30} {'Sessions':>8} {'Events':>8}  Last active")
            print("-" * 70)
            for p in pulse:
                name = p["project_name"] or p["project_id"]
                last = (p["last_activity"] or "?")[:10]
                print(f"{name:<30} {p['session_count']:>8} {p['event_count']:>8}  {last}")

    elif args.command == "context":
        ctx = query_project_context(conn, args.project)
        if args.format == "json":
            print(json.dumps(ctx, indent=2))
        elif not ctx:
            print(f"No sessions found for project: {args.project}")
        else:
            print(f"Recent sessions for \"{args.project}\":\n")
            for s in ctx:
                label = s["label"] or "(no label)"
                last = (s["last_event"] or "?")[:19]
                print(f"  {last} | {s['event_count']} events | {label}")

    elif args.command == "impact":
        impact = query_file_impact(conn, args.session_id)
        if args.format == "json":
            print(json.dumps(impact, indent=2))
        elif not impact:
            print(f"No file operations found for session: {args.session_id}")
        else:
            print(f"{'File':<50} {'Reads':>6} {'Writes':>6}")
            print("-" * 65)
            for f in impact:
                print(f"{f['file']:<50} {f['reads']:>6} {f['writes']:>6}")

    else:
        # Default: full report
        report = query_report(conn)
        if args.format == "json":
            print_json(report)
        else:
            print_report(report)

    conn.close()
