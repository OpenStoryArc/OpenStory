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

/// Federation mode configuration for `NatsBus`.
///
/// Presence of a `Federation` flips the bus from solo (single local `events`
/// stream bound to `events.>`) to federated (host-scoped local `events`, a
/// source-only `events-mirror` sourcing the hub aggregate across the JetStream
/// domain boundary, and self-registration into the hub's `events-agg`).
#[derive(Clone, Debug)]
pub struct Federation {
    /// This node's host identity. Used as the `events.{host}.>` subject
    /// prefix and as this node's JetStream domain (so sources on the hub
    /// aggregate distinguish leaves by domain).
    pub host: String,
    /// The hub's JetStream domain (typically `"hub"`). The `events-mirror`
    /// stream sources `events-agg` across this domain.
    pub hub_domain: String,
}

/// Default Bus implementation using NATS JetStream.
///
/// Events are published to JetStream subjects and persisted in durable streams.
/// Subscribers receive events via JetStream consumers. Replay reads from the
/// beginning of the stream for boot recovery.
pub struct NatsBus {
    /// JetStream context for *this node's own NATS*. In solo mode this is a
    /// vanilla context (`$JS.API.>`); in federation mode it's pinned to the
    /// node's local JetStream domain (`$JS.{host_or_hub}.API.>`) — the
    /// underlying NATS server is configured with that domain too.
    jetstream: jetstream::Context,
    client: async_nats::Client,
    federation: Option<Federation>,
}

impl NatsBus {
    /// Connect to NATS and set up JetStream in solo mode.
    ///
    /// Supports token auth via URL userinfo: `nats://TOKEN@host:port`.
    pub async fn connect(nats_url: &str) -> Result<Self> {
        Self::connect_inner(nats_url, None, None).await
    }

    /// Connect to NATS in hub mode: local JetStream is pinned to `hub_domain`
    /// (the hub's NATS config has `jetstream { domain: <hub_domain> }`). Used
    /// by the hub-side server, which holds the `events-agg` aggregate stream
    /// that leaves source.
    pub async fn connect_hub(nats_url: &str, hub_domain: &str) -> Result<Self> {
        Self::connect_inner(nats_url, Some(hub_domain.to_string()), None).await
    }

    /// Connect to NATS and set up JetStream in federation (leaf) mode.
    ///
    /// On `ensure_streams` the bus will create a host-scoped local `events`
    /// stream, a source-only `events-mirror` sourcing the hub aggregate
    /// across the `hub_domain` JetStream domain, and self-register this
    /// leaf's `events` stream as a source on the hub's `events-agg`.
    pub async fn connect_federation(nats_url: &str, federation: Federation) -> Result<Self> {
        // Leaf's local NATS is configured with `domain: <host>`, so the
        // local JetStream API lives under `$JS.<host>.API.>`.
        let local_domain = federation.host.clone();
        Self::connect_inner(nats_url, Some(local_domain), Some(federation)).await
    }

