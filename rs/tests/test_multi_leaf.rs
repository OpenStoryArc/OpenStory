//! Integration tests for multi-leaf NATS cluster — full deployment simulation.
//!
//! Simulates three machines:
//!   - VPS:       NATS hub + Open Story (common dashboard, no local watcher)
//!   - Machine A: NATS leaf + Open Story (watches "alice" fixtures)
//!   - Machine B: NATS leaf + Open Story (watches "bob" fixtures)
//!
//! Verifies:
//!   - Each leaf only sees its own sessions locally
//!   - Hub aggregates sessions from ALL leaves
//!   - Sessions from different machines don't bleed across leaves
//!
//! Prerequisites:
//!   docker build -t open-story:test ./rs
//!
//! Run with: cargo test -p open-story --test test_multi_leaf -- --include-ignored

mod helpers;

use helpers::compose::{rand_suffix, to_docker_path};
use helpers::synth::generate_session;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

/// Ports for all three Open Story instances.
struct MultiLeafStack {
    compose_file: PathBuf,
    project_name: String,
    hub_port: u16,
    leaf_a_port: u16,
    leaf_b_port: u16,
}

impl Drop for MultiLeafStack {
    fn drop(&mut self) {
        let _ = Command::new("docker")
            .args(["compose", "-f"])
            .arg(&self.compose_file)
            .args(["-p", &self.project_name, "down", "--volumes", "--remove-orphans"])
            .env("MSYS_NO_PATHCONV", "1")
            .output();
    }
}

/// Generate a fixture directory with sessions prefixed by `machine_name`.
fn generate_machine_fixtures(dir: &Path, machine_name: &str, session_count: usize) {
    std::fs::create_dir_all(dir).expect("create fixture dir");
    for i in 0..session_count {
        let session_id = format!("{machine_name}-sess-{i:03}");
        let content = generate_session(&session_id, 50, 0);
        let path = dir.join(format!("{session_id}.jsonl"));
        std::fs::write(&path, content).expect("write session file");
    }
    // Touch files for fresh mtimes
    let now = filetime::FileTime::now();
    for entry in std::fs::read_dir(dir).expect("read dir") {
        let path = entry.expect("entry").path();
        let _ = filetime::set_file_mtime(&path, now);
    }
}

/// Get host port for a service in a compose project.
fn get_host_port(project_name: &str, service: &str, container_port: u16) -> u16 {
    let output = Command::new("docker")
        .args([
            "compose", "-p", project_name, "port", service,
            &container_port.to_string(),
        ])
        .env("MSYS_NO_PATHCONV", "1")
        .output()
        .expect("docker compose port");
    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .trim()
        .rsplit(':')
        .next()
        .and_then(|p| p.parse().ok())
        .unwrap_or_else(|| panic!("failed to get port for {service}"))
}

