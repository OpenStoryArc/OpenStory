//! Story backfill report (memory hands, A-12).
//!
//! Seeds a temp SqliteStore with golden event streams and asks the
//! backfill for its report. Read-only over events; nothing is written
//! unless the caller asks.

use open_story_server::story_backfill::{report, StoryReport};
use open_story_store::event_store::{EventStore, SessionRow};
use open_story_store::sqlite_store::SqliteStore;
use std::path::PathBuf;
use std::sync::Arc;

fn golden_events(name: &str) -> Vec<serde_json::Value> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../patterns/tests/fixtures/story")
        .join(name)
        .join("events.jsonl");
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

async fn seeded_store() -> (Arc<dyn EventStore>, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let store: Arc<dyn EventStore> = Arc::new(SqliteStore::new(dir.path()).unwrap());
    for name in ["two_arcs_gap", "ambiguous_closure_opening"] {
        let events = golden_events(name);
        let sid = events[0]["data"]["session_id"]
            .as_str()
            .unwrap()
            .to_string();
        store.insert_batch(&sid, &events).await.unwrap();
        let row: SessionRow =
            serde_json::from_value(serde_json::json!({ "id": sid, "event_count": events.len() }))
                .unwrap();
        store.upsert_session(&row).await.unwrap();
    }
    (store, dir)
}

mod when_backfill_runs_on_a_seeded_store {
    use super::*;

    #[tokio::test]
    async fn it_reports_counts_and_ambiguous_share() {
        let (store, _dir) = seeded_store().await;
        let r: StoryReport = report(store.as_ref(), 1800).await.unwrap();
        assert_eq!(r.sessions, 2);
        assert_eq!(r.exchanges, 8, "4 + 4 exchanges");
        assert_eq!(
            r.arcs, 3,
            "two_arcs_gap splits, the ambiguous golden does not"
        );
        assert_eq!(r.ambiguous_arcs, 1);
        assert!((r.ambiguous_share() - 1.0 / 3.0).abs() < 1e-9);
        let text = r.render();
        assert!(text.contains("sessions: 2"), "{text}");
        assert!(text.contains("arcs: 3"), "{text}");
        assert!(text.contains("ambiguous"), "{text}");
    }
}

// ═══════════════════════════════════════════════════════════════════
// Admin endpoint: POST /api/admin/story-backfill (memory hands, A-12 wiring)
// ═══════════════════════════════════════════════════════════════════

mod when_the_admin_backfill_endpoint_is_posted {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use open_story_bus::noop_bus::NoopBus;
    use open_story_server::config::Config;
    use open_story_server::router::build_router;
    use open_story_server::state::AppState;
    use open_story_server::watcher_diagnostics::WatcherDiagnostics;
    use open_story_store::state::StoreState;
    use std::collections::HashMap;
    use tokio::sync::{broadcast, RwLock};
    use tower::ServiceExt;

    async fn app_with_seeded_store() -> (axum::Router, Arc<dyn EventStore>, tempfile::TempDir) {
        let tmp = tempfile::tempdir().unwrap();
        let store_state = StoreState::new(tmp.path()).unwrap();
        let event_store = store_state.event_store.clone();
        for name in ["two_arcs_gap", "ambiguous_closure_opening"] {
            let events = golden_events(name);
            let sid = events[0]["data"]["session_id"]
                .as_str()
                .unwrap()
                .to_string();
            event_store.insert_batch(&sid, &events).await.unwrap();
            let row: SessionRow = serde_json::from_value(
                serde_json::json!({ "id": sid, "event_count": events.len() }),
            )
            .unwrap();
            event_store.upsert_session(&row).await.unwrap();
        }
        let (broadcast_tx, _) = broadcast::channel(256);
        let mut config = Config::default();
        config.api_token = "api-secret".to_string();
        config.admin_token = "admin-secret".to_string();
        config.local_principal_id = "test-principal".to_string();
        let initial_topology = open_story_server::admin::compute_topology(
            "test-host",
            config.role,
            &open_story_server::admin::EnvInputs::default(),
            &[],
        );
        let (admin_topology_tx, _) = tokio::sync::watch::channel(initial_topology);
        let state = Arc::new(RwLock::new(AppState {
            store: store_state,
            transcript_states: HashMap::new(),
            watcher_diagnostics: WatcherDiagnostics::default(),
            broadcast_tx,
            bus: Arc::new(NoopBus),
            admin_topology_tx,
            config: config.clone(),
            watch_dir: tmp.path().join("watch"),
            account_config_writer: None,
            account_config_reloader: None,
            role_directory: {
                use open_story_server::directory::{
                    EmbeddedRoleDirectory, Participant, Role, RoleDirectory,
                };
                let dir = EmbeddedRoleDirectory::in_memory().unwrap();
                dir.upsert_participant(Participant {
                    principal_id: "test-principal".into(),
                    person_id: "max".into(),
                    role: Role::Admin,
                    created_at: "2026-09-18T00:00:00Z".into(),
                })
                .await
                .unwrap();
                Arc::new(dir)
            },
        }));
        (build_router(state, None, &config), event_store, tmp)
    }

    #[tokio::test]
    async fn it_folds_every_session_and_persists_story_patterns() {
        let (router, store, _tmp) = app_with_seeded_store().await;
        let req = Request::post("/api/admin/story-backfill?write=true")
            .header("authorization", "Bearer admin-secret")
            .body(Body::empty())
            .unwrap();
        let resp = router.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1 << 20)
            .await
            .unwrap();
        let report: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(report["sessions"], 2);
        assert_eq!(report["arcs"], 3);
        assert_eq!(report["exchanges"], 8);
        assert_eq!(report["patterns_written"], 11);

        let sid = golden_events("two_arcs_gap")[0]["data"]["session_id"]
            .as_str()
            .unwrap()
            .to_string();
        let arcs = store
            .session_patterns(&sid, Some("story.arc"))
            .await
            .unwrap();
        assert_eq!(arcs.len(), 2, "story patterns landed in the store");
    }

    #[tokio::test]
    async fn without_write_it_only_reports() {
        let (router, store, _tmp) = app_with_seeded_store().await;
        let req = Request::post("/api/admin/story-backfill")
            .header("authorization", "Bearer admin-secret")
            .body(Body::empty())
            .unwrap();
        let resp = router.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let sid = golden_events("two_arcs_gap")[0]["data"]["session_id"]
            .as_str()
            .unwrap()
            .to_string();
        assert!(
            store
                .session_patterns(&sid, Some("story.arc"))
                .await
                .unwrap()
                .is_empty(),
            "dry run writes nothing"
        );
    }
}
