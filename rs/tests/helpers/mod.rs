//! Shared test helpers for integration tests.

#[allow(dead_code)]
pub mod bus;
#[allow(dead_code)]
pub mod compose;
#[allow(dead_code)]
pub mod container;
#[allow(dead_code)]
pub mod k8s;
#[allow(dead_code)]
pub mod openclaw;
#[allow(dead_code)]
pub mod synth;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::Router;
use serde_json::{json, Value};
use tempfile::TempDir;
use tokio::sync::{broadcast, RwLock};

use open_story::cloud_event::CloudEvent;
use open_story::event_data::{AgentPayload, ClaudeCodePayload, EventData};
use open_story::server::{build_router, AppState, Config, SharedState};
use open_story_bus::noop_bus::NoopBus;
use open_story_store::state::StoreState;

/// Create an isolated AppState backed by a temp directory.
pub fn test_state(tmp: &TempDir) -> SharedState {
    let store = StoreState::new(tmp.path()).unwrap();
    let (broadcast_tx, _) = broadcast::channel(256);

    let watch_dir = tmp.path().join("watch");
    std::fs::create_dir_all(&watch_dir).unwrap();

    Arc::new(RwLock::new(AppState {
        store,
        transcript_states: HashMap::new(),
        broadcast_tx,
        bus: Arc::new(NoopBus),
        config: Config::default(),
        watch_dir,
    }))
}

/// Build a router with no static directory, backed by the given state.
pub fn test_router(state: SharedState) -> Router {
    let config = Config::default();
    build_router(state, None, &config)
}

/// Create a minimal valid CloudEvent.
pub fn make_event(event_type: &str, session_id: &str) -> CloudEvent {
    let mut payload = ClaudeCodePayload::new();
    payload.text = Some("test content".to_string());
    let data = EventData::with_payload(
        json!({}),
        0,
        session_id.to_string(),
        AgentPayload::ClaudeCode(payload),
    );
    CloudEvent::new(
        format!("arc://transcript/{session_id}"),
        event_type.to_string(),
        data,
        None,
        None, // auto-generates UUID
        None, // auto-generates timestamp
        None,
        None,
        None,
    )
}

/// Create a CloudEvent with a specific ID (for dedup testing).
pub fn make_event_with_id(event_type: &str, session_id: &str, id: &str) -> CloudEvent {
    let mut payload = ClaudeCodePayload::new();
    payload.text = Some("test content".to_string());
    let data = EventData::with_payload(
        json!({}),
        0,
        session_id.to_string(),
        AgentPayload::ClaudeCode(payload),
    );
    CloudEvent::new(
        format!("arc://transcript/{session_id}"),
        event_type.to_string(),
        data,
        None,
        Some(id.to_string()),
        None,
        None,
        None,
        None,
    )
}

/// Create a CloudEvent with a tool_result whose output is `size` bytes.
/// Uses the unified format (io.arc.event + message.user.tool_result).
pub fn make_event_with_large_payload(session_id: &str, id: &str, size: usize) -> CloudEvent {
    let content = "x".repeat(size);
    let data = EventData::with_payload(
        json!({
            "type": "user",
            "message": {
                "content": [{
                    "type": "tool_result",
                    "tool_use_id": "toolu_large",
                    "content": content
                }]
            }
        }),
        1,
        session_id.to_string(),
        AgentPayload::ClaudeCode(ClaudeCodePayload::new()),
    );
    CloudEvent::new(
        format!("arc://transcript/{session_id}"),
        "io.arc.event".to_string(),
        data,
        Some("message.user.tool_result".to_string()),
        Some(id.to_string()),
        None,
        None,
        None,
        None,
    )
}

/// Read a response body as a UTF-8 string.
pub async fn body_text(response: axum::http::Response<axum::body::Body>) -> String {
    use http_body_util::BodyExt;
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8(bytes.to_vec()).unwrap()
}

/// Create a CloudEvent that from_cloud_event turns into a UserMessage.
pub fn make_user_prompt(session_id: &str, id: &str) -> CloudEvent {
    let mut payload = ClaudeCodePayload::new();
    payload.text = Some("test prompt".to_string());
    let data = EventData::with_payload(
        json!({
            "type": "user",
            "message": {"content": [{"type": "text", "text": "test prompt"}]}
        }),
        1,
        session_id.to_string(),
        AgentPayload::ClaudeCode(payload),
    );
    CloudEvent::new(
        format!("arc://transcript/{session_id}"),
        "io.arc.event".to_string(),
        data,
        Some("message.user.prompt".to_string()),
        Some(id.to_string()),
        None,
        None,
        None,
        None,
    )
}

/// Create a CloudEvent that from_cloud_event turns into a ToolCall.
pub fn make_tool_use(
    session_id: &str,
    id: &str,
    parent_id: Option<&str>,
    tool_name: &str,
    command: &str,
) -> CloudEvent {
    let mut payload = ClaudeCodePayload::new();
    payload.tool = Some(tool_name.to_string());
    payload.args = Some(json!({"command": command}));
    if let Some(pid) = parent_id {
        payload.parent_uuid = Some(pid.to_string());
    }
    let data = EventData::with_payload(
        json!({
            "type": "assistant",
            "message": {
                "model": "claude-4",
                "content": [{
                    "type": "tool_use",
                    "id": format!("toolu_{id}"),
                    "name": tool_name,
                    "input": {"command": command}
                }]
            }
        }),
        2,
        session_id.to_string(),
        AgentPayload::ClaudeCode(payload),
    );
    CloudEvent::new(
        format!("arc://transcript/{session_id}"),
        "io.arc.event".to_string(),
        data,
        Some("message.assistant.tool_use".to_string()),
        Some(id.to_string()),
        None,
        None,
        None,
        None,
    )
}

