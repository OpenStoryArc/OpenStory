//! Integration test: pattern detection wired into ingest_events.
//!
//! Verifies that patterns are detected during ingestion and included
//! in broadcast messages and initial_state.

mod helpers;

use serde_json::json;
use tempfile::TempDir;

use open_story::cloud_event::CloudEvent;
use open_story::event_data::{AgentPayload, ClaudeCodePayload, EventData};
use open_story::server::{ingest_events, BroadcastMessage};
use open_story::server::ws::build_initial_state;

use helpers::{test_state, send_request, body_json};
use open_story::server::replay_boot_sessions;

use axum::body::Body;
use axum::http::Request;

/// Create a CloudEvent for a Bash tool_use with a specific command.
fn bash_tool_use(session_id: &str, id: &str, command: &str) -> CloudEvent {
    let mut payload = ClaudeCodePayload::new();
    payload.tool = Some("Bash".to_string());
    payload.args = Some(json!({"command": command}));
    let data = EventData::with_payload(
        json!({
            "type": "assistant",
            "message": {
                "model": "claude-4",
                "content": [{
                    "type": "tool_use",
                    "id": format!("toolu_{id}"),
                    "name": "Bash",
                    "input": {"command": command}
                }]
            }
        }),
        1,
        session_id.to_string(),
        AgentPayload::ClaudeCode(payload),
    );
    CloudEvent::new(
        format!("arc://transcript/{session_id}"),
        "io.arc.event".to_string(),
        data,
        Some("message.assistant.tool_use".to_string()),
        Some(id.to_string()),
        None,
        None,
        None,
        None,
    )
}

/// Create a tool_result CloudEvent.
fn tool_result_event(session_id: &str, id: &str, call_id: &str, output: &str) -> CloudEvent {
    let data = EventData::with_payload(
        json!({
            "type": "user",
            "message": {
                "content": [{
                    "type": "tool_result",
                    "tool_use_id": call_id,
                    "content": output
                }]
            }
        }),
        2,
        session_id.to_string(),
        AgentPayload::ClaudeCode(ClaudeCodePayload::new()),
    );
    CloudEvent::new(
        format!("arc://transcript/{session_id}"),
        "io.arc.event".to_string(),
        data,
        Some("message.user.tool_result".to_string()),
        Some(id.to_string()),
        None,
        None,
        None,
        None,
    )
}

/// Create a user_message CloudEvent.
fn user_prompt(session_id: &str, id: &str) -> CloudEvent {
    let mut payload = ClaudeCodePayload::new();
    payload.text = Some("next task".to_string());
    let data = EventData::with_payload(
        json!({
            "type": "user",
            "message": {"content": [{"type": "text", "text": "next task"}]}
        }),
        3,
        session_id.to_string(),
        AgentPayload::ClaudeCode(payload),
    );
    CloudEvent::new(
        format!("arc://transcript/{session_id}"),
        "io.arc.event".to_string(),
        data,
        Some("message.user.prompt".to_string()),
        Some(id.to_string()),
        None,
        None,
        None,
        None,
    )
}

// ═══════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════

#[test]
fn it_should_detect_git_workflow_during_ingest() {
    let tmp = TempDir::new().unwrap();
    let state = test_state(&tmp);

    // Sequence of git commands that should trigger GitFlowDetector
    let events = vec![
        bash_tool_use("sess-1", "e1", "git status"),
        bash_tool_use("sess-1", "e2", "git add -A"),
        bash_tool_use("sess-1", "e3", "git commit -m 'fix'"),
        // Non-git event triggers the workflow emission
        user_prompt("sess-1", "e4"),
    ];

    let mut s = state.blocking_write();
    let result = ingest_events(&mut s, "sess-1", &events, None);
    drop(s);

    // Check returned changes for pattern detection
    let mut found_pattern = false;
    for msg in &result.changes {
        if let BroadcastMessage::Enriched { patterns, .. } = msg {
            if patterns.iter().any(|p| p.pattern_type == "git.workflow") {
                found_pattern = true;
            }
        }
    }
    assert!(found_pattern, "should detect git.workflow pattern during ingest");
}

