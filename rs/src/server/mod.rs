//! Axum web server module — orchestration layer.
//!
//! Server logic lives in the `open-story-server` crate. This module re-exports
//! those types and retains `run_server` which wires the file watcher + server + bus.

pub mod analysis;
pub mod api;
pub mod broadcast;
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
use open_story_server::logging::{event_type_summary, log_event, short_id};

pub use broadcast::BroadcastMessage;
pub use open_story_server::config;
pub use open_story_server::config::{Config, Role};
pub use open_story_server::consumers;
pub use open_story_server::reconcile;
pub use open_story_server::router::{build_router, build_publisher_router};
pub use state::{AppState, SharedState, create_state};
pub use ingest::{ingest_events, is_plan_event, replay_boot_sessions, to_wire_record, IngestResult, ReplayContext};

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
    let hermes_watch_dir = config.hermes_watch_dir.clone();

    let state = create_state(data_dir, watch_dir, bus.clone(), config).await?;

    // ── Banner ──
    {
        let s = state.read().await;
        let role_label = match role {
            Role::Full => "full",
            Role::Publisher => "publisher",
            Role::Consumer => "consumer",
        };
        eprintln!("\n\x1b[1m  Open Story\x1b[0m (\x1b[36m{role_label}\x1b[0m)");
        eprintln!("  \x1b[2m────────────────────────────────────\x1b[0m");

        if is_consumer {
            let session_count = s.store.event_store.list_sessions().await.unwrap_or_default().len();
            eprintln!("  \x1b[2mSessions loaded:\x1b[0m {session_count}");
            eprintln!("  \x1b[2mData dir:\x1b[0m       {}", data_dir.display());
        }
    }

    // ── Async boot replay ──
    //
    // Spawn replay_boot_sessions in the background so the HTTP listener
    // binds immediately. Projections populate asynchronously from SQLite;
    // REST/WS serve partial state during the replay window (eventually
    // consistent by design — no data loss since events are durable).
    //
    // Previously this ran inline before the bind, making startup
    // O(lifetime_events) — visibly minutes-long with large data dirs.
    // Now startup is O(1); full projection rebuild happens concurrently.
    if is_consumer {
        // Snapshot everything replay needs and drop the read guard
        // before spawning. The spawned future owns only `Arc`s and
        // small owned HashMaps, so it never contends with the outer
        // `RwLock<AppState>` — API reads stay unblocked during replay.
        let ctx = {
            let s = state.read().await;
            ReplayContext {
                event_store: s.store.event_store.clone(),
                projections: s.store.projections.clone(),
                subagent_parents: s.store.subagent_parents.clone(),
                session_children: s.store.session_children.clone(),
                full_payloads: s.store.full_payloads.clone(),
                session_projects: s.store.session_projects.clone(),
                session_project_names: s.store.session_project_names.clone(),
            }
        };
        tokio::spawn(async move {
            let start = std::time::Instant::now();
            replay_boot_sessions(&ctx).await;
            let elapsed = start.elapsed();
            log_event(
                "boot",
                &format!("async replay complete in {}s", elapsed.as_secs()),
            );
        });
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
    //
    // NATS JetStream is a hard requirement (commit 1.1). The four actors
    // unconditionally spawn; `bus.is_active()` gating is gone.
    //
    // Historical events from JetStream reach the actors via their own
    // `bus.subscribe("events.>")` calls — the durable-consumer semantics
    // deliver the replay stream to each subscriber independently. The
    // previous inline `bus.replay(...)` + `ingest_events` path was
    // redundant (it processed the same events the actors would have
    // received) and has been deleted. The Actor 1/2/3/4 subscriptions
    // below are the sole ingestion route.
    if is_consumer {
        // ── Actor 1: persist consumer (owns dedup + storage + session row) ──
        //
        // As of commit 1.5, PersistConsumer is the single writer of the
        // SessionRow. It upserts after the batch's events are durable;
        // ingest_events no longer touches the `sessions` table.
        {
            let (event_store, data_dir, shared_projections, shared_projects, shared_names) = {
                let s = state.read().await;
                (
                    s.store.event_store.clone(),
                    s.store.data_dir.clone(),
                    s.store.projections.clone(),
                    s.store.session_projects.clone(),
                    s.store.session_project_names.clone(),
                )
            };
            let session_store = open_story_store::persistence::SessionStore::new(&data_dir)
                .expect("create session store for persist consumer");
            let persist_bus = bus.clone();
            tokio::spawn(async move {
                let mut actor = consumers::persist::PersistConsumer::new(
                    event_store,
                    session_store,
                    shared_projections,
                    shared_projects,
                    shared_names,
                );
                match persist_bus.subscribe("events.>").await {
                    Ok(mut sub) => {
                        while let Some(batch) = sub.receiver.recv().await {
                            let project_id = if batch.project_id.is_empty() {
                                None
                            } else {
                                Some(batch.project_id.as_str())
                            };
                            let result = actor
                                .process_batch(&batch.session_id, &batch.events, project_id)
                                .await;
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

        // ── Actor 2: patterns consumer ──
        // The sole pattern detector. Subscribes to events.>, runs the
        // eval-apply + sentence pipeline, persists turns + patterns to the
        // EventStore, mirrors the pattern stream into AppState's
        // detected_patterns cache (so build_initial_state can serve them
        // to fresh WebSocket clients), and publishes patterns to NATS
        // patterns.{project}.{session} so the next branch's broadcast
        // consumer rewire can subscribe and forward live.
        {
            let event_store = state.read().await.store.event_store.clone();
            let patterns_bus = bus.clone();
            let patterns_state = state.clone();
            tokio::spawn(async move {
                let mut actor = consumers::patterns::PatternsConsumer::new();
                match patterns_bus.subscribe("events.>").await {
                    Ok(mut sub) => {
                        while let Some(batch) = sub.receiver.recv().await {
                            let result = actor.process_batch(&batch.session_id, &batch.events);

                            // Persist turns and patterns to the EventStore
                            for turn in &result.turns {
                                let _ = event_store.insert_turn(&batch.session_id, turn).await;
                            }
                            for pe in &result.patterns {
                                let _ = event_store.insert_pattern(&batch.session_id, pe).await;
                            }

                            if !result.patterns.is_empty() {
                                // Mirror into AppState.detected_patterns so
                                // the WebSocket initial_state handshake can
                                // serve them to fresh clients without a DB
                                // roundtrip. Shared Arc<DashMap> — no outer
                                // lock needed.
                                {
                                    let s = patterns_state.read().await;
                                    let mut entry = s
                                        .store
                                        .detected_patterns
                                        .entry(batch.session_id.clone())
                                        .or_default();
                                    entry.extend(result.patterns.iter().cloned());
                                }

                                // Publish to patterns.{project}.{session}
                                // for downstream consumers (live story
                                // broadcast, etc.). Best-effort: a publish
                                // failure logs but doesn't block the
                                // pipeline — the patterns are already
                                // durable in the EventStore.
                                let project = if batch.project_id.is_empty() {
                                    "default"
                                } else {
                                    batch.project_id.as_str()
                                };
                                let subject = format!(
                                    "patterns.{}.{}",
                                    project, batch.session_id,
                                );
                                if let Ok(payload) = serde_json::to_vec(&result.patterns) {
                                    if let Err(e) = patterns_bus
                                        .publish_bytes(&subject, &payload)
                                        .await
                                    {
                                        eprintln!(
                                            "patterns consumer publish_bytes({subject}) failed: {e}"
                                        );
                                    }
                                }

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
        //
        // The consumer writes into the *shared* projections map on
        // `state.store.projections` (an Arc<DashMap>). API, WebSocket,
        // and other consumers read from that same map. No sync needed.
        {
            let projections_bus = bus.clone();
            let (shared_projections, shared_parents, shared_children) = {
                let s = state.read().await;
                (
                    s.store.projections.clone(),
                    s.store.subagent_parents.clone(),
                    s.store.session_children.clone(),
                )
            };
            tokio::spawn(async move {
                let mut actor = consumers::projections::ProjectionsConsumer::new(
                    shared_projections,
                    shared_parents,
                    shared_children,
                );
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

        // ── Actor 4: broadcast consumer (WebSocket assembly only) ──
        //
        // As of commit 1.5, Actor 4 uses `BroadcastConsumer::process_batch`
        // directly rather than going through the legacy `ingest_events`
        // orchestrator. It doesn't touch SQLite anymore — PersistConsumer
        // (Actor 1) owns event rows and the session row. Actor 4 just
        // transforms CloudEvents → BroadcastMessages and fans them out
        // to WebSocket clients via `broadcast_tx`.
        {
            let broadcast_state = state.clone();
            let broadcast_bus = bus.clone();
            tokio::spawn(async move {
                let mut consumer = consumers::broadcast::BroadcastConsumer::new();
                match broadcast_bus.subscribe("events.>").await {
                    Ok(mut sub) => {
                        while let Some(batch) = sub.receiver.recv().await {
                            let summary = event_type_summary(&batch.events);
                            let session_id = batch.session_id.clone();
                            let project_id = if batch.project_id.is_empty() {
                                None
                            } else {
                                Some(batch.project_id.clone())
                            };

                            // Snapshot projection + project display name from
                            // the shared DashMaps. Drop the outer read guard
                            // before invoking process_batch so API readers
                            // and other actors aren't contended on the tokio
                            // RwLock.
                            let (projection, project_name, tx) = {
                                let s = broadcast_state.read().await;
                                let proj = s
                                    .store
                                    .projections
                                    .get(&session_id)
                                    .map(|r| r.value().clone());
                                let pname = s
                                    .store
                                    .session_project_names
                                    .get(&session_id)
                                    .map(|r| r.value().clone());
                                (proj, pname, s.broadcast_tx.clone())
                            };

                            let Some(proj_snapshot) = projection else {
                                // ProjectionsConsumer hasn't processed this
                                // session's first batch yet — skip broadcast
                                // this tick; the next batch will have a
                                // snapshot and catch up.
                                continue;
                            };

                            let messages = consumer.process_batch(
                                &session_id,
                                &batch.events,
                                &proj_snapshot,
                                project_id,
                                project_name,
                            );
                            let emitted = messages.len();
                            for msg in messages {
                                let _ = tx.send(msg);
                            }

                            if emitted > 0 {
                                log_event("broadcast", &format!(
                                    "\x1b[33m{}\x1b[0m \x1b[32m+{}\x1b[0m ({})",
                                    short_id(&session_id), emitted, summary
                                ));
                            }
                        }
                    }
                    Err(e) => eprintln!("Broadcast consumer error: {e}"),
                }
            });
        }
    }

    // ── File watcher (publisher + full roles) ──
    // Snapshot the backfill window from config before any closures move it.
    let backfill_window: Option<u64> = Some(state.read().await.config.watch_backfill_hours);
    if is_publisher {
        // NATS required (commit 1.1): the watcher always publishes to the
        // bus. The old `else { ... direct ingest_events() ... }` branch
        // for local-mode operation was unreachable and has been deleted.
        let watcher_bus = bus.clone();
        let watcher_dir = watch_dir.to_path_buf();
        tokio::task::spawn_blocking(move || {
            if let Err(e) = crate::watcher::watch_with_callback(&watcher_dir, backfill_window, |session_id, project_id, subject, events| {
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
    }

    // ── Pi-mono watcher (optional second watch directory) ──
    if is_publisher && !pi_watch_dir.is_empty() {
        let pi_dir = std::path::PathBuf::from(&pi_watch_dir);
        if pi_dir.exists() {
            let watcher_bus = bus.clone();
            tokio::task::spawn_blocking(move || {
                if let Err(e) = crate::watcher::watch_with_callback(&pi_dir, backfill_window, |session_id, project_id, subject, events| {
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
            eprintln!("  \x1b[2mPi watch dir:\x1b[0m   {}", pi_watch_dir);
        }
    }

    // ── Hermes snapshot watcher (separate actor from JSONL watcher) ──
    //
    // Hermes Agent writes session_*.json as atomic snapshots (temp → fsync →
    // os.replace). This is a fundamentally different model from Claude Code's
    // append-only JSONL. The snapshot_watcher is a separate actor that diffs
    // the snapshot against previously-seen state and emits new messages.
    //
    // Config: hermes_watch_dir points to ~/.hermes/sessions/ (or wherever
    // Hermes writes its session_*.json files).
    if is_publisher && !hermes_watch_dir.is_empty() {
        let hermes_dir = std::path::PathBuf::from(&hermes_watch_dir);
        if hermes_dir.exists() {
            let watcher_bus = bus.clone();
            tokio::task::spawn_blocking(move || {
                if let Err(e) = crate::snapshot_watcher::watch_snapshots(&hermes_dir, backfill_window, |session_id, project_id, subject, events| {
                    let batch = IngestBatch {
                        session_id: session_id.to_string(),
                        project_id: project_id.unwrap_or("").to_string(),
                        events: events.to_vec(),
                    };
                    let rt = tokio::runtime::Handle::current();
                    if let Err(e) = rt.block_on(watcher_bus.publish(subject, &batch)) {
                        eprintln!("Hermes bus publish error: {e}");
                    }
                }) {
                    eprintln!("Hermes snapshot watcher error: {}", e);
                }
            });
            eprintln!("  \x1b[2mHermes watch dir:\x1b[0m {} (snapshot mode)", hermes_watch_dir);
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
