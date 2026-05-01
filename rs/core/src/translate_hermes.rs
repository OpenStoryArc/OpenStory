//! Pure translation: Hermes Agent JSONL → CloudEvent(s).
//!
//! Hermes Agent (NousResearch/hermes-agent) is a self-improving agent that
//! stores conversations in OpenAI shape regardless of which provider produced
//! the response. This module translates Hermes-native event lines (as written
//! by the `hermes-openstory` plugin) into the same CloudEvent 1.0 format used
//! by `translate.rs` (Claude Code) and `translate_pi.rs` (pi-mono).
//!
//! Wire format
//! -----------
//! Each line written by the Hermes plugin is a JSON object with this shape:
//!
//! ```json
//! {
//!   "envelope": {
//!     "session_id": "...",
//!     "event_seq": 1,
//!     "timestamp": "2026-04-08T14:00:00Z",
//!     "source": "hermes",
//!     "model": "...",      // optional, only on session_start
//!     "platform": "...",   // optional, only on session_start
//!     "hermes_version": "..."
//!   },
//!   "event_type": "session_start" | "message" | "session_end",
//!   "data": { ... event-type-specific shape ... }
//! }
//! ```
//!
//! Verified message shapes (see `docs/research/hermes-integration/SOURCE_VERIFICATION.md`):
//!
//! - **User message**: `{"role": "user", "content": "string"}`
//! - **Assistant text**: `{"role": "assistant", "content": "...", "reasoning": "...", "finish_reason": "..."}`
//! - **Assistant tool call**: `{"role": "assistant", "content": "..." | "",
//!     "tool_calls": [{"id": "tc_1", "function": {"name": "...", "arguments": "<json string>"}}],
//!     "reasoning": "...", "finish_reason": "tool_calls"}`
//! - **Tool result**: `{"role": "tool", "tool_call_id": "tc_1",
//!     "content": "string", "tool_name": "..." (optional)}`
//! - **System (incl. injected)**: `{"role": "system", "content": "..."}`
//!
//! Hermes's internal storage is **always** OpenAI shape — the Anthropic adapter
//! at `tests/agent/test_anthropic_adapter.py:575` is a one-way translator at
//! the API boundary, not bidirectional. The Anthropic-content-block branch
//! exists in the Python prototype as defensive dead code; this Rust port
//! omits it for clarity.

use serde_json::Value;
use uuid::Uuid;

use crate::cloud_event::CloudEvent;
use crate::event_data::{AgentPayload, EventData, HermesPayload};
use crate::translate::{TranscriptState, IO_ARC_EVENT};

/// UUID5 namespace for Hermes synthetic event IDs.
/// Matches the URL namespace used by the Python prototype.
const NAMESPACE_URL_BYTES: [u8; 16] = [
    0x6b, 0xa7, 0xb8, 0x11, 0x9d, 0xad, 0x11, 0xd1, 0x80, 0xb4, 0x00, 0xc0, 0x4f, 0xd4, 0x30, 0xc8,
];

/// Common envelope context threaded through every builder.
///
/// Bundles the per-line fields (source, session_id, timestamp, sequence,
/// raw line) so the builder functions take a single context handle plus
/// their own payload-specific work, instead of seven repeated parameters.
struct LineCtx<'a> {
    source: &'a str,
    session_id: &'a str,
    timestamp: &'a Option<String>,
    env_seq: u64,
    raw: &'a Value,
}

/// Detect whether a JSONL line is a Hermes-shape event.
///
/// Hermes signals: an `envelope` object containing `source: "hermes"` and a
/// known `event_type`.
pub fn is_hermes_format(line: &Value) -> bool {
    let envelope = match line.get("envelope") {
        Some(Value::Object(obj)) => obj,
        _ => return false,
    };
    let source_ok = envelope
        .get("source")
        .and_then(|v| v.as_str())
        .map(|s| s == "hermes")
        .unwrap_or(false);
    if !source_ok {
        return false;
    }
    let event_type = line.get("event_type").and_then(|v| v.as_str()).unwrap_or("");
    matches!(event_type, "session_start" | "message" | "session_end")
}