/// Create a CloudEvent that from_cloud_event turns into a ToolResult.
pub fn make_tool_result(
    session_id: &str,
    id: &str,
    parent_id: Option<&str>,
    call_id: &str,
    output: &str,
) -> CloudEvent {
    let mut payload = ClaudeCodePayload::new();
    if let Some(pid) = parent_id {
        payload.parent_uuid = Some(pid.to_string());
    }
    let data = EventData::with_payload(
        json!({
            "type": "user",
            "message": {
                "content": [{
                    "type": "tool_result",
                    "tool_use_id": call_id,
                    "content": output
                }]
            }
        }),
        3,
        session_id.to_string(),
        AgentPayload::ClaudeCode(payload),
    );
    CloudEvent::new(
        format!("arc://transcript/{session_id}"),
        "io.arc.event".to_string(),
        data,
        Some("message.user.tool_result".to_string()),
        Some(id.to_string()),
        None,
        None,
        None,
        None,
    )
}

/// Create a CloudEvent that from_cloud_event turns into an AssistantMessage.
pub fn make_assistant_text(
    session_id: &str,
    id: &str,
    parent_id: Option<&str>,
    text: &str,
) -> CloudEvent {
    let mut payload = ClaudeCodePayload::new();
    payload.text = Some(text.to_string());
    payload.model = Some("claude-4".to_string());
    if let Some(pid) = parent_id {
        payload.parent_uuid = Some(pid.to_string());
    }
    let data = EventData::with_payload(
        json!({
            "type": "assistant",
            "message": {
                "model": "claude-4",
                "content": [{"type": "text", "text": text}]
            }
        }),
        4,
        session_id.to_string(),
        AgentPayload::ClaudeCode(payload),
    );
    CloudEvent::new(
        format!("arc://transcript/{session_id}"),
        "io.arc.event".to_string(),
        data,
        Some("message.assistant.text".to_string()),
        Some(id.to_string()),
        None,
        None,
        None,
        None,
    )
}

/// Create a progress CloudEvent (ephemeral — doesn't produce meaningful ViewRecords).
pub fn make_progress_event(session_id: &str, id: &str, parent_id: Option<&str>) -> CloudEvent {
    let mut payload = ClaudeCodePayload::new();
    if let Some(pid) = parent_id {
        payload.parent_uuid = Some(pid.to_string());
    }
    let data = EventData::with_payload(
        json!({"type": "progress", "subtype": "bash"}),
        5,
        session_id.to_string(),
        AgentPayload::ClaudeCode(payload),
    );
    CloudEvent::new(
        format!("arc://transcript/{session_id}"),
        "io.arc.event".to_string(),
        data,
        Some("progress.bash".to_string()),
        Some(id.to_string()),
        None,
        None,
        None,
        None,
    )
}

/// Seed events into the test state: persist to EventStore (Actor 1's job)
/// then broadcast (Actor 4's job via ingest_events). Simulates what both
/// actors do in production. Use this instead of calling `ingest_events`
/// directly when the test needs EventStore-backed API endpoints to work.
///
/// Added during the Actor 4 decomposition: `ingest_events` no longer
/// writes to EventStore, so tests that query EventStore-backed endpoints
/// need an explicit persist step first.
pub async fn seed_and_ingest(
    state: &mut open_story::server::AppState,
    session_id: &str,
    events: &[CloudEvent],
    project_id: Option<&str>,
) -> open_story::server::IngestResult {
    use open_story::server::ingest_events;
    use open_story_store::event_store::SessionRow;

    // Actor 1's job: persist each event to EventStore
    for ce in events {
        if let Ok(val) = serde_json::to_value(ce) {
            let _ = state.store.event_store.insert_event(session_id, &val).await;
        }
    }

    // Record project association (same as ingest_events does for live data)
    if let Some(pid) = project_id {
        state.store.session_projects.insert(session_id.to_string(), pid.to_string());
    }

    // Actor 4's job: project + broadcast
    let result = ingest_events(state, session_id, events, project_id).await;

    // Ensure the sessions table has a row — in production this happens
    // via upsert_session at the end of ingest_events when count > 0, but
    // test events without subtypes can't produce ViewRecords and count
    // stays 0. For tests that query /api/sessions, we need the row.
    // Mirror PersistConsumer: pull host off the first event if stamped.
    let host = events.first().and_then(|ce| ce.host.clone());

    let _ = state.store.event_store.upsert_session(&SessionRow {
        id: session_id.to_string(),
        project_id: project_id.map(|s| s.to_string()),
        project_name: None,
        label: state.store.projections.get(session_id).and_then(|p| p.label().map(|s| s.to_string())),
        custom_label: None,
        branch: None,
        event_count: events.len() as u64,
        first_event: events.first().map(|e| e.time.clone()),
        last_event: events.last().map(|e| e.time.clone()),
        host,
    }).await;

    result
}

/// Path to the test fixtures directory.
pub fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
}

/// Helper to send a request to a router and get the response.
/// Each call rebuilds the router since `oneshot` consumes it.
pub async fn send_request(
    state: SharedState,
    request: axum::http::Request<axum::body::Body>,
) -> axum::http::Response<axum::body::Body> {
    use tower::ServiceExt;
    let router = test_router(state);
    router.oneshot(request).await.unwrap()
}

/// Read a response body as JSON Value.
pub async fn body_json(response: axum::http::Response<axum::body::Body>) -> Value {
    use http_body_util::BodyExt;
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}
