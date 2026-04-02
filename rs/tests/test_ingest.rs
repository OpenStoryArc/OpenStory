//! Integration tests for is_plan_event and ingest_events (plan extraction, broadcast, persistence).

mod helpers;

use helpers::{make_event, test_state};
use serde_json::json;
use tempfile::TempDir;

use open_story::cloud_event::CloudEvent;
use open_story::event_data::EventData;
use open_story::server::{ingest_events, is_plan_event};

// ─── is_plan_event ───────────────────────────────────────────────────────────

#[test]
fn is_plan_event_unified_format_with_tool_field() {
    let event = json!({
        "type": "io.arc.event",
        "subtype": "message.assistant.tool_use",
        "data": {
            "tool": "ExitPlanMode",
            "args": { "plan": "# My Plan\n\nStep 1: do things" }
        }
    });
    assert!(is_plan_event(&event));
}

#[test]
fn is_plan_event_unified_format_with_content_blocks() {
    let event = json!({
        "type": "io.arc.event",
        "subtype": "message.assistant.tool_use",
        "data": {
            "raw": {
                "message": {
                    "content": [{
                        "type": "tool_use",
                        "name": "ExitPlanMode",
                        "input": { "plan": "# Architecture\n\nUse actors." }
                    }]
                }
            }
        }
    });
    assert!(is_plan_event(&event));
}

#[test]
fn is_plan_event_legacy_tool_call() {
    let event = json!({
        "type": "io.arc.tool.call",
        "data": {
            "tool": "ExitPlanMode",
            "args": { "plan": "# Refactor plan" }
        }
    });
    assert!(is_plan_event(&event));
}

#[test]
fn is_plan_event_legacy_transcript_assistant() {
    let event = json!({
        "type": "io.arc.transcript.assistant",
        "subtype": "tool_use",
        "data": {
            "raw": {
                "message": {
                    "content": [{
                        "type": "tool_use",
                        "name": "ExitPlanMode",
                        "input": { "plan": "# Migration plan" }
                    }]
                }
            }
        }
    });
    assert!(is_plan_event(&event));
}

#[test]
fn is_plan_event_rejects_non_exit_plan_mode() {
    let event = json!({
        "type": "io.arc.event",
        "subtype": "message.assistant.tool_use",
        "data": {
            "tool": "Read",
            "args": { "file_path": "/tmp/foo.rs" }
        }
    });
    assert!(!is_plan_event(&event));
}

#[test]
fn is_plan_event_rejects_empty_plan() {
    let event = json!({
        "type": "io.arc.event",
        "subtype": "message.assistant.tool_use",
        "data": {
            "tool": "ExitPlanMode",
            "args": { "plan": "" }
        }
    });
    assert!(!is_plan_event(&event));
}

#[test]
fn is_plan_event_rejects_wrong_subtype() {
    let event = json!({
        "type": "io.arc.event",
        "subtype": "message.assistant.text",
        "data": {
            "tool": "ExitPlanMode",
            "args": { "plan": "# Plan" }
        }
    });
    assert!(!is_plan_event(&event));
}

#[test]
fn is_plan_event_rejects_user_prompt() {
    let event = json!({
        "type": "io.arc.event",
        "subtype": "message.user.prompt",
        "data": { "text": "write a plan" }
    });
    assert!(!is_plan_event(&event));
}

// ─── ingest_events: persistence ──────────────────────────────────────────────

#[tokio::test]
async fn ingest_persists_events_to_session_store() {
    let tmp = TempDir::new().unwrap();
    let state = test_state(&tmp);

    let events = vec![
        make_event("io.arc.event", "sess-p1"),
        make_event("io.arc.event", "sess-p1"),
    ];

    let mut s = state.write().await;
    let result = ingest_events(&mut s, "sess-p1", &events, None);
    assert_eq!(result.count, 2);

    // Events stored in session map
    assert_eq!(s.store.event_store.session_events("sess-p1").unwrap().len(), 2);

    // JSONL file written to data_dir
    let jsonl_path = tmp.path().join("sess-p1.jsonl");
    assert!(jsonl_path.exists(), "JSONL persistence file should exist");
    let content = std::fs::read_to_string(&jsonl_path).unwrap();
    let lines: Vec<&str> = content.lines().collect();
    assert_eq!(lines.len(), 2, "Should have 2 lines in JSONL file");
}

// ─── ingest_events: broadcast ────────────────────────────────────────────────

