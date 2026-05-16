#!/usr/bin/env python3
# /// script
# requires-python = ">=3.10"
# ///
"""Read the prompt-shape semantic layer.

Several views over `data/prompt_shapes.db` (built by build_prompt_shapes.py):

    # summary of the whole layer
    uv run scripts/query_prompt_shapes.py

    # full shape of a single session (all prompts)
    uv run scripts/query_prompt_shapes.py --session SESSION_ID

    # which sessions mention a verb, object, or noun chunk
    uv run scripts/query_prompt_shapes.py --verb see
    uv run scripts/query_prompt_shapes.py --object openstory
    uv run scripts/query_prompt_shapes.py --chunk "openstory lab"

    # weekly trajectory of an object/verb/adjective across all sessions
    uv run scripts/query_prompt_shapes.py --trajectory semantic --field adjectives

    # top-N aggregates over a date window
    uv run scripts/query_prompt_shapes.py --top 15 --since 2026-05-01

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

DEFAULT_DB = "data/prompt_shapes.db"

FIELDS = ("root_verbs", "subjects", "direct_objects", "noun_chunks", "adjectives", "adverbs")


def load_all(conn, since: str = "", session_id: str = ""):
    sql = "SELECT event_id, session_id, timestamp, seq, char_count, " \
          "root_verbs, subjects, direct_objects, noun_chunks, adjectives, adverbs, prompt_excerpt " \
          "FROM prompt_shapes WHERE 1=1"
    params: list = []
    if since:
        sql += " AND timestamp >= ?"
        params.append(since)
    if session_id:
        sql += " AND session_id = ?"
        params.append(session_id)
    sql += " ORDER BY timestamp ASC"
    for row in conn.execute(sql, params):
        yield {
            "event_id": row[0],
            "session_id": row[1],
            "timestamp": row[2],
            "seq": row[3],
            "char_count": row[4],
            "root_verbs": json.loads(row[5]),
            "subjects": json.loads(row[6]),
            "direct_objects": json.loads(row[7]),
            "noun_chunks": json.loads(row[8]),
            "adjectives": json.loads(row[9]),
            "adverbs": json.loads(row[10]),
            "prompt_excerpt": row[11],
        }


def cmd_summary(conn, top: int, since: str):
    rows = list(load_all(conn, since=since))
    sessions = {r["session_id"] for r in rows}
    print(f"prompts:  {len(rows)}")
    print(f"sessions: {len(sessions)}")
    if rows:
        print(f"range:    {rows[0]['timestamp'][:10]} → {rows[-1]['timestamp'][:10]}")
    for field in FIELDS:
        c: Counter = Counter()
        for r in rows:
            c.update(r[field])
        print(f"\n## top {field}")
        for word, n in c.most_common(top):
            print(f"  {word:<30}  {n}")


def cmd_session(conn, session_id: str):
    rows = list(load_all(conn, session_id=session_id))
    if not rows:
        print(f"no prompts found for session {session_id}", file=sys.stderr)
        return 1
    print(f"# session {session_id}")
    print(f"prompts: {len(rows)}    range: {rows[0]['timestamp'][:16]} → {rows[-1]['timestamp'][:16]}\n")

    # per-prompt skeleton
    for i, r in enumerate(rows, 1):
        rv = ",".join(r["root_verbs"][:3]) or "—"
        do = ",".join(r["direct_objects"][:3]) or "—"
        nc = ",".join(r["noun_chunks"][:3]) or "—"
        adj = ",".join(r["adjectives"][:3]) or "—"
        print(f"[{i:>2}] {r['timestamp'][11:16]}  verb={rv:<20} obj={do:<18} chunks={nc:<32} adj={adj}")
        print(f"     “{r['prompt_excerpt'][:140].replace(chr(10), ' ')}”")
    print()

    # aggregate
    print("## session aggregate")
    for field in FIELDS:
        c = Counter()
        for r in rows:
            c.update(r[field])
        if c:
            top = ", ".join(f"{w}({n})" for w, n in c.most_common(6))
            print(f"  {field:<16} {top}")
    return 0


def cmd_filter(conn, field: str, word: str):
    """Find sessions where `word` appears in `field` (substring match for multi-word tokens)."""
    needle = word.lower()
    rows = list(load_all(conn))
    hits = defaultdict(list)
    for r in rows:
        for tok in r[field]:
            if needle == tok.lower() or needle in tok.lower():
                hits[r["session_id"]].append(r)
                break
    print(f"# sessions where {field} contains '{word}': {len(hits)} sessions")
    for sid, prompts in sorted(hits.items(), key=lambda kv: kv[1][0]["timestamp"], reverse=True):
        first = prompts[0]
        print(f"\n  {first['timestamp'][:10]}  session={sid[:8]}  prompts={len(prompts)}")
        for p in prompts[:3]:
            print(f"    [{p['seq']:>3}] “{p['prompt_excerpt'][:120].replace(chr(10), ' ')}”")
    return 0


def cmd_trajectory(conn, word: str, field: str):
    """How does `word` appear in `field` across weeks?"""
    rows = load_all(conn)
    weekly: dict[str, int] = defaultdict(int)
    weekly_total: dict[str, int] = defaultdict(int)
    for r in rows:
        ts = r["timestamp"]
        try:
            dt = datetime.fromisoformat(ts.replace("Z", "+00:00"))
            key = f"{dt.isocalendar()[0]}-W{dt.isocalendar()[1]:02d}"
        except Exception:
            continue
        weekly_total[key] += len(r[field])
        weekly[key] += sum(1 for t in r[field] if t.lower() == word.lower())

    print(f"# trajectory of '{word}' in {field}")
    print(f"{'week':<10} {'count':>6} {'/ field-total':>14} {'bar'}")
    if not weekly:
        return 1
    max_c = max(weekly.values()) or 1
    for week in sorted(set(weekly_total)):
        c = weekly[week]
        total = weekly_total[week]
        bar = "█" * int(20 * c / max_c)
        print(f"{week:<10} {c:>6} {total:>14}   {bar}")
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--db", default=DEFAULT_DB)
    ap.add_argument("--session", help="show shape of a single session")
    ap.add_argument("--verb", help="find sessions with this root verb")
    ap.add_argument("--object", dest="obj", help="find sessions with this direct object")
    ap.add_argument("--chunk", help="find sessions with this noun chunk")
    ap.add_argument("--adj", help="find sessions with this adjective")
    ap.add_argument("--trajectory", metavar="WORD", help="weekly trajectory of a word")
    ap.add_argument("--field", default="noun_chunks",
                    choices=FIELDS, help="field to search/trajectory against")
    ap.add_argument("--top", type=int, default=12)
    ap.add_argument("--since", default="", help="ISO date floor (e.g. 2026-05-01)")
    args = ap.parse_args()

    db_path = Path(args.db)
    if not db_path.exists():
        print(f"db not found: {db_path}\nbuild it first: uv run scripts/build_prompt_shapes.py", file=sys.stderr)
        return 1

    conn = sqlite3.connect(db_path)

    if args.session:
        return cmd_session(conn, args.session)
    if args.verb:
        return cmd_filter(conn, "root_verbs", args.verb)
    if args.obj:
        return cmd_filter(conn, "direct_objects", args.obj)
    if args.chunk:
        return cmd_filter(conn, "noun_chunks", args.chunk)
    if args.adj:
        return cmd_filter(conn, "adjectives", args.adj)
    if args.trajectory:
        return cmd_trajectory(conn, args.trajectory, args.field)
    cmd_summary(conn, args.top, args.since)
    return 0


if __name__ == "__main__":
    sys.exit(main())
