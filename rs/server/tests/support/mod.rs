//! Shared test support for the server's memory hands tests: a recording bus,
//! a golden-seeded store, and a router with an Admin principal.

#![allow(dead_code)]

use axum::Router;
use open_story_bus::Bus;
use open_story_server::config::Config;
use open_story_server::directory::{EmbeddedRoleDirectory, Participant, Role, RoleDirectory};
use open_story_server::router::build_router;
use open_story_server::state::AppState;
use open_story_server::watcher_diagnostics::WatcherDiagnostics;
use open_story_store::event_store::{EventStore, SessionRow};
use open_story_store::state::StoreState;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::{broadcast, RwLock};

/// A bus that records `publish_bytes` calls and does nothing else.
#[derive(Default)]
pub struct RecordingBus {
    pub published: Arc<Mutex<Vec<(String, Vec<u8>)>>>,
}

impl RecordingBus {
    pub fn subjects(&self) -> Vec<String> {
        self.published
            .lock()
            .unwrap()
            .iter()
            .map(|(s, _)| s.clone())
            .collect()
    }
    pub fn payloads(&self) -> Vec<serde_json::Value> {
        self.published
            .lock()
            .unwrap()
            .iter()
            .filter_map(|(_, b)| serde_json::from_slice(b).ok())
            .collect()
    }
}

#[async_trait::async_trait]
impl Bus for RecordingBus {
    async fn publish(
        &self,
        _subject: &str,
        _batch: &open_story_bus::IngestBatch,
    ) -> anyhow::Result<()> {
        Ok(())
    }
    async fn publish_bytes(&self, subject: &str, data: &[u8]) -> anyhow::Result<()> {
        self.published
            .lock()
            .unwrap()
            .push((subject.to_string(), data.to_vec()));
        Ok(())
    }
    async fn subscribe(&self, _pattern: &str) -> anyhow::Result<open_story_bus::BusSubscription> {
        let (_tx, rx) = tokio::sync::mpsc::channel(1);
        Ok(open_story_bus::BusSubscription { receiver: rx })
    }
    async fn replay(&self, _pattern: &str) -> anyhow::Result<Vec<open_story_bus::IngestBatch>> {
        Ok(vec![])
    }
    fn is_active(&self) -> bool {
        false
    }
}

pub fn golden_events(name: &str) -> Vec<serde_json::Value> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../patterns/tests/fixtures/story")
        .join(name)
        .join("events.jsonl");
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

pub fn golden_expected(name: &str) -> serde_json::Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../patterns/tests/fixtures/story")
        .join(name)
        .join("expected.json");
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

/// Seed a golden's events and its folded story patterns; returns the session id.
pub async fn seed_golden(store: &Arc<dyn EventStore>, name: &str) -> String {
    let events = golden_events(name);
    let sid = events[0]["data"]["session_id"]
        .as_str()
        .unwrap()
        .to_string();
    store.insert_batch(&sid, &events).await.unwrap();
    let row: SessionRow =
        serde_json::from_value(serde_json::json!({ "id": sid, "event_count": events.len() }))
            .unwrap();
    store.upsert_session(&row).await.unwrap();
    for p in open_story_server::story_backfill::fold_session(&events, 1800) {
        store.insert_pattern(&sid, &p).await.unwrap();
    }
    sid
}

pub struct TestApp {
    pub router: Router,
    pub store: Arc<dyn EventStore>,
    pub bus: Arc<RecordingBus>,
    pub api_token: String,
    pub admin_token: String,
    _tmp: tempfile::TempDir,
}

/// A router over a temp store with the given goldens seeded, a recording
/// bus, and an Admin local principal. Tokens: api-secret / admin-secret.
pub async fn app_with(goldens: &[&str]) -> TestApp {
    let tmp = tempfile::tempdir().unwrap();
    let store_state = StoreState::new(tmp.path()).unwrap();
    let store = store_state.event_store.clone();
    for g in goldens {
        seed_golden(&store, g).await;
    }
    let (broadcast_tx, _) = broadcast::channel(256);
    let mut config = Config::default();
    config.api_token = "api-secret".to_string();
    config.admin_token = "admin-secret".to_string();
    config.local_principal_id = "test-principal".to_string();
    let initial_topology = open_story_server::admin::compute_topology(
        "test-host",
        config.role,
        &open_story_server::admin::EnvInputs::default(),
        &[],
    );
    let (admin_topology_tx, _) = tokio::sync::watch::channel(initial_topology);
    let dir = EmbeddedRoleDirectory::in_memory().unwrap();
    dir.upsert_participant(Participant {
        principal_id: "test-principal".into(),
        person_id: "max".into(),
        role: Role::Admin,
        created_at: "2026-09-18T00:00:00Z".into(),
    })
    .await
    .unwrap();
    let bus = Arc::new(RecordingBus::default());
    let state = Arc::new(RwLock::new(AppState {
        store: store_state,
        transcript_states: HashMap::new(),
        watcher_diagnostics: WatcherDiagnostics::default(),
        broadcast_tx,
        bus: bus.clone(),
        admin_topology_tx,
        config: config.clone(),
        watch_dir: tmp.path().join("watch"),
        account_config_writer: None,
        account_config_reloader: None,
        role_directory: Arc::new(dir),
    }));
    TestApp {
        router: build_router(state, None, &config),
        store,
        bus,
        api_token: "api-secret".into(),
        admin_token: "admin-secret".into(),
        _tmp: tmp,
    }
}
