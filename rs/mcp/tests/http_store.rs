//! Parity tests for `HttpEventStore`.
//!
//! These stand up a tiny axum mock that returns the *exact* JSON shapes
//! the real `open-story-server` handlers emit (each route is annotated
//! with the rs/server/src/api.rs handler it mirrors), then assert that
//! `HttpEventStore` maps every response back into the typed struct the
//! MCP tools consume. The parity that matters is "the client decodes the
//! server's wire shape correctly" — envelope unwrapping, the
//! `session_id`/`start_time` → `SessionRow` remap, and 404 → `None`.
//!
//! BDD: scenario(given a server returning shape X, when the store calls
//! method M, then it yields the typed value Y).

use axum::extract::Path;
use axum::routing::get;
use axum::{Json, Router};
use open_story_mcp::http_store::HttpEventStore;
use open_story_store::event_store::EventStore;
use serde_json::{json, Value};
use std::net::SocketAddr;
use tokio::net::TcpListener;

/// Boot the mock router on an ephemeral port; return its base URL.
async fn spawn_mock(router: Router) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr: SocketAddr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    format!("http://{addr}")
}

/// The full mock surface — every route mirrors one api.rs handler.
fn mock_router() -> Router {
    Router::new()
        // api.rs:40 list_sessions → {"sessions":[trimmed], "total":N}
        .route(
            "/api/sessions",
            get(|| async {
                Json(json!({
                    "sessions": [
                        {
                            "session_id": "s-newer",
                            "status": "ongoing",
                            "start_time": "2026-06-13T10:00:00.000Z",
                            "last_event": "2026-06-13T11:00:00.000Z",
                            "event_count": 42,
                            "project_id": "proj-a",
                            "project_name": "Project A",
                            "label": "newer session"
                        },
                        {
                            "session_id": "s-older",
                            "status": "complete",
                            "start_time": "2026-06-01T09:00:00.000Z",
                            "last_event": "2026-06-01T09:30:00.000Z",
                            "event_count": 7,
                            "project_id": "proj-b",
                            "project_name": null,
                            "label": null
                        }
                    ],
                    "total": 2
                }))
            }),
        )
        // api.rs:405 get_events → Json(Array)
        .route(
            "/api/sessions/{id}/events",
            get(|Path(id): Path<String>| async move {
                Json(json!([
                    {"id": "e1", "time": "2026-06-13T10:00:00Z",
                     "data": {"raw": {"data": {"role": "user", "content": "hi"}}}},
                    {"id": "e2", "time": "2026-06-13T10:01:00Z", "session": id,
                     "data": {"raw": {"data": {"role": "assistant", "content": "hello"}}}}
                ]))
            }),
        )
        // api.rs:762 get_patterns → {"patterns":[...]}
        .route(
            "/api/sessions/{id}/patterns",
            get(|Path(_id): Path<String>| async {
                Json(json!({
                    "patterns": [
                        {
                            "pattern_type": "turn.sentence",
                            "session_id": "s",
                            "summary": "edited file",
                            "started_at": "2026-06-13T10:00:00Z",
                            "ended_at": "2026-06-13T10:00:05Z",
                            "event_ids": ["e1"],
                            "metadata": {"verb": "edit", "object": "file.rs",
                                         "human_prompt": "fix the bug"}
                        }
                    ]
                }))
            }),
        )
        // api.rs:855 get_session_synopsis → bare SessionSynopsis
        .route(
            "/api/sessions/{id}/synopsis",
            get(|Path(id): Path<String>| async move {
                if id == "missing" {
                    // Real handler 404s unknown sessions; mock null body is
                    // the other "no synopsis" path. Both → None.
                    return Json(Value::Null);
                }
                Json(json!({
                    "session_id": id,
                    "label": "demo",
                    "project_id": "proj-a",
                    "project_name": "Project A",
                    "event_count": 42,
                    "tool_count": 10,
                    "error_count": 1,
                    "first_event": "2026-06-13T10:00:00Z",
                    "last_event": "2026-06-13T11:00:00Z",
                    "duration_secs": 3600,
                    "top_tools": [{"tool": "Edit", "count": 6}, {"tool": "Bash", "count": 4}]
                }))
            }),
        )
        // api.rs:868 get_tool_journey → bare Vec<ToolStep>
        .route(
            "/api/sessions/{id}/tool-journey",
            get(|Path(_id): Path<String>| async {
                Json(json!([
                    {"tool": "Read", "file": "a.rs", "timestamp": "2026-06-13T10:00:00Z"},
                    {"tool": "Edit", "file": "b.rs", "timestamp": "2026-06-13T10:01:00Z"}
                ]))
            }),
        )
        // api.rs:879 get_file_impact → bare Vec<FileImpact>
        .route(
            "/api/sessions/{id}/file-impact",
            get(|Path(_id): Path<String>| async {
                Json(json!([{"file": "a.rs", "reads": 3, "writes": 1}]))
            }),
        )
        // api.rs:890 get_session_errors → bare Vec<SessionError>
        .route(
            "/api/sessions/{id}/errors",
            get(|Path(_id): Path<String>| async {
                Json(json!([{"timestamp": "2026-06-13T10:05:00Z", "message": "boom"}]))
            }),
        )
        // api.rs:822 get_session_plans → bare array {id,session_id,title,timestamp}
        .route(
            "/api/sessions/{id}/plans",
            get(|Path(id): Path<String>| async move {
                Json(json!([
                    {"id": "p1", "session_id": id, "title": "Refactor plan",
                     "timestamp": "2026-06-13T10:00:00Z"}
                ]))
            }),
        )
        // /api/insights/pulse → bare Vec<ProjectPulse>
        .route(
            "/api/insights/pulse",
            get(|| async {
                Json(json!([{
                    "project_id": "proj-a", "project_name": "Project A",
                    "session_count": 3, "event_count": 120,
                    "last_activity": "2026-06-13T11:00:00Z"
                }]))
            }),
        )
        // /api/insights/productivity → bare Vec<HourlyActivity>
        .route(
            "/api/insights/productivity",
            get(|| async { Json(json!([{"hour": 10, "event_count": 50}])) }),
        )
        // /api/insights/token-usage → bare TokenUsageSummary
        .route(
            "/api/insights/token-usage",
            get(|| async {
                // TokenUsage is #[serde(flatten)]'d into the summary, so the
                // token fields sit at top level (api.rs serializes the same).
                Json(json!({
                    "session_count": 2,
                    "input_tokens": 100, "output_tokens": 200,
                    "cache_read_tokens": 10, "cache_creation_tokens": 5,
                    "message_count": 3, "total_tokens": 315,
                    "cost": {"input": 0.1, "output": 0.2, "cache_read": 0.01,
                             "cache_creation": 0.005, "total": 0.315, "model": "opus"},
                    "sessions": []
                }))
            }),
        )
        // /api/insights/token-usage/daily → bare Vec<DailyTokenUsage> (flattened usage)
        .route(
            "/api/insights/token-usage/daily",
            get(|| async {
                Json(json!([{
                    "date": "2026-06-13",
                    "input_tokens": 100, "output_tokens": 200,
                    "cache_read_tokens": 10, "cache_creation_tokens": 5,
                    "message_count": 3, "total_tokens": 315
                }]))
            }),
        )
        // /api/agent/project-context → bare Vec<ProjectSession>
        .route(
            "/api/agent/project-context",
            get(|| async {
                Json(json!([{
                    "session_id": "s-newer", "label": "newer session",
                    "event_count": 42, "last_event": "2026-06-13T11:00:00Z"
                }]))
            }),
        )
        // /api/agent/recent-files → bare Vec<String>
        .route(
            "/api/agent/recent-files",
            get(|| async { Json(json!(["a.rs", "b.rs"])) }),
        )
        // api.rs:1221 search_events → bare Vec<FtsSearchResult>
        .route(
            "/api/search",
            get(|| async {
                Json(json!([{
                    "event_id": "e1", "session_id": "s-newer",
                    "record_type": "message.user.prompt", "snippet": "fix the <b>bug</b>",
                    "rank": -1.5
                }]))
            }),
        )
}

