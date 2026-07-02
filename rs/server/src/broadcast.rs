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
    /// Admin v0.2: the full topology snapshot is pushed whenever the
    /// admin_broadcaster actor's `compute_topology` produces a different
    /// frame from the cached one. The UI's `admin$` BehaviorSubject
    /// emits each frame as it arrives — UI is a pure sink.
    #[serde(rename = "admin_topology_changed")]
    AdminTopologyChanged {
        topology: crate::admin::Topology,
    },
    /// Agent/operator "view intent": a request to drive the OpenStory UI
    /// (navigate, filter, highlight, present). This is the WRITE side of the
    /// agent-in-UI seam, and it is scoped by design — it only changes what the
    /// dashboard is *showing*, never the observed sources ("drive the mirror,
    /// never the watched"). `params` is free-form so the control vocabulary can
    /// grow without a server change; the UI branches on `action`.
    /// A durable overlay annotation was pinned — pushed to every dashboard so
    /// the note appears live. Overlay namespace only; never an observed event.
    #[serde(rename = "annotation_added")]
    AnnotationAdded {
        annotation: crate::annotations::Annotation,
    },
    /// The user's current view state (the READ half of the agent-in-UI seam):
    /// a projection over the interaction event stream, pushed live so agents
    /// (and other dashboards) can see where the human is. It's the mirror-image
    /// of Control — commands flow in, interactions flow back out.
    #[serde(rename = "ui_state")]
    UiState {
        /// interaction kind (navigate|filter|select|zoom|view). Named
        /// `interaction` to avoid clashing with the enum's "kind" serde tag.
        interaction: String,
        view: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        session_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        filters: Option<serde_json::Value>,
        at: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        issuer: Option<String>,
    },
    #[serde(rename = "control")]
    Control {
        /// The view action, e.g. "open_view" | "highlight" | "present".
        action: String,
        /// Action parameters (route, session_id, filters, note, steps…).
        #[serde(default)]
        params: serde_json::Value,
        /// Who issued this — surfaced in the UI's "driven by X" indicator.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        issuer: Option<String>,
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
    fn ui_state_serializes_with_kind_tag() {
        let msg = BroadcastMessage::UiState {
            interaction: "navigate".into(),
            view: "story".into(),
            session_id: Some("s1".into()),
            filters: None,
            at: "2026-07-02T00:00:00Z".into(),
            issuer: None,
        };
        let json = serde_json::to_value(&msg).unwrap();
        assert_eq!(json["kind"], "ui_state"); // enum tag
        assert_eq!(json["interaction"], "navigate");
        assert_eq!(json["view"], "story");
        assert_eq!(json["session_id"], "s1");
        assert!(json.get("filters").is_none()); // None skipped
    }

    #[test]
    fn control_serializes_with_kind_tag_and_params() {
        let msg = BroadcastMessage::Control {
            action: "open_view".to_string(),
            params: serde_json::json!({ "route": "#/explore/abc123" }),
            issuer: Some("agent:claude".to_string()),
        };
        let json = serde_json::to_value(&msg).unwrap();
        assert_eq!(json["kind"], "control");
        assert_eq!(json["action"], "open_view");
        assert_eq!(json["params"]["route"], "#/explore/abc123");
        assert_eq!(json["issuer"], "agent:claude");
    }

    #[test]
    fn control_omits_absent_issuer() {
        let msg = BroadcastMessage::Control {
            action: "highlight".to_string(),
            params: serde_json::json!({ "sessionIds": ["a", "b"] }),
            issuer: None,
        };
        let json = serde_json::to_value(&msg).unwrap();
        assert_eq!(json["kind"], "control");
        assert!(json.get("issuer").is_none());
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
