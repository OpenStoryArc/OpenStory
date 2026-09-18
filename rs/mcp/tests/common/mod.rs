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

pub mod story_fixture;
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
    let (store, plan_store, dir) = make_test_store();
    let subscriber = LoopbackSubscriber::new();
    (
        Server::new(subscriber.clone(), store, plan_store),
        subscriber,
        dir,
    )
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
    pump_patterns, pump_subscription, CancelGuard, StreamEvent, Subscribe, Subscription,
};
use open_story_patterns::PatternEvent;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};

/// In-process Subscribe impl. Each subscribe() opens a fresh source
/// channel; publish() fans out to all routes for the given session_id.
#[derive(Clone, Default)]
pub struct LoopbackSubscriber {
    routes: Arc<Mutex<HashMap<String, Vec<Route>>>>,
    /// Story-pattern routes keyed by session id, or "*" for follow-all.
    arc_routes: Arc<Mutex<HashMap<String, Vec<ArcRoute>>>>,
    /// Every published pattern batch: (batch_seq, session_id, JSON array).
    /// The stored prefix a `from_seq` cursor replays from.
    history: Arc<Mutex<Vec<(u64, String, Value)>>>,
}

struct ArcRoute {
    route_id: uuid::Uuid,
    src_tx: mpsc::Sender<(u64, Value)>,
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

    /// Publish a pattern batch the way Actor 2 does: one JSON array per
    /// batch on `patterns.{project}.{session}`. Recorded in history so a
    /// later `subscribe_arcs(from_seq)` can replay it.
    pub async fn publish_patterns(&self, session_id: &str, patterns: Vec<PatternEvent>) {
        let batch = serde_json::to_value(&patterns).unwrap();
        let batch_seq = {
            let mut history = self.history.lock().await;
            let seq = history.len() as u64 + 1;
            history.push((seq, session_id.to_string(), batch.clone()));
            seq
        };
        let routes = self.arc_routes.lock().await;
        for key in [session_id, "*"] {
            if let Some(rs) = routes.get(key) {
                for r in rs {
                    let _ = r.src_tx.send((batch_seq, batch.clone())).await;
                }
            }
        }
    }

    pub async fn arc_route_count(&self, session_id: &str) -> usize {
        let routes = self.arc_routes.lock().await;
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

        Ok(Subscription::from_parts(
            route_id, session_id, sink_rx, cancel,
        ))
    }

    async fn subscribe_arcs(
        &self,
        session_id: Option<&str>,
        from_seq: Option<u64>,
    ) -> Result<Subscription> {
        let key = session_id.unwrap_or("*").to_string();
        let route_id = uuid::Uuid::new_v4();
        let (src_tx, src_rx) = mpsc::channel::<(u64, Value)>(256);
        let (sink_tx, sink_rx) = mpsc::channel::<StreamEvent>(256);
        // Replay the stored prefix from the cursor before going live.
        if let Some(start) = from_seq {
            let history = self.history.lock().await;
            for (seq, sid, batch) in history.iter() {
                if *seq >= start && (session_id.is_none() || session_id == Some(sid.as_str())) {
                    let _ = src_tx.send((*seq, batch.clone())).await;
                }
            }
        }
        {
            let mut routes = self.arc_routes.lock().await;
            routes
                .entry(key.clone())
                .or_default()
                .push(ArcRoute { route_id, src_tx });
        }
        let pump = tokio::spawn(pump_patterns(src_rx, sink_tx, session_id.map(String::from)));
        let routes_for_cancel = self.arc_routes.clone();
        let key_for_cancel = key.clone();
        let cancel = CancelGuard::from_fn(move || {
            pump.abort();
            let routes_for_cancel = routes_for_cancel.clone();
            let key_for_cancel = key_for_cancel.clone();
            tokio::spawn(async move {
                let mut routes = routes_for_cancel.lock().await;
                if let Some(rs) = routes.get_mut(&key_for_cancel) {
                    rs.retain(|r| r.route_id != route_id);
                    if rs.is_empty() {
                        routes.remove(&key_for_cancel);
                    }
                }
            });
        });
        Ok(Subscription::from_parts(route_id, key, sink_rx, cancel))
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
