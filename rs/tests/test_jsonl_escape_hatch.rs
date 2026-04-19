//! JSONL escape-hatch capstone test.
//!
//! Sovereignty invariant (CLAUDE.md):
//! > "your data stays local, in open formats, portable and unencumbered."
//!
//! The JSONL backup (`data/{session_id}.jsonl`) is the load-bearing
//! promise. If `PersistConsumer.session_store.append(...)` ever silently
//! fails — or a future refactor forgets it — users lose the sovereignty
//! escape hatch with no test failing.
//!
//! This test runs the pi-mono fixture through the full NATS pipeline,
//! then walks the host-side `/data` bind mount and asserts that every
//! event UUID landed in SQLite also landed in the JSONL backup, exactly
//! once, with no torn lines.
//!
//! This is commit 0d of the TDD plan at
//! `/Users/maxglassie/.claude/plans/can-we-plan-a-cuddly-moler.md`.

mod helpers;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use helpers::fixtures_dir;
use serde_json::Value;
use testcontainers::compose::DockerCompose;

fn compose_file() -> PathBuf {
    PathBuf::from(format!(
        "{}/tests/docker-compose.jsonl-escape-hatch.yml",
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

    let now = filetime::FileTime::now();
    let _ = filetime::set_file_mtime(&dst, now);

    let path_str = docker_path_string(tmp.path());
    (tmp, path_str)
}

/// Create a writable host dir that will receive the container's /data
/// bind mount.
fn data_dir_mount_path() -> (tempfile::TempDir, String) {
    let tmp = tempfile::TempDir::new().expect("create data temp dir");
    std::fs::create_dir_all(tmp.path()).expect("ensure data dir exists");
    let path_str = docker_path_string(tmp.path());
    (tmp, path_str)
}

fn docker_path_string(path: &Path) -> String {
    let canonical = path.canonicalize().expect("canonicalize");
    let s = canonical.to_string_lossy().to_string();
    let s = s.strip_prefix(r"\\?\").unwrap_or(&s).to_string();
    s.replace('\\', "/")
}

async fn start_stack() -> (DockerCompose, tempfile::TempDir, tempfile::TempDir, u16) {
    let (fixture_tmp, fixture_path) = pi_mono_mount_path();
    let (data_tmp, data_path) = data_dir_mount_path();

    let mut compose = DockerCompose::with_local_client(&[compose_file()])
        .with_env("FIXTURE_DIR", &fixture_path)
        .with_env("DATA_DIR", &data_path)
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
    (compose, fixture_tmp, data_tmp, port)
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

fn find_pi_session(sessions: &[Value]) -> Option<&Value> {
    sessions.iter().find(|s| {
        s["session_id"]
            .as_str()
            .map_or(false, |id| id.contains("pi_mono"))
    })
}

/// Poll `/events` for the pi-mono session until the count stabilizes.
async fn wait_for_events_stable(port: u16, session_id: &str) -> Vec<Value> {
    let url = format!("http://localhost:{port}/api/sessions/{session_id}/events");
    let mut last = 0usize;
    let mut stable = 0;
    for _ in 0..60 {
        if let Ok(resp) = reqwest::get(&url).await {
            if let Ok(events) = resp.json::<Vec<Value>>().await {
                if !events.is_empty() && events.len() == last {
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
    panic!("events never stabilized for session {session_id}");
}

/// Collect all UUIDs appearing as `"id"` in JSONL files under `data_dir`.
fn read_jsonl_uuids(data_dir: &Path) -> HashMap<String, usize> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for entry in walkdir::WalkDir::new(data_dir).into_iter().filter_map(|e| e.ok()) {
        if entry.file_type().is_file()
            && entry.path().extension().and_then(|e| e.to_str()) == Some("jsonl")
        {
            let text = std::fs::read_to_string(entry.path()).unwrap_or_default();
            for (line_num, line) in text.lines().enumerate() {
                if line.trim().is_empty() {
                    continue;
                }
                let parsed: Value = serde_json::from_str(line).unwrap_or_else(|e| {
                    panic!(
                        "torn JSONL line in sovereignty escape hatch: {:?} line {}: {}",
                        entry.path(),
                        line_num + 1,
                        e
                    )
                });
                if let Some(id) = parsed.get("id").and_then(|v| v.as_str()) {
                    *counts.entry(id.to_string()).or_insert(0) += 1;
                }
            }
        }
    }
    counts
}

// ── Test ──

/// Every event UUID in SQLite appears exactly once in the JSONL escape hatch.
#[tokio::test]
async fn jsonl_escape_hatch_matches_sqlite_event_ids() {
    let (_compose, _fixture_tmp, data_tmp, port) = start_stack().await;

    // Wait for the session to appear.
    for _ in 0..30 {
        if !get_sessions(port).await.is_empty() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    let sessions = get_sessions(port).await;
    let session = find_pi_session(&sessions).expect("pi-mono session not loaded");
    let session_id = session["session_id"].as_str().unwrap().to_string();

    // Wait for events to stabilize in SQLite.
    let events = wait_for_events_stable(port, &session_id).await;

    let sqlite_ids: HashSet<String> = events
        .iter()
        .filter_map(|e| e.get("id").and_then(|v| v.as_str()).map(String::from))
        .collect();
    assert!(
        !sqlite_ids.is_empty(),
        "no events reached SQLite — nothing to verify against JSONL"
    );

    // Collect UUIDs from every JSONL file in the bind-mounted data dir.
    // Give PersistConsumer a brief extra tick to drain queued appends.
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    let jsonl_counts = read_jsonl_uuids(data_tmp.path());

    // Every SQLite UUID must appear at least once in the JSONL.
    let missing: Vec<&String> = sqlite_ids.iter().filter(|id| !jsonl_counts.contains_key(*id)).collect();
    assert!(
        missing.is_empty(),
        "sovereignty escape hatch broken — {} event(s) in SQLite not present in JSONL: {:?}",
        missing.len(),
        missing
    );

    // And exactly once (duplicates would pollute the append-only log).
    for id in &sqlite_ids {
        let count = jsonl_counts.get(id).copied().unwrap_or(0);
        assert_eq!(
            count, 1,
            "event {id} appears {count}x in JSONL — must be exactly once"
        );
    }
}
