//! REST API handlers — all /api/* routes.

use std::collections::HashMap;
use std::path::Path;

use axum::extract::{Path as AxumPath, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use chrono::Utc;
use open_story_store::analysis::{activity_summary, session_summary, tool_call_distribution};

use crate::logging::{log_event, short_id};
use crate::state::SharedState;
use crate::tool_schemas::schemas_to_json;
use crate::transcript::{find_transcript_path, read_transcript};

#[derive(Deserialize)]
pub struct SessionListQuery {
    /// Maximum number of sessions to return (default: all).
    pub limit: Option<usize>,
    /// Number of sessions to skip (default: 0). Applied after sort by last_event DESC.
    pub offset: Option<usize>,
    /// Only include sessions with activity at or after this timestamp (RFC 3339).
    pub since: Option<String>,
}

pub async fn list_sessions(
    State(state): State<SharedState>,
    Query(query): Query<SessionListQuery>,
) -> Json<Value> {
    let s = state.read().await;
    let all_rows = s.store.event_store.list_sessions()
        .await
        .unwrap_or_default();

    // Filter by `since` if provided (compare last_event timestamp strings lexicographically —
    // they're RFC 3339 so lexicographic order == chronological order).
    let filtered: Vec<&_> = if let Some(ref since) = query.since {
        all_rows.iter()
            .filter(|r| r.last_event.as_deref().unwrap_or("") >= since.as_str())
            .collect()
    } else {
        all_rows.iter().collect()
    };
    let total = filtered.len();

    // Apply offset/limit (sessions already sorted by last_event DESC from store)
    let offset = query.offset.unwrap_or(0);
    let page: Vec<&&_> = match query.limit {
        Some(limit) => filtered.iter().skip(offset).take(limit).collect(),
        None => filtered.iter().skip(offset).collect(),
    };

    log_event("api", &format!(
        "GET /api/sessions ({}/{} sessions, offset={}, limit={:?})",
        page.len(), total, offset, query.limit,
    ));

    // Build response from SessionRow + projections (no per-session event loading).
    // Detailed fields (tool_calls, files_edited, model, etc.) are available via
    // GET /api/sessions/{id}/summary when a specific session is selected.
    let mut result = Vec::new();
    for row in &page {
        let sid = row.id.as_str();
        let project_id = s.store.session_projects.get(sid).map(|r| r.value().clone());
        let project_name = s.store.session_project_names.get(sid).map(|r| r.value().clone());
        let (label, branch, total_input_tokens, total_output_tokens) =
            match s.store.projections.get(sid) {
                Some(proj) => (
                    proj.label().map(|s| s.to_string()),
                    proj.branch().map(|s| s.to_string()),
                    proj.total_input_tokens(),
                    proj.total_output_tokens(),
                ),
                None => (None, None, 0, 0),
            };
        // Derive status from last_event timestamp (stale if >5min old)
        let status = match row.last_event.as_deref() {
            Some(ts) => {
                if let Ok(t) = chrono::DateTime::parse_from_rfc3339(ts) {
                    if Utc::now().signed_duration_since(t).num_seconds() > 300 {
                        "completed"
                    } else {
                        "ongoing"
                    }
                } else {
                    "completed"
                }
            }
            None => "completed",
        };
        result.push(json!({
            "session_id": sid,
            "status": status,
            "start_time": row.first_event,
            "last_event": row.last_event,
            "event_count": row.event_count,
            "project_id": project_id.as_ref().or(row.project_id.as_ref()),
            "project_name": project_name.as_ref().or(row.project_name.as_ref()),
            "label": label.or(row.label.clone()),
            "branch": branch,
            "total_input_tokens": total_input_tokens,
            "total_output_tokens": total_output_tokens,
        }));
    }
    Json(json!({
        "sessions": result,
        "total": total,
    }))
}

pub async fn get_events(
    State(state): State<SharedState>,
    AxumPath(session_id): AxumPath<String>,
) -> Json<Value> {
    let s = state.read().await;
    let events = s.store.event_store.session_events(&session_id).await.unwrap_or_default();
    log_event("api", &format!("GET /api/sessions/{}/events ({} events)", short_id(&session_id), events.len()));
    Json(Value::Array(events))
}

pub async fn get_summary(
    State(state): State<SharedState>,
    AxumPath(session_id): AxumPath<String>,
) -> Json<Value> {
    log_event("api", &format!("GET /api/sessions/{}/summary", short_id(&session_id)));
    let s = state.read().await;
    let events = s.store.event_store.session_events(&session_id).await.unwrap_or_default();
    let summary = session_summary(&session_id, &events, Some(Utc::now()));
    let project_id = s.store.session_projects.get(&session_id).map(|r| r.value().clone());
    Json(json!({
        "session_id": summary.session_id,
        "status": summary.status,
        "start_time": summary.start_time,
        "duration_ms": summary.duration_ms,
        "event_count": summary.event_count,
        "error_count": summary.error_count,
        "tool_calls": summary.tool_calls,
        "files_edited": summary.files_edited,
        "unique_tools": summary.unique_tools,
        "exit_code": summary.exit_code,
        "model": summary.model,
        "prompt_count": summary.prompt_count,
        "response_count": summary.response_count,
        "project_id": project_id,
    }))
}

pub async fn get_activity(
    State(state): State<SharedState>,
    AxumPath(session_id): AxumPath<String>,
) -> Json<Value> {
    log_event("api", &format!("GET /api/sessions/{}/activity", short_id(&session_id)));
    let s = state.read().await;
    let events = s.store.event_store.session_events(&session_id).await.unwrap_or_default();
    let a = activity_summary(&events);
    Json(json!({
        "first_prompt": a.first_prompt,
        "files_touched": a.files_touched,
        "tool_breakdown": a.tool_breakdown,
        "error_messages": a.error_messages,
        "last_response": a.last_response,
        "conversation_turns": a.conversation_turns,
        "plan_count": a.plan_count,
        "duration_ms": a.duration_ms,
        "start_time": a.start_time,
    }))
}

pub async fn get_tools(
    State(state): State<SharedState>,
    AxumPath(session_id): AxumPath<String>,
) -> Json<Value> {
    let s = state.read().await;
    let events = s.store.event_store.session_events(&session_id).await.unwrap_or_default();
    let dist = tool_call_distribution(&events);
    Json(serde_json::to_value(dist).unwrap_or(json!({})))
}

#[derive(Deserialize)]
pub struct TranscriptQuery {
    #[serde(default)]
    pub assistant_only: bool,
}

pub async fn get_transcript(
    State(state): State<SharedState>,
    AxumPath(session_id): AxumPath<String>,
    Query(query): Query<TranscriptQuery>,
) -> Json<Value> {
    log_event("api", &format!("GET /api/sessions/{}/transcript", short_id(&session_id)));
    let s = state.read().await;
    let events = s.store.event_store.session_events(&session_id).await.unwrap_or_default();
    let transcript_path = find_transcript_path(&events);

    let data_dir = s.store.data_dir.clone();
    drop(s); // Release the read lock before doing file I/O

    let transcript_path = match transcript_path {
        Some(p) => {
            let path = Path::new(&p);
            // Reject paths containing traversal components
            let path_str = p.replace('\\', "/");
            if path_str.contains("..") {
                return Json(json!({
                    "error": "invalid transcript path",
                    "entries": []
                }));
            }
            if path.is_absolute() && path.exists() {
                p
            } else {
                // Try resolving relative to data_dir first
                let from_data = data_dir.join(&p);
                if let Ok(canonical) = from_data.canonicalize() {
                    if let Ok(canonical_data) = data_dir.canonicalize() {
                        if canonical.starts_with(&canonical_data) {
                            canonical.to_string_lossy().to_string()
                        } else {
                            return Json(json!({
                                "error": "transcript path outside data directory",
                                "entries": []
                            }));
                        }
                    } else {
                        p
                    }
                } else {
                    p
                }
            }
        }
        None => {
            // Fallback: reconstruct transcript from stored events.
            // Hermes sessions (and any agent that ingests via the plugin/watcher
            // path) don't have a transcript_path — the events ARE the transcript.
            let mut entries: Vec<Value> = Vec::new();
            for ev in &events {
                let raw = ev.get("data").and_then(|d| d.get("raw")).unwrap_or(ev);
                let data = raw.get("data").unwrap_or(raw);
                let role = data.get("role").and_then(|v| v.as_str()).unwrap_or("");
                if role.is_empty() {
                    continue;
                }
                let content = data.get("content").and_then(|v| v.as_str()).unwrap_or("");
                let subtype = ev.get("subtype").and_then(|v| v.as_str()).unwrap_or("");
                let kind = if subtype.contains("tool_use") {
                    "tool_call"
                } else if subtype.contains("tool_result") {
                    "tool_result"
                } else if subtype.contains("thinking") {
                    "thinking"
                } else {
                    "text"
                };
                let mut entry = json!({
                    "role": match role {
                        "tool" => "user",
                        _ => role,
                    },
                    "kind": kind,
                    "content": content,
                });
                // Add tool info if present
                if let Some(ap) = ev.get("data").and_then(|d| d.get("agent_payload")) {
                    if let Some(tool) = ap.get("tool").and_then(|v| v.as_str()) {
                        entry["tool"] = json!(tool);
                    }
                    if let Some(args) = ap.get("args") {
                        entry["args"] = args.clone();
                    }
                }
                if !query.assistant_only || (role == "assistant" && kind == "text") {
                    entries.push(entry);
                }
            }
            return Json(json!({
                "source": "events",
                "entries": entries,
            }));
        }
    };

    let mut entries = read_transcript(Path::new(&transcript_path));
    if query.assistant_only {
        entries.retain(|e| e.role == "assistant" && e.kind == "text");
    }

    Json(json!({
        "path": transcript_path,
        "entries": entries,
    }))
}

// ---------------------------------------------------------------------------
// View-model endpoints (typed records from open-story-views)
// ---------------------------------------------------------------------------

pub async fn get_view_records(
    State(state): State<SharedState>,
    AxumPath(session_id): AxumPath<String>,
) -> Json<Value> {
    log_event("api", &format!("GET /api/sessions/{}/view-records", short_id(&session_id)));
    let s = state.read().await;
    let events = s.store.event_store.session_events(&session_id).await.unwrap_or_default();

    let view_records: Vec<Value> = events
        .iter()
        .filter_map(|event| serde_json::from_value::<open_story_core::cloud_event::CloudEvent>(event.clone()).ok())
        .flat_map(|ce| open_story_views::from_cloud_event::from_cloud_event(&ce))
        .filter_map(|vr| serde_json::to_value(vr).ok())
        .collect();

    Json(Value::Array(view_records))
}

#[derive(Deserialize)]
pub struct ConversationQuery {
    /// Output format: json (default) or markdown
    #[serde(default)]
    pub format: Option<String>,
}

pub async fn get_conversation(
    State(state): State<SharedState>,
    AxumPath(session_id): AxumPath<String>,
    Query(query): Query<ConversationQuery>,
) -> axum::response::Response {
    let fmt = query.format.as_deref().unwrap_or("json");
    log_event("api", &format!("GET /api/sessions/{}/conversation?format={fmt}", short_id(&session_id)));
    let s = state.read().await;
    let events = s.store.event_store.session_events(&session_id).await.unwrap_or_default();

    let view_records: Vec<_> = events
        .iter()
        .filter_map(|event| serde_json::from_value::<open_story_core::cloud_event::CloudEvent>(event.clone()).ok())
        .flat_map(|ce| open_story_views::from_cloud_event::from_cloud_event(&ce))
        .collect();

    let paired = open_story_views::pair::pair_records(&view_records);

    match fmt {
        "markdown" | "md" => {
            let md = open_story_views::markdown::conversation_to_markdown(&paired, &session_id);
            axum::response::Response::builder()
                .header("content-type", "text/markdown; charset=utf-8")
                .body(axum::body::Body::from(md))
                .unwrap()
        }
        "html" => {
            let md = open_story_views::markdown::conversation_to_markdown(&paired, &session_id);
            let title = format!("Session {}", &session_id[..12.min(session_id.len())]);
            let html = open_story_views::html::markdown_to_html_page(&md, &title);
            axum::response::Response::builder()
                .header("content-type", "text/html; charset=utf-8")
                .body(axum::body::Body::from(html))
                .unwrap()
        }
        _ => {
            axum::response::Json(serde_json::to_value(paired).unwrap_or(json!({"entries": []}))).into_response()
        }
    }
}

pub async fn get_file_changes(
    State(state): State<SharedState>,
    AxumPath(session_id): AxumPath<String>,
) -> Json<Value> {
    log_event("api", &format!("GET /api/sessions/{}/file-changes", short_id(&session_id)));
    let s = state.read().await;
    let events = s.store.event_store.session_events(&session_id).await.unwrap_or_default();

    let view_records: Vec<_> = events
        .iter()
        .filter_map(|event| serde_json::from_value::<open_story_core::cloud_event::CloudEvent>(event.clone()).ok())
        .flat_map(|ce| open_story_views::from_cloud_event::from_cloud_event(&ce))
        .collect();

    let edits: Vec<Value> = open_story_views::filter::file_edits(&view_records)
        .into_iter()
        .filter_map(|vr| serde_json::to_value(vr).ok())
        .collect();

    Json(Value::Array(edits))
}

pub async fn get_tool_schemas() -> Json<Value> {
    Json(schemas_to_json())
}

/// GET /api/sessions/{session_id}/meta
///
/// Returns cached projection metadata: event_count and filter_counts.
/// O(1) — reads from the projection cache, never iterates rows.
pub async fn get_session_meta(
    State(state): State<SharedState>,
    AxumPath(session_id): AxumPath<String>,
) -> Result<Json<Value>, StatusCode> {
    log_event("api", &format!("GET /api/sessions/{}/meta", short_id(&session_id)));
    let s = state.read().await;
    let proj = s.store.projections.get(&session_id).ok_or(StatusCode::NOT_FOUND)?;
    let meta = proj.query_meta();
    Ok(Json(json!({
        "event_count": meta.event_count,
        "filter_counts": meta.filter_counts,
    })))
}

/// GET /api/sessions/{session_id}/events/{event_id}/content
///
/// Returns the full (untruncated) payload for a truncated record.
/// Returns 404 if the session/event doesn't exist or wasn't truncated.
pub async fn get_event_content(
    State(state): State<SharedState>,
    AxumPath((session_id, event_id)): AxumPath<(String, String)>,
) -> Result<String, StatusCode> {
    log_event("api", &format!(
        "GET /api/sessions/{}/events/{}/content",
        short_id(&session_id), short_id(&event_id)
    ));
    let s = state.read().await;
    // Try in-memory cache first, then fall back to EventStore.
    // Key is (session_id, event_id) — the DashMap guard derefs to `&String`.
    if let Some(entry) = s
        .store
        .full_payloads
        .get(&(session_id.clone(), event_id.clone()))
    {
        return Ok(entry.value().clone());
    }
    // Fall back: extract tool output from full event payload in EventStore
    let payload = s
        .store
        .event_store
        .full_payload(&event_id)
        .await
        .ok()
        .flatten()
        .ok_or(StatusCode::NOT_FOUND)?;
    let val: Value = serde_json::from_str(&payload).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    // Extract tool_result content from the CloudEvent
    let output = val
        .pointer("/data/raw/message/content")
        .and_then(|c| c.as_array())
        .and_then(|blocks| {
            blocks.iter().find_map(|b| {
                if b.get("type").and_then(|t| t.as_str()) == Some("tool_result") {
                    b.get("content").and_then(|c| c.as_str()).map(|s| s.to_string())
                } else {
                    None
                }
            })
        })
        .or_else(|| val.pointer("/data/output").and_then(|v| v.as_str()).map(|s| s.to_string()))
        .ok_or(StatusCode::NOT_FOUND)?;
    Ok(output)
}

#[derive(Deserialize)]
pub struct PatternQuery {
    #[serde(rename = "type")]
    pub pattern_type: Option<String>,
}

/// GET /api/sessions/{session_id}/patterns
///
/// Returns all detected patterns for a session. Optional `?type=` query
/// parameter filters by pattern_type (e.g., `?type=git.workflow`).
pub async fn get_patterns(
    State(state): State<SharedState>,
    AxumPath(session_id): AxumPath<String>,
    Query(query): Query<PatternQuery>,
) -> Json<Value> {
    log_event("api", &format!("GET /api/sessions/{}/patterns", short_id(&session_id)));
    let s = state.read().await;
    let result = s
        .store
        .event_store
        .session_patterns(&session_id, query.pattern_type.as_deref())
        .await
        .unwrap_or_default();
    Json(json!({ "patterns": result }))
}

pub async fn get_turns(
    State(state): State<SharedState>,
    AxumPath(session_id): AxumPath<String>,
) -> Json<Value> {
    log_event("api", &format!("GET /api/sessions/{}/turns", short_id(&session_id)));
    let s = state.read().await;
    let turns = s
        .store
        .event_store
        .session_turns(&session_id)
        .await
        .unwrap_or_default();
    Json(json!({ "turns": turns }))
}

pub async fn list_plans(State(state): State<SharedState>) -> Json<Value> {
    let s = state.read().await;
    let plans: Vec<Value> = s
        .store.plan_store
        .list_plans()
        .iter()
        .map(|p| {
            json!({
                "id": p.id,
                "session_id": p.session_id,
                "title": p.title,
                "timestamp": p.timestamp,
            })
        })
        .collect();
    Json(Value::Array(plans))
}

pub async fn get_plan(
    State(state): State<SharedState>,
    AxumPath(plan_id): AxumPath<String>,
) -> Result<Json<Value>, StatusCode> {
    let s = state.read().await;
    match s.store.plan_store.load(&plan_id) {
        Some(plan) => Ok(Json(serde_json::to_value(plan).unwrap_or(json!({})))),
        None => Err(StatusCode::NOT_FOUND),
    }
}

pub async fn get_session_plans(
    State(state): State<SharedState>,
    AxumPath(session_id): AxumPath<String>,
) -> Json<Value> {
    let s = state.read().await;
    let mut all_plans = s.store.plan_store.list_for_session(&session_id);
    // Include plans from subagent sessions. DashMap::get returns a Ref
    // guard; deref to &Vec<String> for iteration.
    if let Some(children_ref) = s.store.session_children.get(&session_id) {
        for child_id in children_ref.value() {
            all_plans.extend(s.store.plan_store.list_for_session(child_id));
        }
    }
    // Sort by timestamp desc
    all_plans.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));

    let plans: Vec<Value> = all_plans
        .iter()
        .map(|p| {
            json!({
                "id": p.id,
                "session_id": p.session_id,
                "title": p.title,
                "timestamp": p.timestamp,
            })
        })
        .collect();
    Json(Value::Array(plans))
}

