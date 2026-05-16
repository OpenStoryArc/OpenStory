"""Cross-reference every merged + open PR with its OpenStory session and
emit a markdown retrospective.

For each PR returned by `gh pr list`, find the session that ran the
matching `gh pr create --title "<title>"` command, then walk the session
to recover its prompt trail, plans, and commits. PRs that predate
OpenStory's listening of itself (early March 2026) get a "no session"
note rather than being dropped.

The output is the deterministic skeleton — facts only. Hand-written
narrative arcs can be layered on top selectively (the 90/10 split).

Run:
    python3 scripts/pr_retrospective.py > docs/research/introspection-scripts/pr-retrospective.md
"""

from __future__ import annotations

import json
import re
import subprocess
import sys
import urllib.error
import urllib.request
from collections import defaultdict
from dataclasses import dataclass, field
from datetime import datetime, timezone

import why_this_pr


URL = "http://localhost:3002"

# Prompts to filter — these are harness injections, not user intent.
NOISE_PREFIXES = (
    "<system-reminder>",
    "<task-notification>",
    "<command-name>",
    "<local-command-",
    "[Image: source:",
    "[Request interrupted",
    "Caveat: The messages below",
)


def is_noisy_prompt(p: str) -> bool:
    if not p:
        return True
    p = p.lstrip()
    return any(p.startswith(pref) for pref in NOISE_PREFIXES)


# -- gh PR list ---------------------------------------------------

def gh_prs(state: str) -> list[dict]:
    """Run `gh pr list` and return the JSON. State is 'merged' or 'open'."""
    cmd = [
        "gh", "pr", "list", "--state", state, "--limit", "200",
        "--json", "number,title,state,mergedAt,createdAt,author,baseRefName,headRefName,additions,deletions,isDraft",
    ]
    out = subprocess.run(cmd, capture_output=True, text=True, check=True)
    return json.loads(out.stdout)


# -- Session matching ---------------------------------------------

def title_key(title: str) -> str:
    """Normalize a PR title for fuzzy matching against gh pr create --title.
    Strip whitespace and lowercase. We don't try harder than this — gh edits
    the title sometimes."""
    return re.sub(r"\s+", " ", title or "").strip().lower()


def candidate_sessions() -> list[str]:
    """Find every session that contains a `gh pr create` OR `gh pr edit` event."""
    seen: set[str] = set()
    ordered: list[str] = []
    for q in ("%22gh+pr+create%22", "%22gh+pr+edit%22"):
        try:
            res = json.loads(urllib.request.urlopen(
                f"{URL}/api/search?q={q}&limit=100", timeout=30
            ).read())
        except urllib.error.URLError:
            continue
        hits = res if isinstance(res, list) else (res.get("results") or [])
        for h in hits:
            sid = h.get("session_id")
            if sid and sid not in seen:
                seen.add(sid)
                ordered.append(sid)
    return ordered


PR_EDIT_RE = re.compile(r"gh\s+pr\s+edit\s+(\d+)\b")


def extract_pr_edits(records: list[dict]) -> list[int]:
    """Extract PR numbers from `gh pr edit <N> ...` commands. Used as a
    third matching path for PRs whose title was renamed post-creation."""
    out: list[int] = []
    for r in records:
        if r.get("record_type") != "tool_call":
            continue
        payload = r.get("payload") or {}
        if payload.get("name") != "Bash":
            continue
        cmd = ((payload.get("input") or {}).get("command")) or ""
        for m in PR_EDIT_RE.finditer(cmd):
            try:
                out.append(int(m.group(1)))
            except ValueError:
                pass
    return out


def session_meta_index() -> dict[str, dict]:
    """Index /api/sessions by session_id → metadata (for branch lookup)."""
    res = json.loads(urllib.request.urlopen(f"{URL}/api/sessions", timeout=30).read())
    sess_list = res.get("sessions") or res
    return {s["session_id"]: s for s in sess_list}


