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
    ///
    /// Supports token auth via URL userinfo: `nats://TOKEN@host:port`.
    pub async fn connect(nats_url: &str) -> Result<Self> {
        let client = if let Some(token) = Self::extract_token(nats_url) {
            let clean_url = Self::strip_userinfo(nats_url);
            async_nats::ConnectOptions::with_token(token)
                .connect(&clean_url)
                .await
                .with_context(|| format!("failed to connect to NATS at {clean_url} (with token)"))?
        } else {
            async_nats::connect(nats_url)
                .await
                .with_context(|| format!("failed to connect to NATS at {nats_url}"))?
        };

        let jetstream = jetstream::new(client);

        Ok(Self { jetstream })
    }

    /// Extract token from `nats://TOKEN@host:port` URL.
    fn extract_token(url: &str) -> Option<String> {
        let after_scheme = url.strip_prefix("nats://")?;
        let at_pos = after_scheme.find('@')?;
        let userinfo = &after_scheme[..at_pos];
        // Only treat as token if there's no colon (user:pass is different)
        if userinfo.contains(':') {
            None
        } else {
            Some(userinfo.to_string())
        }
    }

    /// Strip userinfo from URL: `nats://token@host:port` → `nats://host:port`.
    fn strip_userinfo(url: &str) -> String {
        if let Some(after_scheme) = url.strip_prefix("nats://") {
            if let Some(at_pos) = after_scheme.find('@') {
                return format!("nats://{}", &after_scheme[at_pos + 1..]);
            }
        }
        url.to_string()
    }

    /// Ensure the "events" stream exists with durable retention.
    /// Call this once on startup.
    pub async fn ensure_streams(&self) -> Result<()> {
        // Events stream — durable, limits-based retention. Solo mode binds
        // `events.>`; federation mode (host-scoped binding + mirror) is set up
        // via `ensure_federation_streams`.
        self.jetstream
            .get_or_create_stream(events_stream_config("", None))
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

        // Patterns stream — durable, limits-based. Carries derived pattern
        // events (turn.sentence, eval_apply.*) published by the patterns
        // consumer. Subscribed by future Live Story consumers (next branch's
        // stream architecture rewrite); right now no subscriber exists, but
        // the stream needs to exist so `publish_bytes` to `patterns.>`
        // doesn't error out at the broker.
        self.jetstream
            .get_or_create_stream(stream::Config {
                name: "patterns".to_string(),
                subjects: vec!["patterns.>".to_string()],
                retention: stream::RetentionPolicy::Limits,
                max_bytes: 268_435_456, // 256 MB — patterns are smaller than events
                ..Default::default()
            })
            .await
            .context("failed to create/get 'patterns' JetStream stream")?;

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
                // Catch-up subscription: deliver the full backlog, then continue
                // live. `New` delivered no history, so a subscriber that came up
                // after a publisher had already forwarded its events missed them
                // forever — the boot-window race that lost ~1 session per 10
                // concurrently-joining nodes (federation-boot-window-loss). PK
                // dedup makes the redelivered backlog harmless.
                //
                // PROTOTYPE NOTE: this is an *ephemeral* consumer, so `All`
                // re-reads the whole `events` stream on every (re)subscribe —
                // O(stream) per boot. Correct, but wasteful at scale. The
                // production refinement is a *durable named* consumer that
                // resumes from its last ack (backlog-since-last-seen + live),
                // the standard catch-up-subscription pattern.
                deliver_policy: jetstream::consumer::DeliverPolicy::All,
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

// ── Federation stream-config builders (Phase 2) ──────────────────────────
// Pure functions producing the JetStream configs for federation. Kept pure so
// the config shapes are unit-testable without a broker; the topology/scale
// behavior is proven by the testcontainer lab. See
// docs/research/jetstream-sources-federation.md.

const EVENTS_MAX_BYTES: i64 = 1_073_741_824; // 1 GB

/// The local `events` stream this node publishes into.
///
/// - **Solo** (`hub_domain = None`): binds `events.>` — captures everything,
///   unchanged from pre-federation behavior.
/// - **Federation**: binds ONLY `events.{host}.>` so the racy core leafnode
///   propagation can't cross-pollinate streams and the hub aggregate can't
///   double-count (the load-bearing spike finding). Publish-only, no sources.
pub(crate) fn events_stream_config(host: &str, hub_domain: Option<&str>) -> stream::Config {
    let subjects = match hub_domain {
        None => vec!["events.>".to_string()],
        Some(_) => vec![format!("events.{host}.>")],
    };
    stream::Config {
        name: "events".to_string(),
        subjects,
        retention: stream::RetentionPolicy::Limits,
        max_bytes: EVENTS_MAX_BYTES,
        ..Default::default()
    }
}

/// The source-only fleet mirror: pulls everyone else's events down from the
/// hub aggregate across the JetStream domain boundary. No subjects → no
/// publishers → structurally cannot loop (and JetStream self-origin loop
/// prevention excludes this leaf's own events). The complete fleet view on a
/// node is therefore `events ∪ events-mirror`.
pub(crate) fn events_mirror_config(hub_domain: &str) -> stream::Config {
    stream::Config {
        name: "events-mirror".to_string(),
        subjects: vec![],
        retention: stream::RetentionPolicy::Limits,
        max_bytes: EVENTS_MAX_BYTES,
        sources: Some(vec![stream::Source {
            name: "events-agg".to_string(),
            domain: Some(hub_domain.to_string()),
            ..Default::default()
        }]),
        ..Default::default()
    }
}

/// The hub aggregate that leaves self-register into (decentralized enumeration
/// — Option 3). Source-only; each leaf adds its own `events` stream as a source
/// via the cross-domain API. Created empty here.
pub(crate) fn events_aggregate_config() -> stream::Config {
    stream::Config {
        name: "events-agg".to_string(),
        subjects: vec![],
        retention: stream::RetentionPolicy::Limits,
        max_bytes: EVENTS_MAX_BYTES,
        ..Default::default()
    }
}

#[cfg(test)]
mod federation_config_tests {
    //! Phase 2 (federation): the pure stream-config builders. See
    //! docs/research/jetstream-sources-federation.md. These are container-free
    //! unit tests of the config shapes; the topology/scale behavior is proven
    //! by the testcontainer lab.
    use super::*;

    #[test]
    fn solo_events_stream_binds_everything() {
        // No hub domain → solo mode → unchanged from today: capture `events.>`.
        let cfg = events_stream_config("maxs-air", None);
        assert_eq!(cfg.name, "events");
        assert_eq!(cfg.subjects, vec!["events.>".to_string()]);
        assert!(cfg.sources.is_none(), "solo events stream sources nothing");
        assert!(matches!(cfg.retention, stream::RetentionPolicy::Limits));
    }

    #[test]
    fn federation_events_stream_binds_only_own_host() {
        // Hub domain set → bind ONLY this host's namespace so core leafnode
        // propagation can't cross-pollinate and the hub aggregate can't
        // double-count (the load-bearing spike finding).
        let cfg = events_stream_config("maxs-air", Some("hub"));
        assert_eq!(cfg.name, "events");
        assert_eq!(cfg.subjects, vec!["events.maxs-air.>".to_string()]);
        assert!(cfg.sources.is_none(), "local events stream is publish-only");
    }

    #[test]
    fn mirror_stream_sources_the_hub_aggregate_cross_domain() {
        // Source-only (no subjects → no publishers → cannot loop). Pulls the
        // fleet down from `events-agg` living in the hub JetStream domain.
        let cfg = events_mirror_config("hub");
        assert_eq!(cfg.name, "events-mirror");
        assert!(
            cfg.subjects.is_empty(),
            "mirror is source-only — binds no subjects, so it cannot loop"
        );
        let sources = cfg.sources.as_ref().expect("mirror must have a source");
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].name, "events-agg");
        assert_eq!(
            sources[0].domain.as_deref(),
            Some("hub"),
            "source must reach across the hub JetStream domain"
        );
    }

    #[test]
    fn aggregate_config_is_source_only_with_no_subjects() {
        // The hub aggregate the leaves self-register into: source-only, named
        // events-agg, no direct subjects.
        let cfg = events_aggregate_config();
        assert_eq!(cfg.name, "events-agg");
        assert!(cfg.subjects.is_empty());
    }
}
