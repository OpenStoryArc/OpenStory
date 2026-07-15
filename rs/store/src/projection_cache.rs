//! ProjectionCache — recency + byte-budget + live-pin cache over
//! `session_id -> SessionProjection`.
//!
//! A read-through cache for per-session projections. The durable events
//! always remain in SQLite; an evicted projection is fully re-derivable via
//! `rebuild::rebuild_session`. This cache never deletes from the store — it
//! only holds an in-memory, best-effort copy bounded by a byte budget.
//!
//! Same byte-accounting shape as `PayloadCache`, plus two eviction rules the
//! payload cache doesn't have:
//!   1. a **pinned** session (live / streaming) is never evicted;
//!   2. if `working_set_days > 0`, a session touched within that window is
//!      never evicted.
//!
//! When *every* over-budget candidate is protected (all pinned, or all inside
//! the working-set window), the cache **overshoots** its budget rather than
//! dropping a live/hot projection. That's correct: the budget is a target,
//! not a hard cap, and a pinned session is being actively streamed to.
//!
//! Built on `DashMap` (sharded, lock-per-shard) plus a few atomics for the
//! byte budget, a monotonic recency clock, and an eviction counter. Recency is
//! tracked in a side `access` map (`id -> (tick, Instant)`) so `get` can hand
//! back a bare `Ref<'_, String, SessionProjection>` into the value map. Under
//! concurrent access the LRU ordering is best-effort (an entry's `tick` can be
//! bumped by a `get()` a moment after it was already chosen as the eviction
//! victim) — an accepted looseness for an approximate cache, not a correctness
//! bug: it never double-frees, never double-counts bytes, and always
//! terminates (see `insert`).

use crate::projection::SessionProjection;
use dashmap::mapref::one::Ref;
use dashmap::DashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Recency + byte-budget + live-pin cache of `SessionProjection`s, keyed by
/// session id.
///
/// This is a cache, never a source of truth: everything it holds is also
/// durably persisted (SQLite) and re-derivable via `rebuild_session`. Eviction
/// here never deletes anything from the durable store.
pub struct ProjectionCache {
    /// The projections themselves.
    map: DashMap<String, SessionProjection>,
    /// Recency side-table: id -> (monotonic tick, last-access instant).
    access: DashMap<String, (u64, Instant)>,
    /// Pin **ref-counts** by session id — an id with count > 0 is never
    /// evicted. Ref-counted (not a set) so protections compose: a transient
    /// pin (e.g. `get_or_rebuild` guarding a just-rebuilt entry against its own
    /// insert-time eviction) can nest inside a genuine live pin without the
    /// transient `unpin` clobbering the live one. `pin_live` increments,
    /// `unpin_live` decrements saturating; the entry is removed at 0.
    pins: DashMap<String, u32>,
    max_bytes: u64,
    /// Working-set window: if set, sessions touched within it are not evicted.
    /// `None` when `working_set_days == 0`.
    working_set: Option<Duration>,
    bytes: AtomicU64,
    clock: AtomicU64,
    evictions: AtomicU64,
}

impl ProjectionCache {
    pub fn new(max_bytes: u64, working_set_days: u32) -> Self {
        let working_set = if working_set_days == 0 {
            None
        } else {
            Some(Duration::from_secs(working_set_days as u64 * 86_400))
        };
        Self {
            map: DashMap::new(),
            access: DashMap::new(),
            pins: DashMap::new(),
            max_bytes,
            working_set,
            bytes: AtomicU64::new(0),
            clock: AtomicU64::new(0),
            evictions: AtomicU64::new(0),
        }
    }

    fn tick(&self) -> u64 {
        self.clock.fetch_add(1, Ordering::Relaxed)
    }

