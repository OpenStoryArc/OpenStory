# Shape-layer methodology — claim · layer · query · interpretation

How to turn the shape-layer family into trustworthy, re-derivable claims about a
corpus of OpenStory sessions. This is the methodology side of the work; the
data model itself lives in [`../semantic-layer.md`](../semantic-layer.md).

The point of writing this down is that **the artifacts produced from shape
layers (slides, reports, figures, narratives) are projections of a substrate
you can re-derive from raw events at any time.** The methodology is the
trustworthy thing; the artifact is just one rendering.

---

## What shape-layers are

Seven SQLite databases under `data/*_shapes.db`, all idempotent on `event_id`,
all joinable on `session_id`. Each is built by a script in `scripts/` that
walks the event store via the REST API, applies a selector to find relevant
records, runs an extractor, and upserts shaped rows.

```
agent-interaction layers (intent / attention / action):
  prompt-shape   user_message → spaCy dep parse → verbs / subjects / objects / chunks
  path-shape     tool_call w/ file_path → directory / basename / extension / depth / tokens
  bash-shape     tool_call name=Bash → shlex tokenize → program / subcommand / flags / pipeline%

content layers (code deltas + interior vocabulary + prose-named concepts):
  change-shape   Edit/Write/MultiEdit → lines & chars added/removed, edit_count, excerpt
  code-vocab     change-shape, code files → identifiers (regex; tree-sitter someday)
  docs-vocab     change-shape, prose files → headers + bold + noun chunks (spaCy)

working memory:
  snapshot-shape file_snapshot → tracked_count / new_files / bumped_files

cross-layer composition:
  query_session_shape.py SID         one session, one report
  query_cross_shape.py --when-verb   cross-cuts (e.g. paths-touched when verb=debug)
  analyze_session_shape.py           7-layer aggregator (has --test flag)
```

Full spec, schemas, and the original promotion path:
[`../semantic-layer.md`](../semantic-layer.md).

---

## The traceability pattern

Every claim derived from shape layers should be expressible as a four-part chain:

```
claim     →  one sentence, in plain language
layer     →  which shape DB the evidence comes from
query     →  the SQL (or join) that produced the numbers
numbers   →  the actual counts, in context
interpretation →  what the numbers mean, named as a reading
```

This is the *traceability discipline*. It lets a reader (or a future you)
verify, refine, or refute any claim without rebuilding the methodology from
scratch.

### A worked example

A retrospective slide carried the claim *"Engineer A is a deep reader, not a
typer."* Here is the traceability for that one line:

- **Layer**: path-shape (`tool` column)
- **Query**:
  ```sql
  SELECT tool, COUNT(*) FROM path_shapes
  WHERE session_id IN (engineer_a_sessions) GROUP BY tool;
  ```
- **Numbers**: `Read=3861, Edit=1833, Grep=714, Write=645, Glob=79`
- **Ratios**: Read : Edit : Write = 6 : 3 : 1.
- **Interpretation**: A 6× Read : Write ratio is materially higher than the
  2–3× ratio typical of editor-heavy sessions. Combined with the
  `COUNT(DISTINCT session_id)` over `code_vocab_shapes` — only **20.9%** of
  Engineer A's sessions wrote code at all — the "deep reader" framing is
  defensible. The numbers are derivation; "deep reader" is the reading.

The slide says one sentence; this doc shows the four pieces behind it.

### A second example, illustrating cross-layer composition

The same retrospective claimed *"Vocabulary incubation is real and
reproducible — words appear in prose ~8 weeks before they appear in code."*
That claim is too strong to rest on any one layer:

- **Layers involved**: docs-vocab + code-vocab + path-shape (naming tokens) +
  prompt-shape (noun chunks).
- **Cross-query** (sketch):
  ```sql
  -- earliest appearance of token "shape" in each layer
  SELECT 'docs',   MIN(timestamp) FROM docs_vocab_shapes
    WHERE headers GLOB '*shape*' OR noun_chunks GLOB '*shape*'
  UNION ALL
  SELECT 'code',   MIN(timestamp) FROM code_vocab_shapes
    WHERE identifiers GLOB '*shape*'
  UNION ALL
  SELECT 'path',   MIN(timestamp) FROM path_shapes
    WHERE naming_tokens GLOB '*shape*'
  UNION ALL
  SELECT 'prompt', MIN(timestamp) FROM prompt_shapes
    WHERE noun_chunks GLOB '*shape*';
  ```
- **Findings**: docs preceded code by ~10 days; code preceded path-as-script-name
  by ~10 more; path preceded prompt by ~4 weeks. Total arc: ~56 days from first
  appearance to dominance.
- **Interpretation**: This is the "incubate words before they become
  architecture" reading. The data shows ordering; the framing is a reading on
  top of it.

---

## Corroboration: one layer is a hypothesis, multiple layers is a finding

A claim supported by a single layer's query should be marked as a hypothesis,
not a fact. A claim that survives independent corroboration across two or more
layers built from different event types is much harder to explain away as an
extractor quirk or a sampling artifact.

A useful technique is the **corroboration table** — rows are claims, columns
are the seven layers, a dot marks every layer that contributes evidence:

```
                                prompt  path  bash  change  code  docs  snap
"deep reader, not a typer"                ●           ●      ●
"interrogative posture"            ●
"60.7% pipelined shell"                         ●
"vocabulary incubates ~8wk"        ●     ●            ●      ●     ●
```

