//! Router construction — all routes wired together.

use std::path::Path;

use axum::extract::{DefaultBodyLimit, State};
use axum::middleware;
use axum::Router;
use tower_http::cors::{AllowOrigin, Any, CorsLayer};
use tower_http::services::ServeDir;

use crate::config::Config;
use crate::state::SharedState;

/// Maximum request body size (50 MB). Prevents OOM from oversized POST payloads.
const MAX_BODY_SIZE: usize = 50 * 1024 * 1024;

/// Build a CORS layer from config.
///
/// If `allowed_origins` is empty, allows localhost defaults (port 5173 + server port).
/// If `allowed_origins` is set, uses those exact origins.
pub fn build_cors(config: &Config) -> CorsLayer {
    if config.allowed_origins.is_empty() {
        // Default: allow localhost on common dev ports
        let origins: Vec<_> = [
            format!("http://localhost:{}", config.port),
            "http://localhost:5173".to_string(),
            "http://127.0.0.1:5173".to_string(),
            format!("http://127.0.0.1:{}", config.port),
        ]
        .into_iter()
        .filter_map(|o| o.parse().ok())
        .collect();
        CorsLayer::new()
            .allow_origin(origins)
            .allow_methods(Any)
            .allow_headers(Any)
    } else {
        let origins: Vec<_> = config
            .allowed_origins
            .iter()
            .filter_map(|o| o.parse().ok())
            .collect();
        if origins.is_empty() {
            // All configured origins were invalid — deny all cross-origin requests
            eprintln!(
                "WARNING: all configured CORS origins are invalid ({:?}). Denying cross-origin requests.",
                config.allowed_origins
            );
            CorsLayer::new()
                .allow_origin(AllowOrigin::list(Vec::<axum::http::HeaderValue>::new()))
                .allow_methods(Any)
                .allow_headers(Any)
        } else {
            CorsLayer::new()
                .allow_origin(origins)
                .allow_methods(Any)
                .allow_headers(Any)
        }
    }
}

/// Health check endpoint — returns 200 with role info.
async fn health(State(state): State<SharedState>) -> axum::Json<serde_json::Value> {
    let s = state.read().await;
    axum::Json(serde_json::json!({
        "status": "ok",
        "role": s.config.role.to_string(),
    }))
}

/// Build a minimal router for publisher mode — hooks + health only.
///
/// Publisher doesn't serve API/WebSocket endpoints. It only accepts
/// hook POSTs (to publish to NATS) and responds to health checks.
pub fn build_publisher_router(state: SharedState, config: &Config) -> Router {
    let publisher_router = Router::new()
        .route("/hooks", axum::routing::post(crate::hooks::receive_hook))
        .route("/health", axum::routing::get(health));

    let cors = build_cors(config);

    let api_token = config.api_token.clone();
    publisher_router
        .layer(middleware::from_fn(move |req, next| {
            let token = api_token.clone();
            async move { crate::auth::auth_middleware(req, next, token).await }
        }))
        .layer(cors)
        .layer(DefaultBodyLimit::max(MAX_BODY_SIZE))
        .with_state(state)
}

