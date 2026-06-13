# Shape-layer architecture — elevating the deterministic foundation

*How the offline "shape-layer" script family becomes a first-class part of the
OpenStory runtime — and the exact algorithm each script encodes, because the
scripts are the spec.*

Status: research / design. Incubating before any Rust is written.
Source of the prototypes: branch `wip/shape-layers-and-friends` (`b038c43`).
Companion design doc on that branch: `docs/research/semantic-layer.md`.

---

## 1. The thesis

A session is a *process*. A prompt is a *procedure*. The same procedure can
generate radically different processes (SICP 1.2). OpenStory already shows
**what** happened (the timeline) and, via the patterns crate, **why** it
happened (eval-apply + `turn.sentence`). The shape layers add a third axis:
**what shape, and in what direction** — measured deterministically, no model
in the loop.

Every shape script is a *pure deterministic projection* of the CloudEvent
stream. That is the same thing every consumer actor in the runtime already is
(persist, patterns, projections, broadcast). The foundation is therefore
already shaped like the architecture — it just lives outside it, as batch
Python hitting REST and writing seven gitignored SQLite files.

### Soul alignment

- **Observe, never interfere** — every layer is read-only over the event stream.
- **Functional core, effects at the edge** — each builder is `record → row`,
  pure; I/O (`fetch`, `sqlite`) is at `main()`.
- **Open, user-owned data** — SQLite + (target) JSONL backup; reproducible from
  a clean checkout against the live store.
- **Scripts over rawdogging / scripts are the spec** — these scripts *are* the
  specification for the Rust port. §3 is that spec.

---

## 2. Current state (the prototype shape)

```
OpenStory REST API  ──►  build_*_shapes.py  ──►  data/*_shapes.db   (7 files, gitignored)
                                                      │
                         query_*_shapes.py  ◄─────────┤   per-layer readers
                         query_session_shape.py  ◄────┤   cross-layer (one session)
                         query_cross_shape.py  ◄───────┘   cross-layer (corpus inversion)
```

Seven layers, ~111k rows currently materialized on disk (built 2026-05-16):

| Layer | DB | Rows | Heavy dep |
|---|---|---|---|
| prompt-shape | `prompt_shapes.db` | 3,209 | spaCy |
| path-shape | `path_shapes.db` | 9,808 | — |
| bash-shape | `bash_shapes.db` | 9,844 | — |
| change-shape | `change_shapes.db` | 3,497 | — |
| code-vocab | `code_vocab_shapes.db` | 2,260 | — |
| docs-vocab | `docs_vocab_shapes.db` | 725 | spaCy |
| snapshot-shape | `snapshot_shapes.db` | 89,295 | — |

Every builder is **idempotent on `event_id`** (path-shape on `(event_id,path)`).
That property is not incidental — it *is* the streaming-consumer contract,
already satisfied. The batch design pre-paid for live ingestion.

---

## 3. Script inventory — what each does + primary algorithm

This is the spec. Each entry is `record → row` (builders), `rows → ranked
summary` (readers), or `records → score` (analyzers).

### 3a. Shape builders (the generators — one `ShapeExtractor` each)

**`build_prompt_shapes.py`** — grammatical skeleton of every user prompt.
- *Filter:* `record_type == "user_message"`, not `is_sidechain`, non-empty.
- *Clean:* strip synthetic harness tags (`SYNTHETIC_TAG_RE`); if an unmatched
  opening tag survives, drop the whole prompt as synthetic.
- *Algorithm:* spaCy parse (`en_core_web_sm`, input capped to 4000 chars).
  Walk `doc.sents`; for each `ROOT` token whose POS ∈ {VERB, AUX} record the
  lemma as a **root verb**, and its children with dep ∈ {nsubj,nsubjpass} as
  **subjects**, {dobj,obj} as **direct objects**. Collect `doc.noun_chunks`
  (drop pronoun-rooted, len≤1). Collect non-stop ADJ → adjectives, ADV →
  adverbs. Store all as JSON lists + a 200-char excerpt.

