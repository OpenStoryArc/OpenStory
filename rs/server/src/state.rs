//! Application state — AppState wraps StoreState + server-specific fields.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use tokio::sync::{broadcast as tokio_broadcast, RwLock};

use open_story_bus::Bus;
use open_story_semantic::SemanticStore;
use open_story_store::state::StoreState;

use open_story_store::analysis::{self, extract_cwd_from_events};
use open_story_store::ingest::is_plan_event;

use crate::broadcast::BroadcastMessage;
use crate::config::Config;

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

    // ── semantic search ── optional vector search backend
    pub semantic_store: Arc<dyn SemanticStore>,

    // ── embedding worker ── sends records to background embedder
    pub embedding_tx: Option<tokio::sync::mpsc::Sender<open_story_semantic::worker::EmbedRequest>>,

    // ── embedder ── for generating query embeddings (search endpoint)
    pub embedder: Option<Arc<dyn open_story_semantic::embedder::Embedder>>,

    // ── configuration ──
    pub config: Config,
    pub watch_dir: PathBuf,
}

pub type SharedState = Arc<RwLock<AppState>>;

/// Create the application state. Boots from SQLite if available, else JSONL.
///
/// Boot priority:
/// 1. SQLite has sessions → load from DB (instant boot)
/// 2. SQLite empty → load recent JSONL sessions (boot_window_hours), populate SQLite
pub fn create_state(data_dir: &Path, watch_dir: &Path, bus: Arc<dyn Bus>, semantic_store: Arc<dyn SemanticStore>, config: Config) -> Result<SharedState> {
    let db_key = if config.db_key.is_empty() { None } else { Some(config.db_key.as_str()) };
    let mut store = StoreState::new_with_key(data_dir, db_key)?;

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

    // Try boot from SQLite first
    let sqlite_sessions = store.event_store.list_sessions().unwrap_or_default();
    if !sqlite_sessions.is_empty() {
        boot_from_sqlite(&mut store, &sqlite_sessions);
    } else {
        boot_from_jsonl(&mut store, &config);
    }

    // Derive project_id and project_name from cwd for all loaded sessions
    let boot_session_ids: Vec<String> = store.event_store
        .list_sessions()
        .unwrap_or_default()
        .iter()
        .map(|r| r.id.clone())
        .collect();
    for sid in &boot_session_ids {
        let events = store.event_store.session_events(sid).unwrap_or_default();
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
        semantic_store,
        embedding_tx: None, // Set by run_server when worker is spawned
        embedder: None,     // Set by run_server when embedder is loaded
        config,
        watch_dir: watch_dir.to_path_buf(),
    })))
}

/// Boot from SQLite — sessions already in the DB.
fn boot_from_sqlite(
    store: &mut StoreState,
    sqlite_sessions: &[open_story_store::event_store::SessionRow],
) {
    eprintln!(
        "  \x1b[32mBooting from SQLite ({} sessions)\x1b[0m",
        sqlite_sessions.len()
    );
    for row in sqlite_sessions {
        let events = store.event_store.session_events(&row.id).unwrap_or_default();
        // Track event IDs for dedup + detect subagent relationships
        for event in &events {
            if let Some(id) = event.get("id").and_then(|v| v.as_str()) {
                store.seen_event_ids.insert(id.to_string());
            }
            // Detect subagent → parent relationship
            if let Some(data_sid) = event.get("data")
                .and_then(|d| d.get("session_id"))
                .and_then(|v| v.as_str())
            {
                if data_sid != row.id && !store.subagent_parents.contains_key(row.id.as_str()) {
                    store.subagent_parents.insert(row.id.clone(), data_sid.to_string());
                    store.session_children
                        .entry(data_sid.to_string())
                        .or_default()
                        .push(row.id.clone());
                }
            }
        }
    }
}

