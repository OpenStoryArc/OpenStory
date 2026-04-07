// Transform CloudEvent into typed ViewRecords.
//
// Two entry points:
//   from_cloud_event(&CloudEvent) — typed access, preferred
//   from_cloud_event_value(&Value) — for stored JSON (deserializes and delegates)

use serde_json::Value;

use open_story_core::cloud_event::CloudEvent;
use open_story_core::event_data::AgentPayload;

use crate::tool_input;
use crate::unified::*;
use crate::view_record::ViewRecord;

/// Normalize legacy CloudEvent type+subtype to unified hierarchical subtype.
///
/// Legacy types: io.arc.transcript.user, io.arc.transcript.assistant,
/// io.arc.transcript.progress, io.arc.prompt.submit, io.arc.tool.call, etc.
/// Unified: io.arc.event with subtype like message.user.prompt, message.assistant.text.
fn normalize_subtype(event_type: &str, raw_subtype: &str) -> String {
    // Already unified format — return as-is
    if event_type == "io.arc.event" || raw_subtype.starts_with("message.") || raw_subtype.starts_with("system.") || raw_subtype.starts_with("progress.") || raw_subtype.starts_with("file.") {
        return raw_subtype.to_string();
    }

    match event_type {
        "io.arc.transcript.user" => match raw_subtype {
            "tool_result" => "message.user.tool_result".to_string(),
            _ => "message.user.prompt".to_string(),
        },
        "io.arc.transcript.assistant" => match raw_subtype {
            "tool_use" => "message.assistant.tool_use".to_string(),
            "thinking" => "message.assistant.thinking".to_string(),
            _ => "message.assistant.text".to_string(),
        },
        "io.arc.transcript.progress" => format!("progress.{}", if raw_subtype.is_empty() { "unknown" } else { raw_subtype }),
        "io.arc.transcript.system" => format!("system.{}", if raw_subtype.is_empty() { "unknown" } else { raw_subtype }),
        "io.arc.transcript.snapshot" => "file.snapshot".to_string(),
        "io.arc.prompt.submit" => "message.user.prompt".to_string(),
        "io.arc.tool.call" => "message.assistant.tool_use".to_string(),
        "io.arc.tool.result" => "message.user.tool_result".to_string(),
        "io.arc.session.start" | "io.arc.session.end" => "system.session".to_string(),
        _ => raw_subtype.to_string(),
    }
}

