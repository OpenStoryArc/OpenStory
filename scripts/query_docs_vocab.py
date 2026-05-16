#!/usr/bin/env python3
# /// script
# requires-python = ">=3.10"
# ///
"""Read the docs-vocab semantic layer.

    uv run scripts/query_docs_vocab.py                          # overview
    uv run scripts/query_docs_vocab.py --session SID            # prose vocab for one session
    uv run scripts/query_docs_vocab.py --term "shape layer"     # find sessions where this term appeared
    uv run scripts/query_docs_vocab.py --header backlog         # find sessions where a header contains this

Design: docs/research/semantic-layer.md
"""

from __future__ import annotations

import argparse
import json
import sqlite3
import sys
from collections import Counter, defaultdict
from pathlib import Path

DEFAULT_DB = "data/docs_vocab_shapes.db"


def all_rows(conn, **filters):
    sql = ("SELECT event_id, session_id, timestamp, seq, tool, path, extension, "
           "headers, bold_terms, link_labels, noun_chunks, char_count "
           "FROM docs_vocab_shapes WHERE 1=1")
    params: list = []
    for k, v in filters.items():
        if v:
            sql += f" AND {k} = ?"
            params.append(v)
    sql += " ORDER BY timestamp ASC"
    for r in conn.execute(sql, params):
        yield {
            "event_id": r[0], "session_id": r[1], "timestamp": r[2],
            "seq": r[3], "tool": r[4], "path": r[5], "extension": r[6],
            "headers": json.loads(r[7]),
            "bold_terms": json.loads(r[8]),
            "link_labels": json.loads(r[9]),
            "noun_chunks": json.loads(r[10]),
            "char_count": r[11],
        }


def cmd_summary(conn, top: int) -> int:
    rows = list(all_rows(conn))
    print(f"docs-vocab events: {len(rows)}")
    print(f"sessions:          {len({r['session_id'] for r in rows})}")
    if rows:
        print(f"range:             {rows[0]['timestamp'][:10]} → {rows[-1]['timestamp'][:10]}")

    exts: Counter = Counter(r["extension"] for r in rows)
    chunks: Counter = Counter()
    headers: Counter = Counter()
    bold: Counter = Counter()
    links: Counter = Counter()
    for r in rows:
        chunks.update(r["noun_chunks"])
        for h in r["headers"]:
            headers[h] += 1
        bold.update(r["bold_terms"])
        links.update(r["link_labels"])

    print(f"\n## by extension")
    for e, n in exts.most_common():
        print(f"  {e:<14} {n}")

    print(f"\n## top noun chunks (named concepts from prose)")
    for c, n in chunks.most_common(top * 2):
        print(f"  {c:<40} {n}")

    print(f"\n## top headers")
    for h, n in headers.most_common(top):
        print(f"  {h[:60]:<60} {n}")

    print(f"\n## top bold terms")
    for b, n in bold.most_common(top):
        print(f"  {b[:40]:<40} {n}")

    print(f"\n## top link labels")
    for l, n in links.most_common(top):
        print(f"  {l[:60]:<60} {n}")
    return 0


def cmd_session(conn, sid: str) -> int:
    rows = list(all_rows(conn, session_id=sid))
    if not rows:
        print(f"no docs-vocab events for session {sid}", file=sys.stderr)
        return 1
    chunks: Counter = Counter()
    headers: list[str] = []
    bold: Counter = Counter()
    links: Counter = Counter()
    paths: Counter = Counter()
    for r in rows:
        chunks.update(r["noun_chunks"])
        headers.extend(r["headers"])
        bold.update(r["bold_terms"])
        links.update(r["link_labels"])
        paths[r["path"]] += 1

    print(f"# session {sid}")
    print(f"docs events: {len(rows)}   range: {rows[0]['timestamp'][:16]} → {rows[-1]['timestamp'][:16]}")

    print(f"\n## paths touched")
    for p, n in paths.most_common(10):
        print(f"  [{n:>2}x] {p[-70:]}")

    print(f"\n## headers written")
    for h in headers[:20]:
        print(f"  {h[:80]}")

    print(f"\n## top noun chunks")
    for c, n in chunks.most_common(20):
        print(f"  {c:<40} {n}")

    if bold:
        print(f"\n## top bold terms")
        for b, n in bold.most_common(10):
            print(f"  {b:<40} {n}")
    return 0


def cmd_term(conn, term: str) -> int:
    """Find sessions whose docs contain this noun_chunk / bold / header / link."""
    needle = term.lower()
    sess: dict[str, list] = defaultdict(list)
    for r in all_rows(conn):
        hit = False
        if needle in (c.lower() for c in r["noun_chunks"]): hit = True
        if any(needle in b.lower() for b in r["bold_terms"]): hit = True
        if any(needle in h.lower() for h in r["headers"]):    hit = True
        if any(needle in l.lower() for l in r["link_labels"]): hit = True
        if hit:
            sess[r["session_id"]].append(r)
    if not sess:
        print(f"no occurrences of '{term}' in docs vocabulary", file=sys.stderr)
        return 1
    print(f"# term '{term}' appears in docs across {len(sess)} sessions")
    for sid, hits in sorted(sess.items(), key=lambda kv: kv[1][0]["timestamp"], reverse=True)[:20]:
        first = hits[0]
        unique_paths = {h["path"] for h in hits}
        print(f"\n  {first['timestamp'][:10]}  session={sid[:8]}  events={len(hits)}  paths={len(unique_paths)}")
        for p in list(unique_paths)[:4]:
            print(f"    {p[-80:]}")
    return 0


def cmd_header(conn, hdr_sub: str) -> int:
    needle = hdr_sub.lower()
    sess: dict[str, list] = defaultdict(list)
    for r in all_rows(conn):
        if any(needle in h.lower() for h in r["headers"]):
            sess[r["session_id"]].append(r)
    if not sess:
        print(f"no headers contain '{hdr_sub}'", file=sys.stderr)
        return 1
    print(f"# sessions with header containing '{hdr_sub}': {len(sess)}")
    for sid, hits in sorted(sess.items(), key=lambda kv: kv[1][0]["timestamp"], reverse=True)[:20]:
        first = hits[0]
        print(f"\n  {first['timestamp'][:10]}  session={sid[:8]}  events={len(hits)}")
        seen_headers = set()
        for h in hits:
            for hdr in h["headers"]:
                if needle in hdr.lower() and hdr not in seen_headers:
                    seen_headers.add(hdr)
                    print(f"    “{hdr}”  ← {h['path'][-50:]}")
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--db", default=DEFAULT_DB)
    ap.add_argument("--session")
    ap.add_argument("--term")
    ap.add_argument("--header")
    ap.add_argument("--top", type=int, default=15)
    args = ap.parse_args()

    if not Path(args.db).exists():
        print(f"db not found: {args.db}\nbuild it first: uv run scripts/build_docs_vocab.py", file=sys.stderr)
        return 1
    conn = sqlite3.connect(args.db)
    if args.session:
        return cmd_session(conn, args.session)
    if args.term:
        return cmd_term(conn, args.term)
    if args.header:
        return cmd_header(conn, args.header)
    return cmd_summary(conn, args.top)


if __name__ == "__main__":
    sys.exit(main())
