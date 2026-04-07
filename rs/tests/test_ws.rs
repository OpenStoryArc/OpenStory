//! WebSocket transport integration tests.
//!
//! Tests actual WS handshake and live broadcast — complements the unit tests
//! in test_broadcast.rs which test build_initial_state in isolation.

mod helpers;

use std::collections::HashMap;
use std::net::SocketAddr;

use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use tempfile::TempDir;
use tokio::net::TcpListener;
use tokio_tungstenite::connect_async;

use helpers::{make_event, make_user_prompt, test_state};
use open_story::server::{ingest_events, BroadcastMessage};

/// Start a test server on a random port and return its address.
async fn start_test_server(state: open_story::server::SharedState) -> SocketAddr {
    let config = open_story::server::Config::default();
    let router = open_story::server::build_router(state, None, &config);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    addr
}

// ── Handshake tests ─────────────────────────────────────────────────

#[tokio::test]
async fn ws_handshake_receives_initial_state() {
    let data_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);
    let addr = start_test_server(state).await;

    let url = format!("ws://{addr}/ws");
    let (mut ws, _) = connect_async(&url).await.expect("WS connect failed");

    let msg = ws.next().await.expect("no message").expect("ws error");
    let text = msg.into_text().expect("not text");
    let json: Value = serde_json::from_str(&text).unwrap();

    assert_eq!(json["kind"], "initial_state");
    assert!(json["records"].is_array());
    assert!(json["filter_counts"].is_object());
    assert!(json["patterns"].is_array());
    assert!(json["session_labels"].is_object());

    ws.close(None).await.ok();
}

#[tokio::test]
async fn ws_initial_state_contains_ingested_records() {
    let data_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);

    // Ingest events before connecting
    {
        let mut s = state.write().await;
        let events = vec![
            make_user_prompt("ws-sess", "ws-evt-1"),
            make_event("io.arc.event", "ws-sess"),
        ];
        ingest_events(&mut s, "ws-sess", &events, None).await;
    }

    let addr = start_test_server(state).await;
    let url = format!("ws://{addr}/ws");
    let (mut ws, _) = connect_async(&url).await.expect("WS connect failed");

    let msg = ws.next().await.expect("no message").expect("ws error");
    let text = msg.into_text().expect("not text");
    let json: Value = serde_json::from_str(&text).unwrap();

    assert_eq!(json["kind"], "initial_state");
    let records = json["records"].as_array().unwrap();
    assert!(
        !records.is_empty(),
        "initial_state should contain ingested records"
    );

    ws.close(None).await.ok();
}

// ── Live broadcast tests ────────────────────────────────────────────

#[tokio::test]
async fn ws_receives_broadcast_after_hook_ingest() {
    let data_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);
    let addr = start_test_server(state.clone()).await;

    let url = format!("ws://{addr}/ws");
    let (mut ws, _) = connect_async(&url).await.expect("WS connect failed");

    // Consume the initial_state message
    let _ = ws.next().await.expect("no initial_state").expect("ws error");

    // Broadcast an event through the broadcast channel
    {
        let s = state.read().await;
        let msg = BroadcastMessage::Enriched {
            session_id: "live-sess".to_string(),
            records: Vec::new(),
            ephemeral: Vec::new(),
            filter_deltas: HashMap::new(),
            patterns: Vec::new(),
            project_id: None,
            project_name: None,
            session_label: Some("test-broadcast".to_string()),
            session_branch: None,
            agent_labels: HashMap::new(),
            total_input_tokens: None,
            total_output_tokens: None,
        };
        s.broadcast_tx.send(msg).unwrap();
    }

    // Should receive the broadcast
    let msg = tokio::time::timeout(std::time::Duration::from_secs(5), ws.next())
        .await
        .expect("timeout waiting for broadcast")
        .expect("no message")
        .expect("ws error");

    let text = msg.into_text().expect("not text");
    let json: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(json["kind"], "enriched");
    assert_eq!(json["session_id"], "live-sess");

    ws.close(None).await.ok();
}

#[tokio::test]
async fn ws_multiple_clients_each_receive_broadcast() {
    let data_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);
    let addr = start_test_server(state.clone()).await;

    let url = format!("ws://{addr}/ws");

    // Connect two clients
    let (mut ws1, _) = connect_async(&url).await.expect("WS1 connect failed");
    let (mut ws2, _) = connect_async(&url).await.expect("WS2 connect failed");

    // Consume initial_state from both
    let _ = ws1.next().await;
    let _ = ws2.next().await;

    // Broadcast
    {
        let s = state.read().await;
        let msg = BroadcastMessage::Enriched {
            session_id: "multi-sess".to_string(),
            records: Vec::new(),
            ephemeral: Vec::new(),
            filter_deltas: HashMap::new(),
            patterns: Vec::new(),
            project_id: None,
            project_name: None,
            session_label: Some("multi-test".to_string()),
            session_branch: None,
            agent_labels: HashMap::new(),
            total_input_tokens: None,
            total_output_tokens: None,
        };
        s.broadcast_tx.send(msg).unwrap();
    }

    let timeout = std::time::Duration::from_secs(5);

    let msg1 = tokio::time::timeout(timeout, ws1.next())
        .await
        .expect("ws1 timeout")
        .expect("ws1 no msg")
        .expect("ws1 error");
    let json1: Value = serde_json::from_str(&msg1.into_text().unwrap()).unwrap();
    assert_eq!(json1["session_id"], "multi-sess");

    let msg2 = tokio::time::timeout(timeout, ws2.next())
        .await
        .expect("ws2 timeout")
        .expect("ws2 no msg")
        .expect("ws2 error");
    let json2: Value = serde_json::from_str(&msg2.into_text().unwrap()).unwrap();
    assert_eq!(json2["session_id"], "multi-sess");

    ws1.close(None).await.ok();
    ws2.close(None).await.ok();
}
