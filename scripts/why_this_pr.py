"""Reconstruct the "why" behind a PR by joining its session's prompt trail.

Given a session id (or 'all' to scan the corpus), find every `gh pr create`
event, then walk the session's turn.sentence patterns to render the
prompt → plan → ship trail behind each PR.

Companion to sessionstory.py. That one is session-shaped; this one is
PR-shaped: the unit of analysis is one PR, not one session.

Usage:
    python3 scripts/why_this_pr.py SESSION_ID            # markdown report
    python3 scripts/why_this_pr.py SESSION_ID --json     # machine-readable
    python3 scripts/why_this_pr.py all                   # all PR sessions in store
    python3 scripts/why_this_pr.py all --csv             # csv: one row per PR
    python3 scripts/why_this_pr.py --test                # run self-tests
"""

from __future__ import annotations

import argparse
import json
import re
import sys
import urllib.error
import urllib.request
from dataclasses import asdict, dataclass, field
from typing import Iterable


DEFAULT_URL = "http://localhost:3002"
PR_CREATE_RE = re.compile(r"gh\s+pr\s+create\b")
PR_TITLE_RE = re.compile(r"--title\s+\\?\"([^\"]+)\\?\"")


def fetch(base_url: str, path: str) -> object:
    url = f"{base_url}{path}"
    try:
        with urllib.request.urlopen(url, timeout=30) as resp:
            return json.loads(resp.read())
    except urllib.error.URLError as e:
        sys.stderr.write(f"error: failed to fetch {url}: {e}\n")
        sys.exit(2)


@dataclass
class PrEvent:
    session_id: str
    event_id: str
    timestamp: str
    title: str  # extracted from --title flag, "" if not parseable
    command: str  # full raw command for context


@dataclass
class WhyPr:
    pr: PrEvent
    project_name: str | None
    branch: str | None
    user: str | None
    duration_hours: float
    sentence_count: int
    prompt_trail: list[dict]  # [{turn, time, prompt, summary, verb}]
    plan_writes: list[str]    # paths under */plans/*
    commits: list[str]        # commit titles seen on the same session


# -- PR detection -----------------------------------------------------

def find_pr_events(records: Iterable[dict]) -> list[PrEvent]:
    """Scan records for tool_call events whose payload command runs gh pr create."""
    out: list[PrEvent] = []
    for r in records:
        if r.get("record_type") != "tool_call":
            continue
        payload = r.get("payload") or {}
        if payload.get("name") != "Bash":
            continue
        cmd = ((payload.get("input") or {}).get("command")) or ""
        if not PR_CREATE_RE.search(cmd):
            continue
        title_match = PR_TITLE_RE.search(cmd)
        out.append(PrEvent(
            session_id=r.get("session_id", ""),
            event_id=r.get("id", ""),
            timestamp=r.get("timestamp", ""),
            title=(title_match.group(1) if title_match else ""),
            command=cmd[:600],
        ))
    return out


# -- Prompt trail ----------------------------------------------------

def human_prompt(meta: dict) -> str:
    """Extract the human prompt content from sentence metadata.

    The live REST shape is metadata.human.content (a dict). Earlier MCP tooling
    flattened it to metadata.human_prompt — accept both for safety.
    """
    h = meta.get("human")
    if isinstance(h, dict):
        return h.get("content") or ""
    return meta.get("human_prompt") or meta.get("prompt") or ""


def trail_from_sentences(sentences: list[dict]) -> list[dict]:
    """Build a flat trail of (turn, time, prompt, summary, verb) from turn.sentence patterns."""
    trail: list[dict] = []
    for s in sentences:
        meta = s.get("metadata") or {}
        trail.append({
            "turn": meta.get("turn"),
            "time": (s.get("started_at") or "")[:19],
            "verb": meta.get("verb") or "",
            "prompt": human_prompt(meta)[:240].replace("\n", " / "),
            "summary": (s.get("summary") or "")[:200].replace("\n", " / "),
        })
    trail.sort(key=lambda x: (x.get("turn") or 0))
    return trail


# -- Plans + commits ------------------------------------------------

def find_plan_writes(records: Iterable[dict]) -> list[str]:
    out: list[str] = []
    seen: set[str] = set()
    for r in records:
        if r.get("record_type") != "tool_call":
            continue
        payload = r.get("payload") or {}
        if payload.get("name") not in ("Write", "Edit"):
            continue
        path = ((payload.get("input") or {}).get("file_path")) or ""
        if "/plans/" in path or path.endswith("plan.md"):
            if path not in seen:
                seen.add(path)
                out.append(path)
    return out


COMMIT_RE = re.compile(r"git\s+commit\b[^\n]*-m\s+\"([^\"]+)\"")
HEREDOC_RE = re.compile(r"git\s+commit\b[^\n]*-m\s+\"\$\(cat\s*<<\s*['\"]?EOF['\"]?\s*\n+([^\n]+)")

