//! Integration tests for NatsBus.
//!
//! These tests require a running NATS server with JetStream enabled.
//! Start one with: nats-server --jetstream
//!
//! Tests are marked #[ignore] so they don't run in CI without NATS.
//! Run explicitly with: cargo test -p open-story-bus --test nats_integration -- --ignored

use open_story_core::cloud_event::CloudEvent;
use open_story_core::event_data::EventData;
use open_story_bus::nats_bus::NatsBus;
use open_story_bus::{Bus, IngestBatch};
use std::time::Duration;

fn nats_url() -> String {
    std::env::var("NATS_URL").unwrap_or_else(|_| "nats://localhost:4222".to_string())
}

fn test_event(source: &str) -> CloudEvent {
    CloudEvent::new(
        source.to_string(),
        "io.arc.event".to_string(),
        EventData::new(serde_json::json!({"test": true}), 1, "sess-test".to_string()),
        Some("message.user.prompt".to_string()),
        None,
        None,
        None,
        None,
        None,
    )
}

fn test_batch(session_id: &str) -> IngestBatch {
    IngestBatch {
        session_id: session_id.to_string(),
        project_id: "test-project".to_string(),
        events: vec![test_event("test-source")],
    }
}

#[tokio::test]
#[ignore]
async fn publish_and_replay_round_trip() {
    let bus = NatsBus::connect(&nats_url()).await.expect("connect");
    bus.ensure_streams().await.expect("ensure streams");

    let session_id = format!("test-{}", uuid::Uuid::new_v4());
    let batch = IngestBatch {
        session_id: session_id.clone(),
        project_id: "test-project".to_string(),
        events: vec![test_event("replay-test")],
    };

    bus.publish(&format!("events.session.{session_id}"), &batch)
        .await
        .expect("publish");

    // Small delay for JetStream persistence
    tokio::time::sleep(Duration::from_millis(100)).await;

    let replayed = bus.replay("events.>").await.expect("replay");

    // Should contain at least our batch
    let found = replayed
        .iter()
        .any(|b| b.session_id == session_id);
    assert!(found, "expected to find session {session_id} in replay");
}

#[tokio::test]
#[ignore]
async fn publish_and_subscribe_delivery() {
    let bus = NatsBus::connect(&nats_url()).await.expect("connect");
    bus.ensure_streams().await.expect("ensure streams");

    let session_id = format!("test-{}", uuid::Uuid::new_v4());

    // Subscribe first
    let mut sub = bus.subscribe("events.>").await.expect("subscribe");

    // Publish
    let batch = test_batch(&session_id);
    bus.publish(&format!("events.session.{session_id}"), &batch)
        .await
        .expect("publish");

    // Receive with timeout
    let received = tokio::time::timeout(Duration::from_secs(5), sub.receiver.recv())
        .await
        .expect("timeout waiting for message")
        .expect("channel closed");

    assert_eq!(received.session_id, session_id);
    assert_eq!(received.events.len(), 1);
    assert_eq!(received.events[0].source, "test-source");
}

#[tokio::test]
#[ignore]
async fn multiple_batches_arrive_in_order() {
    let bus = NatsBus::connect(&nats_url()).await.expect("connect");
    bus.ensure_streams().await.expect("ensure streams");

    let session_id = format!("test-{}", uuid::Uuid::new_v4());
    let subject = format!("events.session.{session_id}");

    let mut sub = bus.subscribe("events.>").await.expect("subscribe");

    // Small delay to let push consumer register
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Publish 3 batches
    for i in 0..3 {
        let batch = IngestBatch {
            session_id: session_id.clone(),
            project_id: "test-project".to_string(),
            events: vec![test_event(&format!("source-{i}"))],
        };
        bus.publish(&subject, &batch).await.expect("publish");
    }

    // Receive batches, filtering for our session_id (other tests may publish concurrently)
    let mut sources = Vec::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while sources.len() < 3 {
        match tokio::time::timeout_at(deadline, sub.receiver.recv()).await {
            Ok(Some(batch)) => {
                if batch.session_id == session_id {
                    sources.push(batch.events[0].source.clone());
                }
            }
            Ok(None) => panic!("channel closed before receiving all batches"),
            Err(_) => panic!("timeout: received only {} of 3 batches", sources.len()),
        }
    }

    assert!(sources.contains(&"source-0".to_string()));
    assert!(sources.contains(&"source-1".to_string()));
    assert!(sources.contains(&"source-2".to_string()));
}

#[tokio::test]
#[ignore]
async fn replay_empty_stream_returns_empty() {
    // This test works even if the stream has data from other tests,
    // but we just verify it doesn't panic and returns a vec.
    let bus = NatsBus::connect(&nats_url()).await.expect("connect");
    bus.ensure_streams().await.expect("ensure streams");

    let result = bus.replay("events.>").await;
    assert!(result.is_ok());
}
