//! PayloadCache — byte-bounded LRU over `(session_id, event_id) -> body`.
//!
//! A read-through cache for full event bodies. The durable body always
//! remains in SQLite (fetched on miss by a later task); this cache never
//! deletes from the store — it only holds an in-memory, best-effort copy
//! bounded by a byte budget, evicting least-recently-used entries when
//! that budget is exceeded.
//!
//! Built on `DashMap` (sharded, lock-per-shard) plus a couple of atomics
//! for the byte budget and a monotonic recency clock. Under concurrent
//! access the LRU ordering is best-effort (an entry's `tick` can be
//! bumped by a `get()` a moment after it was already chosen as the
//! eviction victim) — that's an accepted looseness for an approximate
//! cache, not a correctness bug: it never double-frees, never
//! double-counts bytes, and always terminates (see `insert` below).

use dashmap::DashMap;
use std::sync::atomic::{AtomicU64, Ordering};

struct Entry {
    body: String,
    tick: u64,
}

/// Byte-bounded LRU cache of full event bodies, keyed by `(session_id, event_id)`.
///
/// This is a cache, never a source of truth: it holds nothing that isn't
/// also durably persisted elsewhere (SQLite), and `insert`/eviction here
/// never deletes anything from the durable store.
pub struct PayloadCache {
    map: DashMap<(String, String), Entry>,
    max_bytes: u64,
    bytes: AtomicU64,
    clock: AtomicU64,
}

impl PayloadCache {
    pub fn new(max_bytes: u64) -> Self {
        Self {
            map: DashMap::new(),
            max_bytes,
            bytes: AtomicU64::new(0),
            clock: AtomicU64::new(0),
        }
    }

    fn tick(&self) -> u64 {
        self.clock.fetch_add(1, Ordering::Relaxed)
    }

    /// Subtract from the resident-byte counter, saturating at 0.
    ///
    /// A plain `fetch_sub` can underflow to a ~2^64 value if a subtract
    /// ever races ahead of its matching add (or double-fires); that would
    /// pin `bytes > max_bytes` forever and make the cache evict everything
    /// without self-healing. Clamping at 0 turns any such mistiming into a
    /// transient *undercount* that corrects itself on the next insert.
    fn sub_bytes(&self, n: u64) {
        let _ = self
            .bytes
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |b| Some(b.saturating_sub(n)));
    }

    pub fn resident_bytes(&self) -> u64 {
        self.bytes.load(Ordering::Relaxed)
    }

    /// True when no body is resident.
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// Event ids of the resident bodies for a session. Introspection helper
    /// (snapshot tests, diagnostics) that avoids exposing the private `Entry`
    /// via a raw `iter()`. Order is unspecified — callers sort if they need
    /// determinism.
    pub fn session_event_ids(&self, session_id: &str) -> Vec<String> {
        self.map
            .iter()
            .filter(|e| e.key().0 == session_id)
            .map(|e| e.key().1.clone())
            .collect()
    }

    /// Drop every cached body for a session (used when the durable session is
    /// deleted). Re-accounts bytes for each removed entry. Keys are
    /// `(session_id, event_id)`, so this walks and prunes the matching prefix —
    /// the focused replacement for the previous `iter()` + `remove()` dance.
    pub fn remove_session(&self, session_id: &str) {
        let to_drop: Vec<(String, String)> = self
            .map
            .iter()
            .filter(|e| e.key().0 == session_id)
            .map(|e| e.key().clone())
            .collect();
        for k in to_drop {
            if let Some((_, e)) = self.map.remove(&k) {
                self.sub_bytes(e.body.len() as u64);
            }
        }
    }

    /// Look up a cached body, marking it as most-recently-used on a hit.
    pub fn get(&self, key: &(String, String)) -> Option<String> {
        let t = self.tick();
        self.map.get_mut(key).map(|mut e| {
            e.tick = t;
            e.body.clone()
        })
    }

    /// Insert (or replace) a cached body, then evict least-recently-used
    /// entries — never the durable record, just this in-memory copy —
    /// while resident bytes exceed the budget.
    pub fn insert(&self, key: (String, String), body: String) {
        let len = body.len() as u64;
        // Count the bytes BEFORE making the entry visible/evictable in the
        // map. If we inserted first, another thread could evict this entry
        // (and `sub_bytes` its length) in the window before we added it —
        // subtracting bytes that were never counted. Adding first closes
        // that window: an entry is never evictable-before-counted.
        self.bytes.fetch_add(len, Ordering::Relaxed);
        let t = self.tick();
        if let Some(old) = self.map.insert(key, Entry { body, tick: t }) {
            // Overwrite: drop the replaced entry's contribution.
            self.sub_bytes(old.body.len() as u64);
        }

        // Evict least-recently-used entries (by min tick) while over
        // budget. Each successful removal strictly shrinks the map, so
        // this terminates even if `bytes` were ever inconsistent; if the
        // map is (or becomes) empty, `victim` is `None` and we stop
        // rather than spin.
        while self.bytes.load(Ordering::Relaxed) > self.max_bytes {
            let victim = self
                .map
                .iter()
                .min_by_key(|e| e.value().tick)
                .map(|e| e.key().clone());
            match victim {
                Some(k) => {
                    if let Some((_, e)) = self.map.remove(&k) {
                        self.sub_bytes(e.body.len() as u64);
                    }
                }
                None => break,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payload_cache_evicts_lru_over_budget() {
        let c = PayloadCache::new(1000);
        c.insert(("s".into(), "a".into()), "x".repeat(600));
        c.insert(("s".into(), "b".into()), "x".repeat(600)); // now >1000 → evict LRU ("a")
        assert!(c.get(&("s".into(), "a".into())).is_none(), "a should be evicted");
        assert!(c.get(&("s".into(), "b".into())).is_some(), "b stays");
        assert!(c.resident_bytes() <= 1000);
    }

    #[test]
    fn payload_cache_overwrite_replaces_byte_accounting() {
        let c = PayloadCache::new(1000);
        c.insert(("s".into(), "k".into()), "x".repeat(600));
        assert_eq!(c.resident_bytes(), 600);
        c.insert(("s".into(), "k".into()), "x".repeat(300)); // same key, smaller body
        assert_eq!(
            c.resident_bytes(),
            300,
            "overwrite must drop the old body's bytes, not accumulate"
        );
    }

    #[test]
    fn payload_cache_get_refreshes_recency_so_reread_key_survives_eviction() {
        let c = PayloadCache::new(1000);
        c.insert(("s".into(), "a".into()), "x".repeat(600));
        c.insert(("s".into(), "b".into()), "x".repeat(300)); // 900 total, no eviction yet

        // Touch "a" so it becomes more-recently-used than "b".
        assert!(c.get(&("s".into(), "a".into())).is_some());

        // Pushes total to 1200 → over budget → LRU ("b", untouched) is evicted, not "a".
        c.insert(("s".into(), "c".into()), "x".repeat(300));

        assert!(
            c.get(&("s".into(), "a".into())).is_some(),
            "a was just read, should survive eviction"
        );
        assert!(
            c.get(&("s".into(), "b".into())).is_none(),
            "b is the true LRU, should be evicted"
        );
        assert!(c.get(&("s".into(), "c".into())).is_some(), "c is newly inserted, should stay");
        assert!(c.resident_bytes() <= 1000);
    }
}
