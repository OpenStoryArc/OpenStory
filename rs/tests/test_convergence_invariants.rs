//! Convergence invariants for the NATS-backed pipeline.
//!
//! The four actor-consumers write to independent SQLite tables
//! asynchronously. The tables are eventually consistent — not strictly
//! ordered. These tests pin invariants that must converge within a
//! bounded polling window regardless of the Phase 1 refactor:
//!
//!   1. `sessions.event_count` converges to match the actual row count
//!      in the `events` table for that session (or stays at/above the
//!      row count under EC — session row may lag events by a tick).
//!   2. Every row in `events_fts` references an existing `events` row
//!      (no orphan FTS entries).
//!   3. No session row exists without at least one corresponding event
//!      row (after convergence — sessions with zero events should not
//!      appear under the NATS path since PersistConsumer writes session
//!      rows only after events are durable).
//!
//! This is commit 0e of the TDD plan at
//! `/Users/maxglassie/.claude/plans/can-we-plan-a-cuddly-moler.md`.
//!
//! These invariants pass today and must keep passing through Phase 1.

mod helpers;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use helpers::fixtures_dir;
use serde_json::Value;
use testcontainers::compose::DockerCompose;

fn compose_file() -> PathBuf {
    PathBuf::from(format!(
        "{}/tests/docker-compose.test.yml",
        env!("CARGO_MANIFEST_DIR")
    ))
}

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

fn docker_path_string(path: &Path) -> String {
    let canonical = path.canonicalize().expect("canonicalize");
    let s = canonical.to_string_lossy().to_string();
    let s = s.strip_prefix(r"\\?\").unwrap_or(&s).to_string();
    s.replace('\\', "/")
}

async fn start_stack() -> (DockerCompose, tempfile::TempDir, u16) {
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

async fn get_events(port: u16, session_id: &str) -> Vec<Value> {
    let url = format!("http://localhost:{port}/api/sessions/{session_id}/events");
    reqwest::get(&url)
        .await
        .unwrap()
        .json::<Vec<Value>>()
        .await
        .unwrap_or_default()
}

/// Poll until convergence: the event count on /sessions agrees with
/// the length of /events, AND the length has been stable for 3 polls.
async fn wait_for_convergence(port: u16, session_id: &str) -> (u64, usize) {
    let mut last_count = 0usize;
    let mut stable = 0;
    for _ in 0..60 {
        let sessions = get_sessions(port).await;
        let session = sessions
            .iter()
            .find(|s| s["session_id"].as_str() == Some(session_id));
        if let Some(s) = session {
            let row_count = s.get("event_count").and_then(|v| v.as_u64()).unwrap_or(0);
            let events = get_events(port, session_id).await;
            if row_count as usize == events.len() && events.len() > 0 {
                if events.len() == last_count {
                    stable += 1;
                    if stable >= 3 {
                        return (row_count, events.len());
                    }
                } else {
                    stable = 0;
                    last_count = events.len();
                }
            } else {
                stable = 0;
                last_count = events.len();
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    panic!("sessions.event_count never converged with /events length for {session_id}");
}

// ── Invariants ──

/// After convergence, `sessions.event_count` equals the number of rows
/// returned by `/api/sessions/{id}/events`.
#[tokio::test]
#[ignore = "docker-required: needs open-story:test image; run locally via `cd rs && docker build -t open-story:test . && cargo test --test test_convergence_invariants -- --ignored`"]
async fn sessions_event_count_converges_with_events_table() {
    let (_compose, _tmp, port) = start_stack().await;

    // Wait for at least one session.
    for _ in 0..30 {
        if !get_sessions(port).await.is_empty() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    let sessions = get_sessions(port).await;
    let session_id = sessions
        .iter()
        .find(|s| {
            s["session_id"]
                .as_str()
                .map_or(false, |id| id.contains("pi_mono"))
        })
        .expect("pi-mono session not found")
        ["session_id"]
        .as_str()
        .unwrap()
        .to_string();

    let (sessions_count, events_count) = wait_for_convergence(port, &session_id).await;
    assert_eq!(
        sessions_count as usize, events_count,
        "after convergence, sessions.event_count ({sessions_count}) should equal /events length ({events_count})"
    );
}

/// After convergence, every FTS search hit has a corresponding event.
/// (FTS rows should never outlive their parent event.)
#[tokio::test]
#[ignore = "docker-required: needs open-story:test image; run locally via `cd rs && docker build -t open-story:test . && cargo test --test test_convergence_invariants -- --ignored`"]
async fn fts_references_valid_events() {
    let (_compose, _tmp, port) = start_stack().await;

    for _ in 0..30 {
        if !get_sessions(port).await.is_empty() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    let sessions = get_sessions(port).await;
    let session_id = sessions
        .iter()
        .find(|s| {
            s["session_id"]
                .as_str()
                .map_or(false, |id| id.contains("pi_mono"))
        })
        .expect("pi-mono session not found")
        ["session_id"]
        .as_str()
        .unwrap()
        .to_string();

    let _ = wait_for_convergence(port, &session_id).await;

    let events = get_events(port, &session_id).await;
    let event_ids: HashSet<String> = events
        .iter()
        .filter_map(|e| e.get("id").and_then(|v| v.as_str()).map(String::from))
        .collect();

    // Search for the fixture's known user prompt: "Read the config file".
    let results: Vec<Value> = reqwest::get(format!("http://localhost:{port}/api/search?q=config"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap_or_default();

    let mut orphans: Vec<String> = Vec::new();
    for r in &results {
        if let Some(eid) = r.get("event_id").and_then(|v| v.as_str()) {
            if !event_ids.contains(eid) {
                orphans.push(eid.to_string());
            }
        }
    }
    assert!(
        orphans.is_empty(),
        "FTS returned event_ids not present in /events: {:?}",
        orphans
    );
}

/// No session row exists without at least one event under its session_id
/// (after convergence). PersistConsumer should not surface empty
/// sessions in /api/sessions.
#[tokio::test]
#[ignore = "docker-required: needs open-story:test image; run locally via `cd rs && docker build -t open-story:test . && cargo test --test test_convergence_invariants -- --ignored`"]
async fn no_empty_session_rows_after_convergence() {
    let (_compose, _tmp, port) = start_stack().await;

    for _ in 0..30 {
        if !get_sessions(port).await.is_empty() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }

    // Give the pipeline a moment to settle.
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    let sessions = get_sessions(port).await;
    let mut empty_sessions: HashMap<String, u64> = HashMap::new();
    for s in &sessions {
        let sid = s["session_id"].as_str().unwrap_or("").to_string();
        let count = s.get("event_count").and_then(|v| v.as_u64()).unwrap_or(0);
        let events = get_events(port, &sid).await;
        if events.is_empty() && count == 0 {
            empty_sessions.insert(sid, count);
        }
    }
    assert!(
        empty_sessions.is_empty(),
        "found session rows with zero events: {:?}",
        empty_sessions
    );
}
