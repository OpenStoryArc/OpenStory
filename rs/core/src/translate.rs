//! Pure translation: serde_json::Value → CloudEvent(s).
//!
//! Port of transcript_translator.py. Input is untyped Value because Claude Code's
//! transcript format evolves. Output is typed CloudEvent for compile-time guarantees.

use std::collections::HashSet;

use serde_json::Value;

use crate::cloud_event::CloudEvent;
use crate::event_data::{derive_tool_outcome, AgentPayload, ClaudeCodePayload, EventData};

/// Unified CloudEvent type constant — all events use this single type.
pub const IO_ARC_EVENT: &str = "io.arc.event";

/// Legacy constants — all resolve to IO_ARC_EVENT for backward-compat in tests.
#[allow(dead_code)]
pub const TRANSCRIPT_ASSISTANT: &str = IO_ARC_EVENT;
#[allow(dead_code)]
pub const TRANSCRIPT_USER: &str = IO_ARC_EVENT;
#[allow(dead_code)]
pub const TRANSCRIPT_PROGRESS: &str = IO_ARC_EVENT;
#[allow(dead_code)]
pub const TRANSCRIPT_SNAPSHOT: &str = IO_ARC_EVENT;
#[allow(dead_code)]
pub const TRANSCRIPT_QUEUE: &str = IO_ARC_EVENT;
#[allow(dead_code)]
pub const TRANSCRIPT_SYSTEM: &str = IO_ARC_EVENT;

/// Returns true if the transcript type is one we know how to translate.
fn is_known_type(transcript_type: &str) -> bool {
    matches!(
        transcript_type,
        "assistant" | "user" | "progress" | "file-history-snapshot" | "queue-operation" | "system"
    )
}

/// Transcript format — detected once per file, then locked.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum TranscriptFormat {
    #[default]
    Unknown,
    ClaudeCode,
    PiMono,
}

/// A pending tool call: name + input, keyed by tool_use_id.
/// Used to derive ToolOutcome when the tool_result arrives.
#[derive(Debug, Clone)]
struct PendingToolCall {
    name: String,
    input: Value,
}

/// Mutable state for one transcript file's translation session.
pub struct TranscriptState {
    pub session_id: String,
    pub byte_offset: u64,
    pub line_count: u64,
    pub seen_uuids: HashSet<String>,
    pub format: TranscriptFormat,
    seq: u64,
    /// tool_use_id → (tool_name, tool_input) for domain event derivation.
    pending_tool_calls: std::collections::HashMap<String, PendingToolCall>,
}

impl TranscriptState {
    pub fn new(session_id: String) -> Self {
        Self {
            session_id,
            byte_offset: 0,
            line_count: 0,
            seen_uuids: HashSet::new(),
            format: TranscriptFormat::Unknown,
            seq: 0,
            pending_tool_calls: std::collections::HashMap::new(),
        }
    }

    pub fn next_seq(&mut self) -> u64 {
        self.seq += 1;
        self.seq
    }
}