**`build_path_shapes.py`** — codebase attention footprint, per file touch.
- *Filter:* tool calls that carry a file path (Read/Edit/Write/Grep/Glob/…).
- *Algorithm (`decompose`):* normalize away the absolute project anchor to get
  a repo-relative path, then `PurePosixPath` it into directory / basename /
  stem / extension / depth / `top_segment` (first dir segment) / `dir_segments`.
  Recover compound extensions (`.spec.ts` → stem `foo`). `tokenize_stem`:
  split on `_-.` then on camelCase boundaries (`CAMEL_SPLIT_RE`), lowercase,
  drop digits/<2-char/stopwords → **naming tokens**. PK `(event_id, path)`.

**`build_bash_shapes.py`** — the shell dialect, per `Bash` call.
- *Filter:* `tool_call` with `name == "Bash"`.
- *Algorithm (`decompose`):* `shlex.split` (fallback to whitespace split);
  `program` = token0 with any `/usr/bin/` prefix stripped; `subcommand` = first
  non-flag token after the program *iff* program ∈ `SUBCOMMAND_PROGRAMS`
  (git/cargo/npm/…); `flags` = `-`-prefixed tokens; `args` = the rest.
  `is_pipeline`/`is_redirect` computed by `_outside_quotes` — a hand-rolled
  quote-state scanner so a `|` or `>` inside a quoted string isn't counted.

**`build_change_shapes.py`** — the actual delta of every edit.
- *Filter:* `tool_call` with `name` ∈ {Edit, Write, MultiEdit}.
- *Algorithm:* per tool — Edit: old_string→removed, new_string→added; Write:
  content→added (old unknown); MultiEdit: sum over `edits[]`, `edit_count` =
  len. `count_lines` = `\n` count + 1 if no trailing newline. Also chars
  added/removed and a 200-char excerpt of the new text for grep.

**`build_code_vocab.py`** — identifier vocabulary of code written.
- *Filter:* Edit/Write/MultiEdit against a code file (`.rs .py .ts .tsx .js
  .jsx .go .java .c .cpp .h .hpp`); others stored with empty map.
- *Algorithm:* regex `IDENT_RE` over the new text → `Counter{identifier:count}`,
  dropping stopwords (language keywords) and <3-char tokens. Stored as JSON.
  *Promotion path: tree-sitter is the rigorous form (named in the docstring).*

**`build_docs_vocab.py`** — named concepts in prose written.
- *Filter:* Edit/Write/MultiEdit against `.md .markdown .rst .adoc .txt`.
- *Algorithm:* four extractors — `headers` (per-format header regex, inner
  markup stripped), `bold_terms` (`**…**`, 2–80 chars), `link_labels`
  (`[label](url)`, skip URL-ish labels), and **noun chunks**: first
  `preprocess_for_prose` strips code fences / inline code / HTML / emphasis /
  list+table markers so code never masquerades as prose, then spaCy noun_chunks
  (doc capped at 50k chars, drop pronoun-rooted / all-stopword / out-of-range).

**`build_snapshot_shapes.py`** — working-memory timeline (a manifest, no content).
- *Filter:* `record_type == "file_snapshot"`, sorted by (timestamp, seq).
- *Algorithm:* fold over the session's snapshots carrying `prev_versions`. From
  `payload.tracked_files.trackedFileBackups` read each path's `version`.
  `new_paths` = paths absent in prev; `bumped_paths` = paths whose version
  changed; `max_version` = max; `tracked_count` = size. Path lists capped to 50.
  This is the only **stateful** builder (it diffs against the previous snapshot
  in-session) — relevant for the streaming port (§4).

### 3b. Per-layer readers (`query_*_shapes.py`)

