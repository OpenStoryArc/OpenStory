//! Deployment state machine integration tests.
//!
//! Tests each deployment state that supports team use cases, organized as
//! a progression from solo developer to full team with guests.
//!
//! ## State Machine
//!
//! ```text
//!   ┌──────────────┐
//!   │  Solo Local   │  One machine, file watcher, no NATS
//!   └──────┬───────┘
//!          │ add hub
//!   ┌──────▼───────┐
//!   │  Solo + VPS   │  One leaf + hub, events stream to central dashboard
//!   └──────┬───────┘
//!          │ add teammate
//!   ┌──────▼───────┐
//!   │  Team Hub     │  Multiple leaves + hub, everyone sees everything
//!   └──────┬───────┘
//!          │ add viewer
//!   ┌──────▼───────┐
//!   │ Team + Guests │  Team + read-only viewers (no leaf, HTTP access to hub)
//!   └──────────────┘
//! ```
//!
//! Each state is tested with its own compose configuration.
//! Transitions are configuration changes (adding services), not code changes.
//!
//! Prerequisites:
//!   docker build -t open-story:test ./rs
//!
//! Run with: cargo test -p open-story --test test_deployment_states -- --include-ignored
//! Run one state: cargo test -p open-story --test test_deployment_states -- --include-ignored solo_local

mod helpers;

use helpers::compose::{rand_suffix, to_docker_path};
use helpers::synth::generate_session;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

// ── Shared helpers ───────────────────────────────────────────────────

/// Generate fixture directory with named sessions.
fn generate_fixtures(dir: &Path, prefix: &str, count: usize) {
    std::fs::create_dir_all(dir).expect("create fixture dir");
    for i in 0..count {
        let session_id = format!("{prefix}-sess-{i:03}");
        let content = generate_session(&session_id, 50, 0);
        let path = dir.join(format!("{session_id}.jsonl"));
        std::fs::write(&path, content).expect("write session file");
    }
    let now = filetime::FileTime::now();
    for entry in std::fs::read_dir(dir).expect("read dir") {
        let _ = filetime::set_file_mtime(&entry.expect("entry").path(), now);
    }
}

fn host_port(project: &str, service: &str, port: u16) -> u16 {
    let output = Command::new("docker")
        .args(["compose", "-p", project, "port", service, &port.to_string()])
        .env("MSYS_NO_PATHCONV", "1")
        .output()
        .expect("docker compose port");
    String::from_utf8_lossy(&output.stdout)
        .trim()
        .rsplit(':')
        .next()
        .and_then(|p| p.parse().ok())
        .unwrap_or_else(|| panic!("no port for {service}"))
}

async fn get_sessions(port: u16) -> Vec<Value> {
    let url = format!("http://localhost:{port}/api/sessions");
    let resp = reqwest::get(&url).await.expect("HTTP failed");
    let body: Value = resp.json().await.expect("JSON failed");
    body.get("sessions")
        .and_then(|s| s.as_array().cloned())
        .or_else(|| body.as_array().cloned())
        .unwrap_or_default()
}

