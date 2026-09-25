//! Converge, tier 1 (consistency C-05): `POST /api/ops/converge`.
//!
//! Two in-memory nodes, each an AppState with a bus whose `events.*`
//! publishes feed that node's own persist and projections consumers (the
//! production path, minus NATS) and an HTTP listener on an ephemeral port
//! that a peer's catch-up reaches. A partition is a flag that makes the
//! listener answer 503. Converge is a union (catch-up pulls through the
//! existing path) and a re-fold (reproject); it never writes an event body
//! and never publishes to `events.*` itself.

mod helpers;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::Result;
use async_trait::async_trait;
use axum::body::Body;
use axum::http::Request;
use axum::response::IntoResponse;
use helpers::recording_bus::test_state_with_bus;
use helpers::{body_json, make_event_with_time, send_request, test_router};
use open_story::cloud_event::CloudEvent;
use open_story::server::{consumers, SharedState};
use open_story_bus::{Bus, BusSubscription, IngestBatch};
use open_story_server::boot;
use open_story_store::persistence::SessionStore;
use serde_json::{json, Value};

/// The boot phase is process-wide; specs that set it must not overlap.
fn serial() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// A bus that records every publish and hands `events.*` batches to the
/// node's own consumers, as NATS would.
struct LocalBus {
    ingest: tokio::sync::mpsc::Sender<IngestBatch>,
    published: Mutex<Vec<(String, IngestBatch)>>,
}

