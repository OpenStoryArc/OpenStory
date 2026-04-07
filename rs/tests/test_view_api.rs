//! Integration tests for GET /api/sessions/{id}/view-records,
//! GET /api/sessions/{id}/conversation, and GET /api/sessions/{id}/file-changes.

mod helpers;

use axum::body::Body;
use axum::http::Request;
use helpers::{body_json, send_request, test_state};
use serde_json::json;
use tempfile::TempDir;

use open_story::cloud_event::CloudEvent;
use open_story::event_data::{AgentPayload, ClaudeCodePayload, EventData};
use open_story::server::ingest_events;

fn make_tool_use_event(session_id: &str, tool: &str, args: serde_json::Value, call_id: &str) -> CloudEvent {
    let mut payload = ClaudeCodePayload::new();
    payload.tool = Some(tool.to_string());
    payload.args = Some(args.clone());
    let data = EventData::with_payload(
        json!({
            "type": "assistant",
            "message": {
                "model": "claude-sonnet-4-20250514",
                "content": [
                    {"type": "tool_use", "id": call_id, "name": tool, "input": args}
                ]
            }
        }),
        1,
        session_id.to_string(),
        AgentPayload::ClaudeCode(payload),
    );
    CloudEvent::new(
        format!("arc://transcript/{session_id}"),
        "io.arc.event".into(),
        data,
        Some("message.assistant.tool_use".into()),
        None,
        None,
        None,
        None,
        None,
    )
}

fn make_tool_result_event(session_id: &str, call_id: &str, output: &str) -> CloudEvent {
    let data = EventData::with_payload(
        json!({
            "type": "user",
            "message": {
                "content": [
                    {"type": "tool_result", "tool_use_id": call_id, "content": output}
                ]
            }
        }),
        2,
        session_id.to_string(),
        AgentPayload::ClaudeCode(ClaudeCodePayload::new()),
    );
    CloudEvent::new(
        format!("arc://transcript/{session_id}"),
        "io.arc.event".into(),
        data,
        Some("message.user.tool_result".into()),
        None,
        None,
        None,
        None,
        None,
    )
}

fn make_user_prompt_event(session_id: &str, text: &str) -> CloudEvent {
    let mut payload = ClaudeCodePayload::new();
    payload.text = Some(text.to_string());
    let data = EventData::with_payload(
        json!({
            "type": "user",
            "message": {"content": [{"type": "text", "text": text}]}
        }),
        0,
        session_id.to_string(),
        AgentPayload::ClaudeCode(payload),
    );
    CloudEvent::new(
        format!("arc://transcript/{session_id}"),
        "io.arc.event".into(),
        data,
        Some("message.user.prompt".into()),
        None,
        None,
        None,
        None,
        None,
    )
}

// describe("GET /api/sessions/{id}/view-records")
mod view_records_endpoint {
    use super::*;

    #[tokio::test]
    async fn it_should_return_view_record_array_with_typed_records() {
        let tmp = TempDir::new().unwrap();
        let state = test_state(&tmp);
        {
            let mut s = state.write().await;
            let events = vec![
                make_user_prompt_event("sess-1", "Fix the bug"),
                make_tool_use_event("sess-1", "Bash", json!({"command": "cargo test"}), "toolu_1"),
                make_tool_result_event("sess-1", "toolu_1", "test result: ok"),
            ];
            ingest_events(&mut s, "sess-1", &events, None).await;
        }

        let req = Request::get("/api/sessions/sess-1/view-records")
            .body(Body::empty())
            .unwrap();
        let resp = send_request(state, req).await;
        assert_eq!(resp.status(), 200);

        let body = body_json(resp).await;
        let records = body.as_array().unwrap();
        assert!(records.len() >= 3, "should have at least 3 records, got {}", records.len());

        // Check that records have expected structure
        for record in records {
            assert!(record.get("id").is_some(), "record should have id");
            assert!(record.get("seq").is_some(), "record should have seq");
            assert!(record.get("record_type").is_some(), "record should have record_type");
        }
    }

    #[tokio::test]
    async fn it_should_have_typed_tool_input_on_tool_calls() {
        let tmp = TempDir::new().unwrap();
        let state = test_state(&tmp);
        {
            let mut s = state.write().await;
            let events = vec![
                make_tool_use_event("sess-1", "Edit", json!({
                    "file_path": "/src/main.rs",
                    "old_string": "fn old()",
                    "new_string": "fn new()"
                }), "toolu_edit"),
            ];
            ingest_events(&mut s, "sess-1", &events, None).await;
        }

        let req = Request::get("/api/sessions/sess-1/view-records")
            .body(Body::empty())
            .unwrap();
        let resp = send_request(state, req).await;
        let body = body_json(resp).await;
        let records = body.as_array().unwrap();

        let tool_calls: Vec<_> = records.iter()
            .filter(|r| r.get("record_type").and_then(|v| v.as_str()) == Some("tool_call"))
            .collect();
        assert!(!tool_calls.is_empty(), "should have tool_call records");

        let tc = &tool_calls[0]["payload"];
        assert_eq!(tc["name"], "Edit");
        assert!(tc.get("typed_input").is_some(), "should have typed_input");
        assert_eq!(tc["typed_input"]["tool"], "edit");
        assert_eq!(tc["typed_input"]["file_path"], "/src/main.rs");
    }

