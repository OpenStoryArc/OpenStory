//! Self-test for the `TestActors` helper (Phase 1 commit 1.2).
//!
//! Proves the helper drives events through the four-actor pipeline
//! synchronously and reports consistent results across all four:
//!   - PersistConsumer writes the events to SQLite
//!   - ProjectionsConsumer populates the shared DashMap on AppState
//!   - BroadcastConsumer returns BroadcastMessages
//!
//! When commit 1.6 migrates all 15 test files away from `ingest_events`
//! to `TestActors::drive_batch`, those migrated tests rely on the
//! invariants pinned here.

mod helpers;

use helpers::bus::TestActors;
use helpers::make_event_with_id;
use tempfile::TempDir;

#[tokio::test]
async fn drive_batch_persists_events_and_populates_projection() {
    let tmp = TempDir::new().unwrap();
    let mut actors = TestActors::new(&tmp).await;

    let events = vec![
        make_event_with_id("io.arc.event", "sess-actors", "evt-test-1"),
        make_event_with_id("io.arc.event", "sess-actors", "evt-test-2"),
    ];

    let result = actors.drive_batch("sess-actors", &events, None).await;

    // PersistConsumer wrote events.
    let persisted = actors
        .state
        .read()
        .await
        .store
        .event_store
        .session_events("sess-actors")
        .await
        .unwrap();
    assert!(!persisted.is_empty(), "events should be persisted");

    // ProjectionsConsumer populated the shared map.
    assert!(
        actors
            .state
            .read()
            .await
            .store
            .projections
            .contains_key("sess-actors"),
        "projection should be present in shared DashMap"
    );

    // Broadcast returned something (or at worst an empty vec, but no panic).
    // Events with text payloads should yield at least one message.
    assert!(
        !result.messages.is_empty() || persisted.is_empty(),
        "if events persisted, broadcast should produce at least one message"
    );
}

#[tokio::test]
async fn drive_batch_dedup_is_reported_via_persist_result() {
    // PersistConsumer uses EventStore PK dedup. Feeding the same event
    // twice should result in one `persisted` + one `skipped` (or two
    // insertions in separate batches — this is about the skip count
    // on the second call).
    let tmp = TempDir::new().unwrap();
    let mut actors = TestActors::new(&tmp).await;

    let event = make_event_with_id("io.arc.event", "sess-dup", "evt-dup-1");
    let events = vec![event.clone()];

    let first = actors.drive_batch("sess-dup", &events, None).await;
    assert_eq!(first.persisted, 1, "first call should persist 1 event");
    assert_eq!(first.skipped, 0, "first call should skip 0 events");

    let second = actors.drive_batch("sess-dup", &events, None).await;
    assert_eq!(second.persisted, 0, "second call should persist 0 new events");
    assert_eq!(second.skipped, 1, "second call should skip 1 event via PK dedup");
}

#[tokio::test]
async fn drive_batch_returns_empty_for_empty_input() {
    let tmp = TempDir::new().unwrap();
    let mut actors = TestActors::new(&tmp).await;

    let result = actors.drive_batch("sess-empty", &[], None).await;
    assert!(result.messages.is_empty());
    assert_eq!(result.persisted, 0);
    assert_eq!(result.skipped, 0);
}