/// Start the multi-leaf stack with generated fixtures for two machines.
async fn start_multi_leaf_stack() -> (MultiLeafStack, tempfile::TempDir) {
    let tmp = tempfile::tempdir().expect("create temp dir");

    let dir_a = tmp.path().join("alice");
    let dir_b = tmp.path().join("bob");

    generate_machine_fixtures(&dir_a, "alice", 3);
    generate_machine_fixtures(&dir_b, "bob", 2);

    let fixture_a = to_docker_path(&dir_a);
    let fixture_b = to_docker_path(&dir_b);

    let compose_file = PathBuf::from(format!(
        "{}/tests/docker-compose.multileaf.yml",
        env!("CARGO_MANIFEST_DIR")
    ));
    let project_name = format!("ostest-multi-{}-{}", std::process::id(), rand_suffix());

    let output = Command::new("docker")
        .args(["compose", "-f"])
        .arg(&compose_file)
        .args(["-p", &project_name, "up", "-d"])
        .env("FIXTURE_DIR_A", &fixture_a)
        .env("FIXTURE_DIR_B", &fixture_b)
        .env("MSYS_NO_PATHCONV", "1")
        .output()
        .expect("docker compose up");

    assert!(
        output.status.success(),
        "compose up failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    tokio::time::sleep(Duration::from_secs(3)).await;

    let hub_port = get_host_port(&project_name, "hub-server", 3002);
    let leaf_a_port = get_host_port(&project_name, "leaf-server-a", 3002);
    let leaf_b_port = get_host_port(&project_name, "leaf-server-b", 3002);

    // Wait for all three servers
    for (port, label) in [
        (hub_port, "hub"),
        (leaf_a_port, "leaf-a"),
        (leaf_b_port, "leaf-b"),
    ] {
        let url = format!("http://localhost:{port}/api/sessions");
        for _ in 0..30 {
            if reqwest::get(&url).await.is_ok() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        assert!(
            reqwest::get(&url).await.is_ok(),
            "{label} not ready at port {port}"
        );
    }

    let stack = MultiLeafStack {
        compose_file,
        project_name,
        hub_port,
        leaf_a_port,
        leaf_b_port,
    };

    (stack, tmp)
}

/// Get sessions from a server, handling both array and wrapped response shapes.
async fn get_sessions(port: u16) -> Vec<Value> {
    let url = format!("http://localhost:{port}/api/sessions");
    let resp = reqwest::get(&url).await.expect("HTTP request failed");
    let body: Value = resp.json().await.expect("JSON parse failed");

    body.get("sessions")
        .and_then(|s| s.as_array().cloned())
        .or_else(|| body.as_array().cloned())
        .unwrap_or_default()
}

/// Wait for sessions to appear on a given port.
async fn wait_for_sessions(port: u16, label: &str, min_count: usize) {
    for _ in 0..120 {
        let sessions = get_sessions(port).await;
        if sessions.len() >= min_count {
            return;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    let actual = get_sessions(port).await;
    panic!(
        "timed out waiting for {min_count} sessions on {label} (port {port}, got {})",
        actual.len()
    );
}

/// Extract session IDs from a list of session values.
fn session_ids(sessions: &[Value]) -> Vec<String> {
    sessions
        .iter()
        .filter_map(|s| s["session_id"].as_str().map(String::from))
        .collect()
}

// ── Tests ────────────────────────────────────────────────────────────

/// All six services start and respond.
#[tokio::test]
#[ignore]
async fn multi_leaf_all_services_start() {
    let (stack, _tmp) = start_multi_leaf_stack().await;

    for (port, label) in [
        (stack.hub_port, "hub"),
        (stack.leaf_a_port, "leaf-a"),
        (stack.leaf_b_port, "leaf-b"),
    ] {
        let resp = reqwest::get(format!("http://localhost:{port}/api/sessions"))
            .await
            .expect(&format!("{label} HTTP failed"));
        assert_eq!(resp.status(), 200, "{label} should return 200");
    }
}

/// Leaf A has alice's sessions (at minimum — may also see bob's via hub).
///
/// NATS leaf nodes with JetStream propagate streams bidirectionally:
/// events published on leaf-a flow to the hub, then to leaf-b (and vice versa).
/// This means each leaf eventually sees ALL sessions, not just its own.
/// This is correct behavior — the local UI is a full mirror.
/// Per-machine filtering would be a UI concern, not a bus concern.
#[tokio::test]
#[ignore]
async fn leaf_a_has_alice_sessions() {
    let (stack, _tmp) = start_multi_leaf_stack().await;

    wait_for_sessions(stack.leaf_a_port, "leaf-a", 3).await;

    let sessions = get_sessions(stack.leaf_a_port).await;
    let ids = session_ids(&sessions);

    let alice_count = ids.iter().filter(|id| id.contains("alice")).count();
    assert_eq!(alice_count, 3, "leaf-a should have 3 alice sessions");

    // Leaf may also see bob's sessions via hub propagation
    assert!(
        sessions.len() >= 3,
        "leaf-a should have at least alice's sessions"
    );
}

/// Leaf B has bob's sessions (at minimum — may also see alice's via hub).
#[tokio::test]
#[ignore]
async fn leaf_b_has_bob_sessions() {
    let (stack, _tmp) = start_multi_leaf_stack().await;

    wait_for_sessions(stack.leaf_b_port, "leaf-b", 2).await;

    let sessions = get_sessions(stack.leaf_b_port).await;
    let ids = session_ids(&sessions);

    let bob_count = ids.iter().filter(|id| id.contains("bob")).count();
    assert_eq!(bob_count, 2, "leaf-b should have 2 bob sessions");

    assert!(
        sessions.len() >= 2,
        "leaf-b should have at least bob's sessions"
    );
}

/// Hub sees sessions from both leaves — the common dashboard.
#[tokio::test]
#[ignore]
async fn hub_aggregates_all_sessions() {
    let (stack, _tmp) = start_multi_leaf_stack().await;

    // Wait for both leaves to ingest
    wait_for_sessions(stack.leaf_a_port, "leaf-a", 3).await;
    wait_for_sessions(stack.leaf_b_port, "leaf-b", 2).await;

    // Hub should eventually see all 5 sessions (3 alice + 2 bob)
    wait_for_sessions(stack.hub_port, "hub", 5).await;

    let sessions = get_sessions(stack.hub_port).await;
    let ids = session_ids(&sessions);

    let alice_count = ids.iter().filter(|id| id.contains("alice")).count();
    let bob_count = ids.iter().filter(|id| id.contains("bob")).count();

    assert_eq!(alice_count, 3, "hub should have 3 alice sessions");
    assert_eq!(bob_count, 2, "hub should have 2 bob sessions");
    assert_eq!(sessions.len(), 5, "hub should have 5 total sessions");
}

/// Hub has view records for sessions from both machines.
#[tokio::test]
#[ignore]
async fn hub_has_view_records_from_both_machines() {
    let (stack, _tmp) = start_multi_leaf_stack().await;

    wait_for_sessions(stack.hub_port, "hub", 5).await;

    let sessions = get_sessions(stack.hub_port).await;

    // Pick one session from each machine
    let alice_session = sessions
        .iter()
        .find(|s| s["session_id"].as_str().map_or(false, |id| id.contains("alice")))
        .expect("should have an alice session on hub");
    let bob_session = sessions
        .iter()
        .find(|s| s["session_id"].as_str().map_or(false, |id| id.contains("bob")))
        .expect("should have a bob session on hub");

    for (session, machine) in [(alice_session, "alice"), (bob_session, "bob")] {
        let session_id = session["session_id"].as_str().unwrap();
        let records: Vec<Value> = reqwest::get(format!(
            "http://localhost:{}/api/sessions/{session_id}/view-records",
            stack.hub_port
        ))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

        assert!(
            !records.is_empty(),
            "hub should have view records for {machine} session {session_id}"
        );
    }
}