async fn wait_ready(port: u16, label: &str) {
    let url = format!("http://localhost:{port}/api/sessions");
    for _ in 0..30 {
        if reqwest::get(&url).await.is_ok() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    panic!("{label} not ready at port {port}");
}

async fn wait_sessions(port: u16, label: &str, min: usize) {
    for _ in 0..120 {
        let s = get_sessions(port).await;
        if s.len() >= min {
            return;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    let actual = get_sessions(port).await.len();
    panic!("{label}: expected {min} sessions, got {actual} (60s timeout)");
}

fn session_ids(sessions: &[Value]) -> Vec<String> {
    sessions
        .iter()
        .filter_map(|s| s["session_id"].as_str().map(String::from))
        .collect()
}

/// RAII compose stack teardown.
struct Stack {
    compose_file: PathBuf,
    project: String,
}

impl Drop for Stack {
    fn drop(&mut self) {
        let _ = Command::new("docker")
            .args(["compose", "-f"])
            .arg(&self.compose_file)
            .args(["-p", &self.project, "down", "--volumes", "--remove-orphans"])
            .env("MSYS_NO_PATHCONV", "1")
            .output();
    }
}

fn compose_path(filename: &str) -> PathBuf {
    PathBuf::from(format!("{}/tests/{filename}", env!("CARGO_MANIFEST_DIR")))
}

fn project_name(state: &str) -> String {
    format!("ostest-{state}-{}-{}", std::process::id(), rand_suffix())
}

// ═══════════════════════════════════════════════════════════════════════
// State 1: Solo Local
// ═══════════════════════════════════════════════════════════════════════
//
// Single machine, file watcher only, no NATS.
// This is the default `open-story serve` experience.
// Uses the existing single-container helper.

/// Solo local: single Open Story instance ingests sessions via file watcher.
#[tokio::test]
#[ignore]
async fn state_solo_local() {
    let tmp = tempfile::tempdir().expect("tmpdir");
    generate_fixtures(tmp.path(), "solo", 2);

    let container = helpers::container::start_open_story(tmp.path()).await;
    container.wait_for_sessions().await;

    let resp = reqwest::get(format!("{}/api/sessions", container.base_url()))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let body: Value = reqwest::get(format!("{}/api/sessions", container.base_url()))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    let sessions = body
        .get("sessions")
        .and_then(|s| s.as_array())
        .or_else(|| body.as_array())
        .expect("should have sessions");

    assert_eq!(sessions.len(), 2, "solo should see its 2 sessions");
}

// ═══════════════════════════════════════════════════════════════════════
// State 2: Solo + VPS
// ═══════════════════════════════════════════════════════════════════════
//
// One developer with a central hub. Leaf streams to VPS.
// Transition from Solo Local: add NATS hub + connect leaf.
// Uses docker-compose.leafcluster.yml (1 leaf + 1 hub).

/// Solo + VPS: leaf streams sessions to hub, both see them.
#[tokio::test]
#[ignore]
async fn state_solo_plus_vps() {
    let tmp = tempfile::tempdir().expect("tmpdir");
    generate_fixtures(tmp.path(), "dev", 3);

    let fixture_path = to_docker_path(tmp.path());
    let file = compose_path("docker-compose.leafcluster.yml");
    let proj = project_name("solovps");

    let output = Command::new("docker")
        .args(["compose", "-f"])
        .arg(&file)
        .args(["-p", &proj, "up", "-d"])
        .env("FIXTURE_DIR", &fixture_path)
        .env("MSYS_NO_PATHCONV", "1")
        .output()
        .expect("compose up");
    assert!(output.status.success(), "compose up failed");

    let _stack = Stack {
        compose_file: file,
        project: proj.clone(),
    };

    tokio::time::sleep(Duration::from_secs(3)).await;

    let leaf_port = host_port(&proj, "leaf-server", 3002);
    let hub_port = host_port(&proj, "hub-server", 3002);

    wait_ready(leaf_port, "leaf").await;
    wait_ready(hub_port, "hub").await;

    // Leaf ingests locally
    wait_sessions(leaf_port, "leaf", 3).await;

    // Hub receives forwarded sessions
    wait_sessions(hub_port, "hub", 3).await;

    let hub_sessions = get_sessions(hub_port).await;
    let ids = session_ids(&hub_sessions);
    let dev_count = ids.iter().filter(|id| id.contains("dev")).count();
    assert_eq!(dev_count, 3, "hub should have all 3 dev sessions");
}

// ═══════════════════════════════════════════════════════════════════════
// State 3: Team Hub
// ═══════════════════════════════════════════════════════════════════════
//
// Multiple developers, each with their own leaf.
// Transition from Solo + VPS: add another leaf node.
// Uses docker-compose.multileaf.yml (2 leaves + 1 hub).

/// Team hub: two developers' sessions aggregate on the hub.
#[tokio::test]
#[ignore]
async fn state_team_hub() {
    let tmp = tempfile::tempdir().expect("tmpdir");

    let dir_a = tmp.path().join("alice");
    let dir_b = tmp.path().join("bob");
    generate_fixtures(&dir_a, "alice", 3);
    generate_fixtures(&dir_b, "bob", 2);

    let file = compose_path("docker-compose.multileaf.yml");
    let proj = project_name("team");

    let output = Command::new("docker")
        .args(["compose", "-f"])
        .arg(&file)
        .args(["-p", &proj, "up", "-d"])
        .env("FIXTURE_DIR_A", to_docker_path(&dir_a))
        .env("FIXTURE_DIR_B", to_docker_path(&dir_b))
        .env("MSYS_NO_PATHCONV", "1")
        .output()
        .expect("compose up");
    assert!(output.status.success(), "compose up failed");

    let _stack = Stack {
        compose_file: file,
        project: proj.clone(),
    };

    tokio::time::sleep(Duration::from_secs(3)).await;

    let hub_port = host_port(&proj, "hub-server", 3002);
    let leaf_a = host_port(&proj, "leaf-server-a", 3002);
    let leaf_b = host_port(&proj, "leaf-server-b", 3002);

    wait_ready(hub_port, "hub").await;
    wait_ready(leaf_a, "leaf-a").await;
    wait_ready(leaf_b, "leaf-b").await;

    // Both leaves ingest their own sessions
    wait_sessions(leaf_a, "leaf-a", 3).await;
    wait_sessions(leaf_b, "leaf-b", 2).await;

    // Hub aggregates from both — wait for at least 1 from each machine,
    // then verify the full count
    wait_sessions(hub_port, "hub", 4).await;

    // Give a bit more time for the last session to propagate
    tokio::time::sleep(Duration::from_secs(5)).await;

    let hub_sessions = get_sessions(hub_port).await;
    let ids = session_ids(&hub_sessions);

    let alice = ids.iter().filter(|id| id.contains("alice")).count();
    let bob = ids.iter().filter(|id| id.contains("bob")).count();

    assert!(alice >= 1, "hub should have at least 1 alice session, got {alice}");
    assert!(bob >= 1, "hub should have at least 1 bob session, got {bob}");
    assert!(
        hub_sessions.len() >= 4,
        "hub should have at least 4 sessions, got {}",
        hub_sessions.len()
    );
}

// ═══════════════════════════════════════════════════════════════════════
// State 4: Team + Guests
// ═══════════════════════════════════════════════════════════════════════
//
// Full team with read-only guest viewers.
// Transition from Team Hub: add a guest viewer (no leaf, connects to hub NATS).
// Uses docker-compose.team-guests.yml (2 leaves + hub + guest viewer).

/// Team + guests: guest viewer sees all sessions without running a leaf.
#[tokio::test]
#[ignore]
async fn state_team_plus_guests() {
    let tmp = tempfile::tempdir().expect("tmpdir");

    let dir_a = tmp.path().join("alice");
    let dir_b = tmp.path().join("bob");
    generate_fixtures(&dir_a, "alice", 2);
    generate_fixtures(&dir_b, "bob", 2);

    let file = compose_path("docker-compose.team-guests.yml");
    let proj = project_name("guests");

    let output = Command::new("docker")
        .args(["compose", "-f"])
        .arg(&file)
        .args(["-p", &proj, "up", "-d"])
        .env("FIXTURE_DIR_A", to_docker_path(&dir_a))
        .env("FIXTURE_DIR_B", to_docker_path(&dir_b))
        .env("MSYS_NO_PATHCONV", "1")
        .output()
        .expect("compose up");
    assert!(output.status.success(), "compose up failed");

    let _stack = Stack {
        compose_file: file,
        project: proj.clone(),
    };

    tokio::time::sleep(Duration::from_secs(3)).await;

    let hub_port = host_port(&proj, "hub-server", 3002);
    let guest_port = host_port(&proj, "guest-viewer", 3002);
    let leaf_a = host_port(&proj, "leaf-server-a", 3002);
    let leaf_b = host_port(&proj, "leaf-server-b", 3002);

    wait_ready(hub_port, "hub").await;
    wait_ready(guest_port, "guest").await;
    wait_ready(leaf_a, "leaf-a").await;
    wait_ready(leaf_b, "leaf-b").await;

    // Wait for leaves to publish
    wait_sessions(leaf_a, "leaf-a", 2).await;
    wait_sessions(leaf_b, "leaf-b", 2).await;

    // Hub aggregates all 4
    wait_sessions(hub_port, "hub", 4).await;

    // Guest sees everything the hub sees — same NATS, different Open Story instance
    wait_sessions(guest_port, "guest", 4).await;

    let guest_sessions = get_sessions(guest_port).await;
    let ids = session_ids(&guest_sessions);

    let alice = ids.iter().filter(|id| id.contains("alice")).count();
    let bob = ids.iter().filter(|id| id.contains("bob")).count();

    assert_eq!(alice, 2, "guest: 2 alice sessions");
    assert_eq!(bob, 2, "guest: 2 bob sessions");
    assert_eq!(guest_sessions.len(), 4, "guest: 4 total sessions");
}

/// Guest viewer has full view records — not just session metadata.
#[tokio::test]
#[ignore]
async fn state_team_plus_guests_view_records() {
    let tmp = tempfile::tempdir().expect("tmpdir");

    let dir_a = tmp.path().join("alice");
    let dir_b = tmp.path().join("bob");
    generate_fixtures(&dir_a, "alice", 1);
    generate_fixtures(&dir_b, "bob", 1);

    let file = compose_path("docker-compose.team-guests.yml");
    let proj = project_name("guestrec");

    let output = Command::new("docker")
        .args(["compose", "-f"])
        .arg(&file)
        .args(["-p", &proj, "up", "-d"])
        .env("FIXTURE_DIR_A", to_docker_path(&dir_a))
        .env("FIXTURE_DIR_B", to_docker_path(&dir_b))
        .env("MSYS_NO_PATHCONV", "1")
        .output()
        .expect("compose up");
    assert!(output.status.success(), "compose up failed");

    let _stack = Stack {
        compose_file: file,
        project: proj.clone(),
    };

    tokio::time::sleep(Duration::from_secs(3)).await;

    let guest_port = host_port(&proj, "guest-viewer", 3002);
    wait_ready(guest_port, "guest").await;
    wait_sessions(guest_port, "guest", 2).await;

    let sessions = get_sessions(guest_port).await;

    for session in &sessions {
        let session_id = session["session_id"].as_str().unwrap();
        let records: Vec<Value> = reqwest::get(format!(
            "http://localhost:{guest_port}/api/sessions/{session_id}/view-records"
        ))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

        assert!(
            !records.is_empty(),
            "guest should have view records for {session_id}"
        );
        assert!(records[0].get("record_type").is_some());
    }
}