/// One mock, reused across assertions.
async fn store() -> HttpEventStore {
    let base = spawn_mock(mock_router()).await;
    HttpEventStore::new(base, None)
}

#[tokio::test]
async fn list_sessions_unwraps_envelope_and_remaps_fields() {
    // given the REST {"sessions":[…]} envelope with session_id/start_time,
    // when list_sessions runs, then it yields SessionRows with id/first_event.
    let rows = store().await.list_sessions().await.unwrap();
    assert_eq!(rows.len(), 2);
    let newer = rows.iter().find(|r| r.id == "s-newer").expect("s-newer present");
    assert_eq!(newer.first_event.as_deref(), Some("2026-06-13T10:00:00.000Z"));
    assert_eq!(newer.last_event.as_deref(), Some("2026-06-13T11:00:00.000Z"));
    assert_eq!(newer.event_count, 42);
    assert_eq!(newer.project_name.as_deref(), Some("Project A"));
    // fields the trimmed shape omits map to None, not a decode error
    assert!(newer.custom_label.is_none());
    assert!(newer.branch.is_none());
}

#[tokio::test]
async fn session_events_returns_raw_array() {
    let events = store().await.session_events("s-newer").await.unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0]["id"], "e1");
    assert_eq!(events[1]["data"]["raw"]["data"]["role"], "assistant");
}

