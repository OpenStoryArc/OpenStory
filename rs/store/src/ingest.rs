//! Ingest pipeline — pure functions for event classification and transformation.
//!
//! This module contains the stateless parts of the ingest pipeline:
//! - `is_plan_event()` — detect ExitPlanMode tool_use events
//! - `extract_plan_content()` — extract plan text from transcript events
//! - `to_wire_record()` — convert ViewRecord + projection metadata to WireRecord

use serde_json::Value;

use open_story_views::unified::RecordBody;
use open_story_views::view_record::ViewRecord;
use open_story_views::wire_record::{truncate_payload, WireRecord, TRUNCATION_THRESHOLD};

use crate::projection::SessionProjection;

/// Check if a CloudEvent represents a plan event (ExitPlanMode tool_use).
pub fn is_plan_event(event: &Value) -> bool {
    let etype = event.get("type").and_then(|v| v.as_str()).unwrap_or("");
    let subtype = event.get("subtype").and_then(|v| v.as_str()).unwrap_or("");

    // New unified type: io.arc.event with message.assistant.tool_use subtype
    if etype == "io.arc.event" && subtype == "message.assistant.tool_use" {
        let data = match event.get("data") {
            Some(d) => d,
            None => return false,
        };
        // Check agent_payload.tool field (monadic format)
        let ap = data.get("agent_payload").unwrap_or(&Value::Null);
        if ap.get("tool").and_then(|v| v.as_str()) == Some("ExitPlanMode") {
            if let Some(args) = ap.get("args") {
                if let Some(plan) = args.get("plan").and_then(|v| v.as_str()) {
                    if !plan.is_empty() {
                        return true;
                    }
                }
            }
        }
        // Also check raw content blocks (belt-and-suspenders)
        let raw = data.get("raw").unwrap_or(data);
        let message = raw.get("message").unwrap_or(&Value::Null);
        let content = message.get("content").unwrap_or(&Value::Null);
        if let Value::Array(blocks) = content {
            for block in blocks {
                if block.get("type").and_then(|v| v.as_str()) == Some("tool_use")
                    && block.get("name").and_then(|v| v.as_str()) == Some("ExitPlanMode")
                {
                    if let Some(input) = block.get("input") {
                        if let Some(plan) = input.get("plan").and_then(|v| v.as_str()) {
                            if !plan.is_empty() {
                                return true;
                            }
                        }
                    }
                }
            }
        }
        return false;
    }

    // Legacy hook-style (.tool.call)
    if etype.ends_with(".tool.call") {
        let data = match event.get("data") {
            Some(d) => d,
            None => return false,
        };
        let ap = data.get("agent_payload").unwrap_or(data);
        return ap.get("tool").and_then(|v| v.as_str()) == Some("ExitPlanMode")
            && ap
                .get("args")
                .and_then(|a| a.get("plan"))
                .and_then(|v| v.as_str())
                .map(|s| !s.is_empty())
                .unwrap_or(false);
    }
    // Legacy transcript-style (.transcript.assistant)
    if etype.ends_with(".transcript.assistant") {
        let subtype = event.get("subtype").and_then(|v| v.as_str());
        if subtype != Some("tool_use") && subtype != Some("message.assistant.tool_use") {
            return false;
        }
        let data = match event.get("data") {
            Some(d) => d,
            None => return false,
        };
        let raw = data.get("raw").unwrap_or(data);
        let message = raw.get("message").unwrap_or(&Value::Null);
        let content = message.get("content").unwrap_or(&Value::Null);
        if let Value::Array(blocks) = content {
            for block in blocks {
                if block.get("type").and_then(|v| v.as_str()) == Some("tool_use")
                    && block.get("name").and_then(|v| v.as_str()) == Some("ExitPlanMode")
                {
                    if let Some(input) = block.get("input") {
                        if let Some(plan) = input.get("plan").and_then(|v| v.as_str()) {
                            if !plan.is_empty() {
                                return true;
                            }
                        }
                    }
                }
            }
        }
    }
    false
}

/// Extract plan content from a transcript assistant event with ExitPlanMode tool_use.
pub fn extract_plan_content(event: &Value) -> Option<String> {
    let data = event.get("data")?;
    let raw = data.get("raw").unwrap_or(data);
    let message = raw.get("message")?;
    let content = message.get("content")?;
    if let Value::Array(blocks) = content {
        for block in blocks {
            if block.get("type").and_then(|v| v.as_str()) == Some("tool_use")
                && block.get("name").and_then(|v| v.as_str()) == Some("ExitPlanMode")
            {
                if let Some(plan) = block
                    .get("input")
                    .and_then(|i| i.get("plan"))
                    .and_then(|v| v.as_str())
                {
                    if !plan.is_empty() {
                        return Some(plan.to_string());
                    }
                }
            }
        }
    }
    None
}

