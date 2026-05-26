//! Codex rollout events compose through reader -> CloudEvent -> projection -> ViewRecord.

use std::io::Write;

use open_story::reader::read_new_lines;
use open_story::translate::{TranscriptFormat, TranscriptState};
use open_story_store::projection::SessionProjection;
use open_story_views::unified::RecordBody;
use serde_json::Value;
use tempfile::NamedTempFile;

#[test]
fn codex_rollout_slice_projects_into_view_records_and_session_label() {
    let mut file = NamedTempFile::new().expect("create temp file");
    writeln!(
        file,
        r#"{{"timestamp":"2026-05-23T12:11:57.894Z","type":"session_meta","payload":{{"id":"codex-thread-1","cwd":"/Users/maxglassie/projects/codex","originator":"codex-tui","cli_version":"0.133.0"}}}}"#
    )
    .expect("write session_meta");
    writeln!(
        file,
        r#"{{"timestamp":"2026-05-23T12:11:57.908Z","type":"event_msg","payload":{{"type":"user_message","message":"Can you find the session data for this current codex session?","images":[],"local_images":[],"text_elements":[]}}}}"#
    )
    .expect("write user_message");
    writeln!(
        file,
        r#"{{"timestamp":"2026-05-23T12:12:00.552Z","type":"response_item","payload":{{"type":"function_call","name":"exec_command","arguments":"{{\"cmd\":\"pwd\"}}","call_id":"call_pwd"}}}}"#
    )
    .expect("write function_call");
    writeln!(
        file,
        r#"{{"timestamp":"2026-05-23T12:12:00.614Z","type":"response_item","payload":{{"type":"function_call_output","call_id":"call_pwd","output":"Output:\n/Users/maxglassie/projects/codex\n"}}}}"#
    )
    .expect("write function_call_output");
    writeln!(
        file,
        r#"{{"timestamp":"2026-05-23T12:12:01.896Z","type":"event_msg","payload":{{"type":"agent_message","message":"Current Codex session data is in ~/.codex/sessions/...jsonl.","phase":"final_answer","memory_citation":null}}}}"#
    )
    .expect("write agent_message");
    file.flush().expect("flush");

    let mut state = TranscriptState::new("codex-thread-1".to_string());
    let events = read_new_lines(file.path(), &mut state).expect("read codex fixture");

    assert_eq!(state.format, TranscriptFormat::Codex);

    let mut projection = SessionProjection::new("codex-thread-1");
    let mut bodies = Vec::new();
    for event in &events {
        let event_json: Value = serde_json::to_value(event).expect("serialize event");
        let result = projection.append(&event_json);
        bodies.extend(result.records.into_iter().map(|record| record.body));
    }

    assert_eq!(
        projection.label(),
        Some("Can you find the session data for this current cod")
    );
    assert!(bodies
        .iter()
        .any(|body| matches!(body, RecordBody::UserMessage(_))));
    assert!(bodies
        .iter()
        .any(|body| matches!(body, RecordBody::ToolCall(_))));
    assert!(bodies
        .iter()
        .any(|body| matches!(body, RecordBody::ToolResult(_))));
    assert!(bodies
        .iter()
        .any(|body| matches!(body, RecordBody::AssistantMessage(_))));
}
