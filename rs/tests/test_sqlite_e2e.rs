//! End-to-end integration test: synthetic data → ingest → SQLite → API.
//!
//! Tests the full lifecycle:
//! 1. Generate synthetic transcript JSONL files
//! 2. Boot from JSONL (first run, SQLite empty)
//! 3. Replay populates SQLite via dual-write
//! 4. API serves events from SQLite
//! 5. Simulate restart — boot from SQLite (no JSONL needed)
//! 6. API still serves the same data

mod helpers;

use axum::body::Body;
use axum::http::Request;
use serde_json::Value;

use open_story::server::{create_state, ingest_events, replay_boot_sessions, Config, ReplayContext};
use open_story_bus::noop_bus::NoopBus;
use helpers::{body_json, send_request, synth};

use std::sync::Arc;

/// Full lifecycle: ingest → SQLite → API → restart → API.
///
// `synthetic_data_survives_full_lifecycle` retired — it asserted that
// session rows written via `ingest_events` would survive a restart and
// be visible in `event_store.list_sessions()` + `/api/sessions`.
// Steps 1-2 of the test (events round-trip through the API + appear in
// SQLite) work fine. Step 4 fails because the actor decomposition
// (Phase 1.4.5) moved session-row writes from `ingest_events` itself
// to PersistConsumer, which subscribes to the bus. With NoopBus, no
// consumer runs, so events land in the events table but the sessions
// table stays empty — restart-from-SQLite then sees 0 sessions even
// though the events are there.
//
// Equivalent coverage in the new world:
//   - `state.rs::tests::boot_from_sqlite_when_db_has_sessions` —
//     pre-populates SQLite directly (bypasses ingest), then verifies
//     boot loads the rows. Tests the boot path; doesn't depend on
//     PersistConsumer running in-test.
//   - `consumers::persist::tests::dedup_*` — tests that PersistConsumer
//     writes the session row + dedups by event_id PK. Tests the
//     ingestion path.
//   - `test_compose::*` — full end-to-end with NATS + real consumers
//     against a docker compose. Tests both halves stitched together
//     for real.

/// Verify that patterns detected during replay are persisted in SQLite
/// and survive a restart.
#[tokio::test]
async fn patterns_survive_restart() {
    let tmp = tempfile::tempdir().unwrap();
    let data_dir = tmp.path().join("data");
    let watch_dir = tmp.path().join("watch");
    std::fs::create_dir_all(&data_dir).unwrap();
    std::fs::create_dir_all(&watch_dir).unwrap();

    // Generate enough events to trigger pattern detection (200 per session)
    synth::generate_fixture_dir(&data_dir, 1, 200, 0);

    let state = create_state(&data_dir, &watch_dir, Arc::new(NoopBus), Config::default()).await.unwrap();
    {
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
        replay_boot_sessions(&ctx).await;
    }

    // Check patterns were detected
    let pattern_count = {
        let s = state.read().await;
        s.store.event_store
            .session_patterns("perf-sess-000", None)
            .await
            .unwrap()
            .len()
    };

    // Patterns API should work
    let req = Request::get("/api/sessions/perf-sess-000/patterns")
        .body(Body::empty()).unwrap();
    let resp = send_request(Arc::clone(&state), req).await;
    assert_eq!(resp.status(), 200);
    let patterns: Value = body_json(resp).await;
    let api_pattern_count = patterns["patterns"].as_array().unwrap().len();
    assert_eq!(api_pattern_count, pattern_count, "API should serve all patterns from SQLite");

    // Restart
    drop(state);
    for path in std::fs::read_dir(&data_dir).unwrap().flatten() {
        if path.path().extension().map(|e| e == "jsonl").unwrap_or(false) {
            std::fs::remove_file(path.path()).unwrap();
        }
    }

    let state2 = create_state(&data_dir, &watch_dir, Arc::new(NoopBus), Config::default()).await.unwrap();

    // Patterns should still be in SQLite after restart
    let req = Request::get("/api/sessions/perf-sess-000/patterns")
        .body(Body::empty()).unwrap();
    let resp = send_request(Arc::clone(&state2), req).await;
    assert_eq!(resp.status(), 200);
    let patterns2: Value = body_json(resp).await;
    assert_eq!(
        patterns2["patterns"].as_array().unwrap().len(),
        pattern_count,
        "patterns should survive restart via SQLite"
    );
}

/// Verify that new events ingested after boot are persisted to SQLite.
#[tokio::test]
async fn live_ingest_persists_to_sqlite() {
    let tmp = tempfile::tempdir().unwrap();
    let data_dir = tmp.path().join("data");
    let watch_dir = tmp.path().join("watch");
    std::fs::create_dir_all(&data_dir).unwrap();
    std::fs::create_dir_all(&watch_dir).unwrap();

    let state = create_state(&data_dir, &watch_dir, Arc::new(NoopBus), Config::default()).await.unwrap();

    // Ingest events programmatically (simulating watcher/hooks)
    {
        let mut s = state.write().await;
        let events: Vec<_> = (0..10).map(|i| {
            helpers::make_user_prompt("live-session", &format!("live-evt-{}", i))
        }).collect();
        ingest_events(&mut s, "live-session", &events, Some("test-project")).await;
    }

    // Verify SQLite has the events
    {
        let s = state.read().await;
        let stored = s.store.event_store.session_events("live-session").await.unwrap();
        assert_eq!(stored.len(), 10, "SQLite should have all 10 ingested events");
    }

    // Verify API serves them
    let req = Request::get("/api/sessions/live-session/events")
        .body(Body::empty()).unwrap();
    let resp = send_request(Arc::clone(&state), req).await;
    let events: Value = body_json(resp).await;
    assert_eq!(events.as_array().unwrap().len(), 10);

    // Restart — should find events in SQLite
    drop(state);
    let state2 = create_state(&data_dir, &watch_dir, Arc::new(NoopBus), Config::default()).await.unwrap();

    let req = Request::get("/api/sessions/live-session/events")
        .body(Body::empty()).unwrap();
    let resp = send_request(Arc::clone(&state2), req).await;
    let events2: Value = body_json(resp).await;
    assert_eq!(
        events2.as_array().unwrap().len(), 10,
        "live-ingested events should survive restart via SQLite"
    );
}
