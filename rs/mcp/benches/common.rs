//! Shared helpers for the criterion benches. Duplicates a slim subset
//! of `tests/common/mod.rs` because cargo bench targets can't import
//! from `tests/`.

#![allow(dead_code)]

use anyhow::Result;
use async_trait::async_trait;
use open_story_bus::IngestBatch;
use open_story_mcp::server::Server;
use open_story_mcp::subscription::{
    pump_subscription, CancelGuard, StreamEvent, Subscribe, Subscription,
};
use open_story_store::event_store::{EventStore, SessionRow};
use open_story_store::plan_store::PlanStore;
use open_story_store::sqlite_store::SqliteStore;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use tempfile::TempDir;
use tokio::sync::{mpsc, Mutex};

// ── Store + seed helpers ────────────────────────────────────────────

pub struct SeededStore {
    pub store: Arc<dyn EventStore>,
    pub plan_store: Arc<PlanStore>,
    pub dir: TempDir,
}

pub async fn seed_store(session_count: usize, events_per_session: usize) -> SeededStore {
    let dir = tempfile::tempdir().expect("tempdir");
    let store: Arc<dyn EventStore> = Arc::new(
        SqliteStore::new(dir.path()).expect("open SqliteStore"),
    );
    let plans_dir = dir.path().join("plans");
    std::fs::create_dir_all(&plans_dir).expect("mkdir plans");
    let plan_store = Arc::new(PlanStore::new(&plans_dir).expect("open PlanStore"));

    for s in 0..session_count {
        let sid = format!("bench-sess-{s:04}");
        let project = format!("bench-proj-{}", s % 4);
        let row = SessionRow {
            id: sid.clone(),
            project_id: Some(project.clone()),
            project_name: Some(project.clone()),
            label: Some(format!("session-{s}")),
            custom_label: None,
            branch: Some("main".into()),
            event_count: events_per_session as u64,
            first_event: Some("2026-05-01T00:00:00Z".into()),
            last_event: Some("2026-05-01T01:00:00Z".into()),
            host: Some("bench-host".into()),
            user: Some("bench-user".into()),
        };
        store.upsert_session(&row).await.expect("upsert_session");

        let mut events = Vec::with_capacity(events_per_session);
        for e in 0..events_per_session {
            // Two-arm event mix: prompts (FTS-indexable) and tool uses
            // (so the synopsis tool histogram has substance).
            let event = if e % 3 == 0 {
                let tool_name = ["Bash", "Read", "Edit", "Grep"][e % 4];
                json!({
                    "id": format!("evt-{s:04}-{e:04}"),
                    "type": "io.arc.event",
                    "subtype": "message.assistant.tool_use",
                    "source": format!("arc://transcript/{sid}"),
                    "time": format!("2026-05-01T00:{:02}:{:02}Z", e / 60, e % 60),
                    "data": {
                        "raw": {
                            "message": {
                                "content": [{
                                    "type": "tool_use",
                                    "name": tool_name,
                                    "input": { "command": "ls -la" }
                                }]
                            }
                        },
                        "seq": e,
                        "session_id": sid,
                    }
                })
            } else {
                json!({
                    "id": format!("evt-{s:04}-{e:04}"),
                    "type": "io.arc.event",
                    "subtype": "message.user.prompt",
                    "source": format!("arc://transcript/{sid}"),
                    "time": format!("2026-05-01T00:{:02}:{:02}Z", e / 60, e % 60),
                    "data": {
                        "raw": {
                            "type": "user",
                            "message": {
                                "content": [{
                                    "type": "text",
                                    "text": format!("benchmark prompt {e} in session {s} — find the error in auth.rs"),
                                }]
                            }
                        },
                        "seq": e,
                        "session_id": sid,
                    }
                })
            };
            events.push(event);
        }
        store
            .insert_batch(&sid, &events)
            .await
            .expect("insert_batch");
    }

    SeededStore { store, plan_store, dir }
}

// ── Tool-call driver over an in-memory duplex pipe ──────────────────

pub async fn call_tool<S>(
    server: Server<S>,
    name: &str,
    args: Value,
) -> Value
where
    S: Subscribe,
{
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

    let (mut client_w, server_r) = tokio::io::duplex(64 * 1024);
    let (server_w, client_r) = tokio::io::duplex(64 * 1024);

    let server_task = tokio::spawn(async move {
        let _ = open_story_mcp::stdio::run(server_r, server_w, server).await;
    });

    let req = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": {"name": name, "arguments": args},
    });
    let mut line = serde_json::to_string(&req).unwrap();
    line.push('\n');
    client_w.write_all(line.as_bytes()).await.unwrap();
    drop(client_w);

    let mut reader = tokio::io::BufReader::new(client_r).lines();
    let response = reader.next_line().await.unwrap().unwrap();
    let _ = server_task.await;
    serde_json::from_str(&response).unwrap()
}

// ── LoopbackSubscriber: in-process Subscribe impl ───────────────────

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

    pub async fn publish(&self, session_id: &str, batch: IngestBatch) {
        let routes = self.routes.lock().await;
        if let Some(routes) = routes.get(session_id) {
            for route in routes {
                let _ = route.src_tx.send(batch.clone()).await;
            }
        }
    }
}

#[async_trait]
impl Subscribe for LoopbackSubscriber {
    async fn subscribe(&self, session_id: &str) -> Result<Subscription> {
        let session_id = session_id.to_string();
        let route_id = uuid::Uuid::new_v4();
        let (src_tx, src_rx) = mpsc::channel::<IngestBatch>(1024);
        let (sink_tx, sink_rx) = mpsc::channel::<StreamEvent>(1024);

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

// ── Streaming-bench event helpers ───────────────────────────────────

use open_story_core::cloud_event::CloudEvent;
use open_story_core::event_data::EventData;

/// Build an IngestBatch carrying `event_count` events for `session_id`.
/// Events have valid CloudEvent shape so the broadcaster can serialize.
pub fn batch_with_events(session_id: &str, event_count: usize) -> IngestBatch {
    let mut events = Vec::with_capacity(event_count);
    for i in 0..event_count {
        let raw = json!({
            "type": "user",
            "message": {
                "content": [{"type": "text", "text": format!("event {i}")}]
            }
        });
        events.push(CloudEvent::new(
            format!("arc://transcript/{session_id}"),
            "io.arc.event".to_string(),
            EventData::new(raw, i as u64, session_id.to_string()),
            None,
            None,
            None,
            None,
            None,
            None,
        ));
    }
    IngestBatch {
        session_id: session_id.to_string(),
        project_id: "bench-proj".to_string(),
        events,
    }
}
