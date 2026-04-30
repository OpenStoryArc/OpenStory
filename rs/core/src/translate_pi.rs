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
use uuid::Uuid;

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

/// Derive a deterministic event ID for a decomposed content block.
/// Uses UUID5 so the same input always produces the same ID.
fn derive_block_event_id(session_id: &str, entry_id: &str, block_index: usize, subtype: &str) -> String {
    let seed = format!("pi-mono:{}:{}:{}:{}", session_id, entry_id, block_index, subtype);
    Uuid::new_v5(&Uuid::NAMESPACE_URL, seed.as_bytes()).to_string()
}

/// Decompose a pi-mono assistant message into one CloudEvent per content block.
///
/// A line with content: [thinking, text, toolCall] produces 3 CloudEvents.
/// Each event shares the same raw data (the full bundled line).
/// Token usage is attached to the last event only.
fn decompose_assistant(
    line: &Value,
    state: &mut TranscriptState,
    source: &str,
    entry_id: &Option<String>,
) -> Vec<CloudEvent> {
    let message = match line.get("message") {
        Some(m) => m,
        None => return vec![],
    };
    let content = match message.get("content") {
        Some(Value::Array(blocks)) => blocks,
        _ => return vec![],
    };

    let model = message.get("model").and_then(|v| v.as_str()).map(|s| s.to_string());
    let stop_reason = message.get("stopReason").and_then(|v| v.as_str()).map(|s| s.to_string());
    let usage = message.get("usage").cloned();
    let timestamp = line.get("timestamp").and_then(|v| v.as_str()).map(|s| s.to_string());
    let eid_str = entry_id.as_deref().unwrap_or("");

    let mut events = Vec::new();

    for (i, block) in content.iter().enumerate() {
        let block_type = block.get("type").and_then(|v| v.as_str()).unwrap_or("");

        let (subtype, mut payload) = match block_type {
            "thinking" => {
                let mut p = PiMonoPayload::new();
                apply_pi_envelope(&mut p, line);
                p.text = block.get("thinking").and_then(|v| v.as_str()).map(|s| s.to_string());
                p.model = model.clone();
                ("message.assistant.thinking", p)
            }
            "text" => {
                let mut p = PiMonoPayload::new();
                apply_pi_envelope(&mut p, line);
                p.text = block.get("text").and_then(|v| v.as_str()).map(|s| s.to_string());
                p.model = model.clone();
                p.stop_reason = if i == content.len() - 1 { stop_reason.clone() } else { None };
                ("message.assistant.text", p)
            }
            "toolCall" => {
                let mut p = PiMonoPayload::new();
                apply_pi_envelope(&mut p, line);
                p.tool = block.get("name").and_then(|v| v.as_str()).map(|s| s.to_string());
                p.tool_call_id = block.get("id").and_then(|v| v.as_str()).map(|s| s.to_string());
                p.args = block.get("arguments").cloned();
                p.model = model.clone();
                p.stop_reason = if i == content.len() - 1 { stop_reason.clone() } else { None };
                ("message.assistant.tool_use", p)
            }
            _ => continue,
        };

        // Content types for downstream (same for all decomposed events)
        let content_types: Vec<String> = content
            .iter()
            .filter_map(|b| b.get("type").and_then(|v| v.as_str()).map(|s| s.to_string()))
            .collect();
        if !content_types.is_empty() {
            payload.content_types = Some(content_types);
        }

        // Token usage only on the last event
        if i == content.len() - 1 {
            if let Some(ref u) = usage {
                payload.token_usage = Some(u.clone());
            }
        }

        let derived_id = derive_block_event_id(&state.session_id, eid_str, i, subtype);

        let data = EventData::with_payload(
            line.clone(),
            state.next_seq(),
            state.session_id.clone(),
            AgentPayload::PiMono(payload),
        );

        events.push(
            CloudEvent::new(
                source.to_string(),
                IO_ARC_EVENT.to_string(),
                data,
                Some(subtype.to_string()),
                Some(derived_id),
                timestamp.clone(),
                None,
                None,
                Some("pi-mono".to_string()),
            )
            .with_host(crate::host::host()),
        );
    }

    // Synthetic turn boundary.
    //
    // Pi-mono never emits `system.turn.complete` natively. Without it,
    // the eval-apply state machine in `open-story-patterns` can't
    // crystallize a `StructuralTurn` from pi-mono events, so the
    // sentence detector never fires and pi-mono sessions get NO
    // narrated story in the UI.
    //
    // Pi-mono's `message.stopReason` is the matching signal — across
    // every captured fixture it's exactly `"stop"` (turn ended, nothing
    // more coming until the user prompts again) or `"toolUse"` (more
    // assistant content coming after the tool result). We synthesize a
    // turn.complete event after the last decomposed block iff
    // stopReason == "stop", which mirrors when Claude Code's Stop hook
    // fires.
    //
    // Surfaced by the recursion test in
    // `rs/tests/test_principle_recursive_observability.rs`.
    if stop_reason.as_deref() == Some("stop") && !events.is_empty() {
        let mut p = PiMonoPayload::new();
        apply_pi_envelope(&mut p, line);
        p.model = model.clone();
        p.stop_reason = Some("stop".to_string());

        let derived_id = derive_block_event_id(
            &state.session_id,
            eid_str,
            content.len(), // index past the last block
            "system.turn.complete",
        );

        let data = EventData::with_payload(
            line.clone(),
            state.next_seq(),
            state.session_id.clone(),
            AgentPayload::PiMono(p),
        );

        events.push(
            CloudEvent::new(
                source.to_string(),
                IO_ARC_EVENT.to_string(),
                data,
                Some("system.turn.complete".to_string()),
                Some(derived_id),
                timestamp.clone(),
                None,
                None,
                Some("pi-mono".to_string()),
            )
            .with_host(crate::host::host()),
        );
    }

    events
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
            // Check if this is an assistant message — decompose into N events
            let role = line
                .get("message")
                .and_then(|m| m.get("role"))
                .and_then(|v| v.as_str());
            if role == Some("assistant") {
                return decompose_assistant(line, state, &source, &entry_id);
            }
            // Non-assistant messages: single event (user, toolResult, bash, etc.)
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
        data,
        subtype,
        entry_id,
        timestamp,
        None,
        None,
        Some("pi-mono".to_string()),
    )
    .with_host(crate::host::host())]
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
        match event.data.agent_payload.as_ref().expect("agent_payload should be Some") {
            AgentPayload::PiMono(pm) => pm.clone(),
            _ => panic!("expected PiMono payload"),
        }
    }

    /// Helper: extract EventData from event.
    fn event_data(event: &CloudEvent) -> &EventData {
        &event.data
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
                "assistant single toolCall → message.assistant.tool_use",
                json!({
                    "type": "message",
                    "timestamp": "2025-01-01T00:00:03Z",
                    "message": {
                        "role": "assistant",
                        "content": [
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
                "assistant single thinking → message.assistant.thinking",
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
            // The boundary table is testing PRIMARY subtype dispatch.
            // Assistant messages with stopReason="stop" also emit a
            // synthetic system.turn.complete (post-decomposition) — that
            // behavior is exercised in the decomposition tests below.
            // Here we just assert the FIRST event has the expected
            // primary subtype.
            assert!(
                !events.is_empty(),
                "{desc}: expected at least 1 event, got 0"
            );
            assert_eq!(
                events[0].subtype.as_deref(),
                Some(expected_subtype),
                "{desc}: wrong primary subtype"
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
        assert_eq!(events.len(), 1, "single toolCall → 1 event");
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

    // ── Decomposition tests (RED → GREEN) ─────────────────────
    //
    // These test the decomposing translator: one JSONL line with
    // multiple content blocks → multiple CloudEvents.

    #[test]
    fn test_decompose_thinking_text_produces_two_events_plus_turn_complete() {
        // [thinking, text] with stopReason="stop" → 2 decomposed events
        // + 1 synthetic system.turn.complete (so eval-apply gets a turn
        // boundary). The synthetic event is the recursion-test fix —
        // pi-mono never emits turn.complete natively.
        let line = json!({
            "type": "message", "id": "dec-01",
            "timestamp": "2025-01-01T00:00:00Z",
            "message": {
                "role": "assistant",
                "content": [
                    {"type": "thinking", "thinking": "let me reason"},
                    {"type": "text", "text": "the answer is 42"},
                ],
                "model": "claude-opus-4-6",
                "stopReason": "stop",
                "usage": {"input": 10, "output": 20, "totalTokens": 30},
                "timestamp": 1234567890,
            },
        });
        let events = translate_pi_line(&line, &mut state());
        assert_eq!(events.len(), 3, "thinking+text+stop → 2 decomposed + 1 synthetic turn.complete");
        assert_eq!(events[0].subtype.as_deref(), Some("message.assistant.thinking"));
        assert_eq!(events[1].subtype.as_deref(), Some("message.assistant.text"));
        assert_eq!(events[2].subtype.as_deref(), Some("system.turn.complete"));
    }

    #[test]
    fn test_decompose_thinking_text_tool_produces_three_events() {
        // [thinking, text, toolCall] → 3 CloudEvents (the worst case)
        let line = json!({
            "type": "message", "id": "dec-02",
            "timestamp": "2025-01-01T00:00:00Z",
            "message": {
                "role": "assistant",
                "content": [
                    {"type": "thinking", "thinking": "I should read the file"},
                    {"type": "text", "text": "Let me read that for you."},
                    {"type": "toolCall", "id": "tc-1", "name": "read",
                     "arguments": {"path": "/config.toml"}},
                ],
                "model": "claude-opus-4-6",
                "stopReason": "toolUse",
                "usage": {"input": 100, "output": 50, "totalTokens": 150},
                "timestamp": 1234567890,
            },
        });
        let events = translate_pi_line(&line, &mut state());
        assert_eq!(events.len(), 3, "thinking+text+tool should produce 3 events");
        assert_eq!(events[0].subtype.as_deref(), Some("message.assistant.thinking"));
        assert_eq!(events[1].subtype.as_deref(), Some("message.assistant.text"));
        assert_eq!(events[2].subtype.as_deref(), Some("message.assistant.tool_use"));
    }

    #[test]
    fn test_decompose_multi_tool_produces_two_events() {
        // [toolCall, toolCall] → 2 CloudEvents
        let line = json!({
            "type": "message", "id": "dec-03",
            "timestamp": "2025-01-01T00:00:00Z",
            "message": {
                "role": "assistant",
                "content": [
                    {"type": "toolCall", "id": "tc-1", "name": "read",
                     "arguments": {"path": "/a.txt"}},
                    {"type": "toolCall", "id": "tc-2", "name": "read",
                     "arguments": {"path": "/b.txt"}},
                ],
                "model": "claude-opus-4-6",
                "stopReason": "toolUse",
                "timestamp": 1234567890,
            },
        });
        let events = translate_pi_line(&line, &mut state());
        assert_eq!(events.len(), 2, "two toolCalls should produce 2 events");
        assert_eq!(events[0].subtype.as_deref(), Some("message.assistant.tool_use"));
        assert_eq!(events[1].subtype.as_deref(), Some("message.assistant.tool_use"));
    }

    #[test]
    fn test_decompose_single_text_with_stop_produces_text_plus_turn_complete() {
        // [text] with stopReason="stop" → 1 decomposed event + 1 synthetic
        // turn.complete. The decomposed text passes through unchanged;
        // the synthetic event closes the turn for eval-apply.
        let line = json!({
            "type": "message", "id": "dec-04",
            "timestamp": "2025-01-01T00:00:00Z",
            "message": {
                "role": "assistant",
                "content": [{"type": "text", "text": "hello"}],
                "model": "claude-opus-4-6",
                "stopReason": "stop",
                "timestamp": 1234567890,
            },
        });
        let events = translate_pi_line(&line, &mut state());
        assert_eq!(events.len(), 2, "text+stop → 1 decomposed + 1 synthetic turn.complete");
        assert_eq!(events[0].subtype.as_deref(), Some("message.assistant.text"));
        assert_eq!(events[1].subtype.as_deref(), Some("system.turn.complete"));
    }

    #[test]
    fn test_decompose_single_tool_still_one_event() {
        // [toolCall] → 1 CloudEvent (no regression)
        let line = json!({
            "type": "message", "id": "dec-05",
            "timestamp": "2025-01-01T00:00:00Z",
            "message": {
                "role": "assistant",
                "content": [
                    {"type": "toolCall", "id": "tc-1", "name": "read",
                     "arguments": {"path": "/foo"}},
                ],
                "model": "claude-opus-4-6",
                "stopReason": "toolUse",
                "timestamp": 1234567890,
            },
        });
        let events = translate_pi_line(&line, &mut state());
        assert_eq!(events.len(), 1, "single tool should still produce 1 event");
        assert_eq!(events[0].subtype.as_deref(), Some("message.assistant.tool_use"));
    }

    #[test]
    fn test_decomposed_ids_unique() {
        let line = json!({
            "type": "message", "id": "dec-06",
            "timestamp": "2025-01-01T00:00:00Z",
            "message": {
                "role": "assistant",
                "content": [
                    {"type": "thinking", "thinking": "hmm"},
                    {"type": "text", "text": "ok"},
                    {"type": "toolCall", "id": "tc-1", "name": "read",
                     "arguments": {"path": "/x"}},
                ],
                "model": "claude-opus-4-6",
                "stopReason": "toolUse",
                "timestamp": 1234567890,
            },
        });
        let events = translate_pi_line(&line, &mut state());
        let ids: Vec<&str> = events.iter().map(|e| e.id.as_str()).collect();
        let unique: std::collections::HashSet<&str> = ids.iter().cloned().collect();
        assert_eq!(ids.len(), unique.len(), "all decomposed event IDs must be unique");
    }

    #[test]
    fn test_decomposed_ids_deterministic() {
        let line = json!({
            "type": "message", "id": "dec-07",
            "timestamp": "2025-01-01T00:00:00Z",
            "message": {
                "role": "assistant",
                "content": [
                    {"type": "thinking", "thinking": "hmm"},
                    {"type": "text", "text": "ok"},
                ],
                "model": "claude-opus-4-6",
                "stopReason": "stop",
                "timestamp": 1234567890,
            },
        });
        let events1 = translate_pi_line(&line, &mut state());
        let events2 = translate_pi_line(&line, &mut state());
        for (e1, e2) in events1.iter().zip(events2.iter()) {
            assert_eq!(e1.id, e2.id, "same input should produce same IDs");
        }
    }

    #[test]
    fn test_decomposed_raw_shared() {
        let line = json!({
            "type": "message", "id": "dec-08",
            "timestamp": "2025-01-01T00:00:00Z",
            "message": {
                "role": "assistant",
                "content": [
                    {"type": "thinking", "thinking": "reason"},
                    {"type": "text", "text": "answer"},
                    {"type": "toolCall", "id": "tc-1", "name": "read",
                     "arguments": {"path": "/x"}},
                ],
                "model": "claude-opus-4-6",
                "stopReason": "toolUse",
                "timestamp": 1234567890,
            },
        });
        let events = translate_pi_line(&line, &mut state());
        assert_eq!(events.len(), 3);
        // All decomposed events share the same raw line
        let raw0 = serde_json::to_string(&events[0].data.raw).unwrap();
        let raw1 = serde_json::to_string(&events[1].data.raw).unwrap();
        let raw2 = serde_json::to_string(&events[2].data.raw).unwrap();
        assert_eq!(raw0, raw1, "thinking and text should share raw");
        assert_eq!(raw1, raw2, "text and tool should share raw");
        // Raw preserves the bundled content array
        assert_eq!(events[0].data.raw["message"]["content"][0]["type"], "thinking");
        assert_eq!(events[0].data.raw["message"]["content"][1]["type"], "text");
        assert_eq!(events[0].data.raw["message"]["content"][2]["type"], "toolCall");
    }

    #[test]
    fn test_token_usage_on_last_decomposed_event() {
        let line = json!({
            "type": "message", "id": "dec-09",
            "timestamp": "2025-01-01T00:00:00Z",
            "message": {
                "role": "assistant",
                "content": [
                    {"type": "thinking", "thinking": "reason"},
                    {"type": "text", "text": "answer"},
                    {"type": "toolCall", "id": "tc-1", "name": "read",
                     "arguments": {"path": "/x"}},
                ],
                "model": "claude-opus-4-6",
                "stopReason": "toolUse",
                "usage": {"input": 100, "output": 50, "totalTokens": 150},
                "timestamp": 1234567890,
            },
        });
        let events = translate_pi_line(&line, &mut state());
        assert_eq!(events.len(), 3);
        // Token usage only on the last event
        let pm0 = pi_payload(&events[0]);
        let pm2 = pi_payload(&events[2]);
        assert!(pm0.token_usage.is_none(), "first event should NOT have token_usage");
        assert!(pm2.token_usage.is_some(), "last event SHOULD have token_usage");
        assert_eq!(pm2.token_usage.as_ref().unwrap()["input"], 100);
    }

    // ── Synthetic turn.complete tests (recursion principle fix) ──
    //
    // Pi-mono never emits system.turn.complete. The decomposer
    // synthesizes one when stopReason="stop" so eval-apply can
    // crystallize a StructuralTurn and SentenceDetector can render
    // a turn.sentence. See docs/research/architecture-audit/PRINCIPLES.md
    // and rs/tests/test_principle_recursive_observability.rs.

    #[test]
    fn test_synthetic_turn_complete_only_when_stop_reason_is_stop() {
        // stopReason="toolUse" → assistant called a tool, more LLM
        // round trips coming. Turn continues. NO synthetic turn.complete.
        let line = json!({
            "type": "message", "id": "syn-tool",
            "timestamp": "2025-01-01T00:00:00Z",
            "message": {
                "role": "assistant",
                "content": [
                    {"type": "toolCall", "id": "tc-1", "name": "read",
                     "arguments": {"path": "/x"}},
                ],
                "model": "claude-opus-4-6",
                "stopReason": "toolUse",
                "timestamp": 1234567890,
            },
        });
        let events = translate_pi_line(&line, &mut state());
        assert_eq!(events.len(), 1, "toolUse → no synthetic turn.complete");
        assert_ne!(
            events.last().unwrap().subtype.as_deref(),
            Some("system.turn.complete"),
            "toolUse must NOT emit turn.complete"
        );
    }

    #[test]
    fn test_synthetic_turn_complete_carries_pi_mono_agent_and_raw() {
        // The synthetic event must look like a real pi-mono CloudEvent:
        // - agent = "pi-mono"
        // - data.raw = same as the bundle (sovereignty: raw preserved)
        // - data.agent_payload.stop_reason = "stop"
        // - unique deterministic id derived from session+entry+index+subtype
        let line = json!({
            "type": "message", "id": "syn-stop",
            "timestamp": "2025-01-01T00:00:00Z",
            "message": {
                "role": "assistant",
                "content": [{"type": "text", "text": "done"}],
                "model": "claude-opus-4-6",
                "stopReason": "stop",
                "timestamp": 1234567890,
            },
        });
        let events = translate_pi_line(&line, &mut state());
        let tc = events.last().expect("at least one event");
        assert_eq!(tc.subtype.as_deref(), Some("system.turn.complete"));
        assert_eq!(tc.agent.as_deref(), Some("pi-mono"));
        // Raw preserved
        let raw_str = serde_json::to_string(&tc.data.raw).unwrap();
        assert!(raw_str.contains("\"stopReason\":\"stop\""), "synthetic event preserves bundled raw");
        // Stop reason on the typed payload
        let pm = pi_payload(tc);
        assert_eq!(pm.stop_reason.as_deref(), Some("stop"));
        // ID is unique vs the decomposed text event
        assert_ne!(events[0].id, events[1].id, "synthetic event has its own id");
    }

    #[test]
    fn test_decompose_dedup_skips_entire_line() {
        // If a line's entry id is already seen, ALL decomposed events are skipped
        let line = json!({
            "type": "message", "id": "dec-10",
            "timestamp": "2025-01-01T00:00:00Z",
            "message": {
                "role": "assistant",
                "content": [
                    {"type": "thinking", "thinking": "hmm"},
                    {"type": "text", "text": "ok"},
                ],
                "model": "claude-opus-4-6",
                "stopReason": "stop",
                "timestamp": 1234567890,
            },
        });
        let mut s = state();
        let first = translate_pi_line(&line, &mut s);
        let second = translate_pi_line(&line, &mut s);
        // 2 decomposed (thinking, text) + 1 synthetic turn.complete (stopReason="stop") = 3
        assert_eq!(first.len(), 3, "first should produce 2 decomposed + 1 turn.complete");
        assert_eq!(second.len(), 0, "duplicate should produce 0 events");
    }

    #[test]
    fn test_decomposed_thinking_has_text_field() {
        let line = json!({
            "type": "message", "id": "dec-11",
            "timestamp": "2025-01-01T00:00:00Z",
            "message": {
                "role": "assistant",
                "content": [
                    {"type": "thinking", "thinking": "deep thoughts"},
                    {"type": "text", "text": "shallow answer"},
                ],
                "model": "claude-opus-4-6",
                "stopReason": "stop",
                "timestamp": 1234567890,
            },
        });
        let events = translate_pi_line(&line, &mut state());
        let pm0 = pi_payload(&events[0]); // thinking
        let pm1 = pi_payload(&events[1]); // text
        assert_eq!(pm0.text.as_deref(), Some("deep thoughts"), "thinking event text");
        assert_eq!(pm1.text.as_deref(), Some("shallow answer"), "text event text");
    }

    #[test]
    fn test_decomposed_tool_has_name_and_args() {
        let line = json!({
            "type": "message", "id": "dec-12",
            "timestamp": "2025-01-01T00:00:00Z",
            "message": {
                "role": "assistant",
                "content": [
                    {"type": "text", "text": "reading"},
                    {"type": "toolCall", "id": "tc-1", "name": "read",
                     "arguments": {"path": "/config.toml"}},
                ],
                "model": "claude-opus-4-6",
                "stopReason": "toolUse",
                "timestamp": 1234567890,
            },
        });
        let events = translate_pi_line(&line, &mut state());
        let tool_event = events.iter().find(|e| e.subtype.as_deref() == Some("message.assistant.tool_use")).unwrap();
        let pm = pi_payload(tool_event);
        assert_eq!(pm.tool.as_deref(), Some("read"));
        assert_eq!(pm.args.as_ref().unwrap()["path"], "/config.toml");
        assert_eq!(pm.tool_call_id.as_deref(), Some("tc-1"));
    }

    #[test]
    fn test_decomposed_seq_increments() {
        let line = json!({
            "type": "message", "id": "dec-13",
            "timestamp": "2025-01-01T00:00:00Z",
            "message": {
                "role": "assistant",
                "content": [
                    {"type": "thinking", "thinking": "hmm"},
                    {"type": "text", "text": "ok"},
                    {"type": "toolCall", "id": "tc-1", "name": "read",
                     "arguments": {"path": "/x"}},
                ],
                "model": "claude-opus-4-6",
                "stopReason": "toolUse",
                "timestamp": 1234567890,
            },
        });
        let events = translate_pi_line(&line, &mut state());
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].data.seq, 1);
        assert_eq!(events[1].data.seq, 2);
        assert_eq!(events[2].data.seq, 3);
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
