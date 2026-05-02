---
name: storyvideo
description: Generate a daystory video from OpenStory data — a 30-60s recap of a day's coding sessions, told as kinetic typography. A three-phase skill — facts (deterministic) → plan (model writes the narrative) → render (deterministic, via HyperFrames + FFmpeg). Use when the user asks "make a video of today" / "recap yesterday" / "what did I build this week" / "tell the story of session X as a video."
---

# storyvideo

A three-phase skill: a script collects the facts, **you write the story**, a script renders it to MP4.

The model's job is the middle phase — taking a slate of structured facts about a day and authoring a scene plan. The plan is what makes the video honest: which moments matter, which tone they hit, what arc emerges, what to skip.

## Phase 1 — Collect facts (deterministic)

```bash
python3 scripts/storyvideo.py 2026-04-19 --print-facts > /tmp/facts.json
```

Returns a JSON document with:

- `date`, `weekday_label`, `session_count`, `turn_count`, `record_count`
- `top_tools` — `[["Bash", N], ["Edit", N], ...]` aggregated across all sessions
- `sessions[]` — per session: `session_id`, `started_at`, `duration_hours`, `turn_count`, `tool_call_counts`, `sample_sentences` (the gold quotes from the `turn.sentence` detector), `prompt_timeline` (cleaned user prompts with timestamps)

**Read it carefully.** This is the raw material. The sentences and prompts ARE the day's voice — your job is to find the shape inside them, not to invent one.

## Phase 2 — Author the scene plan (model)

You write a JSON file at `data/plans/storyvideo/YYYY-MM-DD.json` (or anywhere — pass with `--plan`). The plan is a list of scenes that will become a video. Each scene is one of these templates:

### `title` — opening shot

```json
{ "template": "title", "duration": 5, "eyebrow": "▸ DAYSTORY", "headline_lines": ["from a tab character", "to a video that tells the day"], "subtitle": "Saturday · April 19, 2026" }
```

Use once at the start. The `eyebrow` is the video kind (`▸ DAYSTORY`, `▸ WORKSTORY`, `▸ SESSIONSTORY`). The `headline` is your *editorial summary* — what the work was about. Two short lines beat one long line.

### `chapter` — act break / section marker

```json
{ "template": "chapter", "duration": 4, "label": "ACT II — THE BUILD", "subtitle": "three prototypes, in parallel" }
```

Use 2-4 times to mark transitions in the arc. Acts (I/II/III) work for stories with clear stages. Or thematic chapters (`DETOUR`, `DISCOVERY`, `REFRAME`). Short — chapters are the silence between movements, not the music.

### `quote` — moment + context

```json
{ "template": "quote", "duration": 5, "eyebrow": "01:36 // OPENING", "text": "UI Work — can you fix and make this have code syntax highlighting?", "tone": "neutral", "attribution": "session ddba5c82" }
```

The workhorse. Use for prompts, decisions, discoveries, reframes. The `eyebrow` labels the moment (HH:MM, or a thematic label like `DISCOVERY` / `PIVOT` / `REFRAME` / `BUILD`). `text` is the on-screen line. `attribution` is optional — small text below.

Tones:
- `neutral` — default
- `warm` — italics + amber, for tender or generous moments (genuine compliments, gentle corrections, philosophical pivots)

### `reflection` — contemplative paragraph

```json
{ "template": "reflection", "duration": 7, "text": "All along, the skill we needed was already there — we just hadn't asked it the right question.", "attribution": "in retrospect" }
```

Italics, slower pace, longer text allowed (up to ~25 words). For: **thinking on top of what happened**. The reflection is where you say what a moment *meant*, not what it *was*. Use 1-3 per video. They're the air the rest of the scenes need to breathe.

### `moment` — energy or resistance beat

```json
{ "template": "moment", "duration": 4, "text_lines": ["Red.", "Green."], "subtext": "65 / 65 tests pass", "tone": "positive" }
```

Big text, short. For peaks. Use sparingly — these land hardest when surrounded by quieter scenes.

Tones:
- `positive` — amber, for breakthroughs, "we did it", green-test moments
- `negative` — dim red, for friction, dead ends, "subagents can't write"
- `neutral` — off-white

### `outro` — closing shot

```json
{ "template": "outro", "duration": 5, "tagline": "every quest has the moment you almost missed the gift" }
```

Use once at the end. The tagline is one line that resonates with the opening — not a summary, an echo.

## Phase 3 — Render (deterministic)

```bash
python3 scripts/storyvideo.py 2026-04-19 --plan data/plans/storyvideo/2026-04-19.json
cd scripts/recap-prototype/daystory-video
npx hyperframes lint
npx hyperframes render --quality draft --output daystory-2026-04-19.mp4
```

The script combines your plan with the day's stats (which become a persistent footer) and writes a HyperFrames composition to `scripts/recap-prototype/daystory-video/compositions/main-graphics.html`. The render takes ~1 min for 50s of video at draft quality.

