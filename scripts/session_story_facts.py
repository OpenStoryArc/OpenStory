#!/usr/bin/env python3
# /// script
# requires-python = ">=3.10"
# ///
"""Gather shape-layer facts for a single-session story.

Companion to `scripts/sessionstory.py`. That script reads OpenStory's
record/pattern API and emits the baseline fact sheet (record types, tool
histograms, eval-apply patterns, turn.sentence detector output, prompt
timeline). This one reads the **seven shape-layer databases** under
`data/*_shapes.db` and emits complementary fact blocks that ground a
richer narrative:

  1. Seven-layer shape report
     - prompt skeleton   (intent — verbs / objects / chunks / modifiers)
     - path footprint    (attention — top segments / extensions / naming-vocab)
     - bash dialect      (action — programs / subcommands / pipeline rate)
     - change profile    (deltas — lines added/removed, top-changed files)
     - code vocabulary   (interior — identifiers per language, top names)
     - docs vocabulary   (prose — headers, named concepts, link labels)
     - snapshot timeline (working-set — tracked files growth, version bumps)
  2. Prompt sequence            — every prompt in time order with HH:MM stamps
  3. Hourly rhythm              — prompts/paths/bash per hour (reveals working
                                  blocks and natural breaks/sleep gaps)
  4. PR & git activity          — every `gh pr` / `git push` / `git merge`
                                  command with its timestamp

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
CHANGE_DB = "data/change_shapes.db"
VOCAB_DB = "data/code_vocab_shapes.db"
DOCS_DB = "data/docs_vocab_shapes.db"
SNAP_DB = "data/snapshot_shapes.db"

ALL_DBS = [
    ("prompt-shape",   PROMPT_DB),
    ("path-shape",     PATH_DB),
    ("bash-shape",     BASH_DB),
    ("change-shape",   CHANGE_DB),
    ("code-vocab",     VOCAB_DB),
    ("docs-vocab",     DOCS_DB),
    ("snapshot-shape", SNAP_DB),
]


def _check() -> bool:
    # The first three are required. Others are nice-to-have; warn but don't fail.
    required = [PROMPT_DB, PATH_DB, BASH_DB]
    missing_required = [p for p in required if not Path(p).exists()]
    if missing_required:
        print(f"missing required layer dbs: {missing_required}", file=sys.stderr)
        print(f"build them: uv run scripts/build_{{prompt,path,bash}}_shapes.py", file=sys.stderr)
        return False
    optional = [CHANGE_DB, VOCAB_DB, DOCS_DB, SNAP_DB]
    missing_optional = [p for p in optional if not Path(p).exists()]
    if missing_optional:
        print(f"note: optional layers missing (story will skip them): {missing_optional}", file=sys.stderr)
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

    # --- change (Edit/Write/MultiEdit deltas) ---
    if Path(CHANGE_DB).exists():
        conn = sqlite3.connect(CHANGE_DB)
        rows = list(conn.execute(
            "SELECT tool, path, lines_added, lines_removed, chars_added, chars_removed "
            "FROM change_shapes WHERE session_id = ?", (sid,)))
        if rows:
            tools_c: Counter = Counter()
            per_path: dict[str, list[int]] = {}
            total_la = total_lr = total_ca = total_cr = 0
            for tool, path, la, lr, ca, cr in rows:
                tools_c[tool] += 1
                per_path.setdefault(path, [0, 0, 0])
                per_path[path][0] += 1
                per_path[path][1] += la
                per_path[path][2] += lr
                total_la += la; total_lr += lr; total_ca += ca; total_cr += cr
            top_paths = sorted(per_path.items(), key=lambda kv: -kv[1][1])[:10]
            out["change"] = {
                "n": len(rows),
                "by_tool": tools_c.most_common(),
                "lines_added": total_la,
                "lines_removed": total_lr,
                "lines_net": total_la - total_lr,
                "chars_added": total_ca,
                "chars_removed": total_cr,
                "top_paths": [
                    {"path": p, "edits": v[0], "lines_added": v[1], "lines_removed": v[2]}
                    for p, v in top_paths
                ],
            }

    # --- code-vocab (identifiers from new code) ---
    if Path(VOCAB_DB).exists():
        conn = sqlite3.connect(VOCAB_DB)
        rows = list(conn.execute(
            "SELECT language, identifiers, total_idents FROM code_vocab_shapes "
            "WHERE session_id = ?", (sid,)))
        if rows:
            by_lang: dict[str, Counter] = defaultdict(Counter)
            lang_events: Counter = Counter()
            total_inst = 0
            for lang, ident_json, total in rows:
                by_lang[lang].update(json.loads(ident_json))
                lang_events[lang] += 1
                total_inst += total
            combined: Counter = Counter()
            for c in by_lang.values():
                combined.update(c)
            out["code_vocab"] = {
                "n": len(rows),
                "total_identifier_instances": total_inst,
                "unique_identifiers": len(combined),
                "by_language": {
                    lang: {
                        "events": lang_events[lang],
                        "unique": len(by_lang[lang]),
                        "top": by_lang[lang].most_common(12),
                    }
                    for lang in by_lang
                },
                "top_overall": combined.most_common(20),
            }

    # --- docs-vocab (headers + named concepts from prose) ---
    if Path(DOCS_DB).exists():
        conn = sqlite3.connect(DOCS_DB)
        rows = list(conn.execute(
            "SELECT path, extension, headers, bold_terms, link_labels, noun_chunks "
            "FROM docs_vocab_shapes WHERE session_id = ?", (sid,)))
        if rows:
            headers: list[str] = []
            bold: Counter = Counter()
            links: Counter = Counter()
            chunks: Counter = Counter()
            paths: Counter = Counter()
            exts: Counter = Counter()
            for path, ext, h, b, l, c in rows:
                headers.extend(json.loads(h))
                bold.update(json.loads(b))
                links.update(json.loads(l))
                chunks.update(json.loads(c))
                paths[path] += 1
                exts[ext] += 1
            out["docs_vocab"] = {
                "n": len(rows),
                "paths_touched": paths.most_common(8),
                "extensions": exts.most_common(),
                "headers": headers[:25],
                "top_noun_chunks": chunks.most_common(20),
                "top_bold_terms": bold.most_common(10),
                "top_link_labels": links.most_common(10),
            }

    # --- snapshot (working-set timeline) ---
    if Path(SNAP_DB).exists():
        conn = sqlite3.connect(SNAP_DB)
        rows = list(conn.execute(
            "SELECT timestamp, tracked_count, new_files, bumped_files, max_version "
            "FROM snapshot_shapes WHERE session_id = ? ORDER BY timestamp, seq", (sid,)))
        if rows:
            tracked_end = rows[-1][1]
            max_v_end = rows[-1][4]
            total_new = sum(r[2] for r in rows)
            total_bumped = sum(r[3] for r in rows)
            # working-set growth — sample 8 points across the session
            n = len(rows)
            step = max(1, n // 8)
            samples = []
            for i in range(0, n, step):
                ts, tracked, nf, bf, mv = rows[i]
                samples.append({"ts": ts, "tracked": tracked, "new": nf,
                               "bumped": bf, "max_version": mv})
            out["snapshot"] = {
                "n": n,
                "final_tracked": tracked_end,
                "final_max_version": max_v_end,
                "total_new_files": total_new,
                "total_bumped_files": total_bumped,
                "samples": samples,
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

    ch = shape.get("change") or {}
    if ch:
        print(f"\n## change profile  (n={ch['n']} change events)")
        print(f"  by_tool        {', '.join(f'{t}({n})' for t, n in ch['by_tool'])}")
        print(f"  lines          +{ch['lines_added']:,}  -{ch['lines_removed']:,}  (net +{ch['lines_net']:,})")
        print(f"  chars          +{ch['chars_added']:,}  -{ch['chars_removed']:,}")
        print(f"  top-changed files:")
        for entry in ch['top_paths']:
            short = entry['path'][-80:]
            print(f"    +{entry['lines_added']:>5} -{entry['lines_removed']:>5}  edits={entry['edits']:>3}  {short}")

    cv = shape.get("code_vocab") or {}
    if cv:
        print(f"\n## code vocabulary  ({cv['n']} events, {cv['total_identifier_instances']:,} instances, {cv['unique_identifiers']:,} unique)")
        for lang, info in cv['by_language'].items():
            print(f"  [{lang}]  events={info['events']:<3} unique_idents={info['unique']:<5}")
            print(f"      top: {', '.join(f'{w}({n})' for w, n in info['top'])}")
        print(f"  top across all languages:")
        for ident, n in cv['top_overall'][:15]:
            print(f"    {ident:<28} {n}")

    dv = shape.get("docs_vocab") or {}
    if dv:
        print(f"\n## docs vocabulary  ({dv['n']} prose events)")
        print(f"  paths_touched: {', '.join(f'{p.split(chr(47))[-1]}({n})' for p, n in dv['paths_touched'])}")
        print(f"  extensions:    {', '.join(f'{e}({n})' for e, n in dv['extensions'])}")
        if dv['headers']:
            print(f"  headers written:")
            for h in dv['headers'][:15]:
                print(f"    “{h[:80]}”")
        if dv['top_noun_chunks']:
            print(f"  top noun chunks: {', '.join(f'{c}({n})' for c, n in dv['top_noun_chunks'][:12])}")
        if dv['top_bold_terms']:
            print(f"  top bold terms:  {', '.join(f'{w}({n})' for w, n in dv['top_bold_terms'][:8])}")

    sn = shape.get("snapshot") or {}
    if sn:
        print(f"\n## working-memory timeline  ({sn['n']} snapshots)")
        print(f"  final tracked_count: {sn['final_tracked']}   final max_version: {sn['final_max_version']}")
        print(f"  total new files: {sn['total_new_files']}   total version bumps: {sn['total_bumped_files']}")
        print(f"  growth samples:")
        print(f"    {'ts':<17} {'tracked':>8} {'new':>5} {'bumped':>7} {'max_v':>6}")
        for s in sn['samples']:
            print(f"    {s['ts'][:16]:<17} {s['tracked']:>8} {s['new']:>5} {s['bumped']:>7} {s['max_version']:>6}")

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