#[test]
fn it_should_detect_test_cycle_during_ingest() {
    let tmp = TempDir::new().unwrap();
    let state = test_state(&tmp);

    let events = vec![
        bash_tool_use("sess-1", "e1", "cargo test"),
        tool_result_event("sess-1", "e2", "toolu_e1", "FAILED 3 tests"),
        // An Edit tool call
        {
            let mut edit_payload = ClaudeCodePayload::new();
            edit_payload.tool = Some("Edit".to_string());
            edit_payload.args = Some(json!({"file_path": "/fix.rs", "old_string": "a", "new_string": "b"}));
            let edit_data = EventData::with_payload(
                json!({
                    "type": "assistant",
                    "message": {
                        "model": "claude-4",
                        "content": [{
                            "type": "tool_use",
                            "id": "toolu_edit",
                            "name": "Edit",
                            "input": {"file_path": "/fix.rs", "old_string": "a", "new_string": "b"}
                        }]
                    }
                }),
                3,
                "sess-1".to_string(),
                AgentPayload::ClaudeCode(edit_payload),
            );
            CloudEvent::new(
                "arc://transcript/sess-1".into(),
                "io.arc.event".to_string(),
                edit_data,
                Some("message.assistant.tool_use".to_string()),
                Some("e3".to_string()),
                None, None, None, None,
            )
        },
        bash_tool_use("sess-1", "e4", "cargo test"),
        tool_result_event("sess-1", "e5", "toolu_e4", "test result: ok. 54 passed"),
    ];

    let mut s = state.blocking_write();
    let result = ingest_events(&mut s, "sess-1", &events, None);
    drop(s);

    let mut found_cycle = false;
    for msg in &result.changes {
        if let BroadcastMessage::Enriched { patterns, .. } = msg {
            if patterns.iter().any(|p| p.pattern_type == "test.cycle") {
                found_cycle = true;
            }
        }
    }
    assert!(found_cycle, "should detect test.cycle pattern during ingest");
}

#[test]
fn it_should_include_patterns_in_initial_state() {
    let tmp = TempDir::new().unwrap();
    let state = test_state(&tmp);

    // Ingest a git workflow
    let events = vec![
        bash_tool_use("sess-1", "e1", "git status"),
        bash_tool_use("sess-1", "e2", "git push"),
        user_prompt("sess-1", "e3"), // triggers git.workflow emit
    ];

    {
        let mut s = state.blocking_write();
        ingest_events(&mut s, "sess-1", &events, None);
    }

    // Build initial_state — should include detected patterns
    let s = state.blocking_read();
    let patterns = build_initial_state(&s).patterns;
    assert!(
        patterns.iter().any(|p| p.pattern_type == "git.workflow"),
        "initial_state should include detected patterns"
    );
}

// ═══════════════════════════════════════════════════════════════════
// Boot replay: projections + patterns from pre-loaded sessions
// ═══════════════════════════════════════════════════════════════════

#[test]
fn it_should_replay_boot_sessions_through_projections() {
    let tmp = TempDir::new().unwrap();
    let state = test_state(&tmp);

    // Simulate boot-loaded session (raw events in state.sessions)
    {
        let mut s = state.blocking_write();
        let events = vec![
            bash_tool_use("sess-boot", "e1", "git status"),
            bash_tool_use("sess-boot", "e2", "git add ."),
            bash_tool_use("sess-boot", "e3", "git commit -m 'done'"),
            user_prompt("sess-boot", "e4"),
        ];
        // Insert as raw Values into EventStore (simulating what boot does)
        let values: Vec<serde_json::Value> = events
            .iter()
            .map(|ce| serde_json::to_value(ce).unwrap())
            .collect();
        let _ = s.store.event_store.insert_batch("sess-boot", &values);
        let _ = s.store.event_store.upsert_session(&open_story_store::event_store::SessionRow {
            id: "sess-boot".into(), project_id: None, project_name: None,
            label: None, branch: None, event_count: values.len() as u64,
                custom_label: None,
            first_event: None, last_event: None,
        });
    }

    // Before replay: projections should be empty
    {
        let s = state.blocking_read();
        assert!(s.store.projections.is_empty(), "projections empty before replay");
    }

    // Run boot replay
    {
        let mut s = state.blocking_write();
        replay_boot_sessions(&mut s);
    }

    // After replay: projections and patterns should be populated
    let s = state.blocking_read();
    assert!(
        s.store.projections.contains_key("sess-boot"),
        "projection should exist after replay"
    );
    let proj = s.store.projections.get("sess-boot").unwrap();
    assert!(
        proj.timeline_rows().len() >= 4,
        "projection should have timeline rows after replay"
    );

    // Git workflow pattern should be detected
    let patterns = s.store.detected_patterns.get("sess-boot").unwrap_or(&Vec::new()).clone();
    assert!(
        patterns.iter().any(|p| p.pattern_type == "git.workflow"),
        "boot replay should detect git.workflow pattern"
    );
}

