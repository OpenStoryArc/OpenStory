#!/usr/bin/env python3
# /// script
# requires-python = ">=3.10"
# dependencies = [
#     "spacy==3.7.5",
#     "en-core-web-sm @ https://github.com/explosion/spacy-models/releases/download/en_core_web_sm-3.7.1/en_core_web_sm-3.7.1-py3-none-any.whl",
# ]
# ///
"""Probe the *grammatical shape* of the user's opening prompts.

For each session's first user message, run spaCy's dependency parser and
extract:

- root verb (the imperative / declarative action)
- subject of that verb (often "you", "we", "I", or elided)
- direct object (what's being acted on)
- noun-phrase chunks (the "things" the prompt is about)
- adjectives + adverbs (the qualitative texture)

Aggregates over the most recent N sessions and splits the time range into
halves to compare early vs recent direction.

Usage:
    uv run scripts/_probe_prompt_shape.py
    uv run scripts/_probe_prompt_shape.py --limit 100
    uv run scripts/_probe_prompt_shape.py --json
"""

from __future__ import annotations

import argparse
import json
import sys
import urllib.request
from collections import Counter
from dataclasses import dataclass, field

import spacy

API = "http://localhost:3002/api"


def fetch_json(path: str) -> object:
    with urllib.request.urlopen(f"{API}{path}", timeout=15) as r:
        return json.loads(r.read())


def first_prompt(session_id: str) -> str:
    try:
        data = fetch_json(f"/sessions/{session_id}/records")
    except Exception:
        return ""
    records = data if isinstance(data, list) else data.get("records", [])
    for rec in records:
        if rec.get("record_type") != "user_message":
            continue
        if rec.get("is_sidechain"):
            continue
        content = (rec.get("payload") or {}).get("content", "")
        if isinstance(content, str) and content.strip():
            return content.strip()
    return ""


@dataclass
class Shape:
    session_id: str
    start_time: str
    branch: str
    prompt: str
    root_verbs: list[str] = field(default_factory=list)
    subjects: list[str] = field(default_factory=list)
    direct_objects: list[str] = field(default_factory=list)
    noun_chunks: list[str] = field(default_factory=list)
    adjectives: list[str] = field(default_factory=list)
    adverbs: list[str] = field(default_factory=list)


def parse_prompt(nlp, prompt: str, session_id: str, start_time: str, branch: str) -> Shape:
    shape = Shape(session_id=session_id, start_time=start_time, branch=branch, prompt=prompt)
    doc = nlp(prompt[:4000])  # cap input length

    for sent in doc.sents:
        # find root + its arguments
        for token in sent:
            if token.dep_ == "ROOT" and token.pos_ in {"VERB", "AUX"}:
                shape.root_verbs.append(token.lemma_.lower())
                for child in token.children:
                    if child.dep_ in {"nsubj", "nsubjpass"}:
                        shape.subjects.append(child.lemma_.lower())
                    elif child.dep_ in {"dobj", "obj"}:
                        shape.direct_objects.append(child.lemma_.lower())

    # all noun chunks (the "things" referenced)
    for chunk in doc.noun_chunks:
        text = chunk.lemma_.lower().strip()
        # filter pronouns and single-stopword chunks
        if len(text) > 1 and chunk.root.pos_ != "PRON":
            shape.noun_chunks.append(text)

    # adjectives + adverbs across the whole prompt
    for token in doc:
        if not token.lemma_.isalpha():
            continue
        if token.pos_ == "ADJ" and not token.is_stop:
            shape.adjectives.append(token.lemma_.lower())
        elif token.pos_ == "ADV" and not token.is_stop:
            shape.adverbs.append(token.lemma_.lower())

    return shape


# Sprint-9 templated prompts are agentic CI-step labels, not authentic
# semantic intent. Detect and exclude.
SPRINT_TEMPLATE_MARKERS = (
    "sprint 9, step",
    "feature slug:",
)


def is_templated(prompt: str) -> bool:
    low = prompt.lower()
    return any(marker in low for marker in SPRINT_TEMPLATE_MARKERS)


def aggregate(shapes: list[Shape]) -> dict[str, Counter]:
    agg = {
        "root_verbs": Counter(),
        "subjects": Counter(),
        "direct_objects": Counter(),
        "noun_chunks": Counter(),
        "adjectives": Counter(),
        "adverbs": Counter(),
    }
    for sh in shapes:
        for k in agg:
            agg[k].update(getattr(sh, k))
    return agg


def print_section(title: str, counter: Counter, top: int = 15) -> None:
    print(f"\n## {title}")
    if not counter:
        print("  (none)")
        return
    width = max(len(w) for w, _ in counter.most_common(top))
    for word, n in counter.most_common(top):
        print(f"  {word:<{width}}  {n}")


