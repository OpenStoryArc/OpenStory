#!/usr/bin/env python3
# /// script
# requires-python = ">=3.10"
# ///
"""Probe the *horizon* of the project's vocabulary — words currently in the
"W11 'shape'-stage" of their lifecycle, before promotion to architecture.

The W11-shape signature is:
  - Present in docs-vocab (someone is writing about it)
  - Absent or near-absent in code-vocab (not yet a callable name)
  - Absent in path naming-tokens (not yet a filename)
  - Recent (in the last few weeks, not historical noise)

Words matching this signature are *architectures-in-waiting*. Some will
fade. A few will become next month's `prompt_shapes`-style proliferation.

Usage:
    uv run scripts/_probe_horizon.py
    uv run scripts/_probe_horizon.py --recent-days 21 --top 20
"""

from __future__ import annotations

import argparse
import json
import re
import sqlite3
import sys
from collections import Counter, defaultdict
from datetime import datetime, timedelta, timezone
from pathlib import Path

PROMPT_DB = "data/prompt_shapes.db"
PATH_DB = "data/path_shapes.db"
CODE_DB = "data/code_vocab_shapes.db"
DOCS_DB = "data/docs_vocab_shapes.db"

# Words too generic to be worth surfacing even if they hit the sieve.
GENERIC_STOP = {
    "thing", "things", "way", "ways", "part", "parts", "case", "cases",
    "side", "sides", "end", "ends", "kind", "kinds", "fact", "facts",
    "use", "uses", "user", "users", "type", "types", "value", "values",
    "data", "datum", "test", "tests", "file", "files", "code", "name",
    "names", "system", "systems", "place", "places", "point", "points",
    "term", "terms", "lot", "lots", "today", "yesterday", "tomorrow",
    "week", "weeks", "month", "year", "time", "times", "moment", "moments",
    "person", "persons", "people", "team", "teams", "thing's", "way's",
    "the time", "this time", "the user", "the system", "the data",
    "a lot", "a thing", "some way", "some kind", "the day", "the week",
    "this week", "next week", "last week", "the past", "the future",
    "the present", "the world", "the question", "the answer",
    "every time", "any time", "each time",
}


def parse_iso(ts: str) -> datetime:
    return datetime.fromisoformat(ts.replace("Z", "+00:00"))


def is_quality_token(tok: str) -> bool:
    """Filter for tokens that could be project-level concepts."""
    if not tok or len(tok) < 4 or len(tok) > 60:
        return False
    if tok.lower() in GENERIC_STOP:
        return False
    # at least one alphabetic char; no obvious noise patterns
    if not re.search(r"[a-zA-Z]", tok):
        return False
    # skip tokens that look like file paths, code fragments, urls
    if any(c in tok for c in "/\\<>{}|"):
        return False
    return True


def load_docs_counts(recent_cutoff: datetime):
    """Return per-token: total count, recent count, set of sessions."""
    total: Counter = Counter()
    recent: Counter = Counter()
    sessions: dict[str, set[str]] = defaultdict(set)
    first_seen: dict[str, str] = {}
    contexts: dict[str, list[str]] = defaultdict(list)

    conn = sqlite3.connect(DOCS_DB)
    for ts, sid, path, h, b, l, c in conn.execute(
        "SELECT timestamp, session_id, path, headers, bold_terms, link_labels, noun_chunks "
        "FROM docs_vocab_shapes ORDER BY timestamp"):
        try:
            dt = parse_iso(ts)
        except Exception:
            continue
        is_recent = dt >= recent_cutoff

        # gather all docs-derived terms with weights
        terms_here: Counter = Counter()
        for hdr in json.loads(h):
            hdr_l = hdr.lower().strip()
            if is_quality_token(hdr_l):
                terms_here[hdr_l] += 2  # headers count double — they're emphatic
        for term, n in json.loads(b).items():
            tl = term.lower().strip()
            if is_quality_token(tl):
                terms_here[tl] += n * 2  # bold counts double too
        for term, n in json.loads(l).items():
            tl = term.lower().strip()
            if is_quality_token(tl):
                terms_here[tl] += n
        for term, n in json.loads(c).items():
            tl = term.lower().strip()
            if is_quality_token(tl):
                terms_here[tl] += n

        for term, weight in terms_here.items():
            total[term] += weight
            if is_recent:
                recent[term] += weight
                sessions[term].add(sid)
            if term not in first_seen:
                first_seen[term] = ts
                contexts[term].append(f"{ts[:10]}  {path.split('/')[-1]}")

    return total, recent, sessions, first_seen, contexts


