//! Integration tests for event deduplication.

mod helpers;

use std::sync::Arc;

use helpers::{make_event_with_id, test_state};
use tempfile::TempDir;

use open_story::server::ingest_events;
use open_story_bus::noop_bus::NoopBus;

#[tokio::test]
async fn test_ingest_same_batch_twice() {
    let data_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);

    let events = vec![
        make_event_with_id("io.arc.event", "sess-1", "evt-aaa"),
        make_event_with_id("io.arc.event", "sess-1", "evt-bbb"),
    ];

    let mut s = state.write().await;

    let first = ingest_events(&mut s, "sess-1", &events, None);
    assert_eq!(first.count, 2);

    // Same events again — should all be deduplicated
    let second = ingest_events(&mut s, "sess-1", &events, None);
    assert_eq!(second.count, 0);

    assert_eq!(s.store.event_store.session_events("sess-1").unwrap().len(), 2);
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
    let result = ingest_events(&mut s, "sess-1", &events, None);
    assert_eq!(result.count, 1);

    // Simulate same event coming through a different path (e.g., watcher)
    let result = ingest_events(&mut s, "sess-1", &events, None);
    assert_eq!(result.count, 0);

    // Only 1 event stored
    assert_eq!(s.store.event_store.session_events("sess-1").unwrap().len(), 1);
}

#[tokio::test]
async fn test_seen_ids_loaded_from_persistence() {
    let data_dir = TempDir::new().unwrap();

    // Phase 1: ingest events into state backed by temp dir
    {
        let state = test_state(&data_dir);
        let events = vec![
            make_event_with_id("io.arc.event", "sess-1", "evt-persist-1"),
            make_event_with_id("io.arc.event", "sess-1", "evt-persist-2"),
        ];
        let mut s = state.write().await;
        let result = ingest_events(&mut s, "sess-1", &events, None);
        assert_eq!(result.count, 2);
    }

    // Phase 2: create new state from same directory — IDs should be loaded
    let state2 = open_story::server::create_state(data_dir.path(), data_dir.path(), Arc::new(NoopBus), Arc::new(open_story_semantic::NoopSemanticStore), open_story::server::Config::default()).unwrap();
    let mut s2 = state2.write().await;

    // These events already exist in persistence — should be deduplicated
    let events = vec![
        make_event_with_id("io.arc.event", "sess-1", "evt-persist-1"),
        make_event_with_id("io.arc.event", "sess-1", "evt-persist-2"),
    ];
    let result = ingest_events(&mut s2, "sess-1", &events, None);
    assert_eq!(result.count, 0);

    // But a new event should succeed
    let new_events = vec![
        make_event_with_id("io.arc.event", "sess-1", "evt-persist-3"),
    ];
    let result = ingest_events(&mut s2, "sess-1", &new_events, None);
    assert_eq!(result.count, 1);
}