// ── Query API endpoints (Phase B3) ──────────────────────────────────

/// GET /api/sessions/{session_id}/synopsis
pub async fn get_session_synopsis(
    State(state): State<SharedState>,
    AxumPath(session_id): AxumPath<String>,
) -> Result<Json<Value>, StatusCode> {
    log_event("api", &format!("GET /api/sessions/{}/synopsis", short_id(&session_id)));
    let s = state.read().await;
    let synopsis = s.store.event_store.query_session_synopsis(&session_id)
        .await
        .ok_or(StatusCode::NOT_FOUND)?;
    Ok(Json(serde_json::to_value(synopsis).unwrap_or(json!({}))))
}

/// GET /api/sessions/{session_id}/tool-journey
pub async fn get_tool_journey(
    State(state): State<SharedState>,
    AxumPath(session_id): AxumPath<String>,
) -> Json<Value> {
    log_event("api", &format!("GET /api/sessions/{}/tool-journey", short_id(&session_id)));
    let s = state.read().await;
    let journey = s.store.event_store.query_tool_journey(&session_id).await;
    Json(serde_json::to_value(journey).unwrap_or(json!([])))
}

/// GET /api/sessions/{session_id}/file-impact
pub async fn get_file_impact(
    State(state): State<SharedState>,
    AxumPath(session_id): AxumPath<String>,
) -> Json<Value> {
    log_event("api", &format!("GET /api/sessions/{}/file-impact", short_id(&session_id)));
    let s = state.read().await;
    let impact = s.store.event_store.query_file_impact(&session_id).await;
    Json(serde_json::to_value(impact).unwrap_or(json!([])))
}