## What makes a good plan

A good plan tells **the story of the work and the co-creative process**. It has **drama, nuance, and reflection** — not just a chronology of moments. Bad plans read like a database; good plans read like a person who was there is telling you what happened.

### Structural elements to look for

1. **The opening prompt** — what was the user actually trying to do? Quote it. It anchors everything.
2. **A discovery moment** — somewhere, the truth about a problem became visible. Often hidden in `sample_sentences` ("Found the real issue", "Looks like X is already...").
3. **A red→green beat** — TDD red/green, build failure → success, "stuck" → "unstuck". Energy peaks.
4. **A pivot** — the user changes direction, asks something tangentially curious. Often a question.
5. **A reframe / correction** — the user gently redirects. Warm moments that reveal the actual shape of the collaboration.
6. **Resistance** — limits, blockers, frustration, errors that mattered. Don't sanitize these out.
7. **A synthesis** — what did this thread of work add up to? The through-line.
8. **Closing / current state** — where it ended, or where it is right now.

### Writing with voice

Each scene is a few words on screen for a few seconds. The constraint is brutal — every word counts. Three rules:

**1. Specific concrete language beats labels.**
- Bad: `"text": "Found a bug in formatting."`
- Good: `"text": "Hidden in the data: a tab character pretending to be an arrow."`

**2. First-person collaborative voice beats neutral observer.**
- Bad: `"text": "The agent was unable to write files."`
- Good: `"text": "The agents we delegated to could read the world but not change it. We took the work back."`

**3. Reflection beats summary.**
- Summary (`quote`): `"Use the existing skills. sessionstory + daystory."`
- Reflection (`reflection`): `"All along, the skill we needed was already there — we just hadn't asked it the right question."`

The summary tells you what happened. The reflection tells you what it *meant*. A good plan has both, used in the right places.

### Pacing and rhythm

Aim for **9-13 scenes**, ~50-70s total. Vary scene types — too many `quote`s in a row reads like a list; too many `moment`s dilutes them. Use `chapter` to mark act breaks; use `reflection` to slow down at meaning-bearing moments.

A good rhythm for a multi-act story:

```
title → chapter(ACT I) → quote → quote → moment(±) → reflection
      → chapter(ACT II) → quote → quote(warm) → moment(±) → reflection
      → chapter(ACT III) → quote → reflection → outro
```

For a single-arc story (one session, one focused query):

```
title → quote(opening) → quote → moment(discovery) → quote(decision)
      → moment(red) → moment(green) → reflection → outro
```

### Honesty rules

- **Quote the data, don't invent it.** Use real prompts and real `sample_sentences`. Tighten and trim, but keep the voice.
- **Don't sanitize.** If something broke, if you got stuck, if the user gently corrected you — those moments are the most honest and they make the video real. Include them, including in the closing if appropriate.
- **Skip noise.** Generic acks ("sure :)", "ok"), tool boilerplate, self-referential loops. The detector includes everything; the plan is curated.
- **The closing should resonate, not summarize.** The outro tagline echoes the opening or the discovery — it's the meaning the work earned, not a stats recap.

## Plan schema reference

```json
{
  "$schema": "storyvideo-plan-v1",
  "date": "YYYY-MM-DD",
  "weekday": "Saturday",
  "headline": "one-line summary (used by the renderer's metadata)",
  "scenes": [ /* ordered list of scene objects */ ]
}
```

Stats (sessions / turns / records / tools) come from facts and become the persistent footer — DO NOT include them in the plan.

## Sanity-check mode

If you want to see the renderer pipeline run without authoring a plan, use `--raw` for mechanical sentence sampling. It produces a coherent-ish video with no narrative judgment — useful for verifying the rendering path works, not for showing anyone.

```bash
python3 scripts/storyvideo.py 2026-04-19 --raw
```

## When NOT to use this skill

- The user wants a *single-session* recap — author a plan with that session's prompts and `sample_sentences` only, OR start with the `sessionstory` skill and turn its output into a plan.
- The user wants live/streaming visualization — that's the dashboard, not a video.
- The user wants to query data, not narrate it — use `sessionstory`, `query_store.py`, or the OpenStory API directly.

## Adjacent skills and scripts

- **[sessionstory](../sessionstory/SKILL.md)** — single-session fact sheet. Pairs with this skill: use it to dig into a specific session before writing a plan that highlights that session.
- **`docs/research/scheme/daystory.sh`** — heuristic day-narration script (text only). Inspired this skill's narrative shape (energy / resistance markers, key moments, closing question).
- **`scripts/story_html.py`** — static HTML rendering of a session's structural turns. Complementary view of the same data.
- **HyperFrames docs** — `~/.agents/skills/hyperframes/SKILL.md` (composition rules) and `~/.agents/skills/hyperframes-cli/SKILL.md` (CLI). Read if you want to author the composition by hand instead of via `storyvideo.py`.
