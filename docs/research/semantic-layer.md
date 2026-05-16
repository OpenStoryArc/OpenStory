# Semantic layer over session data

> **Status: family of seven layers.** This doc was originally written for the
> prompt-shape layer alone. The pattern (**selector + extractor + storage**,
> keyed on `session_id`) has been replicated to six siblings. All seven live
> in `data/*_shapes.db` and are joinable on session_id.
>
> | layer | selector | extractor | db |
> |---|---|---|---|
> | **prompt-shape** | `user_message` (harness tags stripped) | spaCy dep parse → verbs/subjects/objects/chunks/adjectives/adverbs | `prompt_shapes.db` |
> | **path-shape** | `tool_call` with `file_path` / `notebook_path` / `path` | decompose → directory / basename / extension / depth / top_segment / naming tokens | `path_shapes.db` |
> | **bash-shape** | `tool_call` where `name == "Bash"` | `shlex` tokenize → program / subcommand / flags / args / pipeline / redirect | `bash_shapes.db` |
> | **change-shape** | `tool_call` where `name ∈ {Edit, Write, MultiEdit}` | extract `old_string` / `new_string` / `content` → lines & chars added/removed, edit_count, excerpt | `change_shapes.db` |
> | **code-vocab** | same as change-shape, code files only | regex-extract identifiers from `new_string` / `content`, exclude language keywords + stopwords, map `identifier → count` | `code_vocab_shapes.db` |
> | **docs-vocab** | same as change-shape, prose files (`.md / .markdown / .rst / .adoc / .txt`) | regex headers + bold + link labels; spaCy noun chunks from prose body (code fences stripped) | `docs_vocab_shapes.db` |
> | **snapshot-shape** | `file_snapshot` | diff `trackedFileBackups` against previous snapshot → `tracked_count`, `new_files`, `bumped_files`, `max_version` | `snapshot_shapes.db` |
>
> The first three are **agent-interaction layers** (intent, attention, action).
> The next three are **content layers** (code deltas + interior identifier
> vocabulary + prose-named concepts).
> The last is a **state-of-working-memory layer** (file tracking manifest over time).
>
> Why both `code-vocab` and `docs-vocab` rather than one unified vocab table:
> identifiers (callables / types) and noun chunks (named concepts) are
> semantically different artifacts and shouldn't share a histogram. Mixing them
> dilutes signal. Separate tables also let either evolve independently —
> `code-vocab` will upgrade to tree-sitter; `docs-vocab` already uses spaCy.
>
> Cross-layer composition lives in `scripts/query_session_shape.py SID` (one
> session, the original three views) and `scripts/query_cross_shape.py --when-verb /
> --when-program / --when-segment` (cross-cuts). The content layers are queried
> independently for now (`query_change_shapes.py`, `query_code_vocab.py`,
> `query_snapshot_shapes.py`); folding them into `query_session_shape.py` is the
> next natural step.
>
> A note on the snapshot-shape volume: `file_snapshot` records are emitted
> ~per-turn, so most rows are no-op heartbeats. Filtering to `new_files > 0 OR
> bumped_files > 0` cuts the noise (~57% of rows have non-zero deltas).

## What

A persisted, queryable layer of *grammatical shape* for every user prompt in
the store. Each record is the dependency-parsed skeleton of one user message:
root verb(s), subject(s), direct object(s), noun chunks, adjectives, adverbs.
Keyed by `event_id`, joinable on `session_id`.

## Why

OpenStory captures **what happened**: events, tools, files, turns. Deterministic
and trustworthy. The semantic layer captures **where the mind was pointing**
when each prompt was issued — the verbs of intent, the objects of attention,
the modifiers that color them.

Two things become possible:

1. **Read the direction of a session at a glance.** Dominant verb + object +
   modifier per session, plus a per-week trajectory. ("Last week was
   `see / openstory / semantic`; this week is `build / fleet / deterministic`.")
2. **Drill into areas of interest.** "Which sessions mentioned `openstory lab`?
   What were the surrounding verbs?" "When did `agentic` peak in my prompts?"
   "Which sessions changed direction mid-stream?"

This is the seed of the conversation in session `f3496df3`: the deterministic
ground (events) is what makes a semantic layer trustworthy rather than vibes.

## Where it lives (now)

`data/prompt_shapes.db` — a separate SQLite database next to `open-story.db`.
Disposable, rebuildable from events, never written to by the server. The main
store is unaffected.

Schema:

```sql
CREATE TABLE prompt_shapes (
    event_id        TEXT PRIMARY KEY,
    session_id      TEXT NOT NULL,
    timestamp       TEXT NOT NULL,
    seq             INTEGER NOT NULL,
    char_count      INTEGER NOT NULL,
    root_verbs      TEXT NOT NULL,   -- JSON array of lemmas
    subjects        TEXT NOT NULL,
    direct_objects  TEXT NOT NULL,
    noun_chunks     TEXT NOT NULL,
    adjectives      TEXT NOT NULL,
    adverbs         TEXT NOT NULL,
    prompt_excerpt  TEXT NOT NULL    -- first 200 chars for grep-ability
);
```

## Scripts

- `scripts/build_prompt_shapes.py` — walks sessions via REST API, parses every
  user prompt with spaCy (`en_core_web_sm`), upserts shapes. Idempotent on
  `event_id`. Re-run after new sessions arrive.
- `scripts/query_prompt_shapes.py` — read queries: shape of one session,
  sessions containing a verb/object/modifier, top-K aggregates, trajectory
  over time.

## Promotion path

When this earns its keep, it follows the standard OpenStory pattern:

1. **Promote to a consumer actor** in `rs/server/src/consumers/`. Subscribes to
   `events.>`, parses on the fly, writes to a `prompt_shapes` table in the
   main `EventStore`.
2. **Add to the `EventStore` trait** so both SQLite and Mongo back it. Mongo
   gets a `prompt_shapes` collection mirroring the SQLite schema.
3. **Expose via REST**: `/api/sessions/{id}/shape`, `/api/shapes/search?verb=...`.
4. **Render in the UI**: per-session shape strip in the sidebar, semantic
   trajectory chart on the project view.

Until then it stays in `scripts/` and `data/prompt_shapes.db`, in the research
garden where it can be reshaped freely without touching the durable store.

## Out of scope for v0

- Embedding-based similarity (use shapes first; embeddings are the next layer up).
- Real-time streaming (rebuild is cheap; ~30s for hundreds of sessions).
- Assistant-message shapes (the question is about *user intent*, not response).
- Cross-session trajectory metrics beyond per-week histograms.
