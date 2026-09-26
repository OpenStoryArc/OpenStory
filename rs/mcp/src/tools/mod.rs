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
pub mod help;
pub mod ops;
pub mod per_session;
pub mod projects;
pub mod reels;
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
    // In-band curriculum (no store) — agents without resources/read still get hands.
    ToolDef {
        name: "openstory_help",
        description: "WHEN: you are stuck or need the body schema without reading the repo. \
                      MOTION: stuck / any. CALL: { need?: orient|what-touched|find|cost|live|show-human, \
                      topic?: hands|physics|ui|session_story|… }. \
                      RETURNS: markdown card + resource URIs (no store I/O). \
                      NEXT: call the suggested tools; resources/read openstory://docs/hands for full curriculum. \
                      LAW: history is read-only; sentences are projections not intent.",
        input_schema: help::openstory_help_schema,
    },
    // Agent-in-UI WRITE seam — drive the dashboard (never the observed sources).
    ToolDef {
        name: "navigate_to",
        description: "WHEN: put attention on ANY event, session, file, or canvas graph (primary click-parity hand). \
                      MOTION: show-human. CALL: { kind, id?, sessionId?, eventId?, view?, details?, canvasMode?, spotlight? }. \
                      kind=event + sessionId → focus that event (story/explore); details:true expands sentence depth. \
                      kind=session → open explore/story; canvasMode → open that chart then select session. \
                      kind=canvas + canvasMode → switch graph mode; + sessionId → click that session on the chart. \
                      kind=file|person|project → search/filter. \
                      kind=facet + chip-id (facet-host-…) or {facet,id} → sidebar facet chip land. \
                      RETURNS: {ok, delivered, ui_state, hint}. Steers ui.* only. \
                      NEXT: where_is_user. Prefer this over assembling ui_control steps by hand.",
        input_schema: control::navigate_to_schema,
    },
    ToolDef {
        name: "ui_control",
        description: "WHEN: low-level dashboard drive (or navigate_to is not enough). MOTION: show-human. \
                      CALL: { action, params }. Verbs: open_view | focus_event | navigate_to | present | query | toggle | set. \
                      Prefer navigate_to for events/graphs. set canvas.select_session {sessionId} = chart click. \
                      set story.details {open, sessionId, eventId}. toggle canvas.mode. \
                      RETURNS: {ok, delivered}. NEXT: where_is_user. Docs: openstory://docs/agent-in-ui.",
        input_schema: control::ui_control_schema,
    },
    ToolDef {
        name: "save_reel",
        description: "WHEN: you have a story to keep — turn verified events into a saved, replayable reel. \
                      MOTION: show-human. CALL: { title, stops: [{sessionId, eventId, line, clipAt?}], closer?, author? }. \
                      Every stop must reference a REAL recorded event — invented ids are rejected with invalid_stops. \
                      RETURNS: {ok, id} | {ok:false, invalid_stops}. NEXT: play_reel {id}. \
                      LAW: reels are curation about history; they never mutate events.",
        input_schema: reels::save_reel_schema,
    },
    ToolDef {
        name: "list_reels",
        description: "WHEN: see saved reels (yours and others'). MOTION: orient / show-human. \
                      CALL: {}. RETURNS: [{id, title, created, author, stopCount}]. NEXT: play_reel.",
        input_schema: reels::list_reels_schema,
    },
    ToolDef {
        name: "play_reel",
        description: "WHEN: play a saved reel on the human's dashboard (Reels tab, Event Spotlight per stop, \
                      caption + voice). MOTION: show-human. CALL: { id }. RETURNS: {ok, delivered}. \
                      NEXT: where_is_user — the human can take the wheel any time.",
        input_schema: reels::play_reel_schema,
    },
    ToolDef {
        name: "subscribe_ui_state",
        description: "WHEN: live-follow the human on the dashboard. MOTION: show-human. STREAMING. \
                      CALL: {} (no args). Emits notifications/openstory/ui_state (same shape as where_is_user). \
                      Subscribes to authored ui.* only — never observed events.*. \
                      NEXT: ui_control from where they just moved. Docs: openstory://docs/agent-in-ui.",
        input_schema: control::where_is_user_schema,
    },
    ToolDef {
        name: "where_is_user",
        description: "WHEN: follow the human before/after driving the UI. MOTION: show-human. \
                      CALL: {} (no args). RETURNS: {present, view, session_id?, event_id?, detail_view?, \
                      filters?, summary, …}. NEXT: ui_control from that context; subscribe_ui_state for live. \
                      Prefer driving when the user is idle (tempo on GET /api/ui-state).",
        input_schema: control::where_is_user_schema,
    },
    // Query tools (routed through dispatch_query_tool).
    ToolDef {
        name: "list_sessions",
        description: "WHEN: find sessions to inspect / resume. MOTION: orient. \
                      CALL: { days?, project?, limit?, after? }. \
                      RETURNS: trim rows (id, label, project, times, event_count). \
                      NEXT: session_story | session_synopsis on a chosen id.",
        input_schema: sessions::list_sessions_schema,
    },
    ToolDef {
        name: "session_synopsis",
        description: "WHEN: quick structured overview of one session. MOTION: orient. \
                      CALL: { session_id }. RETURNS: counts, time range, top tools. \
                      NEXT: session_story for the full fact sheet.",
        input_schema: sessions::session_synopsis_schema,
    },
    ToolDef {
        name: "project_pulse",
        description: "WHEN: activity across projects over a window. MOTION: orient (fleet). \
                      CALL: { days? default 7 }. RETURNS: per-project session/event counts. \
                      NEXT: list_sessions | project_context.",
        input_schema: sessions::project_pulse_schema,
    },
    // Per-session detail tools.
    ToolDef {
        name: "tool_journey",
        description: "WHEN: chronological tool sequence for a session. MOTION: what-touched. \
                      CALL: { session_id }. RETURNS: {tool, file, timestamp}[]. \
                      NEXT: file_impact | session_story.",
        input_schema: per_session::tool_journey_schema,
    },
    ToolDef {
        name: "file_impact",
        description: "WHEN: which files were read/written in a session. MOTION: what-touched. \
                      CALL: { session_id }. RETURNS: per-file read/write counts (sorted). \
                      NEXT: session_sentences | search path across fleet.",
        input_schema: per_session::file_impact_schema,
    },
    ToolDef {
        name: "session_errors",
        description: "WHEN: error events in a session. MOTION: find / orient. \
                      CALL: { session_id }. RETURNS: {timestamp, message}[] system.error. \
                      NEXT: search for the message across fleet.",
        input_schema: per_session::session_errors_schema,
    },
    ToolDef {
        name: "session_patterns",
        description: "WHEN: raw detected patterns (eval-apply, sentences, …). MOTION: what-touched. \
                      CALL: { session_id, pattern_type? e.g. turn.sentence }. \
                      RETURNS: pattern list. NEXT: session_sentences for SVO projection view. \
                      LIMITS: may be empty without turn boundaries — see openstory://docs/physics.",
        input_schema: per_session::session_patterns_schema,
    },
    ToolDef {
        name: "session_sentences",
        description: "WHEN: turn-level SVO coordinates of what tools did. MOTION: what-touched / orient. \
                      CALL: { session_id }. RETURNS: verb/object/human_prompt from turn.sentence \
                      (deterministic projection — NOT an intent label or monologue). \
                      NEXT: session_story | file_impact. \
                      LIMITS: empty if no turn.complete / turn boundaries — try session_activity.",
        input_schema: per_session::session_sentences_schema,
    },
    ToolDef {
        name: "session_plans",
        description: "WHEN: /plan documents from a session. MOTION: orient. \
                      CALL: { session_id }. RETURNS: {id, session_id, title, timestamp}[]. \
                      NEXT: session_story.",
        input_schema: per_session::session_plans_schema,
    },
    ToolDef {
        name: "session_transcript",
        description: "WHEN: reconstructed message transcript (heavier than story). MOTION: orient. \
                      CALL: { session_id, assistant_only?, limit? }. RETURNS: {role, content, time, id}[]. \
                      NEXT: prefer session_story first to avoid dumping the territory.",
        input_schema: per_session::session_transcript_schema,
    },
    ToolDef {
        name: "session_activity",
        description: "WHEN: rich single-shot activity without full story shape. MOTION: orient. \
                      CALL: { session_id }. RETURNS: first_prompt, files_touched, tool_breakdown, \
                      errors, duration, …. NEXT: session_story if available; fallback when sentences empty.",
        input_schema: per_session::session_activity_schema,
    },
    // Search tools.
    ToolDef {
        name: "search",
        description: "WHEN: full-text find across events. MOTION: find. \
                      CALL: { query, limit?, session_id? }. RETURNS: raw FTS hits. \
                      NEXT: session_story on hit sessions; agent_search for session-grouped ranking.",
        input_schema: search::search_schema,
    },
    ToolDef {
        name: "agent_search",
        description: "WHEN: find across fleet, grouped by session (agent-friendly). MOTION: find. \
                      CALL: { query, project?, limit? }. RETURNS: top sessions by rank + matching events. \
                      NEXT: session_story on a hit session_id.",
        input_schema: search::agent_search_schema,
    },
    // Project-scoped tools.
    ToolDef {
        name: "project_context",
        description: "WHEN: recent sessions for one project. MOTION: orient. \
                      CALL: { project, limit? }. RETURNS: recent session rows. \
                      NEXT: session_story.",
        input_schema: projects::project_context_schema,
    },
    ToolDef {
        name: "recent_files",
        description: "WHEN: files touched in a project's recent sessions. MOTION: what-touched. \
                      CALL: { project, session_limit? }. RETURNS: file list. \
                      NEXT: file_impact on a session | search path.",
        input_schema: projects::recent_files_schema,
    },
    // Analytics tools.
    ToolDef {
        name: "token_usage",
        description: "WHEN: aggregated token spend / cache. MOTION: cost. \
                      CALL: { days?, session_id?, model? }. RETURNS: usage + cost estimate (incl. cache). \
                      NEXT: daily_token_usage for trends.",
        input_schema: analytics::token_usage_schema,
    },
    ToolDef {
        name: "daily_token_usage",
        description: "WHEN: per-day token spend over a window. MOTION: cost. \
                      CALL: { days? default 7 }. RETURNS: one TokenUsage-shaped row per UTC date. \
                      NEXT: token_usage for session scope.",
        input_schema: analytics::daily_token_usage_schema,
    },
    ToolDef {
        name: "productivity",
        description: "WHEN: hourly activity density. MOTION: orient (fleet rhythm). \
                      CALL: { days? default 30 }. RETURNS: event counts per hour-of-day 0–23.",
        input_schema: analytics::productivity_schema,
    },
    // Narrative tool.
    ToolDef {
        name: "session_story",
        description: "WHEN: best single fact sheet for a session (prefer before full transcript). \
                      MOTION: orient. CALL: { session_id }. \
                      RETURNS: record types, tool histogram, pattern counts, sample sentences \
                      (verb/object projections), opening+closing prompts, duration. \
                      NEXT: session_sentences | tool_journey | session_errors. \
                      NOT: an LLM interpretation of intent.",
        input_schema: story::session_story_schema,
    },
    // Streaming tools (handled inline in stdio.rs; entries here so
    // tools/list reports them).
    ToolDef {
        name: "subscribe_session",
        description: "WHEN: watch a session live as events land. MOTION: live. \
                      CALL: { session_id }. RETURNS: {stream_id, status:started} then \
                      notifications/openstory/stream. Cancel via notifications/cancelled. \
                      Read-only observation stream.",
        input_schema: subscribe_session_schema,
    },
    ToolDef {
        name: "subscribe_tokens",
        description: "WHEN: watch running token tally for a session (self-reflection). MOTION: live. \
                      CALL: { session_id }. RETURNS: started then notifications/openstory/tokens \
                      (input/output/cache). Cancel via notifications/cancelled.",
        input_schema: subscribe_session_schema,
    },
    // Ops hands, tier 0 (group M): watch and diagnose the node; read-only.
    ToolDef {
        name: "node_health",
        description: "WHEN: something looks wrong, or before proposing any change. MOTION: diagnose. \
                      CALL: {}. RETURNS: /api/health with the node's own verdict {level, findings[{id, level, text}]}; \
                      a finding id is the evidence a proposal cites. NEXT: node_logs {actor} for the why; \
                      node_streams for caps.",
        input_schema: ops::empty_schema,
    },
    ToolDef {
        name: "node_logs",
        description: "WHEN: you need the why behind a finding. MOTION: diagnose. \
                      CALL: { since?, actor?, level?, limit? }. RETURNS: {lines[{seq, level, actor, event, …}], next}; \
                      pass next back as since to page. LAW: read-only.",
        input_schema: ops::node_logs_schema,
    },
    ToolDef {
        name: "node_streams",
        description: "WHEN: a stream_cap finding, or before a resize proposal. MOTION: diagnose. \
                      CALL: {}. RETURNS: {streams[{name, bytes, messages, max_bytes, percent, level}]}; \
                      warn at 70 %, critical at 90 %.",
        input_schema: ops::empty_schema,
    },
    ToolDef {
        name: "fleet_presence",
        description: "WHEN: who is alive across the fleet. MOTION: watch. \
                      CALL: {}. RETURNS: {interval_secs, stale_after_secs, nodes[{host, principal_id, age_secs, stale, git_sha, …}]}; \
                      a node is stale past three beats.",
        input_schema: ops::empty_schema,
    },
    ToolDef {
        name: "subscribe_health",
        description: "WHEN: you are watching a node and want to hear only when its verdict moves. MOTION: watch. \\
                      CALL: { interval_secs? (default 15) }. RETURNS: started with the current verdict, then \\
                      notifications/openstory/health {from, to, added, cleared, verdict, seq} on every transition; \\
                      silence means nothing changed. Cancel via notifications/cancelled.",
        input_schema: ops::subscribe_health_schema,
    },
    // Ops hands, tier 1 (M-06): change only what is derived; propose first.
    ToolDef {
        name: "node_reproject",
        description: "WHEN: projections_stale, or a session reads empty after a restart. MOTION: propose. \\
                      CALL: { session_id?, evidence?, idempotency_key?, author? }. DOES: publishes ops.proposal.reproject, \\
                      then the node rebuilds the projection from the store and records ops.command.reproject. \\
                      RETURNS: {proposal, result{sessions_reprojected, events_applied}, command_subject}. Refused while not serving.",
        input_schema: ops::node_reproject_schema,
    },
    ToolDef {
        name: "node_verify",
        description: "WHEN: you suspect the store, the JSONL backup, and FTS disagree on a session. MOTION: propose. \\
                      CALL: { session_id, evidence?, idempotency_key?, author? }. DOES: publishes ops.proposal.verify, then the node \\
                      counts all three. RETURNS: {result{store_events, jsonl_lines, fts_documents, fts_unindexed, agree}}; agree means \\
                      the store and its backup match, FTS is reported. Read-only act.",
        input_schema: ops::node_verify_schema,
    },
    ToolDef {
        name: "node_catch_up",
        description: "WHEN: a peer holds sessions this node is missing. MOTION: propose. \\
                      CALL: { peer?, evidence?, idempotency_key?, author? }. DOES: publishes ops.proposal.catch_up, then one \\
                      digest diff against the peer, pulling what is missing. RETURNS: {result{peer, healed}}.",
        input_schema: ops::node_catch_up_schema,
    },
    ToolDef {
        name: "node_prune",
        description: "WHEN: a stream_cap or store size finding and old fleet-mirrored sessions. MOTION: propose. \\
                      CALL: { older_than_days, evidence?, idempotency_key?, author? }. DOES: publishes ops.proposal.prune, then \\
                      the node deletes fleet sessions older than that, never its own. RETURNS: {result{deleted}}.",
        input_schema: ops::node_prune_schema,
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
        "openstory_help" => help::openstory_help(args),
        "navigate_to" => control::navigate_to(&server.api_base, args).await,
        "ui_control" => control::ui_control(&server.api_base, args).await,
        "where_is_user" => control::where_is_user(&server.api_base, args).await,
        "save_reel" => reels::save_reel(&server.api_base, args).await,
        "list_reels" => reels::list_reels(&server.api_base, args).await,
        "play_reel" => reels::play_reel(&server.api_base, args).await,
        "node_health" => ops::node_health(&server.api_base, args).await,
        "node_logs" => ops::node_logs(&server.api_base, args).await,
        "node_streams" => ops::node_streams(&server.api_base, args).await,
        "fleet_presence" => ops::fleet_presence(&server.api_base, args).await,
        "node_reproject" => ops::tier_one(server, "reproject", args).await,
        "node_verify" => ops::tier_one(server, "verify", args).await,
        "node_catch_up" => ops::tier_one(server, "catch_up", args).await,
        "node_prune" => ops::tier_one(server, "prune", args).await,
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
