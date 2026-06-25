//! Admin v0.2 — sink-architecture tests.
//!
//! These pin the contract that the handler serves from the watch channel
//! (not from a fresh JetStream query / session tally on every request).
//! Updating the channel must immediately reflect in the next read.

mod helpers;

use axum::body::Body;
use axum::http::Request;
use helpers::{body_json, send_request, test_state};
use open_story::server::admin::{EnvInputs, compute_topology};
use open_story::server::config::Role;
use tempfile::TempDir;

/// SICP stream-with-memory: the handler reads `borrow()` of the current
/// frame. The broadcaster will own writes via `send()`; here we prove
/// the wire by writing directly to the channel and watching the next
/// read reflect it.
#[tokio::test]
async fn topology_handler_serves_from_watch_channel() {
    let data_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);

    // Initial GET — reads the seed topology that `test_state` planted.
    let req = Request::get("/api/admin/topology")
        .body(Body::empty())
        .unwrap();
    let resp = send_request(state.clone(), req).await;
    assert_eq!(resp.status(), 200);
    let body = body_json(resp).await;
    assert_eq!(body["self"]["host"], "test-host");
    assert_eq!(body["shape"], "solo");

    // Push a new frame into the stream — the broadcaster's job, simulated
    // here so we prove the wire is connected.
    let next = compute_topology(
        "test-host",
        Role::Full,
        &EnvInputs::default(),
        &[("seen-peer".to_string(), 7u64)],
    );
    {
        let s = state.read().await;
        // `send_replace` is the right idiom for state-broadcast: always
        // update the current value regardless of subscriber count. The
        // handler's `borrow()` reads from the Sender's own slot, which
        // exists for the Sender's lifetime — no receiver required.
        let _previous = s.admin_topology_tx.send_replace(next);
    }

    // The next GET reflects the pushed frame — no JetStream hit, no env
    // re-read, no tally. The cache IS the stream.
    let req = Request::get("/api/admin/topology")
        .body(Body::empty())
        .unwrap();
    let resp = send_request(state.clone(), req).await;
    assert_eq!(resp.status(), 200);
    let body = body_json(resp).await;
    let nodes = body["nodes"].as_array().expect("nodes array");
    let hosts: Vec<&str> = nodes.iter().map(|n| n["host"].as_str().unwrap()).collect();
    assert!(
        hosts.contains(&"seen-peer"),
        "the new frame's peer node must appear (got {hosts:?})"
    );
}