/// Transform a typed CloudEvent into ViewRecords.
///
/// Returns one or more ViewRecords depending on the event type.
/// For assistant tool_use events, extracts each tool_use content block
/// as a separate ToolCall record.
/// Returns empty vec for unrecognized/malformed events.
/// Handles both unified (io.arc.event) and legacy (io.arc.transcript.*) formats.
pub fn from_cloud_event(event: &CloudEvent) -> Vec<ViewRecord> {
    let id = event.id.clone();
    let time = event.time.clone();
    let subtype_owned = normalize_subtype(
        &event.event_type,
        event.subtype.as_deref().unwrap_or(""),
    );
    let subtype = subtype_owned.as_str();

    // Foundation fields — typed access
    let data = &event.data;
    let seq = data.seq;
    let session_id = data.session_id.clone();
    let raw = &data.raw;
    let agent = event.agent.as_deref().unwrap_or("claude-code");

    // Agent payload — typed dispatch on the enum
    let ap = data.agent_payload.as_ref();

    // Extract subagent identity from payload (Story 037)
    let agent_id = match ap {
        Some(AgentPayload::ClaudeCode(cc)) => cc.agent_id.clone(),
        _ => None,
    };
    let is_sidechain = match ap {
        Some(AgentPayload::ClaudeCode(cc)) => cc.is_sidechain.unwrap_or(false),
        _ => false,
    };

    // Convenience: extract shared fields via AgentPayload accessors
    let text = ap.and_then(|p| p.text()).unwrap_or("");
    let model = ap.and_then(|p| p.model()).unwrap_or("unknown");
    let tool = ap.and_then(|p| p.tool());
    let args = ap.and_then(|p| p.args());
    let token_usage = ap.and_then(|p| p.token_usage());
    let stop_reason = ap.and_then(|p| p.stop_reason_str());

    // Agent-specific field access for duration_ms, hook fields
    let duration_ms = match ap {
        Some(AgentPayload::ClaudeCode(cc)) => cc.duration_ms.map(|v| v as u64),
        _ => None,
    };

    // Build records, then stamp agent identity onto each one
    let mut records = match subtype {
        s if s.starts_with("message.user.prompt") => {
            vec![ViewRecord {
                id,
                seq,
                session_id,
                timestamp: time,
                agent_id: None,
                is_sidechain: false,
                body: RecordBody::UserMessage(UserMessage {
                    content: MessageContent::Text(text.to_string()),
                    images: vec![],
                }),
            }]
        }

        "message.user.tool_result" => {
            // Tool results still need raw content block parsing
            let payload_value = ap
                .map(|p| serde_json::to_value(p).unwrap_or(Value::Null))
                .unwrap_or(Value::Null);
            let tool_outcome = ap.and_then(|p| p.tool_outcome()).cloned();
            extract_tool_results(raw, &payload_value, agent, &id, seq, &session_id, &time, tool_outcome)
        }

        s if s.starts_with("message.assistant.tool_use") => {
            // Tool calls: try typed fields first, fall back to raw content blocks
            if let (Some(tool_name), Some(tool_args)) = (tool, args) {
                // Check raw for multiple tool_use blocks
                let content = raw
                    .get("message")
                    .and_then(|m| m.get("content"));
                let has_multiple = content
                    .and_then(|c| c.as_array())
                    .map(|arr| {
                        let tool_type = if agent == "pi-mono" { "toolCall" } else { "tool_use" };
                        arr.iter().filter(|b| b.get("type").and_then(|v| v.as_str()) == Some(tool_type)).count()
                    })
                    .unwrap_or(0);

                if has_multiple > 1 {
                    // Multiple tool blocks — fall back to raw parsing
                    let payload_value = ap
                        .map(|p| serde_json::to_value(p).unwrap_or(Value::Null))
                        .unwrap_or(Value::Null);
                    extract_tool_calls(raw, &payload_value, agent, &id, seq, &session_id, &time)
                } else {
                    // Single tool — use typed fields. We still pull call_id
                    // from the raw content block, since that's the only place
                    // it lives — without it the ToolCall can't be linked to
                    // its ToolResult downstream (call_id is the join key).
                    let typed = tool_input::parse_tool_input(tool_name, tool_args.clone());
                    let tool_type = if agent == "pi-mono" { "toolCall" } else { "tool_use" };
                    let id_field = if agent == "pi-mono" { "toolUseId" } else { "id" };
                    let call_id = raw
                        .get("message")
                        .and_then(|m| m.get("content"))
                        .and_then(|c| c.as_array())
                        .and_then(|arr| {
                            arr.iter().find(|b| {
                                b.get("type").and_then(|v| v.as_str()) == Some(tool_type)
                            })
                        })
                        .and_then(|b| b.get(id_field))
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    vec![ViewRecord {
                        id,
                        seq,
                        session_id,
                        timestamp: time,
                        agent_id: None,
                        is_sidechain: false,
                        body: RecordBody::ToolCall(Box::new(ToolCall {
                            call_id,
                            name: tool_name.to_string(),
                            input: tool_args.clone(),
                            raw_input: tool_args.clone(),
                            typed_input: Some(typed),
                            status: None,
                        })),
                    }]
                }
            } else {
                // No typed tool fields — fall back to raw content blocks
                let payload_value = ap
                    .map(|p| serde_json::to_value(p).unwrap_or(Value::Null))
                    .unwrap_or(Value::Null);
                extract_tool_calls(raw, &payload_value, agent, &id, seq, &session_id, &time)
            }
        }

        s if s.starts_with("message.assistant.thinking") => {
            extract_reasoning(raw, &id, seq, &session_id, &time)
        }

        s if s.starts_with("message.assistant") => {
            let content = extract_content_blocks(raw);
            let mut records = vec![ViewRecord {
                id: id.clone(),
                seq,
                session_id: session_id.clone(),
                timestamp: time.clone(),
                agent_id: None,
                is_sidechain: false,
                body: RecordBody::AssistantMessage(Box::new(AssistantMessage {
                    model: model.to_string(),
                    content,
                    stop_reason: stop_reason.map(|s| s.into()),
                    end_turn: None,
                    phase: None,
                })),
            }];

            // Emit TokenUsage record if token_usage data is present.
            // Field names differ by agent: Claude Code uses input_tokens/output_tokens,
            // pi-mono uses input/output.
            if let Some(usage) = token_usage {
                let (input_tokens, output_tokens, total_tokens) = match agent {
                    "pi-mono" => (
                        usage.get("input").and_then(|v| v.as_u64()),
                        usage.get("output").and_then(|v| v.as_u64()),
                        usage.get("totalTokens").and_then(|v| v.as_u64()),
                    ),
                    _ => (
                        usage.get("input_tokens").and_then(|v| v.as_u64()),
                        usage.get("output_tokens").and_then(|v| v.as_u64()),
                        usage.get("total_tokens").and_then(|v| v.as_u64()),
                    ),
                };
                if input_tokens.is_some() || output_tokens.is_some() {
                    records.push(ViewRecord {
                        id: format!("{id}:usage"),
                        seq: seq + 1,
                        session_id,
                        timestamp: time,
                        agent_id: None,
                        is_sidechain: false,
                        body: RecordBody::TokenUsage(TokenUsage {
                            input_tokens,
                            output_tokens,
                            total_tokens,
                            scope: TokenScope::Turn,
                        }),
                    });
                }
            }

            records
        }

        "system.turn.complete" => {
            vec![ViewRecord {
                id,
                seq,
                session_id,
                timestamp: time,
                agent_id: None,
                is_sidechain: false,
                body: RecordBody::TurnEnd(TurnEnd {
                    turn_id: None,
                    reason: Some("end_turn".into()),
                    duration_ms,
                }),
            }]
        }

        s if s.starts_with("system.") => {
            vec![ViewRecord {
                id,
                seq,
                session_id,
                timestamp: time,
                agent_id: None,
                is_sidechain: false,
                body: RecordBody::SystemEvent(SystemEvent {
                    subtype: subtype.to_string(),
                    message: if text.is_empty() { None } else { Some(text.to_string()) },
                    duration_ms,
                }),
            }]
        }

        s if s.starts_with("progress.") => {
            vec![ViewRecord {
                id,
                seq,
                session_id,
                timestamp: time,
                agent_id: None,
                is_sidechain: false,
                body: RecordBody::SystemEvent(SystemEvent {
                    subtype: subtype.to_string(),
                    message: None,
                    duration_ms: None,
                }),
            }]
        }

        "file.snapshot" => {
            vec![ViewRecord {
                id,
                seq,
                session_id,
                timestamp: time,
                agent_id: None,
                is_sidechain: false,
                body: RecordBody::FileSnapshot(FileSnapshot {
                    git_commit: None,
                    git_message: None,
                    tracked_files: raw.get("snapshot").cloned(),
                }),
            }]
        }

        _ => {
            // Unknown subtype — skip
            vec![]
        }
    };

    // Stamp subagent identity onto every produced record
    for record in &mut records {
        record.agent_id = agent_id.clone();
        record.is_sidechain = is_sidechain;
    }
    records
}