def find_commits(records: Iterable[dict]) -> list[str]:
    """Extract commit subjects. Handles inline -m "msg" and HEREDOC forms.

    HEREDOC: `git commit -m "$(cat <<'EOF'\n<subject>\n<body>\nEOF\n)"` — we
    take the first non-empty line after EOF as the subject.
    """
    out: list[str] = []
    for r in records:
        if r.get("record_type") != "tool_call":
            continue
        payload = r.get("payload") or {}
        if payload.get("name") != "Bash":
            continue
        cmd = ((payload.get("input") or {}).get("command")) or ""
        m = HEREDOC_RE.search(cmd)
        if m:
            out.append(m.group(1).strip()[:120])
            continue
        m = COMMIT_RE.search(cmd)
        if m:
            out.append(m.group(1)[:120])
    return out


# -- Top-level reconstruction --------------------------------------

def session_meta(base_url: str, session_id: str) -> dict:
    """Fetch user/project/branch/host from /api/sessions (which carries them).

    /api/sessions/{id}/summary does not expose user/host/branch — those live
    on the list response. We fetch the whole list once. Caller can pass an
    already-loaded list to avoid the round-trip.
    """
    res = fetch(base_url, "/api/sessions")
    sess_list = res.get("sessions", []) if isinstance(res, dict) else res
    for s in sess_list:
        if s.get("session_id") == session_id:
            return s
    return {}


def why_for_session(base_url: str, session_id: str, sess_meta: dict | None = None) -> list[WhyPr]:
    """Return one WhyPr per `gh pr create` in this session."""
    if sess_meta is None:
        sess_meta = session_meta(base_url, session_id)
    summary = fetch(base_url, f"/api/sessions/{session_id}/summary") or {}
    records = fetch(base_url, f"/api/sessions/{session_id}/records") or []
    if isinstance(records, dict):
        records = records.get("records") or records.get("items") or []
    patterns = fetch(base_url, f"/api/sessions/{session_id}/patterns?type=turn.sentence") or {}
    sentences = patterns.get("patterns", []) if isinstance(patterns, dict) else []

    prs = find_pr_events(records)
    if not prs:
        return []
    trail = trail_from_sentences(sentences)
    plans = find_plan_writes(records)
    commits = find_commits(records)

    started = sess_meta.get("start_time") or summary.get("first_event") or ""
    ended = sess_meta.get("last_event") or summary.get("last_event") or ""
    dur = round(((iso_seconds(ended) - iso_seconds(started)) / 3600), 2) if started and ended else 0.0

    return [
        WhyPr(
            pr=pr,
            project_name=sess_meta.get("project_name") or summary.get("project_id"),
            branch=sess_meta.get("branch"),
            user=sess_meta.get("user"),
            duration_hours=dur,
            sentence_count=len(sentences),
            prompt_trail=trail,
            plan_writes=plans,
            commits=commits,
        )
        for pr in prs
    ]


def iso_seconds(ts: str) -> float:
    if not ts:
        return 0.0
    from datetime import datetime
    try:
        return datetime.fromisoformat(ts.replace("Z", "+00:00")).timestamp()
    except ValueError:
        return 0.0


def all_pr_sessions(base_url: str) -> list[str]:
    """Scan the corpus FTS for `gh pr create`, return distinct session ids."""
    res = fetch(base_url, "/api/search?q=%22gh+pr+create%22&limit=200")
    hits = res if isinstance(res, list) else (res.get("results") or [])
    seen: list[str] = []
    s: set[str] = set()
    for h in hits:
        sid = h.get("session_id")
        if sid and sid not in s:
            s.add(sid)
            seen.append(sid)
    return seen


# -- Output formats ------------------------------------------------

def fmt_md(reports: list[WhyPr]) -> str:
    if not reports:
        return "_no PRs found in this session_\n"
    lines: list[str] = []
    for w in reports:
        lines.append(f"# Why this PR")
        lines.append(f"**Title:** {w.pr.title or '(no --title parsed)'}")
        lines.append(f"**Session:** `{w.pr.session_id}`")
        lines.append(f"**User / Project / Branch:** {w.user or '?'} · {w.project_name or '?'} · {w.branch or '?'}")
        lines.append(f"**Duration:** {w.duration_hours}h  ·  **Sentences:** {w.sentence_count}  ·  **Plans:** {len(w.plan_writes)}  ·  **Commits:** {len(w.commits)}")
        lines.append("")
        if w.plan_writes:
            lines.append("## Plans written")
            for p in w.plan_writes:
                lines.append(f"- `{p}`")
            lines.append("")
        if w.prompt_trail:
            lines.append("## Prompt trail")
            for t in w.prompt_trail:
                lines.append(f"- **t{t['turn']} {t['time']}** _{t['verb']}_ — {t['prompt']}")
            lines.append("")
        if w.commits:
            lines.append("## Commits on this session")
            for c in w.commits:
                lines.append(f"- {c}")
            lines.append("")
    return "\n".join(lines)


