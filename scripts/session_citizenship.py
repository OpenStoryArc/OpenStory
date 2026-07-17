#!/usr/bin/env python3
"""Session citizenship: disk vs store vs watcher — OpenStory sovereignty health.

Emerged from a self-reflective Grok loop that discovered "ghost" sessions:
updates.jsonl growing under ~/.grok/sessions, watcher publishing to NATS with
success=true, but zero rows in SQLite / MCP synopsis not found.

Root cause class (see also bus reconnect fix): Live stream (disk + watcher)
and Explore atom (store) can diverge. This script *looks at the data* before
you build another abstraction.

Usage:
    python3 scripts/session_citizenship.py
    python3 scripts/session_citizenship.py --session 019f71cd-6bc2-7340-b633-3d2aecc507d2
    python3 scripts/session_citizenship.py --agent grok --api http://127.0.0.1:3002
    python3 scripts/session_citizenship.py --test
"""

from __future__ import annotations

import argparse
import json
import sqlite3
import sys
import urllib.error
import urllib.request
from dataclasses import asdict, dataclass, field
from pathlib import Path
from typing import Any


DEFAULT_API = "http://127.0.0.1:3002"
DEFAULT_DB = Path("data/open-story.db")
DEFAULT_GROK_ROOT = Path.home() / ".grok" / "sessions"


@dataclass
class CitizenshipReport:
    session_id: str
    on_disk: bool = False
    disk_path: str | None = None
    updates_bytes: int = 0
    updates_lines: int = 0
    in_store: bool = False
    store_event_count: int = 0
    store_last_event: str | None = None
    watcher_last_path: str | None = None
    watcher_last_event_at: str | None = None
    watcher_publish_failures: int = 0
    watcher_cloud_events_emitted: int = 0
    api_reachable: bool = False
    verdict: str = "unknown"
    notes: list[str] = field(default_factory=list)

    def to_dict(self) -> dict[str, Any]:
        return asdict(self)


def http_json(url: str, timeout: float = 5.0) -> Any | None:
    try:
        with urllib.request.urlopen(url, timeout=timeout) as resp:
            return json.loads(resp.read().decode())
    except (urllib.error.URLError, urllib.error.HTTPError, TimeoutError, json.JSONDecodeError):
        return None


def find_disk_session(session_id: str, grok_root: Path) -> Path | None:
    if not grok_root.is_dir():
        return None
    # Grok layout: ~/.grok/sessions/<project-encoded>/<session-id>/updates.jsonl
    for path in grok_root.rglob(session_id):
        if path.is_dir() and (path / "updates.jsonl").is_file():
            return path
    return None


def store_counts(db_path: Path, session_id: str) -> tuple[int, str | None]:
    if not db_path.is_file():
        return 0, None
    con = sqlite3.connect(str(db_path))
    try:
        n = con.execute(
            "SELECT COUNT(*) FROM events WHERE session_id = ?", (session_id,)
        ).fetchone()[0]
        row = con.execute(
            "SELECT last_event FROM sessions WHERE id = ?", (session_id,)
        ).fetchone()
        last = row[0] if row else None
        return int(n), last
    except sqlite3.Error:
        return 0, None
    finally:
        con.close()


def watcher_hints(api_base: str, session_id: str) -> dict[str, Any]:
    data = http_json(f"{api_base.rstrip('/')}/api/watchers")
    out: dict[str, Any] = {
        "api_reachable": data is not None,
        "last_path": None,
        "last_event_at": None,
        "publish_failures": 0,
        "cloud_events_emitted": 0,
        "agent": None,
    }
    if not isinstance(data, dict):
        return out
    for w in data.get("watchers") or []:
        if not isinstance(w, dict):
            continue
        path = w.get("last_processed_path") or ""
        counters = w.get("counters") or {}
        if session_id in path or session_id in json.dumps(w.get("recent") or []):
            out["last_path"] = path or None
            out["last_event_at"] = w.get("last_event_at")
            out["publish_failures"] = int(counters.get("publish_failures") or 0)
            out["cloud_events_emitted"] = int(counters.get("cloud_events_emitted") or 0)
            out["agent"] = w.get("agent")
            break
        # Prefer grok watcher totals when scanning fleet
        if w.get("agent") == "grok" and out["agent"] is None:
            out["publish_failures"] = int(counters.get("publish_failures") or 0)
            out["cloud_events_emitted"] = int(counters.get("cloud_events_emitted") or 0)
            out["agent"] = "grok"
            out["last_path"] = path or None
            out["last_event_at"] = w.get("last_event_at")
    return out


def classify(report: CitizenshipReport) -> None:
    """Assign verdict from OpenStory soul: Live (disk) vs Explore (store)."""
    if report.on_disk and report.in_store and report.store_event_count > 0:
        report.verdict = "citizen"
        report.notes.append("Disk and Explore atom agree — sovereignty path is intact.")
    elif report.on_disk and not report.in_store:
        report.verdict = "ghost"
        report.notes.append(
            "Live stream exists on disk but Explore atom is empty. "
            "Often: NATS slow-consumer killed persist subscription during backfill; "
            "watcher still publishes (green diagnostics) while store stays silent."
        )
        if report.watcher_last_path and report.session_id in (report.watcher_last_path or ""):
            report.notes.append(
                "Watcher last_processed_path matches this session — publish path is live."
            )
        if report.watcher_publish_failures:
            report.notes.append(
                f"Watcher recorded {report.watcher_publish_failures} publish_failures."
            )
    elif not report.on_disk and report.in_store:
        report.verdict = "orphan-store"
        report.notes.append("Store has rows but no disk updates.jsonl under grok root.")
    else:
        report.verdict = "absent"
        report.notes.append("Neither disk nor store know this session id.")


