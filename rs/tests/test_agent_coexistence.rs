//! L6 smoke: Claude + Grok CloudEvents in one watch dir / one DB.
//!
//! Proves agent field is not cross-contaminated when both platforms
//! are present (pre-translated CloudEvent passthrough on /watch).
//!
//! Requires: `docker build -t open-story:test ./rs`
//!
//! Gate: `cargo test -p open-story --test test_agent_coexistence -- --nocapture`

mod helpers;

use helpers::container::start_open_story;
use serde_json::{json, Value};
use std::path::PathBuf;

fn write_cloud_event_session(dir: &std::path::Path, file_stem: &str, agent: &str, text: &str) {
    let path = dir.join(format!("{file_stem}.jsonl"));
    let events = [
        json!({
            "specversion": "1.0",
            "id": format!("{file_stem}-user"),
            "source": format!("test://{file_stem}"),
            "type": "io.arc.event",
            "time": "2026-07-17T12:00:00.000Z",
            "datacontenttype": "application/json",
            "subtype": "message.user.prompt",
            "agent": agent,
            "data": {
                "seq": 1,
                "session_id": file_stem,
                "raw": {},
                "agent_payload": {
                    "_variant": if agent == "grok-build" { "grok-build" } else { "claude-code" },
                    "meta": { "agent": agent },
                    "text": text
                }
            }
        }),
        json!({
            "specversion": "1.0",
            "id": format!("{file_stem}-asst"),
            "source": format!("test://{file_stem}"),
            "type": "io.arc.event",
            "time": "2026-07-17T12:00:01.000Z",
            "datacontenttype": "application/json",
            "subtype": "message.assistant.text",
            "agent": agent,
            "data": {
                "seq": 2,
                "session_id": file_stem,
                "raw": {},
                "agent_payload": {
                    "_variant": if agent == "grok-build" { "grok-build" } else { "claude-code" },
                    "meta": { "agent": agent },
                    "text": format!("Reply from {agent}: acknowledged."),
                    "model": if agent == "grok-build" { "grok-4.5" } else { "claude-sonnet-4" }
                }
            }
        }),
        json!({
            "specversion": "1.0",
            "id": format!("{file_stem}-turn"),
            "source": format!("test://{file_stem}"),
            "type": "io.arc.event",
            "time": "2026-07-17T12:00:02.000Z",
            "datacontenttype": "application/json",
            "subtype": "system.turn.complete",
            "agent": agent,
            "data": {
                "seq": 3,
                "session_id": file_stem,
                "raw": {},
                "agent_payload": {
                    "_variant": if agent == "grok-build" { "grok-build" } else { "claude-code" },
                    "meta": { "agent": agent },
                    "stop_reason": "end_turn"
                }
            }
        }),
    ];
    let mut body = String::new();
    for e in &events {
        body.push_str(&serde_json::to_string(e).unwrap());
        body.push('\n');
    }
    std::fs::write(&path, body).expect("write session jsonl");
    let now = filetime::FileTime::now();
    let _ = filetime::set_file_mtime(&path, now);
}

fn coexistence_fixture_dir() -> PathBuf {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    write_cloud_event_session(
        tmp.path(),
        "claude-coexist",
        "claude-code",
        "Hello from Claude coexistence fixture",
    );
    write_cloud_event_session(
        tmp.path(),
        "grok-coexist",
        "grok-build",
        "Hello from Grok coexistence fixture",
    );
    let path = tmp.path().to_path_buf();
    std::mem::forget(tmp);
    path
}

async fn get_sessions(base_url: &str) -> Vec<Value> {
    let body: Value = reqwest::get(format!("{}/api/sessions", base_url))
        .await
        .expect("sessions")
        .json()
        .await
        .expect("json");
    body.get("sessions")
        .and_then(|v| v.as_array())
        .or_else(|| body.as_array())
        .cloned()
        .unwrap_or_default()
}

/// describe("when Claude and Grok CloudEvents share one watch dir")
/// it("should list both sessions with distinct origin_agent values")
#[tokio::test]
async fn container_lists_claude_and_grok_without_agent_cross_contamination() {
    let fixture = coexistence_fixture_dir();
    let server = start_open_story(&fixture).await;
    server.wait_for_sessions().await;

    let sessions = get_sessions(&server.base_url()).await;
    assert!(
        sessions.len() >= 2,
        "expected ≥2 sessions (claude + grok), got {sessions:?}"
    );

    let claude = sessions.iter().find(|s| {
        s.get("session_id")
            .and_then(|v| v.as_str())
            .is_some_and(|id| id.contains("claude-coexist"))
    });
    let grok = sessions.iter().find(|s| {
        s.get("session_id")
            .and_then(|v| v.as_str())
            .is_some_and(|id| id.contains("grok-coexist"))
    });
    assert!(claude.is_some(), "missing claude-coexist session: {sessions:?}");
    assert!(grok.is_some(), "missing grok-coexist session: {sessions:?}");

    let claude_agent = claude
        .unwrap()
        .get("origin_agent")
        .and_then(|v| v.as_str());
    let grok_agent = grok.unwrap().get("origin_agent").and_then(|v| v.as_str());

    assert_eq!(
        claude_agent,
        Some("claude-code"),
        "Claude session origin_agent wrong: {claude:?}"
    );
    assert_eq!(
        grok_agent,
        Some("grok-build"),
        "Grok session origin_agent wrong: {grok:?}"
    );
    assert_ne!(
        claude_agent, grok_agent,
        "agent fields must not collapse to a single value"
    );

    // Records for Grok stay labeled grok-build
    let sid = grok
        .unwrap()
        .get("session_id")
        .and_then(|v| v.as_str())
        .unwrap();
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
        .or_else(|| records.get("records").and_then(|r| r.as_array()).cloned())
        .unwrap_or_default();
    assert!(!recs.is_empty(), "grok session should have records");
    for r in &recs {
        let oa = r.get("origin_agent").and_then(|v| v.as_str());
        assert_eq!(
            oa,
            Some("grok-build"),
            "Grok record leaked foreign origin_agent: {r}"
        );
    }

    let csid = claude
        .unwrap()
        .get("session_id")
        .and_then(|v| v.as_str())
        .unwrap();
    let crecords: Value = reqwest::get(format!(
        "{}/api/sessions/{csid}/records",
        server.base_url()
    ))
    .await
    .expect("claude records")
    .json()
    .await
    .expect("claude records json");
    let crecs = crecords
        .as_array()
        .cloned()
        .or_else(|| crecords.get("records").and_then(|r| r.as_array()).cloned())
        .unwrap_or_default();
    for r in &crecs {
        let oa = r.get("origin_agent").and_then(|v| v.as_str());
        assert_eq!(
            oa,
            Some("claude-code"),
            "Claude record leaked foreign origin_agent: {r}"
        );
    }
}
