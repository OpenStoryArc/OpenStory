//! Tool definitions and the registry that serves `tools/list`.
//!
//! Each tool ships as a static definition (name, description,
//! inputSchema) plus — eventually — an async invoker. For now we
//! only need the definitions so `tools/list` returns the seed surface
//! the agent expects.

use serde_json::{json, Value};

/// A tool invocation either produces a JSON result or a human-readable
/// tool-level error. Protocol-level errors (bad JSON, unknown method)
/// are separate — see `protocol::error_code`.
pub type InvokeFn = fn(Value) -> Result<Value, String>;

pub struct ToolDef {
    pub name: &'static str,
    pub description: &'static str,
    pub input_schema: fn() -> Value,
    pub invoke: InvokeFn,
}

/// Stage A seed surface. New tools land here as TDD slices add them.
pub const SEED_TOOLS: &[ToolDef] = &[
    ToolDef {
        name: "list_sessions",
        description: "List coding sessions with optional filters. \
                      Args: days (window in hours/24), project, limit, after. \
                      Returns a trim shape (id, label, project, start, last_event, event_count) — \
                      use session_synopsis for full per-session data.",
        input_schema: list_sessions_schema,
        invoke: stub_not_wired,
    },
    ToolDef {
        name: "session_synopsis",
        description: "Structured overview of one session: counts, time range, top tools. \
                      First tool to call when investigating a specific session id.",
        input_schema: session_synopsis_schema,
        invoke: stub_not_wired,
    },
    ToolDef {
        name: "project_pulse",
        description: "Activity summary across projects over a window. \
                      Args: days (default 7). Returns project_id, name, session_count, \
                      event_count, last_activity.",
        input_schema: project_pulse_schema,
        invoke: stub_not_wired,
    },
    ToolDef {
        name: "subscribe_session",
        description: "Subscribe to a session's events as they happen. \
                      Returns {stream_id, status: 'started'} immediately; subsequent \
                      `notifications/openstory/stream` messages carry events tagged \
                      with stream_id. Cancel via `notifications/cancelled`.",
        input_schema: subscribe_session_schema,
        invoke: subscribe_session_marker,
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

/// Marker invoke for subscribe_session — the stdio layer special-cases
/// this tool because it needs bus + writer access. If this is ever
/// dispatched through the generic path, it means the special-casing
/// got missed.
fn subscribe_session_marker(_args: Value) -> Result<Value, String> {
    Err("subscribe_session must be handled by the streaming transport layer".to_string())
}

/// Placeholder invoke for tools whose store-wiring slice hasn't landed yet.
/// Returns success at the tool-level so dispatch tests pass; the
/// next TDD slice replaces this with a real implementation per-tool.
fn stub_not_wired(_args: Value) -> Result<Value, String> {
    Ok(json!({"status": "scaffolded — store wiring lands in next TDD slice"}))
}

/// Look up a tool by name and call its invoker.
/// Returns the MCP `tools/call` result body shape.
pub fn dispatch_tool_call(name: &str, args: Value) -> Value {
    let Some(tool) = SEED_TOOLS.iter().find(|t| t.name == name) else {
        return json!({
            "isError": true,
            "content": [{"type": "text", "text": format!("Unknown tool: {name}")}],
        });
    };

    match (tool.invoke)(args) {
        Ok(result) => json!({
            "isError": false,
            "content": [{"type": "text", "text": serde_json::to_string(&result).unwrap_or_default()}],
        }),
        Err(err) => json!({
            "isError": true,
            "content": [{"type": "text", "text": err}],
        }),
    }
}

fn list_sessions_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "days":    {"type": "integer", "minimum": 1, "description": "Look back N days"},
            "project": {"type": "string", "description": "Filter by project name"},
            "limit":   {"type": "integer", "minimum": 1, "maximum": 500, "description": "Max rows"},
            "after":   {"type": "string", "description": "ISO-8601 timestamp; only sessions with last_event >= after"},
        },
        "additionalProperties": false
    })
}

fn session_synopsis_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "session_id": {"type": "string", "description": "Session UUID"},
        },
        "required": ["session_id"],
        "additionalProperties": false
    })
}

fn project_pulse_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "days": {"type": "integer", "minimum": 1, "default": 7},
        },
        "additionalProperties": false
    })
}

/// Serialize the seed tool set into the MCP `tools/list` result shape.
pub fn list_tools_result() -> Value {
    let tools: Vec<Value> = SEED_TOOLS
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
