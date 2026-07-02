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

pub mod analytics;
pub mod control;
pub mod per_session;
pub mod projects;
pub mod search;
pub mod sessions;
pub mod story;

use crate::server::Server;
use crate::subscription::Subscribe;
use serde_json::{json, Value};

pub struct ToolDef {
    pub name: &'static str,
    pub description: &'static str,
    pub input_schema: fn() -> Value,
}

/// The static tool surface. `tools/list` serializes this; `tools/call`
/// matches names against it.
pub const TOOLS: &[ToolDef] = &[
    // Agent-in-UI WRITE seam — drive the dashboard (never the observed sources).
    ToolDef {
        name: "ui_control",
        description: "Drive the OpenStory dashboard live (the agent-in-UI WRITE seam): steer what every open \
                      dashboard SHOWS — never the observed sources. Broadcasts to all connected dashboards. \
                      Args: action (open_view | present | toggle | set | query) + params. Examples: \
                      {action:'open_view', params:{route:'/canvas'}} — navigate; \
                      {action:'toggle', params:{target:'canvas.mode', value:'delegation'}} — flip a view control; \
                      {action:'present', params:{message:'…', sessionIds:['…'], route:'/story/…'}} — show a banner + spotlight. \
                      Returns {ok, delivered} (how many dashboards received it). Pair with where_is_user to drive from where the user is.",
        input_schema: control::ui_control_schema,
    },
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
    // Per-session detail tools.
    ToolDef {
        name: "tool_journey",
        description: "Tool-call sequence for a session, in timestamp order. \
                      Each entry: {tool, file, timestamp}.",
        input_schema: per_session::tool_journey_schema,
    },
    ToolDef {
        name: "file_impact",
        description: "Files read or written in a session, with per-file read/write counts. \
                      Sorted by total operations.",
        input_schema: per_session::file_impact_schema,
    },
    ToolDef {
        name: "session_errors",
        description: "Error events from a session (system.error subtype). \
                      Each entry: {timestamp, message}.",
        input_schema: per_session::session_errors_schema,
    },
    ToolDef {
        name: "session_patterns",
        description: "Detected patterns for a session. Optional `pattern_type` arg \
                      filters (e.g., turn.sentence, eval_apply.eval, git.workflow).",
        input_schema: per_session::session_patterns_schema,
    },
    ToolDef {
        name: "session_sentences",
        description: "Narrative sentences extracted from a session's turn.sentence \
                      patterns. Each carries verb/object/human_prompt for agent-level \
                      'what was the agent doing' reasoning.",
        input_schema: per_session::session_sentences_schema,
    },
    ToolDef {
        name: "session_plans",
        description: "List `/plan` documents written during a session, newest first. \
                      Each: {id, session_id, title, timestamp}.",
        input_schema: per_session::session_plans_schema,
    },
    ToolDef {
        name: "session_transcript",
        description: "Reconstructed message-like transcript of a session. \
                      Args: assistant_only (default false), limit (default 500). \
                      Entries: {role, content, time, id}.",
        input_schema: per_session::session_transcript_schema,
    },
    ToolDef {
        name: "session_activity",
        description: "Rich single-shot activity summary: first_prompt, files_touched, \
                      tool_breakdown, error_messages, last_response, conversation_turns, \
                      plan_count, duration_ms, start_time. Lower-level than session_story.",
        input_schema: per_session::session_activity_schema,
    },
    // Search tools.
    ToolDef {
        name: "search",
        description: "Full-text search across indexed events. Args: query (FTS5 syntax), \
                      limit (default 10), session_id (optional scope). Returns raw FTS hits.",
        input_schema: search::search_schema,
    },
    ToolDef {
        name: "agent_search",
        description: "FTS results grouped by session, agent-friendly. Args: query, \
                      project (optional, case-insensitive substring), limit (default 5). \
                      Returns top sessions by best_rank with matching events per session.",
        input_schema: search::agent_search_schema,
    },
    // Project-scoped tools.
    ToolDef {
        name: "project_context",
        description: "Recent sessions for a project. Args: project (id), limit (default 5).",
        input_schema: projects::project_context_schema,
    },
    ToolDef {
        name: "recent_files",
        description: "Files touched in the most recent sessions of a project. \
                      Args: project (id), session_limit (default 5).",
        input_schema: projects::recent_files_schema,
    },
    // Analytics tools.
    ToolDef {
        name: "token_usage",
        description: "Aggregated token usage with cost estimate. Args: days (optional), \
                      session_id (optional scope), model (sonnet/opus/haiku, default sonnet). \
                      Includes prompt-cache fields (cache_creation, cache_read) — the silent \
                      4-5-order-of-magnitude undercount that PR #55 fixed at the views layer.",
        input_schema: analytics::token_usage_schema,
    },
    ToolDef {
        name: "daily_token_usage",
        description: "Per-day token usage over a window. Args: days (default 7). \
                      Returns one entry per UTC date with the same TokenUsage shape.",
        input_schema: analytics::daily_token_usage_schema,
    },
    ToolDef {
        name: "productivity",
        description: "Hourly activity density: event counts per hour-of-day (0–23) \
                      over a window. Args: days (default 30).",
        input_schema: analytics::productivity_schema,
    },
    // Narrative tool.
    ToolDef {
        name: "session_story",
        description: "Structured fact-sheet for a session — record types, tool histogram, \
                      pattern counts, turn-phase mix, sample sentences (verb/object), \
                      opening + closing prompts, duration. Native Rust port of \
                      scripts/sessionstory.py; same JSON schema.",
        input_schema: story::session_story_schema,
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
pub async fn dispatch_query_tool<S: Subscribe>(
    server: &Server<S>,
    name: &str,
    args: Value,
) -> Value {
    let result: Result<Value, String> = match name {
        "ui_control" => control::ui_control(&server.api_base, args).await,
        "list_sessions" => sessions::list_sessions(&server.store, args).await,
        "session_synopsis" => sessions::session_synopsis(&server.store, args).await,
        "project_pulse" => sessions::project_pulse(&server.store, args).await,
        "tool_journey" => per_session::tool_journey(&server.store, args).await,
        "file_impact" => per_session::file_impact(&server.store, args).await,
        "session_errors" => per_session::session_errors(&server.store, args).await,
        "session_patterns" => per_session::session_patterns(&server.store, args).await,
        "session_sentences" => per_session::session_sentences(&server.store, args).await,
        "session_plans" => per_session::session_plans(&server.plan_store, args).await,
        "session_transcript" => per_session::session_transcript(&server.store, args).await,
        "session_activity" => per_session::session_activity(&server.store, args).await,
        "search" => search::search(&server.store, args).await,
        "agent_search" => search::agent_search(&server.store, args).await,
        "project_context" => projects::project_context(&server.store, args).await,
        "recent_files" => projects::recent_files(&server.store, args).await,
        "token_usage" => analytics::token_usage(&server.store, args).await,
        "daily_token_usage" => analytics::daily_token_usage(&server.store, args).await,
        "productivity" => analytics::productivity(&server.store, args).await,
        "session_story" => story::session_story(&server.store, args).await,
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