@dataclass
class SessionIndex:
    """Three lookup paths in order: title (primary), branch (fallback),
    PR-number-via-`gh pr edit` (last resort)."""
    by_title: dict[str, tuple[str, why_this_pr.WhyPr]] = field(default_factory=dict)
    by_branch: dict[str, tuple[str, why_this_pr.WhyPr]] = field(default_factory=dict)
    by_pr_num: dict[int, tuple[str, why_this_pr.WhyPr]] = field(default_factory=dict)

    def lookup(self, pr: dict) -> tuple[str, why_this_pr.WhyPr] | None:
        hit = self.by_title.get(title_key(pr["title"]))
        if hit:
            return hit
        hr = (pr.get("headRefName") or "").lower()
        if hr:
            hit = self.by_branch.get(hr)
            if hit:
                return hit
        return self.by_pr_num.get(pr["number"])


def build_session_index() -> SessionIndex:
    """Build all three indices in one pass over candidate sessions."""
    sessions = candidate_sessions()
    meta = session_meta_index()
    sys.stderr.write(f"# scanning {len(sessions)} candidate sessions\n")
    idx = SessionIndex()
    for sid in sessions:
        try:
            whys = why_this_pr.why_for_session(URL, sid, sess_meta=meta.get(sid))
        except SystemExit:
            continue

        # Also fetch records once more for `gh pr edit <N>` extraction.
        # (why_for_session already fetches them but doesn't expose; cheap to refetch.)
        try:
            recs = json.loads(urllib.request.urlopen(
                f"{URL}/api/sessions/{sid}/records", timeout=30
            ).read())
            if isinstance(recs, dict):
                recs = recs.get("records") or recs.get("items") or []
            edited_pr_nums = extract_pr_edits(recs)
        except urllib.error.URLError:
            edited_pr_nums = []

        for w in whys:
            if w.pr.title:
                idx.by_title[title_key(w.pr.title)] = (sid, w)
            if w.branch:
                idx.by_branch[w.branch.lower()] = (sid, w)

        # If this session has any PR events AND any `gh pr edit N` calls,
        # bind those PR numbers to the (likely first) PR event in the session.
        if whys and edited_pr_nums:
            anchor = whys[0]
            for n in edited_pr_nums:
                if n not in idx.by_pr_num:
                    idx.by_pr_num[n] = (sid, anchor)
    return idx


# -- Render -------------------------------------------------------

