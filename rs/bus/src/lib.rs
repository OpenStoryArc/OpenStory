//! Event bus trait and NATS implementation for open-story.
//!
//! The `Bus` trait defines the architectural boundary between event producers
//! (listener, hooks) and consumers (store). All events flow through the bus —
//! producers and consumers never communicate directly.
//!
//! Default implementation: `NatsBus` (NATS JetStream).

pub mod nats_bus;
pub mod noop_bus;

use anyhow::Result;
use open_story_core::cloud_event::CloudEvent;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

/// A batch of events to publish or received from the bus.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct IngestBatch {
    pub session_id: String,
    pub project_id: String,
    pub events: Vec<CloudEvent>,
}

/// A subscription handle that receives event batches from the bus.
pub struct BusSubscription {
    pub receiver: mpsc::Receiver<IngestBatch>,
}

/// The architectural boundary between event producers and consumers.
///
/// All events in open-story flow through a `Bus` implementation. The trait
/// enforces that producers (listener, hooks) and consumers (store) communicate
/// only through publish/subscribe — never via direct function calls.
///
/// This is the ACL (anti-corruption layer). The transport underneath is pluggable:
/// NatsBus for production (durable streams, fan-out), FileBus for constrained
/// environments (future).
#[async_trait]
pub trait Bus: Send + Sync + 'static {
    /// Publish an event batch to a subject (e.g., "events.session.{session_id}").
    async fn publish(&self, subject: &str, batch: &IngestBatch) -> Result<()>;

    /// Publish raw bytes to a subject (e.g., "changes.store.{session_id}").
    ///
    /// Used for store change notifications — the payload is serialized JSON
    /// that doesn't conform to IngestBatch. Consumers deserialize as needed.
    async fn publish_bytes(&self, subject: &str, data: &[u8]) -> Result<()>;

    /// Subscribe to subjects matching a pattern (e.g., "events.>").
    /// Returns a BusSubscription that yields batches as they arrive.
    async fn subscribe(&self, pattern: &str) -> Result<BusSubscription>;

    /// Replay all historical events matching a pattern.
    /// Used for boot recovery — rebuilds store state from the event log.
    async fn replay(&self, pattern: &str) -> Result<Vec<IngestBatch>>;

    /// Whether this bus implementation is active (connected to real transport).
    /// Returns false for NoopBus. Used to decide whether to route events
    /// through the bus or use direct ingest as fallback.
    fn is_active(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ingest_batch_serialization_round_trip() {
        let batch = IngestBatch {
            session_id: "abc-123".to_string(),
            project_id: "proj-1".to_string(),
            events: vec![],
        };

        let json = serde_json::to_string(&batch).unwrap();
        let deserialized: IngestBatch = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.session_id, "abc-123");
        assert_eq!(deserialized.project_id, "proj-1");
        assert_eq!(deserialized.events.len(), 0);
    }

    #[tokio::test]
    async fn noop_bus_publish_bytes_succeeds() {
        let bus = noop_bus::NoopBus;
        let result = bus.publish_bytes("changes.store.test", b"{}").await;
        assert!(result.is_ok());
    }

    // ── NoopBus contract tests (audit walk #6) ────────────────────────
    //
    // NoopBus is the test-default Bus. Several call sites depend on
    // specific behaviors:
    //   - publish() succeeds silently (test code can ignore Result)
    //   - subscribe() returns a never-receiving channel (test code can
    //     hold a subscription without expecting events)
    //   - replay() returns empty (boot-replay paths reach the "no
    //     historical events" branch without erroring)
    //   - is_active() is false (callers branch on this to avoid the
    //     bus when running standalone)
    //
    // None of these were directly tested before this commit.

    #[tokio::test]
    async fn noop_bus_publish_succeeds_silently() {
        let bus = noop_bus::NoopBus;
        let batch = IngestBatch {
            session_id: "s".into(),
            project_id: "p".into(),
            events: vec![],
        };
        assert!(bus.publish("events.s", &batch).await.is_ok());
    }

    #[tokio::test]
    async fn noop_bus_subscribe_returns_never_receiving_channel() {
        let bus = noop_bus::NoopBus;
        let mut sub = bus.subscribe("events.>").await.expect("noop subscribe always Ok");
        // The channel sender side is dropped immediately, so try_recv
        // returns Disconnected — never any events.
        assert!(sub.receiver.try_recv().is_err());
    }

    #[tokio::test]
    async fn noop_bus_replay_returns_empty_vec() {
        let bus = noop_bus::NoopBus;
        let batches = bus.replay("events.>").await.expect("noop replay always Ok");
        assert!(batches.is_empty());
    }

    #[test]
    fn noop_bus_is_active_returns_false() {
        // Callers branch on is_active() to decide whether to skip the
        // bus entirely (e.g., direct ingest in tests). Locking this in.
        let bus = noop_bus::NoopBus;
        assert!(!bus.is_active());
    }

    #[test]
    fn ingest_batch_with_events_round_trip() {
        use open_story_core::cloud_event::CloudEvent;
        use open_story_core::event_data::EventData;

        let event = CloudEvent::new(
            "test-source".to_string(),
            "io.arc.event".to_string(),
            EventData::new(serde_json::json!({}), 1, "sess-test".to_string()),
            None,
            None,
            None,
            None,
            None,
            None,
        );

        let batch = IngestBatch {
            session_id: "sess-1".to_string(),
            project_id: "proj-1".to_string(),
            events: vec![event],
        };

        let json = serde_json::to_string(&batch).unwrap();
        let deserialized: IngestBatch = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.events.len(), 1);
        assert_eq!(deserialized.events[0].source, "test-source");
    }
}