/// Extract common envelope fields from a Claude Code transcript line into a payload.
///
/// Raw transcripts use camelCase (Claude Code's convention). We normalize to
/// snake_case at extraction time so the payload is purely snake_case from here on.
fn apply_envelope(payload: &mut ClaudeCodePayload, line: &Value) {
    if let Value::Object(obj) = line {
        if let Some(v) = obj.get("uuid").and_then(|v| v.as_str()) {
            payload.uuid = Some(v.to_string());
        }
        if let Some(v) = obj.get("parentUuid").and_then(|v| v.as_str()) {
            payload.parent_uuid = Some(v.to_string());
        }
        if let Some(v) = obj.get("cwd").and_then(|v| v.as_str()) {
            payload.cwd = Some(v.to_string());
        }
        if let Some(v) = obj.get("version").and_then(|v| v.as_str()) {
            payload.version = Some(v.to_string());
        }
        if let Some(v) = obj.get("gitBranch").and_then(|v| v.as_str()) {
            payload.git_branch = Some(v.to_string());
        }
        if let Some(v) = obj.get("slug").and_then(|v| v.as_str()) {
            payload.slug = Some(v.to_string());
        }
        if let Some(v) = obj.get("timestamp").and_then(|v| v.as_str()) {
            payload.timestamp = Some(v.to_string());
        }
        // Subagent identity fields
        if let Some(v) = obj.get("agentId").and_then(|v| v.as_str()) {
            payload.agent_id = Some(v.to_string());
        }
        if let Some(v) = obj.get("parentToolUseID").and_then(|v| v.as_str()) {
            payload.parent_tool_use_id = Some(v.to_string());
        }
        if let Some(v) = obj.get("isSidechain").and_then(|v| v.as_bool()) {
            payload.is_sidechain = Some(v);
        }
        // Progress events carry agentId + parentToolUseID nested inside data.*
        // Extract them when the top-level keys are absent.
        if let Some(Value::Object(data)) = obj.get("data") {
            if payload.agent_id.is_none() {
                if let Some(v) = data.get("agentId").and_then(|v| v.as_str()) {
                    payload.agent_id = Some(v.to_string());
                }
            }
            if payload.parent_tool_use_id.is_none() {
                if let Some(v) = data.get("parentToolUseID").and_then(|v| v.as_str()) {
                    payload.parent_tool_use_id = Some(v.to_string());
                }
            }
        }
    }
}

/// Determine assistant subtype from content blocks (hierarchical).
fn determine_assistant_subtype(content: &Value) -> &'static str {
    if let Value::Array(blocks) = content {
        let mut has_thinking = false;
        let mut has_tool_use = false;
        for block in blocks {
            if let Some(bt) = block.get("type").and_then(|v| v.as_str()) {
                match bt {
                    "thinking" => has_thinking = true,
                    "tool_use" => has_tool_use = true,
                    _ => {}
                }
            }
        }
        if has_tool_use {
            return "message.assistant.tool_use";
        }
        if has_thinking {
            return "message.assistant.thinking";
        }
    }
    "message.assistant.text"
}

/// Apply assistant-specific fields to the payload. Returns subtype.
fn apply_assistant_fields(payload: &mut ClaudeCodePayload, line: &Value) -> String {
    let message = line.get("message").cloned().unwrap_or(Value::Object(Default::default()));
    let content = message.get("content").cloned().unwrap_or(Value::Array(vec![]));

    // Normalize string content to array
    let content_normalized = if content.is_string() {
        Value::Array(vec![serde_json::json!({"type": "text", "text": content.as_str().unwrap_or("")})])
    } else {
        content.clone()
    };

    let subtype = determine_assistant_subtype(&content_normalized);

    // Content types for downstream
    if let Value::Array(ref blocks) = content_normalized {
        let types: Vec<String> = blocks
            .iter()
            .filter_map(|b| b.get("type").and_then(|v| v.as_str()).map(|s| s.to_string()))
            .collect();
        if !types.is_empty() {
            payload.content_types = Some(types);
        }
    }

    if let Some(v) = message.get("model").and_then(|v| v.as_str()) {
        payload.model = Some(v.to_string());
    }
    if let Some(v) = message.get("stop_reason") {
        payload.stop_reason = Some(v.clone());
    }
    if let Some(v) = message.get("usage") {
        payload.token_usage = Some(v.clone());
    }
    if let Some(v) = message.get("id").and_then(|v| v.as_str()) {
        payload.message_id = Some(v.to_string());
    }

    // Extract text to top level
    if let Some(text) = extract_text_from_content(&content_normalized) {
        payload.text = Some(text);
    }

    // Extract tool info for tool_use subtypes
    if subtype == "message.assistant.tool_use" {
        if let Some((tool_name, tool_args)) = extract_first_tool_from_content(&content_normalized) {
            payload.tool = Some(tool_name);
            payload.args = Some(tool_args);
        }
    }

    subtype.to_string()
}