/// Pure function: translate one Hermes JSONL line into CloudEvent(s).
///
/// Returns zero events for unknown event types or unrecognized roles, one
/// event for most messages, and multiple events for assistant turns that
/// fan out (thinking + N tool calls produce one CloudEvent per sub-event so
/// the pattern detector can recognize them as separate eval/apply phases).
pub fn translate_hermes_line(line: &Value, state: &mut TranscriptState) -> Vec<CloudEvent> {
    let envelope = match line.get("envelope") {
        Some(v) => v,
        None => return vec![],
    };
    let event_type = match line.get("event_type").and_then(|v| v.as_str()) {
        Some(t) => t,
        None => return vec![],
    };
    let data = line.get("data").cloned().unwrap_or(Value::Null);

    let session_id = envelope
        .get("session_id")
        .and_then(|v| v.as_str())
        .unwrap_or(&state.session_id)
        .to_string();
    let timestamp = envelope
        .get("timestamp")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let env_seq = envelope.get("event_seq").and_then(|v| v.as_u64()).unwrap_or(0);

    // Source URI mirrors `hermes://session/<id>` to parallel pi-mono.
    let source = format!("hermes://session/{}", session_id);

    let ctx = LineCtx {
        source: &source,
        session_id: &session_id,
        timestamp: &timestamp,
        env_seq,
        raw: line,
    };

    match event_type {
        "session_start" => vec![build_session_start(&ctx, envelope, &data, state)],
        "session_end" => vec![build_turn_complete(&ctx, &data, state)],
        "message" => translate_message(&ctx, &data, state),
        _ => vec![],
    }
}

// ── Message dispatch ──────────────────────────────────────────────

fn translate_message(
    ctx: &LineCtx<'_>,
    msg: &Value,
    state: &mut TranscriptState,
) -> Vec<CloudEvent> {
    let role = msg.get("role").and_then(|v| v.as_str()).unwrap_or("");
    match role {
        "user" => vec![build_user_prompt(ctx, msg, state)],
        "tool" => vec![build_tool_result(ctx, msg, state)],
        "system" => vec![build_system_injected(ctx, msg, state)],
        "assistant" => translate_assistant_message(ctx, msg, state),
        _ => vec![], // Forward-compat: unknown roles dropped silently.
    }
}

/// Assistant turns can fan out into thinking + (text or N tool calls).
fn translate_assistant_message(
    ctx: &LineCtx<'_>,
    msg: &Value,
    state: &mut TranscriptState,
) -> Vec<CloudEvent> {
    let mut out: Vec<CloudEvent> = Vec::new();

    // ── Thinking phase (top-level reasoning string) ──
    if let Some(reasoning) = msg.get("reasoning").and_then(|v| v.as_str()) {
        if !reasoning.is_empty() {
            let mut payload = HermesPayload::new();
            payload.reasoning = Some(reasoning.to_string());
            out.push(build_event(ctx, "message.assistant.thinking", payload, state));
        }
    }

    let text = extract_text(msg);
    let tool_calls = msg.get("tool_calls").and_then(|v| v.as_array());
    let has_tool_calls = tool_calls.map(|arr| !arr.is_empty()).unwrap_or(false);

    // ── Text-only assistant message ──
    if !text.is_empty() && !has_tool_calls {
        let mut payload = HermesPayload::new();
        payload.text = Some(text.clone());
        if let Some(stop) = msg
            .get("stop_reason")
            .and_then(|v| v.as_str())
            .or_else(|| msg.get("finish_reason").and_then(|v| v.as_str()))
        {
            payload.stop_reason = Some(stop.to_string());
        }
        out.push(build_event(ctx, "message.assistant.text", payload, state));
    }

    // ── Tool calls — fan out one event per call ──
    // Each tool call gets a unique index to avoid ID collisions (B2).
    if let Some(tcs) = tool_calls {
        let preceding = if text.is_empty() { None } else { Some(text.clone()) };
        for (i, tc) in tcs.iter().enumerate() {
            out.push(build_tool_use(ctx, tc, &preceding, i, state));
        }
    }

    out
}

