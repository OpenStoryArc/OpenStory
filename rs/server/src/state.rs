//! Application state — AppState wraps StoreState + server-specific fields.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Result;
use tokio::sync::{broadcast as tokio_broadcast, RwLock};

use open_story_bus::Bus;
use open_story_store::state::{BackendChoice, StoreState};

use open_story_store::analysis::{self, extract_cwd_from_events};

use crate::broadcast::BroadcastMessage;
use crate::config::{Config, DataBackend};

/// Shared application state, wrapped in Arc<RwLock<_>>.
///
/// AppState composes StoreState (event storage, projections, patterns) with
/// server-specific fields (broadcast, transcript watcher state, bus, config).
pub struct AppState {
    // ── store ── all event storage, projections, patterns, project resolution
    pub store: StoreState,

    // ── listener ── file watcher state
    pub transcript_states: HashMap<PathBuf, open_story_core::translate::TranscriptState>,

    // ── server ── broadcast to WebSocket subscribers
    pub broadcast_tx: tokio_broadcast::Sender<BroadcastMessage>,

    // ── bus ── event bus for publishing
    pub bus: Arc<dyn Bus>,

    // ── configuration ──
    pub config: Config,
    pub watch_dir: PathBuf,
}

pub type SharedState = Arc<RwLock<AppState>>;

