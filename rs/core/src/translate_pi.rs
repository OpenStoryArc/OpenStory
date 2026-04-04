//! Pure translation: pi-mono session JSONL → CloudEvent(s).
//!
//! Pi-mono is a TypeScript coding agent that stores sessions as JSONL files.
//! This module translates those entries into the same CloudEvent 1.0 format
//! used by the Claude Code translator, enabling Open Story to observe any agent.
//!
//! Key differences from Claude Code format:
//! - Pi-mono uses `type: "message"` for all messages; role is inside `message.role`
//! - Content blocks use `toolCall` (camelCase) instead of `tool_use` (snake_case)
//! - Tree structure uses `id`/`parentId` instead of `uuid`/`parentUuid`
//! - Session header is a separate entry type with provider/model metadata

use serde_json::Value;

use crate::cloud_event::CloudEvent;
use crate::event_data::{AgentPayload, EventData, PiMonoPayload};
use crate::translate::{TranscriptState, IO_ARC_EVENT};

/// Returns true if the entry type is one we know how to translate.
fn is_pi_known_type(entry_type: &str) -> bool {
    matches!(
        entry_type,
        "session"
            | "message"
            | "compaction"
            | "model_change"
            | "thinking_level_change"
            | "branch_summary"
            | "label"
            | "custom"
            | "custom_message"
            | "session_info"
    )
}

/// Extract envelope fields from a pi-mono entry into a PiMonoPayload.
/// Maps pi-mono field names (id → uuid, parentId → parent_uuid, cwd).
fn apply_pi_envelope(payload: &mut PiMonoPayload, line: &Value) {
    if let Value::Object(obj) = line {
        // id → uuid (for dedup compatibility)
        if let Some(v) = obj.get("id").and_then(|v| v.as_str()) {
            payload.uuid = Some(v.to_string());
        }
        // parentId → parent_uuid
        if let Some(v) = obj.get("parentId").and_then(|v| v.as_str()) {
            payload.parent_uuid = Some(v.to_string());
        }
        // cwd (from session header)
        if let Some(v) = obj.get("cwd").and_then(|v| v.as_str()) {
            payload.cwd = Some(v.to_string());
        }
    }
}

/// Determine assistant subtype from content blocks.
/// Pi-mono uses "toolCall" (camelCase) for tool use blocks.
fn determine_pi_assistant_subtype(content: &Value) -> &'static str {
    if let Value::Array(blocks) = content {
        let mut has_thinking = false;
        let mut has_tool_call = false;
        for block in blocks {
            if let Some(bt) = block.get("type").and_then(|v| v.as_str()) {
                match bt {
                    "thinking" => has_thinking = true,
                    "toolCall" => has_tool_call = true,
                    _ => {}
                }
            }
        }
        if has_tool_call {
            return "message.assistant.tool_use";
        }
        if has_thinking {
            return "message.assistant.thinking";
        }
    }
    "message.assistant.text"
}

/// Extract text from message content (string or array of blocks).
fn extract_text_from_content(content: &Value) -> Option<String> {
    match content {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::Array(blocks) => {
            for block in blocks {
                if block.get("type").and_then(|v| v.as_str()) == Some("text") {
                    if let Some(t) = block.get("text").and_then(|v| v.as_str()) {
                        if !t.is_empty() {
                            return Some(t.to_string());
                        }
                    }
                }
            }
            None
        }
        _ => None,
    }
}

/// Extract first tool name and input from content blocks.
fn extract_first_tool_from_content(content: &Value) -> Option<(String, Value)> {
    if let Value::Array(blocks) = content {
        for block in blocks {
            if block.get("type").and_then(|v| v.as_str()) == Some("toolCall") {
                let name = block
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown")
                    .to_string();
                let input = block
                    .get("arguments")
                    .cloned()
                    .unwrap_or(Value::Object(Default::default()));
                return Some((name, input));
            }
        }
    }
    None
}

/// Detect whether a JSONL line is pi-mono format.
///
/// Pi-mono signals: `type: "session"` with `cwd`, `type: "message"` with
/// nested `message.role`, or pi-mono-specific metadata types.
pub fn is_pi_mono_format(line: &Value) -> bool {
    let entry_type = line.get("type").and_then(|v| v.as_str()).unwrap_or("");
    if entry_type == "session" && line.get("cwd").is_some() {
        return true;
    }
    if entry_type == "message" {
        if let Some(msg) = line.get("message") {
            if msg.get("role").is_some() {
                return true;
            }
        }
    }
    matches!(
        entry_type,
        "model_change"
            | "compaction"
            | "thinking_level_change"
            | "branch_summary"
            | "label"
            | "custom_message"
            | "session_info"
    )
}