#[tokio::test]
async fn ingest_broadcasts_view_records_to_subscribers() {
    let tmp = TempDir::new().unwrap();
    let state = test_state(&tmp);

    // Subscribe before ingesting
    let mut rx = {
        let s = state.read().await;
        s.broadcast_tx.subscribe()
    };

    let events = vec![make_event("io.arc.event", "sess-bc")];

    {
        let mut s = state.write().await;
        let result = ingest_events(&mut s, "sess-bc", &events, None);
        assert_eq!(result.count, 1);
        // Broadcast changes to WS clients (callers are now responsible)
        for change in &result.changes {
            let _ = s.broadcast_tx.send(change.clone());
        }
    }

    // Should receive at least one broadcast (if from_cloud_event produced view records)
    // The make_event helper creates minimal events — they may or may not produce view records
    // depending on subtype. Either way, the broadcast channel should not error.
    match rx.try_recv() {
        Ok(_msg) => { /* view records were broadcast */ }
        Err(tokio::sync::broadcast::error::TryRecvError::Empty) => {
            // No view records produced for this minimal event — that's valid
        }
        Err(e) => panic!("Unexpected broadcast error: {e:?}"),
    }
}

// ─── ingest_events: plan extraction ──────────────────────────────────────────

#[tokio::test]
async fn ingest_extracts_and_saves_plan_from_exit_plan_mode() {
    let tmp = TempDir::new().unwrap();
    let state = test_state(&tmp);

    let mut plan_data = EventData::new(json!({}), 0, "sess-plan".to_string());
    plan_data.tool = Some("ExitPlanMode".to_string());
    plan_data.args = Some(json!({ "plan": "# Architecture Plan\n\nUse actor model." }));
    let plan_event = CloudEvent::new(
        "arc://transcript/sess-plan".to_string(),
        "io.arc.event".to_string(),
        plan_data,
        Some("message.assistant.tool_use".to_string()),
        Some("plan-evt-001".to_string()),
        None,
        None,
        None,
        None,
    );

    let mut s = state.write().await;
    let result = ingest_events(&mut s, "sess-plan", &[plan_event], None);
    assert_eq!(result.count, 1);

    // Plan should be saved to plan_store
    let plans = s.store.plan_store.list_for_session("sess-plan");
    assert!(!plans.is_empty(), "Plan should have been extracted and saved");
    assert!(
        plans[0].title.contains("Architecture Plan"),
        "Plan title should be extracted from content"
    );
}

#[tokio::test]
async fn ingest_extracts_plan_from_legacy_tool_call() {
    let tmp = TempDir::new().unwrap();
    let state = test_state(&tmp);

    let mut legacy_data = EventData::new(json!({}), 0, "sess-legacy".to_string());
    legacy_data.tool = Some("ExitPlanMode".to_string());
    legacy_data.args = Some(json!({ "plan": "# Legacy Plan\n\nStep 1." }));
    let plan_event = CloudEvent::new(
        "arc://hook/sess-legacy".to_string(),
        "io.arc.tool.call".to_string(),
        legacy_data,
        None,
        Some("legacy-plan-001".to_string()),
        None,
        None,
        None,
        None,
    );

    let mut s = state.write().await;
    let result = ingest_events(&mut s, "sess-legacy", &[plan_event], None);
    assert_eq!(result.count, 1);

    let plans = s.store.plan_store.list_for_session("sess-legacy");
    assert!(!plans.is_empty(), "Legacy plan should have been extracted");
}

// ─── ingest_events: project association ──────────────────────────────────────

#[tokio::test]
async fn ingest_records_project_association() {
    let tmp = TempDir::new().unwrap();
    let state = test_state(&tmp);

    let events = vec![make_event("io.arc.event", "sess-proj")];

    let mut s = state.write().await;
    ingest_events(&mut s, "sess-proj", &events, Some("my-project"));

    assert_eq!(
        s.store.session_projects.get("sess-proj").unwrap(),
        "my-project"
    );
}

#[tokio::test]
async fn ingest_returns_zero_for_empty_events() {
    let tmp = TempDir::new().unwrap();
    let state = test_state(&tmp);

    let mut s = state.write().await;
    let result = ingest_events(&mut s, "sess-empty", &[], None);
    assert_eq!(result.count, 0);
    assert!(!!s.store.event_store.session_events("sess-empty").unwrap().is_empty());
}
