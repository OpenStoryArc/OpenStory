---
name: team-day
description: Tell the story of a team's work on a given day (or a range of days) by running the deterministic team_day pipeline, reading the resulting facts sheet(s), and composing a structured narrative on top. Use when the user asks "what did our team do today / yesterday / on date X / the past few days", "summarize the team's day", "what did Engineer A and Engineer B work on", or wants a per-author day report grounded in OpenStory data. Dogfoods OpenStory — never guess from the conversation, always run the pipeline first.
---

# team-day

A two-phase skill: a script collects the facts, you write the story.

The pipeline is deterministic so the narrative cannot hallucinate. If a number, prompt, or attribution is not in the facts sheet, do not put it in the report.

## Phase 1 — Run the pipeline

```bash
# Default: today, in the team's local TZ (from roster.json)
python3 scripts/team_day/run.py

# A specific date
python3 scripts/team_day/run.py --date 2026-05-02

# Override TZ
python3 scripts/team_day/run.py --date 2026-05-02 --tz America/New_York

# Include sub-agent sessions in deep enrichment (slower, more API calls)
python3 scripts/team_day/run.py --include-subagents

# Include sessions that started before today but were active (resumed) today
python3 scripts/team_day/run.py --mode active
```

For multi-day requests ("the past few days", "this week"), run the pipeline once per day in the window. Each day produces its own `captures/team_day/{date}/facts.md`. Compose one report on top of the union of those sheets — do not invent a single-pipeline mode that doesn't exist.

Requires the OpenStory server on `http://localhost:3002` (override with `--url`).

The pipeline writes artifacts to `captures/team_day/{date}/`:

- `01_gather.json` → raw sessions in window (canonical synopsis)
- `02_classify.json` → + author / role / kind / repo tags
- `03_enrich.json` → + files_touched, opening_prompt, errors, MCP-OS call counts
- `04_measure.json` → + throughput, hot files, health, tokens (DORA-flavored)
- `05_validate.json` → + warnings on suspect records
- `bundle.json` → final artifact (= 05)
- **`facts.md`** → human-readable fact sheet — **the only thing you read to write the report**

## Phase 2 — Read the facts sheet(s)

`captures/team_day/{date}/facts.md` contains everything you need:

- **Sessions by author** — one table per person, columns include verbatim opening prompts (or labels with explicit `*(label, no transcript)*` markers)
- **Throughput** — commits + merges in window, per author; tokens vs trailing avg if available
- **Hot files** — files touched in ≥2 sessions, with cross-author flag
- **Health** — ghost / error / compaction / recall counts
- **Validation warnings** — sessions whose data is inconsistent. Treat flagged sessions with care: do not quote a prompt or count from a flagged session without acknowledging the warning.

For multi-day reports: read every day's sheet, sum the totals yourself, and pattern-match across them for repeated repos / prompt prefixes / file overlap. Themes that show up on ≥2 days are threads worth naming.

## Phase 3 — Compose the report

The report has a **fixed section order**. Analysis lives at the top; the timeline is the receipts at the bottom. Aim for one scrollable page — every section earns its place or gets cut.

