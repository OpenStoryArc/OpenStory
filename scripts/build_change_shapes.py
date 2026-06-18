#!/usr/bin/env python3
# /// script
# requires-python = ">=3.10"
# ///
"""Build the change-shape semantic layer.

For every Edit / Write / MultiEdit tool_call, extract the actual delta:
lines added/removed, chars added/removed, and a 200-char excerpt of new_string
for grep-ability.

  Edit       : payload.input.old_string → removed, new_string → added
  Write      : payload.input.content    → added (all of it; old is unknown)
  MultiEdit  : payload.input.edits[]    → summed across all edits

Idempotent on event_id.

Usage:
    uv run scripts/build_change_shapes.py
    uv run scripts/build_change_shapes.py --limit 200 --rebuild

Design: docs/research/semantic-layer.md ("shape layers" family)
"""

from __future__ import annotations

import argparse
import json
import sqlite3
import sys
import time
import urllib.request
from dataclasses import dataclass
from pathlib import Path

API = "http://localhost:3002/api"
DEFAULT_DB = "data/change_shapes.db"

CHANGE_TOOLS = {"Edit", "Write", "MultiEdit"}

SCHEMA = """
CREATE TABLE IF NOT EXISTS change_shapes (
    event_id        TEXT PRIMARY KEY,
    session_id      TEXT NOT NULL,
    timestamp       TEXT NOT NULL,
    seq             INTEGER NOT NULL,
    tool            TEXT NOT NULL,
    path            TEXT NOT NULL,
    edit_count      INTEGER NOT NULL,
    lines_added     INTEGER NOT NULL,
    lines_removed   INTEGER NOT NULL,
    chars_added     INTEGER NOT NULL,
    chars_removed   INTEGER NOT NULL,
    new_excerpt     TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_change_session ON change_shapes(session_id);
CREATE INDEX IF NOT EXISTS idx_change_path    ON change_shapes(path);
CREATE INDEX IF NOT EXISTS idx_change_tool    ON change_shapes(tool);
"""


@dataclass
class ChangeEvent:
    event_id: str
    session_id: str
    timestamp: str
    seq: int
    tool: str
    path: str
    edit_count: int
    lines_added: int
    lines_removed: int
    chars_added: int
    chars_removed: int
    new_excerpt: str


def fetch_json(path: str) -> object:
    with urllib.request.urlopen(f"{API}{path}", timeout=20) as r:
        return json.loads(r.read())


def count_lines(s: str) -> int:
    if not s:
        return 0
    # treat each \n as a line separator; a non-empty string with no \n is 1 line
    n = s.count("\n")
    if s and not s.endswith("\n"):
        n += 1
    return n


def iter_change_events(session_id: str):
    try:
        data = fetch_json(f"/sessions/{session_id}/records")
    except Exception as e:
        print(f"  warn: failed to fetch {session_id}: {e}", file=sys.stderr)
        return
    records = data if isinstance(data, list) else data.get("records", [])
    for rec in records:
        if rec.get("record_type") != "tool_call":
            continue
        payload = rec.get("payload") or {}
        tool = payload.get("name", "")
        if tool not in CHANGE_TOOLS:
            continue
        inp = payload.get("input") or {}
        if not isinstance(inp, dict):
            continue

        path = inp.get("file_path") or inp.get("notebook_path") or ""
        if not isinstance(path, str) or not path.strip():
            continue

        added = removed = 0
        chars_a = chars_r = 0
        edit_count = 1
        excerpt = ""

        if tool == "Edit":
            old = inp.get("old_string") or ""
            new = inp.get("new_string") or ""
            removed = count_lines(old)
            added = count_lines(new)
            chars_r = len(old)
            chars_a = len(new)
            excerpt = new[:200]
        elif tool == "Write":
            content = inp.get("content") or ""
            added = count_lines(content)
            chars_a = len(content)
            excerpt = content[:200]
        elif tool == "MultiEdit":
            edits = inp.get("edits") or []
            if isinstance(edits, list):
                edit_count = len(edits)
                for e in edits:
                    if not isinstance(e, dict):
                        continue
                    old = e.get("old_string") or ""
                    new = e.get("new_string") or ""
                    removed += count_lines(old)
                    added += count_lines(new)
                    chars_r += len(old)
                    chars_a += len(new)
                # excerpt: first edit's new_string
                if edits and isinstance(edits[0], dict):
                    excerpt = (edits[0].get("new_string") or "")[:200]

        yield ChangeEvent(
            event_id=rec.get("id", ""),
            session_id=session_id,
            timestamp=rec.get("timestamp", ""),
            seq=int(rec.get("seq") or 0),
            tool=tool,
            path=path.strip(),
            edit_count=edit_count,
            lines_added=added,
            lines_removed=removed,
            chars_added=chars_a,
            chars_removed=chars_r,
            new_excerpt=excerpt,
        )


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--db", default=DEFAULT_DB)
    ap.add_argument("--limit", type=int, default=0)
    ap.add_argument("--rebuild", action="store_true")
    args = ap.parse_args()

    db_path = Path(args.db)
    db_path.parent.mkdir(parents=True, exist_ok=True)
    conn = sqlite3.connect(db_path)
    if args.rebuild:
        conn.executescript("DROP TABLE IF EXISTS change_shapes;")
    conn.executescript(SCHEMA)
    conn.commit()

    existing = {r[0] for r in conn.execute("SELECT event_id FROM change_shapes")}
    print(f"existing change shapes: {len(existing)}", file=sys.stderr)

    sd = fetch_json("/sessions")
    sessions = sd if isinstance(sd, list) else sd.get("sessions", [])
    sessions.sort(key=lambda s: s.get("start_time", ""), reverse=True)
    if args.limit:
        sessions = sessions[: args.limit]

    print(f"scanning {len(sessions)} sessions for changes...", file=sys.stderr)
    t0 = time.monotonic()
    inserted = 0
    batch: list[tuple] = []
    for i, s in enumerate(sessions, 1):
        sid = s["session_id"]
        if i % 25 == 0:
            print(f"  scanned {i}/{len(sessions)} sessions, inserted {inserted}", file=sys.stderr)
        for ev in iter_change_events(sid):
            if ev.event_id in existing or not ev.event_id:
                continue
            batch.append((
                ev.event_id, ev.session_id, ev.timestamp, ev.seq, ev.tool, ev.path,
                ev.edit_count, ev.lines_added, ev.lines_removed,
                ev.chars_added, ev.chars_removed, ev.new_excerpt,
            ))
            existing.add(ev.event_id)
            if len(batch) >= 500:
                conn.executemany(
                    """INSERT OR IGNORE INTO change_shapes
                       (event_id, session_id, timestamp, seq, tool, path,
                        edit_count, lines_added, lines_removed,
                        chars_added, chars_removed, new_excerpt)
                       VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)""",
                    batch,
                )
                conn.commit()
                inserted += len(batch)
                batch.clear()
    if batch:
        conn.executemany(
            """INSERT OR IGNORE INTO change_shapes
               (event_id, session_id, timestamp, seq, tool, path,
                edit_count, lines_added, lines_removed,
                chars_added, chars_removed, new_excerpt)
               VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)""",
            batch,
        )
        conn.commit()
        inserted += len(batch)

    elapsed = time.monotonic() - t0
    print(f"inserted {inserted} change shapes in {elapsed:.1f}s", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
