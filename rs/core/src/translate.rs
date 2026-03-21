//! Pure translation: serde_json::Value → CloudEvent(s).
//!
//! Port of transcript_translator.py. Input is untyped Value because Claude Code's
//! transcript format evolves. Output is typed CloudEvent for compile-time guarantees.

use std::collections::HashSet;

use serde_json::Value;

use crate::cloud_event::CloudEvent;

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

/// Mutable state for one transcript file's translation session.
pub struct TranscriptState {
    pub session_id: String,
    pub byte_offset: u64,
    pub line_count: u64,
    pub seen_uuids: HashSet<String>,
    seq: u64,
}

impl TranscriptState {
    pub fn new(session_id: String) -> Self {
        Self {
            session_id,
            byte_offset: 0,
            line_count: 0,
            seen_uuids: HashSet::new(),
            seq: 0,
        }
    }

    pub fn next_seq(&mut self) -> u64 {
        self.seq += 1;
        self.seq
    }
}

/// Extract common envelope fields present on most transcript entries.
///
/// Raw transcripts use camelCase (Claude Code's convention). We normalize to
/// snake_case at extraction time so the CloudEvent data bag is purely snake_case
/// from here on.
fn extract_envelope(line: &Value) -> serde_json::Map<String, Value> {
    // (raw_camelCase_key, output_snake_case_key)
    let keys: &[(&str, &str)] = &[
        ("uuid", "uuid"),
        ("parentUuid", "parent_uuid"),
        ("sessionId", "session_id"),
        ("cwd", "cwd"),
        ("version", "version"),
        ("gitBranch", "git_branch"),
        ("slug", "slug"),
        ("timestamp", "timestamp"),
        // Subagent identity fields (Story 037)
        ("agentId", "agent_id"),
        ("parentToolUseID", "parent_tool_use_id"),
        ("isSidechain", "is_sidechain"),
    ];
    let mut map = serde_json::Map::new();
    if let Value::Object(obj) = line {
        for &(raw_key, out_key) in keys {
            if let Some(v) = obj.get(raw_key) {
                if !v.is_null() {
                    map.insert(out_key.to_string(), v.clone());
                }
            }
        }
        // Progress events carry agentId + parentToolUseID nested inside data.*
        // Extract them when the top-level keys are absent.
        if let Some(Value::Object(data)) = obj.get("data") {
            if !map.contains_key("agent_id") {
                if let Some(v) = data.get("agentId") {
                    if !v.is_null() {
                        map.insert("agent_id".to_string(), v.clone());
                    }
                }
            }
            if !map.contains_key("parent_tool_use_id") {
                if let Some(v) = data.get("parentToolUseID") {
                    if !v.is_null() {
                        map.insert("parent_tool_use_id".to_string(), v.clone());
                    }
                }
            }
        }
    }
    map
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

/// Extract assistant-specific fields.
fn extract_assistant(line: &Value) -> serde_json::Map<String, Value> {
    let message = line.get("message").cloned().unwrap_or(Value::Object(Default::default()));
    let content = message.get("content").cloned().unwrap_or(Value::Array(vec![]));

    // Normalize string content to array
    let content_arr = if content.is_string() {
        vec![serde_json::json!({"type": "text", "text": content.as_str().unwrap_or("")})]
    } else if let Value::Array(ref arr) = content {
        arr.clone()
    } else {
        vec![]
    };

    let content_types: Vec<Value> = content_arr
        .iter()
        .filter_map(|b| b.get("type").cloned())
        .collect();

    let mut map = serde_json::Map::new();
    if let Some(v) = message.get("model") {
        map.insert("model".to_string(), v.clone());
    }
    if let Some(v) = message.get("stop_reason") {
        map.insert("stop_reason".to_string(), v.clone());
    }
    if let Some(v) = message.get("usage") {
        map.insert("token_usage".to_string(), v.clone());
    }
    if let Some(v) = message.get("id") {
        map.insert("message_id".to_string(), v.clone());
    }
    map.insert("content_types".to_string(), Value::Array(content_types));
    map
}

/// Extract user-specific fields. Returns (subtype, extras_map).
fn extract_user(line: &Value) -> (String, serde_json::Map<String, Value>) {
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

    let mut map = serde_json::Map::new();
    if let Some(v) = line.get("userType") {
        map.insert("user_type".to_string(), v.clone());
    }

    // Extract text to top level for prompt messages
    if subtype == "message.user.prompt" {
        if let Some(text) = extract_text_from_content(&content) {
            map.insert("text".to_string(), Value::String(text));
        }
    }

    (subtype, map)
}

/// Extract progress-specific fields. Returns (subtype, extras_map).
fn extract_progress(line: &Value) -> (String, serde_json::Map<String, Value>) {
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

    let mut map = serde_json::Map::new();
    map.insert("progress_type".to_string(), Value::String(progress_type));
    (subtype, map)
}

/// Extract system-specific fields. Returns (subtype, extras_map).
fn extract_system(line: &Value) -> (String, serde_json::Map<String, Value>) {
    let raw_subtype = line
        .get("subtype")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");

    let mut map = serde_json::Map::new();

    // Map raw system subtypes to hierarchical subtypes
    let subtype = match raw_subtype {
        "turn_duration" => {
            if let Some(v) = line.get("durationMs") {
                map.insert("duration_ms".to_string(), v.clone());
            }
            "system.turn.complete".to_string()
        }
        "stop_hook_summary" => {
            if let Some(v) = line.get("hookCount") {
                map.insert("hook_count".to_string(), v.clone());
            }
            if let Some(v) = line.get("preventedContinuation") {
                map.insert("prevented_continuation".to_string(), v.clone());
            }
            "system.hook".to_string()
        }
        s if s.contains("error") => {
            "system.error".to_string()
        }
        "compact_boundary" => {
            "system.compact".to_string()
        }
        other => format!("system.{other}"),
    };
    (subtype, map)
}

/// Extract queue-operation-specific fields. Returns (subtype, extras_map).
fn extract_queue(line: &Value) -> (String, serde_json::Map<String, Value>) {
    let operation = line
        .get("operation")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();

    let mut map = serde_json::Map::new();
    map.insert("operation".to_string(), Value::String(operation.clone()));
    (format!("queue.{operation}"), map)
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
    let envelope = extract_envelope(line);
    let mut subtype: Option<String> = None;

    let extras: serde_json::Map<String, Value> = match entry_type {
        "assistant" => {
            let mut extras = extract_assistant(line);
            let message = line.get("message").cloned().unwrap_or(Value::Object(Default::default()));
            let content = message.get("content").cloned().unwrap_or(Value::Array(vec![]));
            let ast = determine_assistant_subtype(&content);
            subtype = Some(ast.to_string());

            // Extract text to top level
            let content_normalized = if content.is_string() {
                Value::Array(vec![serde_json::json!({"type": "text", "text": content.as_str().unwrap_or("")})])
            } else {
                content.clone()
            };
            if let Some(text) = extract_text_from_content(&content_normalized) {
                extras.insert("text".to_string(), Value::String(text));
            }
            // Extract tool info to top level for tool_use subtypes
            if ast == "message.assistant.tool_use" {
                if let Some((tool_name, tool_args)) = extract_first_tool_from_content(&content) {
                    extras.insert("tool".to_string(), Value::String(tool_name));
                    extras.insert("args".to_string(), tool_args);
                }
            }
            extras
        }
        "user" => {
            let (st, extras) = extract_user(line);
            subtype = Some(st);
            extras
        }
        "progress" => {
            let (st, extras) = extract_progress(line);
            subtype = Some(st);
            extras
        }
        "system" => {
            let (st, extras) = extract_system(line);
            subtype = Some(st);
            extras
        }
        "queue-operation" => {
            let (st, extras) = extract_queue(line);
            subtype = Some(st);
            extras
        }
        "file-history-snapshot" => {
            subtype = Some("file.snapshot".to_string());
            serde_json::Map::new()
        }
        _ => serde_json::Map::new(),
    };

    // Build data payload
    let mut data = serde_json::Map::new();
    data.insert("raw".to_string(), line.clone());
    data.insert("seq".to_string(), Value::Number(state.next_seq().into()));
    // Filename-derived session_id as fallback; envelope's session_id (from the
    // real sessionId field in the transcript) overwrites this when present.
    data.insert("session_id".to_string(), Value::String(state.session_id.clone()));
    // Merge envelope (may override session_id for sidechain files)
    for (k, v) in envelope {
        data.insert(k, v);
    }
    // Merge extras
    for (k, v) in extras {
        data.insert(k, v);
    }

    let timestamp = line.get("timestamp").and_then(|v| v.as_str()).map(|s| s.to_string());

    vec![CloudEvent::new(
        source,
        IO_ARC_EVENT.to_string(),
        Value::Object(data),
        subtype,
        uuid,
        timestamp,
        None,
        None,
    )]
}