```
# Team — {date or window} ({tz})

> Eyebrow line: dateline + "grounded in OpenStory session data, not vibes."

## Stat strip (4 cards)
Sessions · Commits · Merges · One signature number for the window
(e.g. peak MCP-OS calls, peak files touched, biggest single session).
Each with a sub-line breaking it down by author.

## Executive summary
One paragraph (4–6 sentences) as a pull-quote. Lead with totals + the
center of gravity (the one objective everything else orbits). Name what
each person carried. Close with the shape of the most recent day.
This is the only place narrative voice lives at full strength.

## Key objectives — N threads
Numbered list of 3–6 threads. Each thread:
- Title (short, declarative)
- 1–2 sentences citing the prompt(s) and counts that prove it's a thread
A "thread" requires evidence on ≥2 sessions OR ≥1 session with substantial
scale (≥10 files OR ≥10 MCP-OS OR an explicit "FEATURE:" / "Spike:" prefix).

## Per person
One card per author. Header: name + counts (sessions / commits / merges).
Body: one sentence — asymmetric is fine. If Engineer A worked on one thing their
line is one clause; if Engineer B braided three threads theirs is one sentence
with semicolons. Quote verbatim where transcripts exist, mark (label)
otherwise. Don't pad to match length.

## Hottest files
Top 5–8 files by session count, each tagged with the author who touched
them. Bar visual is optional; the count is the point. Skip files that
only appear in one session unless they're a flagship artifact.

## Timeline
Substantial primary sessions only (kind != chat AND events ≥ 50 OR
files ≥ 1 OR mcp_os ≥ 1). Group by day with a day-header row showing
the day's commits/merges. Inside each day, one row per session with:

  - colored author dot
  - author pill, repo, time
  - verbatim opening prompt (≤100 chars), italic, with (label) marker if needed
  - Δ line: file count, errors, MCP-OS, validation warnings — smallest
    signal that says what shipped

For multi-day windows, the timeline is grouped by day; for a single day,
it's one flat list under the date.

## Notes & limits
Bulleted, ≤4 items. Top of the watch list / validation warnings only.
Always note transcript-vs-label split if it skews per-author readability.
Always note any cross-repo file paths (project-mismatch warnings).
Skip the section only if there is genuinely nothing.
```

That ordering is the contract: **stat strip → executive summary → objectives → per person → hot files → timeline → notes.** Don't reshuffle. The user asked for analysis first and receipts second; honor that.

### Optional: HTML rendering

If the user asks for an HTML version, write to `/tmp/team_report_{window}.html` with the same section order. Use serif headlines (Charter / Iowan Old Style / Georgia stack), system sans body, light-dark CSS variables, two accent colors (one per author — keep them consistent across runs), tag pills for repos, dot markers per author on timeline rows, and a stat strip at the top. Open with `open` afterward. Do not pull external fonts or scripts — the report should render offline.

## Voice rules

- **Quote, don't characterize.** Verbatim openings in italics; never paraphrase intent. Mark `(label)` if transcript was unavailable.
- **State problems as problems.** "X needed Y" / "X was broken" — not "edited X."
- **Credentials before claims.** Counts and session IDs before meaning. The executive summary leads with totals, not adjectives.
- **Per-person sections are mandatory** even if uneven — asymmetry is information. Don't pad the shorter person's line to match.
- **Acknowledge limits inline.** ⚠ in the Δ column for validation-flagged rows; one bullet in Notes for transcript gaps. Don't write a paragraph about it.
- **Asymmetric is fine.** Different days will have different shapes. A "planning / spike day" with low commits but high MCP-OS is a real shape — name it, don't apologize for it.

## Don't

- Rank or compare team members.
- Infer intent from outcomes (a no-files session may be recall, not waste — read the prompt).
- Manufacture drama on routine days. "Routine maintenance" is a valid shape.
- Invent connections between sessions that aren't in the facts sheet.
- Report on subagents as standalone work — they fold into their parent session.
- Quote a prompt or count from a session listed in `validation.warnings` without flagging it.
- Put the timeline above the executive summary. Analysis first.

## Failure modes the pipeline guards against

- Same project name, different human → wrong attribution. **Author is resolved by `user` field then `project_id` path; never by `project_name`.**
- `event_count` from the list endpoint is a snapshot and ages. **Synopsis is canonical.**
- `label` on a session is a derived short string and may not match the actual opening prompt. **`opening_prompt` from the transcript is canonical when present.**
- UTC sessions cross local-day boundaries. **The window is always resolved in the team's TZ first, then converted to UTC bounds.**

## Adjacent skills

- `sessionstory` — single-session deep dive (different scope; use for "what happened in session X")
- `check-docs` — validates docs against codebase (different purpose)

## When NOT to use this skill

- Single session investigation → use `sessionstory`
- Live state ("what is happening right now") → tail the WebSocket or query `/api/sessions` directly
- Cross-month trends → not what this skill is for; sketch a query or use `scripts/cost_report.py`