/// Extract message-specific fields into the payload. Returns subtype on success.
fn apply_message_fields(
    payload: &mut PiMonoPayload,
    line: &Value,
) -> Option<String> {
    let message = line.get("message")?;
    let role = message.get("role").and_then(|v| v.as_str())?;
    let content = message
        .get("content")
        .cloned()
        .unwrap_or(Value::Array(vec![]));

    let subtype = match role {
        "user" => {
            if let Some(text) = extract_text_from_content(&content) {
                payload.text = Some(text);
            }
            "message.user.prompt".to_string()
        }
        "assistant" => {
            let st = determine_pi_assistant_subtype(&content);
            if let Some(text) = extract_text_from_content(&content) {
                payload.text = Some(text);
            }
            if st == "message.assistant.tool_use" {
                if let Some((tool_name, tool_args)) = extract_first_tool_from_content(&content) {
                    payload.tool = Some(tool_name);
                    payload.args = Some(tool_args);
                }
            }
            if let Some(v) = message.get("model").and_then(|v| v.as_str()) {
                payload.model = Some(v.to_string());
            }
            if let Some(v) = message.get("stopReason").and_then(|v| v.as_str()) {
                payload.stop_reason = Some(v.to_string());
            }
            if let Some(v) = message.get("usage") {
                // Pass through pi-mono's native usage keys (input, output, totalTokens, etc.)
                // The views layer branches on agent to parse format-specific fields.
                payload.token_usage = Some(v.clone());
            }
            // Content types for downstream
            let content_types: Vec<String> = if let Value::Array(ref blocks) = content {
                blocks
                    .iter()
                    .filter_map(|b| b.get("type").and_then(|v| v.as_str()).map(|s| s.to_string()))
                    .collect()
            } else {
                vec![]
            };
            if !content_types.is_empty() {
                payload.content_types = Some(content_types);
            }
            st.to_string()
        }
        "toolResult" => {
            if let Some(v) = message.get("toolCallId").and_then(|v| v.as_str()) {
                payload.tool_call_id = Some(v.to_string());
            }
            if let Some(v) = message.get("toolName").and_then(|v| v.as_str()) {
                payload.tool_name = Some(v.to_string());
            }
            if let Some(v) = message.get("isError").and_then(|v| v.as_bool()) {
                payload.is_error = Some(v);
            }
            "message.user.tool_result".to_string()
        }
        "bashExecution" => {
            if let Some(v) = message.get("command").and_then(|v| v.as_str()) {
                payload.command = Some(v.to_string());
            }
            if let Some(v) = message.get("exitCode") {
                payload.exit_code = Some(v.clone());
            }
            if let Some(v) = message.get("output").and_then(|v| v.as_str()) {
                payload.output = Some(v.to_string());
            }
            "progress.bash".to_string()
        }
        "compactionSummary" => {
            if let Some(v) = message.get("summary").and_then(|v| v.as_str()) {
                payload.summary = Some(v.to_string());
            }
            "system.compact".to_string()
        }
        // Skip roles we don't handle in the spike
        "branchSummary" | "custom" => return None,
        _ => return None,
    };

    Some(subtype)
}

