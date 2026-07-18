//! Session citizenship API — Live (disk) vs Explore (store).
//!
//! BDD: when a session exists only on disk / only in store / both / neither,
//! GET /api/sessions/{id}/citizenship should return the matching verdict.

mod helpers;

use std::fs;
use std::path::PathBuf;

use axum::body::Body;
use axum::http::Request;
use helpers::{body_json, make_event, seed_and_ingest, send_request, test_state};
use open_story::server::ingest_events;
use open_story::server::watcher_diagnostics::{
    FileProcessObservation, WatcherActorConfig, WatcherProtocol,
};
use tempfile::TempDir;

/// when disk has updates.jsonl but store is empty → ghost
#[tokio::test]
async fn when_session_is_on_disk_only_it_should_report_ghost() {
    let tmp = TempDir::new().unwrap();
    let state = test_state(&tmp);
    let sid = "ghost-session-aaaa-bbbb-cccc-dddddddddddd";

    let grok_root = tmp.path().join("grok-sessions");
    let sess_dir = grok_root.join("proj-encoded").join(sid);
    fs::create_dir_all(&sess_dir).unwrap();
    fs::write(
        sess_dir.join("updates.jsonl"),
        r#"{"method":"session/update","params":{"update":{"sessionUpdate":"agent_message_chunk"}}}"#,
    )
    .unwrap();

    {
        let mut s = state.write().await;
        s.config.grok_watch_dir = grok_root.to_string_lossy().to_string();
    }

    let req = Request::get(format!("/api/sessions/{sid}/citizenship"))
        .body(Body::empty())
        .unwrap();
    let resp = send_request(state, req).await;
    assert_eq!(resp.status(), 200, "citizenship route must exist");
    let body = body_json(resp).await;
    assert_eq!(body["verdict"], "ghost");
    assert_eq!(body["disk"]["on_disk"], true);
    assert_eq!(body["store"]["in_store"], false);
    assert!(body["disk"]["updates_bytes"].as_u64().unwrap_or(0) > 0);
}

/// when store has events and disk has updates → citizen
#[tokio::test]
async fn when_session_is_on_disk_and_in_store_it_should_report_citizen() {
    let tmp = TempDir::new().unwrap();
    let state = test_state(&tmp);
    let sid = "citizen-session-aaaa-bbbb-cccc-dddddddddd";

    let grok_root = tmp.path().join("grok-sessions");
    let sess_dir = grok_root.join("proj").join(sid);
    fs::create_dir_all(&sess_dir).unwrap();
    fs::write(sess_dir.join("updates.jsonl"), "{}\n").unwrap();

    {
        let mut s = state.write().await;
        s.config.grok_watch_dir = grok_root.to_string_lossy().to_string();
        let ev = make_event("io.arc.event", sid);
        ingest_events(&mut s, sid, &[ev], None).await;
    }

    let req = Request::get(format!("/api/sessions/{sid}/citizenship"))
        .body(Body::empty())
        .unwrap();
    let resp = send_request(state, req).await;
    assert_eq!(resp.status(), 200);
    let body = body_json(resp).await;
    assert_eq!(body["verdict"], "citizen");
    assert_eq!(body["disk"]["on_disk"], true);
    assert_eq!(body["store"]["in_store"], true);
    assert!(body["store"]["event_count"].as_u64().unwrap_or(0) >= 1);
}

/// when store has events but no disk → orphan-store
#[tokio::test]
async fn when_session_is_in_store_only_it_should_report_orphan_store() {
    let tmp = TempDir::new().unwrap();
    let state = test_state(&tmp);
    let sid = "orphan-session-aaaa-bbbb-cccc-ddddddddddd";

    {
        let mut s = state.write().await;
        s.config.grok_watch_dir = tmp.path().join("empty-grok").to_string_lossy().to_string();
        let ev = make_event("io.arc.event", sid);
        ingest_events(&mut s, sid, &[ev], None).await;
    }

    let req = Request::get(format!("/api/sessions/{sid}/citizenship"))
        .body(Body::empty())
        .unwrap();
    let resp = send_request(state, req).await;
    assert_eq!(resp.status(), 200);
    let body = body_json(resp).await;
    assert_eq!(body["verdict"], "orphan-store");
    assert_eq!(body["disk"]["on_disk"], false);
    assert_eq!(body["store"]["in_store"], true);
}

