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
use open_story_store::projection::{is_ephemeral, SessionProjection};

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

impl Default for BroadcastConsumer {
    fn default() -> Self {
        Self::new()
    }
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

    /// Full BFF pipeline: CloudEvents → BroadcastMessages for WebSocket.
    ///
    /// Mirrors the broadcast-assembly half of `ingest_events` (the function
    /// that commit 1.6 will delete). Works against a pre-updated projection
    /// — the caller is expected to run `projection.append(event)` for each
    /// event first (ProjectionsConsumer does this in the NATS path).
    ///
    /// `project_id` / `project_name` come from the enclosing `AppState`'s
    /// project-resolution maps; pass `None` when unknown.
    ///
    /// Returns one `BroadcastMessage::Enriched` per event that produced
    /// ViewRecords. Events that produce nothing (empty view_records, no
    /// filter deltas) are skipped — same as ingest_events.
    pub fn process_batch(
        &mut self,
        session_id: &str,
        events: &[CloudEvent],
        projection: &SessionProjection,
        project_id: Option<String>,
        project_name: Option<String>,
    ) -> Vec<BroadcastMessage> {
        let mut messages = Vec::new();

        for ce in events {
            let Ok(val) = serde_json::to_value(ce) else { continue };

            let view_records = from_cloud_event(ce);

            // Capture full payloads for truncated records (lazy-load cache).
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

            if view_records.is_empty() {
                continue;
            }

            let subtype = val.get("subtype").and_then(|v| v.as_str());
            let eph = is_ephemeral(subtype);

            if eph {
                messages.push(BroadcastMessage::Enriched {
                    session_id: session_id.to_string(),
                    records: Vec::new(),
                    ephemeral: view_records,
                    filter_deltas: HashMap::new(),
                    patterns: Vec::new(),
                    project_id: project_id.clone(),
                    project_name: project_name.clone(),
                    session_label: None,
                    session_branch: None,
                    total_input_tokens: None,
                    total_output_tokens: None,
                });
            } else {
                let wire_records: Vec<WireRecord> = view_records
                    .iter()
                    .map(|vr| to_wire_record(vr, projection))
                    .collect();
                messages.push(BroadcastMessage::Enriched {
                    session_id: session_id.to_string(),
                    records: wire_records,
                    ephemeral: Vec::new(),
                    filter_deltas: HashMap::new(),
                    patterns: Vec::new(),
                    project_id: project_id.clone(),
                    project_name: project_name.clone(),
                    session_label: projection.label().map(|s| s.to_string()),
                    session_branch: projection.branch().map(|s| s.to_string()),
                    total_input_tokens: Some(projection.total_input_tokens()),
                    total_output_tokens: Some(projection.total_output_tokens()),
                });
            }
        }

        messages
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
