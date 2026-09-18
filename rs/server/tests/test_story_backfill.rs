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
