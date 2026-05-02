---
name: workstory
description: Generate a video that tells the story of a focused thread of work — sessions matching a query (e.g. "videos for OpenStory today", "the actor refactor this week", "everything we did on ingestion since the merge"). Same three-phase agentic pipeline as storyvideo (facts → plan → render) but scoped by content, not just date. Use when the user asks "tell the story of [topic] today/this week" / "make a video about the X work" / "recap our work on [thread]" / "what's the story of [thread of work]?"
---

# workstory

A scoped sibling of [storyvideo](../storyvideo/SKILL.md). Same architecture; the difference is **what gets included**.

`storyvideo` gives you the whole day. `workstory` gives you a *thread* — the body of work matching a query, narrated as one arc. It's the right tool when the user has a thread in mind ("the video work", "the ingestion fixes", "everything I did on tests this week") and wants the story of that thread, not a day's grab-bag.

## The whole flow

```bash
# Phase 1 — facts, scoped by query
python3 scripts/storyvideo.py 2026-04-22 --query "video hyperframes daystory" --print-facts > /tmp/facts.json

# Phase 2 — model reads /tmp/facts.json, writes the plan
#          (this is YOUR job — see the storyvideo skill for plan schema and writing rules)
#          save to data/plans/workstory/2026-04-22-video-work.json

# Phase 3 — render
python3 scripts/storyvideo.py 2026-04-22 \
  --query "video hyperframes daystory" \
  --plan data/plans/workstory/2026-04-22-video-work.json
cd scripts/recap-prototype/daystory-video
npx hyperframes lint
npx hyperframes render --quality draft --output workstory-video-2026-04-22.mp4
```

## Picking the query terms

The query is **whitespace-split, lowercased, OR-matched** against each session's `sample_sentences` and `prompt_timeline.content`. Sessions are kept if ANY term appears.

- Use 3-6 specific terms that span the vocabulary the work likely touched.
- Include both the user's framing and the technical surface ("video", "hyperframes" — the user said one, the agent typed the other).
- Avoid terms so common they match everything ("the", "code", "file"). Terms shorter than 3 chars are dropped automatically.
- Iterate: run `--print-facts --query` first to see how many sessions match. If it's 0, broaden. If it's 19 (everything), narrow.

Example queries:
- `--query "hyperframes video skill render"` → today's video infrastructure work
- `--query "actor refactor consumer broadcast"` → the actor pipeline thread
- `--query "yc application pitch demo"` → YC prep work
- `--query "syntax highlighting turncard tdd"` → the morning UI fix

## Title eyebrow

Use `▸ WORKSTORY · <query>` for the title scene's eyebrow so the video is honest about its scope:

```json
{ "template": "title", "duration": 5, "eyebrow": "▸ WORKSTORY · video work", "headline_lines": ["..."], "subtitle": "Wednesday · April 22, 2026" }
```

## Rhythm for a thread (vs a whole day)

A workstory has **one arc**, not many. The good rhythm is more linear:

```
title
  → chapter (ACT I — opening)
    → quote (the prompt that started this thread)
    → quote (early exploration)
  → chapter (ACT II — the work)
    → quote (decision)
    → moment(±) (peak)
    → reflection (what that meant)
  → chapter (ACT III — landing or pivot)
    → quote (resolution)
    → reflection (what the thread added up to)
  → outro
```

Aim for 9-12 scenes, ~55-70s. The chapters are the spine — they make the arc visible.

## What to read in the facts

When the model receives `facts.json` for a workstory, look for:

- **The first prompt that mentions the topic** — that's the opening. Quote it verbatim if it has voice; tighten if it rambles.
- **The pivot points** — moments where the user changed direction, asked "what about", reframed the goal. These are act breaks.
- **The "stuck" moments** — errors, blockers, dead ends. Even small ones. They give the story tension.
- **The "ah, ok" moments** — the agent saying "found it", "looks like X is already...", "actually, this should...". These are the discoveries.
- **The user's gentle corrections** — the warm beats. Quote them in `tone: warm`.
- **What the thread became at the end** — not just "we shipped X", but "what did this work *teach*?" The reflection scene is for this.

## What separates a workstory from a daystory

A daystory says: *here is everything that happened today.*
A workstory says: *here is the story of this body of work.*

The daystory's job is to be inclusive. The workstory's job is to be **focused**. If a moment doesn't serve the thread's arc, leave it out. The 12 scenes are precious; they're the chapters of a single story, not a sample of many.

## When to use this vs storyvideo

- **storyvideo** — "what did I do today?" or "what happened on April 19?" — broad, multi-arc, recap.
- **workstory** — "tell me the story of the video work today" or "what happened with the ingestion refactor this week?" — narrow, single-arc, narrative.
- **sessionstory** — "what happened in session X?" — single session, deepest detail. Use this skill *inside* workstory when you need to dig into one of the matching sessions.

## Adjacent skills and scripts

- **[storyvideo](../storyvideo/SKILL.md)** — full plan schema, scene templates (`title`, `chapter`, `quote`, `reflection`, `moment`, `outro`), narrative writing rules, tone guidance, honesty rules. Read this skill's "What makes a good plan" section before authoring a workstory plan — the rules are the same.
- **[sessionstory](../sessionstory/SKILL.md)** — drill into one of the matching sessions when the facts.json sample isn't enough.
- **`scripts/storyvideo.py`** — the underlying script. `--query`, `--plan`, `--print-facts`, `--raw` modes all available.
