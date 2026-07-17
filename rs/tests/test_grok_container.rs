//! Container e2e for Grok Build seed tree (eventually).
//!
//! Mirrors `test_container.rs` / Codex live-watch intent: mount a
//! `~/.grok/sessions`-shaped tree and assert the server discovers
//! `origin_agent: grok-build` via the watcher.
//!
//! ## Status
//!
//! - **Unit/integration (always on):** seed fixtures + storytelling +
//!   live-watch + goldens cover the Grok path without Docker.
//! - **Container (opt-in):** this file is `#[ignore]` until the image is
//!   built with Grok watch-dir support wired the same way Claude fixtures
//!   mount at `/watch`. Tracked as follow-up when promoting Grok to
//!   first-class in `just test-docker`.
//!
//! ## Prerequisites (when un-ignored)
//!
//! ```bash
//! docker build -t open-story:test ./rs
//! cargo test -p open-story --test test_grok_container -- --ignored --nocapture
//! ```
//!
//! The seed tree is produced by:
//!
//! ```bash
//! python3 scripts/extract_grok_session_seed.py
//! # → rs/tests/fixtures/grok/seed_tree/{urlencoded-cwd}/{session}/updates.jsonl
//! ```

mod helpers;

use helpers::container::start_open_story;
use helpers::fixtures_dir;
use serde_json::Value;
use std::path::PathBuf;

fn grok_seed_tree() -> PathBuf {
    fixtures_dir().join("grok").join("seed_tree")
}

async fn get_sessions(base_url: &str) -> Vec<Value> {
    let body: Value = reqwest::get(format!("{}/api/sessions", base_url))
        .await
        .expect("HTTP request failed")
        .json()
        .await
        .expect("invalid JSON");
    body.get("sessions")
        .and_then(|v| v.as_array())
        .or_else(|| body.as_array())
        .cloned()
        .unwrap_or_default()
}

/// Seed tree exists and has the Grok path layout (always runs — no Docker).
#[test]
fn grok_seed_tree_layout_is_present() {
    let tree = grok_seed_tree();
    assert!(
        tree.is_dir(),
        "missing {} — run: python3 scripts/extract_grok_session_seed.py",
        tree.display()
    );
    let updates: Vec<_> = walkdir::WalkDir::new(&tree)
        .into_iter()
        .filter_map(|e| e.ok())
        .map(|e| e.into_path())
        .filter(|p| p.file_name().and_then(|s| s.to_str()) == Some("updates.jsonl"))
        .collect();
    assert!(
        !updates.is_empty(),
        "seed_tree must contain at least one updates.jsonl"
    );
    // Parent of updates.jsonl must look like a session uuid
    let session_dir = updates[0].parent().unwrap();
    let session_id = session_dir.file_name().unwrap().to_string_lossy();
    assert_eq!(
        session_id.len(),
        36,
        "session dir should be a uuid, got {session_id}"
    );
    // Noise sibling present (watcher must ignore it)
    assert!(
        session_dir.join("chat_history.jsonl").exists(),
        "seed should include chat_history.jsonl noise sibling"
    );
}

/// Container: watch seed_tree, expect a grok-build session on the REST API.
///
/// Ignored until Docker image is built and the container entrypoint watches
/// recursive session trees the same way production watches `~/.grok/sessions`.
#[tokio::test]
#[ignore = "requires docker image open-story:test; run: cargo test -p open-story --test test_grok_container -- --ignored"]
async fn container_loads_grok_seed_tree_as_grok_build_session() {
    let tree = grok_seed_tree();
    assert!(
        tree.is_dir(),
        "run python3 scripts/extract_grok_session_seed.py first"
    );

    // Touch mtimes so backfill window includes the seed.
    let now = filetime::FileTime::now();
    for entry in walkdir::WalkDir::new(&tree)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let _ = filetime::set_file_mtime(entry.path(), now);
    }

    let server = start_open_story(&tree).await;
    server.wait_for_sessions().await;

    let sessions = get_sessions(&server.base_url()).await;
    assert!(
        !sessions.is_empty(),
        "expected ≥1 session from grok seed_tree"
    );

    let grok = sessions.iter().find(|s| {
        s.get("origin_agent").and_then(|v| v.as_str()) == Some("grok-build")
            || s.get("session_id")
                .and_then(|v| v.as_str())
                .is_some_and(|id| id.starts_with("019f6cb5"))
    });
    assert!(
        grok.is_some(),
        "expected a grok-build session, got: {sessions:?}"
    );

    let sid = grok
        .unwrap()
        .get("session_id")
        .and_then(|v| v.as_str())
        .expect("session_id");

    // Records + at least one sentence pattern
    let records: Value = reqwest::get(format!(
        "{}/api/sessions/{sid}/records",
        server.base_url()
    ))
    .await
    .expect("records")
    .json()
    .await
    .expect("records json");
    let n = records
        .as_array()
        .map(|a| a.len())
        .or_else(|| records.get("records").and_then(|r| r.as_array()).map(|a| a.len()))
        .unwrap_or(0);
    assert!(n > 0, "expected records for {sid}");

    let patterns: Value = reqwest::get(format!(
        "{}/api/sessions/{sid}/patterns?type=turn.sentence",
        server.base_url()
    ))
    .await
    .expect("patterns")
    .json()
    .await
    .expect("patterns json");
    let sentences = patterns
        .get("patterns")
        .and_then(|p| p.as_array())
        .cloned()
        .unwrap_or_default();
    assert!(
        !sentences.is_empty(),
        "expected turn.sentence patterns for real seed session"
    );
    let summary = sentences[0]
        .get("summary")
        .and_then(|s| s.as_str())
        .unwrap_or("");
    assert!(
        summary.starts_with("Grok"),
        "sentence should start with Grok, got: {summary}"
    );
}