/// Apply user-specific fields to the payload. Returns subtype.
fn apply_user_fields(payload: &mut ClaudeCodePayload, line: &Value) -> String {
    let message = line.get("message").cloned().unwrap_or(Value::Object(Default::default()));
    let content = message.get("content").cloned().unwrap_or(Value::Array(vec![]));

    let mut subtype = "message.user.prompt".to_string();
    if let Value::Array(ref blocks) = content {
        for block in blocks {
            if block.get("type").and_then(|v| v.as_str()) == Some("tool_result") {
                subtype = "message.user.tool_result".to_string();
                break;
            }
        }
    }

    if let Some(v) = line.get("userType").and_then(|v| v.as_str()) {
        payload.user_type = Some(v.to_string());
    }

    // Extract text to top level for prompt messages
    if subtype == "message.user.prompt" {
        if let Some(text) = extract_text_from_content(&content) {
            payload.text = Some(text);
        }
    }

    subtype
}

/// Apply progress-specific fields to the payload. Returns subtype.
fn apply_progress_fields(payload: &mut ClaudeCodePayload, line: &Value) -> String {
    let data = line.get("data").cloned().unwrap_or(Value::Object(Default::default()));
    let progress_type = data
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();

    let subtype = match progress_type.as_str() {
        "bash_progress" => "progress.bash".to_string(),
        "agent_progress" => "progress.agent".to_string(),
        "hook_progress" => "progress.hook".to_string(),
        other => format!("progress.{other}"),
    };

    payload.progress_type = Some(progress_type);
    subtype
}

/// Apply system-specific fields to the payload. Returns subtype.
fn apply_system_fields(payload: &mut ClaudeCodePayload, line: &Value) -> String {
    let raw_subtype = line
        .get("subtype")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");

    match raw_subtype {
        "turn_duration" => {
            if let Some(v) = line.get("durationMs").and_then(|v| v.as_f64()) {
                payload.duration_ms = Some(v);
            }
            "system.turn.complete".to_string()
        }
        "stop_hook_summary" => {
            if let Some(v) = line.get("hookCount").and_then(|v| v.as_u64()) {
                payload.hook_count = Some(v);
            }
            if let Some(v) = line.get("preventedContinuation").and_then(|v| v.as_bool()) {
                payload.prevented_continuation = Some(v);
            }
            "system.hook".to_string()
        }
        s if s.contains("error") => "system.error".to_string(),
        "compact_boundary" => "system.compact".to_string(),
        other => format!("system.{other}"),
    }
}

/// Apply queue-operation-specific fields to the payload. Returns subtype.
fn apply_queue_fields(payload: &mut ClaudeCodePayload, line: &Value) -> String {
    let operation = line
        .get("operation")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();

    payload.operation = Some(operation.clone());
    format!("queue.{operation}")
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
            if block.get("type").and_then(|v| v.as_str()) == Some("tool_use") {
                let name = block.get("name").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();
                let input = block.get("input").cloned().unwrap_or(Value::Object(Default::default()));
                return Some((name, input));
            }
        }
    }
    None
}

/// Extract all tool_use blocks from content: (tool_use_id, name, input).
fn extract_all_tool_uses(content: &Value) -> Vec<(String, String, Value)> {
    let mut result = Vec::new();
    if let Value::Array(blocks) = content {
        for block in blocks {
            if block.get("type").and_then(|v| v.as_str()) == Some("tool_use") {
                let id = block.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let name = block.get("name").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();
                let input = block.get("input").cloned().unwrap_or(Value::Object(Default::default()));
                if !id.is_empty() {
                    result.push((id, name, input));
                }
            }
        }
    }
    result
}

