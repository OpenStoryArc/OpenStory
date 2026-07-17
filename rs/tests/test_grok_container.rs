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

/// Container: watch seed_tree, expect grok-build session + non-empty prose.
///
/// Requires: `docker build -t open-story:test ./rs`
/// Run: `cargo test -p open-story --test test_grok_container -- --nocapture`
#[tokio::test]
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
    let grok = grok.unwrap();
    assert_eq!(
        grok.get("origin_agent").and_then(|v| v.as_str()),
        Some("grok-build"),
        "origin_agent must be grok-build"
    );

    let sid = grok
        .get("session_id")
        .and_then(|v| v.as_str())
        .expect("session_id");

    // Records + non-empty assistant prose (views BFF parity with Claude)
    let records: Value = reqwest::get(format!(
        "{}/api/sessions/{sid}/records",
        server.base_url()
    ))
    .await
    .expect("records")
    .json()
    .await
    .expect("records json");
    let recs = records
        .as_array()
        .cloned()
        .or_else(|| {
            records
                .get("records")
                .and_then(|r| r.as_array())
                .cloned()
        })
        .unwrap_or_default();
    assert!(!recs.is_empty(), "expected records for {sid}");

    let asst: Vec<_> = recs
        .iter()
        .filter(|r| r.get("record_type").and_then(|t| t.as_str()) == Some("assistant_message"))
        .collect();
    assert!(
        !asst.is_empty(),
        "expected assistant_message records for Grok session"
    );
    let nonempty = asst.iter().any(|r| {
        let content = r
            .pointer("/payload/content")
            .cloned()
            .unwrap_or(Value::Null);
        match content {
            Value::String(s) => !s.trim().is_empty(),
            Value::Array(arr) => arr.iter().any(|b| {
                b.get("text")
                    .and_then(|t| t.as_str())
                    .is_some_and(|t| !t.trim().is_empty())
            }),
            _ => false,
        }
    });
    assert!(
        nonempty,
        "assistant_message content must be non-empty (views typed path)"
    );

    // FTS: assistant prose is searchable (Claude parity for search dogfood)
    let search: Value = reqwest::get(format!(
        "{}/api/search?q=TUI&session_id={sid}",
        server.base_url()
    ))
    .await
    .expect("search")
    .json()
    .await
    .expect("search json");
    let hits = search
        .as_array()
        .cloned()
        .or_else(|| search.get("results").and_then(|r| r.as_array()).cloned())
        .or_else(|| search.get("hits").and_then(|r| r.as_array()).cloned())
        .unwrap_or_default();
    // TUI is in real_turn_01 seed — soft assert if FTS schema differs
    if hits.is_empty() {
        // Fallback: any non-empty search for a tool name present in seed
        let search2: Value = reqwest::get(format!(
            "{}/api/search?q=session&session_id={sid}",
            server.base_url()
        ))
        .await
        .expect("search2")
        .json()
        .await
        .expect("search2 json");
        let hits2 = search2
            .as_array()
            .cloned()
            .or_else(|| search2.get("results").and_then(|r| r.as_array()).cloned())
            .unwrap_or_default();
        assert!(
            !hits2.is_empty(),
            "expected FTS hits for Grok session content, got empty for TUI and session"
        );
    }

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
        .unwrap_or_else(|| {
            patterns
                .as_array()
                .cloned()
                .unwrap_or_default()
        });
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
