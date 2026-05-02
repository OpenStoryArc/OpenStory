# InsightExtraction Consumer — Design Sketch

**Status:** Design exploration. Not implemented. Driven by the DORA Advisor architecture (`workspace/agentic-learning/dora-metrics/ARCHITECTURE.md`).

**Soul check:** This is a mirror, not a leash. The consumer never modifies events, never injects behavior, never blocks any other actor. It produces *derived structured facts* from observed events and writes them to its own table. Same posture as the patterns consumer.

---

## What it is

A fifth actor-consumer in the existing pipeline. Where the **patterns consumer** detects deterministic structural patterns (eval-apply, turn.sentence) using state machines, the **insights consumer** uses an LLM to extract semantic facts: skill patterns, gaps, controllability classifications, prompt-engineering observations.

The output is a queryable index of "what this session reveals about the user's work" — not the verbatim transcript, not a structural summary, but *typed assertions with evidence*.

**Example outputs** (from the DORA Advisor concept):

| Type | Example | Evidence |
|------|---------|----------|
| `skill_pattern` | "User reaches for tests before code in this codebase" | event_ids: [a, b, c] |
| `identified_gap` | "Slow on Rust async — repeats `.await` in non-async fns 3× per session" | event_ids: [d, e, f] |
| `controllability` | "CI flakiness flagged 4× — classified as platform-controlled, not user-controlled" | event_ids: [g, h] |
| `prompt_pattern` | "Iterates on prompts when output exceeds 5 paragraphs" | event_ids: [i, j] |
| `efficiency_loss` | "Tool-loop on Bash → Edit → Bash for 8 cycles before resolving" | event_ids: [k, l, m] |

These are first-class facts a user (or another agent) can query, not paragraphs of LLM-generated prose.

---

## Where it sits in the pipeline

```
Source (file watcher → translator)
    │
    ▼
 CloudEvents (the pure observation)
    │
    ├─→ persist consumer       → events table
    │
    ├─→ patterns consumer      → patterns table (turn.sentence, eval_apply.*)
    │       │
    │       └─→ NATS patterns.{project}.{session}
    │                  │
    │                  ▼
    │           ┌────────────────────────────────┐
    │           │  InsightExtraction consumer    │  ← NEW (Actor 5)
    │           │                                │
    │           │  Buffers patterns per session  │
    │           │  → LLM extract on flush        │
    │           │  → write to insights table     │
    │           └────────────────────────────────┘
    │                       │
    │                       ▼
    │                 insights table (queryable, structured)
    │
    ├─→ projections consumer   → SessionProjection
    │
    └─→ broadcast consumer     → WebSocket
```

**Subscribes to:** `patterns.>` — specifically `turn.sentence` patterns. Each sentence is a complete, structurally-finished model turn — natural unit for insight extraction. (Optionally also `eval_apply.scope_close` for finer granularity, but sentences are the cleaner default.)

**Does NOT subscribe to:** raw `events.>`. The patterns consumer already does the structural pre-processing; the insights consumer reads the post-processed view. This keeps it cheap (fewer messages) and well-typed (PatternEvents have schema, raw CloudEvents are heterogeneous).

