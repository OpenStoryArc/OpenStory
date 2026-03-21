// WireRecord: ViewRecord + tree metadata + truncation info.
//
// This is the wire format sent over WebSocket to the UI.
// Extends ViewRecord with depth, parent_uuid, truncation flag,
// and original payload byte count.

use serde::{Deserialize, Serialize};
use crate::view_record::ViewRecord;

/// Truncation threshold in bytes. Payloads larger than this are truncated
/// on the wire; full content available via REST lazy-load.
pub const TRUNCATION_THRESHOLD: usize = 2000;

/// WireRecord = ViewRecord + tree metadata + truncation info.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WireRecord {
    /// The underlying ViewRecord (flattened into the same JSON object).
    #[serde(flatten)]
    pub record: ViewRecord,
    /// Tree depth (0 = root node).
    pub depth: u16,
    /// Parent event UUID, if not a root node.
    pub parent_uuid: Option<String>,
    /// Whether the payload was truncated for wire transfer.
    pub truncated: bool,
    /// Original payload size in bytes (before truncation).
    pub payload_bytes: u64,
}

/// Result of truncating a payload string.
pub struct TruncateResult {
    /// The (possibly truncated) output content.
    pub output: String,
    /// Whether truncation occurred.
    pub truncated: bool,
    /// Original byte count.
    pub original_bytes: usize,
}

/// Truncate content if it exceeds `max_len` bytes.
///
/// Returns the (possibly truncated) content, whether it was truncated,
/// and the original byte count. Respects UTF-8 char boundaries.
pub fn truncate_payload(content: &str, max_len: usize) -> TruncateResult {
    let original_bytes = content.len();
    if original_bytes <= max_len {
        TruncateResult {
            output: content.to_string(),
            truncated: false,
            original_bytes,
        }
    } else {
        // Find the largest char boundary <= max_len
        let mut end = max_len;
        while end > 0 && !content.is_char_boundary(end) {
            end -= 1;
        }
        TruncateResult {
            output: content[..end].to_string(),
            truncated: true,
            original_bytes,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::unified::{RecordBody, SystemEvent};

    fn make_wire_record(depth: u16, parent_uuid: Option<&str>, truncated: bool, payload_bytes: u64) -> WireRecord {
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

    // describe("WireRecord serialization")
    mod serialization {
        use super::*;

        #[test]
        fn it_should_serialize_with_depth_and_parent_uuid() {
            let wire = make_wire_record(5, Some("abc"), false, 42);
            let json = serde_json::to_value(&wire).unwrap();
            assert_eq!(json["depth"], 5);
            assert_eq!(json["parent_uuid"], "abc");
            assert_eq!(json["truncated"], false);
            assert_eq!(json["payload_bytes"], 42);
            // ViewRecord fields should be flattened
            assert_eq!(json["id"], "evt-001");
            assert_eq!(json["seq"], 1);
            assert_eq!(json["session_id"], "sess-abc");
            assert_eq!(json["record_type"], "system_event");
        }

        #[test]
        fn it_should_serialize_null_parent_uuid_for_root_nodes() {
            let wire = make_wire_record(0, None, false, 10);
            let json = serde_json::to_value(&wire).unwrap();
            assert!(json["parent_uuid"].is_null());
            assert_eq!(json["depth"], 0);
        }

        #[test]
        fn it_should_deserialize_back_from_json() {
            let wire = make_wire_record(3, Some("parent-1"), true, 5000);
            let json = serde_json::to_value(&wire).unwrap();
            let deserialized: WireRecord = serde_json::from_value(json).unwrap();
            assert_eq!(deserialized.depth, 3);
            assert_eq!(deserialized.parent_uuid.as_deref(), Some("parent-1"));
            assert!(deserialized.truncated);
            assert_eq!(deserialized.payload_bytes, 5000);
        }
    }

    // describe("truncate_payload")
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

        #[test]
        fn it_should_handle_utf8_char_boundaries() {
            // "é" is 2 bytes in UTF-8. If max_len falls in the middle,
            // we should truncate to the previous char boundary.
            let content = "é".repeat(1500); // 3000 bytes
            let result = truncate_payload(&content, 2000);
            assert!(result.truncated);
            assert!(result.output.len() <= 2000);
            // Should be an even number of bytes (complete chars)
            assert_eq!(result.output.len() % 2, 0);
        }
    }
}
