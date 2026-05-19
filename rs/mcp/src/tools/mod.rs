//! Tool registry + dispatch.
//!
//! Each tool ships as a static definition (name, description,
//! inputSchema) so `tools/list` can return the surface. The dispatcher
//! routes `tools/call` by name to an async fn that has access to the
//! `Server` context (subscriber for streaming tools, store for query
//! tools).
//!
//! Streaming tools (`subscribe_session`, `subscribe_tokens`) are
//! special-cased in `stdio.rs` because they need the JSON-RPC id +
//! the writer channel to emit notifications. Everything else routes
//! through `dispatch_query_tool` here.

pub mod sessions;

use open_story_store::event_store::EventStore;
use serde_json::{json, Value};
use std::sync::Arc;

pub struct ToolDef {
    pub name: &'static str,
    pub description: &'static str,
    pub input_schema: fn() -> Value,
}

/// The static tool surface. `tools/list` serializes this; `tools/call`
/// matches names against it.
pub const TOOLS: &[ToolDef] = &[
    // Query tools (routed through dispatch_query_tool).
    ToolDef {
        name: "list_sessions",
        description: "List coding sessions with optional filters. \
                      Args: days (window in days back from now), project, limit (max rows, default 100), after \
                      (ISO-8601, only sessions with last_event >= after). \
                      Returns a trim shape (id, label, project_id, project_name, start, last_event, event_count) — \
                      use session_synopsis for full per-session data.",
        input_schema: sessions::list_sessions_schema,
    },
    ToolDef {
        name: "session_synopsis",
        description: "Structured overview of one session: counts, time range, top tools. \
                      First tool to call when investigating a specific session id.",
        input_schema: sessions::session_synopsis_schema,
    },
    ToolDef {
        name: "project_pulse",
        description: "Activity summary across projects over a window. \
                      Args: days (default 7). Returns project_id, project_name, session_count, \
                      event_count, last_activity per project.",
        input_schema: sessions::project_pulse_schema,
    },
    // Streaming tools (handled inline in stdio.rs; entries here so
    // tools/list reports them).
    ToolDef {
        name: "subscribe_session",
        description: "Subscribe to a session's events as they happen. \
                      Returns {stream_id, status: 'started'} immediately; subsequent \
                      `notifications/openstory/stream` messages carry events tagged \
                      with stream_id. Cancel via `notifications/cancelled`.",
        input_schema: subscribe_session_schema,
    },
    ToolDef {
        name: "subscribe_tokens",
        description: "Self-reflective token watcher. Subscribes to a session and streams \
                      a running token tally (input, output, cache_read, cache_create) per \
                      assistant message. Useful for an agent to watch its own context \
                      consumption. Emits `notifications/openstory/tokens` with delta + \
                      running total. Cancel via `notifications/cancelled`.",
        input_schema: subscribe_session_schema,
    },
];

fn subscribe_session_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "session_id": {"type": "string", "description": "Session UUID to subscribe to"},
        },
        "required": ["session_id"],
        "additionalProperties": false
    })
}

/// Serialize the static tool set into the MCP `tools/list` result shape.
pub fn list_tools_result() -> Value {
    let tools: Vec<Value> = TOOLS
        .iter()
        .map(|t| {
            json!({
                "name": t.name,
                "description": t.description,
                "inputSchema": (t.input_schema)(),
            })
        })
        .collect();
    json!({ "tools": tools })
}

/// Dispatch a query tool by name. Returns the body of an MCP
/// `tools/call` result (`{isError, content}`); the caller wraps it in
/// a JSON-RPC success response.
///
/// Streaming tools (`subscribe_*`) are NOT routed here — they need the
/// JSON-RPC id and the writer channel and live in `stdio.rs`.
pub async fn dispatch_query_tool(
    store: &Arc<dyn EventStore>,
    name: &str,
    args: Value,
) -> Value {
    let result: Result<Value, String> = match name {
        "list_sessions" => sessions::list_sessions(store, args).await,
        "session_synopsis" => sessions::session_synopsis(store, args).await,
        "project_pulse" => sessions::project_pulse(store, args).await,
        unknown => {
            return tool_not_found(unknown);
        }
    };
    wrap_tool_result(result)
}

fn tool_not_found(name: &str) -> Value {
    json!({
        "isError": true,
        "content": [{"type": "text", "text": format!("Unknown tool: {name}")}],
    })
}

fn wrap_tool_result(result: Result<Value, String>) -> Value {
    match result {
        Ok(value) => json!({
            "isError": false,
            "content": [{"type": "text", "text": serde_json::to_string(&value).unwrap_or_default()}],
        }),
        Err(err) => json!({
            "isError": true,
            "content": [{"type": "text", "text": err}],
        }),
    }
}
