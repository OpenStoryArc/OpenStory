//! Integration tests for POST /api/watch/{session_id} — agent-directed
//! watch focus. The endpoint emits a `BroadcastMessage::Focus` over the
//! existing broadcast channel so an already-live UI can switch focus to the
//! session. No new transport: the focus signal rides `broadcast_tx -> /ws`.

mod helpers;

use axum::body::Body;
use axum::http::Request;
use helpers::{body_json, make_event, seed_and_ingest, send_request, test_state};
use tempfile::TempDir;

use open_story::server::broadcast::BroadcastMessage;

#[tokio::test]
async fn test_watch_session_broadcasts_focus_to_subscribers() {
    let data_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);

    // Seed a session so it resolves in the store.
    {
        let mut s = state.write().await;
        let events = vec![make_event("io.arc.event", "sess-watch")];
        seed_and_ingest(&mut s, "sess-watch", &events, None).await;
    }

    // Expected enrichment is derived from the same store row the handler
    // reads, so the test pins "handler enriches from the store" without
    // hardcoding what the seed happens to produce.
    let expected = {
        let s = state.read().await;
        s.store
            .event_store
            .list_sessions()
            .await
            .unwrap()
            .into_iter()
            .find(|r| r.id == "sess-watch")
            .expect("seeded session present in store")
    };

    // Subscribe BEFORE firing so we catch the broadcast.
    let mut rx = { state.read().await.broadcast_tx.subscribe() };

    let req = Request::post("/api/watch/sess-watch")
        .body(Body::empty())
        .unwrap();
    let resp = send_request(state.clone(), req).await;
    assert_eq!(resp.status(), 200);

    let body = body_json(resp).await;
    assert_eq!(body["status"], "focusing");
    assert_eq!(body["session_id"], "sess-watch");
    assert_eq!(
        body["delivered_to"].as_u64(),
        Some(1),
        "one subscriber connected"
    );

    // The subscriber receives a Focus message enriched from the store.
    let msg = rx.try_recv().expect("focus message should have been broadcast");
    match msg {
        BroadcastMessage::Focus {
            session_id,
            label,
            project_name,
            host,
            user,
        } => {
            assert_eq!(session_id, "sess-watch");
            assert_eq!(label, expected.label);
            assert_eq!(project_name, expected.project_name);
            assert_eq!(host, expected.host);
            assert_eq!(user, expected.user);
        }
        other => panic!("expected BroadcastMessage::Focus, got {other:?}"),
    }
}

#[tokio::test]
async fn test_watch_unknown_session_returns_404() {
    let data_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);

    let req = Request::post("/api/watch/does-not-exist")
        .body(Body::empty())
        .unwrap();
    let resp = send_request(state, req).await;
    assert_eq!(
        resp.status(),
        404,
        "watching an unknown session is an honest 404"
    );
}

#[tokio::test]
async fn test_watch_with_no_ui_reports_zero_delivered() {
    let data_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);

    {
        let mut s = state.write().await;
        let events = vec![make_event("io.arc.event", "sess-lonely")];
        seed_and_ingest(&mut s, "sess-lonely", &events, None).await;
    }

    // No subscriber is created, so the broadcast reaches zero UIs. The
    // endpoint still succeeds — it just reports the honest reach so the
    // agent can tell the user to open the UI.
    let req = Request::post("/api/watch/sess-lonely")
        .body(Body::empty())
        .unwrap();
    let resp = send_request(state, req).await;
    assert_eq!(resp.status(), 200);

    let body = body_json(resp).await;
    assert_eq!(body["delivered_to"].as_u64(), Some(0));
}