/// Convert a ViewRecord to a WireRecord using projection metadata for tree info.
/// Truncates large ToolResult outputs at TRUNCATION_THRESHOLD.
pub fn to_wire_record(vr: &ViewRecord, proj: &SessionProjection) -> WireRecord {
    let depth = proj.node_depth(&vr.id);
    let parent_uuid = proj.node_parent(&vr.id).map(|s| s.to_string());

    // Check for truncation on ToolResult output
    let (truncated, payload_bytes) = match &vr.body {
        RecordBody::ToolResult(tr) => {
            if let Some(output) = &tr.output {
                let result = truncate_payload(output, TRUNCATION_THRESHOLD);
                (result.truncated, result.original_bytes as u64)
            } else {
                (false, 0)
            }
        }
        _ => (false, 0),
    };

    WireRecord {
        record: vr.clone(),
        depth,
        parent_uuid,
        truncated,
        payload_bytes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use open_story_views::from_cloud_event::from_cloud_event;
    use serde_json::json;

    // ── is_plan_event ──────────────────────────────────────────────────

    #[test]
    fn is_plan_event_detects_unified_format() {
        let event = json!({
            "type": "io.arc.event",
            "subtype": "message.assistant.tool_use",
            "data": {
                "agent_payload": {
                    "tool": "ExitPlanMode",
                    "args": { "plan": "# My Plan\n\nStep 1..." }
                }
            }
        });
        assert!(is_plan_event(&event));
    }

    #[test]
    fn is_plan_event_rejects_empty_plan() {
        let event = json!({
            "type": "io.arc.event",
            "subtype": "message.assistant.tool_use",
            "data": {
                "agent_payload": {
                    "tool": "ExitPlanMode",
                    "args": { "plan": "" }
                }
            }
        });
        assert!(!is_plan_event(&event));
    }

    #[test]
    fn is_plan_event_rejects_non_plan_tool() {
        let event = json!({
            "type": "io.arc.event",
            "subtype": "message.assistant.tool_use",
            "data": {
                "agent_payload": {
                    "tool": "Bash",
                    "args": { "command": "ls" }
                }
            }
        });
        assert!(!is_plan_event(&event));
    }

    #[test]
    fn is_plan_event_detects_legacy_hook_format() {
        let event = json!({
            "type": "session.tool.call",
            "data": {
                "agent_payload": {
                    "tool": "ExitPlanMode",
                    "args": { "plan": "some plan" }
                }
            }
        });
        assert!(is_plan_event(&event));
    }

    #[test]
    fn is_plan_event_detects_raw_content_blocks() {
        let event = json!({
            "type": "io.arc.event",
            "subtype": "message.assistant.tool_use",
            "data": {
                "raw": {
                    "message": {
                        "content": [{
                            "type": "tool_use",
                            "name": "ExitPlanMode",
                            "input": { "plan": "# Plan from raw blocks" }
                        }]
                    }
                }
            }
        });
        assert!(is_plan_event(&event));
    }

    #[test]
    fn is_plan_event_detects_legacy_transcript_format() {
        let event = json!({
            "type": "session.transcript.assistant",
            "subtype": "tool_use",
            "data": {
                "raw": {
                    "message": {
                        "content": [{
                            "type": "tool_use",
                            "name": "ExitPlanMode",
                            "input": { "plan": "# Legacy plan" }
                        }]
                    }
                }
            }
        });
        assert!(is_plan_event(&event));
    }

    #[test]
    fn is_plan_event_returns_false_for_unrelated_event() {
        let event = json!({
            "type": "io.arc.event",
            "subtype": "message.user.prompt",
            "data": { "agent_payload": { "text": "hello" } }
        });
        assert!(!is_plan_event(&event));
    }

    // ── extract_plan_content ───────────────────────────────────────────

    #[test]
    fn extract_plan_finds_content_in_raw_blocks() {
        let event = json!({
            "data": {
                "raw": {
                    "message": {
                        "content": [{
                            "type": "tool_use",
                            "name": "ExitPlanMode",
                            "input": { "plan": "# Plan content here" }
                        }]
                    }
                }
            }
        });
        assert_eq!(
            extract_plan_content(&event),
            Some("# Plan content here".to_string())
        );
    }

    #[test]
    fn extract_plan_returns_none_for_non_plan_event() {
        let event = json!({
            "data": {
                "raw": {
                    "message": {
                        "content": [{
                            "type": "tool_use",
                            "name": "Bash",
                            "input": { "command": "ls" }
                        }]
                    }
                }
            }
        });
        assert_eq!(extract_plan_content(&event), None);
    }

    #[test]
    fn extract_plan_returns_none_when_no_data() {
        let event = json!({"type": "io.arc.event"});
        assert_eq!(extract_plan_content(&event), None);
    }

    // ── to_wire_record ─────────────────────────────────────────────────

    #[test]
    fn to_wire_record_root_has_depth_zero() {
        let mut proj = SessionProjection::new("test-session");
        let val = json!({
            "id": "evt-root",
            "type": "io.arc.event",
            "subtype": "message.user.prompt",
            "source": "arc://test",
            "time": "2025-01-14T00:00:00Z",
            "data": {
                "agent_payload": { "text": "hello" },
                "raw": {"type": "user", "message": {"content": [{"type": "text", "text": "hello"}]}}
            }
        });
        proj.append(&val);

        let vrs = from_cloud_event(&val);
        assert!(!vrs.is_empty());

        let wire = to_wire_record(&vrs[0], &proj);
        assert_eq!(wire.depth, 0);
        assert_eq!(wire.parent_uuid, None);
        assert!(!wire.truncated);
    }

    #[test]
    fn to_wire_record_child_has_correct_depth() {
        let mut proj = SessionProjection::new("test-session");

        let root = json!({
            "id": "evt-parent",
            "type": "io.arc.event",
            "subtype": "message.user.prompt",
            "source": "arc://test",
            "time": "2025-01-14T00:00:00Z",
            "data": {
                "agent_payload": { "text": "hello" },
                "raw": {"type": "user", "message": {"content": [{"type": "text", "text": "hello"}]}}
            }
        });
        proj.append(&root);

        let child = json!({
            "id": "evt-child",
            "type": "io.arc.event",
            "subtype": "message.assistant.tool_use",
            "source": "arc://test",
            "time": "2025-01-14T00:00:01Z",
            "data": {
                "agent_payload": {
                    "parent_uuid": "evt-parent",
                    "tool": "Bash",
                    "args": {"command": "ls"}
                },
                "raw": {"type": "assistant", "message": {"model": "claude-4", "content": [
                    {"type": "tool_use", "id": "toolu_1", "name": "Bash", "input": {"command": "ls"}}
                ]}}
            }
        });
        proj.append(&child);

        let vrs = from_cloud_event(&child);
        let wire = to_wire_record(&vrs[0], &proj);
        assert_eq!(wire.depth, 1);
        assert_eq!(wire.parent_uuid, Some("evt-parent".to_string()));
    }

    #[test]
    fn to_wire_record_truncates_large_tool_result() {
        use open_story_views::unified::ToolResult;

        let proj = SessionProjection::new("test-session");
        let large_output = "y".repeat(TRUNCATION_THRESHOLD + 100);

        let vr = ViewRecord {
            id: "evt-large".to_string(),
            seq: 1,
            session_id: "test-session".to_string(),
            timestamp: "2025-01-14T00:00:00Z".to_string(),
            agent_id: None,
            is_sidechain: false,
            body: RecordBody::ToolResult(ToolResult {
                call_id: "toolu_big".to_string(),
                output: Some(large_output),
                is_error: false,
                tool_outcome: None,
            }),
        };

        let wire = to_wire_record(&vr, &proj);
        assert!(wire.truncated);
        assert_eq!(wire.payload_bytes as usize, TRUNCATION_THRESHOLD + 100);
    }

    #[test]
    fn to_wire_record_does_not_truncate_small_output() {
        use open_story_views::unified::ToolResult;

        let proj = SessionProjection::new("test-session");
        let vr = ViewRecord {
            id: "evt-small".to_string(),
            seq: 1,
            session_id: "test-session".to_string(),
            timestamp: "2025-01-14T00:00:00Z".to_string(),
            agent_id: None,
            is_sidechain: false,
            body: RecordBody::ToolResult(ToolResult {
                call_id: "toolu_sm".to_string(),
                output: Some("small output".to_string()),
                is_error: false,
                tool_outcome: None,
            }),
        };

        let wire = to_wire_record(&vr, &proj);
        assert!(!wire.truncated);
        assert_eq!(wire.payload_bytes, 12);
    }

    #[test]
    fn to_wire_record_non_tool_result_not_truncated() {
        use open_story_views::unified::{MessageContent, UserMessage};

        let proj = SessionProjection::new("test-session");
        let vr = ViewRecord {
            id: "evt-user".to_string(),
            seq: 1,
            session_id: "test-session".to_string(),
            timestamp: "2025-01-14T00:00:00Z".to_string(),
            agent_id: None,
            is_sidechain: false,
            body: RecordBody::UserMessage(UserMessage {
                content: MessageContent::Text("hello".to_string()),
                images: vec![],
            }),
        };

        let wire = to_wire_record(&vr, &proj);
        assert!(!wire.truncated);
        assert_eq!(wire.payload_bytes, 0);
    }
}
