//! Application state — AppState wraps StoreState + server-specific fields.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Result;
use tokio::sync::{RwLock, broadcast as tokio_broadcast};

use open_story_bus::Bus;
use open_story_store::state::{BackendChoice, StoreState};

use open_story_store::analysis::{self, extract_cwd_from_events};

use crate::broadcast::BroadcastMessage;
use crate::config::{Config, DataBackend};
use crate::watcher_diagnostics::WatcherDiagnostics;

/// Shared application state, wrapped in Arc<RwLock<_>>.
///
/// AppState composes StoreState (event storage, projections, patterns) with
/// server-specific fields (broadcast, transcript watcher state, bus, config).
pub struct AppState {
    // ── store ── all event storage, projections, patterns, project resolution
    pub store: StoreState,

    // ── listener ── file watcher state
    pub transcript_states: HashMap<PathBuf, open_story_core::translate::TranscriptState>,
    pub watcher_diagnostics: WatcherDiagnostics,

    // ── server ── broadcast to WebSocket subscribers
    pub broadcast_tx: tokio_broadcast::Sender<BroadcastMessage>,

    // ── bus ── event bus for publishing
    pub bus: Arc<dyn Bus>,

    // ── admin: live topology as a watch channel (SICP stream-with-memory).
    // The broadcaster (consumers/admin_broadcaster) owns updates; the REST
    // handler reads via `.borrow()`. Initialized at boot from a fresh
    // `compute_topology` so the first GET never sees an uninitialized state.
    pub admin_topology_tx: tokio::sync::watch::Sender<crate::admin::Topology>,

    // ── configuration ──
    pub config: Config,
    pub watch_dir: PathBuf,

    // ── account config writer (Phase 5.4) ──
    // `None` for solo / single-account deployments and for tests that don't
    // exercise cross-person sharing. `Some(_)` when the server is configured
    // with a multi-account nats-server conf and is allowed to mutate it via
    // POST /api/admin/share-with-person.
    pub account_config_writer: Option<Arc<crate::account_config::AccountConfigWriter>>,

    /// Reloader called after the writer persists. Tells the running
    /// nats-server to reread its conf so new exports/imports take effect
    /// without a server restart. `None` skips the reload (useful for tests
    /// that only care about the disk write).
    pub account_config_reloader:
        Option<Arc<dyn crate::account_config::NatsReloader>>,