/// Pure function: translate one pi-mono JSONL line into CloudEvent(s).
///
/// Returns zero events for unknown types, duplicate IDs, or skipped roles.
/// All events are produced with type `io.arc.event` and hierarchical subtypes.
pub fn translate_pi_line(line: &Value, state: &mut TranscriptState) -> Vec<CloudEvent> {
    let entry_type = match line.get("type").and_then(|v| v.as_str()) {
        Some(t) => t,
        None => return vec![],
    };

    if !is_pi_known_type(entry_type) {
        return vec![];
    }

    // Deduplication by entry id
    let entry_id = line.get("id").and_then(|v| v.as_str()).map(|s| s.to_string());
    if let Some(ref id) = entry_id {
        if state.seen_uuids.contains(id) {
            return vec![];
        }
        state.seen_uuids.insert(id.clone());
    }

    let source = format!("pi://session/{}", state.session_id);
    let mut payload = PiMonoPayload::new();

    // Apply envelope fields (id → uuid, parentId → parent_uuid, cwd)
    apply_pi_envelope(&mut payload, line);

    let subtype: Option<String> = match entry_type {
        "session" => {
            if let Some(v) = line.get("provider").and_then(|v| v.as_str()) {
                payload.provider = Some(v.to_string());
            }
            if let Some(v) = line.get("modelId").and_then(|v| v.as_str()) {
                payload.model = Some(v.to_string());
            }
            if let Some(v) = line.get("thinkingLevel").and_then(|v| v.as_str()) {
                payload.thinking_level = Some(v.to_string());
            }
            if let Some(v) = line.get("version") {
                payload.version = Some(v.clone());
            }
            Some("system.session_start".to_string())
        }
        "message" => {
            match apply_message_fields(&mut payload, line) {
                Some(st) => Some(st),
                None => return vec![], // Unknown or skipped role
            }
        }
        "compaction" => {
            if let Some(v) = line.get("summary").and_then(|v| v.as_str()) {
                payload.summary = Some(v.to_string());
            }
            if let Some(v) = line.get("tokensBefore").and_then(|v| v.as_u64()) {
                payload.tokens_before = Some(v);
            }
            if let Some(v) = line.get("firstKeptEntryId").and_then(|v| v.as_str()) {
                payload.first_kept_entry_id = Some(v.to_string());
            }
            Some("system.compact".to_string())
        }
        "model_change" => {
            if let Some(v) = line.get("provider").and_then(|v| v.as_str()) {
                payload.provider = Some(v.to_string());
            }
            if let Some(v) = line.get("modelId").and_then(|v| v.as_str()) {
                payload.model = Some(v.to_string());
            }
            Some("system.model_change".to_string())
        }
        // Spike: skip these entry types
        "thinking_level_change" | "branch_summary" | "label" | "custom" | "custom_message"
        | "session_info" => {
            return vec![];
        }
        _ => return vec![],
    };

    // Build EventData with typed payload
    let data = EventData::with_payload(
        line.clone(),
        state.next_seq(),
        state.session_id.clone(),
        AgentPayload::PiMono(payload),
    );

    let timestamp = line
        .get("timestamp")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    vec![CloudEvent::new(
        source,
        IO_ARC_EVENT.to_string(),
        serde_json::to_value(&data).expect("EventData serialization cannot fail"),
        subtype,
        entry_id,
        timestamp,
        None,
        None,
        Some("pi-mono".to_string()),
    )]
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn state() -> TranscriptState {
        TranscriptState::new("test-session".to_string())
    }

    /// Helper: extract PiMonoPayload from event data, panicking if not present.
    fn pi_payload(event: &CloudEvent) -> PiMonoPayload {
        let event_data: EventData =
            serde_json::from_value(event.data.clone()).expect("data should deserialize to EventData");
        match event_data.agent_payload.expect("agent_payload should be Some") {
            AgentPayload::PiMono(pm) => pm,
            _ => panic!("expected PiMono payload"),
        }
    }

    /// Helper: extract EventData from event.
    fn event_data(event: &CloudEvent) -> EventData {
        serde_json::from_value(event.data.clone()).expect("data should deserialize to EventData")
    }

    // ── Boundary table: subtype mapping ──────────────────────

    #[test]
    fn test_subtype_boundary_table() {
        let cases: Vec<(&str, Value, &str)> = vec![
            // (description, input line, expected subtype)
            (
                "session header → system.session_start",
                json!({
                    "type": "session", "id": "sess-1",
                    "timestamp": "2025-01-01T00:00:00Z",
                    "cwd": "/work", "provider": "anthropic",
                    "modelId": "claude-sonnet-4-5", "version": 3,
                }),
                "system.session_start",
            ),
            (
                "user message → message.user.prompt",
                json!({
                    "type": "message",
                    "timestamp": "2025-01-01T00:00:01Z",
                    "message": {
                        "role": "user",
                        "content": [{"type": "text", "text": "hello"}],
                        "timestamp": 1234567890,
                    },
                }),
                "message.user.prompt",
            ),
            (
                "assistant text → message.assistant.text",
                json!({
                    "type": "message",
                    "timestamp": "2025-01-01T00:00:02Z",
                    "message": {
                        "role": "assistant",
                        "content": [{"type": "text", "text": "response"}],
                        "model": "claude-sonnet-4-5",
                        "usage": {"input": 10, "output": 5},
                        "stopReason": "stop",
                        "timestamp": 1234567891,
                    },
                }),
                "message.assistant.text",
            ),
            (
                "assistant toolCall → message.assistant.tool_use",
                json!({
                    "type": "message",
                    "timestamp": "2025-01-01T00:00:03Z",
                    "message": {
                        "role": "assistant",
                        "content": [
                            {"type": "text", "text": "reading file"},
                            {"type": "toolCall", "id": "tc-1", "name": "read",
                             "arguments": {"path": "/foo"}},
                        ],
                        "model": "claude-sonnet-4-5",
                        "stopReason": "toolUse",
                        "timestamp": 1234567892,
                    },
                }),
                "message.assistant.tool_use",
            ),
            (
                "assistant thinking → message.assistant.thinking",
                json!({
                    "type": "message",
                    "timestamp": "2025-01-01T00:00:04Z",
                    "message": {
                        "role": "assistant",
                        "content": [{"type": "thinking", "thinking": "hmm..."}],
                        "model": "claude-sonnet-4-5",
                        "stopReason": "stop",
                        "timestamp": 1234567893,
                    },
                }),
                "message.assistant.thinking",
            ),
            (
                "toolResult → message.user.tool_result",
                json!({
                    "type": "message",
                    "timestamp": "2025-01-01T00:00:05Z",
                    "message": {
                        "role": "toolResult",
                        "toolCallId": "tc-1", "toolName": "read",
                        "content": [{"type": "text", "text": "file contents"}],
                        "isError": false, "timestamp": 1234567894,
                    },
                }),
                "message.user.tool_result",
            ),
            (
                "bashExecution → progress.bash",
                json!({
                    "type": "message",
                    "timestamp": "2025-01-01T00:00:06Z",
                    "message": {
                        "role": "bashExecution",
                        "command": "cargo test",
                        "output": "ok",
                        "exitCode": 0,
                        "cancelled": false,
                        "truncated": false,
                        "timestamp": 1234567895,
                    },
                }),
                "progress.bash",
            ),
            (
                "compaction → system.compact",
                json!({
                    "type": "compaction", "id": "comp-1",
                    "timestamp": "2025-01-01T00:00:07Z",
                    "summary": "did stuff",
                    "firstKeptEntryId": "msg-3",
                    "tokensBefore": 50000,
                }),
                "system.compact",
            ),
            (
                "model_change → system.model_change",
                json!({
                    "type": "model_change",
                    "timestamp": "2025-01-01T00:00:08Z",
                    "provider": "openai", "modelId": "gpt-4o",
                }),
                "system.model_change",
            ),
        ];

        for (desc, input, expected_subtype) in cases {
            let mut s = state();
            let events = translate_pi_line(&input, &mut s);
            assert_eq!(events.len(), 1, "{desc}: expected 1 event");
            assert_eq!(
                events[0].subtype.as_deref(),
                Some(expected_subtype),
                "{desc}: wrong subtype"
            );
            assert_eq!(events[0].event_type, IO_ARC_EVENT, "{desc}: wrong event type");
            assert!(
                events[0].source.starts_with("pi://session/"),
                "{desc}: wrong source prefix"
            );
        }
    }

    // ── Field extraction tests ───────────────────────────────

    #[test]
    fn test_user_message_text_extraction() {
        let line = json!({
            "type": "message",
            "timestamp": "2025-01-01T00:00:00Z",
            "message": {
                "role": "user",
                "content": [{"type": "text", "text": "hello world"}],
                "timestamp": 1234567890,
            },
        });
        let events = translate_pi_line(&line, &mut state());
        let pm = pi_payload(&events[0]);
        assert_eq!(pm.text.as_deref(), Some("hello world"));
    }

    #[test]
    fn test_assistant_tool_extraction() {
        let line = json!({
            "type": "message",
            "timestamp": "2025-01-01T00:00:00Z",
            "message": {
                "role": "assistant",
                "content": [
                    {"type": "toolCall", "id": "tc-1", "name": "read",
                     "arguments": {"path": "/config.toml"}},
                ],
                "model": "claude-sonnet-4-5",
                "stopReason": "toolUse",
                "timestamp": 1234567890,
            },
        });
        let events = translate_pi_line(&line, &mut state());
        let pm = pi_payload(&events[0]);
        assert_eq!(pm.tool.as_deref(), Some("read"));
        assert_eq!(pm.args.as_ref().unwrap()["path"], "/config.toml");
        assert_eq!(pm.model.as_deref(), Some("claude-sonnet-4-5"));
        assert_eq!(pm.stop_reason.as_deref(), Some("toolUse"));
    }

    #[test]
    fn test_token_usage_preserved_native() {
        let line = json!({
            "type": "message",
            "timestamp": "2025-01-01T00:00:00Z",
            "message": {
                "role": "assistant",
                "content": [{"type": "text", "text": "done"}],
                "model": "claude-sonnet-4-5",
                "usage": {
                    "input": 150,
                    "output": 75,
                    "cacheRead": 1000,
                    "cacheWrite": 200,
                    "totalTokens": 1425,
                    "cost": {"input": 0.00045, "output": 0.000375, "total": 0.000825}
                },
                "stopReason": "stop",
                "timestamp": 1234567890,
            },
        });
        let events = translate_pi_line(&line, &mut state());
        let pm = pi_payload(&events[0]);
        let usage = pm.token_usage.as_ref().expect("token_usage should be set");
        // Native pi-mono keys — no normalization
        assert_eq!(usage["input"], 150, "native pi-mono key: input");
        assert_eq!(usage["output"], 75, "native pi-mono key: output");
        assert_eq!(usage["totalTokens"], 1425, "native pi-mono key: totalTokens");
        assert_eq!(usage["cacheRead"], 1000, "native pi-mono key: cacheRead");
        assert_eq!(usage["cacheWrite"], 200, "native pi-mono key: cacheWrite");
        assert!(usage["cost"].is_object(), "cost preserved");
    }

    #[test]
    fn test_raw_is_untouched_for_tool_call() {
        let line = json!({
            "type": "message",
            "timestamp": "2025-01-01T00:00:00Z",
            "message": {
                "role": "assistant",
                "content": [
                    {"type": "toolCall", "id": "tc-1", "name": "read",
                     "arguments": {"path": "/foo"}},
                ],
                "model": "claude-sonnet-4-5",
                "stopReason": "toolUse",
                "timestamp": 1234567890,
            },
        });
        let events = translate_pi_line(&line, &mut state());
        let ed = event_data(&events[0]);
        let raw_content = &ed.raw["message"]["content"];
        // Raw preserves pi-mono's native format — toolCall, not tool_use
        assert_eq!(raw_content[0]["type"], "toolCall", "raw should preserve toolCall type");
        assert_eq!(raw_content[0]["arguments"]["path"], "/foo", "raw should preserve arguments key");
    }

    #[test]
    fn test_tool_result_fields() {
        let line = json!({
            "type": "message",
            "timestamp": "2025-01-01T00:00:00Z",
            "message": {
                "role": "toolResult",
                "toolCallId": "tc-1", "toolName": "read",
                "content": [{"type": "text", "text": "output"}],
                "isError": false, "timestamp": 1234567890,
            },
        });
        let events = translate_pi_line(&line, &mut state());
        let pm = pi_payload(&events[0]);
        assert_eq!(pm.tool_name.as_deref(), Some("read"));
        assert_eq!(pm.tool_call_id.as_deref(), Some("tc-1"));
        assert_eq!(pm.is_error, Some(false));
    }

    #[test]
    fn test_raw_is_untouched_for_tool_result() {
        let line = json!({
            "type": "message",
            "timestamp": "2025-01-01T00:00:00Z",
            "message": {
                "role": "toolResult",
                "toolCallId": "tc-1", "toolName": "read",
                "content": [{"type": "text", "text": "file contents here"}],
                "isError": false, "timestamp": 1234567890,
            },
        });
        let events = translate_pi_line(&line, &mut state());
        let ed = event_data(&events[0]);
        // Raw preserves pi-mono's native format — not normalized
        let raw_msg = &ed.raw["message"];
        assert_eq!(raw_msg["role"], "toolResult", "raw should preserve toolResult role");
        assert_eq!(raw_msg["toolCallId"], "tc-1", "raw should preserve toolCallId");
        assert_eq!(raw_msg["toolName"], "read", "raw should preserve toolName");
        assert_eq!(raw_msg["content"][0]["type"], "text", "raw content stays as text blocks");
        assert_eq!(raw_msg["content"][0]["text"], "file contents here");
    }

    #[test]
    fn test_session_header_fields() {
        let line = json!({
            "type": "session", "id": "sess-1",
            "timestamp": "2025-01-01T00:00:00Z",
            "cwd": "/work/project", "provider": "anthropic",
            "modelId": "claude-sonnet-4-5", "thinkingLevel": "off",
            "version": 3,
        });
        let events = translate_pi_line(&line, &mut state());
        let pm = pi_payload(&events[0]);
        assert_eq!(pm.provider.as_deref(), Some("anthropic"));
        assert_eq!(pm.model.as_deref(), Some("claude-sonnet-4-5"));
        assert_eq!(pm.thinking_level.as_deref(), Some("off"));
        assert_eq!(pm.version, Some(json!(3)));
        assert_eq!(pm.cwd.as_deref(), Some("/work/project"));
    }

    #[test]
    fn test_compaction_fields() {
        let line = json!({
            "type": "compaction", "id": "comp-1",
            "parentId": "msg-5",
            "timestamp": "2025-01-01T00:00:00Z",
            "summary": "did stuff",
            "firstKeptEntryId": "msg-3",
            "tokensBefore": 50000,
        });
        let events = translate_pi_line(&line, &mut state());
        let pm = pi_payload(&events[0]);
        assert_eq!(pm.summary.as_deref(), Some("did stuff"));
        assert_eq!(pm.tokens_before, Some(50000));
        assert_eq!(pm.first_kept_entry_id.as_deref(), Some("msg-3"));
        assert_eq!(pm.parent_uuid.as_deref(), Some("msg-5"));
    }

    #[test]
    fn test_model_change_fields() {
        let line = json!({
            "type": "model_change",
            "timestamp": "2025-01-01T00:00:00Z",
            "provider": "openai", "modelId": "gpt-4o",
        });
        let events = translate_pi_line(&line, &mut state());
        let pm = pi_payload(&events[0]);
        assert_eq!(pm.provider.as_deref(), Some("openai"));
        assert_eq!(pm.model.as_deref(), Some("gpt-4o"));
    }

    #[test]
    fn test_bash_execution_fields() {
        let line = json!({
            "type": "message",
            "timestamp": "2025-01-01T00:00:00Z",
            "message": {
                "role": "bashExecution",
                "command": "cargo test",
                "output": "42 passed",
                "exitCode": 0,
                "cancelled": false,
                "truncated": false,
                "timestamp": 1234567890,
            },
        });
        let events = translate_pi_line(&line, &mut state());
        let pm = pi_payload(&events[0]);
        assert_eq!(pm.command.as_deref(), Some("cargo test"));
        assert_eq!(pm.exit_code, Some(json!(0)));
        assert_eq!(pm.output.as_deref(), Some("42 passed"));
    }

    // ── Edge cases ───────────────────────────────────────────

    #[test]
    fn test_unknown_type_produces_no_events() {
        let line = json!({"type": "foobar"});
        let events = translate_pi_line(&line, &mut state());
        assert!(events.is_empty());
    }

    #[test]
    fn test_missing_type_produces_no_events() {
        let line = json!({"data": "something"});
        let events = translate_pi_line(&line, &mut state());
        assert!(events.is_empty());
    }

    #[test]
    fn test_unknown_role_produces_no_events() {
        let line = json!({
            "type": "message",
            "timestamp": "2025-01-01T00:00:00Z",
            "message": {"role": "totally_unknown"},
        });
        let events = translate_pi_line(&line, &mut state());
        assert!(events.is_empty());
    }

    #[test]
    fn test_duplicate_id_dedup() {
        let line = json!({
            "type": "compaction", "id": "dup-1",
            "timestamp": "2025-01-01T00:00:00Z",
            "summary": "first", "firstKeptEntryId": "a", "tokensBefore": 100,
        });
        let mut s = state();
        let first = translate_pi_line(&line, &mut s);
        let second = translate_pi_line(&line, &mut s);
        assert_eq!(first.len(), 1, "first should produce event");
        assert_eq!(second.len(), 0, "duplicate should be deduped");
    }

    #[test]
    fn test_entry_without_id_not_deduped() {
        // Pi-mono v1 entries may not have id/parentId
        let line = json!({
            "type": "message",
            "timestamp": "2025-01-01T00:00:00Z",
            "message": {
                "role": "user",
                "content": [{"type": "text", "text": "hello"}],
                "timestamp": 1234567890,
            },
        });
        let mut s = state();
        let first = translate_pi_line(&line, &mut s);
        let second = translate_pi_line(&line, &mut s);
        assert_eq!(first.len(), 1);
        assert_eq!(second.len(), 1, "entries without id should not be deduped");
    }

    #[test]
    fn test_skipped_types_produce_no_events() {
        let skipped = vec![
            "thinking_level_change",
            "branch_summary",
            "label",
            "custom",
            "custom_message",
            "session_info",
        ];
        for entry_type in skipped {
            let line = json!({"type": entry_type, "timestamp": "2025-01-01T00:00:00Z"});
            let events = translate_pi_line(&line, &mut state());
            assert!(events.is_empty(), "{entry_type} should produce no events");
        }
    }

    #[test]
    fn test_seq_increments_across_events() {
        let mut s = state();
        let line1 = json!({
            "type": "message",
            "timestamp": "2025-01-01T00:00:00Z",
            "message": {"role": "user", "content": "a", "timestamp": 1},
        });
        let line2 = json!({
            "type": "message",
            "timestamp": "2025-01-01T00:00:01Z",
            "message": {"role": "user", "content": "b", "timestamp": 2},
        });
        let e1 = translate_pi_line(&line1, &mut s);
        let e2 = translate_pi_line(&line2, &mut s);
        let ed1 = event_data(&e1[0]);
        let ed2 = event_data(&e2[0]);
        assert_eq!(ed1.seq, 1);
        assert_eq!(ed2.seq, 2);
    }

    #[test]
    fn test_agent_field_set_to_pi_mono() {
        let line = json!({
            "type": "message",
            "timestamp": "2025-01-01T00:00:00Z",
            "message": {"role": "user", "content": "hello", "timestamp": 1},
        });
        let events = translate_pi_line(&line, &mut state());
        assert_eq!(events[0].agent.as_deref(), Some("pi-mono"));
    }

    #[test]
    fn test_source_format() {
        let line = json!({
            "type": "message",
            "timestamp": "2025-01-01T00:00:00Z",
            "message": {"role": "user", "content": "hello", "timestamp": 1},
        });
        let events = translate_pi_line(&line, &mut state());
        assert_eq!(events[0].source, "pi://session/test-session");
    }

    #[test]
    fn test_payload_meta_agent_tag() {
        let line = json!({
            "type": "message",
            "timestamp": "2025-01-01T00:00:00Z",
            "message": {"role": "user", "content": "hello", "timestamp": 1},
        });
        let events = translate_pi_line(&line, &mut state());
        let pm = pi_payload(&events[0]);
        assert_eq!(pm.meta.agent, "pi-mono");
    }

    #[test]
    fn test_event_data_has_raw_and_session_id() {
        let line = json!({
            "type": "message",
            "timestamp": "2025-01-01T00:00:00Z",
            "message": {"role": "user", "content": "hello", "timestamp": 1},
        });
        let events = translate_pi_line(&line, &mut state());
        let ed = event_data(&events[0]);
        assert_eq!(ed.session_id, "test-session");
        assert_eq!(ed.raw["type"], "message");
    }

    // ── Format detection ─────────────────────────────────────

    #[test]
    fn test_format_detection_boundary_table() {
        let cases: Vec<(&str, Value, bool)> = vec![
            ("session header", json!({"type": "session", "cwd": "/foo"}), true),
            (
                "message with role",
                json!({"type": "message", "message": {"role": "user"}}),
                true,
            ),
            ("model_change", json!({"type": "model_change"}), true),
            ("compaction", json!({"type": "compaction"}), true),
            (
                "claude assistant",
                json!({"type": "assistant", "message": {"role": "assistant"}}),
                false,
            ),
            ("claude user", json!({"type": "user"}), false),
            ("empty", json!({}), false),
        ];

        for (desc, input, expected) in cases {
            assert_eq!(is_pi_mono_format(&input), expected, "{desc}");
        }
    }
}
