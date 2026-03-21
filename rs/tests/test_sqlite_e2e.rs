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

use open_story::server::{create_state, ingest_events, replay_boot_sessions, Config};
use open_story_bus::noop_bus::NoopBus;
use open_story_semantic::NoopSemanticStore;
use helpers::{body_json, send_request, synth};

use std::sync::Arc;

/// Full lifecycle: ingest → SQLite → API → restart → API.
///
/// Uses the ingest pipeline (not raw JSONL files) so events go through
/// the full translate → CloudEvent path and get proper IDs.
#[tokio::test]
async fn synthetic_data_survives_full_lifecycle() {
    let tmp = tempfile::tempdir().unwrap();
    let data_dir = tmp.path().join("data");
    let watch_dir = tmp.path().join("watch");
    std::fs::create_dir_all(&data_dir).unwrap();
    std::fs::create_dir_all(&watch_dir).unwrap();

    // ── Step 1: Boot empty, then ingest events through the pipeline ──
    let state = create_state(&data_dir, &watch_dir, Arc::new(NoopBus), Arc::new(NoopSemanticStore), Config::default()).unwrap();

    let sessions = ["sess-alpha", "sess-beta", "sess-gamma"];
    let events_per_session = 15;

    {
        let mut s = state.write().await;
        for sid in &sessions {
            let mut events = Vec::new();
            for i in 0..events_per_session {
                // Alternate between user prompts, tool uses, and assistant text
                let event = match i % 3 {
                    0 => helpers::make_user_prompt(sid, &format!("{}-evt-{}", sid, i)),
                    1 => helpers::make_tool_use(sid, &format!("{}-evt-{}", sid, i), None, "Bash", "cargo test"),
                    _ => helpers::make_assistant_text(sid, &format!("{}-evt-{}", sid, i), None, "Here is the result."),
                };
                events.push(event);
            }
            ingest_events(&mut s, sid, &events, Some("test-project"));
        }
    }

    // ── Step 2: Verify API serves events from SQLite ──
    let req = Request::get("/api/sessions/sess-alpha/events")
        .body(Body::empty()).unwrap();
    let resp = send_request(Arc::clone(&state), req).await;
    assert_eq!(resp.status(), 200);
    let events: Value = body_json(resp).await;
    let event_count = events.as_array().unwrap().len();
    assert_eq!(event_count, events_per_session, "API should return all ingested events");

    // Verify session summary
    let req = Request::get("/api/sessions/sess-beta/summary")
        .body(Body::empty()).unwrap();
    let resp = send_request(Arc::clone(&state), req).await;
    assert_eq!(resp.status(), 200);
    let summary: Value = body_json(resp).await;
    assert_eq!(summary["session_id"], "sess-beta");
    assert!(summary["event_count"].as_u64().unwrap() > 0);

    // Verify view-records endpoint
    let req = Request::get("/api/sessions/sess-gamma/view-records")
        .body(Body::empty()).unwrap();
    let resp = send_request(Arc::clone(&state), req).await;
    assert_eq!(resp.status(), 200);
    let view_records: Value = body_json(resp).await;
    assert!(!view_records.as_array().unwrap().is_empty(), "should have view records");

    // Verify SQLite has the events
    let sqlite_event_count = {
        let s = state.read().await;
        s.store.event_store.session_events("sess-alpha").unwrap().len()
    };
    assert_eq!(sqlite_event_count, event_count, "SQLite should have same events as API");

    // ── Step 3: Simulate restart — drop state, reboot from SQLite ──
    drop(state);

    // Delete any JSONL files that were dual-written — SQLite is the only survivor
    for entry in std::fs::read_dir(&data_dir).unwrap().flatten() {
        if entry.path().extension().map(|e| e == "jsonl").unwrap_or(false) {
            std::fs::remove_file(entry.path()).unwrap();
        }
    }
    // Also remove plans dir to prove we don't need filesystem persistence
    let plans_dir = data_dir.join("plans");
    if plans_dir.exists() {
        let _ = std::fs::remove_dir_all(&plans_dir);
    }

    // Second boot — should load from SQLite
    let state2 = create_state(&data_dir, &watch_dir, Arc::new(NoopBus), Arc::new(NoopSemanticStore), Config::default()).unwrap();

    // ── Step 4: API still serves data after restart ──
    let req = Request::get("/api/sessions/sess-alpha/events")
        .body(Body::empty()).unwrap();
    let resp = send_request(Arc::clone(&state2), req).await;
    assert_eq!(resp.status(), 200);
    let events2: Value = body_json(resp).await;
    assert_eq!(
        events2.as_array().unwrap().len(),
        event_count,
        "same event count after restart from SQLite"
    );

    // Verify all 3 sessions survived
    {
        let s = state2.read().await;
        assert_eq!(s.store.event_store.list_sessions().unwrap().len(), 3, "all sessions should survive restart");
    }

    // Verify session list API
    let req = Request::get("/api/sessions")
        .body(Body::empty()).unwrap();
    let resp = send_request(Arc::clone(&state2), req).await;
    assert_eq!(resp.status(), 200);
    let sessions_json: Value = body_json(resp).await;
    assert_eq!(
        sessions_json.as_array().unwrap().len(), 3,
        "session list should show all 3 sessions after SQLite boot"
    );
}

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

    let state = create_state(&data_dir, &watch_dir, Arc::new(NoopBus), Arc::new(NoopSemanticStore), Config::default()).unwrap();
    {
        let mut s = state.write().await;
        replay_boot_sessions(&mut s);
    }

    // Check patterns were detected
    let pattern_count = {
        let s = state.read().await;
        s.store.event_store
            .session_patterns("perf-sess-000", None)
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

    let state2 = create_state(&data_dir, &watch_dir, Arc::new(NoopBus), Arc::new(NoopSemanticStore), Config::default()).unwrap();

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

    let state = create_state(&data_dir, &watch_dir, Arc::new(NoopBus), Arc::new(NoopSemanticStore), Config::default()).unwrap();

    // Ingest events programmatically (simulating watcher/hooks)
    {
        let mut s = state.write().await;
        let events: Vec<_> = (0..10).map(|i| {
            helpers::make_user_prompt("live-session", &format!("live-evt-{}", i))
        }).collect();
        ingest_events(&mut s, "live-session", &events, Some("test-project"));
    }

    // Verify SQLite has the events
    {
        let s = state.read().await;
        let stored = s.store.event_store.session_events("live-session").unwrap();
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
    let state2 = create_state(&data_dir, &watch_dir, Arc::new(NoopBus), Arc::new(NoopSemanticStore), Config::default()).unwrap();

    let req = Request::get("/api/sessions/live-session/events")
        .body(Body::empty()).unwrap();
    let resp = send_request(Arc::clone(&state2), req).await;
    let events2: Value = body_json(resp).await;
    assert_eq!(
        events2.as_array().unwrap().len(), 10,
        "live-ingested events should survive restart via SQLite"
    );
}