def investigate(
    session_id: str,
    *,
    db_path: Path,
    grok_root: Path,
    api_base: str,
) -> CitizenshipReport:
    report = CitizenshipReport(session_id=session_id)
    disk = find_disk_session(session_id, grok_root)
    if disk is not None:
        report.on_disk = True
        report.disk_path = str(disk)
        updates = disk / "updates.jsonl"
        if updates.is_file():
            report.updates_bytes = updates.stat().st_size
            report.updates_lines = sum(1 for _ in updates.open())
    n, last = store_counts(db_path, session_id)
    report.store_event_count = n
    report.store_last_event = last
    report.in_store = n > 0 or last is not None
    wh = watcher_hints(api_base, session_id)
    report.api_reachable = bool(wh.get("api_reachable"))
    report.watcher_last_path = wh.get("last_path")
    report.watcher_last_event_at = wh.get("last_event_at")
    report.watcher_publish_failures = int(wh.get("publish_failures") or 0)
    report.watcher_cloud_events_emitted = int(wh.get("cloud_events_emitted") or 0)
    classify(report)
    return report


def list_recent_grok_disk(grok_root: Path, limit: int = 20) -> list[str]:
    if not grok_root.is_dir():
        return []
    sessions: list[tuple[float, str]] = []
    for updates in grok_root.rglob("updates.jsonl"):
        sid = updates.parent.name
        try:
            mtime = updates.stat().st_mtime
        except OSError:
            continue
        sessions.append((mtime, sid))
    sessions.sort(reverse=True)
    return [sid for _, sid in sessions[:limit]]


def run_self_tests() -> int:
    """Lightweight pure tests (no live OpenStory required)."""
    r = CitizenshipReport(session_id="abc")
    r.on_disk = True
    r.in_store = False
    classify(r)
    assert r.verdict == "ghost", r.verdict

    r2 = CitizenshipReport(session_id="abc")
    r2.on_disk = True
    r2.in_store = True
    r2.store_event_count = 3
    classify(r2)
    assert r2.verdict == "citizen", r2.verdict

    r3 = CitizenshipReport(session_id="abc")
    classify(r3)
    assert r3.verdict == "absent", r3.verdict

    assert consumer_backoff_double(250) == 500
    assert consumer_backoff_double(30_000) == 30_000
    print("session_citizenship.py: all self-tests passed")
    return 0


def consumer_backoff_double(current: int) -> int:
    """Mirror of bus `consumer_reconnect_backoff_ms` for docs/tests."""
    return min(current * 2, 30_000)


def main(argv: list[str] | None = None) -> int:
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--session", help="Session UUID to check")
    p.add_argument("--agent", default="grok", help="Hint only; disk scan uses grok root")
    p.add_argument("--data-dir", type=Path, default=Path("data"))
    p.add_argument("--db", type=Path, default=None, help="Override open-story.db path")
    p.add_argument("--grok-root", type=Path, default=DEFAULT_GROK_ROOT)
    p.add_argument("--api", default=DEFAULT_API)
    p.add_argument("--json", action="store_true", help="Machine-readable output")
    p.add_argument("--test", action="store_true")
    p.add_argument("--limit", type=int, default=10, help="When no --session, check N recent disk sessions")
    args = p.parse_args(argv)

    if args.test:
        return run_self_tests()

    db = args.db or (args.data_dir / "open-story.db")
    ids: list[str]
    if args.session:
        ids = [args.session]
    else:
        ids = list_recent_grok_disk(args.grok_root, args.limit)
        if not ids:
            print("No Grok sessions found under", args.grok_root, file=sys.stderr)
            return 1

    reports = [
        investigate(sid, db_path=db, grok_root=args.grok_root, api_base=args.api)
        for sid in ids
    ]

    if args.json:
        print(json.dumps([r.to_dict() for r in reports], indent=2))
        return 0

    ghosts = sum(1 for r in reports if r.verdict == "ghost")
    citizens = sum(1 for r in reports if r.verdict == "citizen")
    print(f"Session citizenship ({len(reports)} checked) — citizens={citizens} ghosts={ghosts}")
    print(f"  db={db}  grok_root={args.grok_root}  api={args.api}")
    print()
    for r in reports:
        flag = {"citizen": "OK", "ghost": "GHOST", "orphan-store": "ORPHAN", "absent": "ABSENT"}.get(
            r.verdict, r.verdict
        )
        print(f"[{flag:6}] {r.session_id}")
        print(f"         disk={r.on_disk} lines={r.updates_lines} bytes={r.updates_bytes}")
        print(f"         store_events={r.store_event_count} last={r.store_last_event}")
        print(f"         watcher_last_at={r.watcher_last_event_at}")
        for n in r.notes:
            print(f"         · {n}")
        print()
    return 0 if ghosts == 0 else 2


if __name__ == "__main__":
    sys.exit(main())
