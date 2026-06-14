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

use anyhow::Result;
use async_trait::async_trait;
use std::sync::atomic::{AtomicUsize, Ordering};

use open_story_bus::accounts::{AccountSpec, UserSpec};
use open_story_bus::noop_bus::NoopBus;
use open_story_server::account_config::{AccountConfigWriter, NatsReloader};
use open_story_server::admin::compute_topology;
use open_story_server::config::Config;
use open_story_server::directory::{
    EmbeddedRoleDirectory, Participant, Role, RoleDirectory,
};
use open_story_server::router::build_router;
use open_story_server::state::AppState;
use open_story_server::watcher_diagnostics::WatcherDiagnostics;
use open_story_store::event_store::SessionRow;
use open_story_store::state::StoreState;

/// Phase 6.6 — share-with-person sits behind the role-required gate.
/// All tests here assume the local principal has Admin role so the gate
/// passes and we can pin handler-side behavior. Role-gating itself is
/// tested in test_admin_auth.rs.
const TEST_PRINCIPAL: &str = "test-principal";

async fn admin_role_directory() -> Arc<dyn RoleDirectory> {
    let dir = EmbeddedRoleDirectory::in_memory().unwrap();
    dir.upsert_participant(Participant {
        principal_id: TEST_PRINCIPAL.into(),
        person_id: "max".into(),
        role: Role::Admin,
        created_at: "2026-05-31T00:00:00Z".into(),
    })
    .await
    .unwrap();
    Arc::new(dir)
}

fn config_with_principal() -> Config {
    let mut c = Config::default();
    c.local_principal_id = TEST_PRINCIPAL.to_string();
    c
}

fn person(name: &str, user: &str) -> AccountSpec {
    AccountSpec {
        name: name.into(),
        users: vec![UserSpec {
            user: user.into(),
            password: format!("{user}-secret"),
            permissions: None,
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
    let config = config_with_principal();
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
        account_config_reloader: None,
        role_directory: admin_role_directory().await,
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
async fn share_with_person_refuses_when_caller_is_not_the_owner() {
    // Owner-consent gate: being Admin authorizes you to manage YOUR sharing,
    // not to re-share data you don't own. The caller (test-principal) is
    // person "max"; this session is owned by "alice". Max must not be able
    // to share alice's session, and the NATS conf must be left untouched.
    let tmp = tempfile::tempdir().unwrap();
    let (state, writer) = test_state_with_writer(&tmp).await;
    seed_session(&state, "sess-alice", "alice").await;

    let before = writer.current_config();

    let resp = post_share(
        Arc::clone(&state),
        json!({"session_id": "sess-alice", "person_id": "katie"}),
    )
    .await;

    assert_eq!(
        resp.status(),
        StatusCode::FORBIDDEN,
        "an admin who is not the owner must not share another person's session"
    );
    assert_eq!(
        writer.current_config(),
        before,
        "a refused share must not mutate the account config"
    );
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

/// Counting reloader — increments `count` every time `reload()` is called.
/// Lets the test assert that the handler actually triggered the reload
/// path after persisting, not just left the conf on disk.
struct CountingReloader {
    count: Arc<AtomicUsize>,
}

#[async_trait]
impl NatsReloader for CountingReloader {
    async fn reload(&self) -> Result<()> {
        self.count.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

#[tokio::test]
async fn share_with_person_auto_creates_stub_for_unknown_target_person() {
    // The boot-wire path seeds the writer with only the local person
    // (PERSON_MAX). The operator should still be able to share with
    // someone (PERSON_BOBBY) whose credentials haven't been provisioned
    // yet — the writer's `add_share_with_stubs` creates the target
    // account as a stub. Real delivery to Bobby still requires his
    // credentials to land in the conf, but the share gesture doesn't
    // have to fail just because the directory side hasn't caught up.
    let tmp = tempfile::tempdir().unwrap();
    let store = StoreState::new(tmp.path()).unwrap();
    let (broadcast_tx, _) = broadcast::channel(256);
    let config = config_with_principal();
    let initial_topology = compute_topology(
        "test-host",
        config.role,
        &open_story_server::admin::EnvInputs::default(),
        &[],
    );
    let (admin_topology_tx, _) = tokio::sync::watch::channel(initial_topology);

    // Writer seeded with ONLY max — no bobby.
    let writer = Arc::new(AccountConfigWriter::new(
        tmp.path().join("nats-server.conf"),
        "listen: 0.0.0.0:4222",
        vec![person("PERSON_MAX", "max")],
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
        account_config_reloader: None,
        role_directory: admin_role_directory().await,
    }));
    seed_session(&state, "sess-Z", "max").await;

    let resp = post_share(
        state,
        json!({"session_id": "sess-Z", "person_id": "bobby"}),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    let conf = writer.current_config();
    assert!(
        conf.contains("PERSON_BOBBY"),
        "expected PERSON_BOBBY stub to be created, got:\n{conf}"
    );
    assert!(conf.contains("accounts: [PERSON_BOBBY]"));
}

#[tokio::test]
async fn share_with_person_invokes_reloader_after_persist() {
    let tmp = tempfile::tempdir().unwrap();
    let store = StoreState::new(tmp.path()).unwrap();
    let (broadcast_tx, _) = broadcast::channel(256);
    let config = config_with_principal();
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
    let reload_count = Arc::new(AtomicUsize::new(0));
    let reloader: Arc<dyn NatsReloader> = Arc::new(CountingReloader {
        count: reload_count.clone(),
    });
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
        account_config_reloader: Some(reloader),
        role_directory: admin_role_directory().await,
    }));
    seed_session(&state, "sess-RX", "max").await;

    let resp = post_share(
        state,
        json!({"session_id": "sess-RX", "person_id": "katie"}),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        reload_count.load(Ordering::SeqCst),
        1,
        "reloader should fire exactly once per successful share"
    );
}

#[tokio::test]
async fn share_with_person_returns_503_when_writer_not_configured() {
    // AppState with NO account_config_writer — simulating a node that's
    // running in non-multi-account mode.
    let tmp = tempfile::tempdir().unwrap();
    let store = StoreState::new(tmp.path()).unwrap();
    let (broadcast_tx, _) = broadcast::channel(256);
    let config = config_with_principal();
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
        account_config_reloader: None,
        role_directory: admin_role_directory().await,
    }));

    let resp = post_share(
        state,
        json!({"session_id": "anything", "person_id": "katie"}),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
}