/// when neither disk nor store know the id → absent
#[tokio::test]
async fn when_session_is_unknown_it_should_report_absent() {
    let tmp = TempDir::new().unwrap();
    let state = test_state(&tmp);
    let sid = "absent-session-aaaa-bbbb-cccc-dddddddddd";

    {
        let mut s = state.write().await;
        s.config.grok_watch_dir = tmp.path().join("empty-grok").to_string_lossy().to_string();
    }

    let req = Request::get(format!("/api/sessions/{sid}/citizenship"))
        .body(Body::empty())
        .unwrap();
    let resp = send_request(state, req).await;
    assert_eq!(resp.status(), 200);
    let body = body_json(resp).await;
    assert_eq!(body["verdict"], "absent");
    assert_eq!(body["disk"]["on_disk"], false);
    assert_eq!(body["store"]["in_store"], false);
}

/// when watchers have emitted CloudEvents but store has zero sessions → ghost_risk
#[tokio::test]
async fn when_watchers_emit_but_store_is_empty_health_should_flag_ghost_risk() {
    let tmp = TempDir::new().unwrap();
    let state = test_state(&tmp);
    let root = tmp.path().join("watch-root");
    fs::create_dir_all(&root).unwrap();

    {
        let s = state.read().await;
        let cfg = WatcherActorConfig::new("grok", WatcherProtocol::AppendJsonl, root.clone());
        let actor = s.watcher_diagnostics.register_actor(&cfg);
        s.watcher_diagnostics.record_file(
            &actor,
            FileProcessObservation {
                path: root.join("updates.jsonl"),
                canonical_path: root.join("updates.jsonl"),
                byte_offset_before: 0,
                byte_offset_after: 100,
                line_count_before: 0,
                line_count_after: 10,
                format: "grok".into(),
                events_emitted: 50,
                subtypes: vec!["message.assistant.text".into()],
            },
        );
    }

    let req = Request::get("/api/health").body(Body::empty()).unwrap();
    let resp = send_request(state, req).await;
    assert_eq!(resp.status(), 200);
    let body = body_json(resp).await;
    assert_eq!(
        body["citizenship"]["ghost_risk"], true,
        "watcher emitted with empty store must raise ghost_risk: {body}"
    );
    assert_eq!(body["citizenship"]["watcher_cloud_events_emitted"], 50);
    assert_eq!(body["citizenship"]["store_sessions"], 0);
}

/// when store has sessions, ghost_risk is false even if watchers emitted
#[tokio::test]
async fn when_store_has_sessions_health_should_not_flag_ghost_risk() {
    let tmp = TempDir::new().unwrap();
    let state = test_state(&tmp);
    let root = tmp.path().join("watch-root");
    fs::create_dir_all(&root).unwrap();

    {
        let mut s = state.write().await;
        let cfg = WatcherActorConfig::new("grok", WatcherProtocol::AppendJsonl, root.clone());
        let actor = s.watcher_diagnostics.register_actor(&cfg);
        s.watcher_diagnostics.record_file(
            &actor,
            FileProcessObservation {
                path: PathBuf::from("x"),
                canonical_path: PathBuf::from("x"),
                byte_offset_before: 0,
                byte_offset_after: 1,
                line_count_before: 0,
                line_count_after: 1,
                format: "grok".into(),
                events_emitted: 10,
                subtypes: vec![],
            },
        );
        // seed_and_ingest upserts the sessions row (ingest_events alone may not).
        let ev = make_event("io.arc.event", "sess-ok");
        seed_and_ingest(&mut s, "sess-ok", &[ev], None).await;
    }

    let req = Request::get("/api/health").body(Body::empty()).unwrap();
    let resp = send_request(state, req).await;
    assert_eq!(resp.status(), 200);
    let body = body_json(resp).await;
    assert_eq!(body["citizenship"]["ghost_risk"], false);
    assert!(body["citizenship"]["store_sessions"].as_u64().unwrap() >= 1);
}
