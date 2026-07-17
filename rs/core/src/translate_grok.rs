//! Grok Build ACP `updates.jsonl` translator.
//!
//! Grok sessions live under `~/.grok/sessions/{urlencoded-cwd}/{session-id}/`
//! and stream ACP session updates as JSONL lines:
//!
//! ```json
//! {"timestamp":…,"method":"session/update","params":{
//!   "sessionId":"…","update":{"sessionUpdate":"tool_call",…},
//!   "_meta":{"eventId":"…"}
//! }}
//! ```
//!
//! Turn boundaries use method `_x.ai/session/update` with
//! `sessionUpdate: "turn_completed"` — we emit a synthetic
//! `system.turn.complete` so the eval-apply fold crystallizes turns
//! (same pattern as Codex `task_complete` / pi-mono stopReason).
//!
//! `data.raw` is always the original line. Format-awareness stays in views.

use serde_json::Value;
use uuid::Uuid;

use crate::cloud_event::CloudEvent;
use crate::event_data::{
    AgentPayload, EventData, GrokPayload, ToolOutcome, derive_tool_outcome,
};
use crate::translate::{TranscriptState, IO_ARC_EVENT};

const AGENT: &str = "grok-build";

/// Detect a Grok Build ACP updates.jsonl line.
pub fn is_grok_format(line: &Value) -> bool {
    let method = line.get("method").and_then(|v| v.as_str()).unwrap_or("");
    if method != "session/update" && method != "_x.ai/session/update" {
        return false;
    }
    line.pointer("/params/update/sessionUpdate")
        .and_then(|v| v.as_str())
        .is_some()
}

/// Translate one ACP updates.jsonl line into zero or more CloudEvents.
pub fn translate_grok_line(line: &Value, state: &mut TranscriptState) -> Vec<CloudEvent> {
    if !is_grok_format(line) {
        return vec![];
    }

    let params = line.get("params").unwrap_or(&Value::Null);
    if let Some(sid) = params.get("sessionId").and_then(|v| v.as_str()) {
        if !sid.is_empty() {
            state.session_id = sid.to_string();
        }
    }

    let update = params.get("update").unwrap_or(&Value::Null);
    let kind = update
        .get("sessionUpdate")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let event_id = params
        .pointer("/_meta/eventId")
        .or_else(|| update.pointer("/_meta/eventId"))
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .or_else(|| {
            // Stable fallback so boot backfill doesn't invent random ids.
            let seed = format!("{}:{}:{}", state.session_id, state.line_count, kind);
            Some(Uuid::new_v5(&Uuid::NAMESPACE_URL, seed.as_bytes()).to_string())
        });

    if let Some(ref id) = event_id {
        if state.seen_uuids.contains(id) {
            return vec![];
        }
        state.seen_uuids.insert(id.clone());
    }

    let timestamp = grok_timestamp(line);
    let source = format!("grok://session/{}", state.session_id);

    match kind {
        "user_message_chunk" => {
            let text = content_text(update);
            let mut payload = GrokPayload::new();
            payload.text = text;
            payload.model = model_from_update(update);
            payload.prompt_id = prompt_id(params, update);
            vec![make_event(
                line,
                state,
                &source,
                "message.user.prompt",
                payload,
                event_id,
                timestamp,
            )]
        }
        "agent_thought_chunk" => {
            let mut payload = GrokPayload::new();
            payload.text = content_text(update);
            payload.prompt_id = prompt_id(params, update);
            vec![make_event(
                line,
                state,
                &source,
                "message.assistant.thinking",
                payload,
                event_id,
                timestamp,
            )]
        }
        "agent_message_chunk" => {
            let mut payload = GrokPayload::new();
            payload.text = content_text(update);
            payload.model = model_from_update(update);
            payload.prompt_id = prompt_id(params, update);
            vec![make_event(
                line,
                state,
                &source,
                "message.assistant.text",
                payload,
                event_id,
                timestamp,
            )]
        }
        "tool_call" => {
            let tool_call_id = update
                .get("toolCallId")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let tool_name = tool_name_from_update(update);
            let args = update
                .get("rawInput")
                .cloned()
                .unwrap_or(Value::Object(Default::default()));

            if !tool_call_id.is_empty() {
                state.remember_tool_call(&tool_call_id, &tool_name, args.clone());
            }

            let mut payload = GrokPayload::new();
            payload.tool = Some(tool_name);
            payload.tool_call_id = Some(tool_call_id);
            payload.args = Some(args);
            payload.prompt_id = prompt_id(params, update);
            payload.model = model_from_update(update);
            vec![make_event(
                line,
                state,
                &source,
                "message.assistant.tool_use",
                payload,
                event_id,
                timestamp,
            )]
        }
        "tool_call_update" => {
            let status = update.get("status").and_then(|v| v.as_str()).unwrap_or("");
            // Only completed updates become tool_result for the eval-apply fold.
            if status != "completed" {
                return vec![];
            }

            let tool_call_id = update
                .get("toolCallId")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let output_text = raw_output_text(update);
            let (tool_name, args) = state
                .take_tool_call(&tool_call_id)
                .unwrap_or_else(|| {
                    (
                        update
                            .get("title")
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown")
                            .to_string(),
                        update
                            .get("rawInput")
                            .cloned()
                            .unwrap_or(Value::Object(Default::default())),
                    )
                });

            let is_error = update
                .get("isError")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let tool_outcome =
                derive_grok_tool_outcome(&tool_name, &args, output_text.as_deref().unwrap_or(""), is_error);

            let mut payload = GrokPayload::new();
            payload.tool = Some(tool_name);
            payload.tool_call_id = Some(tool_call_id);
            payload.args = Some(args);
            payload.text = output_text;
            payload.tool_outcome = tool_outcome;
            payload.is_error = Some(is_error);
            payload.prompt_id = prompt_id(params, update);
            vec![make_event(
                line,
                state,
                &source,
                "message.user.tool_result",
                payload,
                event_id,
                timestamp,
            )]
        }
        "turn_completed" => {
            let mut payload = GrokPayload::new();
            payload.prompt_id = update
                .get("prompt_id")
                .and_then(|v| v.as_str())
                .map(str::to_string)
                .or_else(|| prompt_id(params, update));
            payload.stop_reason = update
                .get("stop_reason")
                .and_then(|v| v.as_str())
                .map(str::to_string);
            payload.token_usage = update.get("usage").cloned();
            // Synthetic turn boundary for the eval-apply coalgebra.
            vec![make_event(
                line,
                state,
                &source,
                "system.turn.complete",
                payload,
                event_id,
                timestamp,
            )]
        }
        _ => {
            // Unknown ACP update kinds still flow as system passthroughs
            // so the fuzzy pipe preserves sovereignty of the record.
            let mut payload = GrokPayload::new();
            payload.text = content_text(update);
            vec![make_event(
                line,
                state,
                &source,
                &format!("system.grok.{kind}"),
                payload,
                event_id,
                timestamp,
            )]
        }
    }
}