/// GET /api/sessions/{session_id}/errors
pub async fn get_session_errors(
    State(state): State<SharedState>,
    AxumPath(session_id): AxumPath<String>,
) -> Json<Value> {
    log_event("api", &format!("GET /api/sessions/{}/errors", short_id(&session_id)));
    let s = state.read().await;
    let errors = s.store.event_store.query_session_errors(&session_id).await;
    Json(serde_json::to_value(errors).unwrap_or(json!([])))
}

#[derive(Deserialize)]
pub struct DaysQuery {
    #[serde(default = "default_days")]
    pub days: u32,
}

fn default_days() -> u32 {
    7
}

/// GET /api/insights/pulse?days=7
pub async fn get_project_pulse(
    State(state): State<SharedState>,
    Query(query): Query<DaysQuery>,
) -> Json<Value> {
    log_event("api", &format!("GET /api/insights/pulse?days={}", query.days));
    let s = state.read().await;
    let pulse = s.store.event_store.query_project_pulse(query.days).await;
    Json(serde_json::to_value(pulse).unwrap_or(json!([])))
}

#[derive(Deserialize)]
pub struct EvolutionQuery {
    #[serde(default = "default_evolution_days")]
    pub days: u32,
}

fn default_evolution_days() -> u32 {
    30
}

