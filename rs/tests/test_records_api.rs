//! Integration tests for GET /api/sessions/{id}/records endpoint.
//!
//! This endpoint returns session events as WireRecords (same format the
//! Timeline renders) from in-memory projections.

mod helpers;

use axum::body::Body;
use axum::http::Request;
use helpers::{
    body_json, make_assistant_text, make_tool_result, make_tool_use, make_user_prompt,
    send_request, test_state,
};
use serde_json::json;
use tempfile::TempDir;

use open_story::server::ingest_events;

// describe("GET /api/sessions/{id}/records")
mod records_endpoint {
    use super::*;

    #[tokio::test]
    async fn it_should_return_empty_array_for_unknown_session() {
        let tmp = TempDir::new().unwrap();
        let state = test_state(&tmp);

        let req = Request::get("/api/sessions/nonexistent/records")
            .body(Body::empty())
            .unwrap();
        let resp = send_request(state, req).await;
        assert_eq!(resp.status(), 200);

        let body = body_json(resp).await;
        assert_eq!(body, json!([]));
    }

    #[tokio::test]
    async fn it_should_return_wire_records_with_depth_and_truncation_fields() {
        let tmp = TempDir::new().unwrap();
        let state = test_state(&tmp);
        {
            let mut s = state.write().await;
            let events = vec![
                make_user_prompt("sess-rec", "evt-1"),
                make_tool_use("sess-rec", "evt-2", None, "Bash", "cargo test"),
                make_tool_result("sess-rec", "evt-3", None, "toolu_evt-2", "test result: ok"),
            ];
            ingest_events(&mut s, "sess-rec", &events, None);
        }

        let req = Request::get("/api/sessions/sess-rec/records")
            .body(Body::empty())
            .unwrap();
        let resp = send_request(state, req).await;
        assert_eq!(resp.status(), 200);

        let body = body_json(resp).await;
        let records = body.as_array().unwrap();
        assert!(
            records.len() >= 3,
            "should have at least 3 wire records (user prompt + tool call + tool result), got {}",
            records.len()
        );

        // Every record should have WireRecord fields
        for record in records {
            assert!(record.get("id").is_some(), "record should have id");
            assert!(record.get("session_id").is_some(), "record should have session_id");
            assert!(record.get("timestamp").is_some(), "record should have timestamp");
            assert!(
                record.get("depth").is_some(),
                "record should have depth (WireRecord field)"
            );
            assert!(
                record.get("truncated").is_some(),
                "record should have truncated (WireRecord field)"
            );
            assert!(
                record.get("payload_bytes").is_some(),
                "record should have payload_bytes (WireRecord field)"
            );
        }
    }

    #[tokio::test]
    async fn it_should_reflect_tree_depth_for_nested_events() {
        let tmp = TempDir::new().unwrap();
        let state = test_state(&tmp);
        {
            let mut s = state.write().await;
            let events = vec![
                make_user_prompt("sess-depth", "evt-root"),
                make_tool_use("sess-depth", "evt-child", Some("evt-root"), "Bash", "ls"),
            ];
            ingest_events(&mut s, "sess-depth", &events, None);
        }

        let req = Request::get("/api/sessions/sess-depth/records")
            .body(Body::empty())
            .unwrap();
        let resp = send_request(state, req).await;
        assert_eq!(resp.status(), 200);

        let body = body_json(resp).await;
        let records = body.as_array().unwrap();

        // Find root record (depth 0) and child record (depth 1)
        let root = records
            .iter()
            .find(|r| r["depth"].as_u64() == Some(0))
            .expect("should have a root record with depth 0");
        assert!(root["parent_uuid"].is_null(), "root should have null parent_uuid");

        let child = records
            .iter()
            .find(|r| r["depth"].as_u64() == Some(1))
            .expect("should have a child record with depth 1");
        assert_eq!(
            child["parent_uuid"].as_str(),
            Some("evt-root"),
            "child should reference parent"
        );
    }

    #[tokio::test]
    async fn it_should_return_records_sorted_by_timestamp() {
        let tmp = TempDir::new().unwrap();
        let state = test_state(&tmp);
        {
            let mut s = state.write().await;
            let events = vec![
                make_user_prompt("sess-sort", "evt-a"),
                make_assistant_text("sess-sort", "evt-b", None, "hello"),
                make_tool_use("sess-sort", "evt-c", None, "Read", "/tmp/file"),
            ];
            ingest_events(&mut s, "sess-sort", &events, None);
        }

        let req = Request::get("/api/sessions/sess-sort/records")
            .body(Body::empty())
            .unwrap();
        let resp = send_request(state, req).await;
        let body = body_json(resp).await;
        let records = body.as_array().unwrap();
        assert!(!records.is_empty());

        // Verify records are in chronological order
        let timestamps: Vec<&str> = records
            .iter()
            .filter_map(|r| r["timestamp"].as_str())
            .collect();
        for window in timestamps.windows(2) {
            assert!(
                window[0] <= window[1],
                "records should be in chronological order: {} <= {}",
                window[0],
                window[1]
            );
        }
    }
}
