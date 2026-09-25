//! Two in-memory nodes for the consistency specs (C-05, C-06).
//!
//! A `Node` is an AppState with a bus whose `events.*` publishes feed the
//! node's own persist and projections consumers (the production path,
//! minus NATS) and an HTTP listener on an ephemeral port that a peer's
//! catch-up reaches. A partition is a flag that makes the listener answer
//! 503. Nothing here publishes on its own: the spec drives what each node
//! observes.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::Result;
use async_trait::async_trait;
use axum::body::Body;
use axum::http::Request;
use axum::response::IntoResponse;
use open_story::cloud_event::CloudEvent;
use open_story::server::{consumers, SharedState};
use open_story_bus::{Bus, BusSubscription, IngestBatch};
use open_story_store::persistence::SessionStore;
use serde_json::{json, Value};

use crate::helpers::recording_bus::test_state_with_bus;
use crate::helpers::{body_json, make_event_with_time, send_request, test_router};

/// A bus that records every publish and hands `events.*` batches to the
/// node's own consumers, as NATS would.
pub struct LocalBus {
    ingest: tokio::sync::mpsc::Sender<IngestBatch>,
    published: Mutex<Vec<(String, IngestBatch)>>,
}

impl LocalBus {
    pub fn under(&self, prefix: &str) -> Vec<(String, IngestBatch)> {
        self.published
            .lock()
            .unwrap()
            .iter()
            .filter(|(s, _)| s.starts_with(prefix))
            .cloned()
            .collect()
    }
}

#[async_trait]
impl Bus for LocalBus {
    async fn publish(&self, subject: &str, batch: &IngestBatch) -> Result<()> {
        self.published
            .lock()
            .unwrap()
            .push((subject.to_string(), batch.clone()));
        if subject.starts_with("events.") {
            self.ingest.send(batch.clone()).await?;
        }
        Ok(())
    }
    async fn publish_bytes(&self, _subject: &str, _data: &[u8]) -> Result<()> {
        Ok(())
    }
    async fn subscribe(&self, _pattern: &str) -> Result<BusSubscription> {
        let (_tx, rx) = tokio::sync::mpsc::channel(1);
        Ok(BusSubscription { receiver: rx })
    }
    async fn replay(&self, _pattern: &str) -> Result<Vec<IngestBatch>> {
        Ok(vec![])
    }
}

/// One in-memory node.
pub struct Node {
    pub state: SharedState,
    pub bus: Arc<LocalBus>,
    pub base: String,
    pub partitioned: Arc<AtomicBool>,
    _tmp: tempfile::TempDir,
}

impl Node {
    pub async fn start() -> Node {
        let tmp = tempfile::tempdir().unwrap();
        let (tx, mut rx) = tokio::sync::mpsc::channel::<IngestBatch>(256);
        let bus = Arc::new(LocalBus {
            ingest: tx,
            published: Mutex::new(Vec::new()),
        });
        let state = test_state_with_bus(&tmp, bus.clone());

        // The node's consumers, fed by its bus.
        let (event_store, session_store, projections, parents, children, projects, names, plans) = {
            let s = state.read().await;
            (
                s.store.event_store.clone(),
                SessionStore::new(s.store.data_dir.as_path()).unwrap(),
                s.store.projections.clone(),
                s.store.subagent_parents.clone(),
                s.store.session_children.clone(),
                s.store.session_projects.clone(),
                s.store.session_project_names.clone(),
                s.store.plan_store.clone(),
            )
        };
        let mut persist = consumers::persist::PersistConsumer::new(
            event_store.clone(),
            session_store,
            projections.clone(),
            projects,
            names,
            plans,
        );
        let mut proj = consumers::projections::ProjectionsConsumer::new(
            event_store,
            projections,
            parents,
            children,
        );
        tokio::spawn(async move {
            while let Some(b) = rx.recv().await {
                let pid = (!b.project_id.is_empty()).then_some(b.project_id.as_str());
                persist.process_batch(&b.session_id, &b.events, pid).await;
                proj.process_batch(&b.session_id, &b.events).await;
            }
        });

        // The API a peer reaches, behind a partition flag.
        let partitioned = Arc::new(AtomicBool::new(false));
        let flag = partitioned.clone();
        let router = test_router(state.clone()).layer(axum::middleware::from_fn(
            move |req: axum::extract::Request, next: axum::middleware::Next| {
                let flag = flag.clone();
                async move {
                    if flag.load(Ordering::SeqCst) {
                        return axum::http::StatusCode::SERVICE_UNAVAILABLE.into_response();
                    }
                    next.run(req).await
                }
            },
        ));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        Node {
            state,
            bus,
            base,
            partitioned,
            _tmp: tmp,
        }
    }

    /// What the watcher egress does: observed history onto `events.*`.
    pub async fn publish(&self, session: &str, project: &str, events: Vec<CloudEvent>) {
        let batch = IngestBatch {
            session_id: session.to_string(),
            project_id: project.to_string(),
            events,
        };
        self.bus
            .publish(&format!("events.{session}"), &batch)
            .await
            .unwrap();
    }

    /// Synthetic events stamped `host`, ids as given, one second apart.
    pub async fn observe(&self, host: &str, session: &str, ids: &[&str]) {
        let events: Vec<CloudEvent> = ids
            .iter()
            .enumerate()
            .map(|(i, id)| {
                let mut ce = make_event_with_time(
                    "io.arc.event",
                    session,
                    &format!("2026-09-25T10:00:{:02}.000Z", i),
                );
                ce.id = id.to_string();
                ce.with_host(host)
            })
            .collect();
        self.publish(session, "proj-1", events).await;
    }

    pub async fn get(&self, path: &str) -> Value {
        let req = Request::get(path).body(Body::empty()).unwrap();
        body_json(send_request(self.state.clone(), req).await).await
    }

    pub async fn post(&self, hand: &str, body: Value) -> (u16, Value) {
        let req = Request::post(format!("/api/ops/{hand}"))
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();
        let resp = send_request(self.state.clone(), req).await;
        let status = resp.status().as_u16();
        (status, body_json(resp).await)
    }

    pub async fn rollup(&self) -> Value {
        self.get("/api/digests?rollup=1").await["rollup"].clone()
    }

    /// Eventually consistent: wait for the store to hold `n` events in all.
    pub async fn settle(&self, n: u64) {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let r = self.rollup().await;
            if r["events"] == json!(n) {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "store never reached {n} events: {r}"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }
}
