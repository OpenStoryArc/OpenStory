//! Axum web server module — orchestration layer.
//!
//! Server logic lives in the `open-story-server` crate. This module re-exports
//! those types and retains `run_server` which wires the file watcher + server + bus.

pub mod analysis;
pub mod api;
pub mod broadcast;
pub mod hooks;
pub mod ingest;
pub mod persistence;
pub mod plan_store;
pub mod projection;
pub mod state;
pub mod tool_schemas;
pub mod transcript;
pub mod ws;

use std::path::Path;
use std::sync::Arc;

use anyhow::Result;

use open_story_bus::{Bus, IngestBatch};
// Consumer actor implementations ready but not yet wired as independent NATS consumers.
// See rs/server/src/consumers/ for the implementations.
#[allow(unused_imports)]
use open_story_server::consumers;
use open_story_server::logging::{event_type_summary, log_event, short_id};

pub use broadcast::BroadcastMessage;
pub use open_story_server::config;
pub use open_story_server::config::{Config, Role};
pub use open_story_server::router::{build_router, build_publisher_router};
pub use state::{AppState, SharedState, create_state};
pub use ingest::{ingest_events, is_plan_event, replay_boot_sessions, to_wire_record, IngestResult};

/// Start the server on the given host:port, with file watcher for live events.
///
/// Behavior depends on `config.role`:
/// - `Full`: watcher + consumer + API (default, current behavior)
/// - `Publisher`: watcher + hooks server, publishes to NATS, no local store/ingest
/// - `Consumer`: subscribes from NATS, runs ingest + API, no watcher
pub async fn run_server(
    host: &str,
    port: u16,
    data_dir: &Path,
    static_dir: Option<&Path>,
    watch_dir: &Path,
    bus: Arc<dyn Bus>,
    config: Config,
) -> Result<()> {
    let role = config.role;
    let is_consumer = matches!(role, Role::Consumer | Role::Full);
    let is_publisher = matches!(role, Role::Publisher | Role::Full);
    let pi_watch_dir = config.pi_watch_dir.clone();

    let state = create_state(data_dir, watch_dir, bus.clone(), config)?;

    // ── Banner ──
    {
        let mut s = state.write().await;
        let role_label = match role {
            Role::Full => "full",
            Role::Publisher => "publisher",
            Role::Consumer => "consumer",
        };
        eprintln!("\n\x1b[1m  Open Story\x1b[0m (\x1b[36m{role_label}\x1b[0m)");
        eprintln!("  \x1b[2m────────────────────────────────────\x1b[0m");

        if is_consumer {
            let session_count = s.store.event_store.list_sessions().unwrap_or_default().len();
            eprintln!("  \x1b[2mSessions loaded:\x1b[0m {session_count}");
            eprintln!("  \x1b[2mData dir:\x1b[0m       {}", data_dir.display());

            // Replay boot-loaded sessions through projections + pattern pipelines
            replay_boot_sessions(&mut s);
        }
    }

    // ── Router ──
    let router = {
        let s = state.read().await;
        match role {
            Role::Publisher => build_publisher_router(state.clone(), &s.config),
            _ => build_router(state.clone(), static_dir, &s.config),
        }
    };

    // ── Independent actor-consumers (consumer + full roles) ──
    if is_consumer && bus.is_active() {
        // Boot replay: recover state from JetStream event log
        match bus.replay("events.>").await {
            Ok(batches) => {
                if !batches.is_empty() {
                    let mut s = state.write().await;
                    let mut total = 0;
                    for batch in &batches {
                        let project_id = if batch.project_id.is_empty() { None } else { Some(batch.project_id.as_str()) };
                        let result = ingest_events(&mut s, &batch.session_id, &batch.events, project_id);
                        for change in result.changes {
                            let _ = s.broadcast_tx.send(change);
                        }
                        total += result.count;
                    }
                    if total > 0 {
                        log_event("boot", &format!(
                            "\x1b[34mbus replay\x1b[0m \x1b[32m+{total}\x1b[0m events from {} batches",
                            batches.len()
                        ));
                    }
                }
            }
            Err(e) => {
                eprintln!("  \x1b[33mBus replay failed: {e}\x1b[0m (using SessionStore fallback)");
            }
        }

        // ── Actor 1: persist consumer (owns dedup + storage) ──
        {
            let event_store = state.read().await.store.event_store.clone();
            let data_dir = state.read().await.store.data_dir.clone();
            let session_store = open_story_store::persistence::SessionStore::new(&data_dir)
                .expect("create session store for persist consumer");
            let persist_bus = bus.clone();
            tokio::spawn(async move {
                let mut actor = consumers::persist::PersistConsumer::new(event_store, session_store);
                match persist_bus.subscribe("events.>").await {
                    Ok(mut sub) => {
                        while let Some(batch) = sub.receiver.recv().await {
                            let result = actor.process_batch(&batch.session_id, &batch.events);
                            if result.persisted > 0 {
                                log_event("persist", &format!(
                                    "\x1b[33m{}\x1b[0m \x1b[32m+{}\x1b[0m persisted ({} skipped)",
                                    short_id(&batch.session_id), result.persisted, result.skipped
                                ));
                            }
                        }
                    }
                    Err(e) => eprintln!("Persist consumer error: {e}"),
                }
            });
        }

        // ── Actor 2: patterns consumer (owns pipelines, publishes PatternEvents) ──
        {
            let event_store = state.read().await.store.event_store.clone();
            let patterns_bus = bus.clone();
            tokio::spawn(async move {
                let mut actor = consumers::patterns::PatternsConsumer::new();
                match patterns_bus.subscribe("events.>").await {
                    Ok(mut sub) => {
                        while let Some(batch) = sub.receiver.recv().await {
                            let result = actor.process_batch(&batch.session_id, &batch.events);
                            // Persist turns and patterns to SQLite
                            for turn in &result.turns {
                                let _ = event_store.insert_turn(&batch.session_id, turn);
                            }
                            for pe in &result.patterns {
                                let _ = event_store.insert_pattern(&batch.session_id, pe);
                            }
                            if !result.patterns.is_empty() {
                                log_event("patterns", &format!(
                                    "\x1b[33m{}\x1b[0m \x1b[35m{} patterns, {} turns\x1b[0m",
                                    short_id(&batch.session_id), result.patterns.len(), result.turns.len()
                                ));
                            }
                        }
                    }
                    Err(e) => eprintln!("Patterns consumer error: {e}"),
                }
            });
        }

        // ── Actor 3: projections consumer (owns session metadata) ──
        {
            let projections_bus = bus.clone();
            tokio::spawn(async move {
                let mut actor = consumers::projections::ProjectionsConsumer::new();
                match projections_bus.subscribe("events.>").await {
                    Ok(mut sub) => {
                        while let Some(batch) = sub.receiver.recv().await {
                            actor.process_batch(&batch.session_id, &batch.events);
                        }
                    }
                    Err(e) => eprintln!("Projections consumer error: {e}"),
                }
            });
        }

        // ── Actor 4: broadcast consumer (uses ingest_events for now) ──
        // Still uses shared AppState because BroadcastMessage assembly depends
        // on projection state. This is the last consumer to decompose.
        {
            let broadcast_state = state.clone();
            let broadcast_bus = bus.clone();
            tokio::spawn(async move {
                match broadcast_bus.subscribe("events.>").await {
                    Ok(mut sub) => {
                        while let Some(batch) = sub.receiver.recv().await {
                            let summary = event_type_summary(&batch.events);
                            let session_id = batch.session_id.clone();
                            let mut s = broadcast_state.write().await;
                            let project_id = if batch.project_id.is_empty() { None } else { Some(batch.project_id.as_str()) };
                            let result = ingest_events(&mut s, &batch.session_id, &batch.events, project_id);
                            for change in &result.changes {
                                let _ = s.broadcast_tx.send(change.clone());
                            }
                            if result.count > 0 {
                                log_event("broadcast", &format!(
                                    "\x1b[33m{}\x1b[0m \x1b[32m+{}\x1b[0m ({})",
                                    short_id(&session_id), result.count, summary
                                ));
                            }
                            drop(s);
                        }
                    }
                    Err(e) => eprintln!("Broadcast consumer error: {e}"),
                }
            });
        }
    }

    // ── File watcher (publisher + full roles) ──
    if is_publisher {
        if bus.is_active() {
            // Bus mode: watcher → bus.publish()
            let watcher_bus = bus.clone();
            let watcher_dir = watch_dir.to_path_buf();
            tokio::task::spawn_blocking(move || {
                if let Err(e) = crate::watcher::watch_with_callback(&watcher_dir, true, |session_id, project_id, subject, events| {
                    let batch = IngestBatch {
                        session_id: session_id.to_string(),
                        project_id: project_id.unwrap_or("").to_string(),
                        events: events.to_vec(),
                    };
                    let rt = tokio::runtime::Handle::current();
                    if let Err(e) = rt.block_on(watcher_bus.publish(subject, &batch)) {
                        eprintln!("Bus publish error: {e}");
                    }
                }) {
                    eprintln!("Watcher error: {}", e);
                }
            });
        } else {
            // Local mode (no bus): watcher → direct ingest_events()
            let watcher_state = state.clone();
            let watcher_dir = watch_dir.to_path_buf();
            tokio::task::spawn_blocking(move || {
                if let Err(e) = crate::watcher::watch_with_callback(&watcher_dir, true, |session_id, project_id, _subject, events| {
                    let summary = event_type_summary(&events);
                    let rt = tokio::runtime::Handle::current();
                    let result = rt.block_on(async {
                        let mut s = watcher_state.write().await;
                        let result = ingest_events(&mut s, session_id, &events, project_id);
                        for change in &result.changes {
                            let _ = s.broadcast_tx.send(change.clone());
                        }
                        result
                    });
                    if result.count > 0 {
                        log_event("watch", &format!(
                            "\x1b[33m{}\x1b[0m \x1b[32m+{}\x1b[0m ({})",
                            short_id(session_id), result.count, summary
                        ));
                    }
                }) {
                    eprintln!("Watcher error: {}", e);
                }
            });
        }
    }

    // ── Pi-mono watcher (optional second watch directory) ──
    if is_publisher && !pi_watch_dir.is_empty() {
        let pi_dir = std::path::PathBuf::from(&pi_watch_dir);
        if pi_dir.exists() {
            if bus.is_active() {
                let watcher_bus = bus.clone();
                tokio::task::spawn_blocking(move || {
                    if let Err(e) = crate::watcher::watch_with_callback(&pi_dir, true, |session_id, project_id, subject, events| {
                        let batch = IngestBatch {
                            session_id: session_id.to_string(),
                            project_id: project_id.unwrap_or("").to_string(),
                            events: events.to_vec(),
                        };
                        let rt = tokio::runtime::Handle::current();
                        if let Err(e) = rt.block_on(watcher_bus.publish(subject, &batch)) {
                            eprintln!("Pi-mono bus publish error: {e}");
                        }
                    }) {
                        eprintln!("Pi-mono watcher error: {}", e);
                    }
                });
            } else {
                let watcher_state = state.clone();
                tokio::task::spawn_blocking(move || {
                    if let Err(e) = crate::watcher::watch_with_callback(&pi_dir, true, |session_id, project_id, _subject, events| {
                        let summary = event_type_summary(&events);
                        let rt = tokio::runtime::Handle::current();
                        let result = rt.block_on(async {
                            let mut s = watcher_state.write().await;
                            let result = ingest_events(&mut s, session_id, &events, project_id);
                            for change in &result.changes {
                                let _ = s.broadcast_tx.send(change.clone());
                            }
                            result
                        });
                        if result.count > 0 {
                            log_event("pi-watch", &format!(
                                "\x1b[33m{}\x1b[0m \x1b[32m+{}\x1b[0m ({})",
                                short_id(session_id), result.count, summary
                            ));
                        }
                    }) {
                        eprintln!("Pi-mono watcher error: {}", e);
                    }
                });
            }
            eprintln!("  \x1b[2mPi watch dir:\x1b[0m   {}", pi_watch_dir);
        }
    }

    // ── Bind and serve ──
    let addr = format!("{host}:{port}");
    if is_publisher {
        eprintln!("  \x1b[2mWatch dir:\x1b[0m      {}", watch_dir.display());
    }
    eprintln!("  \x1b[2mServing on:\x1b[0m      \x1b[4mhttp://{addr}\x1b[0m");
    eprintln!("  \x1b[2m────────────────────────────────────\x1b[0m\n");

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, router).await?;

    Ok(())
}
