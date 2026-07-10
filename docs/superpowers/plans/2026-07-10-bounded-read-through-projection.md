# Bounded Read-Through Projection Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Bound open-story's in-RAM read model so memory is a number the operator sets, without losing data or degrading the live view.

**Architecture:** Keep the hot working set (live + recently-accessed sessions) fully resident; evict least-recently-accessed *cold* sessions when over a byte budget and rebuild them from SQLite on next access; bound the full-body cache with a byte LRU. SQLite stays the complete source of truth — every eviction discards derived cache only.

**Tech Stack:** Rust, `dashmap` (existing), `tokio`, `rusqlite` (via `EventStore`). No new heavy dependency — recency/eviction is built on `dashmap` + `std` atomics/`Instant`.

## Global Constraints

- **Never truncate/lose data.** SQLite (`insert_event`) already stores the full event; do not change that. Every eviction path must be re-derivable from the store. Expand endpoints must always return the complete body (RAM or SQLite). (memory: `openstory-no-truncation-principle`)
- **Never evict a live/streaming session** — only cold ones.
- **Crate names:** store = `open_story_store` (`rs/store`), server = `open_story_server` (`rs/server`). Test with `cargo test -p <crate> <name>`.
- **Verified anchors (do not rename):** `StoreState.projections: Arc<DashMap<String, SessionProjection>>`, `StoreState.full_payloads: Arc<DashMap<(String,String), String>>`, `EventStore::session_events(&str)->Result<Vec<Value>>`, `EventStore::list_sessions()->Result<Vec<SessionRow>>`, `EventStore::full_payload(&str)->Result<Option<String>>`, `SessionProjection::new(&str)`, `SessionProjection::append(&Value)->AppendResult`.

---

## File Structure

