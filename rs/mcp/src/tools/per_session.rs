//! Per-session detail tools that wrap a single `EventStore::query_*`
//! method. Each tool is a thin async wrapper that:
//!
//!   1. Extracts `session_id` from the args (errors if absent)
//!   2. Calls the corresponding trait method
//!   3. Serializes the result to a JSON Value
//!
//! Harder tools that need more than the EventStore trait
//! (session_plans, session_transcript, session_activity, session_sentences)
//! land in a subsequent commit alongside the Server-context extensions
//! they need.

use crate::plan_source::PlanSource;
use open_story_store::analysis::activity_summary;
use open_story_store::event_store::EventStore;
use serde_json::{json, Value};
use std::sync::Arc;

fn extract_session_id(args: &Value) -> Result<&str, String> {
    args.get("session_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "requires `session_id` (string)".to_string())
}

// ── tool_journey ──────────────────────────────────────────────────

pub fn tool_journey_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "session_id": {"type": "string", "description": "Session UUID"},
        },
        "required": ["session_id"],
        "additionalProperties": false
    })
}

pub async fn tool_journey(store: &Arc<dyn EventStore>, args: Value) -> Result<Value, String> {
    let session_id = extract_session_id(&args).map_err(|e| format!("tool_journey {e}"))?;
    let steps = store.query_tool_journey(session_id).await;
    serde_json::to_value(steps).map_err(|e| format!("serialize: {e}"))
}

// ── file_impact ───────────────────────────────────────────────────

pub fn file_impact_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "session_id": {"type": "string", "description": "Session UUID"},
        },
        "required": ["session_id"],
        "additionalProperties": false
    })
}

pub async fn file_impact(store: &Arc<dyn EventStore>, args: Value) -> Result<Value, String> {
    let session_id = extract_session_id(&args).map_err(|e| format!("file_impact {e}"))?;
    let impact = store.query_file_impact(session_id).await;
    serde_json::to_value(impact).map_err(|e| format!("serialize: {e}"))
}

// ── session_errors ───────────────────────────────────────────────

pub fn session_errors_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "session_id": {"type": "string", "description": "Session UUID"},
        },
        "required": ["session_id"],
        "additionalProperties": false
    })
}

pub async fn session_errors(store: &Arc<dyn EventStore>, args: Value) -> Result<Value, String> {
    let session_id = extract_session_id(&args).map_err(|e| format!("session_errors {e}"))?;
    let errors = store.query_session_errors(session_id).await;
    serde_json::to_value(errors).map_err(|e| format!("serialize: {e}"))
}

// ── session_patterns ─────────────────────────────────────────────

pub fn session_patterns_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "session_id": {"type": "string", "description": "Session UUID"},
            "pattern_type": {
                "type": "string",
                "description": "Optional filter (e.g., turn.sentence, eval_apply.eval, git.workflow)",
            },
        },
        "required": ["session_id"],
        "additionalProperties": false
    })
}

pub async fn session_patterns(store: &Arc<dyn EventStore>, args: Value) -> Result<Value, String> {
    let session_id = extract_session_id(&args).map_err(|e| format!("session_patterns {e}"))?;
    let pattern_type = args.get("pattern_type").and_then(|v| v.as_str());
    let patterns = store
        .session_patterns(session_id, pattern_type)
        .await
        .map_err(|e| format!("session_patterns failed: {e}"))?;
    serde_json::to_value(patterns).map_err(|e| format!("serialize: {e}"))
}

// ── session_sentences ───────────────────────────────────────────

pub fn session_sentences_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "session_id": {"type": "string", "description": "Session UUID"},
            "limit": {"type": "integer", "minimum": 1, "default": 50},
        },
        "required": ["session_id"],
        "additionalProperties": false
    })
}

