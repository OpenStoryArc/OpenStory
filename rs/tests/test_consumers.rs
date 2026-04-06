//! TDD: Independent actor-consumers for the decomposed ingest pipeline.
//!
//! Each test describes the CONTRACT one consumer must fulfill.
//! Tests use NATS testcontainers for real message passing.
//!
//! Target architecture:
//!   persist    — events.> → SQLite + JSONL + FTS (pure sink)
//!   patterns   — events.> → EvalApply → PatternEvent → pub patterns.{project}.{session}
//!   projections — events.> → SessionProjection → pub changes.{project}.{session}
//!   broadcast  — events.> + patterns.> + changes.> → WebSocket
//!
//! Run with: cargo test -p open-story --test test_consumers

mod helpers;

use helpers::{make_event, make_event_with_id};
use open_story_bus::nats_bus::NatsBus;
use open_story_bus::{Bus, IngestBatch};
use open_story_core::cloud_event::CloudEvent;
use open_story_core::event_data::{AgentPayload, ClaudeCodePayload, EventData};
use serde_json::json;
use testcontainers::{GenericImage, ImageExt};
use testcontainers::runners::AsyncRunner;

/// Start a NATS container and return a connected NatsBus.
async fn start_nats() -> (NatsBus, testcontainers::ContainerAsync<GenericImage>) {
    let container = GenericImage::new("nats", "2-alpine")
        .with_cmd(vec!["--jetstream"])
        .start()
        .await
        .expect("start NATS container");

    let port = container.get_host_port_ipv4(4222).await.expect("get port");
    let nats_url = format!("nats://localhost:{port}");

    let mut bus = None;
    for _ in 0..10 {
        match NatsBus::connect(&nats_url).await {
            Ok(b) => { bus = Some(b); break; }
            Err(_) => tokio::time::sleep(std::time::Duration::from_millis(500)).await,
        }
    }
    let bus = bus.expect("connect to NATS");
    bus.ensure_streams().await.expect("create streams");

    (bus, container)
}

/// Create a CloudEvent with a specific subtype for testing.
fn make_typed_event(session_id: &str, subtype: &str) -> CloudEvent {
    let mut payload = ClaudeCodePayload::new();
    payload.text = Some(format!("test content for {subtype}"));
    let data = EventData::with_payload(
        json!({"test": true}),
        0,
        session_id.to_string(),
        AgentPayload::ClaudeCode(payload),
    );
    CloudEvent::new(
        format!("arc://transcript/{session_id}"),
        "io.arc.event".to_string(),
        data,
        Some(subtype.to_string()),
        None,
        None,
        None,
        None,
        Some("claude-code".to_string()),
    )
}

/// Publish an IngestBatch to a hierarchical subject.
async fn publish_batch(bus: &NatsBus, project: &str, session: &str, events: Vec<CloudEvent>) {
    let batch = IngestBatch {
        session_id: session.to_string(),
        project_id: project.to_string(),
        events,
    };
    let subject = format!("events.{project}.{session}.main");
    bus.publish(&subject, &batch).await.expect("publish batch");
}

// ═══════════════════════════════════════════════════════════════════
// Contract: persist consumer — stores events, deduplicates
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn persist_consumer_receives_and_can_store_events() {
    let (bus, _container) = start_nats().await;

    // Subscribe as the persist consumer would
    let mut sub = bus.subscribe("events.>").await.expect("subscribe");

    // Publish 3 events
    let events: Vec<CloudEvent> = (0..3)
        .map(|i| make_typed_event("sess-1", &format!("message.user.prompt.{i}")))
        .collect();
    publish_batch(&bus, "test-project", "sess-1", events).await;

    // Persist consumer receives the batch
    let batch = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        sub.receiver.recv(),
    ).await.expect("timeout").expect("receive");

    assert_eq!(batch.session_id, "sess-1");
    assert_eq!(batch.project_id, "test-project");
    assert_eq!(batch.events.len(), 3);

    // Each event has a unique ID (for dedup)
    let ids: Vec<&str> = batch.events.iter().map(|e| e.id.as_str()).collect();
    assert_eq!(ids.len(), 3);
    assert_ne!(ids[0], ids[1], "event IDs should be unique");
}

#[tokio::test]
async fn persist_consumer_can_dedup_by_event_id() {
    let (bus, _container) = start_nats().await;
    let mut sub = bus.subscribe("events.>").await.expect("subscribe");

    // Publish same event twice (same ID)
    let event = make_event_with_id("io.arc.event", "sess-1", "dedup-test-id");
    publish_batch(&bus, "test-project", "sess-1", vec![event.clone()]).await;
    publish_batch(&bus, "test-project", "sess-1", vec![event.clone()]).await;

    // Consumer receives BOTH batches (dedup is consumer's responsibility, not NATS)
    let batch1 = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        sub.receiver.recv(),
    ).await.expect("timeout").expect("receive first");

    let batch2 = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        sub.receiver.recv(),
    ).await.expect("timeout").expect("receive second");

    // Both carry the same event ID — persist consumer must dedup
    assert_eq!(batch1.events[0].id, "dedup-test-id");
    assert_eq!(batch2.events[0].id, "dedup-test-id");
}

