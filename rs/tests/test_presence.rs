//! P-01: the node publishes a `presence` CloudEvent on its own subject at a
//! configurable interval, carrying the health payload (H-04 to H-07) with
//! `agent: "openstory"` and `subtype: node.presence`. Scoreboard:
//! docs/research/openstory-as-node/REQUIREMENTS.md
//!
//! The bus is a recording double: no NATS in this binary. The spec asserts
//! on what was published, not on the transport.

mod helpers;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Result;
use async_trait::async_trait;
use open_story::server::config::{Person, Principal, PrincipalMatchers};
use open_story::server::{presence, AppState, Config, SharedState};
use open_story_bus::{Bus, BusSubscription, IngestBatch};
use open_story_store::state::StoreState;
use tokio::sync::{broadcast, RwLock};

/// A bus that remembers every publish, so a spec can read them back.
#[derive(Default)]
struct RecordingBus {
    published: Mutex<Vec<(String, IngestBatch)>>,
}

impl RecordingBus {
    fn published(&self) -> Vec<(String, IngestBatch)> {
        self.published.lock().unwrap().clone()
    }
}

#[async_trait]
impl Bus for RecordingBus {
    async fn publish(&self, subject: &str, batch: &IngestBatch) -> Result<()> {
        self.published
            .lock()
            .unwrap()
            .push((subject.to_string(), batch.clone()));
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

/// An isolated state whose bus is the recording double and whose person
/// has one principal matching this host.
fn state_with(bus: Arc<RecordingBus>, tmp: &tempfile::TempDir) -> SharedState {
    let store = StoreState::new(tmp.path()).unwrap();
    let (broadcast_tx, _) = broadcast::channel(256);
    let watch_dir = tmp.path().join("watch");
    std::fs::create_dir_all(&watch_dir).unwrap();
    let person = Some(Person {
        id: "person-1".to_string(),
        display_name: "Test person".to_string(),
        email: String::new(),
        principals: vec![
            Principal {
                id: "elsewhere".to_string(),
                display_name: "Another machine".to_string(),
                matchers: PrincipalMatchers {
                    host: Some("not-this-host".to_string()),
                    ..Default::default()
                },
            },
            Principal {
                id: "this-node".to_string(),
                display_name: "This machine".to_string(),
                matchers: PrincipalMatchers {
                    host: Some(open_story_core::host::host().to_string()),
                    ..Default::default()
                },
            },
        ],
    });
    let config = Config {
        person,
        ..Config::default()
    };
    let topology = open_story::server::admin::compute_topology(
        "test-host",
        config.role,
        &open_story::server::admin::EnvInputs::default(),
        &[],
    );
    let (admin_topology_tx, _) = tokio::sync::watch::channel(topology);
    Arc::new(RwLock::new(AppState {
        store,
        transcript_states: HashMap::new(),
        watcher_diagnostics: open_story::server::watcher_diagnostics::WatcherDiagnostics::default(),
        broadcast_tx,
        bus,
        admin_topology_tx,
        config,
        watch_dir,
        account_config_writer: None,
        account_config_reloader: None,
        role_directory: Arc::new(open_story::server::directory::NoopRoleDirectory),
    }))
}

mod when_the_node_runs {
    use super::*;

    #[tokio::test]
    async fn it_publishes_presence_on_its_subject() {
        let tmp = tempfile::tempdir().unwrap();
        let bus = Arc::new(RecordingBus::default());
        let state = state_with(bus.clone(), &tmp);

        let handle = presence::spawn(state.clone(), Duration::from_millis(20));

        // Eventually consistent: poll for two beats, which proves the
        // interval, not just a first publish.
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        while bus.published().len() < 2 && std::time::Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        handle.abort();
        let published = bus.published();
        assert!(
            published.len() >= 2,
            "expected at least two presence beats, got {}",
            published.len()
        );

        let host = open_story_core::host::host();
        let expected = presence::subject(host, "this-node");
        assert!(expected.starts_with("presence."), "{expected}");
        assert!(
            !expected.starts_with("events."),
            "presence never rides the observed stream"
        );
        let (subject, batch) = &published[0];
        assert_eq!(
            subject, &expected,
            "the principal matching this host names the subject"
        );

        assert_eq!(batch.events.len(), 1, "one presence event per beat");
        let ce = &batch.events[0];
        assert_eq!(ce.agent.as_deref(), Some("openstory"));
        assert_eq!(ce.subtype.as_deref(), Some("node.presence"));
        assert_eq!(ce.host.as_deref(), Some(host));
        assert_eq!(ce.principal_id.as_deref(), Some("this-node"));
        assert_eq!(ce.person_id.as_deref(), Some("person-1"));

        // The H-04 to H-07 payload rides in the event body.
        let raw = &ce.data.raw;
        for key in [
            "boot",
            "git_sha",
            "built_at",
            "process",
            "store",
            "bus",
            "streams",
            "consumers",
            "watchers_detail",
        ] {
            assert!(
                raw.get(key).is_some(),
                "presence payload carries `{key}`: {raw}"
            );
        }
        assert_eq!(raw["host"], host, "the payload names its host");
        assert_eq!(raw["principal_id"], "this-node");

        // Every beat is a fresh event, never a replay of the last one.
        let (_, second) = &published[1];
        assert_ne!(ce.id, second.events[0].id, "each beat has its own event id");
    }
}

mod when_the_subject_is_built {
    use super::*;

    #[test]
    fn it_keeps_nats_tokens_clean() {
        // Dots, spaces, and wildcards would split or widen the subject.
        assert_eq!(
            presence::subject("Maxs Air.local", "p 1"),
            "presence.Maxs-Air-local.p-1"
        );
        assert_eq!(presence::subject("hub", "a*b>c"), "presence.hub.a-b-c");
        assert_eq!(presence::subject("", ""), "presence.unknown.unknown");
    }
}

mod when_presence_arrives {
    use super::*;
    use helpers::bus::TestActors;

    /// P-02: the persist consumer routes presence to its own table. It never
    /// becomes a session, an event row, or a JSONL line.
    #[tokio::test]
    async fn it_lands_in_the_presence_table_not_events() {
        let tmp = tempfile::tempdir().unwrap();
        let mut actors = TestActors::new(&tmp).await;
        let host = "node-a";
        let beat = |sha: &str| {
            presence::presence_event(
                host,
                Some("person-1"),
                "this-node",
                serde_json::json!({"status": "ok", "git_sha": sha, "streams": []}),
            )
        };

        let session = format!("presence:{host}");
        let first = actors
            .persist
            .process_batch(&session, &[beat("aaa111")], Some(presence::SOURCE))
            .await;
        assert_eq!(first.persisted, 1, "the beat is stored");
        let second = actors
            .persist
            .process_batch(&session, &[beat("bbb222")], Some(presence::SOURCE))
            .await;
        assert_eq!(second.persisted, 1);

        let store = actors.state.read().await.store.event_store.clone();
        let rows = store.latest_presence().await.unwrap();
        assert_eq!(rows.len(), 1, "one row per node, the latest beat: {rows:?}");
        let row = &rows[0];
        assert_eq!(row.host, host);
        assert_eq!(row.principal_id, "this-node");
        assert_eq!(row.person_id.as_deref(), Some("person-1"));
        assert_eq!(row.body["git_sha"], "bbb222", "the latest beat wins");
        assert!(!row.time.is_empty(), "the beat's time is kept");

        assert!(
            store.session_events(&session).await.unwrap().is_empty(),
            "presence never lands in events"
        );
        assert!(
            store
                .list_sessions()
                .await
                .unwrap()
                .iter()
                .all(|s| s.id != session),
            "presence never becomes a session"
        );
        let jsonl = tmp.path().join(format!("{session}.jsonl"));
        assert!(!jsonl.exists(), "presence never reaches the JSONL backup");
    }

    /// D-02: every beat also lands as one compact line in the data
    /// directory's presence log, the history DORA reads (the table keeps
    /// only the latest beat per node).
    #[tokio::test]
    async fn it_also_appends_to_the_presence_log() {
        let tmp = tempfile::tempdir().unwrap();
        let mut actors = TestActors::new(&tmp).await;
        let beat = |sha: &str, level: &str| {
            presence::presence_event(
                "node-a",
                Some("person-1"),
                "this-node",
                serde_json::json!({
                    "status": "ok",
                    "git_sha": sha,
                    "built_at": "2026-09-24T00:00:00Z",
                    "verdict": {"level": level, "findings": [{"id": "leaf_down", "level": level, "text": "x"}]},
                }),
            )
        };
        let first = beat("aaa111", "critical");
        let t1 = first.time.clone();
        actors
            .persist
            .process_batch("presence:node-a", &[first], Some(presence::SOURCE))
            .await;
        actors
            .persist
            .process_batch(
                "presence:node-a",
                &[beat("bbb222", "ok")],
                Some(presence::SOURCE),
            )
            .await;

        let data_dir = actors.state.read().await.store.data_dir.clone();
        let log = data_dir.join("presence.jsonl");
        let text = std::fs::read_to_string(&log).expect("presence.jsonl exists");
        let lines: Vec<serde_json::Value> = text
            .lines()
            .map(|l| serde_json::from_str(l).expect("one JSON object per line"))
            .collect();
        assert_eq!(lines.len(), 2, "{text}");
        assert_eq!(lines[0]["host"], "node-a");
        assert_eq!(lines[0]["principal_id"], "this-node");
        assert_eq!(lines[0]["git_sha"], "aaa111");
        assert_eq!(lines[0]["built_at"], "2026-09-24T00:00:00Z");
        assert_eq!(lines[0]["level"], "critical");
        assert_eq!(lines[0]["findings"], serde_json::json!(["leaf_down"]));
        assert_eq!(lines[0]["time"], t1, "the beat's own time");
        assert_eq!(lines[1]["git_sha"], "bbb222");
        assert_eq!(lines[1]["level"], "ok");
        assert!(
            lines[0].get("streams").is_none(),
            "compact: not the whole body"
        );
    }
}

mod when_a_node_stops_reporting {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use helpers::{body_json, send_request, test_state};
    use open_story_store::event_store::PresenceRow;

    fn beat(host: &str, seconds_ago: i64, sha: &str) -> PresenceRow {
        let time = (chrono::Utc::now() - chrono::Duration::seconds(seconds_ago))
            .format("%Y-%m-%dT%H:%M:%S%.3fZ")
            .to_string();
        PresenceRow {
            host: host.to_string(),
            principal_id: "dev".to_string(),
            person_id: Some("person-1".to_string()),
            time,
            body: serde_json::json!({"status": "ok", "git_sha": sha}),
        }
    }

    /// P-03 (pure): a node older than three intervals is stale.
    #[test]
    fn it_is_stale_past_three_intervals() {
        // Rows first, then `now`, so every age is at least what the fixture
        // says (a beat built after `now` could truncate 46 s to 45 s).
        let rows = vec![
            beat("fresh", 5, "aaa"),
            beat("quiet", 46, "bbb"),
            beat("gone", 3600, "ccc"),
        ];
        let now = chrono::Utc::now();
        let view = presence::fleet_view(&rows, now, 15);
        assert_eq!(view.len(), 3);
        let by_host = |h: &str| view.iter().find(|n| n["host"] == h).cloned().unwrap();
        let fresh = by_host("fresh");
        assert_eq!(fresh["stale"], false);
        assert!(
            (4..=6).contains(&fresh["age_secs"].as_i64().unwrap()),
            "{fresh}"
        );
        let quiet = by_host("quiet");
        assert_eq!(quiet["stale"], true, "46 s is past 3 × 15 s: {quiet}");
        let gone = by_host("gone");
        assert_eq!(gone["stale"], true);
        assert_eq!(
            gone["git_sha"], "ccc",
            "the beat's sha is lifted to the top level"
        );
        assert_eq!(gone["principal_id"], "dev");
        assert_eq!(gone["person_id"], "person-1");
        assert_eq!(
            gone["body"]["status"], "ok",
            "the whole beat still rides along"
        );
    }

    /// P-03 (edge): an unparseable time is stale with no age, never a crash.
    #[test]
    fn it_treats_an_unreadable_time_as_stale() {
        let mut row = beat("odd", 0, "ddd");
        row.time = "not a time".to_string();
        let view = presence::fleet_view(&[row], chrono::Utc::now(), 15);
        assert_eq!(view[0]["stale"], true);
        assert!(view[0]["age_secs"].is_null(), "{}", view[0]);
    }

    /// P-03 (endpoint): the fleet reads the latest beat per node with ages.
    #[tokio::test]
    async fn it_becomes_stale() {
        let tmp = tempfile::tempdir().unwrap();
        let state = test_state(&tmp);
        let store = state.read().await.store.event_store.clone();
        store
            .upsert_presence(&beat("fresh", 5, "aaa"))
            .await
            .unwrap();
        store
            .upsert_presence(&beat("gone", 600, "bbb"))
            .await
            .unwrap();

        let req = Request::get("/api/fleet/presence")
            .body(Body::empty())
            .unwrap();
        let resp = send_request(state, req).await;
        assert_eq!(resp.status(), 200);
        let body = body_json(resp).await;
        assert_eq!(
            body["interval_secs"], 15,
            "the configured beat interval: {body}"
        );
        let nodes = body["nodes"].as_array().unwrap();
        assert_eq!(nodes.len(), 2, "{body}");
        let fresh = nodes.iter().find(|n| n["host"] == "fresh").unwrap();
        assert_eq!(fresh["stale"], false);
        let gone = nodes.iter().find(|n| n["host"] == "gone").unwrap();
        assert_eq!(gone["stale"], true);
        assert!(gone["age_secs"].as_i64().unwrap() >= 599, "{gone}");
    }
}

/// P-06: a beat that fails to publish is logged in the E-05 shape and
/// counted, and ingestion never waits on it.
mod when_publish_fails {
    use super::*;
    use anyhow::Context;
    use helpers::bus::TestActors;
    use open_story_server::logging::{build_subscriber, LogFormat};
    use std::io::Write;

    #[derive(Clone, Default)]
    struct Capture(Arc<Mutex<Vec<u8>>>);
    struct CaptureWriter(Arc<Mutex<Vec<u8>>>);
    impl Write for CaptureWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Capture {
        type Writer = CaptureWriter;
        fn make_writer(&'a self) -> Self::Writer {
            CaptureWriter(self.0.clone())
        }
    }

    /// A bus whose every publish fails with a chained error.
    struct FailingBus;
    #[async_trait]
    impl Bus for FailingBus {
        async fn publish(&self, subject: &str, _batch: &IngestBatch) -> Result<()> {
            Err(anyhow::Error::from(std::io::Error::other(
                "nats: connection closed",
            )))
            .with_context(|| format!("failed to publish to {subject}"))
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

    /// A bus whose publish never returns.
    struct HangingBus;
    #[async_trait]
    impl Bus for HangingBus {
        async fn publish(&self, _subject: &str, _batch: &IngestBatch) -> Result<()> {
            std::future::pending::<()>().await;
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

    fn state_with_bus(bus: Arc<dyn Bus>, tmp: &tempfile::TempDir) -> SharedState {
        let store = StoreState::new(tmp.path()).unwrap();
        let (broadcast_tx, _) = broadcast::channel(256);
        let watch_dir = tmp.path().join("watch");
        std::fs::create_dir_all(&watch_dir).unwrap();
        let config = Config::default();
        let topology = open_story::server::admin::compute_topology(
            "test-host",
            config.role,
            &open_story::server::admin::EnvInputs::default(),
            &[],
        );
        let (admin_topology_tx, _) = tokio::sync::watch::channel(topology);
        Arc::new(RwLock::new(AppState {
            store,
            transcript_states: HashMap::new(),
            watcher_diagnostics:
                open_story::server::watcher_diagnostics::WatcherDiagnostics::default(),
            broadcast_tx,
            bus,
            admin_topology_tx,
            config,
            watch_dir,
            account_config_writer: None,
            account_config_reloader: None,
            role_directory: Arc::new(open_story::server::directory::NoopRoleDirectory),
        }))
    }

    async fn wait_for_failures(before: u64, want: u64) -> u64 {
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        while presence::stats().failures < before + want && std::time::Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        presence::stats().failures - before
    }

    #[tokio::test]
    async fn it_logs_and_continues() {
        let cap = Capture::default();
        let _g = tracing::subscriber::set_default(build_subscriber(
            LogFormat::Json,
            "info",
            cap.clone(),
        ));
        let tmp = tempfile::tempdir().unwrap();
        let state = state_with_bus(Arc::new(FailingBus), &tmp);
        let before = presence::stats().failures;

        let handle = presence::spawn(state.clone(), Duration::from_millis(20));
        let failed = wait_for_failures(before, 2).await;
        assert!(
            failed >= 2,
            "the beat keeps trying after a failure: {failed} failures"
        );

        // Ingestion never waited on the beat: the persist actor, driven
        // while the failing beat runs, still lands a batch.
        let other = tempfile::tempdir().unwrap();
        let mut actors = TestActors::new(&other).await;
        let ce =
            presence::presence_event("node-x", None, "dev", serde_json::json!({"status": "ok"}));
        let r = actors
            .persist
            .process_batch("presence:node-x", &[ce], Some(presence::SOURCE))
            .await;
        assert_eq!(
            r.persisted, 1,
            "ingestion continued while beats were failing"
        );
        handle.abort();

        // The E-05 shape, with the presence actor and subject.
        let text = String::from_utf8(cap.0.lock().unwrap().clone()).unwrap();
        let line = text
            .lines()
            .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
            .find(|l| l["event"] == "publish_failed" && l["actor"] == "presence")
            .unwrap_or_else(|| panic!("a presence publish_failed line in:\n{text}"));
        assert_eq!(line["level"], "WARN");
        assert!(
            line["subject"].as_str().unwrap().starts_with("presence."),
            "{line}"
        );
        assert_eq!(line["events"], 1);
        let error = line["error"].as_str().unwrap();
        assert!(
            error.contains("failed to publish to"),
            "context kept: {error}"
        );
        assert!(
            error.contains("connection closed"),
            "root cause kept: {error}"
        );

        // Counted, and on the health body for the fleet to see.
        let stats = presence::stats();
        assert!(stats.failures >= before + 2);
        assert!(stats.last_error.is_some());
        let (_status, body) = open_story_server::api::health_body(&state).await;
        assert!(
            body["presence"]["failures"].as_u64().unwrap() >= 2,
            "{}",
            body["presence"]
        );
        assert!(
            body["presence"]["last_error"].is_string(),
            "{}",
            body["presence"]
        );
        assert_eq!(body["presence"]["interval_secs"], 15);
    }

    #[tokio::test]
    async fn it_times_out_a_hung_publish_and_keeps_beating() {
        let tmp = tempfile::tempdir().unwrap();
        let state = state_with_bus(Arc::new(HangingBus), &tmp);
        let before = presence::stats().failures;

        let handle = presence::spawn_with_timeout(
            state,
            Duration::from_millis(20),
            Duration::from_millis(50),
        );
        let failed = wait_for_failures(before, 2).await;
        handle.abort();
        assert!(
            failed >= 2,
            "a hung publish is cut off and the beat goes on: {failed}"
        );
        let err = presence::stats().last_error.unwrap_or_default();
        assert!(
            err.contains("timed out") || err.contains("connection closed"),
            "the last error names the cause: {err}"
        );
    }
}
