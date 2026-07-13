//! Task 9b — real-threads concurrency stress tests for the two in-memory
//! caches (`PayloadCache`, `ProjectionCache`).
//!
//! These are deliberately *not* unit tests of a single code path: they hammer
//! each cache from many real OS threads doing overlapping inserts / reads /
//! pins so that the known bug classes would show up as flaky/failing
//! assertions if reintroduced:
//!   - `PayloadCache` byte-underflow race (a `sub_bytes` racing ahead of its
//!     matching add would pin `resident_bytes()` near `u64::MAX`).
//!   - `ProjectionCache` pin/eviction races (a pinned-but-never-unpinned
//!     session getting evicted anyway, or byte accounting blowing up under
//!     concurrent insert/evict).
//!
//! Both tests are intentionally *not* deterministic proofs — they raise the
//! odds of tripping a race dramatically over the existing single-threaded
//! tests in `payload_cache.rs` / `projection_cache.rs`, which is the point of
//! a stress test.

use crate::payload_cache::PayloadCache;
use crate::projection::SessionProjection;
use crate::projection_cache::ProjectionCache;
use serde_json::{json, Value};
use std::sync::Arc;

#[test]
fn payload_cache_survives_concurrent_hammering() {
    let c = Arc::new(PayloadCache::new(50_000));
    let mut hs = vec![];
    for t in 0..8 {
        let c = c.clone();
        hs.push(std::thread::spawn(move || {
            for i in 0..2_000 {
                let k = (format!("s{}", t), format!("e{}", i % 64));
                if i % 3 == 0 {
                    c.get(&k);
                } else {
                    c.insert(k, "x".repeat(200 + (i % 400)));
                }
            }
        }));
    }
    for h in hs {
        h.join().unwrap();
    }
    // The underflow bug would leave resident_bytes near u64::MAX.
    assert!(
        c.resident_bytes() <= 50_000 * 4,
        "bytes underflowed/blew up: {}",
        c.resident_bytes()
    );
    c.insert(("z".into(), "z".into()), "y".repeat(100)); // still functional
    assert!(c.get(&("z".into(), "z".into())).is_some());
}

/// Build a well-formed CloudEvent envelope carrying a single
/// `message.assistant.text` event, matching the shape `SessionProjection::append`
/// (via `from_cloud_event`) expects. Mirrors the helper already used by
/// `projection_cache`'s own `#[cfg(test)] mod tests`.
fn raw_event(id: &str, session_id: &str, seq: u64, text_len: usize) -> Value {
    json!({
        "specversion": "1.0",
        "id": id,
        "source": "arc://transcript/concurrency-test",
        "type": "io.arc.event",
        "time": format!("2026-07-04T10:00:{:02}Z", seq % 60),
        "datacontenttype": "application/json",
        "subtype": "message.assistant.text",
        "data": {
            "raw": {
                "type": "assistant",
                "message": {
                    "model": "m",
                    "content": [{"type": "text", "text": "x".repeat(text_len)}]
                }
            },
            "seq": seq,
            "session_id": session_id,
            "agent_payload": {
                "_variant": "claude-code",
                "meta": {"agent": "claude-code"},
            },
        },
    })
}

/// Build a `SessionProjection` whose `heap_bytes()` is roughly `target`, by
/// appending assistant-text events (each ~256 bytes overhead + text length)
/// until the target is reached. Same approach as `projection_cache`'s
/// `proj_of_bytes` test helper, duplicated here so this test module doesn't
/// depend on another module's private `#[cfg(test)]` internals.
fn proj_of_bytes(id: &str, target: u64) -> SessionProjection {
    let mut p = SessionProjection::new(id);
    let mut seq = 1u64;
    while p.heap_bytes() < target {
        let remaining = target - p.heap_bytes();
        let text_len = remaining.saturating_sub(256).max(1) as usize;
        p.append(&raw_event(&format!("{id}-e{seq}"), id, seq, text_len));
        seq += 1;
    }
    p
}

