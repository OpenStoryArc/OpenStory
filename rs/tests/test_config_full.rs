//! Full configuration tests — server + NATS.
//!
//! Tests that all three components work together end-to-end.
//!
//! Prerequisites:
//!   docker build -t open-story:test ./rs
//!
//! Run with: cargo test -p open-story --test test_config_full -- --ignored --nocapture

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

/// All three services start and respond.
#[tokio::test]
#[ignore]
async fn full_all_services_healthy() {
    let stack = start_stack(TestConfig::Full, &fixtures_dir()).await;

    // Server responds
    assert!(stack.is_healthy().await, "server should be healthy");

    // NATS is implicitly healthy if server started (it depends_on nats)
}

/// Sessions load via the full bus path: watcher → NATS → consumer → ingest.
#[tokio::test]
#[ignore]
async fn full_watcher_to_nats_to_sqlite() {
    let stack = start_stack(TestConfig::Full, &fixtures_dir()).await;
    stack.wait_for_sessions().await;

    let sessions: Vec<Value> = reqwest::get(format!("{}/api/sessions", stack.base_url()))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    assert!(
        !sessions.is_empty(),
        "sessions should load via NATS bus path"
    );

    // Verify events are accessible
    let session_id = sessions[0]["session_id"].as_str().unwrap();
    let events: Vec<Value> = reqwest::get(format!(
        "{}/api/sessions/{session_id}/events",
        stack.base_url()
    ))
    .await
    .unwrap()
    .json()
    .await
    .unwrap();

    assert!(!events.is_empty(), "events should be available for session");
}

// `full_hooks_accepted` retired alongside the /hooks endpoint
// (the watcher is the sole ingestion source).

/// API consistency: sessions, events, and view-records all agree on data.
#[tokio::test]
#[ignore]
async fn full_api_consistency() {
    let stack = start_stack(TestConfig::Full, &fixtures_dir()).await;
    stack.wait_for_sessions().await;

    let sessions: Vec<Value> = reqwest::get(format!("{}/api/sessions", stack.base_url()))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    assert!(!sessions.is_empty());
    let session_id = sessions[0]["session_id"].as_str().unwrap();

    // Events exist
    let events: Vec<Value> = reqwest::get(format!(
        "{}/api/sessions/{session_id}/events",
        stack.base_url()
    ))
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
    assert!(!events.is_empty(), "events should exist");

    // View records exist
    let records: Vec<Value> = reqwest::get(format!(
        "{}/api/sessions/{session_id}/view-records",
        stack.base_url()
    ))
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
    assert!(!records.is_empty(), "view records should exist");

    // Session summary exists
    let summary: Value = reqwest::get(format!(
        "{}/api/sessions/{session_id}/summary",
        stack.base_url()
    ))
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
    assert_eq!(
        summary["session_id"].as_str(),
        Some(session_id),
        "summary session_id should match"
    );
    assert!(
        summary["event_count"].as_u64().unwrap_or(0) > 0,
        "summary should have events"
    );
}

/// Full agent workflow: hook → search → synopsis → tool-journey.
#[tokio::test]
#[ignore]
async fn full_agent_workflow() {
    let stack = start_stack(TestConfig::Full, &fixtures_dir()).await;
    stack.wait_for_sessions().await;

    // Step 1: List sessions (agent discovers what's available)
    let sessions: Vec<Value> = reqwest::get(format!("{}/api/sessions", stack.base_url()))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(!sessions.is_empty(), "agent should see sessions");

    let session_id = sessions[0]["session_id"].as_str().unwrap();

    // Step 2: Get synopsis (agent understands the session)
    let synopsis: Value = reqwest::get(format!(
        "{}/api/sessions/{session_id}/synopsis",
        stack.base_url()
    ))
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
    assert!(synopsis.get("session_id").is_some());

    // Step 3: Get tool journey (agent understands strategy)
    let journey: Vec<Value> = reqwest::get(format!(
        "{}/api/sessions/{session_id}/tool-journey",
        stack.base_url()
    ))
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
    // Journey may be empty for simple fixtures, but should not error
    let _ = journey.len();

    // Step 4: Discover agent tools
    let tools: Vec<Value> = reqwest::get(format!("{}/api/agent/tools", stack.base_url()))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(
        tools.iter().any(|t| t["name"] == "search"),
        "agent should see search tool"
    );
}
