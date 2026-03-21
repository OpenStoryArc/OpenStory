//! Broadcast message types sent to WebSocket subscribers.

use std::collections::HashMap;

use serde::Serialize;

use open_story_views::view_record::ViewRecord;
use open_story_views::wire_record::WireRecord;

use open_story_patterns::PatternEvent;

/// Messages broadcast to WebSocket subscribers.
///
/// This is the BFF (Backend-For-Frontend) boundary: the server transforms
/// raw CloudEvents into typed ViewRecords before broadcasting. The UI
/// receives pre-typed data and never parses raw transcript formats.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind")]
#[allow(clippy::large_enum_variant)]
pub enum BroadcastMessage {
    #[serde(rename = "view_records")]
    ViewRecords {
        session_id: String,
        view_records: Vec<ViewRecord>,
        #[serde(skip_serializing_if = "Option::is_none")]
        project_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        project_name: Option<String>,
    },
    /// Enriched broadcast: durable WireRecords + ephemeral ViewRecords + filter deltas + patterns.
    /// Phase 3: separates persistent records from transient progress events.
    #[serde(rename = "enriched")]
    Enriched {
        session_id: String,
        /// Durable records (UI accumulates these in state).
        records: Vec<WireRecord>,
        /// Ephemeral records (UI shows transiently, doesn't store).
        ephemeral: Vec<ViewRecord>,
        /// Incremental filter count changes from this batch.
        filter_deltas: HashMap<String, i32>,
        /// Patterns detected from this batch of events.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        patterns: Vec<PatternEvent>,
        #[serde(skip_serializing_if = "Option::is_none")]
        project_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        project_name: Option<String>,
        /// Session label (first user prompt), sent when first set.
        #[serde(skip_serializing_if = "Option::is_none")]
        session_label: Option<String>,
        /// Git branch, sent when first captured.
        #[serde(skip_serializing_if = "Option::is_none")]
        session_branch: Option<String>,
        /// New agent labels discovered in this batch.
        #[serde(default, skip_serializing_if = "HashMap::is_empty")]
        agent_labels: HashMap<String, String>,
        /// Accumulated input tokens for this session (sent when changed).
        #[serde(skip_serializing_if = "Option::is_none")]
        total_input_tokens: Option<u64>,
        /// Accumulated output tokens for this session (sent when changed).
        #[serde(skip_serializing_if = "Option::is_none")]
        total_output_tokens: Option<u64>,
    },
    #[serde(rename = "plan_saved")]
    PlanSaved {
        session_id: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enriched_serializes_with_kind_tag() {
        let msg = BroadcastMessage::Enriched {
            session_id: "test-123".to_string(),
            records: vec![],
            ephemeral: vec![],
            filter_deltas: HashMap::new(),
            patterns: vec![],
            project_id: None,
            project_name: None,
            session_label: None,
            session_branch: None,
            agent_labels: HashMap::new(),
            total_input_tokens: None,
            total_output_tokens: None,
        };
        let json = serde_json::to_value(&msg).unwrap();
        assert_eq!(json["kind"], "enriched");
        assert_eq!(json["session_id"], "test-123");
    }

    #[test]
    fn view_records_serializes_with_kind_tag() {
        let msg = BroadcastMessage::ViewRecords {
            session_id: "sess-1".to_string(),
            view_records: vec![],
            project_id: Some("proj-a".to_string()),
            project_name: None,
        };
        let json = serde_json::to_value(&msg).unwrap();
        assert_eq!(json["kind"], "view_records");
        assert_eq!(json["project_id"], "proj-a");
    }

    #[test]
    fn plan_saved_serializes_with_kind_tag() {
        let msg = BroadcastMessage::PlanSaved {
            session_id: "sess-2".to_string(),
        };
        let json = serde_json::to_value(&msg).unwrap();
        assert_eq!(json["kind"], "plan_saved");
    }

    #[test]
    fn enriched_skips_empty_optional_fields() {
        let msg = BroadcastMessage::Enriched {
            session_id: "test".to_string(),
            records: vec![],
            ephemeral: vec![],
            filter_deltas: HashMap::new(),
            patterns: vec![],
            project_id: None,
            project_name: None,
            session_label: None,
            session_branch: None,
            agent_labels: HashMap::new(),
            total_input_tokens: None,
            total_output_tokens: None,
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(!json.contains("project_id"));
        assert!(!json.contains("session_label"));
        assert!(!json.contains("agent_labels"));
        assert!(!json.contains("total_input_tokens"));
        assert!(!json.contains("patterns"));
    }

    #[test]
    fn enriched_includes_present_optional_fields() {
        let mut agent_labels = HashMap::new();
        agent_labels.insert("agent-1".to_string(), "Fix the bug".to_string());

        let msg = BroadcastMessage::Enriched {
            session_id: "test".to_string(),
            records: vec![],
            ephemeral: vec![],
            filter_deltas: HashMap::new(),
            patterns: vec![],
            project_id: Some("proj".to_string()),
            project_name: Some("My Project".to_string()),
            session_label: Some("Implement feature X".to_string()),
            session_branch: Some("feature/x".to_string()),
            agent_labels,
            total_input_tokens: Some(1500),
            total_output_tokens: Some(800),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"project_id\":\"proj\""));
        assert!(json.contains("\"session_label\":\"Implement feature X\""));
        assert!(json.contains("\"agent-1\""));
        assert!(json.contains("\"total_input_tokens\":1500"));
    }
}
