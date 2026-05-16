#!/usr/bin/env python3
# /// script
# requires-python = ">=3.10"
# ///
"""Gather shape-layer facts for a single-session story.

Companion to `scripts/sessionstory.py`. That script reads OpenStory's
record/pattern API and emits the baseline fact sheet (record types, tool
histograms, eval-apply patterns, turn.sentence detector output, prompt
timeline). This one reads the **shape-layer databases** (`prompt_shapes.db`,
`path_shapes.db`, `bash_shapes.db`) and emits four complementary fact
blocks that ground a richer narrative:

  1. Three-layer shape report   — prompt skeleton + path footprint + bash dialect
  2. Prompt sequence            — every prompt in time order with HH:MM stamps
  3. Hourly rhythm              — prompts/paths/bash per hour (reveals working
                                  blocks and natural breaks/sleep gaps)
  4. PR & git activity          — every `gh pr` / `git push` / `git merge`
                                  command with its timestamp (the artifact
                                  trail of the session)

The script does NOT narrate. It emits structured facts. Narration is the
agent's job per `docs/research/session-stories/README.md`.

Usage:
    uv run scripts/session_story_facts.py SESSION_ID
    uv run scripts/session_story_facts.py SESSION_ID --json
"""

from __future__ import annotations

import argparse
import json
import sqlite3
import sys
from collections import Counter, defaultdict
from datetime import datetime
from pathlib import Path

PROMPT_DB = "data/prompt_shapes.db"
PATH_DB = "data/path_shapes.db"
BASH_DB = "data/bash_shapes.db"


def _check() -> bool:
    missing = [p for p in (PROMPT_DB, PATH_DB, BASH_DB) if not Path(p).exists()]
    if missing:
        print(f"missing layer dbs: {missing}", file=sys.stderr)
        print(f"build them: uv run scripts/build_{{prompt,path,bash}}_shapes.py", file=sys.stderr)
        return False
    return True


def shape_block(sid: str) -> dict:
    out: dict = {"prompt": {}, "path": {}, "bash": {}}

    # --- prompt ---
    conn = sqlite3.connect(PROMPT_DB)
    rows = list(conn.execute(
        "SELECT timestamp, root_verbs, subjects, direct_objects, noun_chunks, "
        "adjectives, adverbs FROM prompt_shapes WHERE session_id = ? "
        "ORDER BY timestamp", (sid,)))
    if rows:
        verbs, subjs, objs, chunks, adjs, advs = (Counter() for _ in range(6))
        for _, v, s, o, c, a, ad in rows:
            verbs.update(json.loads(v))
            subjs.update(json.loads(s))
            objs.update(json.loads(o))
            chunks.update(json.loads(c))
            adjs.update(json.loads(a))
            advs.update(json.loads(ad))
        out["prompt"] = {
            "n": len(rows),
            "first_ts": rows[0][0],
            "last_ts": rows[-1][0],
            "verbs": verbs.most_common(10),
            "subjects": subjs.most_common(8),
            "objects": objs.most_common(10),
            "chunks": chunks.most_common(12),
            "adjectives": adjs.most_common(10),
            "adverbs": advs.most_common(8),
        }

    # --- path ---
    conn = sqlite3.connect(PATH_DB)
    rows = list(conn.execute(
        "SELECT path, top_segment, extension, naming_tokens, tool "
        "FROM path_shapes WHERE session_id = ?", (sid,)))
    if rows:
        segs, exts, tokens, tools, paths = (Counter() for _ in range(5))
        for path, seg, ext, toks_json, tool in rows:
            if seg: segs[seg] += 1
            if ext: exts[ext] += 1
            tokens.update(json.loads(toks_json))
            tools[tool] += 1
            paths[path] += 1
        out["path"] = {
            "n": len(rows),
            "unique_paths": len(paths),
            "top_segments": segs.most_common(10),
            "extensions": exts.most_common(10),
            "tools": tools.most_common(8),
            "naming_vocab": tokens.most_common(15),
            "most_touched": paths.most_common(10),
        }

    # --- bash ---
    conn = sqlite3.connect(BASH_DB)
    rows = list(conn.execute(
        "SELECT program, subcommand, is_pipeline, is_redirect "
        "FROM bash_shapes WHERE session_id = ?", (sid,)))
    if rows:
        progs: Counter = Counter()
        subs_by_prog: dict[str, Counter] = {}
        pipes = reds = 0
        for prog, sub, p, r in rows:
            if prog: progs[prog] += 1
            if sub: subs_by_prog.setdefault(prog, Counter())[sub] += 1
            pipes += p; reds += r
        out["bash"] = {
            "n": len(rows),
            "programs": progs.most_common(10),
            "subcommands": {
                p: subs_by_prog[p].most_common(8)
                for p, _ in progs.most_common(5) if p in subs_by_prog
            },
            "pipeline_pct": round(100 * pipes / len(rows), 1),
            "redirect_pct": round(100 * reds / len(rows), 1),
        }
    return out