All follow one shape: load the layer's rows for a session (or corpus), fold
the JSON columns into `collections.Counter`s, print `most_common(N)`. They are
aggregate-and-rank views, not new logic — the determinism lives in the
builders. (`query_prompt_shapes`, `query_path_shapes`, `query_bash_shapes`,
`query_change_shapes`, `query_code_vocab`, `query_docs_vocab`,
`query_snapshot_shapes`.)

### 3c. Cross-layer joiners (the payoff)

**`query_session_shape.py SESSION_ID`** — one session across prompt+path+bash.
Joins the three agent-interaction layers on `session_id` and prints a unified
picture: what the user pointed at (verbs/objects/chunks/modifiers) → where
attention went (top segments / extensions / naming vocab) → what was run
(programs / subcommands / pipeline rate).

**`query_cross_shape.py`** — corpus inversion; the single most evocative view.
- *Algorithm:* filter on **one** layer to get a session-id set, then aggregate
  the **other** layers over exactly those sessions.
  - `--when-verb V` → `sessions_with_verb` scans `prompt_shapes.root_verbs`;
  - `--when-program P` → `SELECT DISTINCT session_id … WHERE program=P`;
  - `--when-segment S` → `… WHERE top_segment=S`.
  Then `aggregate_prompts/paths/bash` run `… WHERE session_id IN (…)` and fold
  into Counters, printed top-N. This is "sessions whose prompts say *see* →
  what areas and commands followed" — intent→outcome flow over the whole corpus.

### 3d. Process-shape analyzers (SICP made literal)

**`analyze_session_shape.py`** — classify a session's *process shape*.
- *`extract_features`:* raw signals — tool_calls, agent_spawns, burst_widths
  (parallel tool fan-out), read/write/search/bash counts, distinct_files,
  Task create/complete counts, pending_tasks_at_end, accumulation_ratio,
  working_set_growth (`final_window / max_window`).
- *`classify` → five axes in [0,1]:*
  - **tree** — `min(1, spawns/5)*0.85 + min(1, wide_bursts/3)*0.15` (subagent
    fan-out dominates; parallel bursts are a weak secondary).
  - **iterative** — `read_edit_share*1.5 + working_set_growth*0.4 − 0.3`
    (tight Read/Edit loops on a stable recent file set = tail recursion).
  - **accumulating** — Task imbalance (`(ratio−1)/2`) + pending tasks + a
    read/write-imbalance fallback when no Task tool was used (reads ≫ writes
    AND ≥8 files = gathering without committing).
  - **narrowing** — `contraction*0.6 + search_lead*0.4`, gated on
    `working_set_growth < 0.4` and ≥3 files (working set contracts = bisect).
  - **exploratory** — gather-share over edit-share, stateless bonus (SICP
    doesn't name this — it's an instrumental loop, not a value computation).
  - *Dominant label* only when the top axis leads the second by ≥0.15; else a
    two-shape mix (`tree+iterative`); `weak-signal` if top < 0.25.
- *`compute_trajectory`:* the **direction** primitive — slice the session into
  k windows, classify each, emit a path through shape-space
  (`iter→explore→tree→iter`) plus per-axis `drift` (first-half mean → second).
  This is "shape *and direction*" in the most literal form.

**`visualize_session_shape.py`** — draw it the way SICP draws recursion.
`--profile` multi-track ASCII sparklines (TODOs as the deferred-op stack /
agent spawns / rolling workset / tool rate); `--trace` indented event log where
*indent depth = pending TODOs* (the factorial-trace pyramid); `--compare A B`
two profiles side by side (the recursive-vs-iterative diagram).

### 3e. Exploratory probes (ancestors — keep for lineage, don't port)

`_probe_data_layers.py`, `_probe_prompt_shape.py`, `_probe_tool_vocab.py`,
`_session_shape_inventory.py`, `_session_shape_probe.py` — the spikes that
discovered the layer shapes before they were formalized. Prototype-left trail;
not part of the runtime.

### 3f. Introspection tier (B — intentions↔outcomes traversal, sits on top)

The 90/10 family. These read patterns + REST (not the shape DBs) and pin one
traversal of the intentions↔outcomes graph each.

