# Actor 4 Decomposition — Execution Plan

Prerequisite: every test in the pre-decomposition regression net (see
[`DUAL_WRITE_AUDIT.md`](./DUAL_WRITE_AUDIT.md)) is green on the starting
commit. This plan articulates how to land the decomposition without
breaking any of those tests — every flip from green to red must be
deliberate and explained in the PR.

---

## End state

- **Actor 1 (persist)** — single owner of `event_store.insert_event`,
  `session_store.append`, and FTS indexing
- **Actor 2 (patterns)** — single owner of pattern detection (already
  true, no change)
- **Actor 3 (projections)** — single owner of `SessionProjection`
  state; writes to `state.store.projections` for downstream reads
- **Actor 4 (broadcast)** — uses `BroadcastConsumer` (the struct
  that's currently dormant), owns wire-record assembly, reads
  projections, sends `BroadcastMessage` to `broadcast_tx`
- `ingest_events` — **deleted**. All responsibilities moved to the
  owning actor. Replay path rewritten to publish to the bus instead.

---

## Blast radius

**Production files (9):**
1. `rs/src/server/mod.rs` — Actor 3/4 spawn wiring
2. `rs/server/src/ingest.rs` — delete `ingest_events`, delete tests that depend on it
3. `rs/server/src/consumers/projections.rs` — accept shared projection map
4. `rs/server/src/consumers/broadcast.rs` — wire in, add projection read, add full_payloads cache owner
5. `rs/server/src/consumers/persist.rs` — no-change (already correct)
6. `rs/server/src/api.rs` — projections access path (RwLock vs direct)
7. `rs/server/src/state.rs` — `projections` field type change
8. `rs/server/src/ws.rs` — read-side adjustment
9. `rs/store/src/state.rs` — `StoreState.projections` field type

**Test files (15+):** every test that calls `ingest_events` directly
must be rewritten to either spin up the NATS bus + actors, or to
invoke `PersistConsumer::process_batch` directly, or to be deleted if
its behavior is subsumed.

Files affected:
`rs/tests/test_ingest.rs`, `rs/tests/test_dedup.rs`,
`rs/tests/test_broadcast.rs`, `rs/tests/test_ws.rs`,
`rs/tests/test_api.rs`, `rs/tests/test_records_api.rs`,
`rs/tests/test_view_api.rs`, `rs/tests/test_wire_record.rs`,
`rs/tests/test_projection.rs`, `rs/tests/test_projection_e2e.rs`,
`rs/tests/test_sqlite_e2e.rs`, `rs/tests/test_security.rs`,
`rs/tests/test_compose.rs`, `rs/tests/test_compose_perf.rs`,
`rs/server/src/ingest.rs` (inline tests).

---

## Execution order (four commits, roughly)

### Commit 1 — Shared projection map

Change `StoreState.projections` from `HashMap<String, SessionProjection>`
to something concurrent. Two options:

- **`DashMap<String, SessionProjection>`** — lock-free concurrent HashMap. No explicit locking at call sites.
- **`Arc<RwLock<HashMap<String, SessionProjection>>>`** — explicit read/write locks.

**Recommendation: `DashMap`.** All call sites already look like
`projections.get(sid)` / `projections.entry(sid)` / etc. `DashMap` is
a drop-in with the same API. Avoids sprinkling `.read().await` across
9 files.

This commit is pure infrastructure — no behavior change. All existing
tests should pass unchanged after the type swap.

### Commit 2 — Wire Actor 3 live

Actor 3 currently writes to its own internal HashMap. Change:
- `ProjectionsConsumer::new()` → `ProjectionsConsumer::new(projections: Arc<DashMap<...>>)`
- In the spawn at `mod.rs:228`, pass `state.store.projections.clone()`
- `process_batch` writes into the shared map

Remove the dead-code characterization tests in `consumers/projections.rs`
— they were documenting the dead state, which is no longer dead.

At this commit, projections are still *also* written by `ingest_events`
(via the existing call site). Two writers on the same map. The only
safety net against interleaving is `SessionProjection::seen_ids`
dedup (which we verified in an earlier audit test). Acceptable
intermediate state — next commit removes the redundant writer.

### Commit 3 — Wire Actor 4 via BroadcastConsumer, remove ingest_events

The big one. Changes:

1. Activate `BroadcastConsumer` — the struct exists in
   `consumers/broadcast.rs`. Extend it to:
   - Accept `Arc<DashMap<...>>` for projections (read-only)
   - Accept `broadcast::Sender<BroadcastMessage>` for the outbound channel
   - Own its own `full_payloads` cache (already in the struct)
   - Build `BroadcastMessage::Enriched` with wire records
   - Handle the session metadata lookup (labels, token totals) via the
     shared projections

2. Swap `mod.rs:240-270` — the current Actor 4 spawn calls
   `ingest_events`. Replace with `BroadcastConsumer::process_batch` +
   send to `broadcast_tx`.

3. Delete `ingest_events` from `rs/server/src/ingest.rs`.

4. Update `replay_boot_sessions` — it currently calls `ingest_events`
   during boot. Options:
   - Refactor to publish batches to the bus (NATS) and let the actors
     consume them naturally. This is the *right* shape but requires
     the bus to be active during replay.
   - Inline the necessary operations (PersistConsumer::process_batch +
     ProjectionsConsumer::process_batch). Simpler, less Right.
   - **Recommendation:** publish to bus. Matches the production flow.

5. Rewrite the 15+ test files. Each test that did
   `ingest_events(&mut state, sid, events)` now does either:
   - Spin up a NoopBus-backed test rig that exercises all actors
   - Directly call the relevant actor's `process_batch`

   The pre-decomposition regression tests in `rs/server/src/ingest.rs`
   are the hardest case — they assert on `result.changes` which came
   from `ingest_events`. Each of them needs to move to the broadcast
   consumer's output.

### Commit 4 — Remove dual-writes + cleanup

With Actor 4 no longer calling `ingest_events`, the two remaining
dual-writes (`event_store.insert_event`, `event_store.index_fts`) go
away automatically — `ingest_events` is deleted, so the calls are
deleted with it.

The audit tests that flag the dual-writes flip:
- `ingest_still_indexes_fts_for_durable_events_dual_write_with_actor_1`
  — deleted (asserts on deleted function)
- `ingest_persists_to_event_store_but_not_session_store` — deleted
- `ingest_populates_full_payloads_cache_for_truncated_tool_results` —
  replaced with `broadcast_consumer_populates_full_payloads_cache`

Final verification:
- `cargo test` — all tests green
- Capstone test (`test_jsonl_escape_hatch`) — run against a fresh
  session started under the new wiring; zero invalid lines expected.
  If historical torn lines still appear, they're from before the
  2026-04-15 fix, not from this work.
- Dogfood (`test_cloud_event_dogfood`, `test_view_record_dogfood`) —
  run against running OpenStory; all events validate.

---

## Risk assessment

**Highest-risk section**: the `replay_boot_sessions` rewrite. Boot
replay runs sequentially against stored JSONL; publishing to the bus
changes that timing. If the bus isn't fully drained before the server
starts accepting requests, early queries will see partial state.

Mitigation: add a "replay complete" barrier — `bus.flush().await` or
a per-actor "caught up to seq N" signal — before opening the HTTP
listener.

**Second-highest**: `full_payloads` ownership. The REST endpoint at
`/api/sessions/:sid/events/:eid/content` currently reads from
`state.store.full_payloads`. Post-decomposition, that cache lives
inside `BroadcastConsumer` — reachable only via the actor, not via
shared state.

Mitigation: either keep `full_payloads` as a shared `Arc<DashMap>` that
both BroadcastConsumer writes and the API reads, or add a message-
passing call (API asks the actor for content, with a timeout). The
first is simpler; the second is more honest about actor boundaries.
Recommended: shared `Arc<DashMap>` for now, promote to message-
passing later if ownership clarity demands it.

**Lowest-risk**: the metric counters (`record_events_ingested`,
`record_events_deduped`). These can live in `PersistConsumer` —
it's the canonical per-event entry point.

---

## Verification checklist

Before merging the decomposition:

- [ ] All ~200 server + store unit tests green
- [ ] T2 container test (pi-mono parallel tool calls) passes
- [ ] Dogfood tests pass against running OpenStory (CloudEvent, ViewRecord, WireRecord)
- [ ] JSONL capstone reports zero new torn lines (historical stays historical)
- [ ] All 5 pre-decomposition regression tests (session label, token totals, seq ordering, full_payload, two-subscriber) pass after rewrite
- [ ] All 3 PersistConsumer ownership tests pass
- [ ] `ingest_events` deleted, no grep hits for it in production code
- [ ] `rs/src/server/mod.rs:240` comment "This is the last consumer to decompose" removed
- [ ] BACKLOG entry "Decompose Actor 4" removed

---

## What this plan doesn't cover

- **Decomposing Actor 2** (patterns) further — it's already independent but still uses internal state that could be more explicit. Not in scope.
- **Pattern forwarding over NATS** — the BACKLOG entry "stream architecture rewrite" covers it separately. BroadcastConsumer could subscribe to `patterns.>` and forward detected patterns into `BroadcastMessage::Enriched.patterns`. Worth including in commit 3 if the scope fits, otherwise a follow-up.
- **UI behavior tests** — we've been Rust-side only. The UI consumes the same WS frames; verifying the UI still renders correctly after decomposition is a Playwright run, not a Rust test. Recommend: manual smoke test + a Playwright baseline before merge.

---

## Size estimate

~300 LOC production diff, ~500 LOC test-file rewrites (a lot of
`ingest_events(&mut state, ...)` → `persist.process_batch(...) +
broadcast.process_batch(...)`). Call it 1–2 days of focused work with
the safety net we've already laid down.

The safety net is the point. Anyone executing this plan stays within
these tests — any flip from green to red is a real regression, not
noise.