    /// Subtract from the resident-byte counter, saturating at 0.
    ///
    /// A plain `fetch_sub` can underflow to a ~2^64 value if a subtract ever
    /// races ahead of its matching add (or double-fires); that would pin
    /// `bytes > max_bytes` forever and make the cache evict everything without
    /// self-healing. Clamping at 0 turns any such mistiming into a transient
    /// *undercount* that corrects itself on the next insert.
    fn sub_bytes(&self, n: u64) {
        let _ = self
            .bytes
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |b| Some(b.saturating_sub(n)));
    }

    pub fn resident_bytes(&self) -> u64 {
        self.bytes.load(Ordering::Relaxed)
    }

    pub fn resident_sessions(&self) -> usize {
        self.map.len()
    }

    pub fn evictions(&self) -> u64 {
        self.evictions.load(Ordering::Relaxed)
    }

    pub fn contains(&self, id: &str) -> bool {
        self.map.contains_key(id)
    }

    /// True when no projection is resident. (Cache-membership only — the
    /// durable store may still hold sessions that were never loaded.)
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// Iterate the resident projections. Best-effort over the cached subset —
    /// evicted sessions are absent (re-derivable via `get_or_rebuild`). Does
    /// not mark recency; iteration is a read over whatever is currently held.
    pub fn iter(&self) -> dashmap::iter::Iter<'_, String, SessionProjection> {
        self.map.iter()
    }

    /// Drop a session's in-memory copy entirely (used when the durable session
    /// is deleted). Re-accounts bytes and clears the recency/pin side-tables so
    /// nothing leaks. This is the one removal that is *not* an eviction — the
    /// caller has removed the durable record too.
    pub fn remove(&self, id: &str) {
        if let Some((_, old)) = self.map.remove(id) {
            self.sub_bytes(old.heap_bytes());
        }
        self.access.remove(id);
        self.pins.remove(id);
    }

    /// Mutate a session's projection in place (creating it if absent), then
    /// re-account the byte delta, refresh recency, and evict to budget. This is
    /// the in-place-growth path used by live ingest / replay, where appending a
    /// single event to a resident projection must keep the byte budget honest
    /// without a full re-`insert`.
    ///
    /// ⚠️ This is SYNC and cannot read the durable store, so for a session that
    /// is NOT already resident it creates a FRESH EMPTY projection and appends
    /// only `f`'s event(s) — losing prior history. Live-append callers must
    /// hydrate cold sessions first: route through
    /// [`crate::state::hydrate_and_append`] / [`crate::state::StoreState::append_hydrated`],
    /// which rebuild from SQLite before delegating here. Call this directly only
    /// when the caller has already guaranteed the projection is complete (e.g.
    /// `reproject_all` inserts a full rebuild).
    ///
    /// Deadlock-safe: the `RefMut` shard-write guard is dropped before
    /// `evict_to_budget` (which iterates the map). The recency/byte side-tables
    /// it touches after are *different* maps/atomics, matching `insert`.
    pub fn append_or_insert<F, R>(&self, id: &str, f: F) -> R
    where
        F: FnOnce(&mut SessionProjection) -> R,
    {
        let (old_bytes, new_bytes, ret) = {
            let mut entry = self
                .map
                .entry(id.to_string())
                .or_insert_with(|| SessionProjection::new(id));
            let old = entry.heap_bytes();
            let ret = f(entry.value_mut());
            let new = entry.heap_bytes();
            (old, new, ret)
        };
        let t = self.tick();
        self.access.insert(id.to_string(), (t, Instant::now()));
        if new_bytes >= old_bytes {
            self.bytes.fetch_add(new_bytes - old_bytes, Ordering::Relaxed);
        } else {
            self.sub_bytes(old_bytes - new_bytes);
        }
        self.evict_to_budget();
        ret
    }

    /// Pin a session as live/streaming — protected from eviction while its pin
    /// count is > 0. Ref-counted: each `pin_live` must be balanced by an
    /// `unpin_live`, and nested pins (transient + live) compose. Safe to call
    /// before the projection is inserted (the pin is consulted by id,
    /// independent of map membership).
    ///
    /// Streaming-lifecycle note (design decision — Option A): this pin is used
    /// only for `get_or_rebuild`'s *transient* protection (guarding a
    /// just-rebuilt entry against its own insert-time eviction). It is
    /// deliberately NOT wired to the ingest/streaming lifecycle to keep a live
    /// session resident, because there is no reliable "session ended / went
    /// stale" signal to balance a pin-on-start (`system.session.end` is
    /// optional, staleness is computed lazily at query time, `stale_threshold_secs`
    /// is unwired) — a pin with no matching unpin would leak and defeat the byte
    /// bound. Instead, an actively-streaming session stays resident via the
    /// recency **working-set window**: each appended event refreshes its
    /// recency, keeping it inside `working_set_days` and off the eviction list,
    /// with protection auto-expiring when the session goes quiet. At
    /// `working_set_days == 0` there is no window, so a live session may be
    /// evicted and losslessly rebuilt from SQLite on next access.
    pub fn pin_live(&self, id: &str) {
        *self.pins.entry(id.to_string()).or_insert(0) += 1;
    }

    /// Release one pin. Saturating: an unbalanced extra `unpin_live` is a
    /// harmless no-op (never underflows). The entry is reaped when its count
    /// reaches 0 — via `remove_if(== 0)` so a concurrent `pin_live` that raced
    /// in after the decrement (bumping it back to 1) is NOT clobbered.
    pub fn unpin_live(&self, id: &str) {
        if let Some(mut e) = self.pins.get_mut(id) {
            *e = e.saturating_sub(1);
        }
        self.pins.remove_if(id, |_, &count| count == 0);
    }

    /// True while the id holds at least one pin.
    fn is_pinned(&self, id: &str) -> bool {
        self.pins.get(id).map(|c| *c > 0).unwrap_or(false)
    }

    /// Look up a cached projection, marking it most-recently-used on a hit.
    pub fn get(&self, id: &str) -> Option<Ref<'_, String, SessionProjection>> {
        let r = self.map.get(id)?;
        let t = self.tick();
        // Refresh recency in the side-table. `get_mut` on a *different* DashMap
        // than the one `r` borrows — no self-deadlock.
        if let Some(mut a) = self.access.get_mut(id) {
            a.0 = t;
            a.1 = Instant::now();
        }
        Some(r)
    }

    /// Insert (or replace) a projection, then evict least-recently-used
    /// entries — never the durable record, just this in-memory copy — while
    /// resident bytes exceed the budget and an evictable (unpinned,
    /// out-of-window) victim exists.
    pub fn insert(&self, id: String, p: SessionProjection) {
        let new_bytes = p.heap_bytes();
        // Count the bytes BEFORE making the entry visible/evictable in the
        // map. If we inserted first, another thread could evict this entry (and
        // `sub_bytes` its size) in the window before we added it — subtracting
        // bytes that were never counted. Adding first closes that window: an
        // entry is never evictable-before-counted.
        self.bytes.fetch_add(new_bytes, Ordering::Relaxed);
        let t = self.tick();
        // Set recency before inserting into the value map so any concurrent
        // evictor that sees the entry also sees a tick for it.
        self.access.insert(id.clone(), (t, Instant::now()));
        if let Some(old) = self.map.insert(id, p) {
            // Overwrite: drop the replaced projection's contribution, so the
            // net delta is (new - old), not accumulated.
            self.sub_bytes(old.heap_bytes());
        }

        self.evict_to_budget();
    }

    /// Evict least-recently-used entries while over budget. Each successful
    /// removal strictly shrinks the map, so this terminates even if `bytes`
    /// were ever inconsistent; if no evictable victim remains (everything is
    /// pinned or inside the working-set window), we stop and accept a
    /// temporary overshoot rather than dropping a live/hot projection.
    fn evict_to_budget(&self) {
        while self.bytes.load(Ordering::Relaxed) > self.max_bytes {
            let now = Instant::now();
            // Pick the min-tick (least-recently-used) id that is NOT pinned and
            // NOT within the working-set window. Iterating `map` locks its
            // shards; the per-key `pins`/`access` lookups touch *other* maps and
            // always in that order (map -> pins/access), matching `get`, so no
            // lock-order cycle. The iterator is fully consumed by `min_by_key`
            // and dropped before any `remove`.
            let victim = self
                .map
                .iter()
                .filter_map(|e| {
                    let id = e.key();
                    if self.is_pinned(id) {
                        return None; // pinned live session — never evict
                    }
                    match self.access.get(id) {
                        Some(a) => {
                            let (tick, last) = *a;
                            if let Some(window) = self.working_set {
                                if now.duration_since(last) < window {
                                    return None; // within working set — protected
                                }
                            }
                            Some((tick, id.clone()))
                        }
                        // No recency record (shouldn't happen — `insert` writes
                        // it first). Treat as oldest/evictable rather than
                        // leaking bytes for an untracked entry.
                        None => Some((0, id.clone())),
                    }
                })
                .min_by_key(|(tick, _)| *tick)
                .map(|(_, id)| id);

            match victim {
                Some(k) => {
                    if let Some((_, old)) = self.map.remove(&k) {
                        self.sub_bytes(old.heap_bytes());
                        self.access.remove(&k);
                        self.evictions.fetch_add(1, Ordering::Relaxed);
                    } else {
                        // Raced: another thread already removed it. Re-loop.
                        continue;
                    }
                }
                None => break, // nothing evictable — accept the overshoot
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    /// Build a well-formed CloudEvent envelope (mirrors the projection module's
    /// own `raw_event` test helper — the EventData shape `from_cloud_event`
    /// expects: seq/session_id/raw at the top, everything else in a claude-code
    /// AgentPayload). The plan's literal JSON is invalid against CloudEvent, so
    /// we go through the real append path here.
    fn raw_event(id: &str, seq: u64, subtype: &str, data: Value) -> Value {
        let mut obj = data.as_object().cloned().unwrap_or_default();
        let raw = obj.remove("raw").unwrap_or(json!({}));
        let mut payload = serde_json::Map::new();
        payload.insert("_variant".to_string(), json!("claude-code"));
        payload.insert("meta".to_string(), json!({"agent": "claude-code"}));
        for (k, v) in obj {
            payload.insert(k, v);
        }
        json!({
            "specversion": "1.0",
            "id": id,
            "source": "arc://transcript/proj-cache",
            "type": "io.arc.event",
            "time": format!("2026-07-04T10:00:{:02}Z", seq % 60),
            "datacontenttype": "application/json",
            "subtype": subtype,
            "data": {
                "raw": raw,
                "seq": seq,
                "session_id": id_of_event(id),
                "agent_payload": payload,
            },
        })
    }

    fn id_of_event(event_id: &str) -> String {
        // session id is the part before the first '-e'
        event_id.split("-e").next().unwrap_or(event_id).to_string()
    }

    /// Build a `SessionProjection` whose `heap_bytes()` is ~= `target`, by
    /// appending assistant-text events (each contributes ~256 overhead + text
    /// length) until the target is reached.
    fn proj_of_bytes(id: &str, target: u64) -> SessionProjection {
        let mut p = SessionProjection::new(id);
        let mut seq = 1u64;
        while p.heap_bytes() < target {
            let remaining = target - p.heap_bytes();
            // Each event adds ~256 overhead; size the text to fill the rest.
            let text_len = remaining.saturating_sub(256).max(1) as usize;
            p.append(&raw_event(
                &format!("{id}-e{seq}"),
                seq,
                "message.assistant.text",
                json!({"raw": {"type": "assistant", "message": {"model": "m",
                       "content": [{"type": "text", "text": "x".repeat(text_len)}]}}}),
            ));
            seq += 1;
        }
        p
    }

    #[test]
    fn proj_of_bytes_hits_target() {
        // Sanity: the helper produces a projection near the requested size.
        let p = proj_of_bytes("s", 800);
        let n = p.heap_bytes();
        assert!((800..1100).contains(&n), "heap_bytes was {n}, expected ~800");
    }

    #[test]
    fn evicts_lru_cold_but_never_pinned() {
        // Budget 2000 holds two ~800-byte projections; a third forces eviction
        // of the least-recently-used *unpinned* one. (The brief's literal 1500
        // can't hold live+new together, which would wrongly evict `new`; 2000
        // keeps the LRU-vs-pin scenario honest with true ~800-byte projections.)
        let c = ProjectionCache::new(2000, 0);
        c.insert("cold".into(), proj_of_bytes("cold", 800));
        c.pin_live("live");
        c.insert("live".into(), proj_of_bytes("live", 800));
        let _ = c.get("live"); // touch → live is now more-recently-used than cold
        c.insert("new".into(), proj_of_bytes("new", 800)); // over budget → evict LRU cold
        assert!(!c.contains("cold"), "cold is the LRU unpinned entry — evicted");
        assert!(c.contains("live"), "pinned live is never evicted");
        assert!(c.contains("new"), "just-inserted new stays");
        assert!(c.evictions() >= 1);
    }

    #[test]
    fn overshoots_when_all_pinned_rather_than_evicting_live() {
        // Budget 1000, two ~800-byte pinned projections → 1600 > 1000, but both
        // are pinned live. The cache must overshoot, not drop a live session.
        let c = ProjectionCache::new(1000, 0);
        c.pin_live("a");
        c.insert("a".into(), proj_of_bytes("a", 800));
        c.pin_live("b");
        c.insert("b".into(), proj_of_bytes("b", 800));
        assert!(c.contains("a"), "pinned a not evicted");
        assert!(c.contains("b"), "pinned b not evicted");
        assert_eq!(c.evictions(), 0, "no eviction when every candidate is pinned");
        assert!(
            c.resident_bytes() > 1000,
            "overshoots budget rather than evicting a live session"
        );
    }

    #[test]
    fn working_set_window_protects_recently_touched() {
        // working_set_days=1: everything inserted "now" is inside the window,
        // so nothing is evictable even over budget → overshoot.
        let c = ProjectionCache::new(1000, 1);
        c.insert("a".into(), proj_of_bytes("a", 800));
        c.insert("b".into(), proj_of_bytes("b", 800)); // 1600 > 1000 but both fresh
        assert!(c.contains("a"), "a is within the working-set window");
        assert!(c.contains("b"), "b is within the working-set window");
        assert_eq!(c.evictions(), 0);
        assert!(c.resident_bytes() > 1000, "overshoots rather than evicting a hot session");
    }

    #[test]
    fn overwrite_replaces_byte_accounting() {
        // Reinserting the same id must net (new - old), not accumulate.
        let c = ProjectionCache::new(1_000_000, 0);
        c.insert("k".into(), proj_of_bytes("k", 1000));
        let big = c.resident_bytes();

        let small = proj_of_bytes("k", 400);
        let small_bytes = small.heap_bytes();
        c.insert("k".into(), small);

        assert_eq!(c.resident_sessions(), 1, "overwrite keeps a single entry");
        assert!(c.resident_bytes() < big, "smaller projection shrinks resident bytes");
        assert_eq!(
            c.resident_bytes(),
            small_bytes,
            "resident bytes must equal the replacement's heap_bytes, not the sum"
        );
    }

    #[test]
    fn get_refreshes_recency_so_reread_survives_eviction() {
        // a and b fit (1600 <= 2000); touch a; inserting c evicts the true LRU b.
        let c = ProjectionCache::new(2000, 0);
        c.insert("a".into(), proj_of_bytes("a", 800));
        c.insert("b".into(), proj_of_bytes("b", 800));
        assert!(c.get("a").is_some()); // a is now more-recently-used than b
        c.insert("cc".into(), proj_of_bytes("cc", 800)); // over budget → evict LRU (b)
        assert!(c.contains("a"), "a was just read — survives");
        assert!(!c.contains("b"), "b is the true LRU — evicted");
        assert!(c.contains("cc"), "cc newly inserted — stays");
    }
}
