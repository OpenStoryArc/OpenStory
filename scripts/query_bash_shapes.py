#!/usr/bin/env python3
# /// script
# requires-python = ">=3.10"
# ///
"""Read the bash-shape semantic layer.

Views over `data/bash_shapes.db` (built by build_bash_shapes.py):

    uv run scripts/query_bash_shapes.py                          # overview
    uv run scripts/query_bash_shapes.py --session SESSION_ID     # one session's bash dialect
    uv run scripts/query_bash_shapes.py --program git            # all sessions that used git
    uv run scripts/query_bash_shapes.py --subcommand commit      # find a subcommand
    uv run scripts/query_bash_shapes.py --trajectory git --field program

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

DEFAULT_DB = "data/bash_shapes.db"


def rows(conn, **filters):
    sql = ("SELECT event_id, session_id, timestamp, seq, program, subcommand, "
           "flags, args, is_pipeline, is_redirect, raw_command "
           "FROM bash_shapes WHERE 1=1")
    params: list = []
    for k, v in filters.items():
        if v in (None, ""):
            continue
        sql += f" AND {k} = ?"
        params.append(v)
    sql += " ORDER BY timestamp ASC"
    for r in conn.execute(sql, params):
        yield {
            "event_id": r[0], "session_id": r[1], "timestamp": r[2],
            "seq": r[3], "program": r[4], "subcommand": r[5],
            "flags": json.loads(r[6]), "args": json.loads(r[7]),
            "is_pipeline": r[8], "is_redirect": r[9], "raw_command": r[10],
        }


def cmd_summary(conn, top: int):
    all_rows = list(rows(conn))
    sessions = {r["session_id"] for r in all_rows}
    print(f"bash commands: {len(all_rows)}")
    print(f"sessions:      {len(sessions)}")
    if all_rows:
        print(f"range:         {all_rows[0]['timestamp'][:10]} → {all_rows[-1]['timestamp'][:10]}")

    progs = Counter(r["program"] for r in all_rows if r["program"])
    subs: dict[str, Counter] = defaultdict(Counter)
    for r in all_rows:
        if r["subcommand"]:
            subs[r["program"]][r["subcommand"]] += 1
    pipes = sum(r["is_pipeline"] for r in all_rows)
    reds = sum(r["is_redirect"] for r in all_rows)
    flags: Counter = Counter()
    for r in all_rows:
        flags.update(r["flags"])

    print(f"\n## top programs")
    for p, n in progs.most_common(top):
        print(f"  {p:<20} {n}")
    print(f"\n## subcommand mix (per program)")
    for prog in [p for p, _ in progs.most_common(8)]:
        if not subs[prog]:
            continue
        top_subs = ", ".join(f"{s}({n})" for s, n in subs[prog].most_common(8))
        print(f"  {prog:<12} {top_subs}")
    print(f"\n## top flags")
    for f, n in flags.most_common(top):
        print(f"  {f:<20} {n}")
    pct = lambda x: f"{100*x/max(len(all_rows),1):.1f}%"
    print(f"\n## composition")
    print(f"  pipelines:  {pipes} ({pct(pipes)})")
    print(f"  redirects:  {reds} ({pct(reds)})")


def cmd_session(conn, session_id: str):
    sess_rows = list(rows(conn, session_id=session_id))
    if not sess_rows:
        print(f"no bash commands found for session {session_id}", file=sys.stderr)
        return 1
    print(f"# session {session_id}")
    print(f"commands: {len(sess_rows)}    range: {sess_rows[0]['timestamp'][:16]} → {sess_rows[-1]['timestamp'][:16]}\n")

    progs = Counter(r["program"] for r in sess_rows if r["program"])
    subs: dict[str, Counter] = defaultdict(Counter)
    for r in sess_rows:
        if r["subcommand"]:
            subs[r["program"]][r["subcommand"]] += 1

    print("## programs")
    for p, n in progs.most_common(15):
        sub_str = ""
        if subs[p]:
            sub_str = "  →  " + ", ".join(f"{s}({k})" for s, k in subs[p].most_common(5))
        print(f"  {p:<14} {n:>4}{sub_str}")

    print("\n## sample commands (first 8)")
    for r in sess_rows[:8]:
        print(f"  [{r['seq']:>3}] {r['raw_command'][:140]}")
    return 0


def cmd_filter(conn, field: str, value: str):
    all_rows = list(rows(conn, **{field: value}))
    sess: dict[str, list] = defaultdict(list)
    for r in all_rows:
        sess[r["session_id"]].append(r)
    if not sess:
        print(f"no matches for {field}='{value}'", file=sys.stderr)
        return 1
    print(f"# sessions where {field}='{value}': {len(sess)} sessions, {len(all_rows)} commands")
    for sid, hits in sorted(sess.items(), key=lambda kv: kv[1][0]["timestamp"], reverse=True)[:25]:
        first = hits[0]
        subs = Counter(h["subcommand"] for h in hits if h["subcommand"])
        sub_str = ", ".join(f"{s}({n})" for s, n in subs.most_common(5))
        print(f"\n  {first['timestamp'][:10]}  session={sid[:8]}  commands={len(hits)}")
        if sub_str:
            print(f"    subcommands: {sub_str}")
        for h in hits[:3]:
            print(f"    [{h['seq']:>3}] {h['raw_command'][:120]}")
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
    ap.add_argument("--session", help="show bash dialect of one session")
    ap.add_argument("--program", help="filter sessions by program name")
    ap.add_argument("--subcommand", help="filter sessions by subcommand")
    ap.add_argument("--trajectory", metavar="VALUE", help="weekly trajectory of a value")
    ap.add_argument("--field", default="program",
                    choices=("program", "subcommand"),
                    help="field for --trajectory")
    ap.add_argument("--top", type=int, default=15)
    args = ap.parse_args()

    db_path = Path(args.db)
    if not db_path.exists():
        print(f"db not found: {db_path}\nbuild it first: uv run scripts/build_bash_shapes.py", file=sys.stderr)
        return 1

    conn = sqlite3.connect(db_path)

    if args.session:
        return cmd_session(conn, args.session)
    if args.program:
        return cmd_filter(conn, "program", args.program)
    if args.subcommand:
        return cmd_filter(conn, "subcommand", args.subcommand)
    if args.trajectory:
        return cmd_trajectory(conn, args.trajectory, args.field)
    cmd_summary(conn, args.top)
    return 0


if __name__ == "__main__":
    sys.exit(main())
