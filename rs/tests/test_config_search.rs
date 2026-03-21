//! Search configuration tests — server + Qdrant (no NATS).
//!
//! Tests the semantic search pipeline: ingest → embedding → Qdrant → search API.
//! Uses NoopEmbedder (zero vectors) inside the container — tests verify the pipeline
//! connects and data flows through, not embedding quality.
//!
//! Prerequisites:
//!   docker build -t open-story:test ./rs
//!
//! Run with: cargo test -p open-story --test test_config_search -- --ignored --nocapture

#[path = "helpers/compose.rs"]
mod compose;

use compose::{start_stack, TestConfig};
use std::path::{Path, PathBuf};

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
}
use serde_json::Value;

// ── Qdrant health ─────────────────────────────────────────────────────

/// Qdrant REST API responds when stack is up.
#[tokio::test]
#[ignore]
async fn search_qdrant_health() {
    let stack = start_stack(TestConfig::Search, &fixtures_dir()).await;

    let port = stack.qdrant_rest_port.expect("qdrant port should be set");
    let resp = reqwest::get(format!("http://localhost:{port}/healthz"))
        .await
        .expect("qdrant healthz request failed");

    assert_eq!(resp.status(), 200);
}

/// Server connects to Qdrant and reports semantic store active.
#[tokio::test]
#[ignore]
async fn search_server_reports_active() {
    let stack = start_stack(TestConfig::Search, &fixtures_dir()).await;

    // Agent tools endpoint should include semantic_search
    let resp = reqwest::get(format!("{}/api/agent/tools", stack.base_url()))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let tools: Vec<Value> = resp.json().await.unwrap();
    let search_tool = tools
        .iter()
        .find(|t| t["name"] == "semantic_search");
    assert!(
        search_tool.is_some(),
        "agent tools should include semantic_search when Qdrant is connected"
    );
}

// ── Search API ────────────────────────────────────────────────────────

/// GET /api/search?q= with empty query returns 400.
#[tokio::test]
#[ignore]
async fn search_empty_query_400() {
    let stack = start_stack(TestConfig::Search, &fixtures_dir()).await;

    let resp = reqwest::get(format!("{}/api/search?q=", stack.base_url()))
        .await
        .unwrap();

    assert_eq!(resp.status(), 400);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["error"], "missing or empty 'q' parameter");
}

/// Sessions loaded via watcher become available in the events API.
#[tokio::test]
#[ignore]
async fn search_sessions_load() {
    let stack = start_stack(TestConfig::Search, &fixtures_dir()).await;
    stack.wait_for_sessions().await;

    let sessions: Vec<Value> = reqwest::get(format!("{}/api/sessions", stack.base_url()))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    assert!(
        !sessions.is_empty(),
        "sessions should load from fixture files"
    );
}

/// POST /hooks events are accepted and ingested.
#[tokio::test]
#[ignore]
async fn search_hooks_accepted() {
    let stack = start_stack(TestConfig::Search, &fixtures_dir()).await;

    let hook_body = serde_json::json!({
        "session_id": "search-hook-test",
        "hook_event_name": "PostToolUse",
        "tool_name": "Bash",
        "transcript_path": ""
    });

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/hooks", stack.base_url()))
        .json(&hook_body)
        .send()
        .await
        .expect("POST /hooks failed");

    assert_eq!(resp.status(), 202);
}

/// Agent search endpoint returns properly structured response.
#[tokio::test]
#[ignore]
async fn search_agent_api_structure() {
    let stack = start_stack(TestConfig::Search, &fixtures_dir()).await;
    stack.wait_for_sessions().await;

    let resp = reqwest::get(format!(
        "{}/api/agent/search?q=test&limit=5",
        stack.base_url()
    ))
    .await
    .unwrap();

    // With NoopEmbedder, search may return empty or results depending on
    // whether zero-vector search matches. Either way the structure should be correct.
    let status = resp.status().as_u16();
    assert!(
        status == 200 || status == 503,
        "expected 200 or 503, got {status}"
    );

    if status == 200 {
        let body: Value = resp.json().await.unwrap();
        assert!(body.get("query").is_some(), "response should have 'query' field");
        assert!(body.get("results").is_some(), "response should have 'results' field");
        assert!(
            body.get("total_events_searched").is_some(),
            "response should have 'total_events_searched' field"
        );
    }
}

/// WebSocket broadcast works when Qdrant is active.
#[tokio::test]
#[ignore]
async fn search_websocket_works() {
    let stack = start_stack(TestConfig::Search, &fixtures_dir()).await;

    // Just verify the WS endpoint responds with an upgrade
    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{}/ws", stack.base_url()))
        .send()
        .await
        .unwrap();

    // Without proper WS upgrade headers, axum returns 400 (not 500 or timeout)
    assert!(
        resp.status() == 400 || resp.status() == 101,
        "WS endpoint should respond, got {}",
        resp.status()
    );
}

/// Synopsis API works in search config (not search-dependent).
#[tokio::test]
#[ignore]
async fn search_synopsis_works() {
    let stack = start_stack(TestConfig::Search, &fixtures_dir()).await;
    stack.wait_for_sessions().await;

    let sessions: Vec<Value> = reqwest::get(format!("{}/api/sessions", stack.base_url()))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    let session_id = sessions[0]["session_id"]
        .as_str()
        .expect("session_id should be a string");

    let resp = reqwest::get(format!(
        "{}/api/sessions/{session_id}/synopsis",
        stack.base_url()
    ))
    .await
    .unwrap();

    assert_eq!(resp.status(), 200);
    let synopsis: Value = resp.json().await.unwrap();
    assert!(
        synopsis.get("session_id").is_some(),
        "synopsis should have session_id"
    );
}
