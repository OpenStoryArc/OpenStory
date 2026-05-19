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

pub async fn tool_journey(
    store: &Arc<dyn EventStore>,
    args: Value,
) -> Result<Value, String> {
    let session_id = extract_session_id(&args)
        .map_err(|e| format!("tool_journey {e}"))?;
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

pub async fn file_impact(
    store: &Arc<dyn EventStore>,
    args: Value,
) -> Result<Value, String> {
    let session_id = extract_session_id(&args)
        .map_err(|e| format!("file_impact {e}"))?;
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

pub async fn session_errors(
    store: &Arc<dyn EventStore>,
    args: Value,
) -> Result<Value, String> {
    let session_id = extract_session_id(&args)
        .map_err(|e| format!("session_errors {e}"))?;
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

pub async fn session_patterns(
    store: &Arc<dyn EventStore>,
    args: Value,
) -> Result<Value, String> {
    let session_id = extract_session_id(&args)
        .map_err(|e| format!("session_patterns {e}"))?;
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
pub async fn session_sentences(
    store: &Arc<dyn EventStore>,
    args: Value,
) -> Result<Value, String> {
    let session_id = extract_session_id(&args)
        .map_err(|e| format!("session_sentences {e}"))?;
    let limit = args
        .get("limit")
        .and_then(|v| v.as_u64())
        .unwrap_or(50) as usize;
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
