//! Integration tests: pi-mono sessions through the NATS bus path.
//!
//! Mirror of `test_pi_mono_container.rs` but exercises the compose stack
//! (NATS JetStream + open-story), proving the same invariants hold when
//! events flow through the actor-consumer pipeline:
//!   watcher → NATS → PersistConsumer → SQLite → /api
//!
//! The container variant verifies the demo path (`ingest_events` inline
//! dual-writes when `!bus.is_active()`). This file verifies the path we
//! actually care about — decomposed actors, NATS delivery, clean
//! ownership of persistence.
//!
//! Prerequisites:
//!   docker build -t open-story:test ./rs
//!
//! Run with: cargo test -p open-story --test test_pi_mono_compose

mod helpers;

use helpers::fixtures_dir;
use serde_json::Value;
use std::path::PathBuf;
use testcontainers::compose::DockerCompose;

fn compose_file() -> PathBuf {
    PathBuf::from(format!(
        "{}/tests/docker-compose.test.yml",
        env!("CARGO_MANIFEST_DIR")
    ))
}

/// Build a pi-mono-only fixture dir and return its Docker-compatible path.
fn pi_mono_mount_path() -> (tempfile::TempDir, String) {
    let fixtures = fixtures_dir();
    let tmp = tempfile::TempDir::new().expect("create temp dir");
    let src = fixtures.join("pi_mono_session.jsonl");
    let dst = tmp.path().join("pi_mono_session.jsonl");
    std::fs::copy(&src, &dst).expect("copy pi-mono fixture");

    // Fresh mtimes so the watcher's 24h backfill window picks them up.
    let now = filetime::FileTime::now();
    let _ = filetime::set_file_mtime(&dst, now);

    let canonical = tmp.path().canonicalize().expect("canonicalize temp dir");
    let s = canonical.to_string_lossy().to_string();
    let s = s.strip_prefix(r"\\?\").unwrap_or(&s);
    (tmp, s.replace('\\', "/"))
}

async fn start_pi_mono_stack() -> (DockerCompose, tempfile::TempDir, u16) {
    let (tmp, fixture_path) = pi_mono_mount_path();
    let mut compose = DockerCompose::with_local_client(&[compose_file()])
        .with_env("FIXTURE_DIR", &fixture_path)
        .with_wait(false);
    compose.up().await.expect("docker compose up failed");

    let server = compose.service("server").expect("server service not found");
    let port = server
        .get_host_port_ipv4(3002)
        .await
        .expect("failed to get server port");

    let url = format!("http://localhost:{port}/api/sessions");
    for _ in 0..30 {
        if reqwest::get(&url).await.is_ok() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    (compose, tmp, port)
}

async fn wait_for_sessions(port: u16) {
    let url = format!("http://localhost:{port}/api/sessions");
    for _ in 0..60 {
        if let Ok(resp) = reqwest::get(&url).await {
            if let Ok(body) = resp.json::<Value>().await {
                let sessions = body
                    .get("sessions")
                    .and_then(|v| v.as_array())
                    .or_else(|| body.as_array());
                if let Some(arr) = sessions {
                    if !arr.is_empty() {
                        return;
                    }
                }
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    panic!("timed out waiting for sessions to load (30s)");
}

/// Poll /events until the count stops growing. The watcher emits one batch
/// per file-change notification and PersistConsumer drains them asynchronously,
/// so short-lived partial views of the session are normal. "Done" means three
/// consecutive polls see the same count.
async fn wait_for_session_events(port: u16, session_id: &str) -> Vec<Value> {
    let url = format!("http://localhost:{port}/api/sessions/{session_id}/events");
    let mut last = 0usize;
    let mut stable = 0u32;
    for _ in 0..60 {
        if let Ok(resp) = reqwest::get(&url).await {
            if let Ok(events) = resp.json::<Vec<Value>>().await {
                if events.len() == last && last > 0 {
                    stable += 1;
                    if stable >= 3 {
                        return events;
                    }
                } else {
                    stable = 0;
                    last = events.len();
                }
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    panic!("timed out waiting for events on session {session_id} to stabilize (30s)");
}

async fn get_sessions(port: u16) -> Vec<Value> {
    let body: Value = reqwest::get(format!("http://localhost:{port}/api/sessions"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    body.get("sessions")
        .and_then(|v| v.as_array())
        .or_else(|| body.as_array())
        .cloned()
        .unwrap_or_default()
}

fn find_pi_session(sessions: &[Value]) -> &Value {
    sessions
        .iter()
        .find(|s| {
            s["session_id"]
                .as_str()
                .map_or(false, |id| id.contains("pi_mono"))
        })
        .expect("pi-mono session not found in /api/sessions")
}

// ── Tests ──

/// Pi-mono events reach SQLite via the NATS → PersistConsumer path.
/// Exact event-count invariants are covered by the local-mode tests in
/// `test_pi_mono_container.rs` — here we just prove the pipeline delivers
/// a substantial fraction of the fixture. Compose runs share a NATS
/// volume across tests so a strict count assertion would race on replay.
#[tokio::test]
async fn pi_mono_events_persisted_via_nats() {
    let (_compose, _tmp, port) = start_pi_mono_stack().await;
    wait_for_sessions(port).await;

    let session_id = find_pi_session(&get_sessions(port).await)["session_id"]
        .as_str()
        .unwrap()
        .to_string();
    let events = wait_for_session_events(port, &session_id).await;

    assert!(
        events.len() >= 10,
        "expected NATS pipeline to deliver most of the 13-14 fixture events, got {}",
        events.len()
    );
}

/// Decomposition (bundled content blocks → per-block CloudEvents) survives
/// the full NATS round-trip.
#[tokio::test]
async fn pi_mono_decomposition_survives_nats() {
    let (_compose, _tmp, port) = start_pi_mono_stack().await;
    wait_for_sessions(port).await;

    let session_id = find_pi_session(&get_sessions(port).await)["session_id"]
        .as_str()
        .unwrap()
        .to_string();
    let events = wait_for_session_events(port, &session_id).await;

    let subtypes: Vec<&str> = events
        .iter()
        .filter_map(|e| e.get("subtype").and_then(|v| v.as_str()))
        .collect();

    assert!(subtypes.contains(&"message.assistant.thinking"), "thinking block decomposed");
    assert!(subtypes.contains(&"message.assistant.tool_use"), "tool_use block decomposed");
    let text_count = subtypes.iter().filter(|s| **s == "message.assistant.text").count();
    assert!(text_count >= 2, "expected >=2 decomposed text events, got {text_count}");
    assert!(subtypes.contains(&"system.turn.complete"), "synthetic turn.complete present");
}

/// Raw data is unmutated through translate → NATS → PersistConsumer → SQLite.
/// The sovereignty invariant: the agent's native field names survive.
#[tokio::test]
async fn pi_mono_raw_unmutated_through_nats() {
    let (_compose, _tmp, port) = start_pi_mono_stack().await;
    wait_for_sessions(port).await;

    let session_id = find_pi_session(&get_sessions(port).await)["session_id"]
        .as_str()
        .unwrap()
        .to_string();
    let events = wait_for_session_events(port, &session_id).await;

    // Native pi-mono toolCall must NOT be normalized to tool_use in `raw`.
    let tool_use = events
        .iter()
        .find(|e| e.get("subtype").and_then(|v| v.as_str()) == Some("message.assistant.tool_use"))
        .expect("no tool_use event after NATS round-trip");
    let raw_content = &tool_use["data"]["raw"]["message"]["content"];
    let has_toolcall = raw_content
        .as_array()
        .map_or(false, |arr| arr.iter().any(|b| b["type"] == "toolCall"));
    assert!(has_toolcall, "raw should preserve pi-mono's 'toolCall' type through NATS");

    // toolResult role and toolCallId must also survive.
    let tool_result = events
        .iter()
        .find(|e| e.get("subtype").and_then(|v| v.as_str()) == Some("message.user.tool_result"))
        .expect("no tool_result event after NATS round-trip");
    let raw = &tool_result["data"]["raw"];
    assert_eq!(raw["message"]["role"], "toolResult");
    assert_eq!(raw["message"]["toolCallId"], "tc-001");
}

/// View records are derivable from events fetched via the NATS pipeline.
#[tokio::test]
async fn pi_mono_view_records_via_nats() {
    let (_compose, _tmp, port) = start_pi_mono_stack().await;
    wait_for_sessions(port).await;

    let session_id = find_pi_session(&get_sessions(port).await)["session_id"]
        .as_str()
        .unwrap()
        .to_string();
    // Ensure events arrived before requesting derived view records.
    let _ = wait_for_session_events(port, &session_id).await;

    let records: Vec<Value> = reqwest::get(format!(
        "http://localhost:{port}/api/sessions/{session_id}/view-records"
    ))
    .await
    .unwrap()
    .json()
    .await
    .unwrap();

    assert!(!records.is_empty(), "view records missing after NATS ingest");
    for record in &records {
        assert!(record.get("record_type").is_some(), "record_type missing");
        assert!(record.get("payload").is_some(), "payload missing");
    }
}

/// FTS is populated by PersistConsumer — search returns pi-mono content.
#[tokio::test]
async fn pi_mono_fts_indexed_via_nats() {
    let (_compose, _tmp, port) = start_pi_mono_stack().await;
    wait_for_sessions(port).await;

    let session_id = find_pi_session(&get_sessions(port).await)["session_id"]
        .as_str()
        .unwrap()
        .to_string();
    let _ = wait_for_session_events(port, &session_id).await;

    // The fixture's user prompt is "Read the config file and explain it".
    let results: Vec<Value> = reqwest::get(format!(
        "http://localhost:{port}/api/search?q=config"
    ))
    .await
    .unwrap()
    .json()
    .await
    .unwrap();

    assert!(
        !results.is_empty(),
        "FTS should surface pi-mono events after NATS-driven indexing"
    );
}
