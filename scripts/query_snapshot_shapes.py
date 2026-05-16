#!/usr/bin/env python3
# /// script
# requires-python = ">=3.10"
# ///
"""Read the snapshot-shape layer.

    uv run scripts/query_snapshot_shapes.py                   # overview
    uv run scripts/query_snapshot_shapes.py --session SID     # working-set timeline of one session

Design: docs/research/semantic-layer.md
"""

from __future__ import annotations

import argparse
import json
import sqlite3
import sys
from collections import Counter
from pathlib import Path

DEFAULT_DB = "data/snapshot_shapes.db"


def cmd_summary(conn) -> int:
    n, sessions = conn.execute(
        "SELECT COUNT(*), COUNT(DISTINCT session_id) FROM snapshot_shapes").fetchone()
    print(f"snapshots:    {n}")
    print(f"sessions:     {sessions}")

    print(f"\n## sessions with biggest working sets")
    for sid, max_t, max_v in conn.execute(
        "SELECT session_id, MAX(tracked_count), MAX(max_version) "
        "FROM snapshot_shapes GROUP BY session_id ORDER BY MAX(tracked_count) DESC LIMIT 12"):
        print(f"  session={sid[:8]}  max_tracked={max_t:<4}  max_version={max_v}")

    return 0


def cmd_session(conn, sid: str) -> int:
    rows = list(conn.execute(
        "SELECT timestamp, tracked_count, new_files, bumped_files, max_version, new_paths "
        "FROM snapshot_shapes WHERE session_id = ? ORDER BY timestamp, seq", (sid,)))
    if not rows:
        print(f"no snapshots for session {sid}", file=sys.stderr)
        return 1

    print(f"# snapshot timeline for session {sid}")
    print(f"snapshots: {len(rows)}  range: {rows[0][0][:16]} → {rows[-1][0][:16]}")
    final = rows[-1]
    print(f"final tracked_count: {final[1]}    final max_version: {final[4]}")

    new_files = sum(r[2] for r in rows)
    bumped = sum(r[3] for r in rows)
    print(f"total new files entering tracking:    {new_files}")
    print(f"total version bumps across snapshots: {bumped}")

    # working set growth — sample 12 points spread across the session
    n = len(rows)
    step = max(1, n // 12)
    print(f"\n## working-set growth (sampled)")
    print(f"  {'ts':<17} {'tracked':>8} {'new':>5} {'bumped':>7} {'max_v':>6}")
    for i in range(0, n, step):
        ts, tracked, nf, bf, mv, _ = rows[i]
        print(f"  {ts[:16]:<17} {tracked:>8} {nf:>5} {bf:>7} {mv:>6}")

    # which files entered first
    first_entry: dict[str, str] = {}
    for ts, _, _, _, _, new_paths_j in rows:
        for p in json.loads(new_paths_j):
            if p not in first_entry:
                first_entry[p] = ts
    print(f"\n## first 12 files to enter the working set")
    for p, ts in sorted(first_entry.items(), key=lambda kv: kv[1])[:12]:
        print(f"  {ts[:16]}  {p}")
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--db", default=DEFAULT_DB)
    ap.add_argument("--session")
    args = ap.parse_args()

    if not Path(args.db).exists():
        print(f"db not found: {args.db}\nbuild it first: uv run scripts/build_snapshot_shapes.py", file=sys.stderr)
        return 1
    conn = sqlite3.connect(args.db)
    if args.session:
        return cmd_session(conn, args.session)
    return cmd_summary(conn)


if __name__ == "__main__":
    sys.exit(main())
