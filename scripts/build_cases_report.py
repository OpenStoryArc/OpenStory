"""Compile a markdown appendix of live cases for each introspection script.

The cases are pinned (session ids and file paths). Each case has a one-line
hypothesis about what it should reveal. The script runs the appropriate
introspection module against each case, captures a compact rendering, and
emits a single markdown report to stdout.

This is the appendix for `docs/research/introspection-scripts/README.md`.
The README has the methodology; this script produces the data illustration.

Run:
    python3 scripts/build_cases_report.py > docs/research/introspection-scripts/cases.md

Re-run any time data changes. The output is deterministic given a stable
detector and frozen past sessions.
"""

from __future__ import annotations

import sys
from datetime import datetime, timezone

import why_this_pr
import prompt_trail
import state_provenance
import skills_used
import openstory_tier_usage as tier_usage


URL = "http://localhost:3002"


# -- Case definitions ----------------------------------------------
# Each case names the question, pins an id/path, and states what the case
# is meant to *demonstrate* — so a regression (or a real shift in the data)
# is visible against intent.

PR_CASES = [
    ("Long Socratic exploration → ship",
     "0b46ac55-bf3d-462e-b2b3-7b0fcc8a6bd8",
     "26 hours, 23 turns, 1 plan, 12 commits — the PersonId+Fleet PR. "
     "Demonstrates a research-driven feature: prompts open with "
     "philosophy ('is an agent a person?'), pivot to plan at t19, "
     "ship at t20–t21."),
    ("Short directive hotfix",
     "46c94826-9ff5-48dc-9663-aed0537c72ae",
     "Multi-PR security/branch-protection housekeeping. Demonstrates "
     "the directive shape: writes dominate, explain ≈ 0%, ships fast."),
    ("Stack-landing chore",
     "971839de-9470-4111-ad04-c7a8ee12e759",
     "Lands a stacked PR. Demonstrates an outcome-PR session that's "
     "primarily check-and-recovery shaped, not Socratic."),
]

TRAIL_CASES = [
    ("Socratic", "632bdd40-3dc9-48d5-9560-57895c88ed44",
     "Pure conversation/exploration — `/office-hours` & `/plan-ceo-review` invoked."),
    ("Directive", "46c94826-9ff5-48dc-9663-aed0537c72ae",
     "Hotfix: ships through writes, no exploration."),
    ("Recovery", "971839de-9470-4111-ad04-c7a8ee12e759",
     "Debug-shaped: high check, with explain interleaved."),
    ("Mixed", "0b46ac55-bf3d-462e-b2b3-7b0fcc8a6bd8",
     "Sits at the socratic/mixed boundary — 43% explain (threshold 45%). "
     "Reflects a real shift mid-session from explore to ship."),
    ("Plumbing", "f38a31f3-c244-492d-b32c-f077066c0343",
     "92% write, 0% explain — silent refactor."),
]

PROVENANCE_CASES = [
    ("~/projects/OpenStory/rs/server/src/principal_resolver.rs",
     "New file with a single-session origin: born at one prompt."),
    ("~/projects/OpenStory/ui/src/components/Sidebar.tsx",
     "Hot file written across multiple sessions: provenance is a fan."),
    ("~/projects/OpenStory/docs/BACKLOG.md",
     "Cross-engineer file: written by both engineers if the "
     "data has both."),
]

SKILLS_CASES = [
    ("/team-day invocation (Max)", "971839de-9470-4111-ad04-c7a8ee12e759",
     "Engineer A running the shared team-day skill — should detect 1 invoke, "
     "knowledge consulted is the team-day captures."),
    ("Multi-skill conversation", "632bdd40-3dc9-48d5-9560-57895c88ed44",
     "/office-hours + /plan-ceo-review on the same session."),
    ("/sessionstory invocation", "5d043ef5-c27e-41ad-86f2-d7fa97cb3759",
     "Self-introspection skill — the agent looking at its own data."),
]

TIER_CASES = [
    ("All five tiers in one session",
     "5d043ef5-c27e-41ad-86f2-d7fa97cb3759",
     "rest + script + skill + mcp — the maximally-instrumented case."),
    ("REST-heavy (Engineer B)", "4198566e-64a0-49e9-95aa-0f213f0c4cd2",
     "Engineer B's introspection profile is REST-dominant — 68 curls."),
    ("Rawdog anti-pattern subagent",
     "agent-ab2a6347c5798e462",
     "31 raw greps over JSONL transcript files — exactly the "
     "anti-pattern CLAUDE.md flags. Worth knowing this still happens."),
    ("MCP-only (this conversation)",
     "5cb670d8-ce17-41b4-a6d1-55a6b264fdd7",
     "Live session: MCP-dominant, with REST falls-back. Self-reflective."),
]


# -- Renderers ------------------------------------------------------

