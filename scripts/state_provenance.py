"""Reverse-provenance: given a file path, who wrote it, when, and why.

Hits the FTS search endpoint for the path basename, filters to tool_call
records that actually wrote/edited the file, then groups by session and
joins to that session's turn.sentence patterns to recover the prompt
that owned each write.

This is the answer to "why does this file look this way?" — not "what's
in it" but "what prompt caused these bytes to exist".

Usage:
    python3 scripts/state_provenance.py PATH
    python3 scripts/state_provenance.py PATH --json
    python3 scripts/state_provenance.py PATH --since 2026-04-01
    python3 scripts/state_provenance.py --test
"""

from __future__ import annotations

import argparse
import json
import os
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from collections import defaultdict
from dataclasses import asdict, dataclass


# Soft cap on candidate sessions per provenance query. Hot files (Sidebar.tsx,
# CLAUDE.md, etc.) match many FTS hits and overwhelming the local server with
# rapid-fire /records fetches triggers transient `RemoteDisconnected`. The cap
# keeps the answer bounded and focused on the most relevant sessions.
DEFAULT_SESSION_LIMIT = 40

# Small per-fetch delay when iterating candidate sessions, for the same reason.
THROTTLE_SECS = 0.05


DEFAULT_URL = "http://localhost:3002"


def fetch(base_url: str, path: str) -> object:
    try:
        with urllib.request.urlopen(f"{base_url}{path}", timeout=30) as resp:
            return json.loads(resp.read())
    except urllib.error.URLError as e:
        sys.stderr.write(f"error: failed to fetch {path}: {e}\n")
        sys.exit(2)


@dataclass
class Provenance:
    path: str
    session_id: str
    user: str | None
    project: str | None
    started_at: str
    write_count: int
    turns: list[int]                    # turn numbers that wrote it
    prompts: list[str]                  # human prompts that owned those turns


# -- Detection ------------------------------------------------------

def is_write_event(rec: dict, target_path: str) -> bool:
    if rec.get("record_type") not in ("tool_call",):
        return False
    payload = rec.get("payload") or {}
    if payload.get("name") not in ("Write", "Edit", "MultiEdit"):
        return False
    fp = ((payload.get("input") or {}).get("file_path")) or ""
    return fp == target_path


def matching_turns(records: list[dict], target_path: str) -> tuple[int, list[str]]:
    """Return (write_count, list of timestamps for those writes)."""
    times: list[str] = []
    for r in records:
        if is_write_event(r, target_path):
            times.append(r.get("timestamp", ""))
    return len(times), times


def _human_prompt(meta: dict) -> str:
    """Live shape is metadata.human.content; fall back to flat human_prompt."""
    h = meta.get("human")
    if isinstance(h, dict):
        return h.get("content") or ""
    return meta.get("human_prompt") or ""


def turn_for_time(sentences: list[dict], ts: str) -> tuple[int | None, str]:
    """Find the latest sentence whose started_at <= ts. Return (turn, prompt)."""
    if not ts:
        return None, ""
    pick: dict | None = None
    for s in sentences:
        if (s.get("started_at") or "") <= ts:
            pick = s
        else:
            break
    if not pick:
        return None, ""
    meta = pick.get("metadata") or {}
    return meta.get("turn"), _human_prompt(meta)


# -- Top-level ------------------------------------------------------

def provenance_for(base_url: str, path: str, since: str | None = None,
                   session_limit: int = DEFAULT_SESSION_LIMIT) -> list[Provenance]:
    """Search the corpus for sessions that wrote this exact path.

    Caps candidate sessions at session_limit (default 40, ranked by FTS
    relevance) to keep hot-file queries bounded. Throttles between session
    fetches to avoid overwhelming the local server.
    """
    basename = os.path.basename(path)
    q = urllib.parse.quote(basename)
    # Limit must stay ≤50; the local server's /api/search drops the connection
    # for larger limits on common terms. See BACKLOG: "/api/search drops at
    # limit≥200".
    res = fetch(base_url, f"/api/search?q={q}&limit=50")
    hits = res if isinstance(res, list) else (res.get("results") or [])

    # Preserve FTS rank order; dedup session ids.
    candidate_sessions: list[str] = []
    seen: set[str] = set()
    for h in hits:
        sid = h.get("session_id")
        if sid and sid not in seen:
            seen.add(sid)
            candidate_sessions.append(sid)
    candidate_sessions = candidate_sessions[:session_limit]

    # User/project live on /api/sessions, not /summary.
    sess_list = fetch(base_url, "/api/sessions")
    by_id = {s["session_id"]: s for s in
             (sess_list.get("sessions") or sess_list)}

    out: list[Provenance] = []
    for sid in candidate_sessions:
        time.sleep(THROTTLE_SECS)
        recs = fetch(base_url, f"/api/sessions/{sid}/records") or []
        if isinstance(recs, dict):
            recs = recs.get("records") or recs.get("items") or []
        count, times = matching_turns(recs, path)
        if count == 0:
            continue
        if since and times and times[0] < since:
            # earliest write before cutoff — still keep if any post-cutoff
            times = [t for t in times if t >= since]
            count = len(times)
            if count == 0:
                continue

        summ = by_id.get(sid) or fetch(base_url, f"/api/sessions/{sid}/summary") or {}
        pat_res = fetch(base_url, f"/api/sessions/{sid}/patterns?type=turn.sentence") or {}
        sents = pat_res.get("patterns", []) if isinstance(pat_res, dict) else []
        sents.sort(key=lambda s: s.get("started_at", ""))

        turns: list[int] = []
        prompts: list[str] = []
        seen_prompts: set[str] = set()
        for t in times:
            tn, pr = turn_for_time(sents, t)
            if tn is not None and tn not in turns:
                turns.append(tn)
            if pr and pr not in seen_prompts:
                seen_prompts.add(pr)
                prompts.append(pr[:200].replace("\n", " / "))

        out.append(Provenance(
            path=path,
            session_id=sid,
            user=summ.get("user"),
            project=summ.get("project_name"),
            started_at=summ.get("start_time") or summ.get("first_event") or "",
            write_count=count,
            turns=turns,
            prompts=prompts,
        ))
    out.sort(key=lambda p: p.started_at)
    return out