**Output:** rows in a new `session_insights` SQLite table. Optionally also publishes `insights.{project}.{session}` on NATS for live UI updates (Story tab could surface insights as they're extracted).

---

## Schema

```sql
CREATE TABLE session_insights (
    id              TEXT PRIMARY KEY,    -- hash(session_id + fact_type + content)
    session_id      TEXT NOT NULL,
    project_id      TEXT NOT NULL,
    fact_type       TEXT NOT NULL,       -- skill_pattern, identified_gap, controllability, prompt_pattern, efficiency_loss
    content         TEXT NOT NULL,       -- the assertion ("User reaches for tests...")
    evidence_event_ids TEXT NOT NULL,    -- JSON array of CloudEvent ids
    confidence      REAL NOT NULL,       -- 0.0..1.0 from the model
    model           TEXT NOT NULL,       -- which LLM produced this (claude-haiku-4-5-...)
    created_at      TEXT NOT NULL,       -- RFC3339
    superseded_by   TEXT                 -- nullable; set when a re-extraction replaces this row
);

CREATE INDEX idx_insights_session ON session_insights(session_id);
CREATE INDEX idx_insights_project_type ON session_insights(project_id, fact_type);
CREATE INDEX idx_insights_created ON session_insights(created_at);
```

**Deterministic ID** = `hash(session_id + fact_type + content)`. Same session re-processed produces the same id for the same fact, so re-extraction collapses naturally. The `superseded_by` column lets a fact evolve ("user reaches for tests" → "user reaches for tests *for new modules but not refactors*") without losing history.

**Per-backend parity:** mirror in MongoDB as a `session_insights` collection with the same fields, identical conformance helpers (per the existing C1/C2/C3 model in `rs/store/tests/event_store_conformance.rs`).

---

## When extraction runs (the trigger model)

Three options, complementary not exclusive:

### A. Batch on session completion (default)
A session is "complete" when no new events arrive for `stale_threshold_secs` (default 300s; already in `Config`). Once stale:
1. Fetch all `turn.sentence` patterns for the session.
2. Fetch the corresponding ViewRecords (for evidence text the LLM can read).
3. One LLM call: "given these N turns, extract structured facts of the following types: ...".
4. Validate the response (typed schema), write rows.

**Cost:** one LLM call per session. Bounded. Predictable.

### B. Incremental fast-path
Trigger an extraction earlier when a strong signal fires:
- An error pattern appears (`system.error` event)
- An `eval_apply` cycle exceeds N iterations (likely tool-loop)
- A `system.compact` event arrives (context limit hit — high-value moment to capture what happened)

These are cheap to detect (already in the patterns consumer's output). Each is one extra LLM call but earlier, so the user sees insights mid-session instead of after.

### C. On-demand
Expose `POST /api/sessions/{id}/extract-insights` for manual triggering. Useful for re-extraction after a model upgrade, or for sessions ingested from JSONL backfill.

I'd ship **A first** (simplest, fully bounded cost), add **B** when the demand is real, and **C** is trivial to add anytime since it's just a wrapper around the same code path.

---

## Privacy / consent (this is non-negotiable)

OpenStory's soul: "the user's data is theirs." This consumer does the one thing that violates that posture by default — sends conversation content to a third-party LLM. So:

- **Off by default.** New config field: `insight_extraction_enabled = false`.
- **Explicit model choice.** `insight_extraction_model = "claude-haiku-4-5-20251001"` (cheap default) or a local Ollama-style model URL for sovereignty.
- **Per-project opt-in.** A user might want extraction on personal projects but not on a client's codebase. The `project_id` is already in every event; gate at that level.
- **Opt-out audit.** Every extraction call is logged with `(session_id, project_id, model, token_usage)` so the user can see exactly what left their machine.

The "local model" path is important. OpenStory's principle is sovereignty; if the user runs a local model (llama, qwen, etc.), insight extraction stays on the box. The interface is the same — only the URL differs.

---

## Integration with the existing actors

**`open-story-store`:**
- New `insights_store.rs` parallel to `pattern_store.rs`. Same trait shape (write/read/list).
- Extends the `EventStore` trait? Probably not — insights aren't events. Probably its own trait `InsightStore` with `SqliteInsightStore` and `MongoInsightStore` impls.

**`open-story-server`:**
- New consumer in `consumers/insights.rs`. Same actor pattern as `consumers/patterns.rs`.
- New API endpoints: `GET /api/sessions/{id}/insights`, `GET /api/projects/{id}/insights?fact_type=...`, `POST /api/sessions/{id}/extract-insights`.

**`open-story-mcp` (assuming the MCP tools live nearby):**
- New tool `session_insights(session_id, fact_type=None)` — returns the insight rows with evidence event_ids.
- New tool `cross_session_insights(project_id, fact_type=None, days=N)` — aggregates insights over time. Answers "what gaps have I been hitting all month?"

**UI:**
- Story tab adds an "Insights" panel showing the structured facts for the current session, linked to evidence events.
- Sidebar could show insight badges per session ("3 gaps identified", "1 efficiency loss").

---

## What this is *not*

- **Not a session summary.** A summary is prose; insights are structured assertions. Both have value; this consumer produces the structured kind.
- **Not a chat advisor.** A chat advisor (`/api/ask`) consumes insights, doesn't produce them. Different concern. (See backlog: "/api/ask — multi-store agentic advisor".)
- **Not a replacement for the patterns consumer.** The patterns consumer does deterministic structural detection in real time. The insights consumer does LLM-driven semantic interpretation in batches. Both run.
- **Not an anomaly detector.** Anomaly detection (backlog: "Anomaly Detection & Behavioral Alerts") is rule-based, real-time, and on raw events. Different consumer, different concern, would write to its own store.

---

## Open questions

1. **Schema rigidity vs. flexibility.** Should `fact_type` be an enum (typed, queryable, predictable) or a free-text field (lets the LLM invent useful new types)? I'd lead with an enum + a generic `observation` bucket for things that don't fit, and promote types as patterns emerge.

2. **Per-session vs. cross-session.** A single session yields fairly thin insights ("you used Bash 12 times"). The interesting facts emerge across sessions ("you used Bash 12 times *every session this week — usually before reaching for a Read*"). Should the consumer extract cross-session insights too? Probably yes, on a separate cadence (daily? weekly?), as a second pass that takes the per-session insights as input.

3. **Versioning insights when the model changes.** When you upgrade from Haiku 4.5 to Haiku 4.6, do you re-extract everything? The `model` field is there for this — query "show insights extracted by model X" lets you A/B compare and reprocess only what's worth reprocessing.

4. **Confidence calibration.** The model produces a `confidence` score, but LLMs are notoriously miscalibrated. We probably want to gate UI display on confidence ≥ 0.7 by default and let the user dial it. Or: use the LLM's confidence as a tie-breaker but rank primarily by *evidence count* (how many event_ids back the assertion).

5. **Cost ceiling.** Even at Haiku rates, a 9000-event session could be expensive if every turn.sentence is included verbatim in the prompt. Pre-summarize? Sample? Cap at N most-recent sentences? The DORA project doesn't grapple with this because their conversations are short. Worth measuring before committing to a strategy.

---

## Estimated scope

- New `InsightStore` trait + SQLite + Mongo impls + conformance: **~250 LOC**
- `consumers/insights.rs` actor: **~200 LOC**
- LLM integration (Anthropic SDK or local model HTTP): **~150 LOC**
- API handlers: **~100 LOC**
- MCP tools: **~80 LOC** each
- UI Insights panel: **~300 LOC**
- Tests (unit + conformance + golden): **~400 LOC**

**~1500 LOC end-to-end**, with the core pipeline (consumer + store + one tool) being **~600 LOC** for a usable v0.1.

---

## Open questions for the next conversation

- Which fact types matter most to start with? `efficiency_loss` and `identified_gap` feel highest-leverage for a coding agent observer.
- Local model vs. API as the default — is the friction of asking users to run a local model worth the sovereignty win?
- Should insights flow on `insights.>` NATS subjects so the UI can render them live? Or is REST polling fine for v0.1?

This sketch is a starting point. The next step is probably to prototype the LLM call against one real session's `turn.sentence` patterns and see what the output actually looks like before committing to the schema.