Two-dot claims survive single-extractor failure. Four-dot claims are the
trustworthy headlines.

---

## Honesty about tiers

Not every claim should be presented at the same strength. A useful three-tier
convention:

- **Tier 1 — measured.** The number falls directly out of a query. "Engineer A
  ran 6,866 bash commands across 278 sessions." Reproducible to the integer.
- **Tier 2 — extrapolated.** The number comes from a sample, or from a proxy
  for the actual phenomenon. "≈4% error session rate" from a 50-session sample
  is Tier 2. Flag it.
- **Tier 3 — interpretive.** The framing on top of the numbers — "deep reader,"
  "ask before assert," "vocabulary incubation." These are readings, not
  derivations. They depend on choices of taxonomy, threshold, and frame. Name
  them as readings.

The strength of shape-layer claims is in honest tiering. Tier 1 numbers carry
the load; Tier 3 framings give the story; Tier 2 inferences should always be
flagged so the reader can decide how much weight to give them.

A retrospective that mixes the three tiers without labelling them is a vibes
document with footnotes. Labelling them transforms it into a research artifact.

---

## How to produce a traceability document for your own artifact

When you build a deliverable from shape-layer data — a slide, a report, a chart
— write a sibling `TRACEABILITY.md` next to it. The skeleton:

```markdown
# TRACEABILITY — <artifact name>

## Provenance
- Event stream: <which event store, what window, how many sessions>
- Shape layers: <table of which builders produced which rows, with counts>
- Analysis scripts: <which query / probe scripts ran>
- Methodology docs: links to this README + semantic-layer.md + worked example

## Claim-to-query map
For each non-trivial claim in the artifact:
- **Claim**: the exact sentence on the slide/in the report
- **Layer(s)**: which shape DB(s)
- **Query**: the SQL or join
- **Numbers**: the counts the claim rests on
- **Tier**: 1 (measured) / 2 (extrapolated) / 3 (interpretive)
- **Interpretation note**: if Tier 3, what the framing assumes

## How to re-derive
1. `sqlite3 data/<layer>_shapes.db`
2. Run the cited query, filtered as cited.
3. Compare. Divergence = (a) new events arrived, (b) build needs re-running,
   or (c) the methodology has a bug worth fixing.

## What this document is NOT
- Peer-reviewed analysis.
- Definitive — different taxonomies, different thresholds, different conclusions.
- Free of interpretation — Tier 3 framings are *choices*, named as such.
```

The discipline is small. The payoff is that any conversation a month or a year
later can verify, refine, or refute any single claim without you in the room.

---

## When the methodology doesn't help

- **Tiny sessions** (< 30 events). Shape layers will be too sparse. Use the
  baseline `sessionstory` skill alone (`.claude/skills/sessionstory/SKILL.md`).
- **Live sessions still in flight.** Shape layers are batch-built; they lag the
  live event stream. Catch the builds up first or work from the raw records.
- **Cross-corpus comparisons** without per-corpus rebuilds. Shape DBs are
  built from your local event store; comparing across machines requires
  exporting and merging or rebuilding both sides.

---

## Where the methodology is incomplete

These are honest gaps, not blockers — they're the next threads to pull on:

- **No schema versioning.** If a builder changes its output schema, old
  `*_shapes.db` files break silently. A `schema_version` row + a migration
  doc would close this.
- **Six of the seven layers don't yet have a written promotion path.** Only
  prompt-shape has one in `../semantic-layer.md`. The other six will follow the
  same pattern (promote to a consumer actor under `rs/server/src/consumers/`,
  add to the `EventStore` trait, expose via REST), but each needs a sentence
  on what's specific to it.
- **No `--test` flag on the builders.** Only `analyze_session_shape.py` has
  one. The build pattern is identical across the seven, so one `--test` flag
  on a representative builder would cover the family.
- **An eighth layer (token-shape) is hinted but not built.** The TokenUsage
  cache-field fix (commit `2e7b01d`) unblocked it; the design is in the
  BACKLOG.
- **Cross-corpus attribution.** Path-attribution (`/Users/$NAME/`) is brittle —
  it fails for pi-mono sessions, which use `pi://` source URIs. The
  PersonId/Principal work on `wip/shape-layers-and-friends` is the proper
  fix; until then, attribute manually.

---

## Related docs

- [`../semantic-layer.md`](../semantic-layer.md) — the seven-layer spec
  (selectors, extractors, schemas, prompt-shape promotion path)
- [`../session-stories/README.md`](../session-stories/README.md) — methodology
  for narrating *one* session using these layers
- [`../session-stories/4198566e-docker-rerun.md`](../session-stories/4198566e-docker-rerun.md)
  — worked example: a 30-hour session narrated from layer data
- `.claude/skills/sessionstory/SKILL.md` — record-level fact gatherer (the
  baseline this methodology builds on)
- `scripts/session_story_facts.py` — the multi-layer fact gatherer
- `scripts/analyze_session_shape.py` — the 7-layer aggregator (has `--test`)

---

## One-sentence summary

**Build the substrate once, derive claims from it many times, and write down
the four-part chain (claim · layer · query · interpretation) for every claim
that matters.** That's the methodology; the rest is execution.
