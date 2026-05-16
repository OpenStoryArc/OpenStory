# Session stories — methodology

How to write the story of a single OpenStory session, **grounded in the
deterministic data layers** rather than from vibes or transcript skim.

This builds on the existing `sessionstory` skill (`~/projects/OpenStory/.claude/skills/sessionstory/SKILL.md`),
which gathers record-level facts (record types, tool histograms, eval-apply
patterns, `turn.sentence` detector output, prompt timeline). This methodology
*adds* the three semantic shape layers (`prompt_shapes.db`, `path_shapes.db`,
`bash_shapes.db`) plus hourly rhythm and PR/git activity — enough to narrate
not just *what happened* but *what shape the session took*.

Read the existing `sessionstory` skill first. Use it for the baseline.
This methodology is the upper layer.

---

## Prerequisites

1. **OpenStory server running** at `http://localhost:3002` (the shape-layer
   builds and the existing `sessionstory.py` both depend on it).
2. **All six shape layers built** for the time range that contains the
   session of interest:

```bash
# the three agent-interaction layers (intent / attention / action)
uv run scripts/build_prompt_shapes.py    --limit 200
uv run scripts/build_path_shapes.py      --limit 200
uv run scripts/build_bash_shapes.py      --limit 200

# the three content layers (code deltas + identifier vocab + prose vocab)
uv run scripts/build_change_shapes.py    --limit 200
uv run scripts/build_code_vocab.py       --limit 200
uv run scripts/build_docs_vocab.py       --limit 200

# the working-memory layer (file_snapshot manifests)
uv run scripts/build_snapshot_shapes.py  --limit 200
```

(Increase `--limit` or omit it to cover more sessions. Builds are idempotent
on `event_id`, so re-running them is cheap. The first three are required for
`session_story_facts.py`; the latter three give code-level grounding and
are queried independently with their own `query_*_shapes.py` scripts.)

3. **Pick a sizable session.** Sessions with < ~30 events rarely have enough
   signal to narrate; sessions with thousands of events are ideal:

```bash
curl -s "http://localhost:3002/api/sessions" | python3 -c "
import json, sys
data = json.load(sys.stdin)
sessions = data if isinstance(data, list) else data.get('sessions', [])
sessions.sort(key=lambda s: -s.get('event_count', 0))
for s in sessions[:10]:
    print(s['event_count'], s.get('start_time','')[:10], s['session_id'], (s.get('label') or '')[:60])
"
```

---

## Two-phase pipeline

### Phase 1 — Gather the deterministic substrate

Run **both** fact-gatherers. They produce complementary substrates:

```bash
# baseline: records / tools / patterns / turn.sentence / prompt timeline
python3 scripts/sessionstory.py SESSION_ID

# shape layers + rhythm + PR activity (this methodology adds these)
uv run scripts/session_story_facts.py SESSION_ID

# code-level grounding (optional but powerful — adds magnitude + identifier
# vocabulary + working-set timeline for richer narrative)
uv run scripts/query_change_shapes.py     --session SESSION_ID
uv run scripts/query_code_vocab.py        --session SESSION_ID
uv run scripts/query_snapshot_shapes.py   --session SESSION_ID
```

The shape-layer fact sheet has five blocks:

1. **Prompt skeleton** — verbs / subjects / objects / chunks / adjectives / adverbs
   histograms across all 76 prompts. This is *what the user was pointing at*.
2. **Path footprint** — top segments / extensions / tools / naming-vocab and
   the most-touched paths. This is *where attention went in the codebase*.
3. **Bash dialect** — programs / per-program subcommands / pipeline%
   / redirect%. This is *what was actually run*.
4. **Hourly rhythm** — prompts/paths/bash counts per `MM-DD HH` bucket. This
   reveals working blocks, break gaps, and sleep boundaries without anyone
   having to label them.
5. **PR & git activity** — every `gh pr` / `git push` / `git merge` command
   with its timestamp. This is the artifact trail of what shipped.
6. **Prompt sequence** — every prompt in time order with `HH:MM` stamps. This
   is the *script* of the session.

### Phase 2 — Narrate

The fact sheet does not write the story. You do. Apply these principles:

#### Principles

1. **Open with the aim and close with the ending.** The first and last
   prompts bookend the arc. Quote them verbatim. Almost every meaningful
   session has an arc that closes near where it began (often on a
   verification or test question), or that explicitly fails to.

2. **Find the natural phase boundaries from the hourly rhythm.**
   Hours with zero prompts/paths/bash are gaps. Gaps ≥ 30 min are usually
   topic shifts; gaps ≥ 4 hours are usually breaks; gaps ≥ 8 hours are
   usually sleep. The rhythm tells you where to put section headings before
   you read any prompt content.

