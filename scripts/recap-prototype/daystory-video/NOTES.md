# daystory-video — NOTES

## What this is

A video composition built **mechanically from real OpenStory data**, not hand-picked.
The pipeline:

```
GET /api/sessions
  -> filter to sessions overlapping a date
    -> for each: python3 scripts/sessionstory.py SID --json
       -> aggregate sample_sentences (turn.sentence detector "gold quotes")
          -> emit compositions/main-graphics.html
             -> npx hyperframes render
```

The transformer is `scripts/storyvideo.py`. Re-run any day, get a fresh recap.

## What worked

- **Reuses existing skills** — `sessionstory.py` already extracts the structured
  fact sheet; this script just composes them. No duplicated query logic.
- **Honest output** — cards are sampled evenly across the day, no editorial bias.
  Today's reel includes casual prompts ("sure :)", "you rawdoggin again bro")
  alongside substantive ones ("Okay let's do it. plan it out"). That's the truth
  of a day's work.
- **Composition is data-driven** — the timeline + cards are generated from
  Python; the HyperFrames composition is just the rendering surface.

## What didn't / what to change for v2

- The detector's sentence format ("Claude edited X, while testing N checks,
  because '<prompt>...' → answered") is verbose. Reads better as text than as
  on-screen typography. v2 should either (a) split the sentence into
  prompt-on-top + verb-phrase-on-bottom for visual hierarchy, or (b) extract a
  shorter canonical phrase from the sentence record's metadata.
- 10 cards × 4s = 40s body — feels right for ~20 sessions. For lighter days,
  the script should auto-shrink card count.
- `sessionstory.py --json` caps `sample_sentences` at 8 per session. For
  high-turn sessions (66 turns!) we lose context. Worth a `--max-sentences` flag.

## Rerunning

```bash
python3 scripts/storyvideo.py                 # today (UTC)
python3 scripts/storyvideo.py 2026-04-19      # specific date
python3 scripts/storyvideo.py --max-cards 12  # more cards
cd scripts/recap-prototype/daystory-video && npx hyperframes render --quality draft
```

## Source data for THIS render

19 sessions on 2026-04-19, 511 total turns. 10 cards selected by even sampling
across the day's gold sentences.