    /// Phase 6.5 — role lookup for the local principal. Defaults to
    /// `NoopRoleDirectory`, which fails-closed on every role-gated route.
    /// Boot wires `EmbeddedRoleDirectory` when `Config::roles_db_path`
    /// is set.
    pub role_directory: Arc<dyn crate::directory::RoleDirectory>,
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
pub async fn create_state(
    data_dir: &Path,
    watch_dir: &Path,
    bus: Arc<dyn Bus>,
    config: Config,
) -> Result<SharedState> {
    create_state_with_watch_dirs(data_dir, &[watch_dir.to_path_buf()], bus, config).await
}

pub async fn create_state_with_watch_dirs(
    data_dir: &Path,
    watch_dirs: &[PathBuf],
    bus: Arc<dyn Bus>,
    config: Config,
) -> Result<SharedState> {
    let watch_dir = watch_dirs
        .first()
        .cloned()
        .unwrap_or_else(|| PathBuf::from(&config.watch_dir));
    let db_key = if config.db_key.is_empty() {
        None
    } else {
        Some(config.db_key.as_str())
    };
    let backend = match config.data_backend {
        DataBackend::Sqlite => BackendChoice::Sqlite,
        DataBackend::Mongo => BackendChoice::Mongo {
            uri: config.mongo_uri.clone(),
            db_name: config.mongo_db.clone(),
        },
    };
    let mut store = StoreState::with_backend(data_dir, db_key, backend).await?;

    // Reconciler — ensure the EventStore contains every event present in
    // JSONL on disk. Idempotent (PK dedup); no-op when data_dir is empty
    // (so fresh contributors see indistinguishable behavior). Heals drift
    // from prior runs / backend switches before any consumer subscribes
    // to NATS — sequential boot so there is no race with live ingest.
    // See `docs/research/CONSTELLATION.md` R1 for the full architectural
    // framing.
    let reconcile_report = crate::reconcile::reconcile_local(data_dir, &mut store).await?;
    if reconcile_report.did_work() || !reconcile_report.errors.is_empty() {
        eprintln!(
            "  \x1b[36mReconciled: {} events added, {} skipped, {} sessions upserted in {:.1}s\x1b[0m",
            reconcile_report.events_inserted,
            reconcile_report.events_skipped,
            reconcile_report.sessions_upserted,
            reconcile_report.elapsed.as_secs_f64(),
        );
        for err in reconcile_report.errors.iter().take(5) {
            eprintln!("  \x1b[33mreconcile warning: {err}\x1b[0m");
        }
        if reconcile_report.errors.len() > 5 {
            eprintln!(
                "  \x1b[33mreconcile warning: ... and {} more\x1b[0m",
                reconcile_report.errors.len() - 5,
            );
        }
    }

    let (broadcast_tx, _) = tokio_broadcast::channel(config.broadcast_channel_size);

    // List watch root subdirectories for project resolution. Multiple JSONL
    // watcher roots feed the same store; combine the immediate entries so cwd
    // resolution keeps working for Claude-style encoded project directories.
    store.watch_dir_entries = watch_dirs
        .iter()
        .filter(|watch_dir| watch_dir.exists())
        .filter_map(|watch_dir| std::fs::read_dir(watch_dir).ok())
        .flat_map(|entries| entries.filter_map(|entry| entry.ok()))
        .filter(|entry| {
            entry
                .file_type()
                .map(|file_type| file_type.is_dir())
                .unwrap_or(false)
        })
        .filter_map(|entry| entry.file_name().to_str().map(|name| name.to_string()))
        .collect();

    // Boot from SQLite if it has data (restart case — data already translated).
    // A single pass per session inside boot_from_sqlite loads each session's
    // events ONCE and derives both the subagent tree and the cwd→project
    // mapping. Previously this was two separate full scans (subagent detection
    // here, project resolution in a second loop), each deserializing every
    // event of every session to pull one field.
    let sqlite_sessions = store.event_store.list_sessions().await.unwrap_or_default();
    if !sqlite_sessions.is_empty() {
        boot_from_sqlite(&mut store, &sqlite_sessions).await;
        // Rebuild the in-memory read model from the durable store. boot_from_sqlite
        // only derives the subagent tree + project map; without this, projections
        // (which /api/sessions serves token totals + live label/branch from) stay
        // empty until the watcher re-reads a source — so sessions whose source is
        // gone or beyond boot_window show 0 tokens despite being durably stored.
        // Idempotent: the watcher's later re-read dedups against the rebuilt seen_ids.
        let report = crate::reproject::reproject_all(&store).await;
        if report.sessions_reprojected > 0 {
            eprintln!(
                "  \x1b[32mReprojected {} sessions ({} events) from store\x1b[0m",
                report.sessions_reprojected, report.events_applied
            );
        }
    }
    // If SQLite is empty (first boot), watcher backfill handles everything.
    // Events go through: JSONL → translate_line() → NATS → consumers → SQLite.

    // Seed the admin topology stream with the initial snapshot — `nodes`
    // populated from whatever sessions just loaded. The broadcaster will
    // re-derive on every input event after boot.
    //
    // EnvInputs::from_env_and_discover reads explicit env vars AND tries
    // NATS-monitor auto-discovery of the leafnode upstream — best-effort,
    // silent None on any failure.
    let env_inputs = crate::admin::EnvInputs::from_env_and_discover().await;
    let initial_topology = {
        let session_hosts: Vec<(String, u64)> = {
            let mut tally: HashMap<String, u64> = HashMap::new();
            let rows = store.event_store.list_sessions().await.unwrap_or_default();
            for row in rows {
                if let Some(h) = row.host {
                    *tally.entry(h).or_insert(0) += 1;
                }
            }
            tally.into_iter().collect()
        };
        crate::admin::compute_topology(
            open_story_core::host::host(),
            config.role,
            &env_inputs,
            &session_hosts,
        )
    };
    let (admin_topology_tx, _) = tokio::sync::watch::channel(initial_topology);

    // ── Phase 5 boot-wire: build the AccountConfigWriter + reloader
    // when the operator has opted in via `nats_accounts_conf_path` + a
    // `[person]` block. Without both, multi-account mode stays off and
    // POST /api/admin/share-with-person returns 503.
    let (account_config_writer, account_config_reloader) =
        build_account_config(&config);

    // ── Phase 6.5 boot-wire: role directory.
    let role_directory = build_role_directory(&config);

    Ok(Arc::new(RwLock::new(AppState {
        store,
        transcript_states: HashMap::new(),
        watcher_diagnostics: WatcherDiagnostics::default(),
        broadcast_tx,
        bus,
        admin_topology_tx,
        config,
        watch_dir,
        account_config_writer,
        account_config_reloader,
        role_directory,
    })))
}

/// Build the role directory based on config. SQLite-backed when
/// `roles_db_path` is set; `NoopRoleDirectory` otherwise (fail-closed).
fn build_role_directory(
    config: &Config,
) -> Arc<dyn crate::directory::RoleDirectory> {
    use crate::directory::{EmbeddedRoleDirectory, NoopRoleDirectory};
    if config.roles_db_path.is_empty() {
        return Arc::new(NoopRoleDirectory);
    }
    match EmbeddedRoleDirectory::open(Path::new(&config.roles_db_path)) {
        Ok(d) => Arc::new(d),
        Err(e) => {
            eprintln!(
                "warning: could not open roles_db_path {}: {e}; falling back to NoopRoleDirectory (all role-gated routes will 403)",
                config.roles_db_path
            );
            Arc::new(NoopRoleDirectory)
        }
    }
}

/// Build the AccountConfigWriter + matching NatsReloader from config.
/// Returns `(None, None)` when multi-account mode isn't configured.
///
/// Boot semantics:
/// - `nats_accounts_conf_path` empty → no multi-account mode.
/// - `nats_accounts_conf_path` set but `[person]` missing → log a warning
///   and disable (the writer needs at least one local account to seed).
/// - Both set → build writer seeded with this device's person as the
///   only account, persist the initial conf to disk (so nats-server has
///   a file to read on its first boot), and wire the reloader.
fn build_account_config(
    config: &Config,
) -> (
    Option<Arc<crate::account_config::AccountConfigWriter>>,
    Option<Arc<dyn crate::account_config::NatsReloader>>,
) {
    use crate::account_config::{
        AccountConfigWriter, NatsReloader, ShellCommandReloader,
        DEFAULT_NATS_STATIC_PREFIX,
    };
    use open_story_bus::accounts::{AccountSpec, UserSpec};

    if config.nats_accounts_conf_path.is_empty() {
        return (None, None);
    }
    let Some(person) = &config.person else {
        eprintln!(
            "warning: nats_accounts_conf_path is set but [person] is not — \
             multi-account mode disabled; share-with-person will return 503"
        );
        return (None, None);
    };

    // Single seed account: this device's owner. The handler's
    // `add_share_with_stubs` will lazily create stubs for any other persons
    // the operator names as share targets. Real credentials for those other
    // persons must be added by the operator (Phase 6 directory work will
    // generalize this).
    let local_account = AccountSpec {
        name: person_account_name(&person.id),
        users: vec![UserSpec {
            user: person.id.clone(),
            // TODO(Phase 6.7+): swap password for NKEY-based auth. For
            // boot-wire scope the password is derived deterministically so
            // local dev can connect; not a security control by itself.
            password: format!("{}-local-dev", person.id),
            permissions: None,
        }],
        exports: vec![],
        imports: vec![],
    };
    let writer = Arc::new(AccountConfigWriter::new(
        std::path::PathBuf::from(&config.nats_accounts_conf_path),
        DEFAULT_NATS_STATIC_PREFIX,
        vec![local_account],
    ));
    if let Err(e) = writer.persist() {
        eprintln!(
            "warning: could not persist initial nats accounts conf to {}: {e}",
            config.nats_accounts_conf_path
        );
    }

    let reloader: Option<Arc<dyn NatsReloader>> = if config
        .nats_reload_command
        .is_empty()
    {
        None
    } else {
        Some(Arc::new(ShellCommandReloader {
            command: config.nats_reload_command.clone(),
        }))
    };

    (Some(writer), reloader)
}

/// Convention: PersonId `max` → NATS account `PERSON_MAX`. Hyphens become
/// underscores; lowercase becomes uppercase. Single source of truth used
/// by both this boot path and the share-with-person handler so the
/// account names match across the two paths.
pub(crate) fn person_account_name(person_id: &str) -> String {
    format!("PERSON_{}", person_id.to_uppercase().replace('-', "_"))
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
        let events = store
            .event_store
            .session_events(&row.id)
            .await
            .unwrap_or_default();
        // Detect subagent → parent relationships from the boot-loaded events
        // (shared helper). Dedup is the EventStore PK's job.
        for event in &events {
            open_story_store::state::detect_subagent_relationship(
                event,
                &row.id,
                &store.subagent_parents,
                &store.session_children,
            );
        }
        // Derive project_id / project_name from cwd in the SAME pass, reusing
        // the events we already loaded rather than re-scanning every session.
        if let Some(cwd) = extract_cwd_from_events(&events) {
            let resolved = analysis::resolve_project(&cwd, &store.watch_dir_entries);
            store
                .session_projects
                .insert(row.id.clone(), resolved.project_id);
            store
                .session_project_names
                .insert(row.id.clone(), resolved.project_name);
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

        let state = create_state(&data_dir, &watch_dir, Arc::new(NoopBus), Config::default())
            .await
            .unwrap();
        let s = state.read().await;
        assert!(
            s.store
                .event_store
                .list_sessions()
                .await
                .unwrap()
                .is_empty()
        );
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

        let state = create_state(&data_dir, &watch_dir, Arc::new(NoopBus), Config::default())
            .await
            .unwrap();
        let s = state.read().await;
        assert_eq!(s.store.watch_dir_entries.len(), 2);
        assert!(
            s.store
                .watch_dir_entries
                .contains(&"my-project".to_string())
        );
        assert!(
            s.store
                .watch_dir_entries
                .contains(&"other-project".to_string())
        );
    }

    #[tokio::test]
    async fn create_state_scans_all_watch_dir_entries() {
        let tmp = tempfile::tempdir().unwrap();
        let data_dir = tmp.path().join("data");
        let claude_dir = tmp.path().join("claude");
        let codex_dir = tmp.path().join("codex");
        std::fs::create_dir_all(&data_dir).unwrap();
        std::fs::create_dir_all(claude_dir.join("-Users-maxglassie-projects-OpenStory")).unwrap();
        std::fs::create_dir_all(codex_dir.join("2026")).unwrap();

        let state = create_state_with_watch_dirs(
            &data_dir,
            &[claude_dir, codex_dir],
            Arc::new(NoopBus),
            Config::default(),
        )
        .await
        .unwrap();

        let s = state.read().await;
        assert!(
            s.store
                .watch_dir_entries
                .contains(&"-Users-maxglassie-projects-OpenStory".to_string())
        );
        assert!(s.store.watch_dir_entries.contains(&"2026".to_string()));
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
                host: None,
                user: None,
                origin_agent: None,
                person_id: None,
                principal_id: None,
            })
            .await
            .unwrap();
        }
        // No JSONL files exist — boot must come from SQLite

        let state = create_state(&data_dir, &watch_dir, Arc::new(NoopBus), Config::default())
            .await
            .unwrap();
        let s = state.read().await;

        assert!(
            !s.store
                .event_store
                .session_events("sqlite-session")
                .await
                .unwrap()
                .is_empty(),
            "should boot session from SQLite"
        );
        assert_eq!(
            s.store
                .event_store
                .session_events("sqlite-session")
                .await
                .unwrap()
                .len(),
            1
        );
    }

    /// Boot rebuilds the in-memory projection from the durable store via
    /// `reproject_all`. `/api/sessions` serves token totals (and live
    /// label/branch) from `store.projections` (api.rs:~100,141); before the
    /// reproject step those stayed empty until the watcher re-read a source, so
    /// a session whose source is gone or beyond boot_window (pi-mono
    /// --no-session, old sessions) showed 0 tokens despite being durably stored.
    /// This pins the fix: after boot, the projection exists with the right count
    /// even with an empty watch dir (no re-readable source).
    #[tokio::test]
    async fn boot_reprojects_the_in_memory_projection_from_store() {
        let tmp = tempfile::tempdir().unwrap();
        let data_dir = tmp.path().join("data");
        let watch_dir = tmp.path().join("watch");
        std::fs::create_dir_all(&data_dir).unwrap();
        std::fs::create_dir_all(&watch_dir).unwrap();

        {
            use open_story_store::event_store::{EventStore, SessionRow};
            use open_story_store::sqlite_store::SqliteStore;
            let db = SqliteStore::new(&data_dir).unwrap();
            db.insert_event(
                "orphan-session",
                &serde_json::json!({
                    "id": "orphan-evt-1",
                    "type": "io.arc.event",
                    "subtype": "message.user.prompt",
                    "time": "2025-01-14T10:00:00Z",
                    "source": "arc://test",
                    "data": {"text": "stored but no re-readable source"}
                }),
            )
            .await
            .unwrap();
            db.upsert_session(&SessionRow {
                id: "orphan-session".into(),
                project_id: None,
                project_name: None,
                label: Some("orphan".into()),
                custom_label: None,
                branch: None,
                event_count: 1,
                first_event: Some("2025-01-14T10:00:00Z".into()),
                last_event: Some("2025-01-14T10:00:00Z".into()),
                host: None,
                user: None,
                origin_agent: None,
                person_id: None,
                principal_id: None,
            })
            .await
            .unwrap();
        }

        let state = create_state(&data_dir, &watch_dir, Arc::new(NoopBus), Config::default())
            .await
            .unwrap();
        let s = state.read().await;

        // Durable store HAS the session after boot.
        assert_eq!(
            s.store
                .event_store
                .session_events("orphan-session")
                .await
                .unwrap()
                .len(),
            1,
            "event is durably present after boot"
        );

        // And the in-memory projection IS rebuilt from the store — the field
        // /api/sessions serves token totals from is present with the right
        // count, even though the watch dir is empty (no source to re-read).
        let proj = s.store.projections.get("orphan-session");
        assert!(
            proj.is_some(),
            "reproject_all should rebuild the projection from the durable store at boot"
        );
        assert_eq!(
            proj.unwrap().event_count(),
            1,
            "reprojected event_count must match the stored events"
        );
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
            db.insert_event(
                "old-session",
                &serde_json::json!({
                    "id": "old-evt", "type": "io.arc.event", "subtype": "message.user.prompt",
                    "time": "2025-01-01T00:00:00Z", "source": "arc://test",
                    "data": {"text": "ancient history"}
                }),
            )
            .await
            .unwrap();
            db.upsert_session(&SessionRow {
                id: "old-session".into(),
                project_id: None,
                project_name: None,
                label: None,
                branch: None,
                event_count: 1,
                custom_label: None,
                first_event: Some("2025-01-01T00:00:00Z".into()),
                last_event: Some("2025-01-01T00:00:00Z".into()),
                host: None,
                user: None,
                origin_agent: None,
                person_id: None,
                principal_id: None,
            })
            .await
            .unwrap();

            // New session
            db.insert_event(
                "new-session",
                &serde_json::json!({
                    "id": "new-evt", "type": "io.arc.event", "subtype": "message.user.prompt",
                    "time": "2025-01-14T10:00:00Z", "source": "arc://test",
                    "data": {"text": "just now"}
                }),
            )
            .await
            .unwrap();
            db.upsert_session(&SessionRow {
                id: "new-session".into(),
                project_id: None,
                project_name: None,
                label: None,
                branch: None,
                event_count: 1,
                custom_label: None,
                first_event: Some("2025-01-14T10:00:00Z".into()),
                last_event: Some("2025-01-14T10:00:00Z".into()),
                host: None,
                user: None,
                origin_agent: None,
                person_id: None,
                principal_id: None,
            })
            .await
            .unwrap();
        }

        let state = create_state(&data_dir, &watch_dir, Arc::new(NoopBus), Config::default())
            .await
            .unwrap();
        let s = state.read().await;

        assert!(
            !s.store
                .event_store
                .session_events("old-session")
                .await
                .unwrap()
                .is_empty(),
            "SQLite boot should load all sessions, including old ones"
        );
        assert!(
            !s.store
                .event_store
                .session_events("new-session")
                .await
                .unwrap()
                .is_empty()
        );
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
                host: None,
                user: None,
                origin_agent: None,
                person_id: None,
                principal_id: None,
            })
            .await
            .unwrap();
        }

        // Second leg: boot finds SQLite populated, loads from DB.
        let state2 = create_state(&data_dir, &watch_dir, Arc::new(NoopBus), Config::default())
            .await
            .unwrap();
        let s = state2.read().await;

        assert!(
            !s.store
                .event_store
                .session_events("restart-session")
                .await
                .unwrap()
                .is_empty(),
            "should boot from pre-populated SQLite without any JSONL on disk"
        );
    }

    /// When both SQLite and JSONL have data, the reconciler ensures the
    /// EventStore contains the union — JSONL is canonical disk truth, the
    /// EventStore reconciles to it on every boot. This is the intentional
    /// reversal of the old "SQLite takes priority over JSONL" invariant
    /// (see commit history). The reconciler's contract is: any event
    /// present in JSONL must be present in the EventStore after boot, no
    /// exceptions. PK dedup means events already in SQLite are not
    /// duplicated.
    ///
    /// See `docs/research/CONSTELLATION.md` P1 + R1.
    #[tokio::test]
    async fn reconciler_unions_sqlite_and_jsonl_on_boot() {
        let tmp = tempfile::tempdir().unwrap();
        let data_dir = tmp.path().join("data");
        let watch_dir = tmp.path().join("watch");
        std::fs::create_dir_all(&data_dir).unwrap();
        std::fs::create_dir_all(&watch_dir).unwrap();

        // Write JSONL with one event for `mixed-session`.
        std::fs::write(
            data_dir.join("mixed-session.jsonl"),
            serde_json::to_string(&serde_json::json!({
                "id": "jsonl-only-evt",
                "type": "io.arc.event",
                "source": "arc://test",
                "time": "2025-01-14T00:00:00Z",
                "data": {"text": "from jsonl only"}
            }))
            .unwrap()
                + "\n",
        )
        .unwrap();

        // Pre-populate SQLite with a different session — `sqlite-only-session`.
        {
            use open_story_store::event_store::{EventStore, SessionRow};
            use open_story_store::sqlite_store::SqliteStore;
            let db = SqliteStore::new(&data_dir).unwrap();
            db.insert_event(
                "sqlite-only-session",
                &serde_json::json!({
                    "id": "sqlite-only-evt",
                    "type": "io.arc.event",
                    "subtype": "message.user.prompt",
                    "time": "2025-01-14T10:00:00Z",
                    "source": "arc://test",
                    "data": {"text": "from sqlite only"}
                }),
            )
            .await
            .unwrap();
            db.upsert_session(&SessionRow {
                id: "sqlite-only-session".into(),
                project_id: None,
                project_name: None,
                label: None,
                branch: None,
                event_count: 1,
                custom_label: None,
                first_event: Some("2025-01-14T10:00:00Z".into()),
                last_event: Some("2025-01-14T10:00:00Z".into()),
                host: None,
                user: None,
                origin_agent: None,
                person_id: None,
                principal_id: None,
            })
            .await
            .unwrap();
        }

        let state = create_state(&data_dir, &watch_dir, Arc::new(NoopBus), Config::default())
            .await
            .unwrap();
        let s = state.read().await;

        // SQLite-seeded session is preserved.
        assert!(
            !s.store
                .event_store
                .session_events("sqlite-only-session")
                .await
                .unwrap()
                .is_empty(),
            "SQLite-seeded session must remain after reconciliation"
        );
        // JSONL-only session is now reconciled into the EventStore. This is
        // the *new* contract: JSONL is canonical, the EventStore agrees.
        assert!(
            !s.store
                .event_store
                .session_events("mixed-session")
                .await
                .unwrap()
                .is_empty(),
            "JSONL-only session must be reconciled into the EventStore on boot"
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
                db.insert_event(
                    "api-session",
                    &serde_json::json!({
                        "id": format!("api-evt-{}", i),
                        "type": "io.arc.event",
                        "subtype": "message.user.prompt",
                        "time": format!("2025-01-14T00:00:0{}Z", i),
                        "source": "arc://test",
                        "data": {"text": format!("event {}", i)}
                    }),
                )
                .await
                .unwrap();
            }
            db.upsert_session(&SessionRow {
                id: "api-session".into(),
                project_id: None,
                project_name: None,
                label: None,
                branch: None,
                event_count: 5,
                custom_label: None,
                first_event: Some("2025-01-14T00:00:01Z".into()),
                last_event: Some("2025-01-14T00:00:05Z".into()),
                host: None,
                user: None,
                origin_agent: None,
                person_id: None,
                principal_id: None,
            })
            .await
            .unwrap();
        }

        let state = create_state(&data_dir, &watch_dir, Arc::new(NoopBus), Config::default())
            .await
            .unwrap();
        let s = state.read().await;

        // EventStore should serve all 5 events
        let events = s
            .store
            .event_store
            .session_events("api-session")
            .await
            .unwrap();
        assert_eq!(events.len(), 5);
        assert_eq!(events[0]["id"], "api-evt-1");
        assert_eq!(events[4]["id"], "api-evt-5");
    }

    // ── build_account_config — boot-wire for share-with-person ──────────

    use crate::config::{Person, Principal, PrincipalMatchers};

    fn person_max() -> Person {
        Person {
            id: "max".to_string(),
            display_name: "Max".to_string(),
            email: "max@example.test".to_string(),
            principals: vec![Principal {
                id: "laptop".into(),
                display_name: "Laptop".into(),
                matchers: PrincipalMatchers {
                    host: Some("laptop.local".into()),
                    user: Some("max".into()),
                    agent: None,
                    watch_dir_pattern: None,
                },
            }],
        }
    }

    #[test]
    fn empty_conf_path_returns_no_writer_no_reloader() {
        let cfg = Config::default();
        let (writer, reloader) = build_account_config(&cfg);
        assert!(writer.is_none());
        assert!(reloader.is_none());
    }

    #[test]
    fn conf_path_set_without_person_returns_no_writer() {
        let tmp = tempfile::tempdir().unwrap();
        let mut cfg = Config::default();
        cfg.nats_accounts_conf_path = tmp.path().join("nats.conf").to_string_lossy().into_owned();
        // No [person] block.
        cfg.person = None;
        let (writer, reloader) = build_account_config(&cfg);
        assert!(writer.is_none());
        assert!(reloader.is_none());
    }

    #[test]
    fn conf_path_set_with_person_builds_writer_and_persists_initial_conf() {
        let tmp = tempfile::tempdir().unwrap();
        let conf_path = tmp.path().join("nats.conf");
        let mut cfg = Config::default();
        cfg.nats_accounts_conf_path = conf_path.to_string_lossy().into_owned();
        cfg.person = Some(person_max());
        // Use `true` as a no-op reload command so the unit test doesn't
        // try to pkill nats-server in CI.
        cfg.nats_reload_command = "true".into();

        let (writer, reloader) = build_account_config(&cfg);
        assert!(writer.is_some());
        assert!(reloader.is_some());

        // Initial conf is on disk and includes max's account.
        let on_disk = std::fs::read_to_string(&conf_path).unwrap();
        assert!(on_disk.contains("PERSON_MAX"));
        assert!(on_disk.contains("user: \"max\""));
        // Static prefix landed too.
        assert!(on_disk.contains("listen:"));
        assert!(on_disk.contains("jetstream"));
    }

    #[test]
    fn empty_reload_command_disables_the_reloader() {
        let tmp = tempfile::tempdir().unwrap();
        let mut cfg = Config::default();
        cfg.nats_accounts_conf_path =
            tmp.path().join("nats.conf").to_string_lossy().into_owned();
        cfg.person = Some(person_max());
        cfg.nats_reload_command = String::new();

        let (writer, reloader) = build_account_config(&cfg);
        assert!(writer.is_some(), "writer should still build");
        assert!(reloader.is_none(), "reloader should be disabled");
    }

    #[test]
    fn person_account_name_normalizes_case_and_hyphens() {
        assert_eq!(person_account_name("max"), "PERSON_MAX");
        assert_eq!(person_account_name("Katie"), "PERSON_KATIE");
        assert_eq!(person_account_name("uuid-with-hyphens"), "PERSON_UUID_WITH_HYPHENS");
    }
}
