#!/usr/bin/env python3
# /// script
# requires-python = ">=3.10"
# ///
"""Read the change-shape semantic layer.

    uv run scripts/query_change_shapes.py                          # overview
    uv run scripts/query_change_shapes.py --session SID            # change profile of one session
    uv run scripts/query_change_shapes.py --path /abs/path/X.tsx   # change history of one path

Design: docs/research/semantic-layer.md
"""

from __future__ import annotations

import argparse
import sqlite3
import sys
from pathlib import Path

DEFAULT_DB = "data/change_shapes.db"


def cmd_summary(conn) -> int:
    total = conn.execute("SELECT COUNT(*), COUNT(DISTINCT session_id), "
                          "SUM(lines_added), SUM(lines_removed), "
                          "SUM(chars_added), SUM(chars_removed) FROM change_shapes").fetchone()
    n, sessions, la, lr, ca, cr = total
    print(f"change events: {n}")
    print(f"sessions:      {sessions}")
    print(f"lines:         +{la or 0}  -{lr or 0}  (net +{(la or 0) - (lr or 0)})")
    print(f"chars:         +{ca or 0}  -{cr or 0}  (net +{(ca or 0) - (cr or 0)})")

    print(f"\n## by tool")
    for tool, n, la, lr in conn.execute(
        "SELECT tool, COUNT(*), SUM(lines_added), SUM(lines_removed) "
        "FROM change_shapes GROUP BY tool ORDER BY COUNT(*) DESC"):
        print(f"  {tool:<10} events={n:<6} +{la or 0:<6} -{lr or 0}")

    print(f"\n## top-changed files (by lines added)")
    for path, n, la, lr in conn.execute(
        "SELECT path, COUNT(*), SUM(lines_added), SUM(lines_removed) "
        "FROM change_shapes GROUP BY path ORDER BY SUM(lines_added) DESC LIMIT 15"):
        print(f"  +{la or 0:>5} -{lr or 0:>5}  edits={n:>3}  {path[-80:]}")
    return 0


def cmd_session(conn, sid: str) -> int:
    rows = list(conn.execute(
        "SELECT timestamp, tool, path, edit_count, lines_added, lines_removed, "
        "chars_added, chars_removed FROM change_shapes "
        "WHERE session_id = ? ORDER BY timestamp", (sid,)))
    if not rows:
        print(f"no changes for session {sid}", file=sys.stderr)
        return 1

    la = sum(r[4] for r in rows)
    lr = sum(r[5] for r in rows)
    ca = sum(r[6] for r in rows)
    cr = sum(r[7] for r in rows)
    by_tool: dict = {}
    by_path: dict = {}
    for ts, tool, path, ec, lad, lrm, cad, crm in rows:
        by_tool.setdefault(tool, [0, 0, 0])
        by_tool[tool][0] += 1
        by_tool[tool][1] += lad
        by_tool[tool][2] += lrm
        by_path.setdefault(path, [0, 0, 0])
        by_path[path][0] += 1
        by_path[path][1] += lad
        by_path[path][2] += lrm

    print(f"# session {sid}")
    print(f"changes:  {len(rows)}    range: {rows[0][0][:16]} → {rows[-1][0][:16]}")
    print(f"lines:    +{la}  -{lr}  (net +{la - lr})")
    print(f"chars:    +{ca}  -{cr}  (net +{ca - cr})")

    print(f"\n## by tool")
    for tool, (n, lad, lrm) in sorted(by_tool.items(), key=lambda kv: -kv[1][0]):
        print(f"  {tool:<10} events={n:<5} +{lad:<6} -{lrm}")

    print(f"\n## change profile per file (top by lines added)")
    for path, (n, lad, lrm) in sorted(by_path.items(), key=lambda kv: -kv[1][1])[:15]:
        print(f"  +{lad:>5} -{lrm:>5}  edits={n:>3}  {path[-80:]}")
    return 0


def cmd_path(conn, path_sub: str) -> int:
    needle = f"%{path_sub}%"
    rows = list(conn.execute(
        "SELECT timestamp, session_id, tool, path, lines_added, lines_removed, new_excerpt "
        "FROM change_shapes WHERE path LIKE ? ORDER BY timestamp", (needle,)))
    if not rows:
        print(f"no changes for path containing '{path_sub}'", file=sys.stderr)
        return 1
    print(f"# changes touching path~'{path_sub}'  (n={len(rows)})")
    for ts, sid, tool, path, la, lr, exc in rows[:30]:
        print(f"  {ts[:16]}  {tool:<10}  +{la:<5} -{lr:<5}  session={sid[:8]}  {path[-60:]}")
        if exc:
            print(f"    ↳ {exc[:120].replace(chr(10), ' ')}")
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--db", default=DEFAULT_DB)
    ap.add_argument("--session")
    ap.add_argument("--path")
    args = ap.parse_args()

    if not Path(args.db).exists():
        print(f"db not found: {args.db}\nbuild it first: uv run scripts/build_change_shapes.py", file=sys.stderr)
        return 1
    conn = sqlite3.connect(args.db)
    if args.session:
        return cmd_session(conn, args.session)
    if args.path:
        return cmd_path(conn, args.path)
    return cmd_summary(conn)


if __name__ == "__main__":
    sys.exit(main())