/// GET /api/insights/tool-evolution?days=30
pub async fn get_tool_evolution(
    State(state): State<SharedState>,
    Query(query): Query<EvolutionQuery>,
) -> Json<Value> {
    log_event("api", &format!("GET /api/insights/tool-evolution?days={}", query.days));
    let s = state.read().await;
    let evolution = s.store.event_store.query_tool_evolution(query.days).await;
    Json(serde_json::to_value(evolution).unwrap_or(json!([])))
}

/// GET /api/insights/efficiency
pub async fn get_session_efficiency_insights(
    State(state): State<SharedState>,
) -> Json<Value> {
    log_event("api", "GET /api/insights/efficiency");
    let s = state.read().await;
    let efficiency = s.store.event_store.query_session_efficiency().await;
    Json(serde_json::to_value(efficiency).unwrap_or(json!([])))
}

#[derive(Deserialize)]
pub struct ProjectQuery {
    pub project: String,
}

/// GET /api/agent/project-context?project=X
pub async fn get_agent_project_context(
    State(state): State<SharedState>,
    Query(query): Query<ProjectQuery>,
) -> Json<Value> {
    log_event("api", &format!("GET /api/agent/project-context?project={}", query.project));
    let s = state.read().await;
    let context = s.store.event_store.query_project_context(&query.project, 5).await;
    Json(serde_json::to_value(context).unwrap_or(json!([])))
}