// ── Per-CloudEvent builders ──────────────────────────────────────

fn build_session_start(
    ctx: &LineCtx<'_>,
    envelope: &Value,
    data: &Value,
    state: &mut TranscriptState,
) -> CloudEvent {
    let mut payload = HermesPayload::new();
    payload.model = envelope
        .get("model")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    payload.platform = envelope
        .get("platform")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    payload.hermes_version = envelope
        .get("hermes_version")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    payload.system_prompt_preview = data
        .get("system_prompt_preview")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    payload.tools = data.get("tools").and_then(|v| v.as_array()).map(|arr| {
        arr.iter()
            .filter_map(|t| t.as_str().map(|s| s.to_string()))
            .collect()
    });
    build_event(ctx, "system.session.start", payload, state)
}

fn build_turn_complete(
    ctx: &LineCtx<'_>,
    data: &Value,
    state: &mut TranscriptState,
) -> CloudEvent {
    let mut payload = HermesPayload::new();
    payload.reason = data
        .get("reason")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    payload.completed = data.get("completed").and_then(|v| v.as_bool());
    payload.interrupted = data.get("interrupted").and_then(|v| v.as_bool());
    payload.message_count = data.get("message_count").and_then(|v| v.as_u64());
    build_event(ctx, "system.turn.complete", payload, state)
}

fn build_user_prompt(
    ctx: &LineCtx<'_>,
    msg: &Value,
    state: &mut TranscriptState,
) -> CloudEvent {
    let mut payload = HermesPayload::new();
    payload.text = Some(extract_text(msg));
    build_event(ctx, "message.user.prompt", payload, state)
}