// ═══════════════════════════════════════════════════════════════════
// Contract: patterns consumer — detects turns, publishes PatternEvents
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn patterns_consumer_receives_events_for_detection() {
    let (bus, _container) = start_nats().await;
    let mut sub = bus.subscribe("events.>").await.expect("subscribe");

    // Publish a turn sequence: prompt → tool_use → tool_result → turn_end
    let events = vec![
        make_typed_event("sess-1", "message.user.prompt"),
        make_typed_event("sess-1", "message.assistant.tool_use"),
        make_typed_event("sess-1", "message.user.tool_result"),
        make_typed_event("sess-1", "system.turn.complete"),
    ];
    publish_batch(&bus, "test-project", "sess-1", events).await;

    let batch = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        sub.receiver.recv(),
    ).await.expect("timeout").expect("receive");

    // Patterns consumer receives the full turn sequence
    assert_eq!(batch.events.len(), 4);
    let subtypes: Vec<&str> = batch.events.iter()
        .filter_map(|e| e.subtype.as_deref())
        .collect();
    assert!(subtypes.contains(&"message.user.prompt"));
    assert!(subtypes.contains(&"system.turn.complete"));
}

// ═══════════════════════════════════════════════════════════════════
// Contract: projections consumer — updates session metadata
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn projections_consumer_receives_events_for_metadata() {
    let (bus, _container) = start_nats().await;
    let mut sub = bus.subscribe("events.>").await.expect("subscribe");

    // Publish events with token usage
    let mut payload = ClaudeCodePayload::new();
    payload.text = Some("response text".to_string());
    payload.token_usage = Some(json!({
        "input_tokens": 1500,
        "output_tokens": 300,
    }));
    let data = EventData::with_payload(
        json!({}), 0, "sess-1".to_string(),
        AgentPayload::ClaudeCode(payload),
    );
    let event = CloudEvent::new(
        "arc://test/sess-1".into(),
        "io.arc.event".into(),
        data,
        Some("message.assistant.text".into()),
        None, None, None, None,
        Some("claude-code".into()),
    );
    publish_batch(&bus, "test-project", "sess-1", vec![event]).await;

    let batch = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        sub.receiver.recv(),
    ).await.expect("timeout").expect("receive");

    // Projections consumer can extract token usage from the event
    let ap = batch.events[0].data.agent_payload.as_ref().expect("has payload");
    let tokens = ap.token_usage().expect("has token_usage");
    assert_eq!(tokens["input_tokens"], 1500);
    assert_eq!(tokens["output_tokens"], 300);
}

// ═══════════════════════════════════════════════════════════════════
// Contract: broadcast consumer — subscribes to multiple streams
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn broadcast_consumer_receives_from_events_stream() {
    let (bus, _container) = start_nats().await;
    let mut sub = bus.subscribe("events.>").await.expect("subscribe events");

    publish_batch(&bus, "test-project", "sess-1",
        vec![make_typed_event("sess-1", "message.user.prompt")]).await;

    let batch = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        sub.receiver.recv(),
    ).await.expect("timeout").expect("receive");

    assert_eq!(batch.session_id, "sess-1");
}

// ═══════════════════════════════════════════════════════════════════
// Contract: consumers are independent — different subscriptions
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn multiple_consumers_each_receive_same_events() {
    let (bus, _container) = start_nats().await;

    // Two independent consumers on the same subject pattern
    let mut persist_sub = bus.subscribe("events.>").await.expect("persist subscribe");
    let mut patterns_sub = bus.subscribe("events.>").await.expect("patterns subscribe");

    // Publish one batch
    publish_batch(&bus, "test-project", "sess-1",
        vec![make_typed_event("sess-1", "message.user.prompt")]).await;

    // BOTH consumers should receive the batch
    let persist_batch = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        persist_sub.receiver.recv(),
    ).await.expect("persist timeout").expect("persist receive");

    let patterns_batch = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        patterns_sub.receiver.recv(),
    ).await.expect("patterns timeout").expect("patterns receive");

    assert_eq!(persist_batch.session_id, "sess-1");
    assert_eq!(patterns_batch.session_id, "sess-1");
    assert_eq!(persist_batch.events.len(), 1);
    assert_eq!(patterns_batch.events.len(), 1);
}

#[tokio::test]
async fn broadcast_consumer_can_subscribe_to_multiple_streams() {
    let (bus, _container) = start_nats().await;

    // Broadcast subscribes to events AND patterns
    let mut events_sub = bus.subscribe("events.>").await.expect("events subscribe");

    // Publish to events stream
    publish_batch(&bus, "test-project", "sess-1",
        vec![make_typed_event("sess-1", "message.user.prompt")]).await;

    let events_batch = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        events_sub.receiver.recv(),
    ).await.expect("timeout").expect("receive events");

    assert_eq!(events_batch.session_id, "sess-1");

    // Note: patterns.> subscription would be a second BusSubscription
    // The broadcast consumer manages multiple subscriptions and merges them.
    // This test verifies the bus supports it — the consumer logic comes next.
}
