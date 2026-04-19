//! StoreState — owns event storage, projections, patterns, and project resolution.
//!
//! This is the store-owned subset of what was previously AppState. The server
//! composes StoreState with server-specific fields (broadcast_tx, transcript_states, bus).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Result;
use dashmap::DashMap;

use open_story_patterns::PatternEvent;

use crate::event_store::EventStore;
use crate::persistence::{EventLog, SessionStore};
use crate::plan_store::PlanStore;
use crate::projection::SessionProjection;
use crate::sqlite_store::SqliteStore;

/// Detect a subagent → parent relationship from one event and update the maps.
///
/// The convention is that an event's `data.session_id` carries the **parent**
/// session id while the surrounding context (filename / hook envelope /
/// ingest call) carries the **subagent's own** session id. When they differ,
/// the subagent has a parent; record the relationship once per subagent.
///
/// Extracted 2026-04-15 from four byte-identical copies (audit walk):
///   - rs/server/src/ingest.rs:102 (live ingest)
///   - rs/server/src/ingest.rs:350 (replay_boot_sessions)
///   - rs/server/src/state.rs:138 (boot_from_sqlite)
///   - rs/server/src/consumers/projections.rs:78 (Actor 3, dead state)
///
/// All four had the same logic with different variable names — a refactor
/// that adds a third condition (e.g., explicit subagent flag) needs to land
/// in one place, not four.
pub fn detect_subagent_relationship(
    event: &serde_json::Value,
    own_session_id: &str,
    parents: &mut HashMap<String, String>,
    children: &mut HashMap<String, Vec<String>>,
) {
    let Some(data_sid) = event
        .get("data")
        .and_then(|d| d.get("session_id"))
        .and_then(|v| v.as_str())
    else {
        return;
    };
    if data_sid != own_session_id && !parents.contains_key(own_session_id) {
        parents.insert(own_session_id.to_string(), data_sid.to_string());
        children
            .entry(data_sid.to_string())
            .or_default()
            .push(own_session_id.to_string());
    }
}

/// Store state — event storage, projections, patterns, and project resolution.
pub struct StoreState {
    // ── event store (SQLite default, JSONL fallback) ──
    // Arc-wrapped so multiple actor-consumers can hold a reference
    // without a shared RwLock. SQLite handles internal locking.
    pub event_store: Arc<dyn EventStore>,

    pub session_store: SessionStore,
    pub event_log: EventLog,
    pub plan_store: PlanStore,

    // ── projections + patterns ──
    // Shared across actor-consumers (Actor 3 owns writes, API + ws +
    // other actors read). DashMap gives lock-free concurrent reads
    // without forcing all call sites onto an explicit RwLock guard.
    pub projections: Arc<DashMap<String, SessionProjection>>,
    /// Cache of detected patterns keyed by session_id, populated by the
    /// patterns consumer (Actor 2). Read by `build_initial_state` for the
    /// WebSocket handshake. Pattern *detection* lives in the patterns
    /// consumer; this is just the in-memory mirror it pushes into so the
    /// API/WebSocket layers can read it without a DB roundtrip.
    pub detected_patterns: HashMap<String, Vec<PatternEvent>>,
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

/// Persistence backend selector — independent of the server crate's
/// `Config` so the store crate stays standalone. The server's
/// `DataBackend` enum maps onto this at boot time.
#[derive(Debug, Clone)]
pub enum BackendChoice {
    /// SQLite (default, no extra deps).
    Sqlite,
    /// MongoDB. Requires `--features open-story-store/mongo` at build time.
    /// `uri` is a `mongodb://...` connection string.
    Mongo { uri: String, db_name: String },
}

impl StoreState {
    /// Create a new empty StoreState backed by SQLite at `data_dir`.
    ///
    /// Tries SQLite first. Falls back to JSONL if SQLite fails. This is
    /// the legacy entry point — kept for backward compatibility with the
    /// integration test suite. New callers should prefer `with_backend`.
    pub fn new(data_dir: &Path) -> Result<Self> {
        Self::new_with_key(data_dir, None)
    }

