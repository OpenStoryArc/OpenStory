//! Stratum 4 — testcontainer end-to-end tests for the boot-time
//! Reconciler. Validates the **production deploy path**: the actual
//! `open-story:test` Docker image, with `--data-dir /data`, finds and
//! reconciles seeded JSONL fixtures on boot.
//!
//! Build the image first:
//!   docker build -t open-story:test ./rs
//!
//! Run with:
//!   cargo test -p open-story --test test_reconcile_container
//!   cargo test -p open-story --test test_reconcile_container --features mongo

mod helpers;

use std::fs;
use std::path::Path;

use helpers::container::start_open_story_with_seeded_data;
use serde_json::{json, Value};
use tempfile::TempDir;

/// Build a CloudEvent-shaped JSONL line. Mirrors the fixture shape used
/// by the reconciler integration tests.
fn ce_line(id: &str, time: &str, host: &str, user: &str) -> String {
    let v = json!({
        "specversion": "1.0",
        "id": id,
        "type": "io.arc.event",
        "subtype": "message.user.prompt",
        "source": format!("arc://transcript/{id}"),
        "time": time,
        "datacontenttype": "application/json",
        "host": host,
        "user": user,
        "data": {
            "agent": "claude-code",
            "agent_payload": { "_variant": "claude-code", "text": "test" },
            "raw": {},
            "seq": 1,
            "session_id": "fixture",
        },
    });
    serde_json::to_string(&v).unwrap()
}

