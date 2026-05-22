//! Streaming-throughput benchmarks for `subscribe_session`.
//!
//! Uses `LoopbackSubscriber` (in-process Subscribe impl) rather than a
//! real NATS testcontainer so the measurement isolates the MCP server's
//! work — protocol framing, broadcaster fanout, stdout write rate —
//! from network latency. The NATS path is exercised by the
//! `testcontainer_nats` integration test; this bench answers "how fast
//! can the MCP serialize and emit notifications once events arrive."
//!
//! Each iteration:
//!   1. Open a subscribe_session stream over a fresh in-memory duplex.
//!   2. Publish N single-event IngestBatches via LoopbackSubscriber.
//!   3. Read exactly N notification lines off stdout.
//!   4. Drop everything.
//!
//! `pump_subscription` emits one notification per IngestBatch (the whole
//! batch lands in the `data` payload of one `notifications/openstory/stream`).
//! Real agents publish whole turns as batches, but for a microbenchmark
//! the per-batch overhead — channel send + JSON serialize + stdout
//! write — is what dominates throughput, so we measure batches/sec by
//! publishing many small batches.
//!
//! Throughput is reported in batches/sec via criterion's Throughput.
//!
//! Run with: cargo bench -p open-story-mcp --bench streaming

#[path = "common.rs"]
mod common;

use common::{batch_with_events, seed_store, LoopbackSubscriber, SeededStore};
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use open_story_mcp::server::Server;
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
use tokio::runtime::Runtime;

const BATCH_COUNTS: &[usize] = &[100, 1000, 5000];
const SESSION_ID: &str = "bench-stream-sess";

/// Lightweight store — streaming doesn't query the store, only the
/// subscription pump matters. A near-empty SqliteStore avoids the
/// per-iteration setup cost we'd pay re-seeding.
fn shared_store(rt: &Runtime) -> Arc<SeededStore> {
    Arc::new(rt.block_on(seed_store(1, 0)))
}

async fn run_stream(seeded: Arc<SeededStore>, batch_count: usize) {
    let subscriber = LoopbackSubscriber::new();
    let server = Server::new(
        subscriber.clone(),
        seeded.store.clone(),
        seeded.plan_store.clone(),
    );

    let (mut client_w, server_r) = tokio::io::duplex(256 * 1024);
    let (server_w, client_r) = tokio::io::duplex(256 * 1024);

    let server_task = tokio::spawn(async move {
        let _ = open_story_mcp::stdio::run(server_r, server_w, server).await;
    });

    // Open the stream. We send a tools/call for subscribe_session and
    // then drain the streaming notifications.
    let req = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": {
            "name": "subscribe_session",
            "arguments": { "session_id": SESSION_ID },
        }
    });
    let mut line = serde_json::to_string(&req).unwrap();
    line.push('\n');
    client_w.write_all(line.as_bytes()).await.unwrap();

    let mut reader = tokio::io::BufReader::new(client_r).lines();
    // First line is the tools/call ack (the subscribe handshake reply).
    let _ack = reader.next_line().await.unwrap().unwrap();

    // Subscription is live — fire N batches and drain N notifications.
    // Each batch carries 1 event so the per-batch overhead (the thing
    // we're actually measuring) dominates.
    for _ in 0..batch_count {
        let batch = batch_with_events(SESSION_ID, 1);
        subscriber.publish(SESSION_ID, batch).await;
    }

    let mut received: usize = 0;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    while received < batch_count {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        match tokio::time::timeout(remaining, reader.next_line()).await {
            Ok(Ok(Some(line))) => {
                if line.contains("notifications/openstory/stream") {
                    received += 1;
                }
            }
            _ => panic!(
                "streaming bench timed out at {received}/{batch_count} notifications"
            ),
        }
    }

    drop(client_w); // signal end-of-stream
    let _ = server_task.await;
}

fn bench_subscribe_session(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let seeded = shared_store(&rt);
    let mut group = c.benchmark_group("subscribe_session");
    // Streaming benches are I/O-heavy — smaller samples keep the run
    // under a minute.
    group.sample_size(20);
    group.measurement_time(Duration::from_secs(10));

    for &count in BATCH_COUNTS {
        group.throughput(Throughput::Elements(count as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(count),
            &count,
            |b, &count| {
                b.to_async(&rt).iter(|| {
                    let seeded = seeded.clone();
                    async move {
                        run_stream(seeded, count).await
                    }
                });
            },
        );
    }
    group.finish();
}

criterion_group!(benches, bench_subscribe_session);
criterion_main!(benches);
