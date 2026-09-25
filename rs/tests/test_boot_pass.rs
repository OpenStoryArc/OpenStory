//! The boot pass, bounded (row B-02 of
//! docs/research/openstory-as-node/2026-09-25-boot-pass-memory.md).
//!
//! At boot the node derives two facts per stored session — a subagent's
//! parent and the working directory that names its project. It must ask
//! the store for those facts (`session_boot_facts`) and never for a
//! session's event list (`session_events`): on a fleet store the event
//! lists alone drove memory past 3 GB before replay began.
//!
//! Run with: cargo test -p open-story --test test_boot_pass

mod helpers;

use std::sync::Arc;

use serde_json::json;
use tempfile::TempDir;

use helpers::recording_store::RecordingStore;
use open_story::server::{create_state_with_store, Config, SharedState};
use open_story_bus::noop_bus::NoopBus;
use open_story_store::analysis;
use open_story_store::event_store::{EventStore, SessionRow};
use open_story_store::sqlite_store::SqliteStore;
use open_story_store::state::StoreState;

const CWD: &str = "/work/proj-a";

fn event(id: &str, data_session_id: &str, time: &str, cwd: Option<&str>) -> serde_json::Value {
    let mut raw = json!({"type": "user"});
    if let Some(cwd) = cwd {
        raw["cwd"] = json!(cwd);
    }
    json!({
        "id": id,
        "type": "io.arc.event",
        "subtype": "message.user.prompt",
        "source": "arc://test",
        "time": time,
        "data": {"seq": 1, "session_id": data_session_id, "raw": raw}
    })
}

fn row(id: &str) -> SessionRow {
    // Thirty days old: outside the default working set, so the boot-time
    // reproject leaves these sessions to the boot pass alone.
    let old = (chrono::Utc::now() - chrono::Duration::days(30)).to_rfc3339();
    SessionRow {
        id: id.to_string(),
        project_id: None,
        project_name: None,
        label: None,
        custom_label: None,
        branch: None,
        event_count: 1,
        first_event: Some(old.clone()),
        last_event: Some(old),
        host: None,
        user: None,
        origin_agent: None,
        person_id: None,
        principal_id: None,
    }
}

/// A store with three sessions from a previous run — a parent with a cwd,
/// its subagent (events name the parent), and a solo session with neither
/// fact — then a boot through the recording wrapper.
async fn boot_recorded(tmp: &TempDir) -> (SharedState, Arc<RecordingStore>) {
    let data_dir = tmp.path().join("data");
    let watch_dir = tmp.path().join("watch");
    std::fs::create_dir_all(&data_dir).unwrap();
    std::fs::create_dir_all(&watch_dir).unwrap();
    {
        let db = SqliteStore::new(&data_dir).unwrap();
        db.insert_event(
            "parent-1",
            &event("p-1", "parent-1", "2025-01-14T10:00:00Z", Some(CWD)),
        )
        .await
        .unwrap();
        db.insert_event(
            "child-1",
            &event("c-1", "parent-1", "2025-01-14T10:00:01Z", Some(CWD)),
        )
        .await
        .unwrap();
        db.insert_event(
            "solo-1",
            &event("s-1", "solo-1", "2025-01-14T10:00:02Z", None),
        )
        .await
        .unwrap();
        for id in ["parent-1", "child-1", "solo-1"] {
            db.upsert_session(&row(id)).await.unwrap();
        }
    }
    let mut store = StoreState::new(&data_dir).unwrap();
    let recording = RecordingStore::wrap(store.event_store.clone());
    store.event_store = recording.clone();
    let state = create_state_with_store(
        store,
        &data_dir,
        &[watch_dir],
        Arc::new(NoopBus),
        Config::default(),
    )
    .await
    .unwrap();
    (state, recording)
}

mod when_the_state_boots_from_a_store_with_sessions {
    use super::*;

    #[tokio::test]
    async fn it_asks_for_boot_facts_and_never_for_session_events() {
        let tmp = TempDir::new().unwrap();
        let (_state, store) = boot_recorded(&tmp).await;

        assert_eq!(
            store.calls("session_events"),
            0,
            "the boot pass must not load event lists; calls seen: {:?}",
            store.all_calls()
        );
        assert_eq!(
            store.calls("session_boot_facts"),
            3,
            "one boot-facts query per stored session; calls seen: {:?}",
            store.all_calls()
        );
    }

    #[tokio::test]
    async fn it_still_knows_the_parents_and_the_projects() {
        let tmp = TempDir::new().unwrap();
        let (state, _store) = boot_recorded(&tmp).await;
        let s = state.read().await;

        assert_eq!(
            s.store
                .subagent_parents
                .get("child-1")
                .map(|r| r.value().clone()),
            Some("parent-1".to_string())
        );
        assert_eq!(
            s.store
                .session_children
                .get("parent-1")
                .map(|r| r.value().clone()),
            Some(vec!["child-1".to_string()])
        );
        assert!(!s.store.subagent_parents.contains_key("parent-1"));
        assert!(!s.store.subagent_parents.contains_key("solo-1"));

        let resolved = analysis::resolve_project(CWD, &s.store.watch_dir_entries);
        for id in ["parent-1", "child-1"] {
            assert_eq!(
                s.store.session_projects.get(id).map(|r| r.value().clone()),
                Some(resolved.project_id.clone()),
                "{id} resolves its project from the cwd"
            );
            assert_eq!(
                s.store
                    .session_project_names
                    .get(id)
                    .map(|r| r.value().clone()),
                Some(resolved.project_name.clone())
            );
        }
        assert!(
            !s.store.session_projects.contains_key("solo-1"),
            "no cwd, no project"
        );
    }
}