/// GET /api/agent/recent-files?project=X
pub async fn get_agent_recent_files(
    State(state): State<SharedState>,
    Query(query): Query<ProjectQuery>,
) -> Json<Value> {
    log_event("api", &format!("GET /api/agent/recent-files?project={}", query.project));
    let s = state.read().await;
    let files = s.store.event_store.query_recent_files(&query.project, 5).await;
    Json(serde_json::to_value(files).unwrap_or(json!([])))
}

#[derive(Deserialize)]
pub struct ProductivityQuery {
    #[serde(default = "default_evolution_days")]
    pub days: u32,
}

/// GET /api/insights/productivity?days=30
pub async fn get_productivity(
    State(state): State<SharedState>,
    Query(query): Query<ProductivityQuery>,
) -> Json<Value> {
    log_event("api", &format!("GET /api/insights/productivity?days={}", query.days));
    let s = state.read().await;
    let hourly = s.store.event_store.query_productivity_by_hour(query.days).await;
    Json(serde_json::to_value(hourly).unwrap_or(json!([])))
}

// ── Token Usage ──────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct TokenUsageQuery {
    /// Filter to last N days
    pub days: Option<u32>,
    /// Filter to a single session
    pub session_id: Option<String>,
    /// Pricing model: sonnet (default), opus, haiku
    #[serde(default = "default_pricing_model")]
    pub model: String,
}

fn default_pricing_model() -> String {
    "sonnet".to_string()
}

/// GET /api/insights/token-usage?days=7&model=sonnet
///
/// Returns token usage summary with cost estimates.
/// Includes per-session breakdown sorted by output tokens.
pub async fn get_token_usage(
    State(state): State<SharedState>,
    Query(query): Query<TokenUsageQuery>,
) -> Json<Value> {
    log_event("api", &format!(
        "GET /api/insights/token-usage?days={:?}&session_id={:?}&model={}",
        query.days, query.session_id, query.model
    ));
    let s = state.read().await;
    let result = s.store.event_store.query_token_usage(
        query.days,
        query.session_id.as_deref(),
        &query.model,
    ).await;
    Json(serde_json::to_value(result).unwrap_or(json!({})))
}

/// GET /api/insights/token-usage/daily?days=30
///
/// Returns daily token usage trend.
pub async fn get_daily_token_usage(
    State(state): State<SharedState>,
    Query(query): Query<DaysQuery>,
) -> Json<Value> {
    log_event("api", &format!("GET /api/insights/token-usage/daily?days={}", query.days));
    let s = state.read().await;
    let result = s.store.event_store.query_daily_token_usage(Some(query.days)).await;
    Json(serde_json::to_value(result).unwrap_or(json!([])))
}

// ── Agent Tool Definitions (Phase B5) ────────────────────────────────