def fmt_csv(reports: list[WhyPr]) -> str:
    rows = ["session_id,user,project,branch,pr_title,duration_h,sentences,plans,commits"]
    for w in reports:
        title = (w.pr.title or "").replace(",", " ").replace("\n", " ")
        rows.append(",".join([
            w.pr.session_id, w.user or "", w.project_name or "",
            w.branch or "", f'"{title}"', f"{w.duration_hours}",
            str(w.sentence_count), str(len(w.plan_writes)), str(len(w.commits)),
        ]))
    return "\n".join(rows) + "\n"


def fmt_json(reports: list[WhyPr]) -> str:
    return json.dumps([asdict(w) for w in reports], indent=2)


# -- Self-tests -----------------------------------------------------

def _fixture_records() -> list[dict]:
    return [
        {"record_type": "user_message", "id": "u1",
         "timestamp": "2026-05-07T00:31:43Z",
         "payload": {"content": "Help me ship X"}},
        {"record_type": "tool_call", "id": "tc1", "session_id": "S1",
         "timestamp": "2026-05-07T00:32:00Z",
         "payload": {"name": "Write",
                     "input": {"file_path": "/Users/x/.claude/plans/foo.md", "content": "..."}}},
        {"record_type": "tool_call", "id": "tc2", "session_id": "S1",
         "timestamp": "2026-05-07T01:00:00Z",
         "payload": {"name": "Bash",
                     "input": {"command": "git commit -m \"feat: ship X\""}}},
        {"record_type": "tool_call", "id": "tc3", "session_id": "S1",
         "timestamp": "2026-05-07T01:05:00Z",
         "payload": {"name": "Bash",
                     "input": {"command": "gh pr create --title \"feat: ship X\" --body 'why'"}}},
    ]


def _fixture_sentences() -> list[dict]:
    return [
        {"started_at": "2026-05-07T00:31:43Z",
         "summary": "Claude explained the plan",
         "metadata": {"turn": 1, "verb": "explained",
                      "human_prompt": "Help me ship X"}},
        {"started_at": "2026-05-07T01:05:00Z",
         "summary": "Claude pushed the PR",
         "metadata": {"turn": 2, "verb": "committed",
                      "human_prompt": "ship it"}},
    ]


def selftest() -> int:
    failures = 0
    def check(name: str, cond: bool, detail: str = "") -> None:
        nonlocal failures
        if cond: print(f"  ok   {name}")
        else: failures += 1; print(f"  FAIL {name}: {detail}")

    print("== find_pr_events ==")
    prs = find_pr_events(_fixture_records())
    check("one PR event found", len(prs) == 1, str(len(prs)))
    check("title parsed", prs[0].title == "feat: ship X", prs[0].title)
    check("session id propagated", prs[0].session_id == "S1")

    print("== find_plan_writes ==")
    plans = find_plan_writes(_fixture_records())
    check("plan path detected", any("plans/foo.md" in p for p in plans))

    print("== find_commits ==")
    commits = find_commits(_fixture_records())
    check("commit subject parsed", commits == ["feat: ship X"])

    print("== trail_from_sentences ==")
    trail = trail_from_sentences(_fixture_sentences())
    check("trail length", len(trail) == 2)
    check("turn ordering", trail[0]["turn"] == 1 and trail[1]["turn"] == 2)
    check("verb captured", trail[0]["verb"] == "explained")

    print("== fmt_md / fmt_csv / fmt_json ==")
    w = WhyPr(pr=prs[0], project_name="Demo", branch="feat/x", user="u",
              duration_hours=1.0, sentence_count=2, prompt_trail=trail,
              plan_writes=plans, commits=commits)
    md = fmt_md([w])
    check("md has header", "Why this PR" in md)
    check("md lists trail", "explained" in md and "Help me ship X" in md)
    csv = fmt_csv([w])
    check("csv has session id", "S1" in csv)
    js = json.loads(fmt_json([w]))
    check("json round-trips", js[0]["pr"]["title"] == "feat: ship X")

    print("\nFAILED" if failures else "\nall tests passed")
    return 1 if failures else 0


# -- CLI -----------------------------------------------------------

def main() -> None:
    p = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    p.add_argument("session_id", nargs="?", help="session id, or 'all'")
    p.add_argument("--url", default=DEFAULT_URL)
    p.add_argument("--json", action="store_true")
    p.add_argument("--csv", action="store_true")
    p.add_argument("--test", action="store_true")
    args = p.parse_args()

    if args.test:
        sys.exit(selftest())
    if not args.session_id:
        p.error("session_id required (or use 'all', --test)")

    if args.session_id == "all":
        sids = all_pr_sessions(args.url)
        sys.stderr.write(f"# scanning {len(sids)} sessions with `gh pr create` events\n")
        all_reports: list[WhyPr] = []
        for sid in sids:
            try:
                all_reports.extend(why_for_session(args.url, sid))
            except SystemExit:
                continue
        if args.json:
            print(fmt_json(all_reports))
        elif args.csv:
            print(fmt_csv(all_reports))
        else:
            print(fmt_md(all_reports))
        return

    reports = why_for_session(args.url, args.session_id)
    if args.json:
        print(fmt_json(reports))
    elif args.csv:
        print(fmt_csv(reports))
    else:
        print(fmt_md(reports))


if __name__ == "__main__":
    main()
