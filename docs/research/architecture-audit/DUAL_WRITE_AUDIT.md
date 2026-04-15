# Dual-Write Audit — `ingest_events` vs. the Actor Consumers

Triggered by the JSONL torn-line bug. If the `session_store.append` dual-write slipped through the actor decomposition, what else did?

This doc enumerates every side effect in `rs/server/src/ingest.rs::ingest_events` and maps it to which actor (if any) also performs it. Status column: **safe** (idempotent), **bug** (non-idempotent, breaks something), **dead** (parallel state nobody reads), **single** (no dual-write — this function is the only owner).

## The table

| # | Side effect | Line (ingest.rs) | Also done by | Idempotent? | Status |
|---|-------------|------------------|--------------|-------------|--------|
| 1 | `event_store.insert_event` | 117–122 | Actor 1 (persist) | ✓ PK collision → `Ok(false)` | **safe-but-wasteful** |
| 2 | `session_store.append` | (removed 2026-04-15) | Actor 1 | ✗ file append | **FIXED** |
| 3 | `projections.entry().append()` (`state.store.projections`) | 134–140 | Actor 3 (projections) runs in parallel on *its own* `SessionProjection` | n/a — different state | **dead parallel state** (T5) |
| 4 | `plan_store.save` + `event_store.upsert_plan` | 142–159 | nobody | n/a | **single** |
| 5 | `full_payloads` cache (truncated ToolResults) | 164–178 | Actor 4's `BroadcastConsumer` (`consumers/broadcast.rs`) has its own `HashMap<String, HashMap<String, String>>` but **is not wired in** | n/a | **dead parallel state** |
| 6 | `event_store.index_fts` | 202–210 | Actor 1 (persist) | ✓ `INSERT OR IGNORE` on FTS5 | **safe-but-wasteful** |
| 7 | build `BroadcastMessage::Enriched` + send to `broadcast_tx` | 236–273 | nobody (Actor 4 spawn at `rs/src/server/mod.rs:246` *calls* `ingest_events`; `BroadcastConsumer::process_events` defined but dormant) | n/a | **single** |
| 8 | metrics counters | 277–289 | nobody | idempotent counters | **single** |
| 9 | subagent parent/children maps | 98–109 | nobody | n/a | **single** |

## The findings, in plain language

**Four actual dual-writes. One was the bug. Three are safe but wasteful.**

- **FTS (row 6)** — both `ingest_events` and `PersistConsumer::process_batch` call `event_store.index_fts` for the same events. FTS5 uses INSERT OR IGNORE so there's no data corruption, just 2× DB writes per event. Small cost; cleanup target.

- **insert_event (row 1)** — same story, PK collision makes the second write a no-op. 2× DB touches per event. Cleanup target.

- **session_store.append (row 2)** — was the bug. Fixed.

**Two dead parallel states.**

- **Actor 3's projection (row 3)** — `ProjectionsConsumer` subscribes to NATS and builds a `SessionProjection` in its own task. Nothing reads it. All downstream reads (`to_wire_record`, label/branch lookup, session metadata) go through `state.store.projections`, which is mutated by `ingest_events`. Actor 3 is pure dead work today.

- **BroadcastConsumer's `full_payloads` (row 5)** — `consumers/broadcast.rs` defines `BroadcastConsumer::process_events` that captures truncated ToolResults into its own `HashMap`. The actor struct is instantiated and used in unit tests (`BroadcastConsumer::new()`) but **never spawned in the live server** — the Actor 4 wiring at `rs/src/server/mod.rs:246` calls `ingest_events` directly on shared AppState. So the `BroadcastConsumer` struct exists as dormant code.

**Five side effects are single-owner.** No concern.

## Risk profile of the two remaining dual-writes

Row 1 (insert_event) and row 6 (index_fts) are currently safe because SQLite/FTS5 atomicity + idempotent SQL keeps them from corrupting data. But they:

- Waste ~2× DB roundtrips per ingested event
- Hide the intent of the actor contract (reviewers see `ingest_events` doing persistence work it shouldn't own)
- Mean the `PersistConsumer` isn't yet the sole source of truth for persisted data — if the consumer fails or is removed, `ingest_events` still writes. That makes the consumer impossible to isolate-test cleanly.

**Recommendation:** remove both from `ingest_events` once Actor 4 is fully decomposed. The decomposition entry in BACKLOG already scopes the broader work; these two deletions are stepping stones along that path, not separate work items.

## Test coverage we're adding

One test per row, asserting current behavior. When the decomposition lands, these tests flip from "ingest_events does X" to "ingest_events does NOT do X anymore" — and the diff of the test file tells the story of what moved where.

See `rs/server/src/ingest.rs::tests` for the updated + new tests.
