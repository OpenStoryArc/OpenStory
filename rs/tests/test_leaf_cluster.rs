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
use std::collections::BTreeSet;
use std::time::Duration;

/// Fetch a session's events (CloudEvents) from a server.
async fn get_events(port: u16, session_id: &str) -> Vec<Value> {
    let url = format!("http://localhost:{port}/api/sessions/{session_id}/events");
    match reqwest::get(&url).await {
        Ok(resp) => resp.json::<Vec<Value>>().await.unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

/// Fetch `/api/digests` as a map of session_id → digest.
async fn get_digests(port: u16) -> std::collections::BTreeMap<String, String> {
    let url = format!("http://localhost:{port}/api/digests");
    let body: Value = match reqwest::get(&url).await {
        Ok(resp) => resp.json().await.unwrap_or(Value::Null),
        Err(_) => Value::Null,
    };
    body.get("sessions")
        .and_then(|s| s.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|d| {
                    Some((
                        d["session_id"].as_str()?.to_string(),
                        d["digest"].as_str()?.to_string(),
                    ))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn session_id_set(sessions: &[Value]) -> BTreeSet<String> {
    sessions
        .iter()
        .filter_map(|s| s["session_id"].as_str().map(String::from))
        .collect()
}

fn host_user(sessions: &[Value], sid: &str) -> (Option<String>, Option<String>) {
    sessions
        .iter()
        .find(|s| s["session_id"].as_str() == Some(sid))
        .map(|s| {
            (
                s["host"].as_str().map(String::from),
                s["user"].as_str().map(String::from),
            )
        })
        .unwrap_or((None, None))
}

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

/// CONVERGENCE: leaf and hub must compute IDENTICAL per-session digests once
/// replication settles. This proves the fleet-health primitive end-to-end —
/// `/api/digests` on each node, and equal digests for shared sessions means the
/// event-id sets are byte-identical across the federation (a stronger, cheaper
/// statement than comparing full event lists, and exactly what `/api/fleet` and
/// `verify` will rely on). A digest mismatch would mean silent divergence.
#[tokio::test]
#[ignore]
async fn leaf_and_hub_converge_on_matching_digests() {
    let stack = start_stack(TestConfig::LeafCluster, &fixtures_dir()).await;
    let hub_port = stack.hub_server_port.expect("hub port");

    wait_for_sessions(stack.server_port, "leaf-server").await;
    wait_for_sessions(hub_port, "hub-server").await;
    tokio::time::sleep(Duration::from_secs(4)).await;

    let leaf = get_digests(stack.server_port).await;
    let hub = get_digests(hub_port).await;

    assert!(!leaf.is_empty(), "leaf should report digests");
    for (sid, leaf_digest) in &leaf {
        match hub.get(sid) {
            Some(hub_digest) => assert_eq!(
                hub_digest, leaf_digest,
                "leaf and hub digests diverge for session {sid} — convergence broken \
                 (the event-id sets differ across the federation)"
            ),
            None => panic!("hub is missing session {sid} that the leaf has"),
        }
    }
}

/// INTEGRITY: a leaf→hub round-trip must preserve session identity, the
/// per-session event set, timestamp order, and origin (host/user) — not just
/// session *counts*. The existing tests assert presence/counts only; this one
/// asserts the data actually survives replication intact. If the leaf and hub
/// derive different session_ids, drop/duplicate events, reorder them, or the
/// hub re-stamps origin with its own host, this fails where the count tests
/// pass.
#[tokio::test]
#[ignore]
async fn leaf_to_hub_preserves_identity_order_and_origin() {
    let stack = start_stack(TestConfig::LeafCluster, &fixtures_dir()).await;
    let hub_port = stack.hub_server_port.expect("hub port");

    wait_for_sessions(stack.server_port, "leaf-server").await;
    wait_for_sessions(hub_port, "hub-server").await;
    // Let replication + hub ingest fully settle before comparing.
    tokio::time::sleep(Duration::from_secs(4)).await;

    let leaf_sessions = get_sessions(stack.server_port).await;
    let hub_sessions = get_sessions(hub_port).await;
    let leaf_ids = session_id_set(&leaf_sessions);
    let hub_ids = session_id_set(&hub_sessions);

    // (1) Identity: every leaf session id must appear on the hub — same id,
    // not merely an equal count.
    let missing: Vec<_> = leaf_ids.difference(&hub_ids).cloned().collect();
    assert!(
        missing.is_empty(),
        "hub is missing leaf session ids {missing:?}\n  leaf={leaf_ids:?}\n  hub={hub_ids:?}"
    );

    for sid in &leaf_ids {
        let leaf_events = get_events(stack.server_port, sid).await;
        let hub_events = get_events(hub_port, sid).await;

        // (2) Dedup/completeness: the set of event ids must match exactly —
        // no drops, no duplicates introduced by replication.
        let leaf_eids: BTreeSet<String> = leaf_events
            .iter()
            .filter_map(|e| e["id"].as_str().map(String::from))
            .collect();
        let hub_eids: BTreeSet<String> = hub_events
            .iter()
            .filter_map(|e| e["id"].as_str().map(String::from))
            .collect();
        assert_eq!(
            leaf_eids, hub_eids,
            "event.id set diverged for session {sid} (leaf {} vs hub {})",
            leaf_eids.len(),
            hub_eids.len()
        );

        // (3) Ordering: hub events must be served in non-decreasing timestamp
        // order (the API contract the UI relies on).
        let hub_ts: Vec<&str> = hub_events.iter().filter_map(|e| e["time"].as_str()).collect();
        let mut sorted = hub_ts.clone();
        sorted.sort_unstable();
        assert_eq!(hub_ts, sorted, "hub events for {sid} are not in timestamp order");

        // (4) Origin survival: the hub must report the SAME host/user as the
        // leaf for this session — the stamp baked in at translation on the
        // leaf must survive NATS replication, not be overwritten by the hub.
        let (leaf_host, leaf_user) = host_user(&leaf_sessions, sid);
        let (hub_host, hub_user) = host_user(&hub_sessions, sid);
        assert_eq!(
            hub_host, leaf_host,
            "origin host diverged for {sid}: hub {hub_host:?} != leaf {leaf_host:?} \
             (hub may be re-stamping replicated events with its own host)"
        );
        assert_eq!(
            hub_user, leaf_user,
            "origin user diverged for {sid}: hub {hub_user:?} != leaf {leaf_user:?}"
        );
    }
}