/// Write a JSONL fixture file at `data_dir/<sid>.jsonl` with one line per
/// event. Mirrors how `SessionStore::append` lays out per-session files.
fn write_session(data_dir: &Path, sid: &str, lines: &[String]) {
    fs::create_dir_all(data_dir).unwrap();
    let path = data_dir.join(format!("{sid}.jsonl"));
    let mut content = String::new();
    for line in lines {
        content.push_str(line);
        content.push('\n');
    }
    fs::write(path, content).unwrap();
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

async fn get_sessions_by_host(base_url: &str, host: &str) -> Vec<Value> {
    let url = format!("{}/api/sessions?host={}", base_url, host);
    let body: Value = reqwest::get(url)
        .await
        .expect("HTTP request failed")
        .json()
        .await
        .expect("invalid JSON");
    body.get("sessions")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default()
}

/// Empty `data_dir` → reconciler is a no-op, container boots cleanly,
/// API returns zero sessions. This is the fresh-contributor scenario:
/// "I cloned the repo and ran `docker compose up`" — nothing surprising
/// should happen.
#[tokio::test]
#[ignore = "requires open-story:test image + a running NATS sidecar; \
            see tests/helpers/compose.rs for the full-stack pattern. \
            Stratum 5 manual tests cover this end-to-end on real deploys."]
async fn container_with_empty_data_dir_boots_clean() {
    let tmp = TempDir::new().unwrap();
    let data_dir = tmp.path();
    fs::create_dir_all(data_dir).unwrap();

    let server = start_open_story_with_seeded_data(data_dir).await;

    // The container is up and answering — we do NOT call wait_for_sessions
    // because the empty case has zero sessions, which would time it out.
    let resp = reqwest::get(format!("{}/api/sessions", server.base_url()))
        .await
        .expect("HTTP request failed");
    assert_eq!(resp.status(), 200);

    let sessions = get_sessions(&server.base_url()).await;
    assert!(sessions.is_empty(), "fresh data dir must yield zero sessions");
}

/// Pre-populate `data_dir` with three sessions on different hosts, boot
/// the container, and verify that boot-time reconciliation populates the
/// EventStore. Includes one federated-style session (`Katies-Mac-mini`)
/// — the exact shape that motivated this PR. After boot, the API must
/// surface all three sessions with the correct host stamps and the
/// `?host=` filter must narrow to just the federated one.
#[tokio::test]
#[ignore = "requires open-story:test image + a running NATS sidecar; \
            see tests/helpers/compose.rs for the full-stack pattern. \
            Stratum 5 manual tests cover this end-to-end on real deploys."]
async fn container_reconciles_seeded_jsonl_on_boot() {
    let tmp = TempDir::new().unwrap();
    let data_dir = tmp.path();

    write_session(
        data_dir,
        "sess-katie",
        &[
            ce_line("evt-1", "2026-05-01T10:00:00Z", "Katies-Mac-mini", "katie"),
            ce_line("evt-2", "2026-05-01T10:00:01Z", "Katies-Mac-mini", "katie"),
        ],
    );
    write_session(
        data_dir,
        "sess-max",
        &[ce_line("evt-3", "2026-05-01T11:00:00Z", "Maxs-Air", "max")],
    );
    write_session(
        data_dir,
        "sess-legacy",
        // Legacy unstamped event — no host/user fields at the top level.
        // Use a CloudEvent without the host/user keys. The reconciler must
        // tolerate these and persist them with `host: None, user: None`.
        &[{
            let v = json!({
                "specversion": "1.0",
                "id": "evt-legacy",
                "type": "io.arc.event",
                "subtype": "message.user.prompt",
                "source": "arc://transcript/sess-legacy",
                "time": "2026-04-01T08:00:00Z",
                "datacontenttype": "application/json",
                "data": { "agent": "claude-code", "agent_payload": {"_variant":"claude-code","text":"legacy"}, "raw": {}, "seq": 1, "session_id": "sess-legacy" },
            });
            serde_json::to_string(&v).unwrap()
        }],
    );

    let server = start_open_story_with_seeded_data(data_dir).await;
    server.wait_for_sessions().await;

    let sessions = get_sessions(&server.base_url()).await;
    assert_eq!(sessions.len(), 3, "all three seeded sessions must surface");

    // Each session has the expected host/user stamp.
    let katie = sessions
        .iter()
        .find(|s| s["session_id"].as_str() == Some("sess-katie"))
        .expect("Katie's session must be present");
    assert_eq!(katie["host"].as_str(), Some("Katies-Mac-mini"));
    assert_eq!(katie["user"].as_str(), Some("katie"));
    assert_eq!(katie["event_count"].as_u64(), Some(2));

    let max = sessions
        .iter()
        .find(|s| s["session_id"].as_str() == Some("sess-max"))
        .expect("Max's session must be present");
    assert_eq!(max["host"].as_str(), Some("Maxs-Air"));
    assert_eq!(max["user"].as_str(), Some("max"));

    let legacy = sessions
        .iter()
        .find(|s| s["session_id"].as_str() == Some("sess-legacy"))
        .expect("Legacy unstamped session must be present");
    assert!(
        legacy["host"].is_null() || legacy["host"].as_str().is_none(),
        "legacy session must have host: null"
    );
    assert!(
        legacy["user"].is_null() || legacy["user"].as_str().is_none(),
        "legacy session must have user: null"
    );

    // Filtering — the original bug was: the API returned all sessions but
    // the federated host was missing. Verify the filter actually works
    // and finds exactly Katie's session.
    let filtered = get_sessions_by_host(&server.base_url(), "Katies-Mac-mini").await;
    assert_eq!(filtered.len(), 1, "?host=Katies-Mac-mini should narrow to 1");
    assert_eq!(filtered[0]["session_id"].as_str(), Some("sess-katie"));
}

/// Restart the container twice against the same `data_dir`. Each boot
/// runs the reconciler; subsequent boots should be no-ops (every event
/// already in the EventStore). Validates idempotency in the real deploy
/// path — *not* just in unit tests.
#[tokio::test]
#[ignore = "requires open-story:test image + a running NATS sidecar; \
            see tests/helpers/compose.rs for the full-stack pattern. \
            Stratum 5 manual tests cover this end-to-end on real deploys."]
async fn container_restart_is_idempotent_on_seeded_data() {
    let tmp = TempDir::new().unwrap();
    let data_dir = tmp.path();

    write_session(
        data_dir,
        "sess-restart",
        &[
            ce_line("e1", "2026-05-01T10:00:00Z", "host-A", "user-A"),
            ce_line("e2", "2026-05-01T10:00:01Z", "host-A", "user-A"),
            ce_line("e3", "2026-05-01T10:00:02Z", "host-A", "user-A"),
        ],
    );

    // First boot — reconciler populates the EventStore from JSONL.
    {
        let server = start_open_story_with_seeded_data(data_dir).await;
        server.wait_for_sessions().await;
        let sessions = get_sessions(&server.base_url()).await;
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0]["event_count"].as_u64(), Some(3));
        // server drops here — container teardown.
    }

    // Second boot — same data_dir. Reconciler walks JSONL again. Every
    // event hits PK conflict in the EventStore (which lives at /data
    // alongside the JSONL — same mounted volume). Steady state.
    {
        let server = start_open_story_with_seeded_data(data_dir).await;
        server.wait_for_sessions().await;
        let sessions = get_sessions(&server.base_url()).await;
        assert_eq!(sessions.len(), 1, "session count unchanged after restart");
        assert_eq!(
            sessions[0]["event_count"].as_u64(),
            Some(3),
            "event_count must not duplicate on idempotent re-reconcile"
        );
    }
}
