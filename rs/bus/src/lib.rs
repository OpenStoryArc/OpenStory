//! Event bus trait and NATS implementation for open-story.
//!
//! The `Bus` trait defines the architectural boundary between event producers
//! (listener, hooks) and consumers (store). All events flow through the bus —
//! producers and consumers never communicate directly.
//!
//! Default implementation: `NatsBus` (NATS JetStream).

pub mod accounts;
pub mod health;
pub mod nats_bus;
pub mod noop_bus;
pub mod split;

// Re-export the async-nats JetStream context so server-side code
// (admin module) can hold a typed reference without depending on
// async-nats directly. Keeps the dependency graph clean.
pub use async_nats::jetstream::Context as JetstreamContext;

use anyhow::Result;
use async_trait::async_trait;
use open_story_core::cloud_event::CloudEvent;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

/// One JetStream stream's size against its configured cap (H-04).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StreamStats {
    pub name: String,
    pub bytes: u64,
    pub messages: u64,
    /// The configured `max_bytes`; `None` when the stream is uncapped.
    pub max_bytes: Option<i64>,
    /// `bytes / max_bytes`, `None` when uncapped.
    pub percent: Option<f64>,
}

impl StreamStats {
    pub fn new(name: impl Into<String>, bytes: u64, messages: u64, max_bytes: i64) -> Self {
        let cap = (max_bytes > 0).then_some(max_bytes);
        let percent = cap.map(|c| bytes as f64 / c as f64);
        StreamStats {
            name: name.into(),
            bytes,
            messages,
            max_bytes: cap,
            percent,
        }
    }

    pub fn percent(&self) -> Option<f64> {
        self.percent
    }
}

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

/// Where a durable consumer starts the first time it is created (B-11).
/// Once it exists, the server remembers its position and a restart resumes
/// from what the actor has not acknowledged; this only decides the start of
/// a consumer that does not exist yet (first boot after the upgrade, a new
/// node, a consumer the server lost across a process restart).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartFrom {
    /// The whole stream: the actor is the only way what the stream holds
    /// reaches the store (persist, patterns).
    All,
    /// What was published since this process started: the store was
    /// replayed from what existed before, and the watcher publishes while
    /// the node boots, before any consumer exists (projections, patterns).
    SinceBoot,
    /// Only what arrives from now: live readers only (broadcast).
    New,
    /// The last message on each subject: presence is a latest-beat table.
    LastPerSubject,
}

/// A durable consumer's identity and first start (B-11).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableSpec {
    /// Stable across restarts: see [`durable_name`].
    pub name: String,
    pub first_start: StartFrom,
}

/// The durable consumer name for `actor` on `host`: `os-{actor}-{host}`,
/// restricted to the characters JetStream accepts in a consumer name
/// (anything but ASCII letters, digits, `-` and `_` becomes `-`), at most
/// 128 bytes. Pure.
pub fn durable_name(actor: &str, host: &str) -> String {
    let raw = format!("os-{actor}-{host}");
    let mut name: String = raw
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    name.truncate(128);
    name
}

/// How a delivered batch is acknowledged once its actor has handled it.
/// `Acker::none()` for a bus without acknowledgements.
pub struct Acker(Option<nats_bus::NatsAck>);

impl Acker {
    pub fn none() -> Self {
        Acker(None)
    }

    pub(crate) fn nats(ack: nats_bus::NatsAck) -> Self {
        Acker(Some(ack))
    }

    /// Tell the bus the batch is handled; a failure is logged, never fatal
    /// (the message is redelivered and the actors are idempotent).
    pub async fn ack(self) {
        if let Some(a) = self.0 {
            a.ack().await;
        }
    }
}

/// One batch from a durable subscription, with its acknowledgement.
pub struct Delivery {
    pub batch: IngestBatch,
    pub ack: Acker,
}

/// A durable subscription: batches arrive with the handle that
/// acknowledges them (B-11). The actor acks after it has handled a batch.
pub struct DurableSubscription {
    pub receiver: mpsc::Receiver<Delivery>,
}

impl DurableSubscription {
    /// Wrap a plain subscription whose batches need no acknowledgement.
    pub fn unacked(sub: BusSubscription) -> Self {
        let mut rx = sub.receiver;
        let (tx, out) = mpsc::channel(16);
        tokio::spawn(async move {
            while let Some(batch) = rx.recv().await {
                let d = Delivery {
                    batch,
                    ack: Acker::none(),
                };
                if tx.send(d).await.is_err() {
                    break;
                }
            }
        });
        DurableSubscription { receiver: out }
    }
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

