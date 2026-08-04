//! Integration tests for /api/reels — CRUD over saved, replayable story
//! sequences, with event-reference validation on POST.

mod helpers;

use axum::body::Body;
use axum::http::Request;
use helpers::{body_json, send_request, test_state};
use serde_json::json;
use tempfile::TempDir;

/// Seed one real event so a stop can validate against the store.
async fn seed_event(state: &open_story::server::SharedState) -> (String, String) {
    let session_id = "reel-test-session".to_string();
    let event_id = "reel-test-event-1".to_string();
    let event = json!({
        "id": event_id,
        "specversion": "1.0",
        "type": "io.arc.event",
        "source": "test",
        "time": "2026-08-04T00:00:00Z",
        "data": {"subtype": "message.user.prompt", "raw": {"text": "hello"}}
    });
    state
        .read()
        .await
        .store
        .event_store
        .insert_event(&session_id, &event)
        .await
        .unwrap();
    (session_id, event_id)
}

fn post_json(uri: &str, body: &serde_json::Value) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(uri)
        .header("Content-Type", "application/json")
        .body(Body::from(serde_json::to_vec(body).unwrap()))
        .unwrap()
}

#[tokio::test]
async fn post_reel_with_real_event_saves_and_lists() {
    let data_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);
    let (sid, eid) = seed_event(&state).await;
    let body = json!({
        "title": "Test reel",
        "created": "2026-08-04T18:00:00Z",
        "author": "test",
        "closer": "fin",
        "stops": [{"sessionId": sid, "eventId": eid, "line": "one line"}]
    });
    let resp = send_request(state.clone(), post_json("/api/reels", &body)).await;
    assert_eq!(resp.status(), 200);
    let v = body_json(resp).await;
    assert_eq!(v["ok"], json!(true));
    let id = v["id"].as_str().unwrap().to_string();
    assert!(id.starts_with("reel-"));

    let req = Request::get("/api/reels").body(Body::empty()).unwrap();
    let resp = send_request(state.clone(), req).await;
    let list = body_json(resp).await;
    assert_eq!(list.as_array().unwrap().len(), 1);
    assert_eq!(list[0]["stopCount"], json!(1));

    let req = Request::get(format!("/api/reels/{id}")).body(Body::empty()).unwrap();
    let resp = send_request(state.clone(), req).await;
    assert_eq!(resp.status(), 200);
    let reel = body_json(resp).await;
    assert_eq!(reel["stops"][0]["eventId"], json!(eid));
}

#[tokio::test]
async fn post_reel_with_invented_event_is_422_naming_offenders() {
    let data_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);
    let (sid, _eid) = seed_event(&state).await;
    let body = json!({
        "title": "Dishonest reel",
        "stops": [{"sessionId": sid, "eventId": "fabricated-evt", "line": "x"}]
    });
    let resp = send_request(state.clone(), post_json("/api/reels", &body)).await;
    assert_eq!(resp.status(), 422);
    let v = body_json(resp).await;
    assert_eq!(v["ok"], json!(false));
    assert_eq!(v["invalid_stops"][0]["eventId"], json!("fabricated-evt"));
}

#[tokio::test]
async fn get_missing_reel_404_and_delete_round_trip() {
    let data_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);
    let req = Request::get("/api/reels/reel-none").body(Body::empty()).unwrap();
    let resp = send_request(state.clone(), req).await;
    assert_eq!(resp.status(), 404);

    let (sid, eid) = seed_event(&state).await;
    let body = json!({"title": "t", "stops": [{"sessionId": sid, "eventId": eid, "line": "l"}]});
    let resp = send_request(state.clone(), post_json("/api/reels", &body)).await;
    let id = body_json(resp).await["id"].as_str().unwrap().to_string();

    let req = Request::builder()
        .method("DELETE")
        .uri(format!("/api/reels/{id}"))
        .body(Body::empty())
        .unwrap();
    let resp = send_request(state.clone(), req).await;
    assert_eq!(body_json(resp).await["ok"], json!(true));

    let req = Request::get(format!("/api/reels/{id}")).body(Body::empty()).unwrap();
    let resp = send_request(state.clone(), req).await;
    assert_eq!(resp.status(), 404);
}
