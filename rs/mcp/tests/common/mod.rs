//! Shared test fixtures for MCP integration tests.
//!
//! `LoopbackSubscriber` is the in-process test substitute for
//! `NatsBus`. Tests call `subscriber.publish(sid, batch)` and the
//! published IngestBatch is delivered through the SAME
//! `pump_subscription` pipeline that production uses — only the
//! source channel differs.
//!
//! Lives in `tests/common/` so it can never leak into production
//! code paths. `rs/mcp/src/` does not import this.

#![allow(dead_code)]

pub mod nats_container;
pub mod store_fixture;

pub use store_fixture::make_test_store;

use open_story_mcp::server::Server;
use tempfile::TempDir;

/// Build a test `Server` holding a fresh `LoopbackSubscriber` and a
/// temp-dir `SqliteStore`. Returns the server, a clone of the
/// subscriber the test can use for `.publish(...)`, and the
/// `TempDir` guard the test must keep in scope.
///
/// Usage:
///     let (server, subscriber, _tmp) = common::make_test_server();
///     // pass `server` into stdio::run
///     // call `subscriber.publish(sid, batch)` to fire events
pub fn make_test_server() -> (Server<LoopbackSubscriber>, LoopbackSubscriber, TempDir) {
    let (store, dir) = make_test_store();
    let subscriber = LoopbackSubscriber::new();
    (Server::new(subscriber.clone(), store), subscriber, dir)
}

/// Drive a single `tools/call` through stdio against the given server
/// and return the parsed JSON-RPC response. Closes stdin afterward so
/// the server task completes.
///
/// Useful for query tools where you don't need to interact across
/// multiple requests; streaming tools need the manual duplex pattern.
pub async fn call_tool<S>(
    test_server: Server<S>,
    name: &str,
    args: serde_json::Value,
) -> serde_json::Value
where
    S: open_story_mcp::subscription::Subscribe,
{
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

    let (mut client_w, server_r) = tokio::io::duplex(64 * 1024);
    let (server_w, client_r) = tokio::io::duplex(64 * 1024);

    let server_task = tokio::spawn(async move {
        let _ = open_story_mcp::stdio::run(server_r, server_w, test_server).await;
    });

    let req = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": {"name": name, "arguments": args},
    });
    let mut line = serde_json::to_string(&req).unwrap();
    line.push('\n');
    client_w.write_all(line.as_bytes()).await.unwrap();
    drop(client_w); // signal end-of-stream so the server exits

    let mut reader = tokio::io::BufReader::new(client_r).lines();
    let response_line = reader
        .next_line()
        .await
        .expect("readline must not error")
        .expect("server must emit one response line");
    let _ = server_task.await;
    serde_json::from_str(&response_line).expect("response is valid JSON")
}

/// Extract the inner JSON payload from a `tools/call` response.
/// Returns Err(message) if `isError` was true.
pub fn unwrap_tool_result(response: &serde_json::Value) -> Result<serde_json::Value, String> {
    let result = &response["result"];
    let is_err = result["isError"].as_bool().unwrap_or(false);
    let text = result["content"][0]["text"]
        .as_str()
        .ok_or_else(|| "missing content[0].text".to_string())?;
    if is_err {
        Err(text.to_string())
    } else {
        serde_json::from_str(text).map_err(|e| format!("parse failed: {e}"))
    }
}

use anyhow::Result;
use async_trait::async_trait;
use open_story_bus::IngestBatch;
use open_story_mcp::subscription::{
    pump_subscription, CancelGuard, StreamEvent, Subscribe, Subscription,
};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};

/// In-process Subscribe impl. Each subscribe() opens a fresh source
/// channel; publish() fans out to all routes for the given session_id.
#[derive(Clone, Default)]
pub struct LoopbackSubscriber {
    routes: Arc<Mutex<HashMap<String, Vec<Route>>>>,
}

struct Route {
    route_id: uuid::Uuid,
    src_tx: mpsc::Sender<IngestBatch>,
}

impl LoopbackSubscriber {
    pub fn new() -> Self {
        Self::default()
    }

    /// Publish an IngestBatch to every route open for `session_id`.
    pub async fn publish(&self, session_id: &str, batch: IngestBatch) {
        let routes = self.routes.lock().await;
        if let Some(routes) = routes.get(session_id) {
            for route in routes {
                let _ = route.src_tx.send(batch.clone()).await;
            }
        }
    }

    pub async fn route_count(&self, session_id: &str) -> usize {
        let routes = self.routes.lock().await;
        routes.get(session_id).map(|r| r.len()).unwrap_or(0)
    }
}

#[async_trait]
impl Subscribe for LoopbackSubscriber {
    async fn subscribe(&self, session_id: &str) -> Result<Subscription> {
        let session_id = session_id.to_string();
        let route_id = uuid::Uuid::new_v4();
        let (src_tx, src_rx) = mpsc::channel::<IngestBatch>(256);
        let (sink_tx, sink_rx) = mpsc::channel::<StreamEvent>(256);

        {
            let mut routes = self.routes.lock().await;
            routes
                .entry(session_id.clone())
                .or_default()
                .push(Route { route_id, src_tx });
        }

        let pump = tokio::spawn(pump_subscription(src_rx, sink_tx, session_id.clone()));

        let routes_for_cancel = self.routes.clone();
        let sid_for_cancel = session_id.clone();
        let cancel = CancelGuard::from_fn(move || {
            pump.abort();
            // Spawn a cleanup task to remove our route from the map.
            // Drop is sync; spawning is the cleanest way to release the lock.
            let routes_for_cancel = routes_for_cancel.clone();
            let sid_for_cancel = sid_for_cancel.clone();
            tokio::spawn(async move {
                let mut routes = routes_for_cancel.lock().await;
                if let Some(rs) = routes.get_mut(&sid_for_cancel) {
                    rs.retain(|r| r.route_id != route_id);
                    if rs.is_empty() {
                        routes.remove(&sid_for_cancel);
                    }
                }
            });
        });

        Ok(Subscription::from_parts(route_id, session_id, sink_rx, cancel))
    }
}

// ── Helpers for building IngestBatches in tests ────────────────────

use open_story_core::cloud_event::CloudEvent;
use open_story_core::event_data::EventData;
use serde_json::{json, Value};

/// Build an empty-events IngestBatch for tests that only care about
/// "an event arrived for this session id."
pub fn empty_batch(session_id: &str) -> IngestBatch {
    IngestBatch {
        session_id: session_id.to_string(),
        project_id: "test-project".to_string(),
        events: vec![],
    }
}

/// Build a single-event IngestBatch with the given raw payload.
/// Used by tests that need a specific wire-shape (e.g., token usage).
pub fn batch_with_raw(session_id: &str, raw: Value) -> IngestBatch {
    let event = CloudEvent::new(
        "test://source".to_string(),
        "io.arc.event".to_string(),
        EventData::new(raw, 1, session_id.to_string()),
        None,
        None,
        None,
        None,
        None,
        None,
    );
    IngestBatch {
        session_id: session_id.to_string(),
        project_id: "test-project".to_string(),
        events: vec![event],
    }
}

/// Convenience: build a batch whose single event carries `usage` data
/// at the path TokenAggregator extracts (`data.raw.message.usage`).
pub fn batch_with_usage(session_id: &str, usage: Value) -> IngestBatch {
    batch_with_raw(session_id, json!({ "message": { "usage": usage } }))
}