def fmt_pr_block(pr: dict, matched: tuple[str, why_this_pr.WhyPr] | None) -> str:
    num = pr["number"]
    title = pr["title"]
    author = (pr.get("author") or {}).get("login", "?")
    when = (pr.get("mergedAt") or pr.get("createdAt") or "")[:10]
    lines = pr.get("additions", 0)
    minus = pr.get("deletions", 0)
    state = pr["state"].lower()
    draft = " (draft)" if pr.get("isDraft") else ""
    head = f"### #{num} — {title}\n_{when}  ·  {author}  ·  +{lines} -{minus}  ·  {state}{draft}_\n"

    if not matched:
        return head + "\n_No matching OpenStory session found in store_  \n_(predates listener or title was edited post-creation)_\n"

    sid, w = matched
    parts = [head]
    # Note: "Span" is wall-clock (first → last event), not active work hours;
    # sessions that stay open across multiple days have large spans but
    # bursty actual activity.
    parts.append(f"**Session:** `{sid[:8]}…`  ·  **Span:** {w.duration_hours}h  ·  "
                 f"**Sentences:** {w.sentence_count}  ·  **Plans:** {len(w.plan_writes)}  ·  "
                 f"**Commits in session:** {len(w.commits)}\n")

    if w.plan_writes:
        parts.append("**Plan(s):** " + ", ".join(f"`{p.split('/')[-1]}`" for p in w.plan_writes))
        parts.append("")

    # Pick highlight prompts. Filter out harness injections, then choose
    # opener / two evenly-spaced middles / closer from what's left.
    real_turns = [t for t in w.prompt_trail
                  if t.get("prompt") and not is_noisy_prompt(t["prompt"])]
    if real_turns:
        n = len(real_turns)
        picks: list[dict] = [real_turns[0]]
        if n > 4:
            picks.append(real_turns[n // 3])
            picks.append(real_turns[2 * n // 3])
        picks.append(real_turns[-1])
        seen_turns: set[int] = set()
        parts.append("**Highlight prompts:**")
        for t in picks:
            tn = t.get("turn")
            if tn in seen_turns:
                continue
            seen_turns.add(tn)
            parts.append(f"- t{tn} _{t['verb']}_ — {t['prompt'][:180]}")
        parts.append("")

    if w.commits:
        parts.append("**First 5 commits in this session:**")
        for c in w.commits[:5]:
            parts.append(f"- {c[:100]}")
        parts.append("")
    return "\n".join(parts)


def phase_for_date(d: str) -> str:
    """Map merge date to a phase label. Tuned for OpenStory's repo history."""
    if not d:
        return "Unknown"
    if d <= "2026-03-31": return "Phase 1 — Bootstrap (March 22 → 31)"
    if d <= "2026-04-09": return "Phase 2a — First architecture wave (April 6 → 9)"
    if d <= "2026-04-19": return "Phase 2b — Pi-mono + audit + actor refactor (April 11 → 19)"
    if d <= "2026-04-29": return "Phase 3 — Deploy + maintenance (April 20 → 29)"
    if d <= "2026-05-04": return "Phase 4 — Engineer B joins, identity stack (April 30 → May 3)"
    return "Phase 5 — Open / in-flight"


def main() -> None:
    sys.stderr.write("# building session index from OpenStory…\n")
    index = build_session_index()
    sys.stderr.write(f"# indexed {len(index.by_title)} titles + {len(index.by_branch)} branches + {len(index.by_pr_num)} pr_nums\n\n")

    sys.stderr.write("# fetching gh PRs…\n")
    merged = gh_prs("merged")
    open_ = gh_prs("open")

    matched_count = 0
    for pr in merged + open_:
        if index.lookup(pr) is not None:
            matched_count += 1

    now = datetime.now(timezone.utc).strftime("%Y-%m-%d %H:%M UTC")
    out = sys.stdout.write
    out(f"# OpenStory PR retrospective  ·  generated {now}\n\n")
    out("Every PR in the OpenStory repo, cross-referenced with the session "
        "that produced it. The matching is title-based (normalized) — PR "
        "titles edited after creation may unmatch.\n\n")
    out(f"**Coverage:** {matched_count} / {len(merged) + len(open_)} PRs "
        f"have a matched OpenStory session. Unmatched PRs predate OpenStory's "
        "self-listening (early March 2026) or had post-creation title edits.\n\n")

    # Group merged PRs by phase
    by_phase: dict[str, list[dict]] = defaultdict(list)
    for pr in merged:
        by_phase[phase_for_date((pr.get("mergedAt") or "")[:10])].append(pr)

    out("---\n\n## Merged PRs\n\n")
    phase_order = sorted(by_phase.keys())
    for phase in phase_order:
        out(f"## {phase}\n\n")
        for pr in sorted(by_phase[phase], key=lambda p: p["number"]):
            matched = index.lookup(pr)
            out(fmt_pr_block(pr, matched) + "\n")

    # Open PRs at the end
    if open_:
        out("---\n\n## Open / in-flight\n\n")
        for pr in sorted(open_, key=lambda p: -p["number"]):
            matched = index.lookup(pr)
            out(fmt_pr_block(pr, matched) + "\n")

    out("\n---\n\n*Generated by `scripts/pr_retrospective.py`. "
        "Re-run anytime to refresh against current data and gh state.*\n")


if __name__ == "__main__":
    main()