#[test]
fn projection_cache_survives_concurrent_pin_evict() {
    // Small budget relative to the total bytes threads will try to insert,
    // so eviction is forced to fire repeatedly under contention.
    const MAX_BYTES: u64 = 5_000;
    let c = Arc::new(ProjectionCache::new(MAX_BYTES, 0));

    // One session pinned once, up front, and NEVER unpinned. It must survive
    // every round of concurrent eviction below.
    c.pin_live("PINNED");
    c.insert("PINNED".into(), proj_of_bytes("PINNED", 800));

    // A handful of "hot" ids that get pinned/unpinned repeatedly by dedicated
    // threads while other threads insert/evict around them, to exercise the
    // pin/eviction race directly.
    let hot_ids: Vec<String> = (0..4).map(|i| format!("hot{i}")).collect();
    for id in &hot_ids {
        c.insert(id.clone(), proj_of_bytes(id, 300));
    }

    let mut hs = vec![];

    // Inserter threads: many distinct session ids, small-ish bodies, enough
    // total volume to blow way past MAX_BYTES and force eviction.
    for t in 0..6 {
        let c = c.clone();
        hs.push(std::thread::spawn(move || {
            for i in 0..300 {
                let id = format!("t{t}-s{i}");
                let size = 100 + (i % 5) * 150; // 100..=700 bytes
                c.insert(id, proj_of_bytes(&format!("t{t}-s{i}"), size as u64));
            }
        }));
    }

    // Pinner threads: repeatedly pin_live/unpin_live the hot ids, racing
    // against the inserter threads' eviction sweeps.
    for _ in 0..2 {
        let c = c.clone();
        let hot_ids = hot_ids.clone();
        hs.push(std::thread::spawn(move || {
            for i in 0..500 {
                let id = &hot_ids[i % hot_ids.len()];
                c.pin_live(id);
                c.unpin_live(id);
            }
        }));
    }

    // Reader threads: get() on a mix of the never-unpinned PINNED session,
    // the hot ids, and whatever the inserters are producing. Ref guards are
    // dropped immediately (scoped to the `if let`) before any other cache
    // call, per the API's deadlock note.
    for t in 0..2 {
        let c = c.clone();
        let hot_ids = hot_ids.clone();
        hs.push(std::thread::spawn(move || {
            for i in 0..300 {
                if i % 7 == 0 {
                    if let Some(r) = c.get("PINNED") {
                        let _ = r.heap_bytes();
                    }
                } else if i % 3 == 0 {
                    let id = &hot_ids[i % hot_ids.len()];
                    if let Some(r) = c.get(id) {
                        let _ = r.heap_bytes();
                    }
                } else {
                    let id = format!("t{}-s{}", t % 6, i % 300);
                    if let Some(r) = c.get(&id) {
                        let _ = r.heap_bytes();
                    }
                }
            }
        }));
    }

    for h in hs {
        h.join().unwrap();
    }

    // A byte-underflow bug would leave resident_bytes pinned near u64::MAX;
    // a runaway bug would leave it wildly over budget. Either way this bound
    // is generous (the cache is allowed to overshoot when candidates are
    // protected) but rules out both failure modes.
    assert!(
        c.resident_bytes() <= MAX_BYTES * 50,
        "bytes underflowed/blew up: {}",
        c.resident_bytes()
    );

    // The never-unpinned session must still be resident no matter how much
    // eviction pressure the other threads created.
    assert!(
        c.get("PINNED").is_some(),
        "never-unpinned PINNED session was evicted"
    );

    // With a 5_000-byte budget and 6 threads x 300 inserts of 100-700 bytes
    // each, eviction must have fired at least once.
    assert!(c.evictions() > 0, "expected eviction pressure to have fired");

    // The cache must still be fully functional after the hammering.
    c.insert("fresh".into(), proj_of_bytes("fresh", 200));
    assert!(c.get("fresh").is_some());
}
