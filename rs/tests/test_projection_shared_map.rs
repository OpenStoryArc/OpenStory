//! Commit 1.3 landing test: the ProjectionsConsumer writes into the
//! shared `Arc<DashMap>` on `StoreState.projections`. Before 1.3 the
//! consumer kept its own private HashMap that nothing in production read
//! (dead-code state characterized in `rs/server/src/consumers/projections.rs`
//! test comments). After 1.3, every update the consumer makes is
//! immediately visible via the shared map — no sync step, no bridge.
//!
//! This mirrors the production wiring at `rs/src/server/mod.rs:228-247`
//! but without the NATS subscribe loop — we call `process_batch`
//! directly to keep the test fast and deterministic.

mod helpers;

use std::sync::Arc;

use dashmap::DashMap;
use helpers::{make_event_with_id, test_state};
use open_story::server::consumers::projections::ProjectionsConsumer;
use open_story_store::projection::SessionProjection;
use tempfile::TempDir;

#[tokio::test]
async fn consumer_writes_are_visible_on_store_state_projections() {
    let tmp = TempDir::new().unwrap();
    let state = test_state(&tmp);

    // Production wiring: hand the shared maps to the consumer.
    let (shared, parents, children): (
        Arc<DashMap<String, SessionProjection>>,
        Arc<DashMap<String, String>>,
        Arc<DashMap<String, Vec<String>>>,
    ) = {
        let s = state.read().await;
        (
            s.store.projections.clone(),
            s.store.subagent_parents.clone(),
            s.store.session_children.clone(),
        )
    };
    let mut consumer = ProjectionsConsumer::new(shared.clone(), parents, children);

    let events = vec![
        make_event_with_id("message.user.prompt", "sess-shared", "evt-1"),
        make_event_with_id("message.user.prompt", "sess-shared", "evt-2"),
    ];
    consumer.process_batch("sess-shared", &events);

    // Reading via the server's AppState sees the consumer's write.
    let s = state.read().await;
    let entry = s
        .store
        .projections
        .get("sess-shared")
        .expect("projection should be populated via shared DashMap");
    assert_eq!(entry.value().event_count(), 2);
}

#[tokio::test]
async fn api_layer_observes_consumer_updates_without_sync_step() {
    // Models the real read path: API handler reads `state.store.projections.get(sid)`
    // while Actor 3 writes to the same map. Prior to 1.3 the consumer
    // wrote to its internal map and the API saw nothing.
    let tmp = TempDir::new().unwrap();
    let state = test_state(&tmp);

    let (shared, parents, children) = {
        let s = state.read().await;
        (
            s.store.projections.clone(),
            s.store.subagent_parents.clone(),
            s.store.session_children.clone(),
        )
    };
    let mut consumer = ProjectionsConsumer::new(shared, parents, children);

    consumer.process_batch(
        "sess-api",
        &[make_event_with_id("message.user.prompt", "sess-api", "evt-a")],
    );

    // An API-shaped read (clone of the Arc, get by session_id) returns
    // the projection without reaching into ProjectionsConsumer at all.
    let api_view = state.read().await.store.projections.clone();
    assert!(
        api_view.contains_key("sess-api"),
        "API-side read of the shared map should observe the write without a sync step"
    );
}
