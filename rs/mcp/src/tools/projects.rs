//! Project-scoped tools: `project_context`, `recent_files`.
//!
//! Both wrap `EventStore::query_*` methods that take a project_id +
//! a limit. Thin async wrappers.

use open_story_store::event_store::EventStore;
use serde_json::{json, Value};
use std::sync::Arc;

fn extract_project(args: &Value) -> Result<&str, String> {
    args.get("project")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "requires `project` (string)".to_string())
}

pub fn project_context_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "project": {"type": "string", "description": "Project id"},
            "limit":   {"type": "integer", "minimum": 1, "maximum": 50, "default": 5,
                        "description": "Max recent sessions to return"},
        },
        "required": ["project"],
        "additionalProperties": false
    })
}

pub async fn project_context(store: &Arc<dyn EventStore>, args: Value) -> Result<Value, String> {
    let project = extract_project(&args).map_err(|e| format!("project_context {e}"))?;
    let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(5) as usize;
    let sessions = store.query_project_context(project, limit).await;
    serde_json::to_value(sessions).map_err(|e| format!("serialize: {e}"))
}

pub fn recent_files_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "project":       {"type": "string", "description": "Project id"},
            "session_limit": {"type": "integer", "minimum": 1, "maximum": 50, "default": 5,
                              "description": "How many recent sessions to scan for files"},
        },
        "required": ["project"],
        "additionalProperties": false
    })
}

pub async fn recent_files(store: &Arc<dyn EventStore>, args: Value) -> Result<Value, String> {
    let project = extract_project(&args).map_err(|e| format!("recent_files {e}"))?;
    let session_limit = args
        .get("session_limit")
        .and_then(|v| v.as_u64())
        .unwrap_or(5) as usize;
    let files = store.query_recent_files(project, session_limit).await;
    Ok(json!(files))
}
