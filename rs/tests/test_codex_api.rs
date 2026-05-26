//! Codex rollout sessions are visible through the browser-facing API.

mod helpers;

use std::io::Write;

use axum::body::Body;
use axum::http::Request;
use helpers::{body_json, seed_and_ingest, send_request, test_state};
use open_story::reader::read_new_lines;
use open_story::translate::{TranscriptFormat, TranscriptState};
use tempfile::{NamedTempFile, TempDir};

const CODEX_THREAD_ID: &str = "019e5a13-69cf-7b13-baeb-d6891eafd55e";

fn codex_rollout_events() -> Vec<open_story::cloud_event::CloudEvent> {
    let mut file = NamedTempFile::new().expect("create codex fixture");
    writeln!(
        file,
        r#"{{"timestamp":"2026-05-24T13:01:22.000Z","type":"session_meta","payload":{{"id":"{CODEX_THREAD_ID}","timestamp":"2026-05-24T13:01:22.000Z","cwd":"/Users/maxglassie/projects/OpenStory/rs","originator":"codex-api","cli_version":"0.133.0","source":"api","thread_source":"user","model_provider":"openai"}}}}"#
    )
    .expect("write session_meta");
    writeln!(
        file,
        r#"{{"timestamp":"2026-05-24T13:01:23.000Z","type":"event_msg","payload":{{"type":"user_message","message":"I want to be able to see this session in the UI","images":[],"local_images":[],"text_elements":[]}}}}"#
    )
    .expect("write user prompt");
    writeln!(
        file,
        r#"{{"timestamp":"2026-05-24T13:01:24.000Z","type":"response_item","payload":{{"type":"reasoning","summary":[{{"type":"summary_text","text":"Checking the browser-facing OpenStory API."}}]}}}}"#
    )
    .expect("write reasoning");
    writeln!(
        file,
        r#"{{"timestamp":"2026-05-24T13:01:25.000Z","type":"response_item","payload":{{"type":"function_call","name":"exec_command","arguments":"{{\"cmd\":\"cargo test -p open-story --test test_codex_api\"}}","call_id":"call_codex_api"}}}}"#
    )
    .expect("write tool call");
    writeln!(
        file,
        r#"{{"timestamp":"2026-05-24T13:01:26.000Z","type":"response_item","payload":{{"type":"function_call_output","call_id":"call_codex_api","output":"Output:\nCodex API test passed\n"}}}}"#
    )
    .expect("write tool output");
    writeln!(
        file,
        r#"{{"timestamp":"2026-05-24T13:01:27.000Z","type":"event_msg","payload":{{"type":"agent_message","message":"The Codex session is visible in the browser API.","phase":"final_answer","memory_citation":null}}}}"#
    )
    .expect("write assistant text");
    file.flush().expect("flush fixture");

    let mut state = TranscriptState::new("rollout-fixture".to_string());
    let events = read_new_lines(file.path(), &mut state).expect("read codex rollout");
    assert_eq!(state.format, TranscriptFormat::Codex);
    assert_eq!(state.session_id, CODEX_THREAD_ID);
    events
}

#[tokio::test]
async fn codex_session_is_visible_through_browser_api() {
    let data_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);
    let events = codex_rollout_events();

    {
        let mut s = state.write().await;
        seed_and_ingest(&mut s, CODEX_THREAD_ID, &events, None).await;
    }

    let resp = send_request(
        state.clone(),
        Request::get("/api/sessions").body(Body::empty()).unwrap(),
    )
    .await;
    assert_eq!(resp.status(), 200);
    let body = body_json(resp).await;
    let sessions = body["sessions"].as_array().expect("sessions array");
    let row = sessions
        .iter()
        .find(|session| session["session_id"] == CODEX_THREAD_ID)
        .expect("Codex session should be listed");
    assert_eq!(row["event_count"].as_u64(), Some(events.len() as u64));
    assert_eq!(
        row["label"].as_str(),
        Some("I want to be able to see this session in the UI")
    );
    assert_eq!(row["origin_agent"].as_str(), Some("codex"));
    assert!(row["last_event"].as_str().is_some());

    let resp = send_request(
        state.clone(),
        Request::get(format!("/api/sessions/{CODEX_THREAD_ID}/records"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(resp.status(), 200);
    let records_body = body_json(resp).await;
    let records = records_body.as_array().expect("records array");
    let record_types: Vec<&str> = records
        .iter()
        .filter_map(|record| record["record_type"].as_str())
        .collect();
    assert!(record_types.contains(&"user_message"));
    assert!(record_types.contains(&"reasoning"));
    assert!(record_types.contains(&"tool_call"));
    assert!(record_types.contains(&"tool_result"));
    assert!(record_types.contains(&"assistant_message"));

    let resp = send_request(
        state.clone(),
        Request::get(format!("/api/sessions/{CODEX_THREAD_ID}/view-records"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(resp.status(), 200);
    let view_records_body = body_json(resp).await;
    let view_records = view_records_body.as_array().expect("view records array");
    assert_eq!(view_records.len(), records.len());
    assert!(
        view_records
            .iter()
            .all(|record| record["origin_agent"] == "codex"),
        "Codex view records should preserve their origin agent"
    );

    let resp = send_request(
        state,
        Request::get(format!("/api/sessions/{CODEX_THREAD_ID}/conversation"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(resp.status(), 200);
    let conversation = body_json(resp).await;
    let entries = conversation["entries"]
        .as_array()
        .expect("conversation entries");
    assert!(
        entries
            .iter()
            .any(|entry| entry["entry_type"] == "user_message")
    );
    assert!(
        entries
            .iter()
            .any(|entry| entry["entry_type"] == "tool_roundtrip")
    );
    assert!(
        entries
            .iter()
            .any(|entry| entry["entry_type"] == "assistant_message")
    );
}