/// GET /api/agent/tools
///
/// Returns tool definitions for the agentic query endpoints.
/// Agents can discover these tools and call the corresponding endpoints.
/// Format follows MCP-style tool definitions with JSON Schema parameters.
pub async fn get_agent_tools() -> Json<Value> {
    log_event("api", "GET /api/agent/tools");
    Json(json!([
        {
            "name": "project_context",
            "description": "Get the last 5 sessions for a project. Use this to pick up where the last agent left off.",
            "endpoint": "/api/agent/project-context",
            "method": "GET",
            "parameters": {
                "type": "object",
                "properties": {
                    "project": { "type": "string", "description": "Project ID to query" }
                },
                "required": ["project"]
            }
        },
        {
            "name": "recent_files",
            "description": "Get files modified in recent sessions for a project. Focus on active files, not the whole repo.",
            "endpoint": "/api/agent/recent-files",
            "method": "GET",
            "parameters": {
                "type": "object",
                "properties": {
                    "project": { "type": "string", "description": "Project ID to query" }
                },
                "required": ["project"]
            }
        },
        {
            "name": "session_synopsis",
            "description": "Get a synopsis of a session: goal, journey, outcome, top tools, error count.",
            "endpoint": "/api/sessions/{session_id}/synopsis",
            "method": "GET",
            "parameters": {
                "type": "object",
                "properties": {
                    "session_id": { "type": "string", "description": "Session ID" }
                },
                "required": ["session_id"]
            }
        },
        {
            "name": "tool_journey",
            "description": "Get the sequence of tools used in a session with file targets. Understand the agent's strategy.",
            "endpoint": "/api/sessions/{session_id}/tool-journey",
            "method": "GET",
            "parameters": {
                "type": "object",
                "properties": {
                    "session_id": { "type": "string", "description": "Session ID" }
                },
                "required": ["session_id"]
            }
        },
        {
            "name": "file_impact",
            "description": "Get files read vs. written in a session. Understand the blast radius of changes.",
            "endpoint": "/api/sessions/{session_id}/file-impact",
            "method": "GET",
            "parameters": {
                "type": "object",
                "properties": {
                    "session_id": { "type": "string", "description": "Session ID" }
                },
                "required": ["session_id"]
            }
        },
        {
            "name": "project_pulse",
            "description": "Get activity per project over the last N days. See which projects are actively being worked on.",
            "endpoint": "/api/insights/pulse",
            "method": "GET",
            "parameters": {
                "type": "object",
                "properties": {
                    "days": { "type": "integer", "description": "Days to look back (default: 7)", "default": 7 }
                }
            }
        },
        {
            "name": "session_errors",
            "description": "Get errors from a session with timestamps. Understand what went wrong and when.",
            "endpoint": "/api/sessions/{session_id}/errors",
            "method": "GET",
            "parameters": {
                "type": "object",
                "properties": {
                    "session_id": { "type": "string", "description": "Session ID" }
                },
                "required": ["session_id"]
            }
        },
        {
            "name": "productivity_by_hour",
            "description": "Get event density by hour of day. Understand when deep agent work happens.",
            "endpoint": "/api/insights/productivity",
            "method": "GET",
            "parameters": {
                "type": "object",
                "properties": {
                    "days": { "type": "integer", "description": "Days to look back (default: 30)", "default": 30 }
                }
            }
        },
        {
            "name": "token_usage",
            "description": "Get token usage and estimated cost across all sessions. Shows input/output/cache tokens and cost breakdown. Filter by days or session_id. Returns per-session breakdown sorted by output tokens.",
            "endpoint": "/api/insights/token-usage",
            "method": "GET",
            "parameters": {
                "type": "object",
                "properties": {
                    "days": { "type": "integer", "description": "Only include last N days" },
                    "session_id": { "type": "string", "description": "Filter to a single session" },
                    "model": { "type": "string", "description": "Pricing model: sonnet (default), opus, haiku", "default": "sonnet" }
                }
            }
        },
        {
            "name": "daily_token_usage",
            "description": "Get daily token usage trend — how many tokens were used each day with cost estimates.",
            "endpoint": "/api/insights/token-usage/daily",
            "method": "GET",
            "parameters": {
                "type": "object",
                "properties": {
                    "days": { "type": "integer", "description": "Days to look back (default: 7)", "default": 7 }
                }
            }
        },
        {
            "name": "search",
            "description": "Full-text search across past sessions. Find how previous agents approached similar problems, what files they changed, and what strategies worked. Returns sessions ranked by relevance with matching event snippets. Use synopsis and tool_journey on the returned session IDs for deeper investigation.",
            "endpoint": "/api/agent/search",
            "method": "GET",
            "parameters": {
                "type": "object",
                "properties": {
                    "q": { "type": "string", "description": "Natural language search query" },
                    "project": { "type": "string", "description": "Optional project ID to filter results" },
                    "days": { "type": "integer", "description": "Days to look back (default: 30)", "default": 30 },
                    "limit": { "type": "integer", "description": "Max sessions to return (default: 5)", "default": 5 }
                },
                "required": ["q"]
            }
        }
    ]))
}

// ── FTS5 search endpoint ────────────────────────────────────────────

#[derive(Deserialize)]
pub struct SearchQuery {
    pub q: Option<String>,
    #[serde(default = "default_search_limit")]
    pub limit: usize,
    pub session_id: Option<String>,
}

fn default_search_limit() -> usize {
    20
}

/// GET /api/search?q=<query>&limit=20&session_id=<optional>
///
/// Full-text search over indexed events using SQLite FTS5.
pub async fn search_events(
    State(state): State<SharedState>,
    Query(query): Query<SearchQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let q = match &query.q {
        Some(q) if !q.trim().is_empty() => q.trim().to_string(),
        _ => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "missing or empty 'q' parameter"})),
            ));
        }
    };

    log_event(
        "api",
        &format!("GET /api/search?q={}", crate::logging::truncate_at_char_boundary(&q, 50)),
    );

    let s = state.read().await;
    let results = s.store.event_store
        .search_fts(&q, query.limit, query.session_id.as_deref())
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("search failed: {e}")})),
            )
        })?;

    Ok(Json(serde_json::to_value(results).unwrap_or(json!([]))))
}

