//! Router construction — all routes wired together.

use std::path::Path;

use axum::Router;
use axum::extract::{DefaultBodyLimit, State};
use axum::middleware;
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

/// Build a minimal router for publisher mode — health only.
///
/// Publisher doesn't serve API/WebSocket endpoints. After the hooks
/// endpoint was retired, publisher mode only responds to health checks
/// — events arrive via the file watcher and flow through NATS.
pub fn build_publisher_router(state: SharedState, config: &Config) -> Router {
    let publisher_router = Router::new().route("/health", axum::routing::get(health));

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
        .route(
            "/api/sessions",
            axum::routing::get(crate::api::list_sessions),
        )
        .route("/api/health", axum::routing::get(crate::api::node_health))
        .route("/api/control", axum::routing::post(crate::api::post_control))
        .route(
            "/api/annotations",
            axum::routing::post(crate::api::post_annotation).get(crate::api::list_annotations),
        )
        .route(
            "/api/annotations/{id}",
            axum::routing::delete(crate::api::delete_annotation),
        )
        .route("/api/interactions", axum::routing::post(crate::api::post_interaction))
        .route("/api/ui-state", axum::routing::get(crate::api::get_ui_state))
        .route("/api/ui-state/journey", axum::routing::get(crate::api::get_ui_journey))
        .route("/api/digests", axum::routing::get(crate::api::session_digests))
        .route(
            "/api/watchers",
            axum::routing::get(crate::api::list_watchers),
        )
        .route("/api/users", axum::routing::get(crate::api::list_users))
        .route(
            "/api/local-info",
            axum::routing::get(crate::api::list_local_info),
        )
        .route("/api/fleet", axum::routing::get(crate::api::get_fleet))
        .route(
            "/api/admin/topology",
            axum::routing::get(crate::admin::get_topology),
        )
        .route(
            "/api/admin/participants",
            axum::routing::get(crate::admin::list_participants),
        )
        // NB: Policy WRITES (PUT/POST below) move to `admin_writes_router`
        // below so they get the `admin_only_middleware` instead of the
        // generic `auth_middleware`. The GET above is read-only and stays
        // on the api_token surface.
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
            "/api/sessions/{session_id}/turns",
            axum::routing::get(crate::api::get_turns),
        )
        .route(
            "/api/sessions/{session_id}/plans",
            axum::routing::get(crate::api::get_session_plans),
        )
        .route("/api/plans", axum::routing::get(crate::api::list_plans))
        .route(
            "/api/plans/{plan_id}",
            axum::routing::get(crate::api::get_plan),
        )
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
        .route(
            "/api/insights/pulse",
            axum::routing::get(crate::api::get_project_pulse),
        )
        .route(
            "/api/insights/tool-evolution",
            axum::routing::get(crate::api::get_tool_evolution),
        )
        .route(
            "/api/insights/efficiency",
            axum::routing::get(crate::api::get_session_efficiency_insights),
        )
        .route(
            "/api/insights/productivity",
            axum::routing::get(crate::api::get_productivity),
        )
        .route(
            "/api/insights/token-usage",
            axum::routing::get(crate::api::get_token_usage),
        )
        .route(
            "/api/insights/token-usage/daily",
            axum::routing::get(crate::api::get_daily_token_usage),
        )
        .route(
            "/api/agent/tools",
            axum::routing::get(crate::api::get_agent_tools),
        )
        .route(
            "/api/agent/project-context",
            axum::routing::get(crate::api::get_agent_project_context),
        )
        .route(
            "/api/agent/recent-files",
            axum::routing::get(crate::api::get_agent_recent_files),
        )
        .route(
            "/api/agent/search",
            axum::routing::get(crate::api::agent_search),
        )
        // ── Search ──
        .route("/api/search", axum::routing::get(crate::api::search_events))
        // ── Core routes ──
        .route("/ws", axum::routing::get(crate::ws::ws_handler))
        .route("/health", axum::routing::get(health));

    let cors = build_cors(config);

    // Phase 6.2 — policy-write routes get the admin_only middleware so a
    // read-only api_token holder can't mutate participant roles. Empty
    // `admin_token` falls back to api_token semantics (backwards compat
    // for single-token deployments).
    let api_token_for_admin = config.api_token.clone();
    let admin_token = config.admin_token.clone();
    let admin_writes_router = Router::new()
        // Phase 6 polish: participants (role) management. Admin-gated. GET
        // sits on the read tier above so the UI can populate dropdowns
        // without an admin token.
        .route(
            "/api/admin/participants",
            axum::routing::put(crate::admin::upsert_participant),
        )
        .route(
            "/api/admin/participants/{principal_id}",
            axum::routing::delete(crate::admin::delete_participant),
        )
        // Layer order is outermost-first: the require_admin_role check
        // runs BEFORE the token check, but both must pass before the
        // handler runs. (Token check verifies the caller; role check
        // verifies the local principal has the assigned permission.)
        .layer(middleware::from_fn(move |req, next| {
            let api_t = api_token_for_admin.clone();
            let admin_t = admin_token.clone();
            async move {
                crate::auth::admin_only_middleware(req, next, api_t, admin_t).await
            }
        }))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            crate::auth::require_admin_role_middleware,
        ));

    // Apply the generic auth_middleware to api_router BEFORE merging in
    // admin_writes_router. This way the admin writes are wrapped only by
    // admin_only_middleware (which already handles the api_token fallback
    // when admin_token is empty), not by both — which would otherwise
    // reject a valid admin_token as "not the api_token."
    let api_token = config.api_token.clone();
    let api_router = api_router.layer(middleware::from_fn(move |req, next| {
        let token = api_token.clone();
        async move { crate::auth::auth_middleware(req, next, token).await }
    }));

    let router = api_router
        .merge(admin_writes_router)
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
        use open_story_bus::noop_bus::NoopBus;
        use open_story_store::state::StoreState;
        use std::collections::HashMap;
        use std::sync::Arc;
        use tokio::sync::{RwLock, broadcast};

        let tmp = tempfile::tempdir().unwrap();
        let store = StoreState::new(tmp.path()).unwrap();
        let (broadcast_tx, _) = broadcast::channel(256);
        let watch_dir = tmp.path().join("watch");
        std::fs::create_dir_all(&watch_dir).unwrap();

        let config = Config::default();
        let initial_topology = crate::admin::compute_topology(
            "test-host",
            config.role,
            &crate::admin::EnvInputs::default(),
            &[],
        );
        let (admin_topology_tx, _) = tokio::sync::watch::channel(initial_topology);

        Arc::new(RwLock::new(crate::state::AppState {
            store,
            transcript_states: HashMap::new(),
            watcher_diagnostics: crate::watcher_diagnostics::WatcherDiagnostics::default(),
            broadcast_tx,
            bus: Arc::new(NoopBus),
            admin_topology_tx,
            config,
            watch_dir,
            account_config_writer: None,
            account_config_reloader: None,
            role_directory: Arc::new(crate::directory::NoopRoleDirectory),
        }))
    }

    #[tokio::test]
    async fn publisher_router_serves_health_only_after_hooks_retirement() {
        use axum::body::Body;
        use axum::http::Request;
        use tower::ServiceExt;

        let state = test_state();
        let config = Config {
            role: Role::Publisher,
            ..Config::default()
        };
        let router = build_publisher_router(state.clone(), &config);

        // GET /health should return 200
        let req = Request::get("/health").body(Body::empty()).unwrap();
        let resp = router.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), 200);

        // GET /api/sessions should return 404 (not on publisher router)
        let req = Request::get("/api/sessions").body(Body::empty()).unwrap();
        let resp = router.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), 404);
    }

    #[tokio::test]
    async fn full_router_has_health_endpoint() {
        use axum::body::Body;
        use axum::http::Request;
        use tower::ServiceExt;

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

    #[tokio::test]
    async fn fleet_endpoint_returns_404_when_no_person_configured() {
        use tower::ServiceExt;
        use axum::http::Request;
        use axum::body::Body;

        // test_state() uses Config::default() which has person: None.
        let state = test_state();
        let config = Config::default();
        let router = build_router(state.clone(), None, &config);

        let req = Request::get("/api/fleet").body(Body::empty()).unwrap();
        let resp = router.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), 404);
    }

    #[tokio::test]
    async fn fleet_endpoint_returns_configured_fleet() {
        use tower::ServiceExt;
        use axum::http::Request;
        use axum::body::Body;
        use http_body_util::BodyExt;
        use crate::config::{Person, Principal, PrincipalMatchers};

        // Inject a Person + Principal into state's config so the endpoint
        // has a fleet to return.
        let state = test_state();
        {
            let mut s = state.write().await;
            s.config.person = Some(Person {
                id: "person-test".to_string(),
                display_name: "Tester".to_string(),
                email: "tester@example.test".to_string(),
                principals: vec![
                    Principal {
                        id: "k-laptop".to_string(),
                        display_name: "Laptop".to_string(),
                        matchers: PrincipalMatchers {
                            host: Some("test-host".into()),
                            user: Some("tester".into()),
                            agent: None,
                            watch_dir_pattern: None,
                        },
                    },
                ],
            });
        }
        let config = Config::default();
        let router = build_router(state.clone(), None, &config);

        let req = Request::get("/api/fleet").body(Body::empty()).unwrap();
        let resp = router.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), 200);

        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["person"]["id"], "person-test");
        assert_eq!(body["person"]["display_name"], "Tester");
        assert_eq!(body["person"]["email"], "tester@example.test");
        assert_eq!(body["principals"].as_array().unwrap().len(), 1);
        assert_eq!(body["principals"][0]["id"], "k-laptop");
        assert_eq!(body["principals"][0]["display_name"], "Laptop");
        assert_eq!(body["principals"][0]["matchers"]["host"], "test-host");
        assert_eq!(body["principals"][0]["matchers"]["user"], "tester");
        // Wildcard matchers serialize as null (None → JSON null).
        assert!(body["principals"][0]["matchers"]["agent"].is_null());
        assert!(body["principals"][0]["matchers"]["watch_dir_pattern"].is_null());
    }

    #[tokio::test]
    async fn full_router_exposes_watcher_diagnostics() {
        use axum::body::Body;
        use axum::http::Request;
        use http_body_util::BodyExt;
        use tower::ServiceExt;

        let state = test_state();
        {
            let diagnostics = state.read().await.watcher_diagnostics.clone();
            diagnostics.register_actor(&crate::watcher_diagnostics::WatcherActorConfig::new(
                "codex",
                crate::watcher_diagnostics::WatcherProtocol::AppendJsonl,
                std::path::PathBuf::from("/tmp/codex"),
            ));
        }
        let config = Config::default();
        let router = build_router(state.clone(), None, &config);

        let req = Request::get("/api/watchers").body(Body::empty()).unwrap();
        let resp = router.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), 200);
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

        assert_eq!(body["watchers"].as_array().unwrap().len(), 1);
        assert_eq!(body["watchers"][0]["agent"], "codex");
        assert_eq!(body["watchers"][0]["protocol"], "append-jsonl");
    }
}