/// Convenience over `session_patterns(type="turn.sentence")` — returns
/// a flat list of sentence records pulled from the pattern metadata.
pub async fn session_sentences(store: &Arc<dyn EventStore>, args: Value) -> Result<Value, String> {
    let session_id = extract_session_id(&args).map_err(|e| format!("session_sentences {e}"))?;
    let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(50) as usize;
    let patterns = store
        .session_patterns(session_id, Some("turn.sentence"))
        .await
        .map_err(|e| format!("session_patterns failed: {e}"))?;

    let sentences: Vec<Value> = patterns
        .into_iter()
        .take(limit)
        .map(|p| {
            // The pattern's metadata carries the sentence diagram; expose
            // the most useful fields at the top level for agent ergonomics.
            let summary = p.summary.clone();
            let started_at = p.started_at.clone();
            let event_ids = p.event_ids.clone();
            let meta = p.metadata;
            json!({
                "id": format!("turn.sentence:{}:{}", started_at, session_id),
                "session_id": session_id,
                "summary": summary,
                "started_at": started_at,
                "event_ids": event_ids,
                "verb": meta.get("verb").cloned().unwrap_or(Value::Null),
                "object": meta.get("object").cloned().unwrap_or(Value::Null),
                "human_prompt": meta.get("human_prompt").cloned().unwrap_or(Value::Null),
            })
        })
        .collect();
    Ok(json!({ "count": sentences.len(), "sentences": sentences }))
}

// ── session_plans ────────────────────────────────────────────────

pub fn session_plans_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "session_id": {"type": "string", "description": "Session UUID"},
        },
        "required": ["session_id"],
        "additionalProperties": false
    })
}

pub async fn session_plans(
    plan_store: &Arc<dyn PlanSource>,
    args: Value,
) -> Result<Value, String> {
    let session_id = extract_session_id(&args)
        .map_err(|e| format!("session_plans {e}"))?;
    let mut plans = plan_store.list_for_session(session_id).await;
    // Newest first.
    plans.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    serde_json::to_value(plans).map_err(|e| format!("serialize: {e}"))
}

// ── session_transcript ──────────────────────────────────────────

pub fn session_transcript_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "session_id": {"type": "string", "description": "Session UUID"},
            "assistant_only": {
                "type": "boolean", "default": false,
                "description": "Return only assistant messages",
            },
            "limit": {
                "type": "integer", "minimum": 1, "maximum": 5000,
                "description": "Cap on entries returned (default 500)",
            },
        },
        "required": ["session_id"],
        "additionalProperties": false
    })
}

pub async fn session_transcript(store: &Arc<dyn EventStore>, args: Value) -> Result<Value, String> {
    let session_id = extract_session_id(&args).map_err(|e| format!("session_transcript {e}"))?;
    let assistant_only = args
        .get("assistant_only")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(500) as usize;

    let events = store
        .session_events(session_id)
        .await
        .map_err(|e| format!("session_events failed: {e}"))?;

    let entries: Vec<Value> = events
        .iter()
        .filter_map(|ev| transcript_entry_from_event(ev, assistant_only))
        .take(limit)
        .collect();
    Ok(json!({ "entries": entries }))
}

/// Reconstruct a message-like transcript entry from a stored CloudEvent.
///
/// Paths (in order):
/// 1. Hermes / role-in-raw: `data.raw` (or nested) carries `role` + `content`.
/// 2. Subtype + typed payload (Grok ACP, Claude, …): map
///    `message.user.prompt` → user, `message.assistant.text` → assistant,
///    `message.assistant.thinking` → assistant (thinking text), using
///    `data.agent_payload.text` when present.
///
/// Tool events are omitted (not chat turns). Empty content is still emitted
/// when a role is known so callers can see turn structure.
pub(crate) fn transcript_entry_from_event(ev: &Value, assistant_only: bool) -> Option<Value> {
    // Path 1: Hermes / agent raw message shape with role.
    let raw = ev.get("data").and_then(|d| d.get("raw")).unwrap_or(ev);
    let inner = raw.get("data").unwrap_or(raw);
    if let Some(role) = inner.get("role").and_then(|v| v.as_str()) {
        if assistant_only && role != "assistant" {
            return None;
        }
        let content = inner
            .get("content")
            .cloned()
            .unwrap_or(Value::String(String::new()));
        return Some(json!({
            "role": role,
            "content": content,
            "time": ev.get("time").cloned().unwrap_or(Value::Null),
            "id": ev.get("id").cloned().unwrap_or(Value::Null),
        }));
    }

    // Path 2: CloudEvent subtype + typed agent_payload.text (Grok Build ACP).
    let subtype = ev.get("subtype").and_then(|v| v.as_str()).unwrap_or("");
    let (role, content) = match subtype {
        "message.user.prompt" => (
            "user",
            typed_payload_text(ev).unwrap_or_else(|| Value::String(String::new())),
        ),
        "message.assistant.text" | "message.assistant.thinking" => (
            "assistant",
            typed_payload_text(ev).unwrap_or_else(|| Value::String(String::new())),
        ),
        _ => return None,
    };
    if assistant_only && role != "assistant" {
        return None;
    }
    Some(json!({
        "role": role,
        "content": content,
        "time": ev.get("time").cloned().unwrap_or(Value::Null),
        "id": ev.get("id").cloned().unwrap_or(Value::Null),
    }))
}