/// Extract tool_use content blocks into individual ToolCall ViewRecords.
///
/// Branches on `agent` to parse format-specific content blocks:
/// - Claude Code: `type: "tool_use"`, fields: `id`, `name`, `input`
/// - Pi-mono: `type: "toolCall"`, fields: `id`, `name`, `arguments`
fn extract_tool_calls(
    raw: &Value,
    data: &Value,
    agent: &str,
    id: &str,
    seq: u64,
    session_id: &str,
    time: &str,
) -> Vec<ViewRecord> {
    let content = raw
        .get("message")
        .and_then(|m| m.get("content"))
        .unwrap_or(&Value::Null);

    let blocks = match content.as_array() {
        Some(arr) => arr,
        None => {
            // Fall back to top-level tool/args from data
            if let Some(tool_name) = data.get("tool").and_then(|v| v.as_str()) {
                let args = data.get("args").cloned().unwrap_or(Value::Object(Default::default()));
                let typed = tool_input::parse_tool_input(tool_name, args.clone());
                return vec![ViewRecord {
                    id: id.to_string(),
                    seq,
                    session_id: session_id.to_string(),
                    timestamp: time.to_string(),
                    agent_id: None,
                    is_sidechain: false,
                    body: RecordBody::ToolCall(Box::new(ToolCall {
                        call_id: String::new(),
                        name: tool_name.to_string(),
                        input: args.clone(),
                        raw_input: args,
                        typed_input: Some(typed),
                        status: None,
                    })),
                }];
            }
            return vec![];
        }
    };

    // Tool call block type and input field name differ by agent
    let (tool_type, input_key) = match agent {
        "pi-mono" => ("toolCall", "arguments"),
        _ => ("tool_use", "input"),
    };

    let mut records = Vec::new();
    let mut idx = 0;
    for block in blocks {
        let block_type = block.get("type").and_then(|v| v.as_str()).unwrap_or("");
        match block_type {
            t if t == tool_type => {
                let call_id = block.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let name = block.get("name").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();
                let input = block.get(input_key).cloned().unwrap_or(Value::Object(Default::default()));
                let typed = tool_input::parse_tool_input(&name, input.clone());
                let record_id = if idx == 0 { id.to_string() } else { format!("{id}:{idx}") };
                records.push(ViewRecord {
                    id: record_id,
                    seq: seq + idx as u64,
                    session_id: session_id.to_string(),
                    timestamp: time.to_string(),
                    agent_id: None,
                    is_sidechain: false,
                    body: RecordBody::ToolCall(Box::new(ToolCall {
                        call_id,
                        name,
                        input: input.clone(),
                        raw_input: input,
                        typed_input: Some(typed),
                        status: None,
                    })),
                });
                idx += 1;
            }
            "thinking" => {
                let content_text = block.get("thinking").and_then(|v| v.as_str()).map(|s| s.to_string());
                let record_id = if idx == 0 { id.to_string() } else { format!("{id}:{idx}") };
                records.push(ViewRecord {
                    id: record_id,
                    seq: seq + idx as u64,
                    session_id: session_id.to_string(),
                    timestamp: time.to_string(),
                    agent_id: None,
                    is_sidechain: false,
                    body: RecordBody::Reasoning(Reasoning {
                        summary: vec![],
                        content: content_text,
                        encrypted: false,
                    }),
                });
                idx += 1;
            }
            _ => {}
        }
    }
    records
}

