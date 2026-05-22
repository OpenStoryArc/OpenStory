//! Token + productivity analytics tools.
//!
//! All three wrap a single `EventStore::query_*` method. `token_usage`
//! is the most consequential — it exercises the cache-field projection
//! that landed in PR #55 (commit `fix(views)`), so a regression in
//! that fix would show up here as miscounted tokens.

use open_story_store::event_store::EventStore;
use serde_json::{json, Value};
use std::sync::Arc;

// ── token_usage ────────────────────────────────────────────────

pub fn token_usage_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "days":       {"type": "integer", "minimum": 1, "description": "Window in days (omit for all-time)"},
            "session_id": {"type": "string", "description": "Optional — scope to one session"},
            "model":      {"type": "string", "default": "sonnet",
                           "description": "Pricing model for cost estimate: sonnet, opus, haiku"},
        },
        "additionalProperties": false
    })
}

pub async fn token_usage(
    store: &Arc<dyn EventStore>,
    args: Value,
) -> Result<Value, String> {
    let days = args.get("days").and_then(|v| v.as_u64()).map(|d| d as u32);
    let session_id = args.get("session_id").and_then(|v| v.as_str());
    let model = args.get("model").and_then(|v| v.as_str()).unwrap_or("sonnet");

    let summary = store.query_token_usage(days, session_id, model).await;
    serde_json::to_value(summary).map_err(|e| format!("serialize: {e}"))
}

// ── daily_token_usage ──────────────────────────────────────────

pub fn daily_token_usage_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "days": {"type": "integer", "minimum": 1, "default": 7,
                     "description": "Window in days (default 7)"},
        },
        "additionalProperties": false
    })
}

pub async fn daily_token_usage(
    store: &Arc<dyn EventStore>,
    args: Value,
) -> Result<Value, String> {
    let days = args.get("days").and_then(|v| v.as_u64()).map(|d| d as u32);
    let daily = store.query_daily_token_usage(days).await;
    serde_json::to_value(daily).map_err(|e| format!("serialize: {e}"))
}

// ── productivity ───────────────────────────────────────────────

pub fn productivity_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "days": {"type": "integer", "minimum": 1, "default": 30,
                     "description": "Window in days (default 30)"},
        },
        "additionalProperties": false
    })
}

pub async fn productivity(
    store: &Arc<dyn EventStore>,
    args: Value,
) -> Result<Value, String> {
    let days = args
        .get("days")
        .and_then(|v| v.as_u64())
        .unwrap_or(30) as u32;
    let hourly = store.query_productivity_by_hour(days).await;
    serde_json::to_value(hourly).map_err(|e| format!("serialize: {e}"))
}
