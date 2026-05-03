---
name: team-day
description: Tell the story of a team's work on a given day by running the deterministic team_day pipeline, reading the resulting facts sheet, and composing a short narrative on top. Use when the user asks "what did our team do today / yesterday / on date X", "summarize the team's day", "what did Max and Katie work on", or wants a per-author day report grounded in OpenStory data. Dogfoods OpenStory — never guess from the conversation, always run the pipeline first.
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

Requires the OpenStory server on `http://localhost:3002` (override with `--url`).

The pipeline writes artifacts to `captures/team_day/{date}/`:

- `01_gather.json` → raw sessions in window (canonical synopsis)
- `02_classify.json` → + author / role / kind / repo tags
- `03_enrich.json` → + files_touched, opening_prompt, errors, MCP-OS call counts
- `04_measure.json` → + throughput, hot files, health, tokens (DORA-flavored)
- `05_validate.json` → + warnings on suspect records
- `bundle.json` → final artifact (= 05)
- **`facts.md`** → human-readable fact sheet — **the only thing you read to write the report**

## Phase 2 — Read the facts sheet

`captures/team_day/{date}/facts.md` contains everything you need:

- **Sessions by author** — one table per person, columns include verbatim opening prompts (or labels with explicit `*(label, no transcript)*` markers)
- **Throughput** — commits + merges in window, per author; tokens vs trailing avg if available
- **Hot files** — files touched in ≥2 sessions, with cross-author flag
- **Health** — ghost / error / compaction / recall counts
- **Validation warnings** — sessions whose data is inconsistent. Treat flagged sessions with care: do not quote a prompt or count from a flagged session without acknowledging the warning.

## Phase 3 — Compose the report

The report is a **timeline first**. Optimize for one screen. Aim for ~30 lines
total. Every line earns its place or gets cut.

```
# Team Day — {date} ({tz})

## Timeline
One unified chronological strip across the team. Substantial primary sessions
only (kind != chat AND events ≥ 50 OR files ≥ 1 OR mcp_os ≥ 1). One row each.

| EDT  | Who   | Repo             | Headline (verbatim opening, ≤80 chars) | Δ                |
|------|-------|------------------|---------------------------------------|------------------|
| 11:23| Katie | telegram-int-local | "Can you check out this repo…"      | 2 files          |
| 12:17| Max   | OpenStory        | "Did I do work a while ago…"          | 6 MCP, 0 files   |
| 13:47| Katie | OpenStory        | "Quick question on my current setup…" | 12 files, 3 errs |
| 15:42| Max   | OpenStory        | "YC Demo: two things…"                | 18 files, 33 MCP |

Δ column: smallest signal that says what shipped — file count, MCP calls,
errors, ghost flag, validation warning.

## One-liners
One sentence per person. The day in one sentence — that's the test.

- **Katie**: <one sentence>
- **Max**:   <one sentence>

## Numbers
- Commits N · Merges M · Hot file: `path` (N sessions)
- Health: G ghosts · E errors · C compactions · R recall

## Notes (optional, ≤3 lines)
Only the top items from the watch list / validation warnings.
Skip the section if there's nothing.
```

That's the entire report. No "shape of the day" essay, no per-person
paragraphs, no reframe. The timeline + the one-liners ARE the shape.

## Voice rules

- **Quote, don't characterize.** Verbatim openings in the headline column.
  Mark `(label)` if transcript was unavailable.
- **State problems as problems** in the one-liner if you state anything.
- **Asymmetric one-liners are fine** — if Max worked on one thing, his line is
  one clause; if Katie braided three threads, hers is one sentence with a
  semicolon. Don't pad to match.
- **Acknowledge limits.** Use ⚠ inline in the Δ column for validation-flagged
  rows; don't write a paragraph about it.

## Voice rules

- **Quote, don't characterize.** Verbatim prompts in italics; never paraphrase intent.
- **State problems as problems.** "X needed Y" / "X was broken" — not "edited X."
- **Credentials before claims.** Session ID + scale before meaning.
- **Per-person sections are mandatory** even if uneven — asymmetry is information.
- **Acknowledge limits.** If a session was sub-agent-skipped, transcript-empty, or validation-flagged, say so inline rather than substituting fiction.

## Don't

- Rank or compare team members.
- Infer intent from outcomes (a no-files session may be recall, not waste — read the prompt).
- Manufacture drama on routine days. "Routine maintenance" is a valid shape.
- Invent connections between sessions that aren't in the facts sheet.
- Report on subagents as standalone work — they fold into their parent session.
- Quote a prompt or count from a session listed in `validation.warnings` without flagging it.

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