fn build_tool_result(
    ctx: &LineCtx<'_>,
    msg: &Value,
    state: &mut TranscriptState,
) -> CloudEvent {
    // Verified canonical keys: `tool_call_id` (required), `tool_name` (optional).
    // No `id`/`name` aliases — see SOURCE_VERIFICATION.md §4.1.
    let mut payload = HermesPayload::new();
    payload.tool_call_id = msg
        .get("tool_call_id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    payload.tool_name = msg
        .get("tool_name")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    // Mirror `tool_name` into the `tool` accessor field so downstream code
    // that calls `payload.tool()` sees a value when one is present.
    payload.tool = payload.tool_name.clone();
    payload.text = Some(extract_text(msg));
    build_event(ctx, "message.user.tool_result", payload, state)
}

fn build_system_injected(
    ctx: &LineCtx<'_>,
    msg: &Value,
    state: &mut TranscriptState,
) -> CloudEvent {
    // Hermes does not specially tag injected system messages (compression
    // summaries, todo snapshots) — they appear as plain
    // `{"role": "system", "content": "..."}`. All map to system.injected.other.
    let mut payload = HermesPayload::new();
    payload.text = Some(extract_text(msg));
    build_event(ctx, "system.injected.other", payload, state)
}

fn build_tool_use(
    ctx: &LineCtx<'_>,
    tc: &Value,
    preceding_text: &Option<String>,
    tool_index: usize,
    state: &mut TranscriptState,
) -> CloudEvent {
    // Canonical OpenAI shape:
    //   {"id": "...", "function": {"name": "...", "arguments": "<json string>"}}
    let function = tc.get("function");
    let name = function
        .and_then(|f| f.get("name"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let args = parse_arguments_field(function.and_then(|f| f.get("arguments")));
    let tool_use_id = tc
        .get("id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let mut payload = HermesPayload::new();
    payload.tool = Some(name);
    payload.tool_use_id = Some(tool_use_id);
    payload.args = Some(args);
    payload.preceding_text = preceding_text.clone();
    // Preserve Hermes's actual finish_reason ("tool_calls"), don't override
    // with "tool_use" which is an Anthropic convention. (Review concern C2.)
    payload.stop_reason = Some("tool_calls".to_string());

    // Use indexed ID derivation to avoid collisions when a single message
    // fans out to multiple tool_use CloudEvents. (Review blocker B2.)
    let event_id = derive_event_id_indexed(ctx.session_id, ctx.env_seq, "message.assistant.tool_use", tool_index);
    payload.seq = Some(ctx.env_seq);
    payload.timestamp = ctx.timestamp.clone();

    let data = EventData::with_payload(
        ctx.raw.clone(),
        state.next_seq(),
        ctx.session_id.to_string(),
        AgentPayload::Hermes(payload),
    );

    CloudEvent::new(
        ctx.source.to_string(),
        IO_ARC_EVENT.to_string(),
        data,
        Some("message.assistant.tool_use".to_string()),
        Some(event_id),
        ctx.timestamp.clone(),
        None,
        None,
        Some("hermes".to_string()),
    )
    .with_host(crate::host::host())
}

// ── Helpers ──────────────────────────────────────────────────────

/// Hermes's `tool_calls[i].function.arguments` is canonically a JSON STRING
/// (verified from `tests/agent/test_anthropic_adapter.py:586`). Some agent
/// fixtures or future Hermes versions may pass it as a dict directly. We
/// handle both: parse the string, fall through unchanged if it's already an
/// object, and stash the raw value under `_raw` if it's a malformed JSON
/// string so the data isn't lost.
fn parse_arguments_field(args: Option<&Value>) -> Value {
    match args {
        None => Value::Object(Default::default()),
        Some(Value::String(s)) => match serde_json::from_str::<Value>(s) {
            Ok(v) => v,
            Err(_) => {
                let mut m = serde_json::Map::new();
                m.insert("_raw".to_string(), Value::String(s.clone()));
                Value::Object(m)
            }
        },
        Some(v) => v.clone(),
    }
}

/// Pull the textual content out of a Hermes message dict.
///
/// Hermes uses plain string `content` for user, assistant text, tool result,
/// and system messages. The Anthropic-style content-block list never appears
/// in persisted state. We accept both shapes anyway for forward-compat.
fn extract_text(msg: &Value) -> String {
    match msg.get("content") {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(blocks)) => {
            let mut parts: Vec<String> = Vec::new();
            for block in blocks {
                if let Value::Object(obj) = block {
                    if obj.get("type").and_then(|v| v.as_str()) == Some("text") {
                        if let Some(t) = obj.get("text").and_then(|v| v.as_str()) {
                            parts.push(t.to_string());
                        }
                    }
                } else if let Value::String(s) = block {
                    parts.push(s.clone());
                }
            }
            parts.join("\n")
        }
        _ => String::new(),
    }
}

/// Build a complete CloudEvent given a typed Hermes payload.
///
/// The event ID is deterministic: `uuid5(URL, "hermes:{session}:{seq}:{subtype}")`
/// — stable across re-translation passes, mirrors the Python prototype, and
/// addresses the synthetic-event-ID-stability backlog goal for Hermes data.
fn build_event(
    ctx: &LineCtx<'_>,
    subtype: &str,
    mut payload: HermesPayload,
    state: &mut TranscriptState,
) -> CloudEvent {
    payload.seq = Some(ctx.env_seq);
    payload.timestamp = ctx.timestamp.clone();

    let event_id = derive_event_id(ctx.session_id, ctx.env_seq, subtype);

    let data = EventData::with_payload(
        ctx.raw.clone(),
        state.next_seq(),
        ctx.session_id.to_string(),
        AgentPayload::Hermes(payload),
    );

    CloudEvent::new(
        ctx.source.to_string(),
        IO_ARC_EVENT.to_string(),
        data,
        Some(subtype.to_string()),
        Some(event_id),
        ctx.timestamp.clone(),
        None,
        None,
        Some("hermes".to_string()),
    )
    .with_host(crate::host::host())
}

/// Derive a deterministic event ID from (session, seq, subtype, index).
///
/// The `index` parameter disambiguates fan-out events: when a single
/// Hermes assistant message has N tool_calls, each produces a separate
/// CloudEvent with the same (session_id, env_seq, subtype). Without
/// an index, they'd collide. (Review blocker B2.)
fn derive_event_id(session_id: &str, env_seq: u64, subtype: &str) -> String {
    derive_event_id_indexed(session_id, env_seq, subtype, 0)
}

fn derive_event_id_indexed(session_id: &str, env_seq: u64, subtype: &str, index: usize) -> String {
    let namespace = Uuid::from_bytes(NAMESPACE_URL_BYTES);
    let name = if index == 0 {
        format!("hermes:{}:{}:{}", session_id, env_seq, subtype)
    } else {
        format!("hermes:{}:{}:{}:{}", session_id, env_seq, subtype, index)
    };
    Uuid::new_v5(&namespace, name.as_bytes()).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn state() -> TranscriptState {
        TranscriptState::new("hermes-test-session-001".to_string())
    }

    fn payload(event: &CloudEvent) -> HermesPayload {
        match event.data.agent_payload.as_ref().expect("agent_payload should be Some") {
            AgentPayload::Hermes(p) => p.clone(),
            _ => panic!("expected Hermes payload"),
        }
    }

    fn message_event(seq: u64, msg: Value) -> Value {
        json!({
            "envelope": {
                "session_id": "hermes-test-session-001",
                "event_seq": seq,
                "timestamp": "2026-04-08T14:00:00Z",
                "source": "hermes",
            },
            "event_type": "message",
            "data": msg,
        })
    }

    // ── Format detection ─────────────────────────────────────

    #[test]
    fn detects_hermes_format() {
        let line = json!({
            "envelope": {"session_id": "x", "event_seq": 1, "source": "hermes"},
            "event_type": "session_start",
            "data": {},
        });
        assert!(is_hermes_format(&line));
    }

    #[test]
    fn rejects_pi_mono_format() {
        let line = json!({"type": "session", "id": "x", "cwd": "/work"});
        assert!(!is_hermes_format(&line));
    }

    #[test]
    fn rejects_claude_code_format() {
        let line = json!({"type": "user", "uuid": "x"});
        assert!(!is_hermes_format(&line));
    }

    #[test]
    fn rejects_hermes_with_unknown_event_type() {
        let line = json!({
            "envelope": {"source": "hermes"},
            "event_type": "future_event",
        });
        assert!(!is_hermes_format(&line));
    }

    // ── Subtype boundaries: full session walk ────────────────

    #[test]
    fn translates_full_example_session() {
        // Mirrors example_hermes_events.jsonl from the prototype: 6 input
        // lines → 7 CloudEvents in this exact order.
        let mut s = state();
        let lines = vec![
            json!({
                "envelope": {
                    "session_id": "hermes-test-session-001", "event_seq": 1,
                    "timestamp": "2026-04-08T14:00:00Z", "source": "hermes",
                    "model": "anthropic/claude-opus-4-6", "platform": "cli",
                },
                "event_type": "session_start",
                "data": {
                    "system_prompt_preview": "You are Hermes Agent...",
                    "tools": ["Read", "Write", "Bash"],
                },
            }),
            json!({
                "envelope": {"session_id": "hermes-test-session-001", "event_seq": 2,
                             "timestamp": "2026-04-08T14:00:01Z", "source": "hermes"},
                "event_type": "message",
                "data": {"role": "user", "content": "Read the README"},
            }),
            json!({
                "envelope": {"session_id": "hermes-test-session-001", "event_seq": 3,
                             "timestamp": "2026-04-08T14:00:03Z", "source": "hermes"},
                "event_type": "message",
                "data": {
                    "role": "assistant",
                    "content": "I'll read the README.",
                    "reasoning": "User wants summary; Read is the right tool.",
                    "tool_calls": [{
                        "id": "tc_1",
                        "function": {"name": "Read", "arguments": "{\"file_path\": \"/repo/README.md\"}"}
                    }],
                    "finish_reason": "tool_calls",
                },
            }),
            json!({
                "envelope": {"session_id": "hermes-test-session-001", "event_seq": 4,
                             "timestamp": "2026-04-08T14:00:03Z", "source": "hermes"},
                "event_type": "message",
                "data": {"role": "tool", "tool_call_id": "tc_1",
                         "tool_name": "Read", "content": "# Open Story\n..."},
            }),
            json!({
                "envelope": {"session_id": "hermes-test-session-001", "event_seq": 5,
                             "timestamp": "2026-04-08T14:00:06Z", "source": "hermes"},
                "event_type": "message",
                "data": {"role": "assistant",
                         "content": "Open Story is an observability tool.",
                         "finish_reason": "stop"},
            }),
            json!({
                "envelope": {"session_id": "hermes-test-session-001", "event_seq": 6,
                             "timestamp": "2026-04-08T14:00:07Z", "source": "hermes"},
                "event_type": "session_end",
                "data": {"reason": "end_turn", "message_count": 5},
            }),
        ];

        let mut events: Vec<CloudEvent> = Vec::new();
        for line in &lines {
            events.extend(translate_hermes_line(line, &mut s));
        }

        assert_eq!(events.len(), 7, "expected 7 CloudEvents from 6 input lines");

        let subtypes: Vec<&str> = events
            .iter()
            .filter_map(|e| e.subtype.as_deref())
            .collect();
        assert_eq!(
            subtypes,
            vec![
                "system.session.start",
                "message.user.prompt",
                "message.assistant.thinking",
                "message.assistant.tool_use",
                "message.user.tool_result",
                "message.assistant.text",
                "system.turn.complete",
            ]
        );

        // Every event must carry the hermes agent tag.
        for ev in &events {
            assert_eq!(ev.agent.as_deref(), Some("hermes"));
        }
    }

    // ── Canonical OpenAI tool call shape ─────────────────────

    #[test]
    fn translates_canonical_openai_tool_call() {
        // Verified shape from tests/agent/test_anthropic_adapter.py:575
        let mut s = state();
        let line = message_event(
            1,
            json!({
                "role": "assistant",
                "content": "Let me search.",
                "tool_calls": [{
                    "id": "tc_1",
                    "function": {"name": "search", "arguments": "{\"query\": \"test\"}"}
                }],
            }),
        );
        let events = translate_hermes_line(&line, &mut s);
        let tool_use: Vec<&CloudEvent> = events
            .iter()
            .filter(|e| e.subtype.as_deref() == Some("message.assistant.tool_use"))
            .collect();
        assert_eq!(tool_use.len(), 1);
        let p = payload(tool_use[0]);
        assert_eq!(p.tool.as_deref(), Some("search"));
        assert_eq!(p.tool_use_id.as_deref(), Some("tc_1"));
        // arguments must be parsed from JSON string into a structured Value
        assert_eq!(p.args, Some(json!({"query": "test"})));
        assert_eq!(p.preceding_text.as_deref(), Some("Let me search."));
    }

    // ── Tool result shape ────────────────────────────────────

    #[test]
    fn translates_canonical_tool_result() {
        // Verified shape from tests/agent/test_anthropic_adapter.py:590
        let mut s = state();
        let line = message_event(
            1,
            json!({"role": "tool", "tool_call_id": "tc_1", "content": "search results"}),
        );
        let events = translate_hermes_line(&line, &mut s);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].subtype.as_deref(), Some("message.user.tool_result"));
        let p = payload(&events[0]);
        assert_eq!(p.tool_call_id.as_deref(), Some("tc_1"));
        assert_eq!(p.text.as_deref(), Some("search results"));
        // tool_name is optional and absent in the canonical fixture
        assert!(p.tool_name.is_none());
    }

    #[test]
    fn tool_result_with_optional_tool_name() {
        let mut s = state();
        let line = message_event(
            1,
            json!({"role": "tool", "tool_call_id": "tc_2",
                   "tool_name": "Read", "content": "file..."}),
        );
        let events = translate_hermes_line(&line, &mut s);
        let p = payload(&events[0]);
        assert_eq!(p.tool_name.as_deref(), Some("Read"));
        assert_eq!(p.tool.as_deref(), Some("Read"));
    }

    // ── Edge cases verified from hermes-agent test fixtures ──

    #[test]
    fn assistant_with_only_tool_calls_emits_no_text_event() {
        let mut s = state();
        let line = message_event(
            1,
            json!({
                "role": "assistant",
                "content": "",
                "tool_calls": [{
                    "id": "tc_a",
                    "function": {"name": "tool_a", "arguments": "{}"}
                }],
            }),
        );
        let events = translate_hermes_line(&line, &mut s);
        let texts = events
            .iter()
            .filter(|e| e.subtype.as_deref() == Some("message.assistant.text"))
            .count();
        let tool_uses = events
            .iter()
            .filter(|e| e.subtype.as_deref() == Some("message.assistant.tool_use"))
            .count();
        assert_eq!(texts, 0);
        assert_eq!(tool_uses, 1);
        assert!(payload(&events[0]).preceding_text.is_none());
    }

    #[test]
    fn multiple_tool_calls_fan_out() {
        // Verified pattern from tests/agent/test_anthropic_adapter.py:621
        let mut s = state();
        let line = message_event(
            1,
            json!({
                "role": "assistant",
                "content": "",
                "tool_calls": [
                    {"id": "tc_1", "function": {"name": "tool_a", "arguments": "{}"}},
                    {"id": "tc_2", "function": {"name": "tool_b", "arguments": "{}"}}
                ],
            }),
        );
        let events = translate_hermes_line(&line, &mut s);
        let tool_uses: Vec<&CloudEvent> = events
            .iter()
            .filter(|e| e.subtype.as_deref() == Some("message.assistant.tool_use"))
            .collect();
        assert_eq!(tool_uses.len(), 2);
        let names: std::collections::HashSet<&str> = tool_uses
            .iter()
            .filter_map(|e| match e.data.agent_payload.as_ref() {
                Some(AgentPayload::Hermes(p)) => p.tool.as_deref(),
                _ => None,
            })
            .collect();
        assert!(names.contains("tool_a"));
        assert!(names.contains("tool_b"));
    }

    #[test]
    fn orphaned_tool_result_does_not_crash() {
        // Verified pattern from tests/agent/test_anthropic_adapter.py:651
        let mut s = state();
        let line = message_event(
            1,
            json!({"role": "tool", "tool_call_id": "tc_orphan",
                   "content": "stale result"}),
        );
        let events = translate_hermes_line(&line, &mut s);
        assert_eq!(events.len(), 1);
        let p = payload(&events[0]);
        assert_eq!(p.tool_call_id.as_deref(), Some("tc_orphan"));
    }

    #[test]
    fn assistant_text_only_carries_finish_reason() {
        let mut s = state();
        let line = message_event(
            1,
            json!({"role": "assistant", "content": "Here's the answer.",
                   "finish_reason": "stop"}),
        );
        let events = translate_hermes_line(&line, &mut s);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].subtype.as_deref(), Some("message.assistant.text"));
        assert_eq!(payload(&events[0]).stop_reason.as_deref(), Some("stop"));
    }

    #[test]
    fn assistant_with_reasoning_emits_thinking_then_tool_use() {
        let mut s = state();
        let line = message_event(
            1,
            json!({
                "role": "assistant",
                "content": "I'll search.",
                "reasoning": "User wants information; search is right.",
                "tool_calls": [{
                    "id": "tc_1",
                    "function": {"name": "search", "arguments": "{\"q\": \"x\"}"}
                }],
            }),
        );
        let events = translate_hermes_line(&line, &mut s);
        let subtypes: Vec<&str> = events.iter().filter_map(|e| e.subtype.as_deref()).collect();
        assert_eq!(
            subtypes,
            vec!["message.assistant.thinking", "message.assistant.tool_use"]
        );
        let thinking = payload(&events[0]);
        assert!(thinking.reasoning.as_deref().unwrap().contains("information"));
        let tool = payload(&events[1]);
        assert_eq!(tool.preceding_text.as_deref(), Some("I'll search."));
    }

    #[test]
    fn system_message_maps_to_injected_other() {
        let mut s = state();
        let line = message_event(
            1,
            json!({"role": "system",
                   "content": "[Compressed history follows] ..."}),
        );
        let events = translate_hermes_line(&line, &mut s);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].subtype.as_deref(), Some("system.injected.other"));
    }

    #[test]
    fn unknown_role_dropped_silently() {
        let mut s = state();
        let line = message_event(1, json!({"role": "future_role", "content": "..."}));
        let events = translate_hermes_line(&line, &mut s);
        assert!(events.is_empty());
    }

    #[test]
    fn unknown_event_type_dropped_silently() {
        let mut s = state();
        let line = json!({
            "envelope": {"session_id": "x", "event_seq": 99,
                         "timestamp": "2026-04-08T00:00:00Z", "source": "hermes"},
            "event_type": "future_event_type",
            "data": {},
        });
        let events = translate_hermes_line(&line, &mut s);
        assert!(events.is_empty());
    }

    // ── Determinism & uniqueness ─────────────────────────────

    #[test]
    fn event_ids_are_deterministic() {
        let line = message_event(1, json!({"role": "user", "content": "hello"}));
        let mut s1 = state();
        let mut s2 = state();
        let e1 = translate_hermes_line(&line, &mut s1);
        let e2 = translate_hermes_line(&line, &mut s2);
        assert_eq!(e1[0].id, e2[0].id);
    }

    #[test]
    fn event_ids_are_unique_within_a_session_walk() {
        let mut s = state();
        let lines = vec![
            message_event(1, json!({"role": "user", "content": "a"})),
            message_event(2, json!({"role": "user", "content": "b"})),
            message_event(3, json!({"role": "assistant", "content": "c"})),
        ];
        let mut ids: Vec<String> = Vec::new();
        for l in &lines {
            for e in translate_hermes_line(l, &mut s) {
                ids.push(e.id.clone());
            }
        }
        let unique: std::collections::HashSet<&String> = ids.iter().collect();
        assert_eq!(unique.len(), ids.len());
    }

    // ── parse_arguments_field ────────────────────────────────

    #[test]
    fn parses_arguments_json_string() {
        let v = parse_arguments_field(Some(&json!("{\"a\": 1}")));
        assert_eq!(v, json!({"a": 1}));
    }

    #[test]
    fn passes_through_arguments_object() {
        let v = parse_arguments_field(Some(&json!({"a": 1})));
        assert_eq!(v, json!({"a": 1}));
    }

    #[test]
    fn captures_malformed_arguments_under_raw() {
        let v = parse_arguments_field(Some(&json!("not-json{")));
        assert_eq!(v, json!({"_raw": "not-json{"}));
    }
}