#[tokio::test]
async fn session_patterns_unwraps_patterns_key() {
    let pats = store().await.session_patterns("s", Some("turn.sentence")).await.unwrap();
    assert_eq!(pats.len(), 1);
    assert_eq!(pats[0].summary, "edited file");
    assert_eq!(pats[0].metadata.get("verb").unwrap(), "edit");
}

#[tokio::test]
async fn synopsis_decodes_when_present() {
    let syn = store().await.query_session_synopsis("s-newer").await.expect("some synopsis");
    assert_eq!(syn.session_id, "s-newer");
    assert_eq!(syn.tool_count, 10);
    assert_eq!(syn.top_tools.len(), 2);
    assert_eq!(syn.top_tools[0].tool, "Edit");
}

#[tokio::test]
async fn synopsis_null_body_is_none() {
    // unknown session → null/404 → None (not a hard error)
    let syn = store().await.query_session_synopsis("missing").await;
    assert!(syn.is_none());
}

#[tokio::test]
async fn synopsis_unreachable_server_is_none() {
    // a store pointed at a dead port degrades to None, never panics
    let dead = HttpEventStore::new("http://127.0.0.1:1", None);
    assert!(dead.query_session_synopsis("x").await.is_none());
}

#[tokio::test]
async fn tool_journey_file_impact_errors_decode() {
    let s = store().await;
    let journey = s.query_tool_journey("s").await;
    assert_eq!(journey.len(), 2);
    assert_eq!(journey[1].tool, "Edit");

    let impact = s.query_file_impact("s").await;
    assert_eq!(impact[0].file, "a.rs");
    assert_eq!(impact[0].reads, 3);

    let errors = s.query_session_errors("s").await;
    assert_eq!(errors[0].message, "boom");
}

#[tokio::test]
async fn analytics_decode() {
    let s = store().await;
    let pulse = s.query_project_pulse(7).await;
    assert_eq!(pulse[0].session_count, 3);

    let prod = s.query_productivity_by_hour(30).await;
    assert_eq!(prod[0].hour, 10);
    assert_eq!(prod[0].event_count, 50);

    let tokens = s.query_token_usage(None, None, "opus").await;
    assert_eq!(tokens.session_count, 2);
    assert_eq!(tokens.usage.output_tokens, 200);
    assert_eq!(tokens.usage.total_tokens, 315);
    assert_eq!(tokens.cost.model, "opus");

    let daily = s.query_daily_token_usage(Some(7)).await;
    assert_eq!(daily[0].date, "2026-06-13");
}

#[tokio::test]
async fn project_tools_decode() {
    let s = store().await;
    let ctx = s.query_project_context("proj-a", 5).await;
    assert_eq!(ctx.len(), 1);
    let files = s.query_recent_files("proj-a", 5).await;
    assert_eq!(files, vec!["a.rs".to_string(), "b.rs".to_string()]);
}

#[tokio::test]
async fn search_decodes_fts_results() {
    let hits = store().await.search_fts("bug", 10, None).await.unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].event_id, "e1");
    assert_eq!(hits[0].session_id, "s-newer");
}

#[tokio::test]
async fn token_usage_unreachable_returns_zero_with_model_echo() {
    let dead = HttpEventStore::new("http://127.0.0.1:1", None);
    let t = dead.query_token_usage(None, None, "haiku").await;
    assert_eq!(t.session_count, 0);
    assert_eq!(t.cost.model, "haiku");
}