    /// Subscribe to `pattern` on a named stream that is not the events
    /// family (P-02: `presence`). A bus with no stream notion falls back to
    /// its plain subscribe.
    async fn subscribe_stream(&self, _stream: &str, pattern: &str) -> Result<BusSubscription> {
        self.subscribe(pattern).await
    }

    /// Subscribe through a durable consumer named by `spec` (B-11):
    /// `stream` is `"events"` for the events family (own, local, and the
    /// fleet's mirror) or a named stream such as `"presence"`. Each batch
    /// arrives with its acknowledgement; a restart resumes from what was not
    /// acknowledged. A bus without durable consumers falls back to its
    /// plain subscription, with acknowledgements that do nothing.
    async fn subscribe_durable(
        &self,
        stream: &str,
        pattern: &str,
        _spec: &DurableSpec,
    ) -> Result<DurableSubscription> {
        let sub = if stream == "events" {
            self.subscribe(pattern).await?
        } else {
            self.subscribe_stream(stream, pattern).await?
        };
        Ok(DurableSubscription::unacked(sub))
    }

    /// Replay all historical events matching a pattern.
    /// Used for boot recovery — rebuilds store state from the event log.
    async fn replay(&self, pattern: &str) -> Result<Vec<IngestBatch>>;

    /// Whether this bus implementation is active (connected to real transport).
    /// Returns false for NoopBus. Used to decide whether to route events
    /// through the bus or use direct ingest as fallback.
    fn is_active(&self) -> bool {
        true
    }

    /// Size of each stream this bus declares, against its cap (H-04).
    /// Empty for a bus with no JetStream behind it.
    async fn stream_stats(&self) -> Vec<StreamStats> {
        vec![]
    }

    /// Optional JetStream context handle for admin/introspection use.
    ///
    /// `Some(ctx)` when this bus is backed by NATS JetStream (the only
    /// implementation today). `None` for NoopBus and any future non-
    /// NATS backend. Callers use it for read-only stream-info queries —
    /// the admin module's live fleet view depends on this.
    ///
    /// The default `None` means a feature that needs JetStream
    /// gracefully degrades on a NoopBus rather than failing.
    fn jetstream(&self) -> Option<&async_nats::jetstream::Context> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durable_name_is_stable_and_a_legal_consumer_name() {
        assert_eq!(durable_name("persist", "mac-mini"), "os-persist-mac-mini");
        assert_eq!(
            durable_name("presence", "box.local with/slash*>"),
            "os-presence-box-local-with-slash--"
        );
        assert_eq!(durable_name("persist", "h"), durable_name("persist", "h"));
        assert_ne!(durable_name("persist", "h"), durable_name("patterns", "h"));
        assert!(durable_name("persist", &"x".repeat(400)).len() <= 128);
    }

    #[tokio::test]
    async fn a_bus_without_durables_delivers_plain_batches_with_a_noop_ack() {
        let (tx, rx) = mpsc::channel(4);
        let mut sub = DurableSubscription::unacked(BusSubscription { receiver: rx });
        tx.send(IngestBatch {
            session_id: "s".into(),
            project_id: "p".into(),
            events: vec![],
        })
        .await
        .unwrap();
        drop(tx);
        let d = sub.receiver.recv().await.expect("one delivery");
        assert_eq!(d.batch.session_id, "s");
        d.ack.ack().await;
        assert!(sub.receiver.recv().await.is_none(), "ends with its source");
    }

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

    #[test]
    fn noop_bus_jetstream_is_none() {
        // Admin v0.2: features that need JetStream introspection must
        // gracefully degrade on a non-NATS bus. NoopBus says "no, I have
        // no JetStream" so the admin handler can return live_sources: null
        // rather than 500-ing.
        let bus = noop_bus::NoopBus;
        assert!(bus.jetstream().is_none());
    }

    #[test]
    fn noop_bus_is_not_active() {
        // Sanity: paired-default behavior — NoopBus is neither active
        // nor has a JetStream handle. Together these gate every
        // JetStream-aware path.
        let bus = noop_bus::NoopBus;
        assert!(!bus.is_active());
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
        let mut sub = bus
            .subscribe("events.>")
            .await
            .expect("noop subscribe always Ok");
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
