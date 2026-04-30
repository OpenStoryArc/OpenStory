//! Integration tests for event deduplication.

mod helpers;

use helpers::{make_event_with_id, test_state};
use tempfile::TempDir;

use open_story::server::ingest_events;

#[tokio::test]
async fn test_ingest_same_batch_twice() {
    let data_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);

    let events = vec![
        make_event_with_id("io.arc.event", "sess-1", "evt-aaa"),
        make_event_with_id("io.arc.event", "sess-1", "evt-bbb"),
    ];

    let mut s = state.write().await;

    let first = ingest_events(&mut s, "sess-1", &events, None).await;
    assert_eq!(first.count, 2);

    // Same events again — should all be deduplicated
    let second = ingest_events(&mut s, "sess-1", &events, None).await;
    assert_eq!(second.count, 0);

    assert_eq!(s.store.event_store.session_events("sess-1").await.unwrap().len(), 2);
}

#[tokio::test]
async fn test_dedup_across_hook_and_ingest() {
    let data_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);

    let events = vec![
        make_event_with_id("io.arc.event", "sess-1", "evt-xxx"),
    ];

    let mut s = state.write().await;

    // First ingest
    let result = ingest_events(&mut s, "sess-1", &events, None).await;
    assert_eq!(result.count, 1);

    // Simulate same event coming through a different path (e.g., watcher)
    let result = ingest_events(&mut s, "sess-1", &events, None).await;
    assert_eq!(result.count, 0);

    // Only 1 event stored
    assert_eq!(s.store.event_store.session_events("sess-1").await.unwrap().len(), 1);
}

// `test_seen_ids_loaded_from_persistence` retired — it asserted that
// `create_state()` would re-populate an in-memory seen-IDs HashSet by
// replaying JSONL on boot, so a re-ingest of the same event IDs would
// dedup to count=0. That `boot_from_jsonl` path + the in-memory
// `seen_event_ids` HashSet were both removed in commit 5d936fe. Dedup is
// now solely the EventStore PK's job, exercised by
// `consumers::persist::tests::dedup_*` (via SqliteStore PK constraint)
// and the SQLite-boot tests in `rs/server/src/state.rs::tests::
// boot_from_sqlite_when_db_has_sessions`. Same retirement note appears in
// `state.rs` for `create_state_tracks_all_event_ids_for_dedup` (lines
// 199-203); this one was missed in the cleanup.