fn make_event(
    line: &Value,
    state: &mut TranscriptState,
    source: &str,
    subtype: &str,
    payload: GrokPayload,
    event_id: Option<String>,
    timestamp: Option<String>,
) -> CloudEvent {
    let data = EventData::with_payload(
        line.clone(),
        state.next_seq(),
        state.session_id.clone(),
        AgentPayload::Grok(payload),
    );
    CloudEvent::new(
        source.to_string(),
        IO_ARC_EVENT.to_string(),
        data,
        Some(subtype.to_string()),
        event_id,
        timestamp,
        None,
        None,
        Some(AGENT.to_string()),
    )
    .with_host(crate::host::host())
    .with_user(crate::user::user())
}

fn content_text(update: &Value) -> Option<String> {
    update
        .pointer("/content/text")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .or_else(|| {
            // content: [{type: content, content: {type: text, text: "..."}}]
            update
                .get("content")
                .and_then(|c| c.as_array())
                .and_then(|arr| {
                    arr.iter().find_map(|block| {
                        block
                            .pointer("/content/text")
                            .or_else(|| block.get("text"))
                            .and_then(|v| v.as_str())
                            .map(str::to_string)
                    })
                })
        })
}

fn model_from_update(update: &Value) -> Option<String> {
    update
        .pointer("/_meta/modelId")
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

fn prompt_id(params: &Value, update: &Value) -> Option<String> {
    params
        .pointer("/_meta/promptId")
        .or_else(|| update.pointer("/_meta/promptId"))
        .or_else(|| update.get("prompt_id"))
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

fn tool_name_from_update(update: &Value) -> String {
    update
        .pointer("/_meta/x.ai/tool/name")
        .and_then(|v| v.as_str())
        .or_else(|| update.get("title").and_then(|v| v.as_str()))
        .unwrap_or("unknown")
        .to_string()
}

fn raw_output_text(update: &Value) -> Option<String> {
    let raw = update.get("rawOutput")?;
    if let Some(s) = raw.as_str() {
        return Some(s.to_string());
    }
    // Nested shapes: {type, Content: {content: "..."}} or {content: "..."}
    if let Some(s) = raw.pointer("/Content/content").and_then(|v| v.as_str()) {
        return Some(s.to_string());
    }
    if let Some(s) = raw.get("content").and_then(|v| v.as_str()) {
        return Some(s.to_string());
    }
    // Last resort: compact JSON (keeps sovereignty of the result).
    Some(raw.to_string())
}

fn grok_timestamp(line: &Value) -> Option<String> {
    match line.get("timestamp") {
        Some(Value::String(s)) => Some(s.clone()),
        Some(Value::Number(n)) => {
            // Unix seconds (possibly with fractional ms stored as int).
            let secs = n.as_i64().or_else(|| n.as_f64().map(|f| f as i64))?;
            // Heuristic: values > 1e12 are ms.
            let (secs, nsecs) = if secs > 1_000_000_000_000 {
                (secs / 1000, ((secs % 1000) * 1_000_000) as u32)
            } else {
                (secs, 0)
            };
            chrono::DateTime::from_timestamp(secs, nsecs)
                .map(|dt| dt.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
        }
        _ => None,
    }
}

/// Map Grok Build tool names onto the shared ToolOutcome vocabulary.
/// Reuses `derive_tool_outcome` where Claude-style names match; otherwise
/// maps Grok-native names (read_file, run_terminal_command, …).
fn derive_grok_tool_outcome(
    tool_name: &str,
    tool_input: &Value,
    result_output: &str,
    is_error: bool,
) -> Option<ToolOutcome> {
    // Try Claude-style names first (shared helper).
    if let Some(o) = derive_tool_outcome(tool_name, tool_input, result_output, is_error) {
        return Some(o);
    }

    match tool_name {
        "read_file" => {
            let path = str_arg(tool_input, &["target_file", "file_path", "path"])
                .unwrap_or("")
                .to_string();
            if is_error {
                Some(ToolOutcome::FileReadFailed {
                    path,
                    reason: result_output.to_string(),
                })
            } else {
                Some(ToolOutcome::FileRead { path })
            }
        }
        "search_replace" | "write" | "Write" | "Edit" => {
            let path = str_arg(tool_input, &["file_path", "path", "target_file"])
                .unwrap_or("")
                .to_string();
            if is_error {
                Some(ToolOutcome::FileWriteFailed {
                    path,
                    reason: result_output.to_string(),
                })
            } else if tool_name.eq_ignore_ascii_case("write") {
                Some(ToolOutcome::FileCreated { path })
            } else {
                Some(ToolOutcome::FileModified { path })
            }
        }
        "run_terminal_command" | "bash" => {
            let command = str_arg(tool_input, &["command"])
                .unwrap_or("")
                .to_string();
            Some(ToolOutcome::CommandExecuted {
                command,
                succeeded: !is_error,
            })
        }
        "list_dir" | "grep" | "search_tool" => {
            let pattern = str_arg(
                tool_input,
                &["pattern", "query", "target_directory", "path"],
            )
            .unwrap_or("")
            .to_string();
            Some(ToolOutcome::SearchPerformed {
                pattern,
                source: "filesystem".to_string(),
            })
        }
        "web_search" => {
            let pattern = str_arg(tool_input, &["query", "q"])
                .unwrap_or("")
                .to_string();
            Some(ToolOutcome::SearchPerformed {
                pattern,
                source: "web".to_string(),
            })
        }
        "spawn_subagent" | "task" => {
            let description = str_arg(tool_input, &["description", "prompt", "name"])
                .unwrap_or("subagent")
                .to_string();
            Some(ToolOutcome::SubAgentSpawned {
                description,
                agent_id: String::new(),
            })
        }
        _ => None,
    }
}

fn str_arg<'a>(args: &'a Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter().find_map(|k| args.get(*k)?.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::translate::TranscriptState;
    use serde_json::json;

    fn state() -> TranscriptState {
        TranscriptState::new("sess-test".into())
    }

    #[test]
    fn detects_session_update_and_xai_method() {
        assert!(is_grok_format(&json!({
            "method": "session/update",
            "params": {"update": {"sessionUpdate": "tool_call"}}
        })));
        assert!(is_grok_format(&json!({
            "method": "_x.ai/session/update",
            "params": {"update": {"sessionUpdate": "turn_completed"}}
        })));
        assert!(!is_grok_format(&json!({"type": "assistant"})));
        assert!(!is_grok_format(&json!({
            "method": "session/update",
            "params": {"update": {}}
        })));
    }

    #[test]
    fn maps_user_chunk_to_prompt() {
        let mut st = state();
        let line = json!({
            "timestamp": 1784235312,
            "method": "session/update",
            "params": {
                "sessionId": "abc",
                "update": {
                    "sessionUpdate": "user_message_chunk",
                    "content": {"type": "text", "text": "hello"}
                },
                "_meta": {"eventId": "e1"}
            }
        });
        let events = translate_grok_line(&line, &mut st);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].subtype.as_deref(), Some("message.user.prompt"));
        assert_eq!(events[0].agent.as_deref(), Some("grok-build"));
        assert_eq!(events[0].data.raw, line);
        assert_eq!(events[0].data.agent_payload.as_ref().unwrap().text(), Some("hello"));
        assert_eq!(st.session_id, "abc");
    }

    #[test]
    fn maps_tool_pair_and_turn_complete() {
        let mut st = state();
        let call = json!({
            "timestamp": 1,
            "method": "session/update",
            "params": {
                "sessionId": "s1",
                "update": {
                    "sessionUpdate": "tool_call",
                    "toolCallId": "call-1",
                    "title": "list_dir",
                    "rawInput": {"target_directory": "/tmp"},
                    "_meta": {"x.ai/tool": {"name": "list_dir"}}
                },
                "_meta": {"eventId": "e-call"}
            }
        });
        let done = json!({
            "timestamp": 2,
            "method": "session/update",
            "params": {
                "sessionId": "s1",
                "update": {
                    "sessionUpdate": "tool_call_update",
                    "toolCallId": "call-1",
                    "status": "completed",
                    "rawOutput": {"type": "ListDir", "Content": {"content": "a\nb"}}
                },
                "_meta": {"eventId": "e-done"}
            }
        });
        let turn = json!({
            "timestamp": 3,
            "method": "_x.ai/session/update",
            "params": {
                "sessionId": "s1",
                "update": {
                    "sessionUpdate": "turn_completed",
                    "prompt_id": "p1",
                    "stop_reason": "end_turn",
                    "usage": {"inputTokens": 10, "outputTokens": 2}
                },
                "_meta": {"eventId": "e-turn"}
            }
        });

        let e1 = translate_grok_line(&call, &mut st);
        assert_eq!(e1[0].subtype.as_deref(), Some("message.assistant.tool_use"));
        assert_eq!(e1[0].data.agent_payload.as_ref().unwrap().tool(), Some("list_dir"));

        let e2 = translate_grok_line(&done, &mut st);
        assert_eq!(e2[0].subtype.as_deref(), Some("message.user.tool_result"));
        assert!(e2[0]
            .data
            .agent_payload
            .as_ref()
            .unwrap()
            .text()
            .unwrap()
            .contains("a\nb"));
        assert!(e2[0].data.agent_payload.as_ref().unwrap().tool_outcome().is_some());

        let e3 = translate_grok_line(&turn, &mut st);
        assert_eq!(e3[0].subtype.as_deref(), Some("system.turn.complete"));
    }

    #[test]
    fn skips_in_progress_tool_updates() {
        let mut st = state();
        let line = json!({
            "method": "session/update",
            "params": {
                "update": {
                    "sessionUpdate": "tool_call_update",
                    "toolCallId": "c",
                    "status": "in_progress"
                },
                "_meta": {"eventId": "x"}
            }
        });
        assert!(translate_grok_line(&line, &mut st).is_empty());
    }

    #[test]
    fn dedupes_by_event_id() {
        let mut st = state();
        let line = json!({
            "method": "session/update",
            "params": {
                "update": {
                    "sessionUpdate": "user_message_chunk",
                    "content": {"type": "text", "text": "hi"}
                },
                "_meta": {"eventId": "same"}
            }
        });
        assert_eq!(translate_grok_line(&line, &mut st).len(), 1);
        assert_eq!(translate_grok_line(&line, &mut st).len(), 0);
    }
}