    /// Create a new StoreState with an optional SQLCipher encryption key.
    ///
    /// If `key` is Some and non-empty, the SQLite database is encrypted.
    /// Empty or None key = unencrypted (backward compatible).
    pub fn new_with_key(data_dir: &Path, key: Option<&str>) -> Result<Self> {
        let (event_store, session_store, event_log, plan_store) =
            init_sidecar_stores(data_dir, key)?;
        Ok(Self::assemble(
            event_store,
            session_store,
            event_log,
            plan_store,
            data_dir.to_path_buf(),
        ))
    }

    /// Create a StoreState with the chosen backend. This is the entry
    /// point used by `server::create_state` once `data_backend` is read
    /// from `Config`.
    ///
    /// SQLite path is sync-friendly (rusqlite is sync), but MongoStore
    /// boot is async (`MongoStore::connect` opens a TCP connection and
    /// runs index creation), so this constructor as a whole is async.
    /// `data_dir` is still required even when using Mongo because the
    /// JSONL backup, plans dir, and session store all live on disk
    /// regardless of which event store is durable.
    pub async fn with_backend(
        data_dir: &Path,
        key: Option<&str>,
        backend: BackendChoice,
    ) -> Result<Self> {
        // Plans, JSONL backup, and SessionStore live on disk for all
        // backends — they're the sovereignty escape hatch that survives
        // any database choice.
        let plans_dir = data_dir.join("plans");
        std::fs::create_dir_all(&plans_dir)?;
        let session_store = SessionStore::new(data_dir)?;
        let event_log = EventLog::new(data_dir)?;
        let plan_store = PlanStore::new(&plans_dir)?;

        let event_store: Arc<dyn EventStore> = match backend {
            BackendChoice::Sqlite => match SqliteStore::new_with_key(data_dir, key) {
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
            },
            #[cfg(feature = "mongo")]
            BackendChoice::Mongo { uri, db_name } => {
                use crate::mongo_store::MongoStore;
                let store = MongoStore::connect(&uri, &db_name)
                    .await
                    .map_err(|e| anyhow::anyhow!("connect MongoStore at {uri}/{db_name}: {e}"))?;
                eprintln!("  \x1b[32mEvent store: MongoDB ({db_name})\x1b[0m");
                Arc::new(store)
            }
            #[cfg(not(feature = "mongo"))]
            BackendChoice::Mongo { .. } => {
                return Err(anyhow::anyhow!(
                    "data_backend = \"mongo\" requires building with `--features open-story-store/mongo`"
                ));
            }
        };

        Ok(Self::assemble(
            event_store,
            session_store,
            event_log,
            plan_store,
            data_dir.to_path_buf(),
        ))
    }

