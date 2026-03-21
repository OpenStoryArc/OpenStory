// ViewRecord: the wrapper that the server emits and the UI consumes.
// CloudEvent metadata (id, seq) + typed UnifiedRecord body.

use serde::{Deserialize, Serialize};
use crate::unified::RecordBody;

/// Open-story metadata + typed record body.
/// This is what the server emits and the UI consumes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ViewRecord {
    /// CloudEvent ID (UUID) — used as React key, selection, dedup.
    pub id: String,
    /// Sequence number from open-story's ingest pipeline.
    pub seq: u64,
    /// Session this record belongs to.
    pub session_id: String,
    /// When this record was created.
    pub timestamp: String,
    /// Subagent identity: which agent produced this event (None = main agent).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    /// Whether this event belongs to a sidechain (subagent file).
    #[serde(default)]
    pub is_sidechain: bool,
    /// The typed record body.
    #[serde(flatten)]
    pub body: RecordBody,
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use crate::view_record::ViewRecord;
    use crate::unified::RecordBody;

    // describe("ViewRecord")
    mod view_record {
        use super::*;

        #[test]
        fn it_should_serialize_with_flattened_record_body() {
            let vr = ViewRecord {
                id: "evt-001".into(),
                seq: 1,
                session_id: "sess-abc".into(),
                timestamp: "2025-01-09T10:00:00Z".into(),
                agent_id: None,
                is_sidechain: false,
                body: RecordBody::TurnEnd(crate::unified::TurnEnd {
                    turn_id: None,
                    reason: Some("end_turn".into()),
                    duration_ms: Some(3000),
                }),
            };
            let json = serde_json::to_value(&vr).unwrap();
            assert_eq!(json["id"], "evt-001");
            assert_eq!(json["seq"], 1);
            assert_eq!(json["session_id"], "sess-abc");
            assert_eq!(json["record_type"], "turn_end");
            assert_eq!(json["payload"]["duration_ms"], 3000);
        }

        #[test]
        fn it_should_deserialize_from_json() {
            let json = json!({
                "id": "evt-002",
                "seq": 5,
                "session_id": "sess-xyz",
                "timestamp": "2025-01-09T11:00:00Z",
                "record_type": "user_message",
                "payload": {
                    "content": "Fix the bug"
                }
            });
            let vr: ViewRecord = serde_json::from_value(json).unwrap();
            assert_eq!(vr.id, "evt-002");
            assert_eq!(vr.seq, 5);
            match vr.body {
                RecordBody::UserMessage(u) => {
                    match u.content {
                        crate::unified::MessageContent::Text(t) => assert_eq!(t, "Fix the bug"),
                        other => panic!("expected Text, got {:?}", other),
                    }
                }
                other => panic!("expected UserMessage, got {:?}", other),
            }
        }
    }
}