/// Extract tool result into ToolResult ViewRecords.
///
/// Branches on `agent`:
/// - Claude Code: content blocks with `type: "tool_result"`, `tool_use_id`, `content`, `is_error`
/// - Pi-mono: message-level `toolCallId`, `toolName`, `isError`; content is text blocks
fn extract_tool_results(
    raw: &Value,
    data: &Value,
    agent: &str,
    id: &str,
    seq: u64,
    session_id: &str,
    time: &str,
    tool_outcome: Option<open_story_core::event_data::ToolOutcome>,
) -> Vec<ViewRecord> {
    match agent {
        "pi-mono" => {
            // Pi-mono: tool result info is on the message itself, not in content blocks
            let message = raw.get("message").unwrap_or(&Value::Null);
            let call_id = message
                .get("toolCallId")
                .or_else(|| data.get("tool_call_id"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let is_error = message
                .get("isError")
                .or_else(|| data.get("is_error"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            // Extract text from content blocks
            let output = message
                .get("content")
                .and_then(|c| c.as_array())
                .map(|blocks| {
                    blocks
                        .iter()
                        .filter_map(|b| {
                            if b.get("type").and_then(|v| v.as_str()) == Some("text") {
                                b.get("text").and_then(|v| v.as_str())
                            } else {
                                None
                            }
                        })
                        .collect::<Vec<&str>>()
                        .join("\n")
                });

            vec![ViewRecord {
                id: id.to_string(),
                seq,
                session_id: session_id.to_string(),
                timestamp: time.to_string(),
                agent_id: None,
                is_sidechain: false,
                body: RecordBody::ToolResult(ToolResult {
                    call_id,
                    output,
                    is_error,
                    tool_outcome: tool_outcome.clone(),
                }),
            }]
        }
        _ => {
            // Claude Code: content blocks with type "tool_result"
            let content = raw
                .get("message")
                .and_then(|m| m.get("content"))
                .unwrap_or(&Value::Null);

            let blocks = match content.as_array() {
                Some(arr) => arr,
                None => return vec![],
            };

            let mut records = Vec::new();
            let mut idx = 0;
            for block in blocks {
                if block.get("type").and_then(|v| v.as_str()) == Some("tool_result") {
                    let call_id = block
                        .get("tool_use_id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let output = block
                        .get("content")
                        .and_then(|v| {
                            if v.is_string() {
                                v.as_str().map(|s| s.to_string())
                            } else {
                                Some(v.to_string())
                            }
                        });
                    let is_error = block
                        .get("is_error")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    let record_id = if idx == 0 { id.to_string() } else { format!("{id}:{idx}") };
                    // tool_outcome applies to the first result (payload-level field)
                    let outcome = if idx == 0 { tool_outcome.clone() } else { None };
                    records.push(ViewRecord {
                        id: record_id,
                        seq: seq + idx as u64,
                        session_id: session_id.to_string(),
                        timestamp: time.to_string(),
                        agent_id: None,
                        is_sidechain: false,
                        body: RecordBody::ToolResult(ToolResult {
                            call_id,
                            output,
                            is_error,
                            tool_outcome: outcome,
                        }),
                    });
                    idx += 1;
                }
            }
            records
        }
    }
}

/// Extract thinking content blocks into Reasoning ViewRecords.
fn extract_reasoning(
    raw: &Value,
    id: &str,
    seq: u64,
    session_id: &str,
    time: &str,
) -> Vec<ViewRecord> {
    let content = raw
        .get("message")
        .and_then(|m| m.get("content"))
        .unwrap_or(&Value::Null);

    let blocks = match content.as_array() {
        Some(arr) => arr,
        None => return vec![],
    };

    let mut records = Vec::new();
    let mut idx = 0;
    for block in blocks {
        if block.get("type").and_then(|v| v.as_str()) == Some("thinking") {
            let content_text = block.get("thinking").and_then(|v| v.as_str()).map(|s| s.to_string());
            let record_id = if idx == 0 { id.to_string() } else { format!("{id}:{idx}") };
            records.push(ViewRecord {
                id: record_id,
                seq: seq + idx as u64,
                session_id: session_id.to_string(),
                timestamp: time.to_string(),
                agent_id: None,
                is_sidechain: false,
                body: RecordBody::Reasoning(Reasoning {
                    summary: vec![],
                    content: content_text,
                    encrypted: false,
                }),
            });
            idx += 1;
        }
    }
    records
}

/// Extract text content blocks into ContentBlock vec.
fn extract_content_blocks(raw: &Value) -> Vec<ContentBlock> {
    let content = raw
        .get("message")
        .and_then(|m| m.get("content"))
        .unwrap_or(&Value::Null);

    match content {
        Value::String(s) => vec![ContentBlock::Text { text: s.clone() }],
        Value::Array(blocks) => {
            blocks
                .iter()
                .filter_map(|b| {
                    let bt = b.get("type").and_then(|v| v.as_str())?;
                    match bt {
                        "text" => {
                            let text = b.get("text").and_then(|v| v.as_str())?.to_string();
                            Some(ContentBlock::Text { text })
                        }
                        _ => None,
                    }
                })
                .collect()
        }
        _ => vec![],
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use open_story_core::cloud_event::CloudEvent;
    use crate::from_cloud_event::from_cloud_event;
    use crate::unified::*;
    use crate::tool_input::ToolInput;

    /// Wrap a "logical" test fixture into the EventData shape the production
    /// code expects. The logical shape has flat fields (seq, session_id, text,
    /// model, tool, args, raw, …) — same shape these tests have always used.
    /// This helper extracts the foundation fields (seq, session_id, raw) and
    /// wraps everything else in an `AgentPayload::ClaudeCode` so the typed
    /// payload accessors in `from_cloud_event` find what they expect.
    ///
    /// This bridges the test fixture shape to the post-AgentPayload-refactor
    /// data model without requiring every test site to know about
    /// `_variant` / `meta.agent` / `ClaudeCodePayload`.
    fn make_event_data(data: serde_json::Value) -> serde_json::Value {
        let mut obj = data.as_object().cloned().unwrap_or_default();
        let seq = obj.remove("seq").unwrap_or(json!(1));
        let session_id = obj.remove("session_id").unwrap_or(json!("sess-test"));
        let raw = obj.remove("raw").unwrap_or(json!({}));

        // Everything else is payload — wrap it in AgentPayload::ClaudeCode shape.
        // The enum is tagged with `_variant` and ClaudeCodePayload requires
        // `meta.agent`. ClaudeCodePayload has `#[serde(flatten)] extra` so any
        // fields that aren't typed columns still survive.
        let mut payload = serde_json::Map::new();
        payload.insert("_variant".to_string(), json!("claude-code"));
        payload.insert("meta".to_string(), json!({"agent": "claude-code"}));
        for (k, v) in obj {
            payload.insert(k, v);
        }

        json!({
            "raw": raw,
            "seq": seq,
            "session_id": session_id,
            "agent_payload": payload,
        })
    }

    fn make_cloud_event(subtype: &str, data: serde_json::Value) -> CloudEvent {
        serde_json::from_value(json!({
            "specversion": "1.0",
            "id": "evt-001",
            "source": "arc://transcript/sess-abc",
            "type": "io.arc.event",
            "time": "2025-01-09T10:00:00Z",
            "datacontenttype": "application/json",
            "subtype": subtype,
            "data": make_event_data(data),
        }))
        .expect("test fixture should deserialize as CloudEvent — \
                 ensure the data block contains required EventData fields \
                 (raw, seq, session_id)")
    }

    // describe("from_cloud_event")
    // describe("when event is io.arc.event with subtype message.user.prompt")
    mod user_prompt {
        use super::*;

        #[test]
        fn it_should_produce_user_message_with_text() {
            let event = make_cloud_event("message.user.prompt", json!({
                "seq": 1,
                "session_id": "sess-abc",
                "text": "Fix the login bug",
                "raw": {
                    "type": "user",
                    "message": {"content": [{"type": "text", "text": "Fix the login bug"}]}
                }
            }));
            let records = from_cloud_event(&event);
            assert_eq!(records.len(), 1);
            assert_eq!(records[0].seq, 1);
            assert_eq!(records[0].session_id, "sess-abc");
            match &records[0].body {
                RecordBody::UserMessage(u) => match &u.content {
                    MessageContent::Text(t) => assert_eq!(t, "Fix the login bug"),
                    other => panic!("expected Text, got {:?}", other),
                },
                other => panic!("expected UserMessage, got {:?}", other),
            }
        }
    }

    // describe("when event is io.arc.event with subtype message.assistant.text")
    mod assistant_text {
        use super::*;

        #[test]
        fn it_should_produce_assistant_message() {
            let event = make_cloud_event("message.assistant.text", json!({
                "seq": 2,
                "session_id": "sess-abc",
                "text": "I'll fix it now.",
                "model": "claude-sonnet-4-20250514",
                "raw": {
                    "type": "assistant",
                    "message": {
                        "model": "claude-sonnet-4-20250514",
                        "content": [{"type": "text", "text": "I'll fix it now."}],
                        "stop_reason": "end_turn"
                    }
                }
            }));
            let records = from_cloud_event(&event);
            assert_eq!(records.len(), 1);
            match &records[0].body {
                RecordBody::AssistantMessage(a) => {
                    assert_eq!(a.model, "claude-sonnet-4-20250514");
                    assert!(a.content.len() >= 1);
                }
                other => panic!("expected AssistantMessage, got {:?}", other),
            }
        }
    }

    // describe("when event is io.arc.event with subtype message.assistant.tool_use")
    mod assistant_tool_use {
        use super::*;

        #[test]
        fn it_should_produce_tool_call_with_typed_input() {
            let event = make_cloud_event("message.assistant.tool_use", json!({
                "seq": 3,
                "session_id": "sess-abc",
                "tool": "Bash",
                "args": {"command": "cargo test"},
                "raw": {
                    "type": "assistant",
                    "message": {
                        "model": "claude-sonnet-4-20250514",
                        "content": [
                            {"type": "tool_use", "id": "toolu_123", "name": "Bash", "input": {"command": "cargo test"}}
                        ]
                    }
                }
            }));
            let records = from_cloud_event(&event);
            // Should have at least one ToolCall record
            let tool_calls: Vec<_> = records.iter().filter(|r| matches!(&r.body, RecordBody::ToolCall(_))).collect();
            assert!(!tool_calls.is_empty(), "should have at least one ToolCall");
            match &tool_calls[0].body {
                RecordBody::ToolCall(tc) => {
                    assert_eq!(tc.name, "Bash");
                    assert_eq!(tc.call_id, "toolu_123");
                    match tc.typed_input.as_ref().unwrap() {
                        ToolInput::Bash(b) => assert_eq!(b.command, "cargo test"),
                        other => panic!("expected Bash, got {:?}", other),
                    }
                }
                _ => unreachable!(),
            }
        }

        #[test]
        fn it_should_produce_unknown_for_mcp_tools() {
            let event = make_cloud_event("message.assistant.tool_use", json!({
                "seq": 4,
                "session_id": "sess-abc",
                "tool": "mcp__slack__post",
                "args": {"channel": "#dev"},
                "raw": {
                    "type": "assistant",
                    "message": {
                        "model": "claude-sonnet-4-20250514",
                        "content": [
                            {"type": "tool_use", "id": "toolu_456", "name": "mcp__slack__post", "input": {"channel": "#dev"}}
                        ]
                    }
                }
            }));
            let records = from_cloud_event(&event);
            let tool_calls: Vec<_> = records.iter().filter(|r| matches!(&r.body, RecordBody::ToolCall(_))).collect();
            assert!(!tool_calls.is_empty());
            match &tool_calls[0].body {
                RecordBody::ToolCall(tc) => {
                    assert!(matches!(tc.typed_input.as_ref().unwrap(), ToolInput::Unknown { .. }));
                }
                _ => unreachable!(),
            }
        }

        #[test]
        fn it_should_preserve_raw_input_alongside_typed() {
            let event = make_cloud_event("message.assistant.tool_use", json!({
                "seq": 5,
                "session_id": "sess-abc",
                "tool": "Edit",
                "args": {"file_path": "/f.rs", "old_string": "a", "new_string": "b"},
                "raw": {
                    "type": "assistant",
                    "message": {
                        "model": "claude-sonnet-4-20250514",
                        "content": [
                            {"type": "tool_use", "id": "toolu_789", "name": "Edit", "input": {"file_path": "/f.rs", "old_string": "a", "new_string": "b"}}
                        ]
                    }
                }
            }));
            let records = from_cloud_event(&event);
            let tool_calls: Vec<_> = records.iter().filter(|r| matches!(&r.body, RecordBody::ToolCall(_))).collect();
            match &tool_calls[0].body {
                RecordBody::ToolCall(tc) => {
                    assert_eq!(tc.raw_input["file_path"], "/f.rs");
                    assert!(tc.typed_input.is_some());
                }
                _ => unreachable!(),
            }
        }
    }

    // describe("when event is io.arc.event with subtype message.user.tool_result")
    mod user_tool_result {
        use super::*;

        #[test]
        fn it_should_produce_tool_result() {
            let event = make_cloud_event("message.user.tool_result", json!({
                "seq": 6,
                "session_id": "sess-abc",
                "raw": {
                    "type": "user",
                    "message": {
                        "content": [
                            {"type": "tool_result", "tool_use_id": "toolu_123", "content": "test result: ok. 5 passed"}
                        ]
                    }
                }
            }));
            let records = from_cloud_event(&event);
            let results: Vec<_> = records.iter().filter(|r| matches!(&r.body, RecordBody::ToolResult(_))).collect();
            assert!(!results.is_empty(), "should have at least one ToolResult");
            match &results[0].body {
                RecordBody::ToolResult(tr) => {
                    assert_eq!(tr.call_id, "toolu_123");
                    assert!(tr.output.as_ref().unwrap().contains("5 passed"));
                }
                _ => unreachable!(),
            }
        }
    }

    // describe("when event is io.arc.event with subtype system.turn.complete")
    mod turn_complete {
        use super::*;

        #[test]
        fn it_should_produce_turn_end_with_duration() {
            let event = make_cloud_event("system.turn.complete", json!({
                "seq": 10,
                "session_id": "sess-abc",
                "duration_ms": 4500,
                "durationMs": 4500,
                "raw": {
                    "type": "system",
                    "subtype": "turn_duration",
                    "durationMs": 4500
                }
            }));
            let records = from_cloud_event(&event);
            assert_eq!(records.len(), 1);
            match &records[0].body {
                RecordBody::TurnEnd(t) => assert_eq!(t.duration_ms, Some(4500)),
                other => panic!("expected TurnEnd, got {:?}", other),
            }
        }
    }

    // describe("when event is io.arc.event with subtype message.assistant.thinking")
    mod assistant_thinking {
        use super::*;

        #[test]
        fn it_should_produce_reasoning_record() {
            let event = make_cloud_event("message.assistant.thinking", json!({
                "seq": 7,
                "session_id": "sess-abc",
                "raw": {
                    "type": "assistant",
                    "message": {
                        "model": "claude-sonnet-4-20250514",
                        "content": [
                            {"type": "thinking", "thinking": "Let me analyze this..."}
                        ]
                    }
                }
            }));
            let records = from_cloud_event(&event);
            let reasoning: Vec<_> = records.iter().filter(|r| matches!(&r.body, RecordBody::Reasoning(_))).collect();
            assert!(!reasoning.is_empty(), "should have Reasoning record");
            match &reasoning[0].body {
                RecordBody::Reasoning(r) => {
                    assert_eq!(r.content, Some("Let me analyze this...".into()));
                }
                _ => unreachable!(),
            }
        }
    }

    // describe("when event uses legacy CloudEvent types")
    // Legacy events use types like io.arc.transcript.user with subtype "text"
    // instead of io.arc.event with subtype "message.user.prompt"
    mod legacy_format {
        use super::*;

        fn make_legacy_event(event_type: &str, subtype: &str, data: serde_json::Value) -> CloudEvent {
            serde_json::from_value(json!({
                "specversion": "1.0",
                "id": "evt-legacy-001",
                "source": "arc://transcript/sess-abc",
                "type": event_type,
                "time": "2025-01-09T10:00:00Z",
                "datacontenttype": "application/json",
                "subtype": subtype,
                "data": super::make_event_data(data),
            }))
            .expect("legacy test fixture should deserialize as CloudEvent")
        }

        #[test]
        fn it_should_produce_user_message_from_transcript_user_text() {
            let event = make_legacy_event("io.arc.transcript.user", "text", json!({
                "seq": 1,
                "session_id": "sess-abc",
                "text": "Hello Claude",
                "raw": {
                    "type": "user",
                    "message": {"content": [{"type": "text", "text": "Hello Claude"}]}
                }
            }));
            let records = from_cloud_event(&event);
            assert!(!records.is_empty(), "legacy transcript.user with subtype text should produce UserMessage");
            match &records[0].body {
                RecordBody::UserMessage(u) => match &u.content {
                    MessageContent::Text(t) => assert_eq!(t, "Hello Claude"),
                    other => panic!("expected Text, got {:?}", other),
                },
                other => panic!("expected UserMessage, got {:?}", other),
            }
        }

        #[test]
        fn it_should_produce_tool_result_from_transcript_user_tool_result() {
            let event = make_legacy_event("io.arc.transcript.user", "tool_result", json!({
                "seq": 2,
                "session_id": "sess-abc",
                "raw": {
                    "type": "user",
                    "message": {
                        "content": [
                            {"type": "tool_result", "tool_use_id": "toolu_abc", "content": "file contents here"}
                        ]
                    }
                }
            }));
            let records = from_cloud_event(&event);
            assert!(!records.is_empty(), "legacy transcript.user with subtype tool_result should produce ToolResult");
            match &records[0].body {
                RecordBody::ToolResult(tr) => {
                    assert_eq!(tr.call_id, "toolu_abc");
                }
                other => panic!("expected ToolResult, got {:?}", other),
            }
        }

        #[test]
        fn it_should_produce_assistant_message_from_transcript_assistant_text() {
            let event = make_legacy_event("io.arc.transcript.assistant", "text", json!({
                "seq": 3,
                "session_id": "sess-abc",
                "model": "claude-opus-4-6",
                "raw": {
                    "type": "assistant",
                    "message": {
                        "model": "claude-opus-4-6",
                        "content": [{"type": "text", "text": "I'll help you with that."}]
                    }
                }
            }));
            let records = from_cloud_event(&event);
            assert!(!records.is_empty(), "legacy transcript.assistant with subtype text should produce AssistantMessage");
            match &records[0].body {
                RecordBody::AssistantMessage(a) => {
                    assert!(!a.content.is_empty());
                }
                other => panic!("expected AssistantMessage, got {:?}", other),
            }
        }

        #[test]
        fn it_should_produce_tool_call_from_transcript_assistant_tool_use() {
            let event = make_legacy_event("io.arc.transcript.assistant", "tool_use", json!({
                "seq": 4,
                "session_id": "sess-abc",
                "tool": "Read",
                "args": {"file_path": "/foo.rs"},
                "raw": {
                    "type": "assistant",
                    "message": {
                        "model": "claude-opus-4-6",
                        "content": [
                            {"type": "tool_use", "id": "toolu_read", "name": "Read", "input": {"file_path": "/foo.rs"}}
                        ]
                    }
                }
            }));
            let records = from_cloud_event(&event);
            let tool_calls: Vec<_> = records.iter().filter(|r| matches!(&r.body, RecordBody::ToolCall(_))).collect();
            assert!(!tool_calls.is_empty(), "legacy transcript.assistant with subtype tool_use should produce ToolCall");
            match &tool_calls[0].body {
                RecordBody::ToolCall(tc) => assert_eq!(tc.name, "Read"),
                _ => unreachable!(),
            }
        }

        #[test]
        fn it_should_produce_system_event_from_transcript_progress() {
            let event = make_legacy_event("io.arc.transcript.progress", "bash", json!({
                "seq": 5,
                "session_id": "sess-abc",
                "raw": {"type": "progress", "subtype": "bash"}
            }));
            let records = from_cloud_event(&event);
            assert!(!records.is_empty(), "legacy transcript.progress should produce SystemEvent");
        }

        #[test]
        fn it_should_produce_user_message_from_prompt_submit() {
            let event = make_legacy_event("io.arc.prompt.submit", "", json!({
                "seq": 6,
                "session_id": "sess-abc",
                "text": "Fix the bug",
                "raw": {
                    "type": "user",
                    "message": {"content": [{"type": "text", "text": "Fix the bug"}]}
                }
            }));
            let records = from_cloud_event(&event);
            assert!(!records.is_empty(), "legacy prompt.submit should produce UserMessage");
            match &records[0].body {
                RecordBody::UserMessage(u) => match &u.content {
                    MessageContent::Text(t) => assert_eq!(t, "Fix the bug"),
                    other => panic!("expected Text, got {:?}", other),
                },
                other => panic!("expected UserMessage, got {:?}", other),
            }
        }

        #[test]
        fn it_should_produce_tool_call_from_tool_call_type() {
            let event = make_legacy_event("io.arc.tool.call", "Read", json!({
                "seq": 7,
                "session_id": "sess-abc",
                "tool": "Read",
                "args": {"file_path": "/bar.rs"},
                "raw": {
                    "type": "assistant",
                    "message": {
                        "content": [
                            {"type": "tool_use", "id": "toolu_tc", "name": "Read", "input": {"file_path": "/bar.rs"}}
                        ]
                    }
                }
            }));
            let records = from_cloud_event(&event);
            let tool_calls: Vec<_> = records.iter().filter(|r| matches!(&r.body, RecordBody::ToolCall(_))).collect();
            assert!(!tool_calls.is_empty(), "legacy tool.call should produce ToolCall");
        }
    }

    // describe("when event is malformed")
    //
    // Note: these tests previously validated graceful handling of malformed
    // JSON when from_cloud_event took an untyped &Value. The function now
    // takes a typed &CloudEvent, so the malformed-input contract has moved
    // upstream to the deserialization layer (watcher/reader). Garbage JSON
    // can't even be constructed as a CloudEvent — serde::from_value rejects
    // it before from_cloud_event is ever called. The tests for that now
    // belong wherever raw JSON is first parsed into a CloudEvent.
    mod malformed {
        use super::*;

        #[test]
        fn malformed_json_fails_to_deserialize_as_cloud_event() {
            let event_json = json!({"garbage": true});
            let result: Result<CloudEvent, _> = serde_json::from_value(event_json);
            assert!(result.is_err(), "garbage JSON must not deserialize as CloudEvent");
        }

        #[test]
        fn missing_data_field_fails_to_deserialize_as_cloud_event() {
            let event_json = json!({
                "type": "io.arc.event",
                "id": "evt-bad",
                "subtype": "message.user.prompt"
            });
            let result: Result<CloudEvent, _> = serde_json::from_value(event_json);
            assert!(result.is_err(), "CloudEvent without data field must not deserialize");
        }
    }

    // describe("when assistant event has token_usage data (Story 048)")
    mod token_usage {
        use super::*;

        #[test]
        fn it_should_emit_token_usage_alongside_assistant_message() {
            let event = make_cloud_event("message.assistant.text", json!({
                "seq": 2,
                "session_id": "sess-abc",
                "model": "claude-sonnet-4-20250514",
                "token_usage": {
                    "input_tokens": 1500,
                    "output_tokens": 350,
                    "total_tokens": 1850
                },
                "raw": {
                    "type": "assistant",
                    "message": {
                        "model": "claude-sonnet-4-20250514",
                        "content": [{"type": "text", "text": "Done."}]
                    }
                }
            }));
            let records = from_cloud_event(&event);
            assert_eq!(records.len(), 2, "should produce AssistantMessage + TokenUsage");
            assert!(matches!(&records[0].body, RecordBody::AssistantMessage(_)));
            match &records[1].body {
                RecordBody::TokenUsage(tu) => {
                    assert_eq!(tu.input_tokens, Some(1500));
                    assert_eq!(tu.output_tokens, Some(350));
                    assert_eq!(tu.total_tokens, Some(1850));
                    assert_eq!(tu.scope, TokenScope::Turn);
                }
                other => panic!("expected TokenUsage, got {:?}", other),
            }
            assert_eq!(records[1].id, "evt-001:usage");
        }

        #[test]
        fn it_should_skip_token_usage_when_absent() {
            let event = make_cloud_event("message.assistant.text", json!({
                "seq": 2,
                "session_id": "sess-abc",
                "model": "claude-sonnet-4-20250514",
                "raw": {
                    "type": "assistant",
                    "message": {
                        "model": "claude-sonnet-4-20250514",
                        "content": [{"type": "text", "text": "No usage data."}]
                    }
                }
            }));
            let records = from_cloud_event(&event);
            assert_eq!(records.len(), 1, "should produce only AssistantMessage");
            assert!(matches!(&records[0].body, RecordBody::AssistantMessage(_)));
        }
    }

    // describe("subagent identity enrichment (Story 037)")
    mod subagent_identity {
        use super::*;

        #[test]
        fn it_should_default_agent_id_to_none_and_is_sidechain_to_false() {
            let event = make_cloud_event("message.user.prompt", json!({
                "seq": 1,
                "session_id": "sess-abc",
                "text": "hi",
                "raw": {"type": "user", "message": {"content": [{"type": "text", "text": "hi"}]}}
            }));
            let records = from_cloud_event(&event);
            assert_eq!(records.len(), 1);
            assert_eq!(records[0].agent_id, None);
            assert_eq!(records[0].is_sidechain, false);
        }

        #[test]
        fn it_should_set_is_sidechain_false_when_present() {
            let event = make_cloud_event("message.user.prompt", json!({
                "seq": 1,
                "session_id": "sess-abc",
                "text": "hi",
                "is_sidechain": false,
                "raw": {"type": "user", "message": {"content": [{"type": "text", "text": "hi"}]}}
            }));
            let records = from_cloud_event(&event);
            assert_eq!(records[0].is_sidechain, false);
            assert_eq!(records[0].agent_id, None);
        }

        #[test]
        fn it_should_set_agent_id_and_is_sidechain_for_subagent_event() {
            let event = make_cloud_event("message.assistant.text", json!({
                "seq": 2,
                "session_id": "sess-abc",
                "is_sidechain": true,
                "agent_id": "agent-abc-123",
                "model": "claude-sonnet-4-20250514",
                "raw": {
                    "type": "assistant",
                    "message": {
                        "model": "claude-sonnet-4-20250514",
                        "content": [{"type": "text", "text": "searching..."}]
                    }
                }
            }));
            let records = from_cloud_event(&event);
            assert_eq!(records.len(), 1);
            assert_eq!(records[0].agent_id, Some("agent-abc-123".to_string()));
            assert_eq!(records[0].is_sidechain, true);
        }

        #[test]
        fn it_should_set_agent_id_from_progress_event_data() {
            let event = make_cloud_event("progress.agent", json!({
                "seq": 3,
                "session_id": "sess-abc",
                "is_sidechain": false,
                "agent_id": "agent-abc-123",
                "parent_tool_use_id": "toolu_xyz_789",
                "progress_type": "agent_progress",
                "raw": {"type": "progress", "data": {"type": "agent_progress"}}
            }));
            let records = from_cloud_event(&event);
            assert_eq!(records.len(), 1);
            assert_eq!(records[0].agent_id, Some("agent-abc-123".to_string()));
            assert_eq!(records[0].is_sidechain, false);
        }

        #[test]
        fn it_should_stamp_agent_identity_on_all_records_from_tool_use() {
            let event = make_cloud_event("message.assistant.tool_use", json!({
                "seq": 4,
                "session_id": "sess-abc",
                "is_sidechain": true,
                "agent_id": "agent-sub-1",
                "tool": "Read",
                "args": {"file_path": "/foo.rs"},
                "raw": {
                    "type": "assistant",
                    "message": {
                        "model": "claude-sonnet-4-20250514",
                        "content": [
                            {"type": "tool_use", "id": "toolu_1", "name": "Read", "input": {"file_path": "/foo.rs"}}
                        ]
                    }
                }
            }));
            let records = from_cloud_event(&event);
            for r in &records {
                assert_eq!(r.agent_id, Some("agent-sub-1".to_string()), "all records should have agent_id");
                assert_eq!(r.is_sidechain, true, "all records should be sidechain");
            }
        }

        #[test]
        fn it_should_skip_serializing_agent_id_when_none() {
            let event = make_cloud_event("message.user.prompt", json!({
                "seq": 1,
                "session_id": "sess-abc",
                "text": "hi",
                "is_sidechain": false,
                "raw": {"type": "user", "message": {"content": [{"type": "text", "text": "hi"}]}}
            }));
            let records = from_cloud_event(&event);
            let json = serde_json::to_value(&records[0]).unwrap();
            assert!(json.get("agent_id").is_none(), "agent_id should not appear in JSON when None");
            assert_eq!(json["is_sidechain"], false);
        }

        #[test]
        fn it_should_serialize_agent_id_when_present() {
            let event = make_cloud_event("message.assistant.text", json!({
                "seq": 2,
                "session_id": "sess-abc",
                "is_sidechain": true,
                "agent_id": "agent-xyz",
                "model": "claude-sonnet-4-20250514",
                "raw": {
                    "type": "assistant",
                    "message": {
                        "model": "claude-sonnet-4-20250514",
                        "content": [{"type": "text", "text": "done"}]
                    }
                }
            }));
            let records = from_cloud_event(&event);
            let json = serde_json::to_value(&records[0]).unwrap();
            assert_eq!(json["agent_id"], "agent-xyz");
            assert_eq!(json["is_sidechain"], true);
        }
    }
}