impl LocalBus {
    fn under(&self, prefix: &str) -> Vec<(String, IngestBatch)> {
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
struct Node {
    state: SharedState,
    bus: Arc<LocalBus>,
    base: String,
    partitioned: Arc<AtomicBool>,
    _tmp: tempfile::TempDir,
}

impl Node {
    async fn start() -> Node {
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
        let mut proj =
            consumers::projections::ProjectionsConsumer::new(event_store, projections, parents, children);
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
    async fn observe(&self, host: &str, session: &str, ids: &[&str]) {
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
        let batch = IngestBatch {
            session_id: session.to_string(),
            project_id: "proj-1".to_string(),
            events,
        };
        self.bus
            .publish(&format!("events.{session}"), &batch)
            .await
            .unwrap();
    }

    async fn get(&self, path: &str) -> Value {
        let req = Request::get(path).body(Body::empty()).unwrap();
        body_json(send_request(self.state.clone(), req).await).await
    }

    async fn post(&self, hand: &str, body: Value) -> (u16, Value) {
        let req = Request::post(format!("/api/ops/{hand}"))
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();
        let resp = send_request(self.state.clone(), req).await;
        let status = resp.status().as_u16();
        (status, body_json(resp).await)
    }

    async fn rollup(&self) -> Value {
        self.get("/api/digests?rollup=1").await["rollup"].clone()
    }

    /// Eventually consistent: wait for the store to hold `n` events in all.
    async fn settle(&self, n: u64) {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let r = self.rollup().await;
            if r["events"] == json!(n) {
                return;
            }
            assert!(Instant::now() < deadline, "store never reached {n} events: {r}");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }
}

/// Two nodes, partitioned, each observing its own history plus one shared
/// session that diverges (each side holds an event the other lacks).
async fn partitioned_pair() -> (Node, Node) {
    let a = Node::start().await;
    let b = Node::start().await;
    a.partitioned.store(true, Ordering::SeqCst);
    b.partitioned.store(true, Ordering::SeqCst);
    a.observe("node-a", "sess-a", &["a1", "a2", "a3"]).await;
    a.observe("node-a", "shared", &["s1", "s2"]).await;
    b.observe("node-b", "sess-b", &["b1", "b2"]).await;
    b.observe("node-b", "shared", &["s1", "s3"]).await;
    a.settle(5).await;
    b.settle(4).await;
    (a, b)
}

mod when_the_partition_holds {
    use super::*;

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn it_reports_the_peer_unreachable_and_changes_nothing() {
        let _serial = serial();
        boot::set_serving();
        let (a, b) = partitioned_pair().await;
        let before = a.rollup().await;
        let (status, body) = a
            .post(
                "converge",
                json!({"peers": [b.base], "idempotency_key": "k-part", "settle_ms": 0}),
            )
            .await;
        assert_eq!(status, 200, "{body}");
        let r = &body["result"];
        assert_eq!(r["converged"], false, "{r}");
        assert_eq!(r["changed"], false);
        assert_eq!(r["peers"][0]["url"], b.base);
        assert_eq!(r["peers"][0]["reachable"], false);
        assert_eq!(r["rounds"].as_array().unwrap().len(), 1, "one round, nothing to do");
        assert_eq!(a.rollup().await, before, "nothing moved");
        assert_ne!(a.rollup().await["digest"], b.rollup().await["digest"]);
        assert_eq!(a.bus.under("ops.command.converge").len(), 1);
    }
}

mod when_the_partition_heals {
    use super::*;

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn converge_on_each_node_reaches_equal_rollups_within_two_rounds() {
        let _serial = serial();
        boot::set_serving();
        let (a, b) = partitioned_pair().await;
        a.partitioned.store(false, Ordering::SeqCst);
        b.partitioned.store(false, Ordering::SeqCst);

        // A pulls what B has: round 1 heals sess-b and the diverged shared
        // session; round 2 changes nothing and stops. A now holds the
        // union; B is still short of A's sessions, so not yet converged.
        let (status, body) = a
            .post(
                "converge",
                json!({"peers": [b.base], "idempotency_key": "k-a", "settle_ms": 50}),
            )
            .await;
        assert_eq!(status, 200, "{body}");
        let r = &body["result"];
        let rounds = r["rounds"].as_array().unwrap();
        assert!(rounds.len() <= 2, "{r}");
        assert_eq!(rounds[0]["healed"][&b.base], 2, "sess-b and shared: {r}");
        assert_eq!(r["changed"], true);
        assert_eq!(r["peers"][0]["reachable"], true);
        assert_eq!(r["converged"], false, "B still lacks sess-a: {r}");
        assert_eq!(r["peers"][0]["missing_there"], 1, "{r}");
        a.settle(8).await;
        assert_eq!(a.rollup().await["events"], 8, "3 + 2 + {{s1,s2,s3}}");

        // B pulls what A has; now the sets are equal and it says so.
        let (status, body) = b
            .post(
                "converge",
                json!({"peers": [a.base], "idempotency_key": "k-b", "settle_ms": 50}),
            )
            .await;
        assert_eq!(status, 200, "{body}");
        let r = &body["result"];
        assert!(r["rounds"].as_array().unwrap().len() <= 2, "{r}");
        assert_eq!(r["converged"], true, "{r}");
        assert_eq!(r["changed"], true);
        b.settle(8).await;
        assert_eq!(a.rollup().await, b.rollup().await, "equal sets, equal roll-ups");

        // A third call changes nothing and says so, and is not a replay.
        let (_, body) = a
            .post(
                "converge",
                json!({"peers": [b.base], "idempotency_key": "k-a2", "settle_ms": 0}),
            )
            .await;
        assert_eq!(body["replayed"], false, "{body}");
        assert_eq!(body["result"]["changed"], false);
        assert_eq!(body["result"]["converged"], true);
        assert_eq!(body["result"]["rounds"].as_array().unwrap().len(), 1);
        let round = &body["result"]["rounds"][0];
        assert_eq!(round["reprojected"], 0);
        assert_eq!(round["deleted"], 0);
        assert_eq!(round["verify"]["agree"], true);

        // The same key again is a replay: no act, the recorded answer.
        let (_, again) = a
            .post(
                "converge",
                json!({"peers": [b.base], "idempotency_key": "k-a2", "settle_ms": 0}),
            )
            .await;
        assert_eq!(again["replayed"], true, "{again}");

        // Each act left one command; no hand published to events.*: every
        // events.* publish on A is the catch-up re-injection of history B
        // already held (ids present on B), not a new event body.
        assert_eq!(a.bus.under("ops.command.converge").len(), 3);
        let b_ids: Vec<String> = b.get("/api/digests").await["sessions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["session_id"].as_str().unwrap().to_string())
            .collect();
        for (subject, batch) in a.bus.under("events.") {
            if ["sess-a", "shared"].contains(&batch.session_id.as_str())
                && subject == format!("events.{}", batch.session_id)
                && batch.events.iter().all(|e| e.host.as_deref() == Some("node-a"))
            {
                continue; // A's own observed history from the fixture watcher
            }
            assert!(
                b_ids.contains(&batch.session_id),
                "an events.* publish that is not observed history: {subject}"
            );
        }
    }
}

mod when_no_peer_is_given {
    use super::*;
    use open_story_store::event_store::PresenceRow;

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn it_reads_peers_from_presence_and_reports_the_unknown() {
        let _serial = serial();
        boot::set_serving();
        let (a, b) = partitioned_pair().await;
        a.partitioned.store(false, Ordering::SeqCst);
        b.partitioned.store(false, Ordering::SeqCst);
        let store = a.state.read().await.store.event_store.clone();
        let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string();
        for (host, body) in [
            ("node-b", json!({"host": "node-b", "api_url": b.base})),
            ("node-c", json!({"host": "node-c"})),
            (open_story_core::host::host(), json!({"host": open_story_core::host::host(), "api_url": a.base})),
        ] {
            store
                .upsert_presence(&PresenceRow {
                    host: host.to_string(),
                    principal_id: "dev".into(),
                    person_id: None,
                    time: now.clone(),
                    body,
                })
                .await
                .unwrap();
        }
        let (status, body) = a
            .post(
                "converge",
                json!({"idempotency_key": "k-presence", "settle_ms": 50}),
            )
            .await;
        assert_eq!(status, 200, "{body}");
        let r = &body["result"];
        assert_eq!(r["peers"][0]["url"], b.base, "the one with a URL is attempted: {r}");
        assert_eq!(r["peers"].as_array().unwrap().len(), 1, "self is never a peer: {r}");
        assert_eq!(r["unknown_peers"], json!(["node-c"]), "no URL, reported not attempted: {r}");
        assert_eq!(r["changed"], true);
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn it_says_so_when_nobody_is_known() {
        let _serial = serial();
        boot::set_serving();
        let a = Node::start().await;
        let (status, body) = a
            .post("converge", json!({"idempotency_key": "k-nobody"}))
            .await;
        assert_eq!(status, 400, "{body}");
        assert!(body["error"].as_str().unwrap().contains("peer"), "{body}");
        assert!(a.bus.under("ops.command.").is_empty(), "no act, no command");
    }
}

/// The node advertises the URL peers can reach it on, when configured.
mod when_an_advertise_url_is_configured {
    use super::*;

    #[tokio::test]
    async fn the_health_body_and_so_the_beat_carry_it() {
        let a = Node::start().await;
        let health = a.get("/api/health").await;
        assert!(health["api_url"].is_null(), "unset means null: {}", health["api_url"]);
        a.state.write().await.config.advertise_url = "http://a.example:3002/".to_string();
        let health = a.get("/api/health").await;
        assert_eq!(health["api_url"], "http://a.example:3002", "trimmed of its slash");
    }
}