/// Build the axum Router with all routes.
pub fn build_router(state: SharedState, static_dir: Option<&Path>, config: &Config) -> Router {
    let api_router = Router::new()
        .route("/api/sessions", axum::routing::get(crate::api::list_sessions))
        .route(
            "/api/sessions/{session_id}/events",
            axum::routing::get(crate::api::get_events),
        )
        .route(
            "/api/sessions/{session_id}/summary",
            axum::routing::get(crate::api::get_summary),
        )
        .route(
            "/api/sessions/{session_id}/activity",
            axum::routing::get(crate::api::get_activity),
        )
        .route(
            "/api/sessions/{session_id}/tools",
            axum::routing::get(crate::api::get_tools),
        )
        .route(
            "/api/sessions/{session_id}/transcript",
            axum::routing::get(crate::api::get_transcript),
        )
        .route(
            "/api/sessions/{session_id}/view-records",
            axum::routing::get(crate::api::get_view_records),
        )
        .route(
            "/api/sessions/{session_id}/conversation",
            axum::routing::get(crate::api::get_conversation),
        )
        .route(
            "/api/sessions/{session_id}/file-changes",
            axum::routing::get(crate::api::get_file_changes),
        )
        .route(
            "/api/sessions/{session_id}/patterns",
            axum::routing::get(crate::api::get_patterns),
        )
        .route(
            "/api/sessions/{session_id}/plans",
            axum::routing::get(crate::api::get_session_plans),
        )
        .route("/api/plans", axum::routing::get(crate::api::list_plans))
        .route("/api/plans/{plan_id}", axum::routing::get(crate::api::get_plan))
        .route(
            "/api/tool-schemas",
            axum::routing::get(crate::api::get_tool_schemas),
        )
        .route(
            "/api/sessions/{session_id}/meta",
            axum::routing::get(crate::api::get_session_meta),
        )
        .route(
            "/api/sessions/{session_id}/events/{event_id}/content",
            axum::routing::get(crate::api::get_event_content),
        )
        // ── Records endpoint (WireRecords from projections) ──
        .route(
            "/api/sessions/{session_id}/records",
            axum::routing::get(crate::api::get_session_records),
        )
        // ── Lifecycle endpoints (Phase A4) ──
        .route(
            "/api/sessions/{session_id}",
            axum::routing::delete(crate::api::delete_session),
        )
        .route(
            "/api/sessions/{session_id}/export",
            axum::routing::get(crate::api::export_session),
        )
        // ── Query endpoints (Phase B3) ──
        .route(
            "/api/sessions/{session_id}/synopsis",
            axum::routing::get(crate::api::get_session_synopsis),
        )
        .route(
            "/api/sessions/{session_id}/tool-journey",
            axum::routing::get(crate::api::get_tool_journey),
        )
        .route(
            "/api/sessions/{session_id}/file-impact",
            axum::routing::get(crate::api::get_file_impact),
        )
        .route(
            "/api/sessions/{session_id}/errors",
            axum::routing::get(crate::api::get_session_errors),
        )
        .route("/api/insights/pulse", axum::routing::get(crate::api::get_project_pulse))
        .route("/api/insights/tool-evolution", axum::routing::get(crate::api::get_tool_evolution))
        .route("/api/insights/efficiency", axum::routing::get(crate::api::get_session_efficiency_insights))
        .route("/api/insights/productivity", axum::routing::get(crate::api::get_productivity))
        .route("/api/insights/token-usage", axum::routing::get(crate::api::get_token_usage))
        .route("/api/insights/token-usage/daily", axum::routing::get(crate::api::get_daily_token_usage))
        .route("/api/agent/tools", axum::routing::get(crate::api::get_agent_tools))
        .route("/api/agent/project-context", axum::routing::get(crate::api::get_agent_project_context))
        .route("/api/agent/recent-files", axum::routing::get(crate::api::get_agent_recent_files))
        .route("/api/agent/search", axum::routing::get(crate::api::agent_search))
        // ── Search ──
        .route("/api/search", axum::routing::get(crate::api::search_events))
        // ── Core routes ──
        .route("/ws", axum::routing::get(crate::ws::ws_handler))
        .route("/hooks", axum::routing::post(crate::hooks::receive_hook))
        .route("/health", axum::routing::get(health));

    let cors = build_cors(config);

    // Auth middleware — wraps all routes. Empty token = pass-through.
    let api_token = config.api_token.clone();
    let router = api_router
        .layer(middleware::from_fn(move |req, next| {
            let token = api_token.clone();
            async move { crate::auth::auth_middleware(req, next, token).await }
        }))
        .layer(cors)
        .layer(DefaultBodyLimit::max(MAX_BODY_SIZE))
        .with_state(state);

    // Add /metrics endpoint if enabled (outside auth — Prometheus scrapes without tokens)
    let router = if config.metrics_enabled {
        if let Some(handle) = crate::metrics::init_recorder() {
            router.merge(crate::metrics::metrics_router(handle))
        } else {
            router
        }
    } else {
        router
    };

    // Serve static files if directory exists
    if let Some(dir) = static_dir {
        if dir.exists() {
            router.fallback_service(ServeDir::new(dir))
        } else {
            router
        }
    } else {
        router
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Role;

    #[test]
    fn build_cors_empty_origins_uses_localhost_defaults() {
        let config = Config::default();
        // Should not panic — builds successfully with localhost defaults
        let _cors = build_cors(&config);
    }

    #[test]
    fn build_cors_configured_origins() {
        let config = Config {
            allowed_origins: vec!["http://example.com".into()],
            ..Config::default()
        };
        let _cors = build_cors(&config);
    }

    fn test_state() -> SharedState {
        use std::collections::HashMap;
        use std::sync::Arc;
        use tokio::sync::{broadcast, RwLock};
        use open_story_bus::noop_bus::NoopBus;
        use open_story_store::state::StoreState;

        let tmp = tempfile::tempdir().unwrap();
        let store = StoreState::new(tmp.path()).unwrap();
        let (broadcast_tx, _) = broadcast::channel(256);
        let watch_dir = tmp.path().join("watch");
        std::fs::create_dir_all(&watch_dir).unwrap();

        Arc::new(RwLock::new(crate::state::AppState {
            store,
            transcript_states: HashMap::new(),
            broadcast_tx,
            bus: Arc::new(NoopBus),
            config: Config::default(),
            watch_dir,
        }))
    }

    #[tokio::test]
    async fn publisher_router_has_hooks_and_health() {
        use tower::ServiceExt;
        use axum::http::Request;
        use axum::body::Body;

        let state = test_state();
        let config = Config { role: Role::Publisher, ..Config::default() };
        let router = build_publisher_router(state.clone(), &config);

        // GET /health should return 200
        let req = Request::get("/health").body(Body::empty()).unwrap();
        let resp = router.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), 200);

        // POST /hooks with empty body should return 202 (accepted)
        let req = Request::post("/hooks")
            .header("content-type", "application/json")
            .body(Body::from("{}"))
            .unwrap();
        let resp = router.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), 202);

        // GET /api/sessions should return 404 (not on publisher router)
        let req = Request::get("/api/sessions").body(Body::empty()).unwrap();
        let resp = router.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), 404);
    }

    #[tokio::test]
    async fn full_router_has_health_endpoint() {
        use tower::ServiceExt;
        use axum::http::Request;
        use axum::body::Body;

        let state = test_state();
        let config = Config::default();
        let router = build_router(state.clone(), None, &config);

        let req = Request::get("/health").body(Body::empty()).unwrap();
        let resp = router.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), 200);

        // Verify response body contains role
        use http_body_util::BodyExt;
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["status"], "ok");
        assert_eq!(body["role"], "full");
    }
}