def render_why_pr(case: tuple) -> str:
    name, sid, hypothesis = case
    try:
        whys = why_this_pr.why_for_session(URL, sid)
    except SystemExit:
        return f"### {name} — `{sid}`\n_skipped (api error)_\n"
    if not whys:
        return f"### {name} — `{sid}`\n_no PR events found_\n"
    out = [f"### {name}",
           f"`{sid}` — _{hypothesis}_\n"]
    for w in whys[:1]:  # show first PR if multiple
        out.append(f"**Title:** {w.pr.title or '(no title)'}")
        out.append(f"**User · Project · Branch:** {w.user or '?'} · {w.project_name or '?'} · {w.branch or '?'}")
        out.append(f"**Duration:** {w.duration_hours}h  ·  **Sentences:** {w.sentence_count}  ·  **Plans:** {len(w.plan_writes)}  ·  **Commits:** {len(w.commits)}\n")
        # show 5 prompts spaced across the trail
        if w.prompt_trail:
            picks = [w.prompt_trail[0]]
            n = len(w.prompt_trail)
            if n > 4:
                for i in (n // 4, n // 2, 3 * n // 4):
                    picks.append(w.prompt_trail[i])
            picks.append(w.prompt_trail[-1])
            seen = set()
            out.append("**Prompt trail (excerpt):**")
            for t in picks:
                key = t.get("turn")
                if key in seen: continue
                seen.add(key)
                pr = (t.get("prompt") or "")[:140]
                out.append(f"- t{t['turn']} _{t['verb']}_ — {pr}")
            out.append("")
        if w.commits:
            out.append("**Commits (first 4):**")
            for c in w.commits[:4]:
                out.append(f"- {c}")
            out.append("")
    return "\n".join(out)


def render_trail(case: tuple) -> str:
    name, sid, hypothesis = case
    try:
        t = prompt_trail.for_session(URL, sid)
    except SystemExit:
        return f"### {name} — `{sid}`\n_skipped_\n"
    bars = []
    for verb, n in list(t.verb_mix.items())[:6]:
        bars.append(f"- {verb} × {n}")
    return (
        f"### {name} — `{sid}`\n"
        f"_{hypothesis}_\n\n"
        f"- archetype: **{t.archetype}**\n"
        f"- sentences: {t.sentence_count}\n"
        f"- shares: write {t.write_share*100:.0f}% · explain {t.explain_share*100:.0f}% · check {t.check_share*100:.0f}%\n"
        f"- top verbs: " + ", ".join(f"{v} × {n}" for v, n in list(t.verb_mix.items())[:5]) + "\n"
    )


def render_provenance(case: tuple) -> str:
    path, hypothesis = case
    try:
        rows = state_provenance.provenance_for(URL, path)
    except SystemExit:
        return f"### `{path}`\n_skipped_\n"
    short = path.replace("~/projects/OpenStory/", "")
    if not rows:
        return f"### `{short}`\n_{hypothesis}_\n\n_no writes found in corpus_\n"
    out = [f"### `{short}`",
           f"_{hypothesis}_\n",
           f"- total writes across corpus: **{sum(r.write_count for r in rows)}** in **{len(rows)}** session(s)"]
    for r in rows[:5]:
        out.append(f"- session `{r.session_id[:8]}…` ({r.user or '?'} · {r.project or '?'}): "
                   f"{r.write_count} write(s), turns {r.turns}")
        for p in r.prompts[:2]:
            out.append(f"    - prompt: _{p[:120]}_")
    out.append("")
    return "\n".join(out)


def render_skills(case: tuple) -> str:
    name, sid, hypothesis = case
    try:
        s = skills_used.for_session(URL, sid)
    except SystemExit:
        return f"### {name} — `{sid}`\n_skipped_\n"
    inv_str = ", ".join(f"/{n}" for n in sorted(set(s.invoked))) or "_(none)_"
    rs_str = ", ".join(sorted(set(s.read_skills))) or "_(none)_"
    knowledge = list(dict.fromkeys(s.knowledge))[:4]
    out = [f"### {name} — `{sid}`",
           f"_{hypothesis}_\n",
           f"- user · project: **{s.user or '?'}** · **{s.project or '?'}**",
           f"- invoked: {inv_str}",
           f"- skill files read (without invoke): {rs_str}",
           f"- knowledge consulted: {len(s.knowledge)} read(s)"]
    for k in knowledge:
        short = k.replace("/Users/engineer-a/", "~/").replace("/Users/engineer-b/", "~engineer-b/")
        out.append(f"    - `{short}`")
    out.append("")
    return "\n".join(out)


def render_tier(case: tuple) -> str:
    name, sid, hypothesis = case
    try:
        sessions = tier_usage.all_sessions(URL)
        meta = next((s for s in sessions if s.get("session_id") == sid), None)
        u = tier_usage.for_session(URL, sid, meta)
    except SystemExit:
        return f"### {name} — `{sid}`\n_skipped_\n"
    cells = ", ".join(f"{t}={getattr(u,t)}" for t in tier_usage.TIERS if getattr(u, t))
    return (f"### {name} — `{sid}`\n"
            f"_{hypothesis}_\n\n"
            f"- user · project: **{u.user or '?'}** · **{u.project or '?'}**\n"
            f"- tiers used: {cells or '_(none)_'}\n")


# -- Top-level ------------------------------------------------------

def safe_render(render_fn, case) -> str:
    """Run a renderer, catching ALL exceptions so one bad case doesn't abort
    the whole report. Transient API disconnects are common on the local
    server during long scans — that's a server health issue, not a script
    correctness issue. The report flags the failure and moves on."""
    try:
        return render_fn(case)
    except SystemExit as e:
        return f"### {case[0] if isinstance(case, tuple) else case}\n_skipped (api fetch failed, exit {e.code})_\n"
    except Exception as e:
        label = case[0] if isinstance(case, tuple) else str(case)
        return f"### {label}\n_skipped: {type(e).__name__} — {str(e)[:200]}_\n"


def main() -> None:
    now = datetime.now(timezone.utc).strftime("%Y-%m-%d %H:%M UTC")
    sys.stdout.write(f"# Introspection cases  ·  generated {now}\n\n")
    sys.stdout.write(
        "Auto-generated by `scripts/build_cases_report.py`. Each case is a "
        "pinned (session_id or path) + a hypothesis about what the script "
        "should reveal. Re-run after data changes to see drift.\n\n"
        "See [`README.md`](./README.md) for the methodology framing.\n\n"
    )

    # Why this PR
    sys.stdout.write("## `why_this_pr.py` — outcome (PR) → owning intentions\n\n")
    for c in PR_CASES:
        sys.stdout.write(safe_render(render_why_pr, c) + "\n")

    # Prompt trail
    sys.stdout.write("## `prompt_trail.py` — shape of intentions over time\n\n")
    sys.stdout.write(
        "Five archetypes, one example each. Across the 139 sessions "
        "currently classifiable: 48.9% mixed, 20.9% directive, 11.5% "
        "plumbing, 11.5% socratic, 7.2% recovery.\n\n"
    )
    for c in TRAIL_CASES:
        sys.stdout.write(safe_render(render_trail, c) + "\n")

    # Provenance
    sys.stdout.write("## `state_provenance.py` — outcome (file) → owning intention\n\n")
    for c in PROVENANCE_CASES:
        sys.stdout.write(safe_render(render_provenance, c) + "\n")

    # Skills
    sys.stdout.write("## `skills_used.py` — auxiliary tools attached to intentions\n\n")
    for c in SKILLS_CASES:
        sys.stdout.write(safe_render(render_skills, c) + "\n")

    # Tier
    sys.stdout.write("## `openstory_tier_usage.py` — meta: how the agent introspected\n\n")
    sys.stdout.write(
        "Five tiers: rawdog (anti-pattern grep over JSONL), rest "
        "(`/api/*`), script (`scripts/sessionstory.py` etc.), skill "
        "(`/sessionstory`, `/team-day`, ...), mcp (`mcp__openstory__*`).\n\n"
    )
    for c in TIER_CASES:
        sys.stdout.write(safe_render(render_tier, c) + "\n")

    sys.stdout.write("---\n\n")
    sys.stdout.write(
        "## Cross-cutting findings\n\n"
        "1. **The classifier is honest about boundaries.** The PersonId "
        "session lands as `mixed` not `socratic` — and that's correct: "
        "it pivoted from exploration to ship at t19. Forcing it into "
        "`socratic` would be overfitting.\n\n"
        "2. **REST + MCP carry the load; rawdog still happens.** "
        "Despite CLAUDE.md flagging it as anti-pattern, 17 sessions used "
        "raw JSONL grep — usually subagent debug sessions where the "
        "shortest path was fastest. Worth surfacing as a nudge in the "
        "harness.\n\n"
        "3. **Skill discovery is bursty.** /team-day has 6 invocations "
        "split evenly between Engineer A and Engineer B; /sessionstory has 3 "
        "(all Engineer A); /office-hours has 2. Most coding sessions invoke 0 "
        "skills.\n\n"
        "4. **Provenance is usually a path, sometimes a fan.** "
        "Source files under `rs/server/` written in greenfield work "
        "trace back to a single session/turn/prompt. UI files like "
        "`Sidebar.tsx` are written by multiple engineers across multiple "
        "sessions — a different shape of question.\n\n"
        "5. **Subagent record attribution is unresolved.** "
        "`test_introspection_scenarios.py --invariants` flagged 2 "
        "subagent sessions where records carry a non-matching "
        "`session_id`. See `docs/BACKLOG.md`.\n"
    )


if __name__ == "__main__":
    main()