def load_code_path_presence():
    """For each term, how many times it appears in code-vocab + path naming-tokens."""
    code_count: Counter = Counter()
    path_count: Counter = Counter()

    conn = sqlite3.connect(CODE_DB)
    for (ids_json,) in conn.execute("SELECT identifiers FROM code_vocab_shapes"):
        for ident, n in json.loads(ids_json).items():
            code_count[ident.lower()] += n
            # also handle multi-word terms: doc terms like "shape layer" won't match
            # but doc terms like "userfilter" might map to identifier "userFilter"
            # we'll do exact-match-lower-cased + a contains check below at lookup time

    conn = sqlite3.connect(PATH_DB)
    for (toks_json,) in conn.execute("SELECT naming_tokens FROM path_shapes"):
        for tok in json.loads(toks_json):
            path_count[tok.lower()] += 1

    return code_count, path_count


def code_path_score(term: str, code_count: Counter, path_count: Counter) -> tuple[int, int]:
    """How present is this term in code + paths? Handles multi-word terms by
    checking whether any single content word is itself heavily code-present."""
    parts = re.split(r"\s+|-|_", term)
    parts = [p for p in parts if p and len(p) >= 3]

    # max single-word presence (catches "the shape" via "shape")
    code = max((code_count.get(p, 0) for p in parts), default=0)
    code = max(code, code_count.get(term, 0))

    path = max((path_count.get(p, 0) for p in parts), default=0)
    path = max(path, path_count.get(term, 0))

    return code, path


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--recent-days", type=int, default=21,
                    help="window for 'recent' docs activity (default 21)")
    ap.add_argument("--top", type=int, default=20)
    ap.add_argument("--min-docs", type=int, default=3,
                    help="minimum docs mentions in the recent window")
    ap.add_argument("--max-code", type=int, default=8,
                    help="maximum total code-vocab presence (any constituent word)")
    ap.add_argument("--max-path", type=int, default=2,
                    help="maximum total path naming-token presence")
    ap.add_argument("--min-sessions", type=int, default=2,
                    help="minimum sessions the term appears in (recent window)")
    args = ap.parse_args()

    for p in (DOCS_DB, CODE_DB, PATH_DB):
        if not Path(p).exists():
            print(f"missing db: {p}", file=sys.stderr)
            return 1

    # latest timestamp anywhere as the anchor; recent = anchor - N days
    conn = sqlite3.connect(DOCS_DB)
    (max_ts,) = conn.execute("SELECT MAX(timestamp) FROM docs_vocab_shapes").fetchone()
    anchor = parse_iso(max_ts)
    cutoff = anchor - timedelta(days=args.recent_days)
    print(f"anchor: {anchor.isoformat()[:16]}    recent window: last {args.recent_days} days "
          f"(>= {cutoff.isoformat()[:10]})\n", file=sys.stderr)

    total, recent, sessions, first_seen, contexts = load_docs_counts(cutoff)
    code_count, path_count = load_code_path_presence()

    # build candidate list
    rows = []
    for term, recent_count in recent.items():
        if recent_count < args.min_docs:
            continue
        if len(sessions[term]) < args.min_sessions:
            continue
        code_score, path_score = code_path_score(term, code_count, path_count)
        if code_score > args.max_code or path_score > args.max_path:
            continue
        # incubation score: recent docs activity, normalized by code presence
        # high = much-discussed, not-yet-built
        score = recent_count * len(sessions[term]) / (1 + code_score + path_score)
        rows.append({
            "term": term,
            "score": score,
            "docs_recent": recent_count,
            "docs_total": total[term],
            "sessions": len(sessions[term]),
            "code": code_score,
            "path": path_score,
            "first_seen": first_seen[term][:10],
            "contexts": contexts[term][:2],
        })

    rows.sort(key=lambda r: -r["score"])

    print(f"# horizon candidates  (incubating concepts: docs ✓  code/path nearly absent)\n")
    print(f"{'term':<32} {'docs_r':>6} {'sess':>4} {'code':>4} {'path':>4} {'first':<11} sample")
    print("-" * 120)
    for r in rows[:args.top]:
        sample = r["contexts"][0] if r["contexts"] else ""
        print(f"{r['term'][:32]:<32} {r['docs_recent']:>6} "
              f"{r['sessions']:>4} {r['code']:>4} {r['path']:>4} "
              f"{r['first_seen']:<11} {sample}")

    return 0


if __name__ == "__main__":
    sys.exit(main())