def fmt_md(rows: list[Provenance]) -> str:
    if not rows:
        return "_no writes found for that path in the corpus_\n"
    path = rows[0].path
    total = sum(r.write_count for r in rows)
    out = [f"# Provenance: `{path}`",
           f"**Total writes across corpus:** {total}  ·  **Sessions:** {len(rows)}\n"]
    for r in rows:
        out.append(f"## {r.session_id} — {r.user or '?'} · {r.project or '?'}")
        out.append(f"_{r.started_at[:19]}_  ·  {r.write_count} write(s)  ·  turns {r.turns}")
        if r.prompts:
            out.append("")
            for p in r.prompts:
                out.append(f"- {p}")
        out.append("")
    return "\n".join(out)


# -- Self-tests -----------------------------------------------------

def _records_with_writes(path: str) -> list[dict]:
    return [
        {"record_type": "tool_call", "timestamp": "2026-05-07T01:00:00Z",
         "payload": {"name": "Write", "input": {"file_path": path}}},
        {"record_type": "tool_call", "timestamp": "2026-05-07T02:00:00Z",
         "payload": {"name": "Read", "input": {"file_path": path}}},
        {"record_type": "tool_call", "timestamp": "2026-05-07T03:00:00Z",
         "payload": {"name": "Edit", "input": {"file_path": path}}},
        {"record_type": "tool_call", "timestamp": "2026-05-07T04:00:00Z",
         "payload": {"name": "Edit", "input": {"file_path": "/other"}}},
    ]


def _sentences() -> list[dict]:
    return [
        {"started_at": "2026-05-07T00:30:00Z",
         "metadata": {"turn": 1, "human_prompt": "let's start"}},
        {"started_at": "2026-05-07T01:00:00Z",
         "metadata": {"turn": 2, "human_prompt": "write the file"}},
        {"started_at": "2026-05-07T03:00:00Z",
         "metadata": {"turn": 3, "human_prompt": "tweak it"}},
    ]


def selftest() -> int:
    failures = 0
    def check(name, cond, detail=""):
        nonlocal failures
        print(f"  {'ok  ' if cond else 'FAIL'} {name}{('   '+detail) if not cond else ''}")
        if not cond: failures += 1

    target = "/x/y.rs"
    print("== matching_turns ==")
    n, ts = matching_turns(_records_with_writes(target), target)
    check("two writes", n == 2, str(n))
    check("not Read or other path", len(ts) == 2)

    print("== turn_for_time ==")
    sents = _sentences()
    sents.sort(key=lambda s: s["started_at"])
    tn, pr = turn_for_time(sents, "2026-05-07T01:00:00Z")
    check("turn 2 owns the 01:00 write", tn == 2, str(tn))
    check("prompt captured", "write the file" in pr)
    tn2, pr2 = turn_for_time(sents, "2026-05-07T03:00:00Z")
    check("turn 3 owns the 03:00 write", tn2 == 3, str(tn2))

    print("== fmt_md ==")
    p = Provenance(path=target, session_id="S1", user="u", project="P",
                   started_at="2026-05-07T00:00:00Z", write_count=2,
                   turns=[2, 3], prompts=["write the file", "tweak it"])
    md = fmt_md([p])
    check("md has path", target in md)
    check("md lists prompts", "tweak it" in md)

    print("\nFAILED" if failures else "\nall tests passed")
    return 1 if failures else 0


# -- CLI -----------------------------------------------------------

def main() -> None:
    p = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    p.add_argument("path", nargs="?")
    p.add_argument("--url", default=DEFAULT_URL)
    p.add_argument("--since", help="ISO timestamp lower bound")
    p.add_argument("--limit", type=int, default=DEFAULT_SESSION_LIMIT,
                   help=f"max candidate sessions to scan (default {DEFAULT_SESSION_LIMIT})")
    p.add_argument("--json", action="store_true")
    p.add_argument("--test", action="store_true")
    args = p.parse_args()

    if args.test:
        sys.exit(selftest())
    if not args.path:
        p.error("path required (or --test)")

    rows = provenance_for(args.url, args.path, since=args.since, session_limit=args.limit)
    if args.json:
        print(json.dumps([asdict(r) for r in rows], indent=2))
    else:
        print(fmt_md(rows))


if __name__ == "__main__":
    main()
