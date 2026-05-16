#!/usr/bin/env python3
# /// script
# requires-python = ">=3.10"
# ///
"""Cross-layer per-session shape report.

Joins the three shape layers (prompt / path / bash) on session_id and prints
a unified picture of one session:

  * what the user was pointing at (prompt verbs / objects / chunks / modifiers)
  * where attention went in the codebase (top segments / extensions / naming vocab)
  * what was actually run (bash programs / subcommands / pipeline rate)

Usage:
    uv run scripts/query_session_shape.py SESSION_ID

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


def topn(counter: Counter, n: int = 8) -> str:
    if not counter:
        return "—"
    return ", ".join(f"{k}({v})" for k, v in counter.most_common(n))


def prompt_section(sid: str) -> None:
    p = Path(PROMPT_DB)
    if not p.exists():
        print(f"  (prompt_shapes.db not built — run build_prompt_shapes.py)")
        return
    conn = sqlite3.connect(p)
    rows = list(conn.execute(
        "SELECT timestamp, seq, root_verbs, subjects, direct_objects, noun_chunks, "
        "adjectives, adverbs, prompt_excerpt FROM prompt_shapes "
        "WHERE session_id = ? ORDER BY timestamp", (sid,)))
    if not rows:
        print(f"  no prompts in this session")
        return

    verbs: Counter = Counter()
    subjs: Counter = Counter()
    objs: Counter = Counter()
    chunks: Counter = Counter()
    adjs: Counter = Counter()
    advs: Counter = Counter()
    for _, _, v, s, o, c, a, ad, _ in rows:
        verbs.update(json.loads(v))
        subjs.update(json.loads(s))
        objs.update(json.loads(o))
        chunks.update(json.loads(c))
        adjs.update(json.loads(a))
        advs.update(json.loads(ad))

    print(f"## prompt skeleton  (n={len(rows)} prompts, {rows[0][0][:16]} → {rows[-1][0][:16]})")
    print(f"  verbs:       {topn(verbs)}")
    print(f"  subjects:    {topn(subjs)}")
    print(f"  objects:     {topn(objs)}")
    print(f"  chunks:      {topn(chunks)}")
    print(f"  adjectives:  {topn(adjs)}")
    print(f"  adverbs:     {topn(advs)}")
    print(f"\n  first prompt:  “{rows[0][8][:160].replace(chr(10), ' ')}”")
    if len(rows) > 1:
        print(f"  last prompt:   “{rows[-1][8][:160].replace(chr(10), ' ')}”")


def path_section(sid: str) -> None:
    p = Path(PATH_DB)
    if not p.exists():
        print(f"\n  (path_shapes.db not built — run build_path_shapes.py)")
        return
    conn = sqlite3.connect(p)
    rows = list(conn.execute(
        "SELECT path, top_segment, extension, naming_tokens, tool "
        "FROM path_shapes WHERE session_id = ?", (sid,)))
    if not rows:
        print(f"\n## path footprint  (n=0)")
        return

    segs: Counter = Counter()
    exts: Counter = Counter()
    tokens: Counter = Counter()
    tools: Counter = Counter()
    paths: Counter = Counter()
    for path, seg, ext, toks_json, tool in rows:
        if seg:
            segs[seg] += 1
        if ext:
            exts[ext] += 1
        tokens.update(json.loads(toks_json))
        tools[tool] += 1
        paths[path] += 1

    print(f"\n## path footprint  (n={len(rows)} touches, {len(paths)} unique paths)")
    print(f"  areas:       {topn(segs)}")
    print(f"  extensions:  {topn(exts)}")
    print(f"  tools:       {topn(tools)}")
    print(f"  naming vocab: {topn(tokens, 12)}")
    print(f"\n  most-touched paths:")
    for path, n in paths.most_common(6):
        print(f"    [{n:>3}x] {path}")


def bash_section(sid: str) -> None:
    p = Path(BASH_DB)
    if not p.exists():
        print(f"\n  (bash_shapes.db not built — run build_bash_shapes.py)")
        return
    conn = sqlite3.connect(p)
    rows = list(conn.execute(
        "SELECT program, subcommand, is_pipeline, is_redirect, raw_command "
        "FROM bash_shapes WHERE session_id = ?", (sid,)))
    if not rows:
        print(f"\n## bash dialect  (n=0)")
        return

    progs: Counter = Counter()
    subs_by_prog: dict[str, Counter] = {}
    pipes = 0
    reds = 0
    for prog, sub, is_pipe, is_red, _ in rows:
        if prog:
            progs[prog] += 1
        if sub:
            subs_by_prog.setdefault(prog, Counter())[sub] += 1
        pipes += is_pipe
        reds += is_red

    n = len(rows)
    print(f"\n## bash dialect  (n={n} commands)")
    print(f"  programs:    {topn(progs)}")
    for prog in [p for p, _ in progs.most_common(4)]:
        if prog in subs_by_prog:
            print(f"  {prog} →     {topn(subs_by_prog[prog], 8)}")
    pct = lambda x: f"{100*x/max(n,1):.0f}%"
    print(f"  pipelines:   {pipes}/{n} ({pct(pipes)})  redirects: {reds}/{n} ({pct(reds)})")


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("session_id", help="full session UUID")
    args = ap.parse_args()

    print(f"# session {args.session_id}\n")
    prompt_section(args.session_id)
    path_section(args.session_id)
    bash_section(args.session_id)
    return 0


if __name__ == "__main__":
    sys.exit(main())