    #[tokio::test]
    async fn it_should_return_empty_for_unknown_session() {
        let tmp = TempDir::new().unwrap();
        let state = test_state(&tmp);

        let req = Request::get("/api/sessions/nonexistent/view-records")
            .body(Body::empty())
            .unwrap();
        let resp = send_request(state, req).await;
        assert_eq!(resp.status(), 200);

        let body = body_json(resp).await;
        assert_eq!(body, json!([]));
    }
}

// describe("GET /api/sessions/{id}/conversation")
mod conversation_endpoint {
    use super::*;

    #[tokio::test]
    async fn it_should_return_paired_conversation_entries() {
        let tmp = TempDir::new().unwrap();
        let state = test_state(&tmp);
        {
            let mut s = state.write().await;
            let events = vec![
                make_user_prompt_event("sess-1", "Run the tests"),
                make_tool_use_event("sess-1", "Bash", json!({"command": "cargo test"}), "toolu_1"),
                make_tool_result_event("sess-1", "toolu_1", "all tests passed"),
            ];
            ingest_events(&mut s, "sess-1", &events, None).await;
        }

        let req = Request::get("/api/sessions/sess-1/conversation")
            .body(Body::empty())
            .unwrap();
        let resp = send_request(state, req).await;
        assert_eq!(resp.status(), 200);

        let body = body_json(resp).await;
        let entries = body["entries"].as_array().unwrap();
        assert!(entries.len() >= 2, "should have at least 2 entries (user + tool roundtrip), got {}", entries.len());

        // First entry should be user message
        assert_eq!(entries[0]["entry_type"], "user_message");

        // Find the tool roundtrip
        let roundtrips: Vec<_> = entries.iter()
            .filter(|e| e.get("entry_type").and_then(|v| v.as_str()) == Some("tool_roundtrip"))
            .collect();
        assert!(!roundtrips.is_empty(), "should have tool_roundtrip entry");

        let rt = &roundtrips[0];
        assert!(rt.get("call").is_some(), "roundtrip should have call");
        assert!(rt.get("result").is_some(), "roundtrip should have result");
    }

    #[tokio::test]
    async fn it_should_leave_unpaired_calls_with_null_result() {
        let tmp = TempDir::new().unwrap();
        let state = test_state(&tmp);
        {
            let mut s = state.write().await;
            let events = vec![
                make_tool_use_event("sess-1", "Bash", json!({"command": "long running"}), "toolu_pending"),
            ];
            ingest_events(&mut s, "sess-1", &events, None).await;
        }

        let req = Request::get("/api/sessions/sess-1/conversation")
            .body(Body::empty())
            .unwrap();
        let resp = send_request(state, req).await;
        let body = body_json(resp).await;
        let entries = body["entries"].as_array().unwrap();

        let roundtrips: Vec<_> = entries.iter()
            .filter(|e| e.get("entry_type").and_then(|v| v.as_str()) == Some("tool_roundtrip"))
            .collect();
        assert!(!roundtrips.is_empty());
        assert!(roundtrips[0]["result"].is_null(), "unpaired call should have null result");
    }
}

// describe("GET /api/sessions/{id}/file-changes")
mod file_changes_endpoint {
    use super::*;

    #[tokio::test]
    async fn it_should_return_only_edit_and_write_tool_calls() {
        let tmp = TempDir::new().unwrap();
        let state = test_state(&tmp);
        {
            let mut s = state.write().await;
            let events = vec![
                make_tool_use_event("sess-1", "Edit", json!({
                    "file_path": "/src/main.rs",
                    "old_string": "old",
                    "new_string": "new"
                }), "toolu_edit"),
                make_tool_use_event("sess-1", "Write", json!({
                    "file_path": "/src/new.rs",
                    "content": "fn main() {}"
                }), "toolu_write"),
                make_tool_use_event("sess-1", "Bash", json!({"command": "cargo test"}), "toolu_bash"),
                make_tool_use_event("sess-1", "Read", json!({"file_path": "/src/lib.rs"}), "toolu_read"),
            ];
            ingest_events(&mut s, "sess-1", &events, None).await;
        }

        let req = Request::get("/api/sessions/sess-1/file-changes")
            .body(Body::empty())
            .unwrap();
        let resp = send_request(state, req).await;
        assert_eq!(resp.status(), 200);

        let body = body_json(resp).await;
        let changes = body.as_array().unwrap();
        assert_eq!(changes.len(), 2, "should have only Edit and Write, got {}", changes.len());

        let names: Vec<&str> = changes.iter()
            .filter_map(|r| r["payload"]["name"].as_str())
            .collect();
        assert!(names.contains(&"Edit"));
        assert!(names.contains(&"Write"));
        assert!(!names.contains(&"Bash"));
        assert!(!names.contains(&"Read"));
    }
}
