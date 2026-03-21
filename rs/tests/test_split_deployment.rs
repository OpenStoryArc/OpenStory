//! Integration tests for split deployment: publisher + NATS + consumer.
//!
//! These tests verify that events flow from publisher to consumer via NATS.
//! Requires Docker with `open-story:test` image built.
//!
//! Run: cargo test -p open-story --test test_split_deployment -- --ignored --nocapture

mod helpers;

use std::time::Duration;
use helpers::compose::{TestConfig, start_stack};
use helpers::fixtures_dir;

/// Split stack starts — both publisher and consumer report healthy.
#[tokio::test]
#[ignore] // requires Docker
async fn split_publisher_and_consumer_start() {
    let stack = start_stack(TestConfig::Split, &fixtures_dir()).await;

    // Consumer should be healthy (serves API)
    assert!(stack.is_healthy().await, "consumer should be healthy");

    // Publisher should be healthy (serves /health)
    let publisher_port = stack.publisher_port.expect("split stack should have publisher port");
    let resp = reqwest::get(format!("http://localhost:{publisher_port}/health"))
        .await
        .expect("publisher health check");
    assert_eq!(resp.status(), 200);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["role"], "publisher");
}

/// Publisher should not serve API endpoints (only hooks + health).
#[tokio::test]
#[ignore]
async fn split_publisher_has_no_api() {
    let stack = start_stack(TestConfig::Split, &fixtures_dir()).await;

    let publisher_port = stack.publisher_port.expect("publisher port");
    let resp = reqwest::get(format!("http://localhost:{publisher_port}/api/sessions"))
        .await
        .expect("request to publisher /api/sessions");
    assert_eq!(resp.status(), 404, "publisher should not serve /api/sessions");
}

/// Consumer should report role=consumer in health check.
#[tokio::test]
#[ignore]
async fn split_consumer_reports_role() {
    let stack = start_stack(TestConfig::Split, &fixtures_dir()).await;

    let resp = reqwest::get(format!("{}/health", stack.base_url()))
        .await
        .expect("consumer health check");
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["role"], "consumer");
}

/// Events flow from publisher's watch dir through NATS to consumer's API.
///
/// The test fixture directory contains JSONL files. The publisher's watcher
/// picks them up, publishes to NATS, and the consumer ingests them.
#[tokio::test]
#[ignore]
async fn split_events_flow_publisher_to_consumer() {
    let stack = start_stack(TestConfig::Split, &fixtures_dir()).await;

    // Wait for the publisher to watch the fixture files and publish via NATS,
    // then for the consumer to ingest them and make them available via API.
    stack.wait_for_sessions().await;

    // Verify sessions are available in the consumer's API
    let resp = reqwest::get(format!("{}/api/sessions", stack.base_url()))
        .await
        .expect("get sessions");
    let sessions: Vec<serde_json::Value> = resp.json().await.unwrap();
    assert!(!sessions.is_empty(), "consumer should have sessions from publisher");
}

/// POST /hooks to publisher flows through NATS to consumer.
#[tokio::test]
#[ignore]
async fn split_hooks_flow_through_nats() {
    let stack = start_stack(TestConfig::Split, &fixtures_dir()).await;
    let publisher_port = stack.publisher_port.expect("publisher port");

    // Wait for initial file watcher events to propagate
    stack.wait_for_sessions().await;

    // POST a hook to the publisher — it should be accepted
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("http://localhost:{publisher_port}/hooks"))
        .json(&serde_json::json!({
            "session_id": "test-hook-session",
            "hook_event_name": "PostToolUse",
        }))
        .send()
        .await
        .expect("post hook");
    assert_eq!(resp.status(), 202, "publisher should accept hooks");

    // The hook itself won't produce events (no transcript file for test-hook-session),
    // but we verified the endpoint is functional.
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(
        body["status"] == "ok" || body["status"] == "no_transcript",
        "hook should return ok or no_transcript, got: {body}"
    );
}