    async fn connect_inner(
        nats_url: &str,
        local_domain: Option<String>,
        federation: Option<Federation>,
    ) -> Result<Self> {
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

        // Domain-aware local JetStream: when the underlying NATS has
        // `jetstream { domain: D }` configured, its API moves to
        // `$JS.D.API.>` and a vanilla context can't reach it.
        let jetstream = match &local_domain {
            None => jetstream::new(client.clone()),
            Some(d) => jetstream::with_domain(client.clone(), d),
        };

        Ok(Self { jetstream, client, federation })
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
    /// Call this once on startup. Branches on `federation`: solo mode binds
    /// `events.>` and is done; federation also creates the host-scoped local
    /// `events`, the source-only `events-mirror`, and self-registers this
    /// leaf into the hub's `events-agg`.
    pub async fn ensure_streams(&self) -> Result<()> {
        // Events stream — durable, limits-based retention. The same pure
        // builder drives both modes; the (host, hub_domain) it gets decides
        // the subject binding.
        let (host, hub_domain) = match &self.federation {
            None => ("", None),
            Some(fed) => (fed.host.as_str(), Some(fed.hub_domain.as_str())),
        };
        self.jetstream
            .get_or_create_stream(events_stream_config(host, hub_domain))
            .await
            .context("failed to create/get 'events' JetStream stream")?;

        // Federation: also create the source-only mirror and self-register
        // on the hub aggregate. The mirror has no subjects (cannot loop), and
        // self-origin events are excluded by JetStream — so the fleet view is
        // `events ∪ events-mirror` with no duplication.
        if let Some(fed) = &self.federation {
            self.jetstream
                .get_or_create_stream(events_mirror_config(&fed.hub_domain))
                .await
                .context("failed to create/get 'events-mirror' JetStream stream")?;

            register_self_with_hub(
                self.client.clone(),
                &fed.hub_domain,
                &fed.host,
            )
            .await
            .context("failed to self-register as source on hub events-agg")?;
        }

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

    /// Create the federation aggregate stream (`events-agg`). Called on the
    /// **hub** so that leaves can self-register their `events` streams as
    /// sources on it. Idempotent: `get_or_create_stream` handles re-runs.
    /// The aggregate is source-only — leaves register themselves, no
    /// subjects are ever published directly to it.
    pub async fn ensure_aggregate(&self) -> Result<()> {
        self.jetstream
            .get_or_create_stream(events_aggregate_config())
            .await
            .context("failed to create/get 'events-agg' JetStream stream")?;
        Ok(())
    }

    /// Create an ephemeral push consumer on the named stream filtered by
    /// `pattern`, deserialize each message as an `IngestBatch`, and pump
    /// into `tx`. Used by both solo (one consumer on `events`) and
    /// federation (two consumers: `events` + `events-mirror`).
    async fn spawn_consumer(
        &self,
        tx: mpsc::Sender<IngestBatch>,
        stream_name: &str,
        pattern: &str,
    ) -> Result<()> {
        let stream = self
            .jetstream
            .get_stream(stream_name)
            .await
            .with_context(|| format!("failed to get '{stream_name}' stream"))?;

        let consumer = stream
            .create_consumer(jetstream::consumer::push::Config {
                filter_subject: pattern.to_string(),
                deliver_subject: format!("_deliver.{}", uuid_short()),
                // Catch-up subscription semantics: deliver the full backlog,
                // then continue live. See federation-boot-window-loss memory
                // for the race this closes. PK dedup makes redelivery safe.
                // Ephemeral consumer → O(stream) per subscribe (acceptable
                // for boot; production refinement is a durable named
                // consumer resuming from last ack).
                deliver_policy: jetstream::consumer::DeliverPolicy::All,
                ..Default::default()
            })
            .await
            .with_context(|| format!("failed to create push consumer on '{stream_name}'"))?;

        let mut messages = consumer
            .messages()
            .await
            .with_context(|| format!("failed to get message stream on '{stream_name}'"))?;

        let label = stream_name.to_string();
        tokio::spawn(async move {
            while let Some(Ok(msg)) = messages.next().await {
                match serde_json::from_slice::<IngestBatch>(&msg.payload) {
                    Ok(batch) => {
                        if tx.send(batch).await.is_err() {
                            break; // receiver dropped
                        }
                    }
                    Err(e) => {
                        eprintln!("bus[{label}]: failed to deserialize IngestBatch: {e}");
                    }
                }
                if let Err(e) = msg.ack().await {
                    eprintln!("bus[{label}]: failed to ack message: {e}");
                }
            }
        });
        Ok(())
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
        // Solo: one consumer on `events`. Federation: consumers on BOTH
        // `events` (own host's namespace) and `events-mirror` (fleet sourced
        // from the hub aggregate). Both pump into one shared mpsc so callers
        // see a single unified stream.
        let (tx, rx) = mpsc::channel(256);
        self.spawn_consumer(tx.clone(), "events", pattern).await
            .context("failed to spawn 'events' consumer")?;
        if self.federation.is_some() {
            self.spawn_consumer(tx, "events-mirror", pattern).await
                .context("failed to spawn 'events-mirror' consumer")?;
        }
        Ok(BusSubscription { receiver: rx })
    }

    async fn replay(&self, pattern: &str) -> Result<Vec<IngestBatch>> {
        // Solo: replay `events`. Federation: replay BOTH `events`
        // (own host's namespace) and `events-mirror` (fleet sourced from hub
        // aggregate). Order within each stream is preserved; cross-stream
        // ordering is not guaranteed (and event-ID dedup downstream makes
        // ordering irrelevant for correctness).
        let mut all = replay_one(&self.jetstream, "events", pattern).await
            .context("replay 'events' failed")?;
        if self.federation.is_some() {
            let mirror = replay_one(&self.jetstream, "events-mirror", pattern).await
                .context("replay 'events-mirror' failed")?;
            all.extend(mirror);
        }
        Ok(all)
    }
}

async fn replay_one(
    js: &jetstream::Context,
    stream_name: &str,
    pattern: &str,
) -> Result<Vec<IngestBatch>> {
    let mut stream = match js.get_stream(stream_name).await {
        Ok(s) => s,
        // Solo bus replaying when federation streams don't exist locally
        // is not an error — return empty.
        Err(_) => return Ok(vec![]),
    };

    let info = stream.info().await
        .with_context(|| format!("failed to get '{stream_name}' info"))?;
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
        .with_context(|| format!("failed to create pull consumer on '{stream_name}'"))?;

    let mut messages = consumer
        .messages()
        .await
        .with_context(|| format!("failed to get replay messages on '{stream_name}'"))?;

    let mut batches = Vec::new();
    let mut count = 0u64;
    while count < total {
        match tokio::time::timeout(std::time::Duration::from_secs(5), messages.next()).await {
            Ok(Some(Ok(msg))) => {
                match serde_json::from_slice::<IngestBatch>(&msg.payload) {
                    Ok(batch) => batches.push(batch),
                    Err(e) => eprintln!("bus[{stream_name}]: replay deserialize: {e}"),
                }
                let _ = msg.ack().await;
                count += 1;
            }
            Ok(Some(Err(e))) => {
                eprintln!("bus[{stream_name}]: replay message error: {e}");
                break;
            }
            Ok(None) => break,
            Err(_) => break, // timeout = done
        }
    }
    Ok(batches)
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
            external: Some(stream::External {
                api_prefix: js_api_prefix(hub_domain),
                delivery_prefix: None,
            }),
            ..Default::default()
        }]),
        ..Default::default()
    }
}

