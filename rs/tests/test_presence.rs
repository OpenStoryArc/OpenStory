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
}