def prompt_sequence(sid: str) -> list[dict]:
    conn = sqlite3.connect(PROMPT_DB)
    rows = conn.execute(
        "SELECT timestamp, seq, prompt_excerpt FROM prompt_shapes "
        "WHERE session_id = ? ORDER BY timestamp", (sid,))
    return [{"ts": r[0], "seq": r[1], "excerpt": r[2]} for r in rows]


def hourly_rhythm(sid: str) -> dict[str, dict[str, int]]:
    """Return prompts/paths/bash counts per (MM-DD HH) bucket."""
    rhythm: dict[str, dict[str, int]] = defaultdict(lambda: {"prompts": 0, "paths": 0, "bash": 0})
    for db, key in ((PROMPT_DB, "prompts"), (PATH_DB, "paths"), (BASH_DB, "bash")):
        conn = sqlite3.connect(db)
        table = "prompt_shapes" if "prompt" in db else "path_shapes" if "path" in db else "bash_shapes"
        for (ts,) in conn.execute(f"SELECT timestamp FROM {table} WHERE session_id = ?", (sid,)):
            try:
                dt = datetime.fromisoformat(ts.replace("Z", "+00:00"))
                bucket = dt.strftime("%m-%d %H")
            except Exception:
                continue
            rhythm[bucket][key] += 1
    return dict(sorted(rhythm.items()))


def pr_and_git_activity(sid: str) -> list[dict]:
    conn = sqlite3.connect(BASH_DB)
    out = []
    for ts, raw in conn.execute(
        "SELECT timestamp, raw_command FROM bash_shapes WHERE session_id = ? "
        "AND (raw_command LIKE '%gh pr%' OR raw_command LIKE '%git push%' "
        "OR raw_command LIKE '%git merge%') ORDER BY timestamp", (sid,)):
        out.append({"ts": ts, "raw_command": raw[:200]})
    return out


def print_facts(sid: str) -> None:
    shape = shape_block(sid)
    prompts = prompt_sequence(sid)
    rhythm = hourly_rhythm(sid)
    git = pr_and_git_activity(sid)

    print(f"# session_story_facts: {sid}\n")

    p = shape.get("prompt") or {}
    if p:
        print(f"## prompt skeleton  (n={p['n']}, {p['first_ts'][:16]} → {p['last_ts'][:16]})")
        for k in ("verbs", "subjects", "objects", "chunks", "adjectives", "adverbs"):
            cells = ", ".join(f"{w}({n})" for w, n in p[k])
            print(f"  {k:<12} {cells}")

    pa = shape.get("path") or {}
    if pa:
        print(f"\n## path footprint  (n={pa['n']} touches, {pa['unique_paths']} unique)")
        for k in ("top_segments", "extensions", "tools", "naming_vocab"):
            cells = ", ".join(f"{w}({n})" for w, n in pa[k])
            print(f"  {k:<15} {cells}")
        print(f"  most_touched:")
        for path, n in pa["most_touched"]:
            print(f"    [{n:>3}x] {path}")

    b = shape.get("bash") or {}
    if b:
        print(f"\n## bash dialect  (n={b['n']})")
        print(f"  programs       {', '.join(f'{w}({n})' for w, n in b['programs'])}")
        for prog, subs in b["subcommands"].items():
            print(f"  {prog} subs:    {', '.join(f'{w}({n})' for w, n in subs)}")
        print(f"  pipelines: {b['pipeline_pct']}%   redirects: {b['redirect_pct']}%")

    print(f"\n## hourly rhythm  (prompts / paths / bash)")
    print(f"  {'hour':<10} {'prompts':>8} {'paths':>6} {'bash':>5}")
    for hour, counts in rhythm.items():
        print(f"  {hour:<10} {counts['prompts']:>8} {counts['paths']:>6} {counts['bash']:>5}")

    print(f"\n## PR & git activity  (n={len(git)})")
    for entry in git:
        print(f"  {entry['ts'][5:16].replace('T', ' ')}  {entry['raw_command']}")

    print(f"\n## prompt sequence  (n={len(prompts)})")
    for pr in prompts:
        t = pr["ts"][5:16].replace("T", " ")
        e = pr["excerpt"][:130].replace("\n", " ")
        print(f"  {t}  [{pr['seq']:>4}]  {e}")


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("session_id")
    ap.add_argument("--json", action="store_true", help="emit raw JSON for downstream tooling")
    args = ap.parse_args()
    if not _check():
        return 1
    if args.json:
        payload = {
            "session_id": args.session_id,
            "shape": shape_block(args.session_id),
            "prompts": prompt_sequence(args.session_id),
            "rhythm": hourly_rhythm(args.session_id),
            "pr_git_activity": pr_and_git_activity(args.session_id),
        }
        print(json.dumps(payload, indent=2, default=str))
    else:
        print_facts(args.session_id)
    return 0


if __name__ == "__main__":
    sys.exit(main())
