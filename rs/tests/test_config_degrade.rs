//! Graceful degradation tests — verify behavior when components are missing.
//!
//! Tests every trait boundary's fallback: what happens when Qdrant, NATS,
//! or auth tokens are absent?
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

// ── SemanticStore boundary ────────────────────────────────────────────

/// Server without Qdrant: search returns 503, not 500.
#[tokio::test]
#[ignore]
async fn degrade_no_qdrant_search_503() {
    // Minimal config — no Qdrant
    let container = start_open_story(&fixtures_dir()).await;

    let resp = reqwest::get(format!("{}/api/search?q=test", container.base_url()))
        .await
        .unwrap();

    assert_eq!(
        resp.status(),
        503,
        "search without Qdrant should return 503, not 500"
    );

    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["error"], "semantic search not available");
}

/// Server without Qdrant: agent search also returns 503.
#[tokio::test]
#[ignore]
async fn degrade_no_qdrant_agent_search_503() {
    let container = start_open_story(&fixtures_dir()).await;

    let resp = reqwest::get(format!(
        "{}/api/agent/search?q=test",
        container.base_url()
    ))
    .await
    .unwrap();

    assert_eq!(resp.status(), 503);
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

// ── All non-search APIs work without Qdrant ───────────────────────────

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
