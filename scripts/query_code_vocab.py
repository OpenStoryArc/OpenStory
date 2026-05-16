#!/usr/bin/env python3
# /// script
# requires-python = ">=3.10"
# ///
"""Read the code-vocab semantic layer.

    uv run scripts/query_code_vocab.py                          # overview
    uv run scripts/query_code_vocab.py --session SID            # vocabulary of one session
    uv run scripts/query_code_vocab.py --identifier session     # find sessions that wrote that name
    uv run scripts/query_code_vocab.py --language rust          # rust-only overview

Design: docs/research/semantic-layer.md
"""

from __future__ import annotations

import argparse
import json
import sqlite3
import sys
from collections import Counter, defaultdict
from pathlib import Path

DEFAULT_DB = "data/code_vocab_shapes.db"


def aggregate_idents(conn, where: str = "1=1", params: tuple = ()):
    rows = conn.execute(
        f"SELECT identifiers FROM code_vocab_shapes WHERE {where}", params)
    out: Counter = Counter()
    for (j,) in rows:
        out.update(json.loads(j))
    return out


def cmd_summary(conn, top: int, language: str) -> int:
    where = "1=1"
    params: tuple = ()
    if language:
        where = "language = ?"
        params = (language,)

    n, sessions = conn.execute(
        f"SELECT COUNT(*), COUNT(DISTINCT session_id) FROM code_vocab_shapes WHERE {where}",
        params).fetchone()
    print(f"vocab events: {n}")
    print(f"sessions:     {sessions}")

    langs = list(conn.execute(
        "SELECT language, COUNT(*) FROM code_vocab_shapes GROUP BY language ORDER BY COUNT(*) DESC"))
    print(f"\n## by language")
    for lang, n in langs:
        print(f"  {lang:<14} {n}")

    print(f"\n## top identifiers" + (f" (language={language})" if language else ""))
    idents = aggregate_idents(conn, where, params)
    for ident, n in idents.most_common(top * 2):
        print(f"  {ident:<30} {n}")
    return 0


def cmd_session(conn, sid: str) -> int:
    rows = list(conn.execute(
        "SELECT path, language, identifiers, total_idents FROM code_vocab_shapes "
        "WHERE session_id = ?", (sid,)))
    if not rows:
        print(f"no vocab events for session {sid}", file=sys.stderr)
        return 1

    idents: Counter = Counter()
    by_lang: dict[str, Counter] = defaultdict(Counter)
    total_writes_per_lang: Counter = Counter()
    for path, lang, j, total in rows:
        d = json.loads(j)
        idents.update(d)
        by_lang[lang].update(d)
        total_writes_per_lang[lang] += 1

    print(f"# session {sid}")
    print(f"vocab events: {len(rows)}")
    print(f"total identifier instances: {sum(idents.values())}")
    print(f"unique identifiers:         {len(idents)}")

    print(f"\n## by language")
    for lang, c in total_writes_per_lang.most_common():
        print(f"  {lang:<14} events={c:<4} unique_idents={len(by_lang[lang]):<5} total={sum(by_lang[lang].values())}")

    print(f"\n## top identifiers across the session")
    for ident, n in idents.most_common(25):
        print(f"  {ident:<30} {n}")

    print(f"\n## top identifiers per language")
    for lang, c in by_lang.items():
        print(f"  [{lang}]  {', '.join(f'{w}({n})' for w, n in c.most_common(10))}")
    return 0


def cmd_identifier(conn, ident: str) -> int:
    needle = f'%"{ident}"%'  # JSON-encoded key search; fast enough for now
    rows = list(conn.execute(
        "SELECT session_id, timestamp, path, language, identifiers "
        "FROM code_vocab_shapes WHERE identifiers LIKE ? ORDER BY timestamp", (needle,)))
    if not rows:
        print(f"no occurrences of identifier '{ident}'", file=sys.stderr)
        return 1
    by_session: dict[str, list] = defaultdict(list)
    for sid, ts, path, lang, j in rows:
        d = json.loads(j)
        count = d.get(ident, 0)
        if count:
            by_session[sid].append((ts, path, lang, count))
    print(f"# identifier '{ident}' appears in {len(by_session)} sessions, {len(rows)} events total")
    for sid, hits in sorted(by_session.items(), key=lambda kv: kv[1][0][0], reverse=True)[:20]:
        total = sum(h[3] for h in hits)
        print(f"\n  {hits[0][0][:10]}  session={sid[:8]}  events={len(hits)}  total_count={total}")
        for ts, path, lang, count in hits[:3]:
            print(f"    [{count:>3}x]  {path[-70:]}  ({lang})")
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--db", default=DEFAULT_DB)
    ap.add_argument("--session")
    ap.add_argument("--identifier")
    ap.add_argument("--language", default="")
    ap.add_argument("--top", type=int, default=15)
    args = ap.parse_args()

    if not Path(args.db).exists():
        print(f"db not found: {args.db}\nbuild it first: uv run scripts/build_code_vocab.py", file=sys.stderr)
        return 1
    conn = sqlite3.connect(args.db)

    if args.session:
        return cmd_session(conn, args.session)
    if args.identifier:
        return cmd_identifier(conn, args.identifier)
    return cmd_summary(conn, args.top, args.language)


if __name__ == "__main__":
    sys.exit(main())
