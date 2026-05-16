#!/usr/bin/env python3
# /// script
# requires-python = ">=3.10"
# ///
"""Build the snapshot-shape layer.

For each `file_snapshot` record, capture working-set metadata (no content —
the snapshots are a manifest, not a content store):

  - tracked_count:   total files in the tracking manifest at this moment
  - new_files:       files that appeared since the previous snapshot in this session
  - bumped_files:    files whose `version` changed since the previous snapshot
  - max_version:     highest tracked-file version at this moment

This gives a deterministic "working memory" timeline per session — when did
each file enter the tracking set, how often does the version progress, and
how does working-set size grow.

Idempotent on event_id.

Usage:
    uv run scripts/build_snapshot_shapes.py
    uv run scripts/build_snapshot_shapes.py --limit 200 --rebuild

Design: docs/research/semantic-layer.md
"""

from __future__ import annotations

import argparse
import json
import sqlite3
import sys
import time
import urllib.request
from pathlib import Path

API = "http://localhost:3002/api"
DEFAULT_DB = "data/snapshot_shapes.db"

SCHEMA = """
CREATE TABLE IF NOT EXISTS snapshot_shapes (
    event_id          TEXT PRIMARY KEY,
    session_id        TEXT NOT NULL,
    timestamp         TEXT NOT NULL,
    seq               INTEGER NOT NULL,
    tracked_count     INTEGER NOT NULL,
    new_files         INTEGER NOT NULL,
    bumped_files      INTEGER NOT NULL,
    max_version       INTEGER NOT NULL,
    new_paths         TEXT NOT NULL,   -- JSON list (capped to 50 entries)
    bumped_paths      TEXT NOT NULL    -- JSON list (capped to 50 entries)
);
CREATE INDEX IF NOT EXISTS idx_snap_session ON snapshot_shapes(session_id);
"""


def fetch_json(path: str) -> object:
    with urllib.request.urlopen(f"{API}{path}", timeout=20) as r:
        return json.loads(r.read())


def extract_backups(rec) -> dict:
    payload = rec.get("payload") or {}
    tf = payload.get("tracked_files", {})
    if not isinstance(tf, dict):
        return {}
    backups = tf.get("trackedFileBackups", {})
    return backups if isinstance(backups, dict) else {}


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
        conn.executescript("DROP TABLE IF EXISTS snapshot_shapes;")
    conn.executescript(SCHEMA)
    conn.commit()

    existing = {r[0] for r in conn.execute("SELECT event_id FROM snapshot_shapes")}
    print(f"existing snapshot shapes: {len(existing)}", file=sys.stderr)

    sd = fetch_json("/sessions")
    sessions = sd if isinstance(sd, list) else sd.get("sessions", [])
    sessions.sort(key=lambda s: s.get("start_time", ""), reverse=True)
    if args.limit:
        sessions = sessions[: args.limit]

    print(f"scanning {len(sessions)} sessions for file_snapshots...", file=sys.stderr)
    t0 = time.monotonic()
    inserted = 0
    batch: list[tuple] = []
    for i, s in enumerate(sessions, 1):
        sid = s["session_id"]
        if i % 25 == 0:
            print(f"  scanned {i}/{len(sessions)} sessions, inserted {inserted}", file=sys.stderr)
        try:
            data = fetch_json(f"/sessions/{sid}/records")
        except Exception as e:
            print(f"  warn: {sid}: {e}", file=sys.stderr)
            continue
        records = data if isinstance(data, list) else data.get("records", [])
        snaps = [r for r in records if r.get("record_type") == "file_snapshot"]
        snaps.sort(key=lambda r: (r.get("timestamp", ""), r.get("seq", 0)))

        prev_versions: dict[str, int] = {}
        for snap in snaps:
            eid = snap.get("id", "")
            if not eid or eid in existing:
                continue
            backups = extract_backups(snap)
            curr_versions: dict[str, int] = {}
            for path, info in backups.items():
                if not isinstance(info, dict):
                    continue
                v = info.get("version") or 0
                if isinstance(v, (int, float)):
                    curr_versions[path] = int(v)

            new_paths = [p for p in curr_versions if p not in prev_versions]
            bumped_paths = [p for p in curr_versions
                            if p in prev_versions and curr_versions[p] != prev_versions[p]]
            max_v = max(curr_versions.values()) if curr_versions else 0

            batch.append((
                eid, sid,
                snap.get("timestamp", ""),
                int(snap.get("seq") or 0),
                len(curr_versions),
                len(new_paths),
                len(bumped_paths),
                max_v,
                json.dumps(new_paths[:50]),
                json.dumps(bumped_paths[:50]),
            ))
            existing.add(eid)
            prev_versions = curr_versions

            if len(batch) >= 500:
                conn.executemany(
                    """INSERT OR IGNORE INTO snapshot_shapes
                       (event_id, session_id, timestamp, seq, tracked_count,
                        new_files, bumped_files, max_version, new_paths, bumped_paths)
                       VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)""",
                    batch,
                )
                conn.commit()
                inserted += len(batch)
                batch.clear()
    if batch:
        conn.executemany(
            """INSERT OR IGNORE INTO snapshot_shapes
               (event_id, session_id, timestamp, seq, tracked_count,
                new_files, bumped_files, max_version, new_paths, bumped_paths)
               VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)""",
            batch,
        )
        conn.commit()
        inserted += len(batch)

    elapsed = time.monotonic() - t0
    print(f"inserted {inserted} snapshot shapes in {elapsed:.1f}s", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