#[test]
fn it_should_replay_populates_initial_state() {
    let tmp = TempDir::new().unwrap();
    let state = test_state(&tmp);

    {
        let mut s = state.blocking_write();
        let events = vec![
            user_prompt("sess-boot", "e1"),
            bash_tool_use("sess-boot", "e2", "ls -la"),
        ];
        let values: Vec<serde_json::Value> = events
            .iter()
            .map(|ce| serde_json::to_value(ce).unwrap())
            .collect();
        let _ = s.store.event_store.insert_batch("sess-boot", &values);
        let _ = s.store.event_store.upsert_session(&open_story_store::event_store::SessionRow {
            id: "sess-boot".into(), project_id: None, project_name: None,
            label: None, branch: None, event_count: values.len() as u64,
                custom_label: None,
            first_event: None, last_event: None,
        });
        replay_boot_sessions(&mut s);
    }

    let s = state.blocking_read();
    let init = build_initial_state(&s);
    let records = init.records;
    let filter_counts = init.filter_counts;

    assert!(!records.is_empty(), "initial_state should have records after replay");
    assert!(
        filter_counts.contains_key("sess-boot"),
        "filter_counts should have session after replay"
    );
}

// ═══════════════════════════════════════════════════════════════════
// Pattern API endpoint
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn it_should_return_patterns_via_api() {
    let tmp = TempDir::new().unwrap();
    let state = test_state(&tmp);

    // Ingest events that produce a git.workflow pattern
    {
        let mut s = state.write().await;
        let events = vec![
            bash_tool_use("sess-1", "e1", "git status"),
            bash_tool_use("sess-1", "e2", "git push"),
            user_prompt("sess-1", "e3"),
        ];
        ingest_events(&mut s, "sess-1", &events, None);
    }

    let req = Request::get("/api/sessions/sess-1/patterns")
        .body(Body::empty())
        .unwrap();

    let resp = send_request(state, req).await;
    assert_eq!(resp.status(), 200);

    let body = body_json(resp).await;
    let patterns = body.get("patterns").and_then(|v| v.as_array()).unwrap();
    assert!(
        patterns.iter().any(|p| p.get("pattern_type").and_then(|v| v.as_str()) == Some("git.workflow")),
        "API should return detected patterns"
    );
}

#[tokio::test]
async fn it_should_filter_patterns_by_type() {
    let tmp = TempDir::new().unwrap();
    let state = test_state(&tmp);

    {
        let mut s = state.write().await;
        let events = vec![
            bash_tool_use("sess-1", "e1", "git status"),
            bash_tool_use("sess-1", "e2", "git push"),
            user_prompt("sess-1", "e3"),
        ];
        ingest_events(&mut s, "sess-1", &events, None);
    }

    // Filter for git.workflow — should return results
    let req = Request::get("/api/sessions/sess-1/patterns?type=git.workflow")
        .body(Body::empty())
        .unwrap();
    let resp = send_request(state.clone(), req).await;
    let body = body_json(resp).await;
    let patterns = body.get("patterns").and_then(|v| v.as_array()).unwrap();
    assert!(!patterns.is_empty(), "should have git.workflow patterns");

    // Filter for non-existent type — should return empty
    let req = Request::get("/api/sessions/sess-1/patterns?type=nonexistent")
        .body(Body::empty())
        .unwrap();
    let resp = send_request(state, req).await;
    let body = body_json(resp).await;
    let patterns = body.get("patterns").and_then(|v| v.as_array()).unwrap();
    assert!(patterns.is_empty(), "should have no nonexistent patterns");
}

#[tokio::test]
async fn it_should_return_empty_patterns_for_unknown_session() {
    let tmp = TempDir::new().unwrap();
    let state = test_state(&tmp);

    let req = Request::get("/api/sessions/unknown/patterns")
        .body(Body::empty())
        .unwrap();
    let resp = send_request(state, req).await;
    assert_eq!(resp.status(), 200);

    let body = body_json(resp).await;
    let patterns = body.get("patterns").and_then(|v| v.as_array()).unwrap();
    assert!(patterns.is_empty());
}
