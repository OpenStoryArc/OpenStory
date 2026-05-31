//! Integration tests for `admin_only_middleware` (Phase 6.1+6.2).
//!
//! Pins the credential-separation contract: when `admin_token` is set,
//! presenting `api_token` on a policy-write route returns 403 (you're
//! authenticated but not permitted), not 204. This is the escalation gap
//! Phase 6 closes — a leaked read-only API key cannot mutate share policy.

use std::collections::HashMap;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::json;
use tokio::sync::{broadcast, RwLock};
use tower::ServiceExt;

use open_story_bus::noop_bus::NoopBus;
use open_story_server::admin::compute_topology;
use open_story_server::config::Config;
use open_story_server::router::build_router;
use open_story_server::state::AppState;
use open_story_server::watcher_diagnostics::WatcherDiagnostics;
use open_story_store::state::StoreState;

async fn state_with_tokens(
    tmp: &tempfile::TempDir,
    api_token: &str,
    admin_token: &str,
) -> Arc<RwLock<AppState>> {
    let store = StoreState::new(tmp.path()).unwrap();
    let (broadcast_tx, _) = broadcast::channel(256);
    let mut config = Config::default();
    config.api_token = api_token.to_string();
    config.admin_token = admin_token.to_string();
    let initial_topology = compute_topology(
        "test-host",
        config.role,
        &open_story_server::admin::EnvInputs::default(),
        &[],
    );
    let (admin_topology_tx, _) = tokio::sync::watch::channel(initial_topology);

    Arc::new(RwLock::new(AppState {
        store,
        transcript_states: HashMap::new(),
        watcher_diagnostics: WatcherDiagnostics::default(),
        broadcast_tx,
        bus: Arc::new(NoopBus),
        admin_topology_tx,
        config: config.clone(),
        watch_dir: tmp.path().join("watch"),
        account_config_writer: None,
        account_config_reloader: None,
    }))
}

async fn put_share_policy(
    state: Arc<RwLock<AppState>>,
    config: &Config,
    bearer: &str,
) -> axum::response::Response {
    let router = build_router(state, None, config);
    let req = Request::put("/api/admin/share-policy/sess-X")
        .header("authorization", format!("Bearer {bearer}"))
        .header("content-type", "application/json")
        .body(Body::from(json!({"mode": "private"}).to_string()))
        .unwrap();
    router.oneshot(req).await.unwrap()
}

#[tokio::test]
async fn admin_token_set_rejects_api_token_on_policy_write_with_403() {
    let tmp = tempfile::tempdir().unwrap();
    let state = state_with_tokens(&tmp, "api-secret", "admin-secret").await;
    let config = {
        let s = state.read().await;
        s.config.clone()
    };
    let resp = put_share_policy(state, &config, "api-secret").await;
    // Authenticated (token matches a configured tier) but not authorized
    // for the admin surface → 403, NOT 204 or 401.
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn admin_token_accepts_admin_token_on_policy_write() {
    let tmp = tempfile::tempdir().unwrap();
    let state = state_with_tokens(&tmp, "api-secret", "admin-secret").await;
    let config = {
        let s = state.read().await;
        s.config.clone()
    };
    let resp = put_share_policy(state, &config, "admin-secret").await;
    // Success on the policy-write path. Handler may return 204 or 500
    // depending on whether the underlying store call succeeds — what we're
    // pinning is that auth/authz *did not* block the request.
    assert!(
        !matches!(resp.status(), StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN),
        "admin token should NOT be denied; got {}",
        resp.status()
    );
}

#[tokio::test]
async fn admin_token_garbage_returns_401_not_403() {
    let tmp = tempfile::tempdir().unwrap();
    let state = state_with_tokens(&tmp, "api-secret", "admin-secret").await;
    let config = {
        let s = state.read().await;
        s.config.clone()
    };
    let resp = put_share_policy(state, &config, "neither-known").await;
    // Not authenticated at all → 401, not 403.
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn empty_admin_token_falls_back_to_api_token_on_policy_write() {
    // Single-token deployment (no admin separation configured): the
    // api_token must still work on admin routes. Backwards compat.
    let tmp = tempfile::tempdir().unwrap();
    let state = state_with_tokens(&tmp, "api-secret", "").await;
    let config = {
        let s = state.read().await;
        s.config.clone()
    };
    let resp = put_share_policy(state, &config, "api-secret").await;
    assert!(
        !matches!(resp.status(), StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN),
        "single-token deployment: api_token should be accepted on admin routes, got {}",
        resp.status()
    );
}

#[tokio::test]
async fn empty_admin_token_still_rejects_wrong_token_on_policy_write() {
    // Single-token deployment + wrong token → 401 (the existing auth
    // middleware path). Sanity check that the fallback didn't open a hole.
    let tmp = tempfile::tempdir().unwrap();
    let state = state_with_tokens(&tmp, "api-secret", "").await;
    let config = {
        let s = state.read().await;
        s.config.clone()
    };
    let resp = put_share_policy(state, &config, "wrong").await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}
