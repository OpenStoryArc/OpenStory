//! Per-tool latency benchmarks against a seeded SqliteStore.
//!
//! Each bench drives a full `tools/call` JSON-RPC through the stdio
//! handler — the same path the OpenClaw agent hits. That captures the
//! protocol cost (parse, dispatch, response serialize) alongside the
//! store query cost.
//!
//! Three sizes per bench:
//!   small  =   10 sessions × 30 events each =     300 events
//!   medium =  100 sessions × 30 events each =   3 000 events
//!   large  = 1000 sessions × 30 events each =  30 000 events
//!
//! Tools benched:
//!   - tools/list             — handshake baseline (no store hit)
//!   - list_sessions          — full session-row scan
//!   - session_synopsis       — single-session aggregate
//!   - search (FTS)           — full-text scan over all events
//!
//! Run with:   cargo bench -p open-story-mcp --bench tools
//! Quick mode: CRITERION_DEBUG=1 cargo bench -p open-story-mcp --bench tools

#[path = "common.rs"]
mod common;

use common::{call_tool, seed_store, LoopbackSubscriber, SeededStore};
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use open_story_mcp::server::Server;
use serde_json::json;
use std::sync::Arc;
use tokio::runtime::Runtime;

const SIZES: &[(&str, usize, usize)] =
    &[("small", 10, 30), ("medium", 100, 30), ("large", 1000, 30)];

/// One-time setup per size: seed the store and wrap it in an Arc so
/// every bench iteration cheaply rebuilds a fresh `Server`.
fn setup(rt: &Runtime, sessions: usize, events: usize) -> Arc<SeededStore> {
    Arc::new(rt.block_on(seed_store(sessions, events)))
}

fn bench_tools_list(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("tools_list");
    group.sample_size(50);

    // Sized doesn't really apply to tools/list (no store hit) but we
    // still run small-only to get a handshake-cost number.
    let seeded = setup(&rt, 10, 30);
    group.bench_function("baseline", |b| {
        b.to_async(&rt).iter(|| {
            let store = seeded.store.clone();
            let plans = seeded.plan_store.clone();
            async move {
                let server = Server::new(LoopbackSubscriber::new(), store, plans);
                call_tool(server, "tools/list", json!({})).await
            }
        });
    });
    group.finish();
}

fn bench_list_sessions(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("list_sessions");
    group.sample_size(30);

    for &(label, sessions, events) in SIZES {
        let seeded = setup(&rt, sessions, events);
        group.throughput(Throughput::Elements(sessions as u64));
        group.bench_with_input(BenchmarkId::from_parameter(label), &seeded, |b, seeded| {
            b.to_async(&rt).iter(|| {
                let store = seeded.store.clone();
                let plans = seeded.plan_store.clone();
                async move {
                    let server = Server::new(LoopbackSubscriber::new(), store, plans);
                    call_tool(server, "list_sessions", json!({})).await
                }
            });
        });
    }
    group.finish();
}

fn bench_session_synopsis(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("session_synopsis");
    group.sample_size(30);

    for &(label, sessions, events) in SIZES {
        let seeded = setup(&rt, sessions, events);
        // Query a session in the middle of the seeded range so we're
        // not benching the best-case (first session, hot cache).
        let target_sid = format!("bench-sess-{:04}", sessions / 2);
        group.bench_with_input(
            BenchmarkId::from_parameter(label),
            &(seeded, target_sid),
            |b, (seeded, sid)| {
                b.to_async(&rt).iter(|| {
                    let store = seeded.store.clone();
                    let plans = seeded.plan_store.clone();
                    let sid = sid.clone();
                    async move {
                        let server = Server::new(LoopbackSubscriber::new(), store, plans);
                        call_tool(server, "session_synopsis", json!({ "session_id": sid })).await
                    }
                });
            },
        );
    }
    group.finish();
}

fn bench_search(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("search");
    group.sample_size(30);

    for &(label, sessions, events) in SIZES {
        let seeded = setup(&rt, sessions, events);
        group.throughput(Throughput::Elements((sessions * events) as u64));
        group.bench_with_input(BenchmarkId::from_parameter(label), &seeded, |b, seeded| {
            b.to_async(&rt).iter(|| {
                let store = seeded.store.clone();
                let plans = seeded.plan_store.clone();
                async move {
                    let server = Server::new(LoopbackSubscriber::new(), store, plans);
                    call_tool(server, "search", json!({ "query": "auth.rs", "limit": 50 })).await
                }
            });
        });
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_tools_list,
    bench_list_sessions,
    bench_session_synopsis,
    bench_search,
);
criterion_main!(benches);
