//! Grok ACP events compose through reader → CloudEvent → projection → ViewRecord.
//!
//! Mirrors `test_codex_projection.rs`.

use open_story::reader::read_new_lines;
use open_story::translate::{TranscriptFormat, TranscriptState};
use open_story_store::projection::SessionProjection;
use open_story_views::unified::RecordBody;
use serde_json::Value;
use std::path::PathBuf;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/grok")
        .join(name)
}

#[test]
fn grok_single_tool_projects_into_view_records_and_session_label() {
    let path = fixture("scenario_02_single_tool.jsonl");
    let mut state = TranscriptState::new("grok-proj".into());
    let events = read_new_lines(&path, &mut state).expect("read");
    assert_eq!(state.format, TranscriptFormat::Grok);

    // Session id rekeyed from the wire sessionId field.
    let session_id = state.session_id.clone();
    assert!(
        session_id.starts_with("019f6cb5"),
        "expected wire session id, got {session_id}"
    );

    let mut projection = SessionProjection::new(&session_id);
    let mut bodies = Vec::new();
    for event in &events {
        assert_eq!(event.agent.as_deref(), Some("grok-build"));
        let event_json: Value = serde_json::to_value(event).expect("serialize");
        let result = projection.append(&event_json);
        bodies.extend(result.records.into_iter().map(|r| r.body));
    }

    assert_eq!(
        projection.label(),
        Some("List the project root."),
        "label should come from first user prompt"
    );

    assert!(
        bodies
            .iter()
            .any(|b| matches!(b, RecordBody::UserMessage(_))),
        "expected UserMessage body"
    );
    assert!(
        bodies.iter().any(|b| matches!(b, RecordBody::ToolCall(_))),
        "expected ToolCall body"
    );
    assert!(
        bodies
            .iter()
            .any(|b| matches!(b, RecordBody::ToolResult(_))),
        "expected ToolResult body"
    );
    assert!(
        bodies
            .iter()
            .any(|b| matches!(b, RecordBody::AssistantMessage(_))),
        "expected AssistantMessage body"
    );
}

#[test]
fn grok_error_recovery_marks_tool_error_on_failed_read() {
    let path = fixture("scenario_04_error_recovery.jsonl");
    let mut state = TranscriptState::new("grok-err".into());
    let events = read_new_lines(&path, &mut state).expect("read");

    let mut projection = SessionProjection::new(&state.session_id);
    let mut tool_results = Vec::new();
    for event in &events {
        let event_json: Value = serde_json::to_value(event).unwrap();
        let result = projection.append(&event_json);
        for rec in result.records {
            if let RecordBody::ToolResult(tr) = rec.body {
                tool_results.push(tr);
            }
        }
    }

    assert!(
        tool_results.len() >= 2,
        "expected ≥2 tool results, got {}",
        tool_results.len()
    );
    assert!(
        tool_results.iter().any(|tr| tr.is_error),
        "expected at least one is_error tool result"
    );
    assert!(
        tool_results.iter().any(|tr| !tr.is_error),
        "expected at least one successful tool result after recovery"
    );
}
