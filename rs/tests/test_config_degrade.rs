//! Graceful degradation tests — verify behavior when components are missing.
//!
//! Tests trait boundary fallbacks: what happens when NATS or auth tokens are absent?
//! Search always works via FTS5 (no external dependencies).
//!
//! Prerequisites:
//!   docker build -t open-story:test ./rs
//!
//! Run with: cargo test -p open-story --test test_config_degrade -- --ignored --nocapture

#[path = "helpers/container.rs"]
mod container;

use container::start_open_story;
use std::path::{Path, PathBuf};

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
}
use serde_json::Value;

// ── FTS5 search always available ─────────────────────────────────────

/// Search works in minimal config (no external dependencies needed).
#[tokio::test]
#[ignore]
async fn degrade_search_works_without_external_deps() {
    let container = start_open_story(&fixtures_dir()).await;

    let resp = reqwest::get(format!("{}/api/search?q=test", container.base_url()))
        .await
        .unwrap();

    assert_eq!(
        resp.status(),
        200,
        "FTS5 search should always be available"
    );
}

/// Agent search works in minimal config.
#[tokio::test]
#[ignore]
async fn degrade_agent_search_works_without_external_deps() {
    let container = start_open_story(&fixtures_dir()).await;

    let resp = reqwest::get(format!(
        "{}/api/agent/search?q=test",
        container.base_url()
    ))
    .await
    .unwrap();

    assert_eq!(resp.status(), 200);
}

// ── Bus boundary ──────────────────────────────────────────────────────

/// Server without NATS: hooks still work via direct ingest.
#[tokio::test]
#[ignore]
async fn degrade_no_nats_hooks_still_work() {
    // Minimal config — NoopBus, direct ingest
    let container = start_open_story(&fixtures_dir()).await;

    let hook_body = serde_json::json!({
        "session_id": "degrade-hook-test",
        "hook_event_name": "PostToolUse",
        "tool_name": "Read",
        "transcript_path": ""
    });

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/hooks", container.base_url()))
        .json(&hook_body)
        .send()
        .await
        .expect("POST /hooks failed");

    assert_eq!(
        resp.status(),
        202,
        "hooks should work without NATS (direct ingest fallback)"
    );
}

// ── All APIs work in minimal config ──────────────────────────────────

/// Every /api/ endpoint except search works in Minimal config.
#[tokio::test]
#[ignore]
async fn degrade_all_non_search_apis_work() {
    let container = start_open_story(&fixtures_dir()).await;
    container.wait_for_sessions().await;

    let base = container.base_url();

    // List sessions
    let resp = reqwest::get(format!("{base}/api/sessions")).await.unwrap();
    assert_eq!(resp.status(), 200, "list sessions should work");

    let sessions: Vec<Value> = reqwest::get(format!("{base}/api/sessions"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    if let Some(session) = sessions.first() {
        let sid = session["session_id"].as_str().unwrap();

        // Events
        let resp = reqwest::get(format!("{base}/api/sessions/{sid}/events"))
            .await
            .unwrap();
        assert_eq!(resp.status(), 200, "events should work");

        // View records
        let resp = reqwest::get(format!("{base}/api/sessions/{sid}/view-records"))
            .await
            .unwrap();
        assert_eq!(resp.status(), 200, "view-records should work");

        // Summary
        let resp = reqwest::get(format!("{base}/api/sessions/{sid}/summary"))
            .await
            .unwrap();
        assert_eq!(resp.status(), 200, "summary should work");

        // Synopsis
        let resp = reqwest::get(format!("{base}/api/sessions/{sid}/synopsis"))
            .await
            .unwrap();
        assert_eq!(resp.status(), 200, "synopsis should work");

        // Tool journey
        let resp = reqwest::get(format!("{base}/api/sessions/{sid}/tool-journey"))
            .await
            .unwrap();
        assert_eq!(resp.status(), 200, "tool-journey should work");

        // File impact
        let resp = reqwest::get(format!("{base}/api/sessions/{sid}/file-impact"))
            .await
            .unwrap();
        assert_eq!(resp.status(), 200, "file-impact should work");

        // Errors
        let resp = reqwest::get(format!("{base}/api/sessions/{sid}/errors"))
            .await
            .unwrap();
        assert_eq!(resp.status(), 200, "errors should work");

        // Patterns
        let resp = reqwest::get(format!("{base}/api/sessions/{sid}/patterns"))
            .await
            .unwrap();
        assert_eq!(resp.status(), 200, "patterns should work");
    }

    // Plans
    let resp = reqwest::get(format!("{base}/api/plans")).await.unwrap();
    assert_eq!(resp.status(), 200, "plans should work");

    // Agent tools
    let resp = reqwest::get(format!("{base}/api/agent/tools")).await.unwrap();
    assert_eq!(resp.status(), 200, "agent tools should work");

    // Insights
    let resp = reqwest::get(format!("{base}/api/insights/pulse?days=7"))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "pulse should work");

    // Tool schemas
    let resp = reqwest::get(format!("{base}/api/tool-schemas"))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "tool-schemas should work");
}
