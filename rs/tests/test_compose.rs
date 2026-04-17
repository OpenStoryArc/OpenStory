//! Integration tests using testcontainers DockerCompose to run
//! open-story + NATS together.
//!
//! These tests exercise the full bus path:
//!   watcher → NATS JetStream → consumer → ingest_events()
//!
//! Prerequisites:
//!   docker build -t open-story:test ./rs
//!
//! Run with: cargo test -p open-story --test test_compose

mod helpers;

use helpers::fixtures_dir;
use serde_json::Value;
use std::path::PathBuf;
use testcontainers::compose::DockerCompose;

/// Path to the docker-compose.test.yml file.
fn compose_file() -> PathBuf {
    PathBuf::from(format!(
        "{}/tests/docker-compose.test.yml",
        env!("CARGO_MANIFEST_DIR")
    ))
}

/// Canonical, Docker-compatible path to the fixtures directory.
fn fixture_mount_path() -> String {
    let fixture_dir = fixtures_dir();

    // Touch fixture files so they have fresh mtimes (watcher skips old files)
    let now = filetime::FileTime::now();
    for entry in std::fs::read_dir(&fixture_dir).expect("read fixture dir") {
        let path = entry.expect("read entry").path();
        let _ = filetime::set_file_mtime(&path, now);
    }

    let canonical = fixture_dir.canonicalize().expect("canonicalize fixture dir");
    let s = canonical.to_string_lossy().to_string();

    // Strip \\?\ UNC prefix and convert backslashes for Docker on Windows
    let s = s.strip_prefix(r"\\?\").unwrap_or(&s);
    s.replace('\\', "/")
}

/// Start the compose stack (NATS + open-story) and return the server's host port.
async fn start_compose_stack() -> (DockerCompose, u16) {
    let fixture_path = fixture_mount_path();

    let mut compose = DockerCompose::with_local_client(&[compose_file()])
        .with_env("FIXTURE_DIR", &fixture_path)
        .with_wait(false); // we'll poll ourselves

    compose.up().await.expect("docker compose up failed");

    let server = compose.service("server").expect("server service not found");
    let port = server
        .get_host_port_ipv4(3002)
        .await
        .expect("failed to get server port");

    // Wait for HTTP readiness
    let url = format!("http://localhost:{port}/api/sessions");
    for _ in 0..30 {
        if reqwest::get(&url).await.is_ok() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }

    (compose, port)
}

/// Wait for at least one session to appear.
/// Handles both `[...]` and `{"sessions": [...]}` response shapes.
async fn wait_for_sessions(port: u16) {
    let url = format!("http://localhost:{port}/api/sessions");
    for _ in 0..30 {
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
    panic!("timed out waiting for sessions to load (15s)");
}

/// Compose stack starts and both services are discovered.
#[tokio::test]
async fn compose_stack_starts_with_nats() {
    let (compose, port) = start_compose_stack().await;

    // Both services should be present
    let services = compose.services();
    assert!(
        services.contains(&"nats"),
        "expected nats service, got: {services:?}"
    );
    assert!(
        services.contains(&"server"),
        "expected server service, got: {services:?}"
    );

    // Server should respond
    let resp = reqwest::get(format!("http://localhost:{port}/api/sessions"))
        .await
        .expect("HTTP request failed");
    assert_eq!(resp.status(), 200);
}

/// Sessions load via the full bus path: watcher → NATS → consumer → ingest.
#[tokio::test]
async fn compose_sessions_load_via_nats_bus() {
    let (_compose, port) = start_compose_stack().await;

    wait_for_sessions(port).await;

    let body: Value = reqwest::get(format!("http://localhost:{port}/api/sessions"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let sessions = body.get("sessions").and_then(|v| v.as_array())
        .or_else(|| body.as_array())
        .expect("sessions array");

    assert!(
        !sessions.is_empty(),
        "expected sessions loaded via NATS bus"
    );
}

/// View records are available through the full bus path.
#[tokio::test]
async fn compose_view_records_via_nats_bus() {
    let (_compose, port) = start_compose_stack().await;

    wait_for_sessions(port).await;

    let body: Value = reqwest::get(format!("http://localhost:{port}/api/sessions"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let sessions = body.get("sessions").and_then(|v| v.as_array())
        .or_else(|| body.as_array())
        .expect("sessions array");

    assert!(!sessions.is_empty(), "no sessions loaded");

    let session_id = sessions[0]["session_id"]
        .as_str()
        .expect("session_id should be a string");

    let records: Vec<Value> = reqwest::get(format!(
        "http://localhost:{port}/api/sessions/{session_id}/view-records"
    ))
    .await
    .unwrap()
    .json()
    .await
    .unwrap();

    assert!(
        !records.is_empty(),
        "expected view records for session {session_id}"
    );

    // ViewRecords should have the expected shape
    let first = &records[0];
    assert!(first.get("record_type").is_some());
    assert!(first.get("payload").is_some());
}

// `compose_hooks_accepted_with_nats` retired alongside the /hooks endpoint
// (the watcher is the sole ingestion source — see refactor: kill /hooks).
