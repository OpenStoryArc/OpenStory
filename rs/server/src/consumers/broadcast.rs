//! Broadcast consumer — transforms events for WebSocket delivery.
//!
//! Actor contract:
//!   subscribes: events.> + patterns.> + changes.>
//!   publishes:  WebSocket (external, not NATS)
//!   owns:       broadcast_tx, full_payloads (truncation cache)
//!
//! The broadcast consumer is the BFF (backend-for-frontend). It transforms
//! CloudEvents into ViewRecords → WireRecords for the UI, and forwards
//! detected patterns from the patterns consumer.
//!
//! It subscribes to THREE streams:
//!   - events.>   — raw CloudEvents for real-time display
//!   - patterns.> — detected PatternEvents for Story tab
//!   - changes.>  — session metadata updates for sidebar

use std::collections::HashMap;

use open_story_core::cloud_event::CloudEvent;
use open_story_views::from_cloud_event::from_cloud_event;
use open_story_views::unified::RecordBody;
use open_story_views::view_record::ViewRecord;
use open_story_views::wire_record::{WireRecord, TRUNCATION_THRESHOLD};
use open_story_store::projection::SessionProjection;

use crate::broadcast::BroadcastMessage;
use open_story_store::ingest::to_wire_record;

/// State owned by the broadcast consumer actor.
pub struct BroadcastConsumer {
    /// Full payloads for truncated records (lazy-load via API).
    full_payloads: HashMap<String, HashMap<String, String>>,
}

/// Result of transforming one batch of CloudEvents for broadcast.
pub struct BroadcastResult {
    /// BroadcastMessages ready to send to WebSocket clients.
    pub messages: Vec<BroadcastMessage>,
}

impl BroadcastConsumer {
    pub fn new() -> Self {
        Self {
            full_payloads: HashMap::new(),
        }
    }

    /// Transform CloudEvents into BroadcastMessages for WebSocket delivery.
    ///
    /// Requires the SessionProjection for tree depth and wire record enrichment.
    pub fn process_events(
        &mut self,
        session_id: &str,
        events: &[CloudEvent],
        _projection: &SessionProjection,
    ) -> Vec<ViewRecord> {
        let mut all_records = Vec::new();

        for ce in events {
            let view_records = from_cloud_event(ce);

            // Capture full payloads for truncated records
            for vr in &view_records {
                if let RecordBody::ToolResult(tr) = &vr.body {
                    if let Some(output) = &tr.output {
                        if output.len() > TRUNCATION_THRESHOLD {
                            self.full_payloads
                                .entry(session_id.to_string())
                                .or_default()
                                .insert(vr.id.clone(), output.clone());
                        }
                    }
                }
            }

            all_records.extend(view_records);
        }

        all_records
    }

    /// Convert ViewRecords to WireRecords using projection context.
    pub fn to_wire_records(
        records: &[ViewRecord],
        projection: &SessionProjection,
    ) -> Vec<WireRecord> {
        records.iter().map(|vr| to_wire_record(vr, projection)).collect()
    }

    /// Get the full payload for a truncated record (for lazy-load API).
    pub fn full_payload(&self, session_id: &str, event_id: &str) -> Option<&str> {
        self.full_payloads
            .get(session_id)
            .and_then(|m| m.get(event_id))
            .map(|s| s.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use open_story_core::event_data::{AgentPayload, ClaudeCodePayload, EventData};
    use serde_json::json;

    fn make_event(session_id: &str, subtype: &str) -> CloudEvent {
        let mut payload = ClaudeCodePayload::new();
        payload.text = Some("test content".to_string());
        let data = EventData::with_payload(
            json!({}), 0, session_id.to_string(),
            AgentPayload::ClaudeCode(payload),
        );
        CloudEvent::new(
            format!("arc://test/{session_id}"),
            "io.arc.event".into(),
            data,
            Some(subtype.into()),
            None, None, None, None,
            Some("claude-code".into()),
        )
    }

    #[test]
    fn transforms_events_to_view_records() {
        let mut consumer = BroadcastConsumer::new();
        let proj = SessionProjection::new("sess-1");
        let events = vec![make_event("sess-1", "message.user.prompt")];

        let records = consumer.process_events("sess-1", &events, &proj);
        assert!(!records.is_empty(), "should produce view records");
    }

    #[test]
    fn converts_view_records_to_wire_records() {
        let mut consumer = BroadcastConsumer::new();
        let proj = SessionProjection::new("sess-1");
        let events = vec![make_event("sess-1", "message.user.prompt")];

        let view_records = consumer.process_events("sess-1", &events, &proj);
        let wire_records = BroadcastConsumer::to_wire_records(&view_records, &proj);
        assert_eq!(wire_records.len(), view_records.len());
    }
}
