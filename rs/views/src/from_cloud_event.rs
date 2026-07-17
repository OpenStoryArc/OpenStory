// Transform CloudEvent into typed ViewRecords.
//
// Two entry points:
//   from_cloud_event(&CloudEvent) — typed access, preferred
//   from_cloud_event_value(&Value) — for stored JSON (deserializes and delegates)

use std::sync::OnceLock;

use serde_json::Value;

use open_story_core::cloud_event::CloudEvent;
use open_story_core::event_data::AgentPayload;

use crate::tool_input;
use crate::unified::*;
use crate::view_record::ViewRecord;

// ── Envelope schema — compiled once, cached forever ────────────────────
//
// The minimum viable CloudEvent contract: id + type + time + data.raw.
// Used by from_cloud_event_value to classify events that can't fully
// deserialize as typed CloudEvent structs. Events passing the envelope
// get a SystemEvent passthrough (Tier B); events failing it are truly
// broken (Tier C).
//
// include_str! embeds the schema at compile time — no filesystem access,
// no file-not-found. OnceLock compiles the validator on first use.

static ENVELOPE_VALIDATOR: OnceLock<jsonschema::Validator> = OnceLock::new();

fn envelope_schema() -> &'static jsonschema::Validator {
    ENVELOPE_VALIDATOR.get_or_init(|| {
        // Inlined because the Docker build context is rs/ and the schema
        // files live at the repo root. The canonical copy is at
        // schemas/cloud_event_envelope.schema.json — keep in sync.
        let schema = serde_json::json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "type": "object",
            "required": ["id", "type", "time", "data"],
            "properties": {
                "id": { "type": "string", "minLength": 1 },
                "type": { "type": "string" },
                "time": { "type": "string" },
                "data": {
                    "type": "object",
                    "required": ["raw"],
                    "properties": {
                        "raw": { "type": "object" }
                    }
                }
            }
        });
        jsonschema::validator_for(&schema).expect("compile envelope schema")
    })
}

/// Transform a raw JSON Value into ViewRecords, using schema-based
/// classification when typed deserialization fails.
///
/// This is the fuzzy-pipe entry point for stored/received JSON:
///   - Tier A: deserializes as typed CloudEvent → full enrichment via from_cloud_event
///   - Tier B: fails deserialization, passes envelope schema → SystemEvent passthrough
///   - Tier C: fails envelope → truly broken, empty
pub fn from_cloud_event_value(event_json: &Value) -> Vec<ViewRecord> {
    match serde_json::from_value::<CloudEvent>(event_json.clone()) {
        Ok(ce) => from_cloud_event(&ce),
        Err(_) => {
            if envelope_schema().is_valid(event_json) {
                // Tier B: valid envelope but can't fully type.
                // Produce a passthrough record carrying the subtype + raw.
                let id = event_json
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown")
                    .to_string();
                let time = event_json
                    .get("time")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let subtype = event_json
                    .get("subtype")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown")
                    .to_string();
                let session_id = event_json
                    .pointer("/data/session_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown")
                    .to_string();
                let seq = event_json
                    .pointer("/data/seq")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);

                vec![ViewRecord {
                    id,
                    seq,
                    session_id,
                    timestamp: time,
                    origin_agent: None,
                    agent_id: None,
                    is_sidechain: false,
                    body: RecordBody::SystemEvent(SystemEvent {
                        subtype,
                        message: Some(
                            "event passed envelope validation but could not be fully typed"
                                .to_string(),
                        ),
                        duration_ms: None,
                    }),
                }]
            } else {
                // Tier C: below the sovereignty floor.
                vec![]
            }
        }
    }
}