fn typed_payload_text(ev: &Value) -> Option<Value> {
    let text = ev
        .pointer("/data/agent_payload/text")
        .and_then(|v| v.as_str())?;
    Some(Value::String(text.to_string()))
}

// ── session_activity ───────────────────────────────────────────

pub fn session_activity_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "session_id": {"type": "string", "description": "Session UUID"},
        },
        "required": ["session_id"],
        "additionalProperties": false
    })
}

pub async fn session_activity(store: &Arc<dyn EventStore>, args: Value) -> Result<Value, String> {
    let session_id = extract_session_id(&args).map_err(|e| format!("session_activity {e}"))?;
    let events = store
        .session_events(session_id)
        .await
        .map_err(|e| format!("session_events failed: {e}"))?;
    // Reuse open_story_store::analysis::activity_summary — the canonical
    // implementation the server's /api/sessions/{id}/activity endpoint
    // uses. Drop-in shape match.
    let summary = activity_summary(&events);
    serde_json::to_value(summary).map_err(|e| format!("serialize: {e}"))
}

#[cfg(test)]
mod transcript_entry_tests {
    use super::*;
    use serde_json::json;

    fn grok_cloud_event(subtype: &str, text: &str, id: &str) -> Value {
        json!({
            "specversion": "1.0",
            "id": id,
            "source": "grok://session/sess-g",
            "type": "io.arc.event",
            "time": "2026-07-17T00:16:55Z",
            "subtype": subtype,
            "agent": "grok",
            "data": {
                "seq": 1,
                "session_id": "sess-g",
                "raw": {
                    "method": "session/update",
                    "params": {"update": {"sessionUpdate": "agent_message_chunk"}}
                },
                "agent_payload": {
                    "_variant": "grok",
                    "meta": {"agent": "grok"},
                    "text": text
                }
            }
        })
    }

    // describe("when stored event is grok-build message.user.prompt")
    #[test]
    fn it_should_emit_user_role_from_typed_payload_without_raw_role() {
        let ev = grok_cloud_event(
            "message.user.prompt",
            "can you see this session?",
            "evt-user",
        );
        let entry = transcript_entry_from_event(&ev, false).expect("entry");
        assert_eq!(entry["role"], "user");
        assert_eq!(entry["content"], "can you see this session?");
        assert_eq!(entry["id"], "evt-user");
    }

    // describe("when stored event is grok-build message.assistant.text")
    #[test]
    fn it_should_emit_assistant_role_from_typed_payload() {
        let ev = grok_cloud_event(
            "message.assistant.text",
            "Yes — OpenStory MCP is already connected.",
            "evt-asst",
        );
        let entry = transcript_entry_from_event(&ev, false).expect("entry");
        assert_eq!(entry["role"], "assistant");
        assert_eq!(
            entry["content"],
            "Yes — OpenStory MCP is already connected."
        );
    }

    // describe("when assistant_only and event is user prompt")
    #[test]
    fn it_should_skip_user_when_assistant_only() {
        let ev = grok_cloud_event("message.user.prompt", "hi", "evt-u");
        assert!(transcript_entry_from_event(&ev, true).is_none());
    }

    // describe("when event is tool_use")
    #[test]
    fn it_should_skip_tool_events() {
        let mut ev = grok_cloud_event("message.assistant.tool_use", "", "evt-t");
        ev["subtype"] = json!("message.assistant.tool_use");
        assert!(transcript_entry_from_event(&ev, false).is_none());
    }

    // describe("when Hermes-style raw has role")
    #[test]
    fn it_should_still_prefer_raw_role_path() {
        let ev = json!({
            "id": "evt-h",
            "time": "2026-01-01T00:00:00Z",
            "subtype": "message.assistant.text",
            "data": {
                "raw": {
                    "data": {
                        "role": "assistant",
                        "content": "from raw role path"
                    }
                },
                "agent_payload": {
                    "text": "should not win if raw role present"
                }
            }
        });
        let entry = transcript_entry_from_event(&ev, false).expect("entry");
        assert_eq!(entry["role"], "assistant");
        assert_eq!(entry["content"], "from raw role path");
    }
}