**`why_this_pr.py`** — `gh pr create` (`PR_CREATE_RE`) is the anchor; title via
`--title`. For each PR event, the **prompt trail** = `turn.sentence` patterns
in the session ordered by `metadata.turn`; plus plan writes and commits
(`COMMIT_RE` + a HEREDOC form `git commit -m "$(cat <<EOF …`).

**`prompt_trail.py`** — verb-mix → archetype classifier. Tally `turn.sentence`
verbs into WRITE/EXPLAIN/CHECK/SHIP share buckets; ordered first-match rules:
`explain≥0.45 → socratic`; `(write+ship)≥0.55 ∧ explain<0.20 → directive if
ship≥0.10 else plumbing`; `check≥0.30 ∧ explain≥0.20 → recovery`;
`write≥0.40 ∧ explain≤0.30 → directive`; else `mixed`. (ship = "ran"+"committed"
is the directive-vs-plumbing discriminator.)

**`state_provenance.py`** — file → owning prompt, walked backwards. FTS-search
the basename (capped at limit 50 — the server drops the connection above that),
take the top ≤40 sessions by rank, find Write/Edit/MultiEdit calls whose
`file_path` == the target, and for each write timestamp `turn_for_time` = the
latest `turn.sentence` with `started_at ≤ ts` → the owning turn + prompt.

**`skills_used.py`** — `Base directory for this skill:` (`SKILL_INVOKE_RE`) =
the invoke anchor; skill reads via `…/SKILL.md` path; "knowledge" = Reads of
`.md` outside excludes.

**`openstory_tier_usage.py`** — how OpenStory introspects itself, 5 regex
signatures: `RAWDOG_RE` (grep .jsonl), `REST_RE` (`/api/*`), `SCRIPT_RE`
(named scripts), `SKILL_INVOKE_RE`, `MCP_RE` (`mcp__openstory__*`). One label
per record via `classify_record`.

**`pr_retrospective.py`** — joins `gh pr list` to sessions. Builds a session
index, matches PRs (incl. `gh pr edit N` backfill), buckets by `phase_for_date`,
and renders a per-PR block reusing `why_this_pr.WhyPr`. Engine behind the
682-line `pr-retrospective.md`.

### 3g. Session-story tier (C — narration substrate)

**`session_story_facts.py`** — companion to the live `sessionstory.py`. Reads
the three shape DBs (prompt/path/bash) and emits four fact blocks: (1) the
three-layer shape report, (2) prompt sequence with HH:MM stamps, (3) hourly
rhythm (prompts/paths/bash per hour — reveals working blocks and sleep gaps),
(4) PR & git activity (every `gh pr`/`git push`/`git merge` with timestamp).
Emits facts only; narration is the agent's job per
`docs/research/session-stories/README.md`. This is the ancestor of the live
`sessionstory` skill — but only the record-level baseline shipped; this
shape-layer upper tier never merged.

---

## 4. Target architecture — three existing seams

The runtime is already a set of independent consumer actors over NATS, each
owning a derived projection. Shapes slot into three seams that already exist.

### Seam 1 — `ShapeExtractor` trait (functional core, mirrors `TurnDetector`)

```rust
// open-story-shapes (new crate), sibling to open-story-patterns
pub trait ShapeExtractor {
    /// Pure: one CloudEvent → zero or more shape rows. No I/O.
    fn extract(&self, event: &CloudEvent) -> Vec<ShapeRow>;
}
```

Each §3a builder becomes one implementor (`PathShape`, `BashShape`,
`ChangeShape`, `CodeVocab`, `SnapshotShape`, …). This mirrors the existing
`TurnDetector` family exactly. The cardinality differs and that's the reason
to keep them separate: `TurnDetector` is *temporal/structural* (one pattern per
turn); `ShapeExtractor` is *lexical/dimensional* (one row per event).

