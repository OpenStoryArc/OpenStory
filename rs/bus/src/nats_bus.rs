//! NatsBus — default Bus implementation backed by NATS JetStream.
//!
//! Provides durable event streams, multi-store fan-out, and replay from
//! stream history for boot recovery.

use anyhow::{Context, Result};
use async_nats::jetstream::{self, stream};
use async_trait::async_trait;
use futures::StreamExt;
use tokio::sync::mpsc;

use crate::{Bus, BusSubscription, IngestBatch};

/// Default Bus implementation using NATS JetStream.
///
/// Events are published to JetStream subjects and persisted in durable streams.
/// Subscribers receive events via JetStream consumers. Replay reads from the
/// beginning of the stream for boot recovery.
pub struct NatsBus {
    jetstream: jetstream::Context,
}

impl NatsBus {
    /// Connect to NATS and set up JetStream.
    pub async fn connect(nats_url: &str) -> Result<Self> {
        let client = async_nats::connect(nats_url)
            .await
            .with_context(|| format!("failed to connect to NATS at {nats_url}"))?;

        let jetstream = jetstream::new(client);

        Ok(Self { jetstream })
    }

    /// Ensure the "events" stream exists with durable retention.
    /// Call this once on startup.
    pub async fn ensure_streams(&self) -> Result<()> {
        // Events stream — durable, limits-based retention
        self.jetstream
            .get_or_create_stream(stream::Config {
                name: "events".to_string(),
                subjects: vec!["events.>".to_string()],
                retention: stream::RetentionPolicy::Limits,
                max_bytes: 1_073_741_824, // 1 GB default
                ..Default::default()
            })
            .await
            .context("failed to create/get 'events' JetStream stream")?;

        // Changes stream — interest-based (only kept while subscribers exist)
        self.jetstream
            .get_or_create_stream(stream::Config {
                name: "changes".to_string(),
                subjects: vec!["changes.>".to_string()],
                retention: stream::RetentionPolicy::Interest,
                ..Default::default()
            })
            .await
            .context("failed to create/get 'changes' JetStream stream")?;

        Ok(())
    }

    /// Get a reference to the JetStream context (for advanced use).
    pub fn jetstream(&self) -> &jetstream::Context {
        &self.jetstream
    }
}

#[async_trait]
impl Bus for NatsBus {
    async fn publish(&self, subject: &str, batch: &IngestBatch) -> Result<()> {
        let payload = serde_json::to_vec(batch).context("failed to serialize IngestBatch")?;

        self.jetstream
            .publish(subject.to_string(), payload.into())
            .await
            .with_context(|| format!("failed to publish to {subject}"))?
            .await
            .with_context(|| format!("failed to confirm publish to {subject}"))?;

        Ok(())
    }

    async fn publish_bytes(&self, subject: &str, data: &[u8]) -> Result<()> {
        self.jetstream
            .publish(subject.to_string(), data.to_vec().into())
            .await
            .with_context(|| format!("failed to publish bytes to {subject}"))?
            .await
            .with_context(|| format!("failed to confirm publish bytes to {subject}"))?;

        Ok(())
    }

    async fn subscribe(&self, pattern: &str) -> Result<BusSubscription> {
        let stream = self
            .jetstream
            .get_stream("events")
            .await
            .context("failed to get 'events' stream")?;

        let consumer = stream
            .create_consumer(jetstream::consumer::push::Config {
                filter_subject: pattern.to_string(),
                deliver_subject: format!("_deliver.{}", uuid_short()),
                deliver_policy: jetstream::consumer::DeliverPolicy::New,
                ..Default::default()
            })
            .await
            .context("failed to create push consumer")?;

        let mut messages = consumer
            .messages()
            .await
            .context("failed to get message stream")?;

        let (tx, rx) = mpsc::channel(256);

        tokio::spawn(async move {
            while let Some(Ok(msg)) = messages.next().await {
                match serde_json::from_slice::<IngestBatch>(&msg.payload) {
                    Ok(batch) => {
                        if tx.send(batch).await.is_err() {
                            break; // receiver dropped
                        }
                    }
                    Err(e) => {
                        eprintln!("bus: failed to deserialize IngestBatch: {e}");
                    }
                }
                // Acknowledge the message
                if let Err(e) = msg.ack().await {
                    eprintln!("bus: failed to ack message: {e}");
                }
            }
        });

        Ok(BusSubscription { receiver: rx })
    }

    async fn replay(&self, pattern: &str) -> Result<Vec<IngestBatch>> {
        let mut stream = self
            .jetstream
            .get_stream("events")
            .await
            .context("failed to get 'events' stream for replay")?;

        let info = stream.info().await.context("failed to get stream info")?;
        let total = info.state.messages;

        if total == 0 {
            return Ok(vec![]);
        }

        let consumer = stream
            .create_consumer(jetstream::consumer::pull::Config {
                filter_subject: pattern.to_string(),
                deliver_policy: jetstream::consumer::DeliverPolicy::All,
                ..Default::default()
            })
            .await
            .context("failed to create pull consumer for replay")?;

        let mut messages = consumer
            .messages()
            .await
            .context("failed to get replay message stream")?;

        let mut batches = Vec::new();
        let mut count = 0u64;

        while count < total {
            match tokio::time::timeout(std::time::Duration::from_secs(5), messages.next()).await {
                Ok(Some(Ok(msg))) => {
                    match serde_json::from_slice::<IngestBatch>(&msg.payload) {
                        Ok(batch) => batches.push(batch),
                        Err(e) => eprintln!("bus: replay: failed to deserialize: {e}"),
                    }
                    let _ = msg.ack().await;
                    count += 1;
                }
                Ok(Some(Err(e))) => {
                    eprintln!("bus: replay: message error: {e}");
                    break;
                }
                Ok(None) => break,
                Err(_) => {
                    // Timeout — we've read all available messages
                    break;
                }
            }
        }

        Ok(batches)
    }
}

fn uuid_short() -> String {
    uuid::Uuid::new_v4().to_string()[..8].to_string()
}