def diff_top(early: Counter, recent: Counter, top: int = 10) -> list[tuple[str, int, int, int]]:
    """Return tokens with the largest recent - early delta."""
    keys = set(early) | set(recent)
    rows = []
    for k in keys:
        e = early.get(k, 0)
        r = recent.get(k, 0)
        rows.append((k, e, r, r - e))
    rows.sort(key=lambda row: abs(row[3]), reverse=True)
    return rows[:top]


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--limit", type=int, default=60, help="number of most recent sessions to analyze")
    ap.add_argument("--top", type=int, default=15, help="top-N to show in each histogram")
    ap.add_argument("--json", action="store_true", help="emit raw JSON instead of pretty histograms")
    ap.add_argument("--keep-templated", action="store_true",
                    help="keep Sprint-N step prompts (default: exclude them)")
    ap.add_argument("--weekly", action="store_true",
                    help="show per-week histograms instead of early/recent split")
    args = ap.parse_args()

    print(f"loading spaCy en_core_web_sm...", file=sys.stderr)
    nlp = spacy.load("en_core_web_sm")

    print(f"fetching sessions...", file=sys.stderr)
    data = fetch_json("/sessions")
    sessions = data if isinstance(data, list) else data.get("sessions", [])
    sessions.sort(key=lambda s: s.get("start_time", ""), reverse=True)
    sessions = sessions[: args.limit]

    print(f"parsing {len(sessions)} opening prompts...", file=sys.stderr)
    shapes: list[Shape] = []
    excluded = 0
    for s in sessions:
        prompt = first_prompt(s["session_id"])
        if not prompt:
            continue
        if not args.keep_templated and is_templated(prompt):
            excluded += 1
            continue
        shapes.append(
            parse_prompt(
                nlp,
                prompt,
                s["session_id"],
                s.get("start_time", ""),
                s.get("branch", "") or "",
            )
        )
    if excluded:
        print(f"excluded {excluded} templated sprint-step prompts", file=sys.stderr)

    if not shapes:
        print("no prompts to analyze", file=sys.stderr)
        return 1

    # sort chronologically so we can split early/recent
    shapes.sort(key=lambda sh: sh.start_time)
    mid = len(shapes) // 2
    early, recent = shapes[:mid], shapes[mid:]

    if args.weekly:
        # bucket by ISO week
        from datetime import datetime
        buckets: dict[str, list[Shape]] = {}
        for sh in shapes:
            try:
                dt = datetime.fromisoformat(sh.start_time.replace("Z", "+00:00"))
                key = f"{dt.isocalendar()[0]}-W{dt.isocalendar()[1]:02d}"
            except Exception:
                key = "unknown"
            buckets.setdefault(key, []).append(sh)
        print(f"\n# weekly distribution")
        for week, group in sorted(buckets.items()):
            agg = aggregate(group)
            print(f"\n## {week}  (n={len(group)})  {group[0].start_time[:10]} → {group[-1].start_time[:10]}")
            for k in ("root_verbs", "direct_objects", "noun_chunks", "adjectives"):
                top = agg[k].most_common(8)
                if top:
                    print(f"  {k:<16} {', '.join(f'{w}({n})' for w, n in top)}")
        return 0

    total_agg = aggregate(shapes)
    early_agg = aggregate(early)
    recent_agg = aggregate(recent)

    if args.json:
        out = {
            "sessions_analyzed": len(shapes),
            "early_window": {
                "from": early[0].start_time if early else None,
                "to": early[-1].start_time if early else None,
                "n": len(early),
            },
            "recent_window": {
                "from": recent[0].start_time if recent else None,
                "to": recent[-1].start_time if recent else None,
                "n": len(recent),
            },
            "totals": {k: c.most_common(args.top) for k, c in total_agg.items()},
            "early": {k: c.most_common(args.top) for k, c in early_agg.items()},
            "recent": {k: c.most_common(args.top) for k, c in recent_agg.items()},
            "shifts": {
                k: diff_top(early_agg[k], recent_agg[k], args.top)
                for k in total_agg
            },
        }
        print(json.dumps(out, indent=2, default=str))
        return 0

    print(f"\n# prompt-shape probe")
    print(f"sessions analyzed: {len(shapes)}")
    print(f"early window:  {early[0].start_time[:10]} → {early[-1].start_time[:10]}  (n={len(early)})")
    print(f"recent window: {recent[0].start_time[:10]} → {recent[-1].start_time[:10]}  (n={len(recent)})")

    print("\n# overall distribution")
    for k in ("root_verbs", "subjects", "direct_objects", "noun_chunks", "adjectives", "adverbs"):
        print_section(k, total_agg[k], args.top)

    print("\n\n# shifts (recent − early)")
    for k in ("root_verbs", "direct_objects", "noun_chunks", "adjectives", "adverbs"):
        rows = diff_top(early_agg[k], recent_agg[k], args.top)
        print(f"\n## {k}")
        print(f"  {'token':<24}  early  recent  Δ")
        for word, e, r, d in rows:
            sign = "+" if d > 0 else ""
            print(f"  {word:<24}  {e:>5}  {r:>6}  {sign}{d}")

    return 0


if __name__ == "__main__":
    sys.exit(main())
