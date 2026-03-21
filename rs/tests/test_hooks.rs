//! Integration tests for POST /hooks endpoint.

mod helpers;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use helpers::{body_json, send_request, test_state};
use serde_json::json;
use std::io::Write;
use tempfile::TempDir;

/// Write synthetic transcript lines to a temp file, return the path.
fn write_transcript(dir: &TempDir, filename: &str, lines: &[&str]) -> String {
    let path = dir.path().join(filename);
    let mut f = std::fs::File::create(&path).unwrap();
    for line in lines {
        writeln!(f, "{}", line).unwrap();
    }
    path.to_string_lossy().to_string()
}

fn transcript_line(uuid: &str, msg_type: &str, session_id: &str) -> String {
    json!({
        "type": msg_type,
        "uuid": uuid,
        "sessionId": session_id,
        "timestamp": "2025-01-05T17:00:00.000Z",
        "message": {"role": msg_type, "content": "test content"}
    })
    .to_string()
}

#[tokio::test]
async fn test_hooks_with_transcript() {
    let data_dir = TempDir::new().unwrap();
    let transcript_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);

    let path = write_transcript(
        &transcript_dir,
        "sess-1.jsonl",
        &[
            &transcript_line("u-001", "user", "sess-1"),
            &transcript_line("u-002", "assistant", "sess-1"),
        ],
    );

    let req = Request::post("/hooks")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({"session_id": "sess-1", "transcript_path": path}).to_string(),
        ))
        .unwrap();

    let resp = send_request(state.clone(), req).await;
    assert_eq!(resp.status(), StatusCode::ACCEPTED);

    let body = body_json(resp).await;
    assert_eq!(body["status"], "ok");
    assert!(body["events"].as_u64().unwrap() >= 2);

    // Verify events are in session state
    let s = state.read().await;
    let events = s.store.event_store.session_events("sess-1").unwrap();
    assert!(events.len() >= 2);
}

#[tokio::test]
async fn test_hooks_no_transcript_path() {
    let data_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);

    let req = Request::post("/hooks")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({"session_id": "sess-1"}).to_string(),
        ))
        .unwrap();

    let resp = send_request(state, req).await;
    assert_eq!(resp.status(), StatusCode::ACCEPTED);

    let body = body_json(resp).await;
    assert_eq!(body["status"], "no_transcript");
}

#[tokio::test]
async fn test_hooks_nonexistent_path() {
    let data_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);

    let req = Request::post("/hooks")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "session_id": "sess-1",
                "transcript_path": "/nonexistent/path/to/file.jsonl"
            })
            .to_string(),
        ))
        .unwrap();

    let resp = send_request(state, req).await;
    assert_eq!(resp.status(), StatusCode::ACCEPTED);

    let body = body_json(resp).await;
    assert_eq!(body["status"], "no_transcript");
}

#[tokio::test]
async fn test_hooks_derives_session_id() {
    let data_dir = TempDir::new().unwrap();
    let transcript_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);

    let path = write_transcript(
        &transcript_dir,
        "my-derived-session.jsonl",
        &[&transcript_line("u-001", "user", "my-derived-session")],
    );

    // Omit session_id — should derive from filename
    let req = Request::post("/hooks")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({"transcript_path": path}).to_string(),
        ))
        .unwrap();

    let resp = send_request(state.clone(), req).await;
    assert_eq!(resp.status(), StatusCode::ACCEPTED);

    let body = body_json(resp).await;
    assert_eq!(body["status"], "ok");

    let s = state.read().await;
    assert!(!s.store.event_store.session_events("my-derived-session").unwrap().is_empty());
}

#[tokio::test]
async fn test_hooks_incremental_reads() {
    let data_dir = TempDir::new().unwrap();
    let transcript_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);

    // Write initial 2 lines
    let path = transcript_dir.path().join("incr-session.jsonl");
    {
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(f, "{}", transcript_line("u-001", "user", "incr-session")).unwrap();
        writeln!(f, "{}", transcript_line("u-002", "assistant", "incr-session")).unwrap();
    }

    let make_req = |p: &str| {
        Request::post("/hooks")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({"session_id": "incr-session", "transcript_path": p}).to_string(),
            ))
            .unwrap()
    };

    let path_str = path.to_string_lossy().to_string();

    // First POST — should get 2 events
    let resp = send_request(state.clone(), make_req(&path_str)).await;
    let body = body_json(resp).await;
    assert_eq!(body["status"], "ok");
    let first_count = body["events"].as_u64().unwrap();
    assert!(first_count >= 2);

    // Append 1 more line
    {
        let mut f = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
        writeln!(f, "{}", transcript_line("u-003", "user", "incr-session")).unwrap();
    }

    // Second POST — should only get 1 new event
    let resp = send_request(state.clone(), make_req(&path_str)).await;
    let body = body_json(resp).await;
    assert_eq!(body["status"], "ok");
    let second_count = body["events"].as_u64().unwrap();
    assert!(second_count >= 1);

    // Total in session should be first + second
    let s = state.read().await;
    let total = s.store.event_store.session_events("incr-session").unwrap().len();
    assert!(total as u64 >= first_count + second_count);
}

#[tokio::test]
async fn test_hooks_resolves_from_watch_dir() {
    let data_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);

    // Write a transcript file into the watch_dir (data_dir/watch/)
    let watch_dir = data_dir.path().join("watch");
    let transcript_path = watch_dir.join("watch-sess.jsonl");
    {
        let mut f = std::fs::File::create(&transcript_path).unwrap();
        writeln!(f, "{}", transcript_line("u-100", "user", "watch-sess")).unwrap();
        writeln!(f, "{}", transcript_line("u-101", "assistant", "watch-sess")).unwrap();
    }

    // POST with session_id but NO transcript_path — should resolve via watch_dir walk
    let req = Request::post("/hooks")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({"session_id": "watch-sess"}).to_string(),
        ))
        .unwrap();

    let resp = send_request(state.clone(), req).await;
    assert_eq!(resp.status(), StatusCode::ACCEPTED);

    let body = body_json(resp).await;
    assert_eq!(body["status"], "ok");
    assert!(body["events"].as_u64().unwrap() >= 2);

    // Verify events were ingested
    let s = state.read().await;
    let events = s.store.event_store.session_events("watch-sess").unwrap();
    assert!(events.len() >= 2);
}
