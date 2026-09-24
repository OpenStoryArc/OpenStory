//! A bus that remembers every publish, so a spec can read back what a node
//! put on the wire (presence beats, ops commands) without NATS.

use std::sync::{Arc, Mutex};

use anyhow::Result;
use async_trait::async_trait;
use open_story_bus::{Bus, BusSubscription, IngestBatch};

#[derive(Default)]
pub struct RecordingBus {
    published: Mutex<Vec<(String, IngestBatch)>>,
}

impl RecordingBus {
    pub fn published(&self) -> Vec<(String, IngestBatch)> {
        self.published.lock().unwrap().clone()
    }

    /// Publishes whose subject starts with `prefix`.
    pub fn under(&self, prefix: &str) -> Vec<(String, IngestBatch)> {
        self.published()
            .into_iter()
            .filter(|(s, _)| s.starts_with(prefix))
            .collect()
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

/// An isolated state whose bus is `bus`.
pub fn test_state_with_bus(
    tmp: &tempfile::TempDir,
    bus: Arc<dyn Bus>,
) -> open_story::server::SharedState {
    use open_story::server::{AppState, Config};
    let store = open_story_store::state::StoreState::new(tmp.path()).unwrap();
    let (broadcast_tx, _) = tokio::sync::broadcast::channel(256);
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
    Arc::new(tokio::sync::RwLock::new(AppState {
        store,
        transcript_states: std::collections::HashMap::new(),
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
