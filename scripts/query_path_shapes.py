#!/usr/bin/env python3
# /// script
# requires-python = ">=3.10"
# ///
"""Read the path-shape semantic layer.

Views over `data/path_shapes.db` (built by build_path_shapes.py):

    uv run scripts/query_path_shapes.py                          # overview
    uv run scripts/query_path_shapes.py --session SESSION_ID     # paths touched in one session
    uv run scripts/query_path_shapes.py --top-segment rs         # which sessions worked on rs/?
    uv run scripts/query_path_shapes.py --token openstory        # naming-vocab search
    uv run scripts/query_path_shapes.py --trajectory rs --field top_segment
    uv run scripts/query_path_shapes.py --vocab                  # full naming-vocab histogram
    uv run scripts/query_path_shapes.py --extensions             # extension distribution

Design: docs/research/semantic-layer.md
"""

from __future__ import annotations

import argparse
import json
import sqlite3
import sys
from collections import Counter, defaultdict
from datetime import datetime
from pathlib import Path

DEFAULT_DB = "data/path_shapes.db"


def rows(conn, **filters):
    sql = ("SELECT event_id, session_id, timestamp, seq, tool, path, directory, "
           "basename, stem, extension, depth, top_segment, dir_segments, "
           "naming_tokens, absolute FROM path_shapes WHERE 1=1")
    params: list = []
    for k, v in filters.items():
        if not v:
            continue
        sql += f" AND {k} = ?"
        params.append(v)
    sql += " ORDER BY timestamp ASC"
    for row in conn.execute(sql, params):
        yield {
            "event_id": row[0], "session_id": row[1], "timestamp": row[2],
            "seq": row[3], "tool": row[4], "path": row[5], "directory": row[6],
            "basename": row[7], "stem": row[8], "extension": row[9],
            "depth": row[10], "top_segment": row[11],
            "dir_segments": json.loads(row[12]), "naming_tokens": json.loads(row[13]),
            "absolute": row[14],
        }


def cmd_summary(conn, top: int):
    all_rows = list(rows(conn))
    sessions = {r["session_id"] for r in all_rows}
    print(f"path touches: {len(all_rows)}")
    print(f"sessions:     {len(sessions)}")
    if all_rows:
        print(f"range:        {all_rows[0]['timestamp'][:10]} → {all_rows[-1]['timestamp'][:10]}")

    seg = Counter(r["top_segment"] for r in all_rows if r["top_segment"])
    ext = Counter(r["extension"] for r in all_rows if r["extension"])
    tools = Counter(r["tool"] for r in all_rows)
    tokens: Counter = Counter()
    for r in all_rows:
        tokens.update(r["naming_tokens"])
    dirs = Counter(r["directory"] for r in all_rows if r["directory"])

    print(f"\n## top top_segments")
    for s, n in seg.most_common(top):
        print(f"  {s:<28} {n}")
    print(f"\n## top directories")
    for d, n in dirs.most_common(top):
        print(f"  {d:<50} {n}")
    print(f"\n## top extensions")
    for e, n in ext.most_common(top):
        print(f"  {e:<10} {n}")
    print(f"\n## top tools touching paths")
    for t, n in tools.most_common(top):
        print(f"  {t:<20} {n}")
    print(f"\n## top naming tokens")
    for tok, n in tokens.most_common(top * 2):
        print(f"  {tok:<24} {n}")


def cmd_session(conn, session_id: str):
    sess_rows = list(rows(conn, session_id=session_id))
    if not sess_rows:
        print(f"no path touches found for session {session_id}", file=sys.stderr)
        return 1
    print(f"# session {session_id}")
    print(f"touches: {len(sess_rows)}    range: {sess_rows[0]['timestamp'][:16]} → {sess_rows[-1]['timestamp'][:16]}")

    seg = Counter(r["top_segment"] for r in sess_rows if r["top_segment"])
    ext = Counter(r["extension"] for r in sess_rows if r["extension"])
    tokens: Counter = Counter()
    for r in sess_rows:
        tokens.update(r["naming_tokens"])
    paths = Counter(r["path"] for r in sess_rows)

    print(f"\n## areas touched")
    for s, n in seg.most_common(10):
        print(f"  {s:<28} {n}")
    print(f"\n## extensions")
    for e, n in ext.most_common(10):
        print(f"  {e:<10} {n}")
    print(f"\n## naming vocabulary (top tokens in stems)")
    for tok, n in tokens.most_common(15):
        print(f"  {tok:<24} {n}")
    print(f"\n## most-touched paths")
    for p, n in paths.most_common(10):
        print(f"  [{n:>3}x] {p}")
    return 0


