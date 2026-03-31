//! Integration tests using testcontainers to run open-story in Docker.
//!
//! These tests require Docker and a pre-built image:
//!   docker build -t open-story:test ./rs
//!
//! Run with: cargo test -p open-story --test test_container

mod helpers;

use helpers::container::start_open_story;
use helpers::fixtures_dir;
use serde_json::Value;

/// Health check: container starts and responds to GET /api/sessions.
#[tokio::test]
async fn container_responds_to_health_check() {
    let fixture_dir = fixtures_dir();
    let server = start_open_story(&fixture_dir).await;

    let resp = reqwest::get(format!("{}/api/sessions", server.base_url()))
        .await
        .expect("HTTP request failed");

    assert_eq!(resp.status(), 200);
}

/// The server loads JSONL fixtures from the mounted watch dir and returns sessions.
#[tokio::test]
async fn container_loads_fixture_sessions() {
    let fixture_dir = fixtures_dir();
    let server = start_open_story(&fixture_dir).await;

    server.wait_for_sessions().await;

    let resp = reqwest::get(format!("{}/api/sessions", server.base_url()))
        .await
        .expect("HTTP request failed");

    assert_eq!(resp.status(), 200);

    let body: Value = resp.json().await.expect("invalid JSON");
    let sessions = body.as_array().expect("expected array");

    // fixtures/ contains multiple JSONL files — at least one session should load
    assert!(
        !sessions.is_empty(),
        "expected at least one session from fixtures, got none"
    );
}

/// GET /api/sessions/{id}/events returns events for a loaded session.
#[tokio::test]
async fn container_returns_session_events() {
    let fixture_dir = fixtures_dir();
    let server = start_open_story(&fixture_dir).await;

    server.wait_for_sessions().await;

    // Get the first session ID
    let sessions: Vec<Value> = reqwest::get(format!("{}/api/sessions", server.base_url()))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    assert!(!sessions.is_empty(), "no sessions loaded");

    let session_id = sessions[0]["session_id"]
        .as_str()
        .expect("session_id should be a string");

    // Fetch events for that session
    let resp = reqwest::get(format!(
        "{}/api/sessions/{}/events",
        server.base_url(),
        session_id
    ))
    .await
    .unwrap();

    assert_eq!(resp.status(), 200);

    let events: Vec<Value> = resp.json().await.unwrap();
    assert!(
        !events.is_empty(),
        "expected events for session {session_id}"
    );
}

/// GET /api/sessions/{id}/view-records returns typed ViewRecords.
#[tokio::test]
async fn container_returns_view_records() {
    let fixture_dir = fixtures_dir();
    let server = start_open_story(&fixture_dir).await;

    server.wait_for_sessions().await;

    let sessions: Vec<Value> = reqwest::get(format!("{}/api/sessions", server.base_url()))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    assert!(!sessions.is_empty(), "no sessions loaded");

    let session_id = sessions[0]["session_id"].as_str().unwrap();

    let resp = reqwest::get(format!(
        "{}/api/sessions/{}/view-records",
        server.base_url(),
        session_id
    ))
    .await
    .unwrap();

    assert_eq!(resp.status(), 200);

    let records: Vec<Value> = resp.json().await.unwrap();
    assert!(
        !records.is_empty(),
        "expected view records for session {session_id}"
    );

    // ViewRecords should have record_type and payload fields
    let first = &records[0];
    assert!(
        first.get("record_type").is_some(),
        "view record should have record_type"
    );
    assert!(
        first.get("payload").is_some(),
        "view record should have payload"
    );
}

/// GET /api/search returns FTS5 results for indexed events.
#[tokio::test]
async fn container_fts5_search_returns_results() {
    let fixture_dir = fixtures_dir();
    let server = start_open_story(&fixture_dir).await;

    server.wait_for_sessions().await;

    // "Hello" appears in the synthetic.jsonl fixture as a user message
    let resp = reqwest::get(format!("{}/api/search?q=Hello", server.base_url()))
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);

    let results: Vec<Value> = resp.json().await.unwrap();
    assert!(
        !results.is_empty(),
        "FTS5 search for 'Hello' should return results from fixture data"
    );

    // Results should have the expected structure
    let first = &results[0];
    assert!(first.get("event_id").is_some(), "result should have event_id");
    assert!(first.get("session_id").is_some(), "result should have session_id");
    assert!(first.get("record_type").is_some(), "result should have record_type");
    assert!(first.get("snippet").is_some(), "result should have snippet");
    assert!(first.get("rank").is_some(), "result should have rank");
}

/// GET /api/agent/search returns session-grouped FTS5 results.
#[tokio::test]
async fn container_agent_search_returns_grouped_results() {
    let fixture_dir = fixtures_dir();
    let server = start_open_story(&fixture_dir).await;

    server.wait_for_sessions().await;

    let resp = reqwest::get(format!(
        "{}/api/agent/search?q=Hello&limit=5",
        server.base_url()
    ))
    .await
    .unwrap();

    assert_eq!(resp.status(), 200);

    let body: Value = resp.json().await.unwrap();
    assert!(body.get("query").is_some(), "response should have 'query' field");
    assert!(body.get("results").is_some(), "response should have 'results' field");
    assert!(
        body.get("total_events_searched").is_some(),
        "response should have 'total_events_searched' field"
    );

    let results = body["results"].as_array().unwrap();
    if !results.is_empty() {
        let first = &results[0];
        assert!(first.get("session_id").is_some(), "session result should have session_id");
        assert!(first.get("matching_events").is_some(), "session result should have matching_events");
        assert!(first.get("synopsis_url").is_some(), "session result should have synopsis_url");
    }
}

/// GET /api/search with empty query returns 400.
#[tokio::test]
async fn container_search_empty_query_returns_400() {
    let fixture_dir = fixtures_dir();
    let server = start_open_story(&fixture_dir).await;

    let resp = reqwest::get(format!("{}/api/search?q=", server.base_url()))
        .await
        .unwrap();

    assert_eq!(resp.status(), 400);
}

/// POST /hooks accepts hook events and returns 202.
#[tokio::test]
async fn container_accepts_hook_post() {
    let fixture_dir = fixtures_dir();
    let server = start_open_story(&fixture_dir).await;

    let hook_body = serde_json::json!({
        "session_id": "test-hook-session",
        "type": "tool_use",
        "tool": {
            "name": "Read",
            "input": {"file_path": "/tmp/test.txt"}
        },
        "session": {
            "session_id": "test-hook-session",
            "cwd": "/workspace"
        },
        "transcript_path": ""
    });

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/hooks", server.base_url()))
        .json(&hook_body)
        .send()
        .await
        .expect("POST /hooks failed");

    // Hooks endpoint returns 202 Accepted
    assert_eq!(resp.status(), 202);
}