/// Boot from JSONL — fallback when SQLite is empty (first run or fresh DB).
fn boot_from_jsonl(store: &mut StoreState, config: &Config) {
    let boot_window = Duration::from_secs(config.boot_window_hours * 3600);
    let known_plan_sessions: std::collections::HashSet<String> = store
        .plan_store
        .list_plans()
        .iter()
        .map(|p| p.session_id.clone())
        .collect();

    let recent_sessions = store.session_store.list_recent_sessions(boot_window);
    let total_sessions = store.session_store.list_sessions().len();
    if total_sessions > 0 {
        eprintln!(
            "  \x1b[33mBooting from JSONL ({} recent / {} total sessions)\x1b[0m",
            recent_sessions.len(),
            total_sessions,
        );
    }
    if total_sessions > recent_sessions.len() {
        eprintln!(
            "  \x1b[2mSkipped {} old sessions (>{:.0}h)\x1b[0m",
            total_sessions - recent_sessions.len(),
            boot_window.as_secs_f64() / 3600.0,
        );
    }

    for sid in recent_sessions {
        let events = store.session_store.load_session(&sid);
        // Backfill plans from sessions not yet in PlanStore
        if !known_plan_sessions.contains(&sid) {
            for event in &events {
                if is_plan_event(event) {
                    if let Some(plan_content) = event
                        .get("data")
                        .and_then(|d| d.get("args"))
                        .and_then(|a| a.get("plan"))
                        .and_then(|v| v.as_str())
                    {
                        let timestamp = event.get("time").and_then(|v| v.as_str()).unwrap_or("");
                        let _ = store.plan_store.save(&sid, plan_content, timestamp);
                    }
                }
            }
        }
        // Track all loaded event IDs for dedup + detect subagent relationships
        for event in &events {
            if let Some(id) = event.get("id").and_then(|v| v.as_str()) {
                store.seen_event_ids.insert(id.to_string());
            }
            // Detect subagent → parent relationship
            if let Some(data_sid) = event.get("data")
                .and_then(|d| d.get("session_id"))
                .and_then(|v| v.as_str())
            {
                if data_sid != sid && !store.subagent_parents.contains_key(sid.as_str()) {
                    store.subagent_parents.insert(sid.clone(), data_sid.to_string());
                    store.session_children
                        .entry(data_sid.to_string())
                        .or_default()
                        .push(sid.clone());
                }
            }
        }
        // Populate EventStore so replay_boot_sessions can read from it
        let _ = store.event_store.insert_batch(&sid, &events);
        let _ = store.event_store.upsert_session(
            &open_story_store::event_store::SessionRow {
                id: sid.clone(),
                project_id: None,
                project_name: None,
                label: None,
                custom_label: None,
                branch: None,
                event_count: events.len() as u64,
                first_event: events.first()
                    .and_then(|e| e.get("time"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
                last_event: events.last()
                    .and_then(|e| e.get("time"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use open_story_bus::noop_bus::NoopBus;
    use open_story_semantic::NoopSemanticStore;

    #[test]
    fn create_state_returns_empty_state_for_empty_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let data_dir = tmp.path().join("data");
        let watch_dir = tmp.path().join("watch");
        std::fs::create_dir_all(&data_dir).unwrap();
        std::fs::create_dir_all(&watch_dir).unwrap();

        let state = create_state(&data_dir, &watch_dir, Arc::new(NoopBus), Arc::new(NoopSemanticStore), Config::default()).unwrap();
        let s = state.blocking_read();
        assert!(s.store.event_store.list_sessions().unwrap().is_empty());
        assert!(s.store.seen_event_ids.is_empty());
        assert!(s.store.projections.is_empty());
    }

    #[test]
    fn create_state_loads_persisted_sessions() {
        let tmp = tempfile::tempdir().unwrap();
        let data_dir = tmp.path().join("data");
        let watch_dir = tmp.path().join("watch");
        std::fs::create_dir_all(&data_dir).unwrap();
        std::fs::create_dir_all(&watch_dir).unwrap();

        let event = serde_json::json!({
            "id": "evt-boot-1",
            "type": "io.arc.event",
            "subtype": "message.user.prompt",
            "source": "arc://test",
            "time": "2025-01-13T00:00:00Z",
            "data": {"text": "hello"}
        });
        std::fs::write(
            data_dir.join("test-session.jsonl"),
            serde_json::to_string(&event).unwrap() + "\n",
        )
        .unwrap();

        let state = create_state(&data_dir, &watch_dir, Arc::new(NoopBus), Arc::new(NoopSemanticStore), Config::default()).unwrap();
        let s = state.blocking_read();
        assert!(!s.store.event_store.session_events("test-session").unwrap().is_empty());
        assert!(s.store.seen_event_ids.contains("evt-boot-1"));
    }

    #[test]
    fn create_state_scans_watch_dir_entries() {
        let tmp = tempfile::tempdir().unwrap();
        let data_dir = tmp.path().join("data");
        let watch_dir = tmp.path().join("watch");
        std::fs::create_dir_all(&data_dir).unwrap();
        std::fs::create_dir_all(&watch_dir).unwrap();

        std::fs::create_dir_all(watch_dir.join("my-project")).unwrap();
        std::fs::create_dir_all(watch_dir.join("other-project")).unwrap();

        let state = create_state(&data_dir, &watch_dir, Arc::new(NoopBus), Arc::new(NoopSemanticStore), Config::default()).unwrap();
        let s = state.blocking_read();
        assert_eq!(s.store.watch_dir_entries.len(), 2);
        assert!(s.store.watch_dir_entries.contains(&"my-project".to_string()));
        assert!(s.store.watch_dir_entries.contains(&"other-project".to_string()));
    }

    #[test]
    fn create_state_backfills_plans_from_persisted_sessions() {
        let tmp = tempfile::tempdir().unwrap();
        let data_dir = tmp.path().join("data");
        let watch_dir = tmp.path().join("watch");
        std::fs::create_dir_all(&data_dir).unwrap();
        std::fs::create_dir_all(&watch_dir).unwrap();

        let event = serde_json::json!({
            "id": "evt-plan-backfill",
            "type": "io.arc.event",
            "subtype": "message.assistant.tool_use",
            "source": "arc://test",
            "time": "2025-01-13T00:00:00Z",
            "data": {
                "tool": "ExitPlanMode",
                "args": { "plan": "# Backfilled Plan\n\nThis was persisted and should be backfilled." },
                "raw": {
                    "type": "assistant",
                    "message": {
                        "model": "claude-4",
                        "content": [{
                            "type": "tool_use",
                            "id": "toolu_plan_bf",
                            "name": "ExitPlanMode",
                            "input": { "plan": "# Backfilled Plan\n\nThis was persisted and should be backfilled." }
                        }]
                    }
                }
            }
        });
        std::fs::write(
            data_dir.join("sess-plan-bf.jsonl"),
            serde_json::to_string(&event).unwrap() + "\n",
        )
        .unwrap();

        let state = create_state(&data_dir, &watch_dir, Arc::new(NoopBus), Arc::new(NoopSemanticStore), Config::default()).unwrap();
        let s = state.blocking_read();

        assert!(!s.store.event_store.session_events("sess-plan-bf").unwrap().is_empty());

        let plans = s.store.plan_store.list_plans();
        let session_plans: Vec<_> = plans
            .iter()
            .filter(|p| p.session_id == "sess-plan-bf")
            .collect();
        assert!(
            !session_plans.is_empty(),
            "plan should be backfilled from persisted ExitPlanMode event"
        );
    }

    #[test]
    fn create_state_tracks_all_event_ids_for_dedup() {
        let tmp = tempfile::tempdir().unwrap();
        let data_dir = tmp.path().join("data");
        let watch_dir = tmp.path().join("watch");
        std::fs::create_dir_all(&data_dir).unwrap();
        std::fs::create_dir_all(&watch_dir).unwrap();

        let events = format!(
            "{}\n{}\n",
            serde_json::to_string(&serde_json::json!({
                "id": "dedup-a",
                "type": "io.arc.event",
                "source": "arc://test",
                "time": "2025-01-13T00:00:00Z",
                "data": {"text": "first"}
            }))
            .unwrap(),
            serde_json::to_string(&serde_json::json!({
                "id": "dedup-b",
                "type": "io.arc.event",
                "source": "arc://test",
                "time": "2025-01-13T00:00:01Z",
                "data": {"text": "second"}
            }))
            .unwrap()
        );
        std::fs::write(data_dir.join("sess-dedup.jsonl"), events).unwrap();

        let state = create_state(&data_dir, &watch_dir, Arc::new(NoopBus), Arc::new(NoopSemanticStore), Config::default()).unwrap();
        let s = state.blocking_read();

        assert!(s.store.seen_event_ids.contains("dedup-a"));
        assert!(s.store.seen_event_ids.contains("dedup-b"));
        assert_eq!(s.store.event_store.session_events("sess-dedup").unwrap().len(), 2);
    }

    // ── SQLite boot tests ─────────────────────────────────────────────

    /// Pre-populate SQLite, then boot. Should load from DB, not JSONL.
    #[test]
    fn boot_from_sqlite_when_db_has_sessions() {
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
            db.insert_event("sqlite-session", &event).unwrap();
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
            }).unwrap();
        }
        // No JSONL files exist — boot must come from SQLite

        let state = create_state(&data_dir, &watch_dir, Arc::new(NoopBus), Arc::new(NoopSemanticStore), Config::default()).unwrap();
        let s = state.blocking_read();

        assert!(
            !s.store.event_store.session_events("sqlite-session").unwrap().is_empty(),
            "should boot session from SQLite"
        );
        assert_eq!(s.store.event_store.session_events("sqlite-session").unwrap().len(), 1);
        assert!(s.store.seen_event_ids.contains("sqlite-evt-1"));
    }

    /// SQLite boot should pick up ALL sessions, not just recent ones.
    /// (Unlike JSONL boot which uses a 24h window.)
    #[test]
    fn boot_from_sqlite_loads_all_sessions_not_just_recent() {
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
            })).unwrap();
            db.upsert_session(&SessionRow {
                id: "old-session".into(),
                project_id: None, project_name: None,
                label: None, branch: None, event_count: 1,
                custom_label: None,
                first_event: Some("2025-01-01T00:00:00Z".into()),
                last_event: Some("2025-01-01T00:00:00Z".into()),
            }).unwrap();

            // New session
            db.insert_event("new-session", &serde_json::json!({
                "id": "new-evt", "type": "io.arc.event", "subtype": "message.user.prompt",
                "time": "2025-01-14T10:00:00Z", "source": "arc://test",
                "data": {"text": "just now"}
            })).unwrap();
            db.upsert_session(&SessionRow {
                id: "new-session".into(),
                project_id: None, project_name: None,
                label: None, branch: None, event_count: 1,
                custom_label: None,
                first_event: Some("2025-01-14T10:00:00Z".into()),
                last_event: Some("2025-01-14T10:00:00Z".into()),
            }).unwrap();
        }

        let state = create_state(&data_dir, &watch_dir, Arc::new(NoopBus), Arc::new(NoopSemanticStore), Config::default()).unwrap();
        let s = state.blocking_read();

        assert!(
            !s.store.event_store.session_events("old-session").unwrap().is_empty(),
            "SQLite boot should load all sessions, including old ones"
        );
        assert!(!s.store.event_store.session_events("new-session").unwrap().is_empty());
    }

    /// Simulate a restart: first boot loads JSONL → populates SQLite,
    /// second boot finds SQLite populated → loads from DB.
    #[test]
    fn sqlite_survives_restart_cycle() {
        let tmp = tempfile::tempdir().unwrap();
        let data_dir = tmp.path().join("data");
        let watch_dir = tmp.path().join("watch");
        std::fs::create_dir_all(&data_dir).unwrap();
        std::fs::create_dir_all(&watch_dir).unwrap();

        // Write a JSONL file for first boot
        let event = serde_json::json!({
            "id": "restart-evt-1",
            "type": "io.arc.event",
            "subtype": "message.user.prompt",
            "source": "arc://test",
            "time": "2025-01-14T00:00:00Z",
            "data": {"text": "hello"}
        });
        std::fs::write(
            data_dir.join("restart-session.jsonl"),
            serde_json::to_string(&event).unwrap() + "\n",
        ).unwrap();

        // First boot: loads from JSONL, populates SQLite via dual-write in replay
        let state1 = create_state(&data_dir, &watch_dir, Arc::new(NoopBus), Arc::new(NoopSemanticStore), Config::default()).unwrap();
        {
            let mut s = state1.blocking_write();
            // Run replay to populate SQLite (normally called after create_state)
            crate::ingest::replay_boot_sessions(&mut s);
        }
        drop(state1);

        // Delete the JSONL file — simulating data loss or cleanup
        std::fs::remove_file(data_dir.join("restart-session.jsonl")).unwrap();

        // Second boot: JSONL is gone, should still load from SQLite
        let state2 = create_state(&data_dir, &watch_dir, Arc::new(NoopBus), Arc::new(NoopSemanticStore), Config::default()).unwrap();
        let s = state2.blocking_read();

        assert!(
            !s.store.event_store.session_events("restart-session").unwrap().is_empty(),
            "should survive restart via SQLite even after JSONL deletion"
        );
        assert!(s.store.seen_event_ids.contains("restart-evt-1"));
    }

    /// When both SQLite and JSONL have data, SQLite wins.
    #[test]
    fn sqlite_takes_priority_over_jsonl() {
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
            })).unwrap();
            db.upsert_session(&SessionRow {
                id: "sqlite-only-session".into(),
                project_id: None, project_name: None,
                label: None, branch: None, event_count: 1,
                custom_label: None,
                first_event: Some("2025-01-14T10:00:00Z".into()),
                last_event: Some("2025-01-14T10:00:00Z".into()),
            }).unwrap();
        }

        let state = create_state(&data_dir, &watch_dir, Arc::new(NoopBus), Arc::new(NoopSemanticStore), Config::default()).unwrap();
        let s = state.blocking_read();

        // SQLite had data → boot_from_sqlite was used
        assert!(
            !s.store.event_store.session_events("sqlite-only-session").unwrap().is_empty(),
            "SQLite session should be loaded"
        );
        assert!(
            s.store.event_store.session_events("mixed-session").unwrap().is_empty(),
            "JSONL session should NOT be loaded when SQLite has data"
        );
    }

    /// API should serve events from EventStore after SQLite boot.
    #[test]
    fn api_serves_events_after_sqlite_boot() {
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
                })).unwrap();
            }
            db.upsert_session(&SessionRow {
                id: "api-session".into(),
                project_id: None, project_name: None,
                label: None, branch: None, event_count: 5,
                custom_label: None,
                first_event: Some("2025-01-14T00:00:01Z".into()),
                last_event: Some("2025-01-14T00:00:05Z".into()),
            }).unwrap();
        }

        let state = create_state(&data_dir, &watch_dir, Arc::new(NoopBus), Arc::new(NoopSemanticStore), Config::default()).unwrap();
        let s = state.blocking_read();

        // EventStore should serve all 5 events
        let events = s.store.event_store.session_events("api-session").unwrap();
        assert_eq!(events.len(), 5);
        assert_eq!(events[0]["id"], "api-evt-1");
        assert_eq!(events[4]["id"], "api-evt-5");
    }
}
