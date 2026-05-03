# scripts/team_day

A small composable pipeline that answers **"what did our team work on today?"**
deterministically, so the narrative the agent writes on top can never
hallucinate counts, prompts, or attributions.

## Why a pipeline (not one big script)

Every step is a pure function: JSON in, JSON out. You can run any one stage in
isolation, inspect its output, and feed it into the next. When the report is
wrong, the pipeline tells you which stage produced the wrong number.

```
gather   → list_sessions + synopsis              → canonical session records
classify → tag author / role / kind / repo       → records + tags
enrich   → files, opening prompt, errors, MCP    → records + tags + enriched
measure  → throughput, hot files, health         → bundle + metrics
validate → check invariants, flag suspect rows   → bundle + validation
run      → orchestrator: runs all + writes facts → captures/team_day/{date}/
```

## Quick start

```bash
# Today, in the team's local TZ (default America/New_York from roster.json)
python3 scripts/team_day/run.py

# A specific date
python3 scripts/team_day/run.py --date 2026-05-02

# Include sub-agent sessions in deep enrichment
python3 scripts/team_day/run.py --include-subagents

# Mode: include sessions that started before today but were active today
python3 scripts/team_day/run.py --mode active

# Use a remote OpenStory
python3 scripts/team_day/run.py --url https://openstory.example.com
```

Output lands in `captures/team_day/{date}/`. The file you usually want is
`facts.md` — a markdown fact sheet for humans (and for the `team-day` skill
to narrate from).

## Step-by-step (compose your own pipeline)

```bash
python3 scripts/team_day/gather.py --date 2026-05-02 \
  | python3 scripts/team_day/classify.py \
  | python3 scripts/team_day/enrich.py \
  | python3 scripts/team_day/measure.py \
  | python3 scripts/team_day/validate.py \
  > /tmp/bundle.json
```

Each step accepts `--in PATH` (default stdin) and `--out PATH` (default
stdout). Any step's output is a valid input to the next.

## Roster

`roster.json` declares team members:

```json
{
  "members": [
    {"name": "Max",   "users": ["maxglassie"], "path_prefixes": ["/Users/maxglassie/", "-Users-maxglassie-"]},
    {"name": "Katie", "users": ["kloughra"],   "path_prefixes": ["/Users/kloughra/",  "-Users-kloughra-"]}
  ],
  "team_repos": ["OpenStory", "openstory-ui-prototype", "dora-metrics", ...],
  "default_tz": "America/New_York"
}
```

Identity is resolved in this order:
1. `user` field on the session record (the OS username).
2. `project_id` path prefix.
3. `files_touched` paths (fallback during enrichment).

Path is the only reliable signal. `project_name` lies (same name, different humans).

## Methodology summary

- **One source per fact.** Synopsis is canonical for counts/timing/label.
  Activity is canonical for files/tools. Transcript is canonical for the
  literal opening prompt. The pipeline never mixes sources for the same field.
- **No silent normalization.** When the transcript endpoint returns empty,
  `opening_prompt` is `null` and the fact sheet says "*(label, no transcript)*"
  rather than substituting the synopsis label.
- **Window in local TZ first.** A "day" is a local calendar day; the script
  converts it to UTC bounds for filtering. Times are displayed in local TZ.
- **No filtering by author.** Off-team sessions (e.g. Max's resume) are tagged
  `is_team_repo: false` but kept in the bundle. The composer chooses what to
  show.
- **Validation always runs.** Every record gets checked for window consistency,
  duration sanity, author resolution, prompt/label divergence, ship-with-no-
  events, and file/project mismatches. Flagged sessions stay in the bundle
  with a warning so the composer can quote them with care or drop them.

## When something looks wrong

- **Wrong author** → check `roster.json`. Add the user's OS username and any
  new path prefix, then re-run.
- **Sessions missing** → try `--mode active` to catch resumed-old sessions.
- **Repo name truncated** → the slug parser cuts after the last `workspace`
  or `projects` segment. Add new markers in `classify.project_repo_name`.
- **Validation noise** → inspect `05_validate.json`. The warnings are
  designed to be loud rather than silent; some are expected (e.g. clock skew
  in early ingest).

## What this is *not*

- **Not a narrator.** The pipeline produces facts. The story belongs in the
  `team-day` skill (`.claude/skills/team-day/SKILL.md`).
- **Not a metric system.** The "DORA-flavored" numbers in `measure.py` are
  daily snapshots, not trends. For trends, see `scripts/cost_report.py` and
  the analysis scripts under `scripts/analyze_*.py`.
- **Not a substitute for `sessionstory.py`.** That script is for one
  session; this pipeline is for one day across the team.