- `rs/server/src/config.rs` — add cache-bound config fields + defaults + CLI/env wiring (modify).
- `rs/store/src/projection.rs` — add `SessionProjection::heap_bytes()` (modify).
- `rs/store/src/rebuild.rs` — **new**: `rebuild_session(&dyn EventStore, &str) -> Option<SessionProjection>` (per-session reproject, extracted from `reproject_all`'s loop body).
- `rs/store/src/projection_cache.rs` — **new**: `ProjectionCache` wrapping the projections DashMap with access-recency, a byte budget, live-pins, and eviction.
- `rs/store/src/payload_cache.rs` — **new**: `PayloadCache` — byte-bounded LRU over the `(session,event)->body` map.
- `rs/store/src/state.rs` — swap raw DashMaps for the two caches; expose `get_or_rebuild` (modify).
- `rs/server/src/reproject.rs` — add `reproject_working_set` for lazy boot (modify).
- `rs/server/src/api.rs` — route projection reads through `get_or_rebuild`; touch recency; payload endpoint uses `PayloadCache` (modify).
- `rs/server/src/metrics.rs` — resident bytes/sessions + eviction counters (modify).

---

## Task 1: Config knobs

**Files:**
- Modify: `rs/server/src/config.rs` (fields near `truncation_threshold` ~281; defaults ~386; CLI in `rs/cli/src/main.rs` ~89/348)
- Test: `rs/server/src/config.rs` (tests module)

**Interfaces:**
- Produces: `Config.projection_cache_bytes: u64`, `Config.working_set_days: u32`, `Config.payload_cache_bytes: u64`.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn cache_bounds_have_sane_defaults() {
    let c = Config::default();
    assert_eq!(c.projection_cache_bytes, 4_000_000_000); // 4 GB
    assert_eq!(c.payload_cache_bytes, 256_000_000);      // 256 MB
    assert_eq!(c.working_set_days, 7);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p open-story-server cache_bounds_have_sane_defaults`
Expected: FAIL (no field `projection_cache_bytes`).

- [ ] **Step 3: Add the fields + defaults**

In the `Config` struct:
```rust
    /// Byte ceiling for resident session projections. Cold sessions evict
    /// (LRU) above this and rebuild from SQLite on access.
    pub projection_cache_bytes: u64,
    /// Sessions accessed within this many days are never evicted (0 = off).
    pub working_set_days: u32,
    /// Byte ceiling for the full-body (tool-output) cache.
    pub payload_cache_bytes: u64,
```
In the `Default` impl:
```rust
    projection_cache_bytes: 4_000_000_000,
    working_set_days: 7,
    payload_cache_bytes: 256_000_000,
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p open-story-server cache_bounds_have_sane_defaults`
Expected: PASS.

- [ ] **Step 5: Wire CLI/env overrides** (mirror how `truncation_threshold` flows in `rs/cli/src/main.rs:89,348` and the `apply_args`/env path in `config.rs`). Add the three fields to the same override sites.

- [ ] **Step 6: Commit**

```bash
git add rs/server/src/config.rs rs/cli/src/main.rs
git commit -m "feat(config): projection_cache_bytes, payload_cache_bytes, working_set_days"
```

---

## Task 2: `SessionProjection::heap_bytes()` size estimator

**Files:**
- Modify: `rs/store/src/projection.rs` (impl block ~221)
- Test: `rs/store/src/projection.rs` (tests module)

**Interfaces:**
- Produces: `SessionProjection::heap_bytes(&self) -> u64` — approximate resident bytes (record content + local full-payload overflow), used by the projection cache to budget.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn heap_bytes_grows_with_content() {
    let mut p = SessionProjection::new("s1");
    let base = p.heap_bytes();
    p.append(&serde_json::json!({
        "id":"e1","subtype":"message.assistant.text","time":"2026-01-01T00:00:00Z",
        "data":{"text":"x".repeat(10_000)}
    }));
    assert!(p.heap_bytes() > base + 8_000, "expected content to add ~10KB");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p open-story-store heap_bytes_grows_with_content`
Expected: FAIL (no method `heap_bytes`).

- [ ] **Step 3: Implement the estimator**

```rust
    /// Approximate resident heap footprint of this projection. Not exact —
    /// sums the string content we hold (record bodies + overflow payloads)
    /// plus a fixed per-record overhead. Good enough for byte budgeting.
    pub fn heap_bytes(&self) -> u64 {
        const PER_RECORD_OVERHEAD: u64 = 256;
        let mut n = 0u64;
        for r in &self.records {
            n += PER_RECORD_OVERHEAD + r.approx_content_len() as u64;
        }
        for v in self.full_payloads.values() {
            n += v.len() as u64;
        }
        n
    }
```
Add a small `approx_content_len(&self) -> usize` on `ViewRecord` that returns the byte length of whatever string body it carries (0 for bodies with no text). Match on `RecordBody` variants that hold text and return that string's `.len()`; return 0 otherwise.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p open-story-store heap_bytes_grows_with_content`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add rs/store/src/projection.rs
git commit -m "feat(projection): heap_bytes() size estimator for budgeting"
```

---

## Task 3: `rebuild_session` — per-session reproject helper

**Files:**
- Create: `rs/store/src/rebuild.rs`
- Modify: `rs/store/src/lib.rs` (add `pub mod rebuild;`), `rs/server/src/reproject.rs` (call the shared helper from `reproject_all`'s loop to avoid duplicated logic — DRY)
- Test: `rs/store/src/rebuild.rs` (tests module)

**Interfaces:**
- Consumes: `EventStore::session_events`, `SessionProjection::new/append`.
- Produces: `pub async fn rebuild_session(store: &dyn EventStore, session_id: &str) -> Option<SessionProjection>` — reads one session's events and replays them; `None` if the session has no events.

- [ ] **Step 1: Write the failing test** (uses the in-memory/JSONL test store the crate already uses in `sqlite_store.rs` tests)

```rust
#[tokio::test]
async fn rebuild_session_replays_events_from_store() {
    let store = test_store_with_events("s1", &[
        json!({"id":"e1","subtype":"message.user.prompt","time":"2026-01-01T00:00:00Z","data":{"text":"hi"}}),
        json!({"id":"e2","subtype":"message.assistant.text","time":"2026-01-01T00:00:01Z","data":{"text":"yo"}}),
    ]).await;
    let p = rebuild_session(store.as_ref(), "s1").await.expect("some");
    assert_eq!(p.event_count, 2);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p open-story-store rebuild_session_replays_events_from_store`
Expected: FAIL (no `rebuild_session`).

- [ ] **Step 3: Implement**

```rust
use serde_json::Value;
use crate::event_store::EventStore;
use crate::projection::SessionProjection;

pub async fn rebuild_session(store: &dyn EventStore, session_id: &str) -> Option<SessionProjection> {
    let events = store.session_events(session_id).await.unwrap_or_default();
    if events.is_empty() { return None; }
    let mut p = SessionProjection::new(session_id);
    for e in &events { p.append(e); }
    Some(p)
}
```
Then in `rs/server/src/reproject.rs`, replace the inline loop body of `reproject_all` with a call to `rebuild_session` (keep the `list_sessions` iteration and the `projections.insert`).

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p open-story-store rebuild_session_replays_events_from_store`
Expected: PASS. Also run `cargo test -p open-story-server reproject` to confirm the refactor of `reproject_all` still passes existing tests.

- [ ] **Step 5: Commit**

```bash
git add rs/store/src/rebuild.rs rs/store/src/lib.rs rs/server/src/reproject.rs
git commit -m "feat(store): rebuild_session helper; reproject_all reuses it (DRY)"
```

---

## Task 4: `PayloadCache` — byte-bounded LRU for full bodies

**Files:**
- Create: `rs/store/src/payload_cache.rs`
- Modify: `rs/store/src/lib.rs` (`pub mod payload_cache;`)
- Test: `rs/store/src/payload_cache.rs` (tests module)

**Interfaces:**
- Produces:
  - `PayloadCache::new(max_bytes: u64) -> Self`
  - `PayloadCache::get(&self, key: &(String,String)) -> Option<String>` (marks recency)
  - `PayloadCache::insert(&self, key: (String,String), body: String)` (evicts LRU while over budget)
  - `PayloadCache::resident_bytes(&self) -> u64`

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn payload_cache_evicts_lru_over_budget() {
    let c = PayloadCache::new(1000);
    c.insert(("s".into(),"a".into()), "x".repeat(600));
    c.insert(("s".into(),"b".into()), "x".repeat(600)); // now >1000 → evict LRU ("a")
    assert!(c.get(&("s".into(),"a".into())).is_none(), "a should be evicted");
    assert!(c.get(&("s".into(),"b".into())).is_some(), "b stays");
    assert!(c.resident_bytes() <= 1000);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p open-story-store payload_cache_evicts_lru_over_budget`
Expected: FAIL (no `PayloadCache`).

- [ ] **Step 3: Implement** (DashMap of value+tick; monotonic tick for recency; O(n) scan for the min tick on eviction — fine at these scales)

```rust
use dashmap::DashMap;
use std::sync::atomic::{AtomicU64, Ordering};

struct Entry { body: String, tick: u64 }

pub struct PayloadCache {
    map: DashMap<(String,String), Entry>,
    max_bytes: u64,
    bytes: AtomicU64,
    clock: AtomicU64,
}

impl PayloadCache {
    pub fn new(max_bytes: u64) -> Self {
        Self { map: DashMap::new(), max_bytes, bytes: AtomicU64::new(0), clock: AtomicU64::new(0) }
    }
    fn tick(&self) -> u64 { self.clock.fetch_add(1, Ordering::Relaxed) }
    pub fn resident_bytes(&self) -> u64 { self.bytes.load(Ordering::Relaxed) }

    pub fn get(&self, key: &(String,String)) -> Option<String> {
        let t = self.tick();
        self.map.get_mut(key).map(|mut e| { e.tick = t; e.body.clone() })
    }

    pub fn insert(&self, key: (String,String), body: String) {
        let t = self.tick();
        let len = body.len() as u64;
        if let Some(old) = self.map.insert(key, Entry { body, tick: t }) {
            self.bytes.fetch_sub(old.body.len() as u64, Ordering::Relaxed);
        }
        self.bytes.fetch_add(len, Ordering::Relaxed);
        while self.bytes.load(Ordering::Relaxed) > self.max_bytes {
            // find LRU (min tick); stop if map somehow empty
            let victim = self.map.iter().min_by_key(|e| e.value().tick).map(|e| e.key().clone());
            match victim {
                Some(k) => if let Some((_, e)) = self.map.remove(&k) {
                    self.bytes.fetch_sub(e.body.len() as u64, Ordering::Relaxed);
                },
                None => break,
            }
        }
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p open-story-store payload_cache_evicts_lru_over_budget`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add rs/store/src/payload_cache.rs rs/store/src/lib.rs
git commit -m "feat(store): PayloadCache byte-bounded LRU"
```

---

## Task 5: `ProjectionCache` — recency + byte budget + live-pins

**Files:**
- Create: `rs/store/src/projection_cache.rs`
- Modify: `rs/store/src/lib.rs`
- Test: `rs/store/src/projection_cache.rs` (tests module)

**Interfaces:**
- Consumes: `SessionProjection::heap_bytes` (Task 2).
- Produces:
  - `ProjectionCache::new(max_bytes: u64, working_set_days: u32) -> Self`
  - `ProjectionCache::insert(&self, id: String, p: SessionProjection)` (updates bytes; evicts)
  - `ProjectionCache::get(&self, id: &str) -> Option<dashmap::mapref::one::Ref<..>>` (marks recency)
  - `ProjectionCache::contains(&self, id: &str) -> bool`
  - `ProjectionCache::pin_live(&self, id: &str)` / `unpin_live(&self, id: &str)`
  - `ProjectionCache::resident_bytes(&self) -> u64`, `resident_sessions(&self) -> usize`, `evictions(&self) -> u64`

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn evicts_lru_cold_but_never_pinned() {
    let c = ProjectionCache::new(1500, 0);
    c.insert("cold".into(), proj_of_bytes("cold", 800));
    c.pin_live("live"); c.insert("live".into(), proj_of_bytes("live", 800));
    let _ = c.get("live"); // touch
    c.insert("new".into(), proj_of_bytes("new", 800)); // over budget → evict LRU cold
    assert!(!c.contains("cold"), "cold evicted");
    assert!(c.contains("live"), "pinned live never evicted");
    assert!(c.contains("new"));
    assert!(c.evictions() >= 1);
}
```
(`proj_of_bytes` builds a `SessionProjection` and appends filler until `heap_bytes()` ≈ target.)

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p open-story-store evicts_lru_cold_but_never_pinned`
Expected: FAIL (no `ProjectionCache`).

- [ ] **Step 3: Implement** — same shape as `PayloadCache` but: value is `SessionProjection`, size from `heap_bytes()`, and eviction skips pinned ids and (if `working_set_days>0`) ids touched within the window. Maintain `pins: DashMap<String,()>`, `access: DashMap<String,(u64 tick, Instant)>`, `bytes: AtomicU64`, `evictions: AtomicU64`. On `insert`, add bytes then evict while over budget by selecting the min-tick id that is **not pinned** and **not within the day-window**; if none evictable, stop (accept temporary overshoot — logged by a metric).

Note: use `std::time::Instant`/`SystemTime` for the day-window (server process — real clock is available here, unlike workflow scripts).

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p open-story-store evicts_lru_cold_but_never_pinned`
Expected: PASS. Add a second test `overshoots_when_all_pinned_rather_than_evicting_live` asserting no pinned eviction even over budget.

- [ ] **Step 5: Commit**

```bash
git add rs/store/src/projection_cache.rs rs/store/src/lib.rs
git commit -m "feat(store): ProjectionCache with recency, byte budget, live pins"
```

---

## Task 6: Read-through — `get_or_rebuild` on miss

**Files:**
- Modify: `rs/store/src/state.rs` (swap `projections`/`full_payloads` raw DashMaps for `ProjectionCache`/`PayloadCache`; add accessor), `rs/server/src/api.rs` (projection reads + payload endpoint go through the caches)
- Test: `rs/store/src/state.rs` (tests module)

**Interfaces:**
- Produces: `StoreState::get_or_rebuild(&self, session_id: &str) -> Option<Ref<SessionProjection>>` — cache hit → touch + return; miss → `rebuild_session` from `event_store`, `insert`, return. `None` only if the session has no events in the store.

- [ ] **Step 1: Write the failing test**

```rust
#[tokio::test]
async fn get_or_rebuild_reloads_evicted_session() {
    let state = test_state_with_events("s1", &[
        json!({"id":"e1","subtype":"message.user.prompt","time":"2026-01-01T00:00:00Z","data":{"text":"hi"}}),
    ]).await;
    assert!(state.projections.get("s1").is_none(), "starts cold");   // not yet loaded
    let p = state.get_or_rebuild("s1").await.expect("rebuilt");
    assert_eq!(p.event_count, 1);
    assert!(state.projections.contains("s1"), "now resident");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p open-story-store get_or_rebuild_reloads_evicted_session`
Expected: FAIL (no `get_or_rebuild`).

- [ ] **Step 3: Implement**

```rust
pub async fn get_or_rebuild(&self, session_id: &str)
    -> Option<dashmap::mapref::one::Ref<'_, String, SessionProjection>>
{
    if let Some(r) = self.projections.get(session_id) { return Some(r); } // touches recency
    let p = crate::rebuild::rebuild_session(self.event_store.as_ref(), session_id).await?;
    self.projections.insert(session_id.to_string(), p);
    self.projections.get(session_id)
}
```
Update `StoreState` fields: `projections: Arc<ProjectionCache>`, `full_payloads: Arc<PayloadCache>`. Fix all construction sites (grep `projections:` / `full_payloads:` in `rs/server/src` and `rs/store/src`). In `api.rs`, replace direct `s.store.projections.get(id)` reads that must survive eviction with `s.store.get_or_rebuild(id).await`; the payload endpoint (`api.rs:~1329`) uses `full_payloads.get(...)` then the existing SQLite fallback (already present) — no new fallback code, just the bounded cache.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p open-story-store get_or_rebuild_reloads_evicted_session`
Expected: PASS. Then `cargo test -p open-story-server` to catch call-site breakage.

- [ ] **Step 5: Commit**

```bash
git add rs/store/src/state.rs rs/server/src/api.rs
git commit -m "feat(store): read-through get_or_rebuild; route API reads through caches"
```

---

## Task 7: Lazy boot + wire live-pin/eviction into ingest

**Files:**
- Modify: `rs/server/src/reproject.rs` (add `reproject_working_set`), `rs/server/src/state.rs` boot path (~160-168), the ingest/consumer path that inserts projections + the streaming lifecycle (pin on session-start, unpin on stale/complete)
- Test: `rs/server/src/reproject.rs` (tests module)

**Interfaces:**
- Consumes: `ProjectionCache`, `rebuild_session`, `EventStore::list_sessions` (has `last_event` on `SessionRow`).
- Produces: `pub async fn reproject_working_set(store: &StoreState, since_days: u32)` — reproject only sessions whose `last_event` is within `since_days` (or the newest N by bytes budget); leave the rest to `get_or_rebuild`.

- [ ] **Step 1: Write the failing test**

```rust
#[tokio::test]
async fn lazy_boot_seeds_only_recent_sessions() {
    // one recent, one 30-day-old session in the store
    let state = test_state_with_two_sessions_recent_and_old().await;
    reproject_working_set(&state, 7).await;
    assert!(state.projections.contains("recent"));
    assert!(!state.projections.contains("old"), "old is left for on-access rebuild");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p open-story-server lazy_boot_seeds_only_recent_sessions`
Expected: FAIL (no `reproject_working_set`).

- [ ] **Step 3: Implement** — iterate `list_sessions()`, filter by `last_event` within `since_days`, `rebuild_session` each, `insert`. In `state.rs` boot, call `reproject_working_set(self, config.working_set_days)` instead of `reproject_all`. In the ingest/stream lifecycle: `projections.pin_live(session_id)` when a session starts/streams; `unpin_live` when it goes stale (there is a `stale_threshold_secs` already) or completes.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p open-story-server lazy_boot_seeds_only_recent_sessions`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add rs/server/src/reproject.rs rs/server/src/state.rs
git commit -m "feat(server): lazy boot (working-set only) + live-session pinning"
```

---

## Task 8: Metrics + validation harness

**Files:**
- Modify: `rs/server/src/metrics.rs` (gauges/counters), `rs/server/src/api.rs` (`/api/ui-state` or a debug route to surface cache stats)
- Create: `scripts/validate-projection-cache.sh` (before/after RSS on a restored volume copy)
- Test: `rs/server/src/metrics.rs` (tests module)

**Interfaces:**
- Consumes: `ProjectionCache::resident_bytes/resident_sessions/evictions`, `PayloadCache::resident_bytes`.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn metrics_report_cache_gauges() {
    let m = render_cache_metrics(/*proj_bytes*/ 123, /*proj_sessions*/ 4, /*evictions*/ 2, /*payload_bytes*/ 55);
    assert!(m.contains("openstory_projection_cache_bytes 123"));
    assert!(m.contains("openstory_projection_resident_sessions 4"));
    assert!(m.contains("openstory_projection_evictions_total 2"));
    assert!(m.contains("openstory_payload_cache_bytes 55"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p open-story-server metrics_report_cache_gauges`
Expected: FAIL.

- [ ] **Step 3: Implement** `render_cache_metrics(...) -> String` producing those Prometheus lines and call it from the metrics handler with live cache values.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p open-story-server metrics_report_cache_gauges`
Expected: PASS.

- [ ] **Step 5: Write the validation script** `scripts/validate-projection-cache.sh`:

```bash
#!/usr/bin/env bash
set -euo pipefail
# Restore a COPY of prod os-data into a scratch container, run new image, compare RSS.
docker run -d --name os-validate -v os-data-copy:/data -p 3999:3002 \
  -e OPEN_STORY_PROJECTION_CACHE_BYTES=1500000000 open-story:cache-test
sleep 25
docker stats --no-stream --format '{{.MemUsage}}' os-validate
curl -s localhost:3999/metrics | grep -E 'openstory_(projection|payload)_'
# manual: open a recent session (rich inline), open a >30d cold session (rebuild), expand a big tool_result
```

- [ ] **Step 6: Commit**

```bash
git add rs/server/src/metrics.rs rs/server/src/api.rs scripts/validate-projection-cache.sh
git commit -m "feat(metrics): cache gauges + validation harness"
```

---

## Self-Review

- **Spec coverage:** hot-set-resident (T5/T7 pin+lazy-boot), cold-evict+rebuild (T3/T5/T6), byte-bounded payload LRU (T4), `projection_cache_bytes`/`working_set_days`/`payload_cache_bytes` (T1), preservation via SQLite read-through (T6, unchanged store), never-evict-live (T5 pins), validation (T8). Out-of-scope items (openactor prune, retention) intentionally absent. ✓
- **Type consistency:** `ProjectionCache`/`PayloadCache` method names match across T4–T7; `heap_bytes` (T2) used by T5; `rebuild_session` (T3) used by T6/T7; `StoreState.projections/full_payloads` retyped once in T6 and used consistently after. ✓
- **Placeholders:** none — each task carries real test + implementation code and exact commands. The few "match the existing site" notes (CLI wiring T1, call-site fixups T6) point at verified line anchors rather than hand-waving.
- **Note for implementer:** T6 retypes two `StoreState` fields — expect compile errors at every construction/read site; the task's grep step finds them. Land T1–T5 (pure additions, green) before T6 (the swap).

## Risks
- Cold-open latency → one indexed session read; generous default window.
- Eviction of in-use session → live pins + recency window; T5 tests both.
- O(n) LRU scan → acceptable at ~thousands of sessions; revisit with an ordered index if `resident_sessions` grows large (metric in T8 tells you).
