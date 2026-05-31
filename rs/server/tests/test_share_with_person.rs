//! Integration tests for `POST /api/admin/share-with-person` (Phase 5.5).
//!
//! The endpoint records consent for one person to receive another's session
//! events via per-account NATS export/import. These tests exercise the
//! handler end-to-end against a real `AccountConfigWriter` wired into
//! AppState — no NATS, no Docker, just the persist path.
//!
//! The corresponding end-to-end test that walks `share_with_person` → conf
//! update → SIGHUP → cross-account delivery lives at
//! `rs/bus/tests/test_multi_account_isolation.rs` (Phase 5.10).

use std::collections::HashMap;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::{json, Value};
use tempfile::TempDir;
use tokio::sync::{broadcast, RwLock};
use tower::ServiceExt;

use open_story_bus::accounts::{AccountSpec, UserSpec};
use open_story_bus::noop_bus::NoopBus;
use open_story_server::account_config::AccountConfigWriter;
use open_story_server::admin::compute_topology;
use open_story_server::config::Config;
use open_story_server::router::build_router;
use open_story_server::state::AppState;
use open_story_server::watcher_diagnostics::WatcherDiagnostics;
use open_story_store::event_store::SessionRow;
use open_story_store::state::StoreState;

fn person(name: &str, user: &str) -> AccountSpec {
    AccountSpec {
        name: name.into(),
        users: vec![UserSpec {
            user: user.into(),
            password: format!("{user}-secret"),
        }],
        exports: vec![],
        imports: vec![],
    }
}

async fn test_state_with_writer(
    tmp: &TempDir,
) -> (
    Arc<RwLock<AppState>>,
    Arc<AccountConfigWriter>,
) {
    let store = StoreState::new(tmp.path()).unwrap();
    let (broadcast_tx, _) = broadcast::channel(256);
    let config = Config::default();
    let initial_topology = compute_topology(
        "test-host",
        config.role,
        &open_story_server::admin::EnvInputs::default(),
        &[],
    );
    let (admin_topology_tx, _) = tokio::sync::watch::channel(initial_topology);

    let writer = Arc::new(AccountConfigWriter::new(
        tmp.path().join("nats-server.conf"),
        "listen: 0.0.0.0:4222",
        vec![
            person("PERSON_MAX", "max"),
            person("PERSON_KATIE", "katie"),
        ],
    ));

    let state = Arc::new(RwLock::new(AppState {
        store,
        transcript_states: HashMap::new(),
        watcher_diagnostics: WatcherDiagnostics::default(),
        broadcast_tx,
        bus: Arc::new(NoopBus),
        admin_topology_tx,
        config,
        watch_dir: tmp.path().join("watch"),
        account_config_writer: Some(writer.clone()),
    }));
    (state, writer)
}

async fn seed_session(state: &Arc<RwLock<AppState>>, id: &str, owner: &str) {
    let s = state.read().await;
    s.store
        .event_store
        .upsert_session(&SessionRow {
            id: id.to_string(),
            project_id: None,
            project_name: None,
            label: None,
            custom_label: None,
            branch: None,
            event_count: 0,
            first_event: None,
            last_event: None,
            host: None,
            user: None,
            origin_agent: None,
            person_id: Some(owner.to_string()),
            principal_id: None,
        })
        .await
        .unwrap();
}

async fn post_share(state: Arc<RwLock<AppState>>, body: Value) -> axum::response::Response {
    let router = build_router(state, None, &Config::default());
    let req = Request::post("/api/admin/share-with-person")
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    router.oneshot(req).await.unwrap()
}

#[tokio::test]
async fn share_with_person_returns_204_and_updates_writer() {
    let tmp = tempfile::tempdir().unwrap();
    let (state, writer) = test_state_with_writer(&tmp).await;
    seed_session(&state, "sess-X", "max").await;

    let resp = post_share(
        state,
        json!({"session_id": "sess-X", "person_id": "katie"}),
    )
    .await;

    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    let conf = writer.current_config();
    assert!(
        conf.contains("{ stream: \"events.*.sess-X.>\", accounts: [PERSON_KATIE] }"),
        "expected export in conf, got:\n{conf}"
    );
    assert!(
        conf.contains("{ stream: { account: PERSON_MAX, subject: \"events.*.sess-X.>\" } }"),
        "expected matching import in conf, got:\n{conf}"
    );
}

#[tokio::test]
async fn share_with_person_persists_conf_to_disk() {
    let tmp = tempfile::tempdir().unwrap();
    let (state, _) = test_state_with_writer(&tmp).await;
    seed_session(&state, "sess-Y", "max").await;

    let resp = post_share(
        state,
        json!({"session_id": "sess-Y", "person_id": "katie"}),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    let on_disk = std::fs::read_to_string(tmp.path().join("nats-server.conf")).unwrap();
    assert!(on_disk.contains("PERSON_KATIE"));
    assert!(on_disk.contains("events.*.sess-Y.>"));
}

#[tokio::test]
async fn share_with_person_returns_404_for_unknown_session() {
    let tmp = tempfile::tempdir().unwrap();
    let (state, _) = test_state_with_writer(&tmp).await;

    let resp = post_share(
        state,
        json!({"session_id": "never-seen", "person_id": "katie"}),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn share_with_person_returns_409_when_session_has_no_person_id() {
    let tmp = tempfile::tempdir().unwrap();
    let (state, _) = test_state_with_writer(&tmp).await;

    // Seed a session with NO person_id stamped — pre-PR-#54 data.
    {
        let s = state.read().await;
        s.store
            .event_store
            .upsert_session(&SessionRow {
                id: "legacy".into(),
                project_id: None,
                project_name: None,
                label: None,
                custom_label: None,
                branch: None,
                event_count: 0,
                first_event: None,
                last_event: None,
                host: None,
                user: None,
                origin_agent: None,
                person_id: None,
                principal_id: None,
            })
            .await
            .unwrap();
    }

    let resp = post_share(
        state,
        json!({"session_id": "legacy", "person_id": "katie"}),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn share_with_person_returns_503_when_writer_not_configured() {
    // AppState with NO account_config_writer — simulating a node that's
    // running in non-multi-account mode.
    let tmp = tempfile::tempdir().unwrap();
    let store = StoreState::new(tmp.path()).unwrap();
    let (broadcast_tx, _) = broadcast::channel(256);
    let config = Config::default();
    let initial_topology = compute_topology(
        "test-host",
        config.role,
        &open_story_server::admin::EnvInputs::default(),
        &[],
    );
    let (admin_topology_tx, _) = tokio::sync::watch::channel(initial_topology);

    let state = Arc::new(RwLock::new(AppState {
        store,
        transcript_states: HashMap::new(),
        watcher_diagnostics: WatcherDiagnostics::default(),
        broadcast_tx,
        bus: Arc::new(NoopBus),
        admin_topology_tx,
        config,
        watch_dir: tmp.path().join("watch"),
        account_config_writer: None,
    }));

    let resp = post_share(
        state,
        json!({"session_id": "anything", "person_id": "katie"}),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
}