3. **Triangulate across layers.** Don't narrate from prompts alone. If you
   claim "this was a debugging session," check that the bash shows
   `Edit:Read` skewed toward Read, and the paths show clusters not novelty.
   If the prompt verbs say `merge`, the bash should show `gh pr` and
   `git merge` activity. **When two layers disagree, that's the most
   interesting finding** — flag it.

4. **Quote prompts verbatim where they matter.** The prompt excerpt
   substring (first 200 chars) is what you have. Use direct quotes for
   pivots, openings, endings, and surprises. Don't paraphrase — the
   user's voice is part of the story.

5. **Use the verb histogram to detect mode shifts.** When `merge(5)`
   appears on day 2 but `merge(0)` on day 1, that's a mode shift. When
   `let(19)` and `like(9)` dominate, the session was conversational. When
   `create / build / write` dominate, it was directive. Name the mode.

6. **Use the path footprint to detect topic.** The naming-vocab histogram
   names the *thing being worked on*. The `most_touched` paths name the
   *specific surfaces*. Together they tell you what the session was about
   without reading any code.

7. **Use the PR/git activity as the artifact trail.** Every `gh pr create`
   is a moment of commitment; every `gh pr view` is a moment of inspection;
   every `git merge` is a moment of decision. Stamps make the timeline
   concrete.

8. **Don't claim what the data doesn't show.** If you don't have evidence
   for an inference, leave it out. The strength of these stories is their
   groundedness. If you start narrating motivations or feelings without
   evidence, you've left the methodology.

9. **End with "what the layers caught that prose wouldn't."** This is the
   *value-add* section — the things only legible because the deterministic
   substrate was there. If you can't write this section, the story is just
   prose; the methodology didn't earn its keep.

#### Output format

A markdown file at `docs/research/session-stories/{SHORT_SID}-{slug}.md`,
where `SHORT_SID` is the first 8 chars of the UUID and `slug` is a
hyphenated phrase from the first prompt or the dominant theme.

Suggested structure:

```markdown
# Session `<SID>` — <one-line title from the aim + the ending>

> Author: <user from path footprint, e.g. /Users/X/...>
> Window: <first_ts → last_ts (~N hours wall-clock)>
> Volume: <N prompts · N file touches · N shell commands · N PRs>
> Branch arc: <the sequence of branches if visible from PR activity>
>
> Methodology: gathered by `scripts/session_story_facts.py`. See README.

---

## <HH:MM> — <natural phase title from the rhythm + prompts>

<narrative, with verbatim quotes>

## <HH:MM> — <next phase>

...

---

## What the layers caught that the prose alone wouldn't

- <quantitative observation that anchors a qualitative claim>
- <another>

## Shape of the session, in one line

<the gestalt sentence>
```

---

## Example

See `4198566e-docker-rerun.md` in this directory — the worked example for
Engineer B's 30-hour docker-rerun → identity-gap → Users-tab session on May 1–2.
That file demonstrates the structure end-to-end and is the canonical
reference output of this methodology.

---

## When this methodology doesn't help

- **Tiny sessions** (< 30 events). The shape layers will be too sparse.
  Use the baseline `sessionstory` skill alone.
- **Multi-session arcs.** This is single-session. For a day or week
  across sessions, use the existing `team-day` skill instead.
- **Live sessions still in flight.** The shape layers are batch-built;
  they lag the live event stream. Catch up the builds first
  (`uv run scripts/build_*_shapes.py`) or use the baseline `sessionstory`
  skill on the live records.

---

## Honest limitations

- **Authorship attribution** is inferred from file paths (e.g.,
  `~/workspace/` → Engineer B). For sessions where everyone works
  in the same path, you can't tell who ran the session without other
  signals (host name, person_id from `/api/sessions`).
- **Tone and motivation** are not in the data. Don't write speculative
  emotional content. Stick to *behavior* and *artifacts*.
- **Sprint-step templated prompts** ("Sprint 9, Step N: …") are still
  parsed even though they're not authentic. Filter them out by hand if
  they distort the verb mix.
- **Path-shape's project-anchor** only normalizes paths inside
  `/OpenStory/`. Work in sibling repos (e.g.,
  `~/projects/openstory-ui-prototype/`) shows up under `top_segment="/"`
  and the naming-vocab dilutes. Pick sessions inside the project for the
  cleanest stories.

---

## Next thread to pull on

The cross-layer queries in `scripts/query_cross_shape.py` are not yet used
here. A natural extension: *"sessions whose shape resembles 4198566e"* — a
similarity query over the per-session shape histograms that would surface
*other* deploy-debug → feature-pivot sessions. The shape-as-fingerprint
question is the obvious next stop.