// ── Agentic search endpoint ──────────────────────────────────────────

#[derive(Deserialize)]
pub struct AgentSearchQuery {
    pub q: Option<String>,
    #[serde(default = "default_agent_search_limit")]
    pub limit: usize,
    pub project: Option<String>,
    #[serde(default = "default_agent_search_days")]
    pub days: u32,
}

fn default_agent_search_limit() -> usize {
    5
}

fn default_agent_search_days() -> u32 {
    30
}

/// GET /api/agent/search?q=<query>&project=<optional>&days=30&limit=5
///
/// Session-grouped full-text search for agents. Returns sessions ranked by
/// relevance with matching event snippets and pointers to synopsis/journey
/// endpoints for deeper investigation.
pub async fn agent_search(
    State(state): State<SharedState>,
    Query(query): Query<AgentSearchQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let q = match &query.q {
        Some(q) if !q.trim().is_empty() => q.trim().to_string(),
        _ => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "missing or empty 'q' parameter"})),
            ));
        }
    };

    log_event(
        "api",
        &format!(
            "GET /api/agent/search?q={}{}",
            &q[..q.len().min(50)],
            query.project.as_ref().map(|p| format!("&project={p}")).unwrap_or_default()
        ),
    );

    let s = state.read().await;

    // Search with a higher event limit — we'll group by session
    let event_limit = query.limit * 10;
    let results = s.store.event_store
        .search_fts(&q, event_limit, None)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("search failed: {e}")})),
            )
        })?;

    // Collect project filter info
    let project_filter = query.project.clone();
    let session_projects = s.store.session_projects.clone();
    let session_project_names = s.store.session_project_names.clone();

    // Collect session metadata for enrichment. DashMap iteration gives
    // RefMulti guards; `.key()` and `.value()` extract the pair.
    let mut session_labels: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let mut session_event_counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for entry in s.store.projections.iter() {
        if let Some(label) = entry.value().label() {
            session_labels.insert(entry.key().clone(), label.to_string());
        }
        session_event_counts.insert(entry.key().clone(), entry.value().event_count());
    }

    // Group results by session
    let mut session_groups: std::collections::HashMap<String, Vec<&open_story_store::queries::FtsSearchResult>> =
        std::collections::HashMap::new();
    for result in &results {
        session_groups
            .entry(result.session_id.clone())
            .or_default()
            .push(result);
    }

    // Build session-level results
    let mut session_results: Vec<Value> = session_groups
        .into_iter()
        .filter_map(|(sid, events)| {
            // Project filter: skip sessions not matching the requested project
            if let Some(ref proj) = project_filter {
                let session_project = session_projects.get(&sid)?;
                if !session_project.value().contains(proj) {
                    return None;
                }
            }

            // Session-level relevance = min rank (FTS5 rank is negative, more negative = more relevant)
            let best_rank = events.iter().map(|e| e.rank).fold(0.0f64, f64::min);

            let matching_events: Vec<Value> = events
                .iter()
                .take(3)
                .map(|e| {
                    json!({
                        "event_id": e.event_id,
                        "rank": e.rank,
                        "snippet": e.snippet,
                        "record_type": e.record_type,
                    })
                })
                .collect();

            let project_name = session_project_names.get(&sid).map(|r| r.value().clone());
            let project_id = session_projects.get(&sid).map(|r| r.value().clone());
            let label = session_labels.get(&sid);
            let event_count = session_event_counts.get(&sid).copied().unwrap_or(0);

            Some(json!({
                "session_id": sid,
                "label": label,
                "project_id": project_id,
                "project_name": project_name,
                "event_count": event_count,
                "relevance_rank": best_rank,
                "matching_events": matching_events,
                "synopsis_url": format!("/api/sessions/{sid}/synopsis"),
                "tool_journey_url": format!("/api/sessions/{sid}/tool-journey"),
            }))
        })
        .collect();

    // Sort by rank (more negative = more relevant, so ascending sort)
    session_results.sort_by(|a, b| {
        let rank_a = a["relevance_rank"].as_f64().unwrap_or(0.0);
        let rank_b = b["relevance_rank"].as_f64().unwrap_or(0.0);
        rank_a.partial_cmp(&rank_b).unwrap_or(std::cmp::Ordering::Equal)
    });

    session_results.truncate(query.limit);

    Ok(Json(json!({
        "query": q,
        "results": session_results,
        "total_events_searched": results.len(),
    })))
}

// ── Records endpoint (WireRecords from projections) ─────────────────

