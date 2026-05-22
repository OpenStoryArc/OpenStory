//! Session-scoped tools wired to `open-story-store::queries`.
//!
//! Three tools live here in Commit B:
//!   - `list_sessions` — filterable, trim-row response (fixes the
//!     178KB blowup the Python MCP had)
//!   - `session_synopsis` — wraps `EventStore::query_session_synopsis`
//!   - `project_pulse` — wraps `EventStore::query_project_pulse`
//!
//! Subsequent commits add `session_activity` and `session_story` to
//! this module.

use open_story_store::event_store::EventStore;
use serde_json::{json, Value};
use std::sync::Arc;

// ── list_sessions ──────────────────────────────────────────────────

pub fn list_sessions_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "days": {
                "type": "integer", "minimum": 1,
                "description": "Look back N days (filters by last_event)",
            },
            "project": {
                "type": "string",
                "description": "Filter by project_id or project_name (case-insensitive substring match)",
            },
            "limit": {
                "type": "integer", "minimum": 1, "maximum": 500,
                "description": "Max rows to return (default 100)",
            },
            "after": {
                "type": "string",
                "description": "ISO-8601 timestamp; only sessions with last_event >= after",
            },
        },
        "additionalProperties": false
    })
}

pub async fn list_sessions(
    store: &Arc<dyn EventStore>,
    args: Value,
) -> Result<Value, String> {
    let days = args.get("days").and_then(|v| v.as_u64());
    let project = args.get("project").and_then(|v| v.as_str()).map(str::to_lowercase);
    let limit = args
        .get("limit")
        .and_then(|v| v.as_u64())
        .unwrap_or(100) as usize;
    let after = args.get("after").and_then(|v| v.as_str()).map(str::to_string);

    // Compute the lower bound on last_event from `days` or `after`,
    // whichever is more restrictive.
    let cutoff_from_days = days.map(|d| {
        let cutoff = chrono::Utc::now() - chrono::Duration::days(d as i64);
        cutoff.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
    });
    let cutoff = match (cutoff_from_days, after) {
        (Some(a), Some(b)) => Some(if a > b { a } else { b }),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    };

    let rows = store
        .list_sessions()
        .await
        .map_err(|e| format!("list_sessions failed: {e}"))?;

    let mut filtered: Vec<Value> = rows
        .into_iter()
        .filter(|r| {
            if let Some(ref c) = cutoff {
                match &r.last_event {
                    Some(last) => last.as_str() >= c.as_str(),
                    None => false,
                }
            } else {
                true
            }
        })
        .filter(|r| match &project {
            Some(p) => {
                let in_id = r
                    .project_id
                    .as_ref()
                    .map(|x| x.to_lowercase().contains(p))
                    .unwrap_or(false);
                let in_name = r
                    .project_name
                    .as_ref()
                    .map(|x| x.to_lowercase().contains(p))
                    .unwrap_or(false);
                in_id || in_name
            }
            None => true,
        })
        .map(|r| {
            // Trim shape — drop host/user/branch/custom_label that the
            // Python MCP carried and blew the token budget with.
            json!({
                "id": r.id,
                "label": r.label,
                "project_id": r.project_id,
                "project_name": r.project_name,
                "start": r.first_event,
                "last_event": r.last_event,
                "event_count": r.event_count,
            })
        })
        .collect();

    // Sort by last_event DESC so most-recent activity comes first.
    filtered.sort_by(|a, b| {
        b["last_event"]
            .as_str()
            .unwrap_or("")
            .cmp(a["last_event"].as_str().unwrap_or(""))
    });

    filtered.truncate(limit);
    Ok(Value::Array(filtered))
}

// ── session_synopsis ──────────────────────────────────────────────

pub fn session_synopsis_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "session_id": {"type": "string", "description": "Session UUID"},
        },
        "required": ["session_id"],
        "additionalProperties": false
    })
}

pub async fn session_synopsis(
    store: &Arc<dyn EventStore>,
    args: Value,
) -> Result<Value, String> {
    let session_id = args
        .get("session_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "session_synopsis requires `session_id`".to_string())?;

    match store.query_session_synopsis(session_id).await {
        Some(synopsis) => serde_json::to_value(synopsis)
            .map_err(|e| format!("serialize synopsis: {e}")),
        None => Err(format!("session {session_id} not found")),
    }
}

// ── project_pulse ─────────────────────────────────────────────────

pub fn project_pulse_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "days": {"type": "integer", "minimum": 1, "default": 7,
                     "description": "Look back N days (default 7)"},
        },
        "additionalProperties": false
    })
}

pub async fn project_pulse(
    store: &Arc<dyn EventStore>,
    args: Value,
) -> Result<Value, String> {
    let days = args
        .get("days")
        .and_then(|v| v.as_u64())
        .unwrap_or(7) as u32;
    let pulse = store.query_project_pulse(days).await;
    serde_json::to_value(pulse).map_err(|e| format!("serialize pulse: {e}"))
}