def cmd_filter(conn, field: str, value: str):
    """Find sessions where `field` equals `value` (used for top_segment, extension, etc.)."""
    if field == "token":
        # special: token search over JSON naming_tokens column
        all_rows = list(rows(conn))
        sess: dict[str, list] = defaultdict(list)
        for r in all_rows:
            if value.lower() in (t.lower() for t in r["naming_tokens"]):
                sess[r["session_id"]].append(r)
    else:
        all_rows = list(rows(conn, **{field: value}))
        sess: dict[str, list] = defaultdict(list)
        for r in all_rows:
            sess[r["session_id"]].append(r)

    if not sess:
        print(f"no matches for {field}='{value}'", file=sys.stderr)
        return 1

    print(f"# sessions where {field}='{value}': {len(sess)}")
    for sid, hits in sorted(sess.items(), key=lambda kv: kv[1][0]["timestamp"], reverse=True)[:25]:
        first = hits[0]
        unique_paths = {h["path"] for h in hits}
        print(f"\n  {first['timestamp'][:10]}  session={sid[:8]}  touches={len(hits)}  paths={len(unique_paths)}")
        for p in list(unique_paths)[:5]:
            print(f"    {p}")
    return 0


def cmd_trajectory(conn, value: str, field: str):
    all_rows = list(rows(conn))
    weekly: dict[str, int] = defaultdict(int)
    weekly_total: dict[str, int] = defaultdict(int)
    for r in all_rows:
        ts = r["timestamp"]
        try:
            dt = datetime.fromisoformat(ts.replace("Z", "+00:00"))
            key = f"{dt.isocalendar()[0]}-W{dt.isocalendar()[1]:02d}"
        except Exception:
            continue
        weekly_total[key] += 1
        if field == "token":
            if value.lower() in (t.lower() for t in r["naming_tokens"]):
                weekly[key] += 1
        else:
            if str(r.get(field, "")).lower() == value.lower():
                weekly[key] += 1

    print(f"# weekly trajectory of {field}='{value}'")
    print(f"{'week':<10} {'count':>6} {'/ total':>10}  bar")
    if not weekly_total:
        return 1
    max_c = max(weekly.values()) or 1
    for week in sorted(weekly_total):
        c = weekly[week]
        total = weekly_total[week]
        bar = "█" * int(20 * c / max_c)
        print(f"{week:<10} {c:>6} {total:>10}  {bar}")
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--db", default=DEFAULT_DB)
    ap.add_argument("--session", help="show paths touched in one session")
    ap.add_argument("--top-segment", dest="topseg", help="filter by project area (rs/ui/scripts/...)")
    ap.add_argument("--extension", help="filter by file extension (.rs, .ts, ...)")
    ap.add_argument("--token", help="naming-vocab search")
    ap.add_argument("--trajectory", metavar="VALUE", help="weekly trajectory of a value")
    ap.add_argument("--field", default="top_segment",
                    choices=("top_segment", "extension", "token"),
                    help="field for --trajectory")
    ap.add_argument("--top", type=int, default=15)
    args = ap.parse_args()

    db_path = Path(args.db)
    if not db_path.exists():
        print(f"db not found: {db_path}\nbuild it first: uv run scripts/build_path_shapes.py", file=sys.stderr)
        return 1

    conn = sqlite3.connect(db_path)

    if args.session:
        return cmd_session(conn, args.session)
    if args.topseg:
        return cmd_filter(conn, "top_segment", args.topseg)
    if args.extension:
        return cmd_filter(conn, "extension", args.extension)
    if args.token:
        return cmd_filter(conn, "token", args.token)
    if args.trajectory:
        return cmd_trajectory(conn, args.trajectory, args.field)
    cmd_summary(conn, args.top)
    return 0


if __name__ == "__main__":
    sys.exit(main())
