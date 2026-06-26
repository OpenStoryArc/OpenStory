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
#[derive(Debug, Clone, Serialize, serde::Deserialize, schemars::JsonSchema)]
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
        /// Accumulated input tokens for this session (sent when changed).
        #[serde(skip_serializing_if = "Option::is_none")]
        total_input_tokens: Option<u64>,
        /// Accumulated output tokens for this session (sent when changed).
        #[serde(skip_serializing_if = "Option::is_none")]
        total_output_tokens: Option<u64>,
    },
    #[serde(rename = "plan_saved")]
    PlanSaved { session_id: String },
    /// Agent-directed watch focus: emitted when `POST /api/watch/{session_id}`
    /// is called. An already-live UI reacts context-aware — instant focus
    /// switch on the Live tab, a dismissible "Follow" banner elsewhere. This
    /// rides the existing broadcast channel; no new transport. Enrichment
    /// fields are looked up from the session row so the UI can render a
    /// human-readable banner without a follow-up fetch.
    #[serde(rename = "focus")]
    Focus {
        session_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        label: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        project_name: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        host: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        user: Option<String>,
    },
    /// Admin v0.2: the full topology snapshot is pushed whenever the
    /// admin_broadcaster actor's `compute_topology` produces a different
    /// frame from the cached one. The UI's `admin$` BehaviorSubject
    /// emits each frame as it arrives — UI is a pure sink.
    #[serde(rename = "admin_topology_changed")]
    AdminTopologyChanged {
        topology: crate::admin::Topology,
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
            total_input_tokens: None,
            total_output_tokens: None,
        };
        let json = serde_json::to_value(&msg).unwrap();
        assert_eq!(json["kind"], "enriched");
        assert_eq!(json["session_id"], "test-123");
    }

    #[test]
    fn focus_serializes_with_kind_tag_and_omits_none() {
        // The UI's WsMessage union keys on `kind` and reads snake_case
        // enrichment fields; absent enrichment must be omitted, not null.
        let msg = BroadcastMessage::Focus {
            session_id: "sess-abc".to_string(),
            label: Some("Where are we at?".to_string()),
            project_name: Some("agent-harness".to_string()),
            host: Some("a1".to_string()),
            user: None,
        };
        let json = serde_json::to_value(&msg).unwrap();
        assert_eq!(json["kind"], "focus");
        assert_eq!(json["session_id"], "sess-abc");
        assert_eq!(json["label"], "Where are we at?");
        assert_eq!(json["project_name"], "agent-harness");
        assert_eq!(json["host"], "a1");
        assert!(
            json.get("user").is_none(),
            "None enrichment must be omitted, not serialized as null"
        );
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
            total_input_tokens: None,
            total_output_tokens: None,
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(!json.contains("project_id"));
        assert!(!json.contains("session_label"));
        assert!(!json.contains("total_input_tokens"));
        assert!(!json.contains("patterns"));
    }

    #[test]
    fn enriched_includes_present_optional_fields() {
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
            total_input_tokens: Some(1500),
            total_output_tokens: Some(800),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"project_id\":\"proj\""));
        assert!(json.contains("\"session_label\":\"Implement feature X\""));
        assert!(json.contains("\"total_input_tokens\":1500"));
    }
}