/// Create the application state. Boots from SQLite if available.
///
/// Boot priority:
/// 1. SQLite has sessions → load from DB (instant boot, data already translated)
/// 2. SQLite empty → start empty, watcher backfill handles JSONL → translate → NATS → consumers
///
/// The JSONL boot path was removed because it bypassed translate_line(), storing
/// raw Claude Code JSON in SQLite as if they were CloudEvents. This caused
/// agent_payload, tool_outcome, and agent_id to be missing on boot-loaded data.
/// Now all events go through one path: JSONL → translate → NATS → consumers.
pub async fn create_state(data_dir: &Path, watch_dir: &Path, bus: Arc<dyn Bus>, config: Config) -> Result<SharedState> {
    let db_key = if config.db_key.is_empty() { None } else { Some(config.db_key.as_str()) };
    let backend = match config.data_backend {
        DataBackend::Sqlite => BackendChoice::Sqlite,
        DataBackend::Mongo => BackendChoice::Mongo {
            uri: config.mongo_uri.clone(),
            db_name: config.mongo_db.clone(),
        },
    };
    let mut store = StoreState::with_backend(data_dir, db_key, backend).await?;

    let (broadcast_tx, _) = tokio_broadcast::channel(config.broadcast_channel_size);

    // List watch_dir subdirectories for project resolution
    store.watch_dir_entries = if watch_dir.exists() {
        std::fs::read_dir(watch_dir)
            .ok()
            .map(|entries| {
                entries
                    .filter_map(|e| e.ok())
                    .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
                    .filter_map(|e| e.file_name().to_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default()
    } else {
        Vec::new()
    };

    // Boot from SQLite if it has data (restart case — data already translated)
    let sqlite_sessions = store.event_store.list_sessions().await.unwrap_or_default();
    if !sqlite_sessions.is_empty() {
        boot_from_sqlite(&mut store, &sqlite_sessions).await;
    }
    // If SQLite is empty (first boot), watcher backfill handles everything.
    // Events go through: JSONL → translate_line() → NATS → consumers → SQLite.

    // Derive project_id and project_name from cwd for all loaded sessions
    let boot_session_ids: Vec<String> = store.event_store
        .list_sessions()
        .await
        .unwrap_or_default()
        .iter()
        .map(|r| r.id.clone())
        .collect();
    for sid in &boot_session_ids {
        let events = store.event_store.session_events(sid).await.unwrap_or_default();
        if let Some(cwd) = extract_cwd_from_events(&events) {
            let resolved = analysis::resolve_project(&cwd, &store.watch_dir_entries);
            store
                .session_projects
                .insert(sid.clone(), resolved.project_id);
            store
                .session_project_names
                .insert(sid.clone(), resolved.project_name);
        }
    }

    Ok(Arc::new(RwLock::new(AppState {
        store,
        transcript_states: HashMap::new(),
        broadcast_tx,
        bus,
        config,
        watch_dir: watch_dir.to_path_buf(),
    })))
}

/// Boot from SQLite — sessions already in the DB.
async fn boot_from_sqlite(
    store: &mut StoreState,
    sqlite_sessions: &[open_story_store::event_store::SessionRow],
) {
    eprintln!(
        "  \x1b[32mBooting from SQLite ({} sessions)\x1b[0m",
        sqlite_sessions.len()
    );
    for row in sqlite_sessions {
        let events = store.event_store.session_events(&row.id).await.unwrap_or_default();
        // Detect subagent → parent relationships from the boot-loaded events
        // (shared helper). Dedup is the EventStore PK's job.
        for event in &events {
            open_story_store::state::detect_subagent_relationship(
                event,
                &row.id,
                &mut store.subagent_parents,
                &mut store.session_children,
            );
        }
    }
}

// boot_from_jsonl removed — all events now go through the watcher path:
// JSONL → translate_line() → NATS → consumers → SQLite.
// See commit history for the old implementation.

#[cfg(test)]
mod tests {
    use super::*;
    use open_story_bus::noop_bus::NoopBus;

    #[tokio::test]
    async fn create_state_returns_empty_state_for_empty_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let data_dir = tmp.path().join("data");
        let watch_dir = tmp.path().join("watch");
        std::fs::create_dir_all(&data_dir).unwrap();
        std::fs::create_dir_all(&watch_dir).unwrap();

        let state = create_state(&data_dir, &watch_dir, Arc::new(NoopBus), Config::default()).await.unwrap();
        let s = state.read().await;
        assert!(s.store.event_store.list_sessions().await.unwrap().is_empty());
        assert!(s.store.projections.is_empty());
    }

    // `create_state_loads_persisted_sessions` retired — it asserted that
    // writing a JSONL file to `data_dir` would populate the EventStore on
    // `create_state()`. That was the `boot_from_jsonl` path, removed in
    // commit 5d936fe. The watcher is now the only ingestion route, and it
    // runs as a separate task spawned by `run_server()`, not by
    // `create_state()` itself. Equivalent coverage now lives in
    // `boot_from_sqlite_when_db_has_sessions` (which pre-populates SQLite
    // directly) plus the watcher integration tests in `rs/tests/test_watcher.rs`.

    #[tokio::test]
    async fn create_state_scans_watch_dir_entries() {
        let tmp = tempfile::tempdir().unwrap();
        let data_dir = tmp.path().join("data");
        let watch_dir = tmp.path().join("watch");
        std::fs::create_dir_all(&data_dir).unwrap();
        std::fs::create_dir_all(&watch_dir).unwrap();

        std::fs::create_dir_all(watch_dir.join("my-project")).unwrap();
        std::fs::create_dir_all(watch_dir.join("other-project")).unwrap();

        let state = create_state(&data_dir, &watch_dir, Arc::new(NoopBus), Config::default()).await.unwrap();
        let s = state.read().await;
        assert_eq!(s.store.watch_dir_entries.len(), 2);
        assert!(s.store.watch_dir_entries.contains(&"my-project".to_string()));
        assert!(s.store.watch_dir_entries.contains(&"other-project".to_string()));
    }

    // `create_state_backfills_plans_from_persisted_sessions` retired —
    // same reason as above: depended on the deleted `boot_from_jsonl`
    // path. Plan extraction now happens in `ingest_events`, exercised by
    // `ingest_extracts_plan_from_exit_plan_mode` below.

    // `create_state_tracks_all_event_ids_for_dedup` retired — depended on
    // the deleted `boot_from_jsonl` path AND on the deleted in-memory
    // `seen_event_ids` HashSet. Dedup is now solely the EventStore PK's
    // job, exercised by `consumers::persist::tests::dedup_*` (via
    // SqliteStore which enforces the PK constraint).

    // ── SQLite boot tests ─────────────────────────────────────────────

    /// Pre-populate SQLite, then boot. Should load from DB, not JSONL.
    #[tokio::test]
    async fn boot_from_sqlite_when_db_has_sessions() {
        let tmp = tempfile::tempdir().unwrap();
        let data_dir = tmp.path().join("data");
        let watch_dir = tmp.path().join("watch");
        std::fs::create_dir_all(&data_dir).unwrap();
        std::fs::create_dir_all(&watch_dir).unwrap();

        // Pre-populate SQLite directly (simulating a previous run)
        {
            use open_story_store::event_store::{EventStore, SessionRow};
            use open_story_store::sqlite_store::SqliteStore;
            let db = SqliteStore::new(&data_dir).unwrap();
            let event = serde_json::json!({
                "id": "sqlite-evt-1",
                "type": "io.arc.event",
                "subtype": "message.user.prompt",
                "time": "2025-01-14T10:00:00Z",
                "source": "arc://test",
                "data": {"text": "from sqlite"}
            });
            db.insert_event("sqlite-session", &event).await.unwrap();
            db.upsert_session(&SessionRow {
                id: "sqlite-session".into(),
                project_id: None,
                project_name: None,
                label: Some("sqlite test".into()),
                custom_label: None,
                branch: None,
                event_count: 1,
                first_event: Some("2025-01-14T10:00:00Z".into()),
                last_event: Some("2025-01-14T10:00:00Z".into()),
            }).await.unwrap();
        }
        // No JSONL files exist — boot must come from SQLite

        let state = create_state(&data_dir, &watch_dir, Arc::new(NoopBus), Config::default()).await.unwrap();
        let s = state.read().await;

        assert!(
            !s.store.event_store.session_events("sqlite-session").await.unwrap().is_empty(),
            "should boot session from SQLite"
        );
        assert_eq!(s.store.event_store.session_events("sqlite-session").await.unwrap().len(), 1);
    }

    /// SQLite boot should pick up ALL sessions, not just recent ones.
    /// (Unlike JSONL boot which uses a 24h window.)
    #[tokio::test]
    async fn boot_from_sqlite_loads_all_sessions_not_just_recent() {
        let tmp = tempfile::tempdir().unwrap();
        let data_dir = tmp.path().join("data");
        let watch_dir = tmp.path().join("watch");
        std::fs::create_dir_all(&data_dir).unwrap();
        std::fs::create_dir_all(&watch_dir).unwrap();

        {
            use open_story_store::event_store::{EventStore, SessionRow};
            use open_story_store::sqlite_store::SqliteStore;
            let db = SqliteStore::new(&data_dir).unwrap();

            // Old session (would be skipped by JSONL 24h window)
            db.insert_event("old-session", &serde_json::json!({
                "id": "old-evt", "type": "io.arc.event", "subtype": "message.user.prompt",
                "time": "2025-01-01T00:00:00Z", "source": "arc://test",
                "data": {"text": "ancient history"}
            })).await.unwrap();
            db.upsert_session(&SessionRow {
                id: "old-session".into(),
                project_id: None, project_name: None,
                label: None, branch: None, event_count: 1,
                custom_label: None,
                first_event: Some("2025-01-01T00:00:00Z".into()),
                last_event: Some("2025-01-01T00:00:00Z".into()),
            }).await.unwrap();

            // New session
            db.insert_event("new-session", &serde_json::json!({
                "id": "new-evt", "type": "io.arc.event", "subtype": "message.user.prompt",
                "time": "2025-01-14T10:00:00Z", "source": "arc://test",
                "data": {"text": "just now"}
            })).await.unwrap();
            db.upsert_session(&SessionRow {
                id: "new-session".into(),
                project_id: None, project_name: None,
                label: None, branch: None, event_count: 1,
                custom_label: None,
                first_event: Some("2025-01-14T10:00:00Z".into()),
                last_event: Some("2025-01-14T10:00:00Z".into()),
            }).await.unwrap();
        }

        let state = create_state(&data_dir, &watch_dir, Arc::new(NoopBus), Config::default()).await.unwrap();
        let s = state.read().await;

        assert!(
            !s.store.event_store.session_events("old-session").await.unwrap().is_empty(),
            "SQLite boot should load all sessions, including old ones"
        );
        assert!(!s.store.event_store.session_events("new-session").await.unwrap().is_empty());
    }

    /// Simulate a restart: first boot pre-populates SQLite directly (via
    /// the EventStore API), second boot finds SQLite populated → loads
    /// from DB. The first leg used to load from JSONL via `boot_from_jsonl`
    /// (now deleted); the test now uses the same SqliteStore-direct
    /// approach as `boot_from_sqlite_when_db_has_sessions`.
    #[tokio::test]
    async fn sqlite_survives_restart_cycle() {
        let tmp = tempfile::tempdir().unwrap();
        let data_dir = tmp.path().join("data");
        let watch_dir = tmp.path().join("watch");
        std::fs::create_dir_all(&data_dir).unwrap();
        std::fs::create_dir_all(&watch_dir).unwrap();

        // First leg: pre-populate SQLite directly (the new shape of "data
        // already exists from a previous run", since `boot_from_jsonl` is gone).
        {
            use open_story_store::event_store::{EventStore, SessionRow};
            use open_story_store::sqlite_store::SqliteStore;
            let db = SqliteStore::new(&data_dir).unwrap();
            let event = serde_json::json!({
                "id": "restart-evt-1",
                "type": "io.arc.event",
                "subtype": "message.user.prompt",
                "source": "arc://test",
                "time": "2025-01-14T00:00:00Z",
                "data": {"text": "hello"}
            });
            db.insert_event("restart-session", &event).await.unwrap();
            db.upsert_session(&SessionRow {
                id: "restart-session".into(),
                project_id: None,
                project_name: None,
                label: None,
                custom_label: None,
                branch: None,
                event_count: 1,
                first_event: Some("2025-01-14T00:00:00Z".into()),
                last_event: Some("2025-01-14T00:00:00Z".into()),
            }).await.unwrap();
        }

        // Second leg: boot finds SQLite populated, loads from DB.
        let state2 = create_state(&data_dir, &watch_dir, Arc::new(NoopBus), Config::default()).await.unwrap();
        let s = state2.read().await;

        assert!(
            !s.store.event_store.session_events("restart-session").await.unwrap().is_empty(),
            "should boot from pre-populated SQLite without any JSONL on disk"
        );
    }

    /// When both SQLite and JSONL have data, SQLite wins.
    #[tokio::test]
    async fn sqlite_takes_priority_over_jsonl() {
        let tmp = tempfile::tempdir().unwrap();
        let data_dir = tmp.path().join("data");
        let watch_dir = tmp.path().join("watch");
        std::fs::create_dir_all(&data_dir).unwrap();
        std::fs::create_dir_all(&watch_dir).unwrap();

        // Write JSONL with one event
        std::fs::write(
            data_dir.join("mixed-session.jsonl"),
            serde_json::to_string(&serde_json::json!({
                "id": "jsonl-only-evt",
                "type": "io.arc.event",
                "source": "arc://test",
                "time": "2025-01-14T00:00:00Z",
                "data": {"text": "from jsonl only"}
            })).unwrap() + "\n",
        ).unwrap();

        // Write SQLite with a different session
        {
            use open_story_store::event_store::{EventStore, SessionRow};
            use open_story_store::sqlite_store::SqliteStore;
            let db = SqliteStore::new(&data_dir).unwrap();
            db.insert_event("sqlite-only-session", &serde_json::json!({
                "id": "sqlite-only-evt",
                "type": "io.arc.event",
                "subtype": "message.user.prompt",
                "time": "2025-01-14T10:00:00Z",
                "source": "arc://test",
                "data": {"text": "from sqlite only"}
            })).await.unwrap();
            db.upsert_session(&SessionRow {
                id: "sqlite-only-session".into(),
                project_id: None, project_name: None,
                label: None, branch: None, event_count: 1,
                custom_label: None,
                first_event: Some("2025-01-14T10:00:00Z".into()),
                last_event: Some("2025-01-14T10:00:00Z".into()),
            }).await.unwrap();
        }

        let state = create_state(&data_dir, &watch_dir, Arc::new(NoopBus), Config::default()).await.unwrap();
        let s = state.read().await;

        // SQLite had data → boot_from_sqlite was used
        assert!(
            !s.store.event_store.session_events("sqlite-only-session").await.unwrap().is_empty(),
            "SQLite session should be loaded"
        );
        assert!(
            s.store.event_store.session_events("mixed-session").await.unwrap().is_empty(),
            "JSONL session should NOT be loaded when SQLite has data"
        );
    }

    /// API should serve events from EventStore after SQLite boot.
    #[tokio::test]
    async fn api_serves_events_after_sqlite_boot() {
        let tmp = tempfile::tempdir().unwrap();
        let data_dir = tmp.path().join("data");
        let watch_dir = tmp.path().join("watch");
        std::fs::create_dir_all(&data_dir).unwrap();
        std::fs::create_dir_all(&watch_dir).unwrap();

        {
            use open_story_store::event_store::{EventStore, SessionRow};
            use open_story_store::sqlite_store::SqliteStore;
            let db = SqliteStore::new(&data_dir).unwrap();
            for i in 1..=5 {
                db.insert_event("api-session", &serde_json::json!({
                    "id": format!("api-evt-{}", i),
                    "type": "io.arc.event",
                    "subtype": "message.user.prompt",
                    "time": format!("2025-01-14T00:00:0{}Z", i),
                    "source": "arc://test",
                    "data": {"text": format!("event {}", i)}
                })).await.unwrap();
            }
            db.upsert_session(&SessionRow {
                id: "api-session".into(),
                project_id: None, project_name: None,
                label: None, branch: None, event_count: 5,
                custom_label: None,
                first_event: Some("2025-01-14T00:00:01Z".into()),
                last_event: Some("2025-01-14T00:00:05Z".into()),
            }).await.unwrap();
        }

        let state = create_state(&data_dir, &watch_dir, Arc::new(NoopBus), Config::default()).await.unwrap();
        let s = state.read().await;

        // EventStore should serve all 5 events
        let events = s.store.event_store.session_events("api-session").await.unwrap();
        assert_eq!(events.len(), 5);
        assert_eq!(events[0]["id"], "api-evt-1");
        assert_eq!(events[4]["id"], "api-evt-5");
    }
}