    /// Internal: assemble a StoreState from its parts.
    fn assemble(
        event_store: Arc<dyn EventStore>,
        session_store: SessionStore,
        event_log: EventLog,
        plan_store: PlanStore,
        data_dir: PathBuf,
    ) -> Self {
        Self {
            event_store,
            session_store,
            event_log,
            plan_store,
            projections: Arc::new(DashMap::new()),
            detected_patterns: HashMap::new(),
            full_payloads: HashMap::new(),
            subagent_parents: HashMap::new(),
            session_children: HashMap::new(),
            session_projects: HashMap::new(),
            session_project_names: HashMap::new(),
            watch_dir_entries: Vec::new(),
            data_dir,
        }
    }
}

/// Internal helper used by the legacy sync constructors. Creates the
/// SQLite-backed event store and the disk-backed sidecars in one shot.
fn init_sidecar_stores(
    data_dir: &Path,
    key: Option<&str>,
) -> Result<(Arc<dyn EventStore>, SessionStore, EventLog, PlanStore)> {
    let plans_dir = data_dir.join("plans");
    std::fs::create_dir_all(&plans_dir)?;
    let session_store = SessionStore::new(data_dir)?;
    let event_log = EventLog::new(data_dir)?;
    let plan_store = PlanStore::new(&plans_dir)?;

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
    Ok((event_store, session_store, event_log, plan_store))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// Without the `mongo` feature, asking for the Mongo backend must
    /// fail at boot with a clear, actionable error message — never
    /// silently fall back to SQLite. The Phase 7 contract.
    #[cfg(not(feature = "mongo"))]
    #[tokio::test]
    async fn with_backend_mongo_without_feature_errors_clearly() {
        let tmp = TempDir::new().unwrap();
        let result = StoreState::with_backend(
            tmp.path(),
            None,
            BackendChoice::Mongo {
                uri: "mongodb://localhost:27017".to_string(),
                db_name: "openstory".to_string(),
            },
        )
        .await;
        // Can't use `expect_err` because StoreState doesn't impl Debug.
        let err = match result {
            Err(e) => e,
            Ok(_) => panic!("must error without the mongo feature"),
        };
        let msg = err.to_string();
        assert!(
            msg.contains("mongo") && msg.contains("feature"),
            "error message must mention the feature flag, got: {msg}"
        );
    }

    /// With or without the feature, SQLite is always selectable and
    /// always works.
    #[tokio::test]
    async fn with_backend_sqlite_works() {
        let tmp = TempDir::new().unwrap();
        let state = StoreState::with_backend(tmp.path(), None, BackendChoice::Sqlite)
            .await
            .expect("sqlite backend must always boot");
        assert!(state.event_store.list_sessions().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn new_creates_empty_store() {
        let tmp = TempDir::new().unwrap();
        let state = StoreState::new(tmp.path()).unwrap();

        assert!(state.event_store.list_sessions().await.unwrap().is_empty());
        assert!(state.projections.is_empty());
        assert!(state.detected_patterns.is_empty());
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

        // Dedup is now solely the EventStore PRIMARY KEY's job — the legacy
        // in-memory `seen_event_ids` HashSet was retired alongside the
        // /hooks endpoint that needed it.
        assert!(state.event_store.insert_event("sess-1", &event).await.unwrap());
        assert!(!state.event_store.insert_event("sess-1", &event).await.unwrap(), "dedup via PK");

        let mut proj = state.projections.entry("sess-1".to_string())
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

    // ── detect_subagent_relationship — extracted from 4 call sites ────

    #[test]
    fn subagent_relationship_records_when_data_session_differs() {
        let event = serde_json::json!({"data": {"session_id": "parent-123"}});
        let mut parents: HashMap<String, String> = HashMap::new();
        let mut children: HashMap<String, Vec<String>> = HashMap::new();
        detect_subagent_relationship(&event, "agent-456", &mut parents, &mut children);

        assert_eq!(parents.get("agent-456").map(String::as_str), Some("parent-123"));
        assert_eq!(children.get("parent-123"), Some(&vec!["agent-456".to_string()]));
    }

    #[test]
    fn subagent_relationship_skips_when_data_session_matches() {
        // Normal session — its own session_id equals data.session_id.
        // No subagent relationship to record.
        let event = serde_json::json!({"data": {"session_id": "sess-1"}});
        let mut parents: HashMap<String, String> = HashMap::new();
        let mut children: HashMap<String, Vec<String>> = HashMap::new();
        detect_subagent_relationship(&event, "sess-1", &mut parents, &mut children);

        assert!(parents.is_empty());
        assert!(children.is_empty());
    }

    #[test]
    fn subagent_relationship_only_records_first_event_per_subagent() {
        // The condition `!parents.contains_key(own)` means we only record
        // once per subagent. Subsequent events from the same subagent
        // don't add duplicate child entries.
        let e1 = serde_json::json!({"data": {"session_id": "p"}});
        let e2 = serde_json::json!({"data": {"session_id": "p"}});
        let mut parents: HashMap<String, String> = HashMap::new();
        let mut children: HashMap<String, Vec<String>> = HashMap::new();

        detect_subagent_relationship(&e1, "a", &mut parents, &mut children);
        detect_subagent_relationship(&e2, "a", &mut parents, &mut children);

        assert_eq!(children.get("p").map(|v| v.len()), Some(1));
    }

    #[test]
    fn subagent_relationship_handles_missing_data_field() {
        // Event without data.session_id — should be a no-op, not panic.
        let event = serde_json::json!({"id": "evt-1"});
        let mut parents: HashMap<String, String> = HashMap::new();
        let mut children: HashMap<String, Vec<String>> = HashMap::new();
        detect_subagent_relationship(&event, "a", &mut parents, &mut children);

        assert!(parents.is_empty());
        assert!(children.is_empty());
    }
}