/// Pure function: translate one parsed transcript JSON line into CloudEvent(s).
///
/// Returns zero events for unknown types or duplicate UUIDs.
/// All events are produced with type `io.arc.event` and hierarchical subtypes.
pub fn translate_line(line: &Value, state: &mut TranscriptState) -> Vec<CloudEvent> {
    let entry_type = match line.get("type").and_then(|v| v.as_str()) {
        Some(t) => t,
        None => return vec![],
    };

    if !is_known_type(entry_type) {
        return vec![];
    }

    // Deduplication by UUID
    let uuid = line.get("uuid").and_then(|v| v.as_str()).map(|s| s.to_string());
    if let Some(ref u) = uuid {
        if state.seen_uuids.contains(u) {
            return vec![];
        }
        state.seen_uuids.insert(u.clone());
    }

    let source = format!("arc://transcript/{}", state.session_id);
    let mut payload = ClaudeCodePayload::new();

    // Apply envelope fields (camelCase → snake_case typed fields)
    apply_envelope(&mut payload, line);

    let subtype: Option<String> = match entry_type {
        "assistant" => Some(apply_assistant_fields(&mut payload, line)),
        "user" => Some(apply_user_fields(&mut payload, line)),
        "progress" => Some(apply_progress_fields(&mut payload, line)),
        "system" => Some(apply_system_fields(&mut payload, line)),
        "queue-operation" => Some(apply_queue_fields(&mut payload, line)),
        "file-history-snapshot" => Some("file.snapshot".to_string()),
        _ => None,
    };

    // ── Domain event tracking ──
    // Track tool_use blocks from assistant messages for later outcome derivation.
    if entry_type == "assistant" {
        let message = line.get("message").unwrap_or(&Value::Null);
        let content = message.get("content").unwrap_or(&Value::Null);
        for (tool_use_id, name, input) in extract_all_tool_uses(content) {
            state.pending_tool_calls.insert(
                tool_use_id,
                PendingToolCall { name, input },
            );
        }
    }

    // Derive tool_outcome for tool_result events by correlating with pending tool calls.
    if subtype.as_deref() == Some("message.user.tool_result") {
        let message = line.get("message").unwrap_or(&Value::Null);
        let content = message.get("content").unwrap_or(&Value::Null);
        if let Value::Array(blocks) = content {
            for block in blocks {
                if block.get("type").and_then(|v| v.as_str()) == Some("tool_result") {
                    let tool_use_id = block.get("tool_use_id").and_then(|v| v.as_str()).unwrap_or("");
                    if let Some(pending) = state.pending_tool_calls.remove(tool_use_id) {
                        let result_output = block
                            .get("content")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        let is_error = block
                            .get("is_error")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false);
                        payload.tool_outcome = derive_tool_outcome(
                            &pending.name,
                            &pending.input,
                            result_output,
                            is_error,
                        );
                        // Surface result text so the eval-apply detector
                        // can populate output_summary on ApplyRecords
                        if !result_output.is_empty() {
                            payload.text = Some(result_output.to_string());
                        }
                        break; // First result wins for the payload-level field
                    }
                }
            }
        }
    }

    // Extract agent session ID from Agent tool_result's toolUseResult.agentId
    if subtype.as_deref() == Some("message.user.tool_result") {
        if let Some(agent_id) = line
            .get("toolUseResult")
            .and_then(|v| v.get("agentId"))
            .and_then(|v| v.as_str())
        {
            if !agent_id.is_empty() {
                payload.agent_session_id = Some(format!("agent-{agent_id}"));
            }
        }
    }

    // Session ID from envelope overrides filename-derived one (for sidechain files)
    let session_id = line
        .get("sessionId")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| state.session_id.clone());

    // Build EventData with typed payload
    let data = EventData::with_payload(
        line.clone(),
        state.next_seq(),
        session_id,
        AgentPayload::ClaudeCode(payload),
    );

    let timestamp = line.get("timestamp").and_then(|v| v.as_str()).map(|s| s.to_string());

    vec![CloudEvent::new(
        source,
        IO_ARC_EVENT.to_string(),
        data,
        subtype,
        uuid,
        timestamp,
        None,
        None,
        Some("claude-code".to_string()),
    )]
}