**Statefulness note:** six of seven builders are stateless per event and port
cleanly. `build_snapshot_shapes` diffs against the previous in-session snapshot
— in a streaming consumer it needs a small per-session `prev_versions` map held
in actor state (the projections consumer already keeps per-session state, so
this is an established pattern, not a new one).

### Seam 2 — a `shapes` consumer actor (streaming, mirrors persist/patterns)

A tokio task subscribing to `events.>`, independent failure domain, calling the
extractors per event and writing rows. Idempotency-on-`event_id` is already
guaranteed by the builders, so live dedup is free. Output may flow to its own
NATS stream (`shapes`) for symmetry with `patterns`, or write straight to the
store — decide when there's a second reader that needs the stream.

### Seam 3 — a `ShapeStore` behind the trait seam (sovereignty preserved)

Shape rows become tables/collections behind the same async store trait as
`EventStore`, inheriting:
- the **SQLite + Mongo** duality for free,
- the **C1/C2/C3 conformance suite** (the scripts' `--test` fixtures *are* the
  parity spec),
- the **JSONL backup** guarantee — today the shape DBs have no sovereignty
  escape hatch; behind the store they get the same "grep-able from outside the
  DB" promise as every other projection.

### The user-facing payoff (what justifies the work)

Not "shapes in Rust" — the payoff is `query_cross_shape` becoming a first-class
API + UI dimension: `GET /api/shapes/cross?when-verb=see` →
`{areas, commands, …}`, and `GET /api/sessions/{id}/shape`. That turns a
private CLI against a gitignored DB into the reactive third axis the UI is
missing (what → why → **what-shape/direction**). Build the work toward that
endpoint; don't port a layer no view reads yet.

---

## 5. The one real obstacle — spaCy

Five layers (path, bash, change, snapshot, code-vocab) are pure string/structural
work and port to Rust trivially. **Two depend on spaCy** (prompt-shape's
verb/subject/object parse; docs-vocab's noun chunks). spaCy has no real Rust
equivalent. Options, in preference order:

1. **Reframe onto existing machinery.** The `SentenceDetector` already derives
   verb/subject/object deterministically *without NLP*, from tool roles and
   structural position. Re-derive prompt-shape's skeleton from that rather than
   from spaCy POS tags. Weaker than spaCy, native, composes with the pattern
   pipeline.
2. **Keep linguistic layers as optional batch enrichment** (`--features nlp` or
   a sidecar nobody must deploy) while the cheap five stream live. Honest tier
   split.
3. **(Avoid)** a permanent Python NLP sidecar — violates "minimal, honest code"
   and drags a heavy runtime into a single-binary product.

Recommendation: ship the five structural layers native+streaming; treat the two
linguistic layers as a separate, later decision. Don't let the hardest 2/7 gate
the easy 5/7.

---

## 6. Phased promotion path

1. **Restore the prototypes** into the working tree (they're stranded on
   `wip/shape-layers-and-friends`; the data is alive locally) so the family is
   runnable and the `--test` fixtures are exercisable as the spec.
2. **One layer, end-to-end.** Pick path-shape or bash-shape (cheap, no NLP,
   immediately demoable). Build `ShapeExtractor` + the `shapes` consumer +
   `ShapeStore` + one `/api/shapes/cross` endpoint + a minimal UI panel. Prove
   the seam.
3. **Replicate** the remaining four structural layers — mechanical once the
   trait is real.
4. **Decide the linguistic two** (§5) only when a feature needs them.
5. **B and C tiers** (introspection traversal, session-story facts) stay as
   scripts longer — they lean on patterns + REST, not the shape DBs, and are
   the natural place for the "10% narrative overlay." Promote individual
   traversals to projection consumers only when a query proves hot.

## 7. Don't-build list (per project principles)

- Don't port all seven at once.
- Don't materialize a layer no API/UI reads yet.
- Don't build the `shapes` NATS stream until a second reader needs it; write to
  the store directly first.
- Don't solve spaCy heroically; the five native layers carry most of the value.