/// Normalize legacy CloudEvent type+subtype to unified hierarchical subtype.
///
/// Legacy types: io.arc.transcript.user, io.arc.transcript.assistant,
/// io.arc.transcript.progress, io.arc.prompt.submit, io.arc.tool.call, etc.
/// Unified: io.arc.event with subtype like message.user.prompt, message.assistant.text.
fn normalize_subtype(event_type: &str, raw_subtype: &str) -> String {
    // Already unified format — return as-is
    if event_type == "io.arc.event"
        || raw_subtype.starts_with("message.")
        || raw_subtype.starts_with("system.")
        || raw_subtype.starts_with("progress.")
        || raw_subtype.starts_with("file.")
    {
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
        "io.arc.transcript.progress" => format!(
            "progress.{}",
            if raw_subtype.is_empty() {
                "unknown"
            } else {
                raw_subtype
            }
        ),
        "io.arc.transcript.system" => format!(
            "system.{}",
            if raw_subtype.is_empty() {
                "unknown"
            } else {
                raw_subtype
            }
        ),
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
    let subtype_owned =
        normalize_subtype(&event.event_type, event.subtype.as_deref().unwrap_or(""));
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

    // Build records, then stamp origin/subagent identity onto each one.
    let mut records = match subtype {
        s if s.starts_with("message.user.prompt") => {
            vec![ViewRecord {
                id,
                seq,
                session_id,
                timestamp: time,
                origin_agent: None,
                agent_id: None,
                is_sidechain: false,
                body: RecordBody::UserMessage(UserMessage {
                    content: MessageContent::Text(text.to_string()),
                    images: vec![],
                }),
            }]
        }

        "message.user.tool_result" => {
            if agent == "hermes" || agent == "codex" || agent == "grok-build" {
                // Hermes, Codex, and Grok tool results carry call_id and
                // content on the typed payload; there are no Claude-style
                // raw content blocks to parse.
                let (call_id, content_text, is_error, tool_outcome) = match ap {
                    Some(AgentPayload::Hermes(h)) => (
                        h.tool_call_id.clone().unwrap_or_default(),
                        h.text.clone().unwrap_or_default(),
                        false,
                        None,
                    ),
                    Some(AgentPayload::Codex(c)) => (
                        c.call_id.clone().unwrap_or_default(),
                        c.output
                            .clone()
                            .or_else(|| c.text.clone())
                            .unwrap_or_default(),
                        false,
                        None,
                    ),
                    Some(AgentPayload::Grok(g)) => (
                        g.tool_call_id.clone().unwrap_or_default(),
                        g.text.clone().unwrap_or_default(),
                        g.is_error.unwrap_or(false),
                        g.tool_outcome.clone(),
                    ),
                    _ => (String::new(), text.to_string(), false, None),
                };
                vec![ViewRecord {
                    id,
                    seq,
                    session_id,
                    timestamp: time,
                    origin_agent: None,
                    agent_id: None,
                    is_sidechain: false,
                    body: RecordBody::ToolResult(ToolResult {
                        call_id,
                        output: Some(content_text),
                        is_error,
                        tool_outcome,
                    }),
                }]
            } else {
                // Claude Code / pi-mono: raw content block parsing
                let payload_value = ap
                    .map(|p| serde_json::to_value(p).unwrap_or(Value::Null))
                    .unwrap_or(Value::Null);
                let tool_outcome = ap.and_then(|p| p.tool_outcome()).cloned();
                extract_tool_results(
                    raw,
                    &payload_value,
                    agent,
                    &id,
                    seq,
                    &session_id,
                    &time,
                    tool_outcome,
                )
            }
        }

        s if s.starts_with("message.assistant.tool_use") => {
            // Tool calls: try typed fields first, fall back to raw content blocks
            if let (Some(tool_name), Some(tool_args)) = (tool, args) {
                // Hermes: tool_use_id is on the typed payload — no raw content
                // block parsing needed. Hermes's translator already fans out
                // one CloudEvent per tool call, so there are never "multiple
                // tool blocks in one event" like Claude Code has.
                if agent == "hermes" {
                    let call_id = match ap {
                        Some(AgentPayload::Hermes(h)) => h.tool_use_id.clone().unwrap_or_default(),
                        _ => String::new(),
                    };
                    let typed = tool_input::parse_tool_input(tool_name, tool_args.clone());
                    vec![ViewRecord {
                        id,
                        seq,
                        session_id,
                        timestamp: time,
                        origin_agent: None,
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
                } else if agent == "codex" {
                    let call_id = match ap {
                        Some(AgentPayload::Codex(c)) => c.call_id.clone().unwrap_or_default(),
                        _ => String::new(),
                    };
                    let typed = tool_input::parse_tool_input(tool_name, tool_args.clone());
                    vec![ViewRecord {
                        id,
                        seq,
                        session_id,
                        timestamp: time,
                        origin_agent: None,
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
                } else if agent == "pi-mono" {
                    // Pi-mono (decomposed): each tool_use CloudEvent has typed
                    // payload fields (tool, tool_call_id, args) from the decomposer.
                    // Use them directly — same pattern as Hermes above.
                    let call_id = match ap {
                        Some(AgentPayload::PiMono(p)) => p.tool_call_id.clone().unwrap_or_default(),
                        _ => String::new(),
                    };
                    let typed = tool_input::parse_tool_input(tool_name, tool_args.clone());
                    vec![ViewRecord {
                        id,
                        seq,
                        session_id,
                        timestamp: time,
                        origin_agent: None,
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
                } else if agent == "grok-build" {
                    let call_id = match ap {
                        Some(AgentPayload::Grok(g)) => g.tool_call_id.clone().unwrap_or_default(),
                        _ => String::new(),
                    };
                    let typed = tool_input::parse_tool_input(tool_name, tool_args.clone());
                    vec![ViewRecord {
                        id,
                        seq,
                        session_id,
                        timestamp: time,
                        origin_agent: None,
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
                } else {
                    // Claude Code: check raw for multiple tool_use blocks
                    let content = raw.get("message").and_then(|m| m.get("content"));
                    let has_multiple = content
                        .and_then(|c| c.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter(|b| {
                                    b.get("type").and_then(|v| v.as_str()) == Some("tool_use")
                                })
                                .count()
                        })
                        .unwrap_or(0);

                    if has_multiple > 1 {
                        // Multiple tool blocks — fall back to raw parsing
                        let payload_value = ap
                            .map(|p| serde_json::to_value(p).unwrap_or(Value::Null))
                            .unwrap_or(Value::Null);
                        extract_tool_calls(raw, &payload_value, agent, &id, seq, &session_id, &time)
                    } else {
                        // Single tool — use typed fields. Pull call_id from raw content block.
                        let typed = tool_input::parse_tool_input(tool_name, tool_args.clone());
                        let call_id = raw
                            .get("message")
                            .and_then(|m| m.get("content"))
                            .and_then(|c| c.as_array())
                            .and_then(|arr| {
                                arr.iter().find(|b| {
                                    b.get("type").and_then(|v| v.as_str()) == Some("tool_use")
                                })
                            })
                            .and_then(|b| b.get("id"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        vec![ViewRecord {
                            id,
                            seq,
                            session_id,
                            timestamp: time,
                            origin_agent: None,
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
            // Codex + Grok: thinking text lives on the typed payload, not Claude
            // raw thinking blocks. Same path as hermes/codex assistant text.
            if (agent == "codex" || agent == "grok-build") && !text.is_empty() {
                vec![ViewRecord {
                    id,
                    seq,
                    session_id,
                    timestamp: time,
                    origin_agent: None,
                    agent_id: None,
                    is_sidechain: false,
                    body: RecordBody::Reasoning(Reasoning {
                        summary: vec![],
                        content: Some(text.to_string()),
                        encrypted: false,
                    }),
                }]
            } else {
                extract_reasoning(raw, &id, seq, &session_id, &time)
            }
        }

        s if s.starts_with("message.assistant") => {
            // Hermes/Codex/Grok: content is on the typed payload (text accessor),
            // not in Claude-style raw content blocks. pi-mono uses raw blocks
            // that extract_content_blocks parses.
            let content = if agent == "hermes" || agent == "codex" || agent == "grok-build" {
                // Wrap it as a single Text content block for the views layer.
                if text.is_empty() {
                    vec![]
                } else {
                    vec![ContentBlock::Text {
                        text: text.to_string(),
                    }]
                }
            } else {
                extract_content_blocks(raw)
            };
            let mut records = vec![ViewRecord {
                id: id.clone(),
                seq,
                session_id: session_id.clone(),
                timestamp: time.clone(),
                origin_agent: None,
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
            // Field names differ by agent: Claude Code uses input_tokens/output_tokens
            // plus cache_creation_input_tokens/cache_read_input_tokens; pi-mono
            // uses input/output plus cacheWrite/cacheRead. Both are preserved.
            if let Some(usage) = token_usage {
                let (input_tokens, output_tokens, total_tokens, cache_creation, cache_read) =
                    match agent {
                        "pi-mono" => (
                            usage.get("input").and_then(|v| v.as_u64()),
                            usage.get("output").and_then(|v| v.as_u64()),
                            usage.get("totalTokens").and_then(|v| v.as_u64()),
                            usage.get("cacheWrite").and_then(|v| v.as_u64()),
                            usage.get("cacheRead").and_then(|v| v.as_u64()),
                        ),
                        _ => (
                            usage.get("input_tokens").and_then(|v| v.as_u64()),
                            usage.get("output_tokens").and_then(|v| v.as_u64()),
                            usage.get("total_tokens").and_then(|v| v.as_u64()),
                            usage
                                .get("cache_creation_input_tokens")
                                .and_then(|v| v.as_u64()),
                            usage
                                .get("cache_read_input_tokens")
                                .and_then(|v| v.as_u64()),
                        ),
                    };
                if input_tokens.is_some() || output_tokens.is_some() {
                    records.push(ViewRecord {
                        id: format!("{id}:usage"),
                        seq: seq + 1,
                        session_id,
                        timestamp: time,
                        origin_agent: None,
                        agent_id: None,
                        is_sidechain: false,
                        body: RecordBody::TokenUsage(TokenUsage {
                            input_tokens,
                            output_tokens,
                            total_tokens,
                            cache_creation_input_tokens: cache_creation,
                            cache_read_input_tokens: cache_read,
                            scope: TokenScope::Turn,
                        }),
                    });
                }
            }

            records
        }

        "system.turn.complete" => {
            let reason = stop_reason
                .map(|s| s.to_string())
                .or_else(|| Some("end_turn".into()));
            let mut records = vec![ViewRecord {
                id: id.clone(),
                seq,
                session_id: session_id.clone(),
                timestamp: time.clone(),
                origin_agent: None,
                agent_id: None,
                is_sidechain: false,
                body: RecordBody::TurnEnd(TurnEnd {
                    turn_id: None,
                    reason,
                    duration_ms,
                }),
            }];
            // Grok (and others) attach turn-scoped usage on turn_completed.
            // Field names differ: Claude snake_case, pi-mono camel input/output,
            // Grok ACP camelCase inputTokens/outputTokens/cachedReadTokens.
            if let Some(usage) = token_usage {
                if let Some(tu) = token_usage_from_agent_map(agent, usage) {
                    records.push(ViewRecord {
                        id: format!("{id}:usage"),
                        seq: seq + 1,
                        session_id,
                        timestamp: time,
                        origin_agent: None,
                        agent_id: None,
                        is_sidechain: false,
                        body: RecordBody::TokenUsage(tu),
                    });
                }
            }
            records
        }

        s if s.starts_with("system.") => {
            vec![ViewRecord {
                id,
                seq,
                session_id,
                timestamp: time,
                origin_agent: None,
                agent_id: None,
                is_sidechain: false,
                body: RecordBody::SystemEvent(SystemEvent {
                    subtype: subtype.to_string(),
                    message: if text.is_empty() {
                        None
                    } else {
                        Some(text.to_string())
                    },
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
                origin_agent: None,
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
                origin_agent: None,
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
            // Fuzzy pipe: unknown subtypes still produce a record so the
            // event flows through the broadcast path. The raw data is on
            // the CloudEvent; this record makes the event visible in the
            // UI as a SystemEvent with the unrecognized subtype shown as
            // the label. The pipeline enriches what it can, never drops
            // what it can't.
            //
            // See rs/tests/test_fuzzy_pipe.rs for the test that enforces
            // this: totally_unknown_prefix_still_produces_records.
            vec![ViewRecord {
                id,
                seq,
                session_id,
                timestamp: time,
                origin_agent: None,
                agent_id: None,
                is_sidechain: false,
                body: RecordBody::SystemEvent(SystemEvent {
                    subtype: subtype.to_string(),
                    message: None,
                    duration_ms: None,
                }),
            }]
        }
    };

    // Stamp subagent identity onto every produced record
    for record in &mut records {
        record.origin_agent = Some(agent.to_string());
        record.agent_id = agent_id.clone();
        record.is_sidechain = is_sidechain;
    }
    records
}

/// Map agent-native token_usage JSON into a TokenUsage view body.
/// Returns None when neither input nor output counts are present.
fn token_usage_from_agent_map(agent: &str, usage: &Value) -> Option<TokenUsage> {
    let (input_tokens, output_tokens, total_tokens, cache_creation, cache_read) = match agent {
        "pi-mono" => (
            usage.get("input").and_then(|v| v.as_u64()),
            usage.get("output").and_then(|v| v.as_u64()),
            usage.get("totalTokens").and_then(|v| v.as_u64()),
            usage.get("cacheWrite").and_then(|v| v.as_u64()),
            usage.get("cacheRead").and_then(|v| v.as_u64()),
        ),
        // Grok Build ACP turn_completed.usage (camelCase).
        "grok-build" | "grok" => (
            usage
                .get("inputTokens")
                .or_else(|| usage.get("input_tokens"))
                .and_then(|v| v.as_u64()),
            usage
                .get("outputTokens")
                .or_else(|| usage.get("output_tokens"))
                .and_then(|v| v.as_u64()),
            usage
                .get("totalTokens")
                .or_else(|| usage.get("total_tokens"))
                .and_then(|v| v.as_u64()),
            usage
                .get("cachedWriteTokens")
                .or_else(|| usage.get("cache_creation_input_tokens"))
                .and_then(|v| v.as_u64()),
            usage
                .get("cachedReadTokens")
                .or_else(|| usage.get("cache_read_input_tokens"))
                .and_then(|v| v.as_u64()),
        ),
        _ => (
            usage.get("input_tokens").and_then(|v| v.as_u64()),
            usage.get("output_tokens").and_then(|v| v.as_u64()),
            usage.get("total_tokens").and_then(|v| v.as_u64()),
            usage
                .get("cache_creation_input_tokens")
                .and_then(|v| v.as_u64()),
            usage
                .get("cache_read_input_tokens")
                .and_then(|v| v.as_u64()),
        ),
    };
    if input_tokens.is_none() && output_tokens.is_none() {
        return None;
    }
    Some(TokenUsage {
        input_tokens,
        output_tokens,
        total_tokens,
        cache_creation_input_tokens: cache_creation,
        cache_read_input_tokens: cache_read,
        scope: TokenScope::Turn,
    })
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
                let args = data
                    .get("args")
                    .cloned()
                    .unwrap_or(Value::Object(Default::default()));
                let typed = tool_input::parse_tool_input(tool_name, args.clone());
                return vec![ViewRecord {
                    id: id.to_string(),
                    seq,
                    session_id: session_id.to_string(),
                    timestamp: time.to_string(),
                    origin_agent: None,
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
                let call_id = block
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let name = block
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown")
                    .to_string();
                let input = block
                    .get(input_key)
                    .cloned()
                    .unwrap_or(Value::Object(Default::default()));
                let typed = tool_input::parse_tool_input(&name, input.clone());
                let record_id = if idx == 0 {
                    id.to_string()
                } else {
                    format!("{id}:{idx}")
                };
                records.push(ViewRecord {
                    id: record_id,
                    seq: seq + idx as u64,
                    session_id: session_id.to_string(),
                    timestamp: time.to_string(),
                    origin_agent: None,
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
                let content_text = block
                    .get("thinking")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let record_id = if idx == 0 {
                    id.to_string()
                } else {
                    format!("{id}:{idx}")
                };
                records.push(ViewRecord {
                    id: record_id,
                    seq: seq + idx as u64,
                    session_id: session_id.to_string(),
                    timestamp: time.to_string(),
                    origin_agent: None,
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
#[allow(clippy::too_many_arguments)]
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
                origin_agent: None,
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
                    let output = block.get("content").and_then(|v| {
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
                    let record_id = if idx == 0 {
                        id.to_string()
                    } else {
                        format!("{id}:{idx}")
                    };
                    // tool_outcome applies to the first result (payload-level field)
                    let outcome = if idx == 0 { tool_outcome.clone() } else { None };
                    records.push(ViewRecord {
                        id: record_id,
                        seq: seq + idx as u64,
                        session_id: session_id.to_string(),
                        timestamp: time.to_string(),
                        origin_agent: None,
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
            let content_text = block
                .get("thinking")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let record_id = if idx == 0 {
                id.to_string()
            } else {
                format!("{id}:{idx}")
            };
            records.push(ViewRecord {
                id: record_id,
                seq: seq + idx as u64,
                session_id: session_id.to_string(),
                timestamp: time.to_string(),
                origin_agent: None,
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
        Value::Array(blocks) => blocks
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
            .collect(),
        _ => vec![],
    }
}

#[cfg(test)]
mod tests {
    use crate::from_cloud_event::from_cloud_event;
    use crate::tool_input::ToolInput;
    use crate::unified::*;
    use open_story_core::cloud_event::CloudEvent;
    use serde_json::json;

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
        .expect(
            "test fixture should deserialize as CloudEvent — \
                 ensure the data block contains required EventData fields \
                 (raw, seq, session_id)",
        )
    }

    // describe("from_cloud_event")
    // describe("when event is io.arc.event with subtype message.user.prompt")
    mod user_prompt {
        use super::*;

        #[test]
        fn it_should_produce_user_message_with_text() {
            let event = make_cloud_event(
                "message.user.prompt",
                json!({
                    "seq": 1,
                    "session_id": "sess-abc",
                    "text": "Fix the login bug",
                    "raw": {
                        "type": "user",
                        "message": {"content": [{"type": "text", "text": "Fix the login bug"}]}
                    }
                }),
            );
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

        #[test]
        fn it_should_preserve_origin_agent_on_view_record() {
            let event = make_cloud_event(
                "message.user.prompt",
                json!({
                    "seq": 1,
                    "session_id": "sess-abc",
                    "text": "Fix the login bug",
                    "raw": {
                        "type": "user",
                        "message": {"content": [{"type": "text", "text": "Fix the login bug"}]}
                    }
                }),
            );

            let records = from_cloud_event(&event);

            assert_eq!(records[0].origin_agent.as_deref(), Some("claude-code"));
        }
    }

    // describe("when event is io.arc.event with subtype message.assistant.text")
    mod assistant_text {
        use super::*;

        #[test]
        fn it_should_produce_assistant_message() {
            let event = make_cloud_event(
                "message.assistant.text",
                json!({
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
                }),
            );
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
            let event = make_cloud_event(
                "message.assistant.tool_use",
                json!({
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
                }),
            );
            let records = from_cloud_event(&event);
            // Should have at least one ToolCall record
            let tool_calls: Vec<_> = records
                .iter()
                .filter(|r| matches!(&r.body, RecordBody::ToolCall(_)))
                .collect();
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
            let event = make_cloud_event(
                "message.assistant.tool_use",
                json!({
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
                }),
            );
            let records = from_cloud_event(&event);
            let tool_calls: Vec<_> = records
                .iter()
                .filter(|r| matches!(&r.body, RecordBody::ToolCall(_)))
                .collect();
            assert!(!tool_calls.is_empty());
            match &tool_calls[0].body {
                RecordBody::ToolCall(tc) => {
                    assert!(matches!(
                        tc.typed_input.as_ref().unwrap(),
                        ToolInput::Unknown { .. }
                    ));
                }
                _ => unreachable!(),
            }
        }

        #[test]
        fn it_should_preserve_raw_input_alongside_typed() {
            let event = make_cloud_event(
                "message.assistant.tool_use",
                json!({
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
                }),
            );
            let records = from_cloud_event(&event);
            let tool_calls: Vec<_> = records
                .iter()
                .filter(|r| matches!(&r.body, RecordBody::ToolCall(_)))
                .collect();
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
            let event = make_cloud_event(
                "message.user.tool_result",
                json!({
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
                }),
            );
            let records = from_cloud_event(&event);
            let results: Vec<_> = records
                .iter()
                .filter(|r| matches!(&r.body, RecordBody::ToolResult(_)))
                .collect();
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
            let event = make_cloud_event(
                "system.turn.complete",
                json!({
                    "seq": 10,
                    "session_id": "sess-abc",
                    "duration_ms": 4500,
                    "durationMs": 4500,
                    "raw": {
                        "type": "system",
                        "subtype": "turn_duration",
                        "durationMs": 4500
                    }
                }),
            );
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
            let event = make_cloud_event(
                "message.assistant.thinking",
                json!({
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
                }),
            );
            let records = from_cloud_event(&event);
            let reasoning: Vec<_> = records
                .iter()
                .filter(|r| matches!(&r.body, RecordBody::Reasoning(_)))
                .collect();
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

        fn make_legacy_event(
            event_type: &str,
            subtype: &str,
            data: serde_json::Value,
        ) -> CloudEvent {
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
            let event = make_legacy_event(
                "io.arc.transcript.user",
                "text",
                json!({
                    "seq": 1,
                    "session_id": "sess-abc",
                    "text": "Hello Claude",
                    "raw": {
                        "type": "user",
                        "message": {"content": [{"type": "text", "text": "Hello Claude"}]}
                    }
                }),
            );
            let records = from_cloud_event(&event);
            assert!(
                !records.is_empty(),
                "legacy transcript.user with subtype text should produce UserMessage"
            );
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
            let event = make_legacy_event(
                "io.arc.transcript.user",
                "tool_result",
                json!({
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
                }),
            );
            let records = from_cloud_event(&event);
            assert!(
                !records.is_empty(),
                "legacy transcript.user with subtype tool_result should produce ToolResult"
            );
            match &records[0].body {
                RecordBody::ToolResult(tr) => {
                    assert_eq!(tr.call_id, "toolu_abc");
                }
                other => panic!("expected ToolResult, got {:?}", other),
            }
        }

        #[test]
        fn it_should_produce_assistant_message_from_transcript_assistant_text() {
            let event = make_legacy_event(
                "io.arc.transcript.assistant",
                "text",
                json!({
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
                }),
            );
            let records = from_cloud_event(&event);
            assert!(
                !records.is_empty(),
                "legacy transcript.assistant with subtype text should produce AssistantMessage"
            );
            match &records[0].body {
                RecordBody::AssistantMessage(a) => {
                    assert!(!a.content.is_empty());
                }
                other => panic!("expected AssistantMessage, got {:?}", other),
            }
        }

        #[test]
        fn it_should_produce_tool_call_from_transcript_assistant_tool_use() {
            let event = make_legacy_event(
                "io.arc.transcript.assistant",
                "tool_use",
                json!({
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
                }),
            );
            let records = from_cloud_event(&event);
            let tool_calls: Vec<_> = records
                .iter()
                .filter(|r| matches!(&r.body, RecordBody::ToolCall(_)))
                .collect();
            assert!(
                !tool_calls.is_empty(),
                "legacy transcript.assistant with subtype tool_use should produce ToolCall"
            );
            match &tool_calls[0].body {
                RecordBody::ToolCall(tc) => assert_eq!(tc.name, "Read"),
                _ => unreachable!(),
            }
        }

        #[test]
        fn it_should_produce_system_event_from_transcript_progress() {
            let event = make_legacy_event(
                "io.arc.transcript.progress",
                "bash",
                json!({
                    "seq": 5,
                    "session_id": "sess-abc",
                    "raw": {"type": "progress", "subtype": "bash"}
                }),
            );
            let records = from_cloud_event(&event);
            assert!(
                !records.is_empty(),
                "legacy transcript.progress should produce SystemEvent"
            );
        }

        #[test]
        fn it_should_produce_user_message_from_prompt_submit() {
            let event = make_legacy_event(
                "io.arc.prompt.submit",
                "",
                json!({
                    "seq": 6,
                    "session_id": "sess-abc",
                    "text": "Fix the bug",
                    "raw": {
                        "type": "user",
                        "message": {"content": [{"type": "text", "text": "Fix the bug"}]}
                    }
                }),
            );
            let records = from_cloud_event(&event);
            assert!(
                !records.is_empty(),
                "legacy prompt.submit should produce UserMessage"
            );
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
            let event = make_legacy_event(
                "io.arc.tool.call",
                "Read",
                json!({
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
                }),
            );
            let records = from_cloud_event(&event);
            let tool_calls: Vec<_> = records
                .iter()
                .filter(|r| matches!(&r.body, RecordBody::ToolCall(_)))
                .collect();
            assert!(
                !tool_calls.is_empty(),
                "legacy tool.call should produce ToolCall"
            );
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
            assert!(
                result.is_err(),
                "garbage JSON must not deserialize as CloudEvent"
            );
        }

        #[test]
        fn missing_data_field_fails_to_deserialize_as_cloud_event() {
            let event_json = json!({
                "type": "io.arc.event",
                "id": "evt-bad",
                "subtype": "message.user.prompt"
            });
            let result: Result<CloudEvent, _> = serde_json::from_value(event_json);
            assert!(
                result.is_err(),
                "CloudEvent without data field must not deserialize"
            );
        }
    }

    // describe("when assistant event has token_usage data (Story 048)")
    mod token_usage {
        use super::*;

        #[test]
        fn it_should_emit_token_usage_alongside_assistant_message() {
            let event = make_cloud_event(
                "message.assistant.text",
                json!({
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
                }),
            );
            let records = from_cloud_event(&event);
            assert_eq!(
                records.len(),
                2,
                "should produce AssistantMessage + TokenUsage"
            );
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
            let event = make_cloud_event(
                "message.assistant.text",
                json!({
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
                }),
            );
            let records = from_cloud_event(&event);
            assert_eq!(records.len(), 1, "should produce only AssistantMessage");
            assert!(matches!(&records[0].body, RecordBody::AssistantMessage(_)));
        }

        // describe("when a Claude Code event carries prompt-cache accounting")
        #[test]
        fn it_should_preserve_claude_code_cache_creation_and_cache_read_tokens() {
            let event = make_cloud_event(
                "message.assistant.text",
                json!({
                    "seq": 2,
                    "session_id": "sess-cache",
                    "model": "claude-sonnet-4-20250514",
                    "token_usage": {
                        "input_tokens": 6,
                        "output_tokens": 1,
                        "cache_creation_input_tokens": 23686,
                        "cache_read_input_tokens": 26875
                    },
                    "raw": {
                        "type": "assistant",
                        "message": {
                            "model": "claude-sonnet-4-20250514",
                            "content": [{"type": "text", "text": "Reading."}]
                        }
                    }
                }),
            );
            let records = from_cloud_event(&event);
            assert_eq!(records.len(), 2);
            match &records[1].body {
                RecordBody::TokenUsage(tu) => {
                    assert_eq!(tu.input_tokens, Some(6));
                    assert_eq!(tu.output_tokens, Some(1));
                    assert_eq!(
                        tu.cache_creation_input_tokens,
                        Some(23686),
                        "cache_creation_input_tokens must be preserved — \
                         it's the bulk of what a turn actually costs"
                    );
                    assert_eq!(
                        tu.cache_read_input_tokens,
                        Some(26875),
                        "cache_read_input_tokens must be preserved — \
                         high cache_read = an efficient prompt design signal"
                    );
                }
                other => panic!("expected TokenUsage, got {:?}", other),
            }
        }

        // TODO: pi-mono view-layer cache-field test. The production code at
        // from_cloud_event.rs maps pi-mono's `cacheWrite`/`cacheRead` onto
        // the same view-layer fields, but a proper test needs a PiMonoPayload
        // fixture that satisfies pi-mono's required fields. The translator
        // side is already covered by translate_pi.rs::test_token_usage_preserved_native.

        // describe("when cache fields are absent (older events)")
        #[test]
        fn it_should_leave_cache_fields_none_when_not_supplied() {
            let event = make_cloud_event(
                "message.assistant.text",
                json!({
                    "seq": 2,
                    "session_id": "sess-old",
                    "model": "claude-sonnet-4-20250514",
                    "token_usage": {
                        "input_tokens": 100,
                        "output_tokens": 50
                    },
                    "raw": {
                        "type": "assistant",
                        "message": {
                            "model": "claude-sonnet-4-20250514",
                            "content": [{"type": "text", "text": "Hi."}]
                        }
                    }
                }),
            );
            let records = from_cloud_event(&event);
            match &records[1].body {
                RecordBody::TokenUsage(tu) => {
                    assert_eq!(tu.cache_creation_input_tokens, None);
                    assert_eq!(tu.cache_read_input_tokens, None);
                }
                other => panic!("expected TokenUsage, got {:?}", other),
            }
        }
    }

    // describe("subagent identity enrichment (Story 037)")
    mod subagent_identity {
        use super::*;

        #[test]
        fn it_should_default_agent_id_to_none_and_is_sidechain_to_false() {
            let event = make_cloud_event(
                "message.user.prompt",
                json!({
                    "seq": 1,
                    "session_id": "sess-abc",
                    "text": "hi",
                    "raw": {"type": "user", "message": {"content": [{"type": "text", "text": "hi"}]}}
                }),
            );
            let records = from_cloud_event(&event);
            assert_eq!(records.len(), 1);
            assert_eq!(records[0].agent_id, None);
            assert_eq!(records[0].is_sidechain, false);
        }

        #[test]
        fn it_should_set_is_sidechain_false_when_present() {
            let event = make_cloud_event(
                "message.user.prompt",
                json!({
                    "seq": 1,
                    "session_id": "sess-abc",
                    "text": "hi",
                    "is_sidechain": false,
                    "raw": {"type": "user", "message": {"content": [{"type": "text", "text": "hi"}]}}
                }),
            );
            let records = from_cloud_event(&event);
            assert_eq!(records[0].is_sidechain, false);
            assert_eq!(records[0].agent_id, None);
        }

        #[test]
        fn it_should_set_agent_id_and_is_sidechain_for_subagent_event() {
            let event = make_cloud_event(
                "message.assistant.text",
                json!({
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
                }),
            );
            let records = from_cloud_event(&event);
            assert_eq!(records.len(), 1);
            assert_eq!(records[0].agent_id, Some("agent-abc-123".to_string()));
            assert_eq!(records[0].is_sidechain, true);
        }

        #[test]
        fn it_should_set_agent_id_from_progress_event_data() {
            let event = make_cloud_event(
                "progress.agent",
                json!({
                    "seq": 3,
                    "session_id": "sess-abc",
                    "is_sidechain": false,
                    "agent_id": "agent-abc-123",
                    "parent_tool_use_id": "toolu_xyz_789",
                    "progress_type": "agent_progress",
                    "raw": {"type": "progress", "data": {"type": "agent_progress"}}
                }),
            );
            let records = from_cloud_event(&event);
            assert_eq!(records.len(), 1);
            assert_eq!(records[0].agent_id, Some("agent-abc-123".to_string()));
            assert_eq!(records[0].is_sidechain, false);
        }

        #[test]
        fn it_should_stamp_agent_identity_on_all_records_from_tool_use() {
            let event = make_cloud_event(
                "message.assistant.tool_use",
                json!({
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
                }),
            );
            let records = from_cloud_event(&event);
            for r in &records {
                assert_eq!(
                    r.agent_id,
                    Some("agent-sub-1".to_string()),
                    "all records should have agent_id"
                );
                assert_eq!(r.is_sidechain, true, "all records should be sidechain");
            }
        }

        #[test]
        fn it_should_skip_serializing_agent_id_when_none() {
            let event = make_cloud_event(
                "message.user.prompt",
                json!({
                    "seq": 1,
                    "session_id": "sess-abc",
                    "text": "hi",
                    "is_sidechain": false,
                    "raw": {"type": "user", "message": {"content": [{"type": "text", "text": "hi"}]}}
                }),
            );
            let records = from_cloud_event(&event);
            let json = serde_json::to_value(&records[0]).unwrap();
            assert!(
                json.get("agent_id").is_none(),
                "agent_id should not appear in JSON when None"
            );
            assert_eq!(json["is_sidechain"], false);
        }

        #[test]
        fn it_should_serialize_agent_id_when_present() {
            let event = make_cloud_event(
                "message.assistant.text",
                json!({
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
                }),
            );
            let records = from_cloud_event(&event);
            let json = serde_json::to_value(&records[0]).unwrap();
            assert_eq!(json["agent_id"], "agent-xyz");
            assert_eq!(json["is_sidechain"], true);
        }
    }

    // ── Pi-mono decomposed event rendering ──────────────────────────
    //
    // These test that decomposed pi-mono CloudEvents render correctly
    // as ViewRecords. Each decomposed event has typed payload fields
    // (tool, args, text, tool_call_id) AND the full bundled raw line.

    fn make_pi_mono_event(subtype: &str, data: serde_json::Value) -> CloudEvent {
        let mut obj = data.as_object().cloned().unwrap_or_default();
        let seq = obj.remove("seq").unwrap_or(json!(1));
        let session_id = obj.remove("session_id").unwrap_or(json!("sess-pi"));
        let raw = obj.remove("raw").unwrap_or(json!({}));

        let mut payload = serde_json::Map::new();
        payload.insert("_variant".to_string(), json!("pi-mono"));
        payload.insert("meta".to_string(), json!({"agent": "pi-mono"}));
        for (k, v) in obj {
            payload.insert(k, v);
        }

        let ce: CloudEvent = serde_json::from_value(json!({
            "specversion": "1.0",
            "id": "evt-pi-001",
            "source": "pi://session/sess-pi",
            "type": "io.arc.event",
            "time": "2026-04-13T18:00:00Z",
            "datacontenttype": "application/json",
            "subtype": subtype,
            "agent": "pi-mono",
            "data": {
                "raw": raw,
                "seq": seq,
                "session_id": session_id,
                "agent_payload": payload,
            },
        }))
        .expect("pi-mono test fixture should deserialize");
        ce
    }

    mod pimono_decomposed_thinking {
        use super::*;

        #[test]
        fn it_should_produce_reasoning_with_content() {
            // Decomposed thinking event from [thinking, toolCall, toolCall] line
            let event = make_pi_mono_event(
                "message.assistant.thinking",
                json!({
                    "seq": 1,
                    "session_id": "sess-pi",
                    "text": "Let me read both files.",
                    "raw": {
                        "type": "message", "id": "890b923d",
                        "message": {
                            "role": "assistant",
                            "content": [
                                {"type": "thinking", "thinking": "Let me read both files."},
                                {"type": "toolCall", "id": "tc-1", "name": "read", "arguments": {"path": "/a.txt"}},
                                {"type": "toolCall", "id": "tc-2", "name": "read", "arguments": {"path": "/b.txt"}}
                            ],
                            "stopReason": "toolUse"
                        }
                    }
                }),
            );
            let records = from_cloud_event(&event);
            assert!(!records.is_empty(), "should produce at least one record");
            let reasoning = records
                .iter()
                .find(|r| matches!(&r.body, RecordBody::Reasoning(_)));
            assert!(reasoning.is_some(), "should have a Reasoning record");
            match &reasoning.unwrap().body {
                RecordBody::Reasoning(r) => {
                    assert_eq!(
                        r.content.as_deref(),
                        Some("Let me read both files."),
                        "reasoning content should be the thinking text"
                    );
                }
                _ => panic!("expected Reasoning"),
            }
        }
    }

    mod pimono_decomposed_text {
        use super::*;

        #[test]
        fn it_should_produce_assistant_message_with_text() {
            // Decomposed text event from [thinking, text, toolCall] line
            let event = make_pi_mono_event(
                "message.assistant.text",
                json!({
                    "seq": 2,
                    "session_id": "sess-pi",
                    "text": "Let me read the file first.",
                    "model": "claude-opus-4-6",
                    "raw": {
                        "type": "message", "id": "5e6b8ad0",
                        "message": {
                            "role": "assistant",
                            "content": [
                                {"type": "thinking", "thinking": "reasoning here"},
                                {"type": "text", "text": "Let me read the file first."},
                                {"type": "toolCall", "id": "tc-1", "name": "read", "arguments": {"path": "/x"}}
                            ],
                            "model": "claude-opus-4-6",
                            "stopReason": "toolUse"
                        }
                    }
                }),
            );
            let records = from_cloud_event(&event);
            let asst = records
                .iter()
                .find(|r| matches!(&r.body, RecordBody::AssistantMessage(_)));
            assert!(asst.is_some(), "should have an AssistantMessage record");
            match &asst.unwrap().body {
                RecordBody::AssistantMessage(a) => {
                    assert!(!a.content.is_empty(), "content should not be empty");
                    // The text should be the decomposed text, not empty
                    let has_text = a
                        .content
                        .iter()
                        .any(|b| matches!(b, ContentBlock::Text { text } if !text.is_empty()));
                    assert!(
                        has_text,
                        "AssistantMessage should have non-empty text content, got: {:?}",
                        a.content
                    );
                }
                _ => panic!("expected AssistantMessage"),
            }
        }
    }

    mod pimono_decomposed_tool_use {
        use super::*;

        #[test]
        fn it_should_produce_tool_call_with_name_and_args() {
            // Decomposed tool_use event from [thinking, text, toolCall] line
            let event = make_pi_mono_event(
                "message.assistant.tool_use",
                json!({
                    "seq": 3,
                    "session_id": "sess-pi",
                    "tool": "read",
                    "tool_call_id": "toolu_01BKoC",
                    "args": {"path": "/tmp/config.toml"},
                    "model": "claude-opus-4-6",
                    "raw": {
                        "type": "message", "id": "890b923d",
                        "message": {
                            "role": "assistant",
                            "content": [
                                {"type": "thinking", "thinking": "hmm"},
                                {"type": "text", "text": "reading"},
                                {"type": "toolCall", "id": "toolu_01BKoC", "name": "read",
                                 "arguments": {"path": "/tmp/config.toml"}}
                            ],
                            "model": "claude-opus-4-6",
                            "stopReason": "toolUse"
                        }
                    }
                }),
            );
            let records = from_cloud_event(&event);
            let tool = records
                .iter()
                .find(|r| matches!(&r.body, RecordBody::ToolCall(_)));
            assert!(tool.is_some(), "should have a ToolCall record");
            match &tool.unwrap().body {
                RecordBody::ToolCall(tc) => {
                    assert_eq!(
                        tc.name, "read",
                        "tool name should be 'read', got '{}'",
                        tc.name
                    );
                    assert!(!tc.input.is_null(), "tool input should not be null");
                    assert_eq!(tc.input["path"], "/tmp/config.toml");
                    assert_eq!(
                        tc.call_id, "toolu_01BKoC",
                        "call_id should come from payload, not raw parsing"
                    );
                }
                _ => panic!("expected ToolCall"),
            }
        }

        #[test]
        fn it_should_use_payload_call_id_not_raw_parsing() {
            // The call_id should come from agent_payload.tool_call_id,
            // not from scanning raw.message.content for toolUseId/id
            let event = make_pi_mono_event(
                "message.assistant.tool_use",
                json!({
                    "seq": 4,
                    "session_id": "sess-pi",
                    "tool": "read",
                    "tool_call_id": "the-correct-id",
                    "args": {"path": "/foo"},
                    "raw": {
                        "type": "message",
                        "message": {
                            "role": "assistant",
                            "content": [
                                {"type": "toolCall", "id": "raw-id-wrong", "name": "read",
                                 "arguments": {"path": "/foo"}}
                            ],
                            "stopReason": "toolUse"
                        }
                    }
                }),
            );
            let records = from_cloud_event(&event);
            let tool = records
                .iter()
                .find(|r| matches!(&r.body, RecordBody::ToolCall(_)));
            match &tool.unwrap().body {
                RecordBody::ToolCall(tc) => {
                    assert_eq!(
                        tc.call_id, "the-correct-id",
                        "should use payload.tool_call_id, not raw content id"
                    );
                }
                _ => panic!("expected ToolCall"),
            }
        }
    }

    /// Grok Build ACP: text lives on typed GrokPayload, not Claude raw blocks.
    fn make_grok_event(subtype: &str, data: serde_json::Value) -> CloudEvent {
        let mut obj = data.as_object().cloned().unwrap_or_default();
        let seq = obj.remove("seq").unwrap_or(json!(1));
        let session_id = obj.remove("session_id").unwrap_or(json!("sess-grok"));
        let raw = obj.remove("raw").unwrap_or(json!({}));

        let mut payload = serde_json::Map::new();
        payload.insert("_variant".to_string(), json!("grok-build"));
        payload.insert("meta".to_string(), json!({"agent": "grok-build"}));
        for (k, v) in obj {
            payload.insert(k, v);
        }

        serde_json::from_value(json!({
            "specversion": "1.0",
            "id": "evt-grok-001",
            "source": "grok://session/sess-grok",
            "type": "io.arc.event",
            "time": "2026-07-17T00:16:55Z",
            "datacontenttype": "application/json",
            "subtype": subtype,
            "agent": "grok-build",
            "data": {
                "raw": raw,
                "seq": seq,
                "session_id": session_id,
                "agent_payload": payload,
            },
        }))
        .expect("grok-build test fixture should deserialize")
    }

    // describe("when event is grok-build message.assistant.text")
    // Behavior: typed GrokPayload.text must become AssistantMessage content
    // even when raw is ACP-shaped (no Claude message.content blocks).
    mod grok_assistant_text {
        use super::*;

        #[test]
        fn it_should_surface_typed_payload_text_when_raw_has_no_claude_blocks() {
            let event = make_grok_event(
                "message.assistant.text",
                json!({
                    "seq": 3,
                    "session_id": "sess-grok",
                    "text": "Yes — OpenStory MCP is already connected.",
                    "model": "grok-4.5",
                    "raw": {
                        "method": "session/update",
                        "params": {
                            "update": {
                                "sessionUpdate": "agent_message_chunk",
                                "content": {
                                    "type": "text",
                                    "text": "Yes — OpenStory MCP is already connected."
                                }
                            }
                        }
                    }
                }),
            );
            let records = from_cloud_event(&event);
            let asst = records
                .iter()
                .find(|r| matches!(&r.body, RecordBody::AssistantMessage(_)))
                .expect("expected AssistantMessage");
            match &asst.body {
                RecordBody::AssistantMessage(a) => {
                    let text = a
                        .content
                        .iter()
                        .find_map(|b| match b {
                            ContentBlock::Text { text } => Some(text.as_str()),
                            _ => None,
                        })
                        .unwrap_or("");
                    assert_eq!(
                        text, "Yes — OpenStory MCP is already connected.",
                        "grok-build assistant text must come from typed payload, got content={:?}",
                        a.content
                    );
                    assert_eq!(a.model, "grok-4.5");
                }
                other => panic!("expected AssistantMessage, got {:?}", other),
            }
        }
    }

    // describe("when event is grok-build system.turn.complete with ACP usage")
    // Grok ACP uses camelCase: inputTokens/outputTokens/cachedReadTokens on
    // the typed payload of turn_completed → system.turn.complete.
    mod grok_turn_token_usage {
        use super::*;

        #[test]
        fn it_should_emit_token_usage_from_turn_complete_camel_case_fields() {
            let event = make_grok_event(
                "system.turn.complete",
                json!({
                    "seq": 10,
                    "session_id": "sess-grok",
                    "stop_reason": "end_turn",
                    "token_usage": {
                        "inputTokens": 36458,
                        "outputTokens": 313,
                        "totalTokens": 36771,
                        "cachedReadTokens": 35456,
                        "reasoningTokens": 57
                    },
                    "raw": {
                        "method": "_x.ai/session/update",
                        "params": {
                            "update": {
                                "sessionUpdate": "turn_completed",
                                "usage": {
                                    "inputTokens": 36458,
                                    "outputTokens": 313,
                                    "totalTokens": 36771,
                                    "cachedReadTokens": 35456
                                }
                            }
                        }
                    }
                }),
            );
            let records = from_cloud_event(&event);
            assert!(
                records.iter().any(|r| matches!(&r.body, RecordBody::TurnEnd(_))),
                "expected TurnEnd, got {:?}",
                records.iter().map(|r| format!("{:?}", r.body)).collect::<Vec<_>>()
            );
            let usage = records
                .iter()
                .find(|r| matches!(&r.body, RecordBody::TokenUsage(_)))
                .expect("expected TokenUsage from Grok turn_completed usage");
            match &usage.body {
                RecordBody::TokenUsage(tu) => {
                    assert_eq!(tu.input_tokens, Some(36458));
                    assert_eq!(tu.output_tokens, Some(313));
                    assert_eq!(tu.total_tokens, Some(36771));
                    assert_eq!(tu.cache_read_input_tokens, Some(35456));
                    assert_eq!(tu.scope, TokenScope::Turn);
                }
                other => panic!("expected TokenUsage, got {:?}", other),
            }
        }
    }

    // describe("when event is grok-build tool_use then tool_result")
    mod grok_tool_call_join {
        use super::*;

        #[test]
        fn it_should_preserve_matching_call_id_on_use_and_result() {
            let use_ev = make_grok_event(
                "message.assistant.tool_use",
                json!({
                    "seq": 4,
                    "session_id": "sess-grok",
                    "tool": "read_file",
                    "tool_call_id": "call-join-42",
                    "args": {"target_file": "/workspace/demo/src/main.rs"},
                    "raw": {"method": "session/update", "params": {"update": {"sessionUpdate": "tool_call"}}}
                }),
            );
            let result_ev = make_grok_event(
                "message.user.tool_result",
                json!({
                    "seq": 5,
                    "session_id": "sess-grok",
                    "tool": "read_file",
                    "tool_call_id": "call-join-42",
                    "text": "fn main() {}",
                    "raw": {"method": "session/update", "params": {"update": {"sessionUpdate": "tool_call_update"}}}
                }),
            );
            let use_recs = from_cloud_event(&use_ev);
            let res_recs = from_cloud_event(&result_ev);
            let call_id = match &use_recs[0].body {
                RecordBody::ToolCall(tc) => {
                    assert_eq!(tc.name, "read_file");
                    assert_eq!(tc.call_id, "call-join-42");
                    tc.call_id.clone()
                }
                other => panic!("expected ToolCall, got {:?}", other),
            };
            match &res_recs[0].body {
                RecordBody::ToolResult(tr) => {
                    assert_eq!(tr.call_id, call_id, "tool_result must join on call_id");
                    assert_eq!(tr.output.as_deref(), Some("fn main() {}"));
                }
                other => panic!("expected ToolResult, got {:?}", other),
            }
        }
    }

    // describe("when event is grok-build message.assistant.thinking")
    mod grok_assistant_thinking {
        use super::*;

        #[test]
        fn it_should_surface_typed_payload_thinking_as_reasoning() {
            let event = make_grok_event(
                "message.assistant.thinking",
                json!({
                    "seq": 2,
                    "session_id": "sess-grok",
                    "text": "The user wants OpenStory MCP — tools are connected.",
                    "raw": {
                        "method": "session/update",
                        "params": {
                            "update": {
                                "sessionUpdate": "agent_thought_chunk",
                                "content": {
                                    "type": "text",
                                    "text": "The user wants OpenStory MCP — tools are connected."
                                }
                            }
                        }
                    }
                }),
            );
            let records = from_cloud_event(&event);
            assert_eq!(records.len(), 1, "expected one Reasoning record");
            match &records[0].body {
                RecordBody::Reasoning(r) => {
                    assert_eq!(
                        r.content.as_deref(),
                        Some("The user wants OpenStory MCP — tools are connected."),
                        "grok-build thinking must come from typed payload"
                    );
                }
                other => panic!("expected Reasoning, got {:?}", other),
            }
        }
    }
}
