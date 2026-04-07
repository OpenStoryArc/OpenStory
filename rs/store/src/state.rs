//! StoreState — owns event storage, projections, patterns, and project resolution.
//!
//! This is the store-owned subset of what was previously AppState. The server
//! composes StoreState with server-specific fields (broadcast_tx, transcript_states, bus).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Result;

use open_story_patterns::{PatternEvent, PatternPipeline};

use crate::event_store::EventStore;
use crate::persistence::{EventLog, SessionStore};
use crate::plan_store::PlanStore;
use crate::projection::SessionProjection;
use crate::sqlite_store::SqliteStore;

/// Store state — event storage, projections, patterns, and project resolution.
pub struct StoreState {
    // ── event store (SQLite default, JSONL fallback) ──
    // Arc-wrapped so multiple actor-consumers can hold a reference
    // without a shared RwLock. SQLite handles internal locking.
    pub event_store: Arc<dyn EventStore>,

    // ── dedup ──
    pub seen_event_ids: HashSet<String>,
    pub session_store: SessionStore,
    pub event_log: EventLog,
    pub plan_store: PlanStore,

    // ── projections + patterns ──
    pub projections: HashMap<String, SessionProjection>,
    pub pattern_pipelines: HashMap<String, PatternPipeline>,
    pub detected_patterns: HashMap<String, Vec<PatternEvent>>,
    pub agent_labels: HashMap<String, String>,
    pub full_payloads: HashMap<String, HashMap<String, String>>,

    // ── subagent parent-child index ──
    /// Subagent session_id → parent session_id
    pub subagent_parents: HashMap<String, String>,
    /// Parent session_id → list of subagent session_ids
    pub session_children: HashMap<String, Vec<String>>,

    // ── project resolution ──
    pub session_projects: HashMap<String, String>,
    pub session_project_names: HashMap<String, String>,
    pub watch_dir_entries: Vec<String>,

    // ── configuration ──
    pub data_dir: PathBuf,
}

impl StoreState {
    /// Create a new empty StoreState backed by the given data directory.
    ///
    /// Tries SQLite first. Falls back to JSONL if SQLite fails.
    pub fn new(data_dir: &Path) -> Result<Self> {
        Self::new_with_key(data_dir, None)
    }

    /// Create a new StoreState with an optional SQLCipher encryption key.
    ///
    /// If `key` is Some and non-empty, the SQLite database is encrypted.
    /// Empty or None key = unencrypted (backward compatible).
    pub fn new_with_key(data_dir: &Path, key: Option<&str>) -> Result<Self> {
        let plans_dir = data_dir.join("plans");
        std::fs::create_dir_all(&plans_dir)?;

        let session_store = SessionStore::new(data_dir)?;
        let event_log = EventLog::new(data_dir)?;
        let plan_store = PlanStore::new(&plans_dir)?;

        // Try SQLite (with optional encryption), fall back to JSONL
        // Arc-wrapped so multiple actor-consumers can share the store.
        let event_store: Arc<dyn EventStore> = match SqliteStore::new_with_key(data_dir, key) {
            Ok(store) => Arc::new(store),
            Err(e) => {
                eprintln!("SQLite unavailable ({}), falling back to JSONL store", e);
                let fallback_session_store = SessionStore::new(data_dir)?;
                let fallback_event_log = EventLog::new(data_dir)?;
                Arc::new(crate::jsonl_store::JsonlStore::new(
                    fallback_session_store,
                    fallback_event_log,
                ))
            }
        };

        Ok(Self {
            event_store,
            seen_event_ids: HashSet::new(),
            session_store,
            event_log,
            plan_store,
            projections: HashMap::new(),
            pattern_pipelines: HashMap::new(),
            detected_patterns: HashMap::new(),
            agent_labels: HashMap::new(),
            full_payloads: HashMap::new(),
            subagent_parents: HashMap::new(),
            session_children: HashMap::new(),
            session_projects: HashMap::new(),
            session_project_names: HashMap::new(),
            watch_dir_entries: Vec::new(),
            data_dir: data_dir.to_path_buf(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn new_creates_empty_store() {
        let tmp = TempDir::new().unwrap();
        let state = StoreState::new(tmp.path()).unwrap();

        assert!(state.event_store.list_sessions().await.unwrap().is_empty());
        assert!(state.seen_event_ids.is_empty());
        assert!(state.projections.is_empty());
        assert!(state.pattern_pipelines.is_empty());
        assert!(state.detected_patterns.is_empty());
        assert!(state.agent_labels.is_empty());
        assert!(state.full_payloads.is_empty());
        assert!(state.subagent_parents.is_empty());
        assert!(state.session_children.is_empty());
        assert!(state.session_projects.is_empty());
        assert!(state.session_project_names.is_empty());
        assert!(state.watch_dir_entries.is_empty());
        assert_eq!(state.data_dir, tmp.path());
    }

    #[test]
    fn new_creates_plans_subdirectory() {
        let tmp = TempDir::new().unwrap();
        let _state = StoreState::new(tmp.path()).unwrap();

        assert!(
            tmp.path().join("plans").exists(),
            "StoreState::new should create plans/ subdirectory"
        );
    }

    #[tokio::test]
    async fn ingest_event_into_store_state() {
        let tmp = TempDir::new().unwrap();
        let mut state = StoreState::new(tmp.path()).unwrap();

        // Simulate what ingest_events does: dedup, persist, project.
        // Event shape mirrors the typed EventData → AgentPayload model the
        // production code now expects (post-refactor): seq + session_id at
        // the data level, text inside the agent_payload.
        let event = serde_json::json!({
            "specversion": "1.0",
            "id": "evt-1",
            "type": "io.arc.event",
            "subtype": "message.user.prompt",
            "source": "arc://test",
            "time": "2025-01-14T00:00:00Z",
            "datacontenttype": "application/json",
            "data": {
                "raw": {"type": "user", "message": {"content": [{"type": "text", "text": "hello"}]}},
                "seq": 1,
                "session_id": "sess-1",
                "agent_payload": {
                    "_variant": "claude-code",
                    "meta": {"agent": "claude-code"},
                    "text": "hello"
                }
            }
        });

        let event_id = "evt-1";
        assert!(state.seen_event_ids.insert(event_id.to_string()), "first insert should succeed");
        assert!(!state.seen_event_ids.insert(event_id.to_string()), "dedup should reject duplicate");

        // Persist via EventStore
        assert!(state.event_store.insert_event("sess-1", &event).await.unwrap());
        assert!(!state.event_store.insert_event("sess-1", &event).await.unwrap(), "dedup via PK");

        let proj = state.projections.entry("sess-1".to_string())
            .or_insert_with(|| SessionProjection::new("sess-1"));
        let result = proj.append(&event);

        let stored = state.event_store.session_events("sess-1").await.unwrap();
        assert_eq!(stored.len(), 1);
        assert_eq!(proj.event_count(), 1);
        assert!(!result.is_empty());

        // Verify round-trip
        assert_eq!(stored[0]["id"].as_str(), Some("evt-1"));
    }

    #[test]
    fn new_persistence_is_functional() {
        let tmp = TempDir::new().unwrap();
        let state = StoreState::new(tmp.path()).unwrap();

        // SessionStore should be able to list sessions (empty)
        assert!(state.session_store.list_sessions().is_empty());

        // PlanStore should be able to list plans (empty)
        assert!(state.plan_store.list_plans().is_empty());
    }
}
