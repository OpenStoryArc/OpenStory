//! Spec: WireRecord type + payload truncation + lazy-load endpoint.
//!
//! Phase 1 of Story 036: Stateful BFF projection.
//!
//! WireRecord = ViewRecord + tree metadata (depth, parent_uuid) + truncation.
//! Large payloads are truncated on the wire; full content available via REST.

mod helpers;

use helpers::{body_text, make_event_with_large_payload, send_request, test_state};
use axum::body::Body;
use axum::http::Request;
use tempfile::TempDir;

use open_story_views::unified::{RecordBody, SystemEvent};
use open_story_views::view_record::ViewRecord;
use open_story_views::wire_record::{truncate_payload, WireRecord, TRUNCATION_THRESHOLD};

// ═══════════════════════════════════════════════════════════════════
// describe("WireRecord serialization")
// ═══════════════════════════════════════════════════════════════════

#[cfg(test)]
mod wire_record_serialization {
    use super::*;

    fn make_wire(depth: u16, parent_uuid: Option<&str>, truncated: bool, payload_bytes: u64) -> WireRecord {
        WireRecord {
            record: ViewRecord {
                id: "evt-001".into(),
                seq: 1,
                session_id: "sess-abc".into(),
                timestamp: "2025-01-09T10:00:00Z".into(),
                body: RecordBody::SystemEvent(SystemEvent {
                    subtype: "test".into(),
                    message: Some("test event".into()),
                    duration_ms: None,
                }),
                agent_id: None,
                is_sidechain: false,
            },
            depth,
            parent_uuid: parent_uuid.map(|s| s.to_string()),
            truncated,
            payload_bytes,
        }
    }

    #[test]
    fn it_should_serialize_with_depth_and_parent_uuid() {
        let wire = make_wire(5, Some("abc"), false, 42);
        let json = serde_json::to_value(&wire).unwrap();
        assert_eq!(json["depth"], 5);
        assert_eq!(json["parent_uuid"], "abc");
        assert_eq!(json["truncated"], false);
        assert_eq!(json["payload_bytes"], 42);
        // ViewRecord fields should be flattened
        assert_eq!(json["id"], "evt-001");
        assert_eq!(json["seq"], 1);
        assert_eq!(json["record_type"], "system_event");
    }

    #[test]
    fn it_should_serialize_null_parent_uuid_for_root_nodes() {
        let wire = make_wire(0, None, false, 10);
        let json = serde_json::to_value(&wire).unwrap();
        assert!(json["parent_uuid"].is_null());
        assert_eq!(json["depth"], 0);
    }
}

// ═══════════════════════════════════════════════════════════════════
// describe("truncate_payload")
// ═══════════════════════════════════════════════════════════════════

#[cfg(test)]
mod truncation {
    use super::*;

    /// Boundary table: content size → truncated flag + output length
    ///
    /// | Content size | truncated | payload_bytes | output length |
    /// |-------------|-----------|---------------|---------------|
    /// | 0           | false     | 0             | 0             |
    /// | 100         | false     | 100           | 100           |
    /// | 2000        | false     | 2000          | 2000          |
    /// | 2001        | true      | 2001          | 2000          |
    /// | 50000       | true      | 50000         | 2000          |
    #[test]
    fn boundary_table_truncation_threshold() {
        let cases: Vec<(usize, bool, usize)> = vec![
            // (content_size, expected_truncated, expected_output_len)
            (0,     false, 0),
            (100,   false, 100),
            (2000,  false, 2000),
            (2001,  true,  2000),
            (50000, true,  2000),
        ];

        for (content_size, expected_truncated, expected_output_len) in cases {
            let content = "x".repeat(content_size);
            let result = truncate_payload(&content, TRUNCATION_THRESHOLD);
            assert_eq!(result.truncated, expected_truncated,
                "content_size={content_size}: truncated");
            assert_eq!(result.output.len(), expected_output_len,
                "content_size={content_size}: output length");
            assert_eq!(result.original_bytes, content_size,
                "content_size={content_size}: original bytes preserved");
        }
    }

    #[test]
    fn it_should_preserve_original_byte_count() {
        let content = "x".repeat(50_000);
        let result = truncate_payload(&content, TRUNCATION_THRESHOLD);
        assert!(result.truncated);
        assert_eq!(result.original_bytes, 50_000);
        assert_eq!(result.output.len(), 2000);
    }
}

// ═══════════════════════════════════════════════════════════════════
// describe("GET /api/sessions/{id}/events/{eid}/content")
// ═══════════════════════════════════════════════════════════════════

#[cfg(test)]
mod content_endpoint {
    use super::*;
    use open_story::server::ingest_events;

    #[tokio::test]
    async fn it_should_return_full_payload_for_truncated_record() {
        let data_dir = TempDir::new().unwrap();
        let state = test_state(&data_dir);
        {
            let mut s = state.write().await;
            let event = make_event_with_large_payload("sess-1", "evt-big", 5000);
            ingest_events(&mut s, "sess-1", &[event], None).await;
        }

        let req = Request::get("/api/sessions/sess-1/events/evt-big/content")
            .body(Body::empty()).unwrap();
        let resp = send_request(state, req).await;
        assert_eq!(resp.status(), 200);
        let body = body_text(resp).await;
        assert_eq!(body.len(), 5000);
    }

    #[tokio::test]
    async fn it_should_return_404_for_nonexistent_event() {
        let data_dir = TempDir::new().unwrap();
        let state = test_state(&data_dir);

        let req = Request::get("/api/sessions/no-session/events/no-event/content")
            .body(Body::empty()).unwrap();
        let resp = send_request(state, req).await;
        assert_eq!(resp.status(), 404);
    }

    #[tokio::test]
    async fn it_should_return_content_for_non_truncated_event() {
        // With SQLite EventStore, all event payloads are available —
        // not just truncated ones. The content endpoint returns 200
        // for any event that exists, regardless of size.
        let data_dir = TempDir::new().unwrap();
        let state = test_state(&data_dir);
        {
            let mut s = state.write().await;
            // 100 bytes — well under TRUNCATION_THRESHOLD
            let event = make_event_with_large_payload("sess-1", "evt-small", 100);
            ingest_events(&mut s, "sess-1", &[event], None).await;
        }

        let req = Request::get("/api/sessions/sess-1/events/evt-small/content")
            .body(Body::empty()).unwrap();
        let resp = send_request(state, req).await;
        assert_eq!(resp.status(), 200);
    }
}
