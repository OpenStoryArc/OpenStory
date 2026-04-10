//! Integration tests for NATS hub-leaf cluster deployment.
//!
//! Verifies that events published on a leaf node forward to the hub,
//! and both Open Story instances see the appropriate sessions.
//!
//! Architecture under test:
//!   leaf-server (watches fixtures) → nats-leaf → nats-hub ← hub-server (common dashboard)
//!
//! The leaf-server watches fixture JSONL files and publishes events to the leaf NATS.
//! The leaf NATS forwards events to the hub NATS via leaf node connection.
//! The hub-server subscribes to the hub NATS and ingests all events.
//!
//! Prerequisites:
//!   docker build -t open-story:test ./rs
//!
//! Run with: cargo test -p open-story --test test_leaf_cluster -- --include-ignored

mod helpers;

use helpers::compose::{start_stack, TestConfig};
use helpers::fixtures_dir;
use serde_json::Value;
use std::time::Duration;

/// Wait for at least one session to appear on a given port.
async fn wait_for_sessions(port: u16, label: &str) {
    let url = format!("http://localhost:{port}/api/sessions");
    for _ in 0..120 {
        if let Ok(resp) = reqwest::get(&url).await {
            if let Ok(body) = resp.json::<Value>().await {
                let sessions = body
                    .get("sessions")
                    .and_then(|s| s.as_array())
                    .or_else(|| body.as_array());
                if let Some(arr) = sessions {
                    if !arr.is_empty() {
                        return;
                    }
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    panic!("timed out waiting for sessions on {label} (port {port}, 60s)");
}

/// Get sessions from a server, handling both `[...]` and `{ sessions: [...] }` response shapes.
async fn get_sessions(port: u16) -> Vec<Value> {
    let url = format!("http://localhost:{port}/api/sessions");
    let resp = reqwest::get(&url).await.expect("HTTP request failed");
    let body: Value = resp.json().await.expect("JSON parse failed");

    body.get("sessions")
        .and_then(|s| s.as_array().cloned())
        .or_else(|| body.as_array().cloned())
        .unwrap_or_default()
}

/// The leaf cluster stack starts and all four services are discovered.
#[tokio::test]
#[ignore]
async fn leaf_cluster_starts() {
    let stack = start_stack(TestConfig::LeafCluster, &fixtures_dir()).await;

    assert!(stack.server_port > 0, "leaf-server port should be assigned");
    assert!(
        stack.hub_server_port.is_some(),
        "hub-server port should be assigned"
    );

    // Both servers should respond to health checks
    let leaf_healthy = reqwest::get(format!(
        "http://localhost:{}/api/sessions",
        stack.server_port
    ))
    .await
    .map(|r| r.status() == 200)
    .unwrap_or(false);

    let hub_healthy = reqwest::get(format!(
        "http://localhost:{}/api/sessions",
        stack.hub_server_port.unwrap()
    ))
    .await
    .map(|r| r.status() == 200)
    .unwrap_or(false);

    assert!(leaf_healthy, "leaf-server should be healthy");
    assert!(hub_healthy, "hub-server should be healthy");
}

/// Sessions from the leaf watcher appear on the leaf server.
#[tokio::test]
#[ignore]
async fn leaf_server_ingests_local_sessions() {
    let stack = start_stack(TestConfig::LeafCluster, &fixtures_dir()).await;

    wait_for_sessions(stack.server_port, "leaf-server").await;

    let sessions = get_sessions(stack.server_port).await;
    assert!(
        !sessions.is_empty(),
        "leaf-server should have sessions from watched fixtures"
    );
}

/// Sessions published on the leaf forward to the hub via NATS leaf node connection.
/// This is the core test: events flow leaf → hub across the cluster.
#[tokio::test]
#[ignore]
async fn hub_receives_sessions_from_leaf() {
    let stack = start_stack(TestConfig::LeafCluster, &fixtures_dir()).await;
    let hub_port = stack.hub_server_port.expect("hub port");

    // Wait for the leaf to ingest first (it watches the fixtures)
    wait_for_sessions(stack.server_port, "leaf-server").await;

    // Then wait for the hub to receive the forwarded events
    wait_for_sessions(hub_port, "hub-server").await;

    let hub_sessions = get_sessions(hub_port).await;
    let leaf_sessions = get_sessions(stack.server_port).await;

    assert!(
        !hub_sessions.is_empty(),
        "hub-server should have sessions forwarded from leaf"
    );

    // Hub should see at least as many sessions as the leaf
    // (session IDs may differ slightly due to path derivation,
    // but the count should match — both see the same fixture data)
    assert!(
        hub_sessions.len() >= leaf_sessions.len(),
        "hub ({}) should have at least as many sessions as leaf ({})",
        hub_sessions.len(),
        leaf_sessions.len()
    );
}

/// View records are available on the hub for sessions that originated on the leaf.
#[tokio::test]
#[ignore]
async fn hub_has_view_records_from_leaf() {
    let stack = start_stack(TestConfig::LeafCluster, &fixtures_dir()).await;
    let hub_port = stack.hub_server_port.expect("hub port");

    // Wait for sessions to propagate
    wait_for_sessions(stack.server_port, "leaf-server").await;
    wait_for_sessions(hub_port, "hub-server").await;

    let sessions = get_sessions(hub_port).await;
    let session_id = sessions[0]["session_id"]
        .as_str()
        .expect("session_id should be a string");

    let records: Vec<Value> = reqwest::get(format!(
        "http://localhost:{hub_port}/api/sessions/{session_id}/view-records"
    ))
    .await
    .unwrap()
    .json()
    .await
    .unwrap();

    assert!(
        !records.is_empty(),
        "hub should have view records for leaf session {session_id}"
    );

    let first = &records[0];
    assert!(first.get("record_type").is_some());
    assert!(first.get("payload").is_some());
}
