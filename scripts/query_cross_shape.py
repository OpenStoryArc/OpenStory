#!/usr/bin/env python3
# /// script
# requires-python = ">=3.10"
# ///
"""Cross-cuts over the three shape layers.

Given a filter on one layer, aggregate the others over the matching sessions.

  # sessions whose prompts contain VERB → what areas + commands followed?
  uv run scripts/query_cross_shape.py --when-verb see

  # sessions that used PROGRAM → what verbs and areas characterized them?
  uv run scripts/query_cross_shape.py --when-program git

  # sessions that touched AREA → what verbs and commands characterized them?
  uv run scripts/query_cross_shape.py --when-segment scripts

Design: docs/research/semantic-layer.md
"""

from __future__ import annotations

import argparse
import json
import sqlite3
import sys
from collections import Counter
from pathlib import Path

PROMPT_DB = "data/prompt_shapes.db"
PATH_DB = "data/path_shapes.db"
BASH_DB = "data/bash_shapes.db"


def _check_dbs() -> bool:
    missing = [p for p in (PROMPT_DB, PATH_DB, BASH_DB) if not Path(p).exists()]
    if missing:
        print(f"missing layer dbs: {missing}", file=sys.stderr)
        print(f"build them with the matching build_*_shapes.py scripts", file=sys.stderr)
        return False
    return True


def sessions_with_verb(verb: str) -> set[str]:
    """Return session_ids whose prompt_shapes contain `verb` in root_verbs."""
    needle = verb.lower()
    conn = sqlite3.connect(PROMPT_DB)
    hits: set[str] = set()
    for sid, rv_json in conn.execute("SELECT session_id, root_verbs FROM prompt_shapes"):
        if needle in (t.lower() for t in json.loads(rv_json)):
            hits.add(sid)
    return hits


def sessions_with_program(program: str) -> set[str]:
    conn = sqlite3.connect(BASH_DB)
    return {row[0] for row in conn.execute(
        "SELECT DISTINCT session_id FROM bash_shapes WHERE program = ?", (program,))}


def sessions_with_segment(segment: str) -> set[str]:
    conn = sqlite3.connect(PATH_DB)
    return {row[0] for row in conn.execute(
        "SELECT DISTINCT session_id FROM path_shapes WHERE top_segment = ?", (segment,))}


def aggregate_prompts(session_ids: set[str]) -> tuple[Counter, Counter, Counter]:
    """Aggregate verbs / direct_objects / noun_chunks across sessions."""
    if not session_ids:
        return Counter(), Counter(), Counter()
    conn = sqlite3.connect(PROMPT_DB)
    qmarks = ",".join("?" * len(session_ids))
    verbs: Counter = Counter()
    objs: Counter = Counter()
    chunks: Counter = Counter()
    for v, o, c in conn.execute(
        f"SELECT root_verbs, direct_objects, noun_chunks FROM prompt_shapes "
        f"WHERE session_id IN ({qmarks})", tuple(session_ids)):
        verbs.update(json.loads(v))
        objs.update(json.loads(o))
        chunks.update(json.loads(c))
    return verbs, objs, chunks


def aggregate_paths(session_ids: set[str]) -> tuple[Counter, Counter, Counter]:
    """Aggregate top_segments / extensions / naming_tokens across sessions."""
    if not session_ids:
        return Counter(), Counter(), Counter()
    conn = sqlite3.connect(PATH_DB)
    qmarks = ",".join("?" * len(session_ids))
    segs: Counter = Counter()
    exts: Counter = Counter()
    tokens: Counter = Counter()
    for seg, ext, toks_json in conn.execute(
        f"SELECT top_segment, extension, naming_tokens FROM path_shapes "
        f"WHERE session_id IN ({qmarks})", tuple(session_ids)):
        if seg:
            segs[seg] += 1
        if ext:
            exts[ext] += 1
        tokens.update(json.loads(toks_json))
    return segs, exts, tokens


