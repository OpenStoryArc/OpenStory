//! Codex rollout JSONL translator.
//!
//! Codex TUI sessions are persisted as JSONL records with an outer
//! `{ timestamp, type, payload }` envelope. This adapter keeps that raw line as
//! foundation data and lifts the stable fields OpenStory needs for stories.

use serde_json::Value;

use crate::cloud_event::CloudEvent;
use crate::event_data::{AgentPayload, CodexPayload, EventData};
use crate::translate::{TranscriptState, IO_ARC_EVENT};

const AGENT: &str = "codex";

pub fn is_codex_rollout_format(line: &Value) -> bool {
    let Some(line_type) = line.get("type").and_then(|v| v.as_str()) else {
        return false;
    };

    matches!(
        line_type,
        "session_meta" | "turn_context" | "event_msg" | "response_item" | "compacted"
    ) && line.get("payload").is_some()
}

pub fn translate_codex_line(line: &Value, state: &mut TranscriptState) -> Vec<CloudEvent> {
    if !is_codex_rollout_format(line) {
        return vec![];
    }

    let line_type = line
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    let payload = line.get("payload").unwrap_or(&Value::Null);
    let mut codex = CodexPayload::new();
    codex.timestamp = line
        .get("timestamp")
        .and_then(|v| v.as_str())
        .map(str::to_string);

    let subtype = match line_type {
        "session_meta" => apply_session_meta(payload, &mut codex),
        "turn_context" => apply_turn_context(payload, &mut codex),
        "event_msg" => apply_event_msg(payload, &mut codex),
        "response_item" => apply_response_item(payload, &mut codex),
        "compacted" => Some("system.compact".to_string()),
        _ => None,
    };

    if let Some(thread_id) = codex.thread_id.clone() {
        state.session_id = thread_id;
    }
    let session_id = state.session_id.clone();
    let source = format!("codex://session/{session_id}");
    let data = EventData::with_payload(
        line.clone(),
        state.next_seq(),
        session_id,
        AgentPayload::Codex(codex),
    );

    vec![CloudEvent::new(
        source,
        IO_ARC_EVENT.to_string(),
        data,
        subtype,
        None,
        line.get("timestamp")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        None,
        None,
        Some(AGENT.to_string()),
    )
    .with_host(crate::host::host())
    .with_user(crate::user::user())]
}

fn apply_session_meta(payload: &Value, codex: &mut CodexPayload) -> Option<String> {
    codex.thread_id = string_field(payload, "id");
    codex.cwd = string_field(payload, "cwd");
    codex.cli_version = string_field(payload, "cli_version");
    codex.originator = string_field(payload, "originator");
    if codex.model.is_none() {
        codex.model = string_field(payload, "model");
    }
    Some("system.session_start".to_string())
}

fn apply_turn_context(payload: &Value, codex: &mut CodexPayload) -> Option<String> {
    codex.turn_id = string_field(payload, "turn_id");
    codex.cwd = string_field(payload, "cwd");
    codex.model = string_field(payload, "model");
    Some("system.turn.context".to_string())
}

fn apply_event_msg(payload: &Value, codex: &mut CodexPayload) -> Option<String> {
    let item_type = payload
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();
    codex.item_type = Some(item_type.clone());

    match item_type.as_str() {
        "user_message" => {
            codex.text = string_field(payload, "message");
            Some("message.user.prompt".to_string())
        }
        "agent_message" => {
            codex.text = string_field(payload, "message");
            codex.phase = string_field(payload, "phase");
            Some("message.assistant.text".to_string())
        }
        "token_count" => {
            codex.token_usage = payload.get("info").cloned();
            Some("system.token_count".to_string())
        }
        "task_started" => {
            codex.turn_id = string_field(payload, "turn_id");
            Some("system.task.started".to_string())
        }
        "task_complete" => {
            codex.turn_id = string_field(payload, "turn_id");
            codex.text = string_field(payload, "last_agent_message");
            Some("system.task.complete".to_string())
        }
        other => Some(format!("event_msg.{other}")),
    }
}

fn apply_response_item(payload: &Value, codex: &mut CodexPayload) -> Option<String> {
    let item_type = payload
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();
    codex.item_type = Some(item_type.clone());

    match item_type.as_str() {
        "message" => apply_response_message(payload, codex),
        "reasoning" => {
            codex.text = extract_reasoning_text(payload);
            Some("message.assistant.thinking".to_string())
        }
        "function_call" | "custom_tool_call" => {
            codex.tool = string_field(payload, "name");
            codex.call_id = string_field(payload, "call_id");
            codex.args = payload
                .get("arguments")
                .and_then(|v| v.as_str())
                .and_then(|s| serde_json::from_str::<Value>(s).ok())
                .or_else(|| payload.get("arguments").cloned())
                .or_else(|| payload.get("input").cloned());
            Some("message.assistant.tool_use".to_string())
        }
        "function_call_output" | "custom_tool_call_output" => {
            codex.call_id = string_field(payload, "call_id");
            codex.output = string_field(payload, "output");
            codex.text = codex.output.clone();
            Some("message.user.tool_result".to_string())
        }
        other => Some(format!("response_item.{other}")),
    }
}

fn apply_response_message(payload: &Value, codex: &mut CodexPayload) -> Option<String> {
    codex.phase = string_field(payload, "phase");
    codex.text = extract_content_text(payload.get("content").unwrap_or(&Value::Null));

    match payload.get("role").and_then(|v| v.as_str()) {
        Some("user") => Some("message.user.prompt".to_string()),
        Some("assistant") => Some("message.assistant.text".to_string()),
        Some(role) => Some(format!("message.{role}.text")),
        None => Some("message.unknown.text".to_string()),
    }
}

fn extract_content_text(content: &Value) -> Option<String> {
    let Value::Array(items) = content else {
        return content.as_str().map(str::to_string);
    };

    let text = items
        .iter()
        .filter_map(|item| {
            item.get("text")
                .or_else(|| item.get("content"))
                .and_then(|v| v.as_str())
        })
        .collect::<Vec<_>>()
        .join("\n");
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

fn extract_reasoning_text(payload: &Value) -> Option<String> {
    payload
        .get("content")
        .and_then(extract_content_text)
        .or_else(|| payload.get("summary").and_then(extract_content_text))
}

fn string_field(value: &Value, field: &str) -> Option<String> {
    value
        .get(field)
        .and_then(|v| v.as_str())
        .map(str::to_string)
}