/// GET /api/sessions/{session_id}/records
///
/// Returns session events as WireRecords read directly from the EventStore.
///
/// This is the same format the Timeline renders — includes depth,
/// parent_uuid, and truncation metadata. Returns empty array if the
/// session has no events.
///
/// Reads from `event_store.session_events()` (the single source of truth)
/// rather than any in-memory cache, so any event persisted to the store
/// is visible here regardless of which ingest path wrote it.
pub async fn get_session_records(
    State(state): State<SharedState>,
    AxumPath(session_id): AxumPath<String>,
) -> Json<Value> {
    use open_story_views::from_cloud_event::from_cloud_event;
    use open_story_views::unified::RecordBody;
    use open_story_views::wire_record::{truncate_payload, WireRecord, TRUNCATION_THRESHOLD};

    log_event("api", &format!("GET /api/sessions/{}/records", short_id(&session_id)));
    let s = state.read().await;

    let events = s
        .store
        .event_store
        .session_events(&session_id)
        .await
        .unwrap_or_default();

    // Build parent_map from raw events — one entry per stored CloudEvent.
    // Fan-out ViewRecords (e.g., parallel tool_use blocks) inherit the
    // same parent_uuid via suffix stripping at lookup time.
    let mut parent_map: HashMap<String, Option<String>> = HashMap::with_capacity(events.len());
    for event in &events {
        let id = match event.get("id").and_then(|v| v.as_str()) {
            Some(id) => id.to_string(),
            None => continue,
        };
        let parent = event
            .get("data")
            .and_then(|d| d.get("agent_payload"))
            .and_then(|ap| ap.get("parent_uuid"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        parent_map.insert(id, parent);
    }

    // Depth: walk the parent chain. Capped at 64 to bound cost on
    // pathological inputs (production trees are shallow).
    fn depth_of(id: &str, parent_map: &HashMap<String, Option<String>>) -> u16 {
        // Strip fan-out suffix: "evt-1:2" → "evt-1"
        let base_id = id.split(':').next().unwrap_or(id);
        let mut depth: u16 = 0;
        let mut current = match parent_map.get(base_id).and_then(|p| p.as_deref()) {
            Some(p) => p.to_string(),
            None => return 0,
        };
        for _ in 0..64 {
            depth += 1;
            match parent_map.get(current.as_str()).and_then(|p| p.as_deref()) {
                Some(next) => current = next.to_string(),
                None => return depth,
            }
        }
        depth
    }

    let mut records: Vec<Value> = Vec::new();
    for event in &events {
        let ce = match serde_json::from_value::<open_story_core::cloud_event::CloudEvent>(event.clone()) {
            Ok(ce) => ce,
            Err(_) => continue,
        };
        for vr in from_cloud_event(&ce) {
            // Parent lookup uses base id (strip fan-out suffix).
            let base_id = vr.id.split(':').next().unwrap_or(&vr.id).to_string();
            let parent_uuid = parent_map
                .get(&base_id)
                .and_then(|p| p.clone());
            let depth = depth_of(&vr.id, &parent_map);

            // Truncation: same rule as the pre-refactor to_wire_record.
            let (truncated, payload_bytes) = match &vr.body {
                RecordBody::ToolResult(tr) => match &tr.output {
                    Some(output) => {
                        let result = truncate_payload(output, TRUNCATION_THRESHOLD);
                        (result.truncated, result.original_bytes as u64)
                    }
                    None => (false, 0),
                },
                _ => (false, 0),
            };

            let wire = WireRecord {
                record: vr,
                depth,
                parent_uuid,
                truncated,
                payload_bytes,
            };
            if let Ok(v) = serde_json::to_value(wire) {
                records.push(v);
            }
        }
    }

    // Events come out of session_events() sorted by (seq, time) per the
    // store contract, which matches the view_records fan-out order.
    Json(Value::Array(records))
}

// ── Session Lifecycle endpoints (Phase A4) ──────────────────────────

/// DELETE /api/sessions/{session_id}
///
/// Removes a session and all its events, patterns, and plans from SQLite.
/// Also clears in-memory projections and caches.
pub async fn delete_session(
    State(state): State<SharedState>,
    AxumPath(session_id): AxumPath<String>,
) -> Result<Json<Value>, StatusCode> {
    log_event("api", &format!("DELETE /api/sessions/{}", short_id(&session_id)));
    let mut s = state.write().await;

    let deleted = s.store.event_store.delete_session(&session_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if deleted == 0 && !s.store.projections.contains_key(&session_id) {
        return Err(StatusCode::NOT_FOUND);
    }

    // Clean up in-memory state
    s.store.projections.remove(&session_id);
    s.store.detected_patterns.remove(&session_id);
    // full_payloads is keyed on (session_id, event_id) — walk to prune.
    let to_drop: Vec<(String, String)> = s
        .store
        .full_payloads
        .iter()
        .filter_map(|e| {
            if e.key().0 == session_id {
                Some(e.key().clone())
            } else {
                None
            }
        })
        .collect();
    for k in to_drop {
        s.store.full_payloads.remove(&k);
    }
    s.store.session_projects.remove(&session_id);
    s.store.session_project_names.remove(&session_id);

    Ok(Json(json!({
        "status": "deleted",
        "session_id": session_id,
        "events_deleted": deleted,
    })))
}

/// GET /api/sessions/{session_id}/export
///
/// Returns all events for a session as newline-delimited JSON (JSONL).
/// Content-Type: application/x-ndjson for proper JSONL handling.
pub async fn export_session(
    State(state): State<SharedState>,
    AxumPath(session_id): AxumPath<String>,
) -> Result<(StatusCode, [(axum::http::header::HeaderName, &'static str); 1], String), StatusCode> {
    log_event("api", &format!("GET /api/sessions/{}/export", short_id(&session_id)));
    let s = state.read().await;

    let jsonl = s.store.event_store.export_session_jsonl(&session_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if jsonl.is_empty() {
        return Err(StatusCode::NOT_FOUND);
    }

    Ok((
        StatusCode::OK,
        [(axum::http::header::CONTENT_TYPE, "application/x-ndjson")],
        jsonl,
    ))
}