/// `$JS.<domain>.API` — the JetStream API prefix for a named domain. This is
/// the wire-format the NATS server expects on a `Source.external.api`; the
/// `Source.domain` field is an async-nats convenience that older brokers do
/// not accept as a top-level Source key, so we always emit `external` here.
fn js_api_prefix(domain: &str) -> String {
    format!("$JS.{domain}.API")
}

/// Idempotently add this leaf's `events` stream (in its own JetStream domain)
/// as a source on the hub aggregate's source list. Returns `true` if a source
/// was added, `false` if it was already present.
///
/// Sources are keyed by `(name, domain)`: every leaf names its stream `events`,
/// so the **domain** distinguishes leaves. The merge is **additive** — it never
/// drops a peer's source — which is what lets a read-modify-write across
/// concurrently-joining leaves not lose registrations (the I/O layer still
/// retries on a write conflict; this keeps the merge itself correct).
pub(crate) fn ensure_self_source(sources: &mut Vec<stream::Source>, my_domain: &str) -> bool {
    let want_prefix = js_api_prefix(my_domain);
    let already = sources.iter().any(|s| {
        s.name == "events"
            && s.external.as_ref().map(|e| e.api_prefix.as_str()) == Some(want_prefix.as_str())
    });
    if already {
        return false;
    }
    sources.push(stream::Source {
        name: "events".to_string(),
        external: Some(stream::External {
            api_prefix: want_prefix,
            delivery_prefix: None,
        }),
        ..Default::default()
    });
    true
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

/// Cross-domain self-registration on the hub aggregate (Option 3 in action).
///
/// Reads the hub's `events-agg` config across the JetStream domain boundary,
/// merges this leaf's source idempotently via [`ensure_self_source`], and
/// writes the updated config back. Retries on write conflict — a read-
/// modify-write across concurrently-joining leaves can collide; the merge is
/// additive (peers preserved), so the loser of a race just sees its peer's
/// source already there and re-applies its own on retry.
///
/// Returns `Ok(())` whether registration was newly added or already present.
pub(crate) async fn register_self_with_hub(
    client: async_nats::Client,
    hub_domain: &str,
    my_domain: &str,
) -> Result<()> {
    let hub_js = jetstream::with_domain(client, hub_domain);
    let mut last_err: Option<anyhow::Error> = None;
    for attempt in 0..5u32 {
        let mut agg = match hub_js.get_stream("events-agg").await {
            Ok(s) => s,
            Err(e) => {
                last_err = Some(anyhow::anyhow!("get_stream(events-agg) failed: {e}"));
                tokio::time::sleep(std::time::Duration::from_millis(100 * (1u64 << attempt))).await;
                continue;
            }
        };
        let mut cfg = agg.info().await
            .with_context(|| "info(events-agg) failed")?
            .config
            .clone();
        let mut sources = cfg.sources.unwrap_or_default();
        let added = ensure_self_source(&mut sources, my_domain);
        cfg.sources = Some(sources);
        if !added {
            return Ok(());
        }
        match hub_js.update_stream(&cfg).await {
            Ok(_) => return Ok(()),
            Err(e) => {
                last_err = Some(anyhow::anyhow!("update_stream(events-agg) failed: {e}"));
                tokio::time::sleep(std::time::Duration::from_millis(50 * (1u64 << attempt))).await;
            }
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("hub registration failed: unknown")))
        .context("hub events-agg self-registration exhausted retries")
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
        // Cross-domain reach is encoded as `external.api = "$JS.<domain>.API"`
        // (the wire format), not the convenience `domain` field — older
        // brokers reject `domain` as a top-level Source key.
        let external = sources[0].external.as_ref().expect("must use external for cross-domain");
        assert_eq!(external.api_prefix, "$JS.hub.API");
    }

    #[test]
    fn aggregate_config_is_source_only_with_no_subjects() {
        // The hub aggregate the leaves self-register into: source-only, named
        // events-agg, no direct subjects.
        let cfg = events_aggregate_config();
        assert_eq!(cfg.name, "events-agg");
        assert!(cfg.subjects.is_empty());
    }

    // ── self-registration merge (decentralized enumeration, Option 3) ──────
    // Each leaf adds its own `events` stream (in its own JetStream domain) as a
    // source on the shared hub aggregate. Sources are keyed by (name, domain) —
    // all leaves name their stream "events", so the DOMAIN distinguishes them.
    // The merge must be idempotent (re-registration is a no-op) and additive
    // (never clobber a peer's source) — the latter is what keeps a read-modify-
    // write across concurrent leaves from losing registrations.

    #[test]
    fn self_register_adds_own_source_to_empty_aggregate() {
        let mut sources = vec![];
        let added = ensure_self_source(&mut sources, "leaf-maxs-air");
        assert!(added, "first registration adds a source");
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].name, "events");
        let ext = sources[0].external.as_ref().expect("external set");
        assert_eq!(ext.api_prefix, "$JS.leaf-maxs-air.API");
    }

    #[test]
    fn self_register_is_idempotent() {
        let mut sources = vec![];
        ensure_self_source(&mut sources, "leaf-maxs-air");
        let added_again = ensure_self_source(&mut sources, "leaf-maxs-air");
        assert!(!added_again, "re-registration is a no-op");
        assert_eq!(sources.len(), 1, "no duplicate source for the same domain");
    }

    #[test]
    fn self_register_preserves_peer_sources() {
        // A leaf joining must NOT clobber peers already registered on the agg.
        let mut sources = vec![stream::Source {
            name: "events".to_string(),
            external: Some(stream::External {
                api_prefix: "$JS.leaf-katies-mini.API".to_string(),
                delivery_prefix: None,
            }),
            ..Default::default()
        }];
        let added = ensure_self_source(&mut sources, "leaf-maxs-air");
        assert!(added);
        assert_eq!(sources.len(), 2, "peer source preserved, own source added");
        let prefixes: Vec<_> = sources
            .iter()
            .filter_map(|s| s.external.as_ref().map(|e| e.api_prefix.as_str()))
            .collect();
        assert!(prefixes.contains(&"$JS.leaf-katies-mini.API"));
        assert!(prefixes.contains(&"$JS.leaf-maxs-air.API"));
    }
}