def aggregate_bash(session_ids: set[str]) -> tuple[Counter, Counter]:
    """Aggregate programs / subcommands across sessions."""
    if not session_ids:
        return Counter(), Counter()
    conn = sqlite3.connect(BASH_DB)
    qmarks = ",".join("?" * len(session_ids))
    progs: Counter = Counter()
    subs: Counter = Counter()
    for prog, sub in conn.execute(
        f"SELECT program, subcommand FROM bash_shapes "
        f"WHERE session_id IN ({qmarks})", tuple(session_ids)):
        if prog:
            progs[prog] += 1
        if sub:
            subs[f"{prog} {sub}"] += 1
    return progs, subs


def topn(counter: Counter, n: int = 10) -> str:
    if not counter:
        return "—"
    return ", ".join(f"{k}({v})" for k, v in counter.most_common(n))


def cmd_when_verb(verb: str) -> int:
    sids = sessions_with_verb(verb)
    if not sids:
        print(f"no sessions with verb='{verb}'", file=sys.stderr)
        return 1
    print(f"# sessions with verb='{verb}'  (n={len(sids)} sessions)")
    _, objs, chunks = aggregate_prompts(sids)
    segs, exts, tokens = aggregate_paths(sids)
    progs, subs = aggregate_bash(sids)
    print(f"\n## prompt context")
    print(f"  objects:        {topn(objs)}")
    print(f"  noun chunks:    {topn(chunks)}")
    print(f"\n## paths touched")
    print(f"  top segments:   {topn(segs)}")
    print(f"  extensions:     {topn(exts)}")
    print(f"  naming vocab:   {topn(tokens, 12)}")
    print(f"\n## bash commands run")
    print(f"  programs:       {topn(progs)}")
    print(f"  top sub-calls:  {topn(subs, 12)}")
    return 0


def cmd_when_program(program: str) -> int:
    sids = sessions_with_program(program)
    if not sids:
        print(f"no sessions used program='{program}'", file=sys.stderr)
        return 1
    print(f"# sessions that used program='{program}'  (n={len(sids)} sessions)")
    verbs, objs, chunks = aggregate_prompts(sids)
    segs, _, tokens = aggregate_paths(sids)
    _, subs = aggregate_bash(sids)
    print(f"\n## prompt context")
    print(f"  verbs:          {topn(verbs)}")
    print(f"  objects:        {topn(objs)}")
    print(f"  chunks:         {topn(chunks)}")
    print(f"\n## paths touched")
    print(f"  top segments:   {topn(segs)}")
    print(f"  naming vocab:   {topn(tokens, 12)}")
    print(f"\n## subcommands seen for {program}")
    print(f"  {topn(subs, 15)}")
    return 0


def cmd_when_segment(segment: str) -> int:
    sids = sessions_with_segment(segment)
    if not sids:
        print(f"no sessions touched top_segment='{segment}'", file=sys.stderr)
        return 1
    print(f"# sessions that touched top_segment='{segment}'  (n={len(sids)} sessions)")
    verbs, objs, chunks = aggregate_prompts(sids)
    progs, subs = aggregate_bash(sids)
    print(f"\n## prompt context")
    print(f"  verbs:          {topn(verbs)}")
    print(f"  objects:        {topn(objs)}")
    print(f"  chunks:         {topn(chunks)}")
    print(f"\n## bash commands run")
    print(f"  programs:       {topn(progs)}")
    print(f"  top sub-calls:  {topn(subs, 12)}")
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--when-verb", dest="verb",
                    help="filter sessions by prompt root verb")
    ap.add_argument("--when-program", dest="program",
                    help="filter sessions by bash program")
    ap.add_argument("--when-segment", dest="segment",
                    help="filter sessions by path top_segment")
    args = ap.parse_args()

    if not _check_dbs():
        return 1

    if args.verb:
        return cmd_when_verb(args.verb)
    if args.program:
        return cmd_when_program(args.program)
    if args.segment:
        return cmd_when_segment(args.segment)
    ap.print_help()
    return 1


if __name__ == "__main__":
    sys.exit(main())
