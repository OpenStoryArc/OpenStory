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
/// stream bound to `events.>`) to federated (host-scoped local `events`
/// stream + a source-only `events-mirror` stream pulling the rest of the
/// fleet). The peer topology — hub-star (T2) or device mesh (T1) — is
/// captured in [`FederationPeers`].
#[derive(Clone, Debug)]
pub struct Federation {
    /// This node's host identity. Used as the `events.{host}.>` subject
    /// prefix and as this node's local JetStream domain.
    pub host: String,
    /// Where this node pulls fleet events from.
    pub peers: FederationPeers,
}

/// Peer topology for federation.
///
/// - **Hub** (T2/T3): leaves source from a single hub aggregate
///   (`events-agg` at `hub_domain`) and self-register into it. The hub
///   itself runs a separate role (`connect_hub` + `ensure_aggregate`).
/// - **Mesh** (T1, solo multi-device): there is no hub aggregate. Each
///   device's `events-mirror` sources directly from every peer's local
///   `events` stream — one source per peer domain. No registration step.
#[derive(Clone, Debug)]
pub enum FederationPeers {
    Hub { hub_domain: String },
    Mesh { peer_domains: Vec<String> },
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
    /// The JetStream domain `jetstream` is pinned to, when federation is
    /// active. `None` in solo mode. Carried explicitly because the
    /// async-nats Context doesn't expose its domain via a public getter.
    local_domain: Option<String>,
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
    /// On `ensure_streams` the bus creates a host-scoped local `events`
    /// stream and a source-only `events-mirror`. The mirror's source set
    /// depends on `federation.peers`:
    /// - `Hub { hub_domain }` — sources the hub's `events-agg` across
    ///   `$JS.<hub_domain>.API`, plus self-registers this leaf into it.
    /// - `Mesh { peer_domains }` — sources every peer's `events` stream
    ///   directly across `$JS.<peer>.API` (no aggregate, no registration).
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
        } else if let Some((user, pass)) = Self::extract_user_pass(nats_url) {
            // async-nats does not parse credentials out of URL userinfo
            // (same constraint the multi-account isolation tests work
            // around) — extract them and pass via ConnectOptions.
            let clean_url = Self::strip_userinfo(nats_url);
            async_nats::ConnectOptions::with_user_and_password(user, pass)
                .connect(&clean_url)
                .await
                .with_context(|| {
                    format!("failed to connect to NATS at {clean_url} (with user/password)")
                })?
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

        Ok(Self {
            jetstream,
            local_domain,
            client,
            federation,
        })
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

    /// Extract `user:pass` userinfo: `nats://u:p@host:port` → `(u, p)`.
    fn extract_user_pass(url: &str) -> Option<(String, String)> {
        let after_scheme = url.strip_prefix("nats://")?;
        let at_pos = after_scheme.find('@')?;
        let userinfo = &after_scheme[..at_pos];
        let colon = userinfo.find(':')?;
        Some((
            userinfo[..colon].to_string(),
            userinfo[colon + 1..].to_string(),
        ))
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
    /// Call this once on startup. Branches on `federation`:
    /// - **solo**: binds `events.>` and is done.
    /// - **federation/Hub**: binds `events.{host}.>` + creates source-only
    ///   `events-mirror` sourcing the hub aggregate + self-registers into
    ///   the hub's `events-agg`.
    /// - **federation/Mesh**: binds `events.{host}.>` + creates source-only
    ///   `events-mirror` sourcing every peer's local `events` stream
    ///   (one source per peer domain, no aggregate, no registration).
    pub async fn ensure_streams(&self) -> Result<()> {
        // Events stream — same pure builder for solo + federation. The
        // (host, federation-active) flag decides the subject binding.
        let (host, fed_active) = match &self.federation {
            None => ("", false),
            Some(fed) => (fed.host.as_str(), true),
        };
        self.jetstream
            .get_or_create_stream(events_stream_config(host, fed_active))
            .await
            .context("failed to create/get 'events' JetStream stream")?;

        if let Some(fed) = &self.federation {
            match &fed.peers {
                FederationPeers::Hub { hub_domain } => {
                    self.jetstream
                        .get_or_create_stream(events_mirror_config(hub_domain))
                        .await
                        .context("failed to create/get 'events-mirror' (Hub) stream")?;
                    register_self_with_hub(self.client.clone(), hub_domain, &fed.host)
                        .await
                        .context("failed to self-register as source on hub events-agg")?;
                }
                FederationPeers::Mesh { peer_domains } => {
                    self.jetstream
                        .get_or_create_stream(events_mirror_mesh_config(peer_domains))
                        .await
                        .context("failed to create/get 'events-mirror' (Mesh) stream")?;
                }
            }
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
    ///
    /// For **T3 multi-hub mesh**, pass the domains of peer hubs whose
    /// `events-agg` should also be sourced into this one. Hub-to-hub
    /// sourcing converges without double-counting thanks to per-host
    /// subject namespacing — a given event has exactly one origin host.
    pub async fn ensure_aggregate(&self, peer_hub_domains: &[String]) -> Result<()> {
        self.jetstream
            .get_or_create_stream(events_aggregate_config())
            .await
            .context("failed to create/get 'events-agg' JetStream stream")?;
        if !peer_hub_domains.is_empty() {
            let my_hub_domain = self
                .jetstream_domain()
                .ok_or_else(|| anyhow::anyhow!("hub mode requires a JetStream domain"))?;
            register_peer_hubs(self.client.clone(), &my_hub_domain, peer_hub_domains)
                .await
                .context("failed to register peer hubs on events-agg")?;
        }
        Ok(())
    }

    /// The JetStream domain this bus's local context is pinned to, if any.
    /// Used by `ensure_aggregate` for T3 cross-domain access into our own
    /// `events-agg`.
    fn jetstream_domain(&self) -> Option<String> {
        // Round-trip through the context's prefix: `$JS.<domain>.API`.
        // We stored the client and domain at construction time, but the
        // Context doesn't expose its domain directly. Carry it explicitly
        // below — see the `local_domain` field added in this commit.
        self.local_domain.clone()
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

    fn jetstream(&self) -> Option<&async_nats::jetstream::Context> {
        Some(&self.jetstream)
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
/// - **Solo** (`federation = false`): binds `events.>` — captures everything,
///   unchanged from pre-federation behavior.
/// - **Federation** (`federation = true`): binds ONLY `events.{host}.>` so
///   the racy core leafnode propagation can't cross-pollinate streams and
///   peers can't double-count (the load-bearing spike finding). Publish-only,
///   no sources. Same shape for Hub (T2/T3) and Mesh (T1) — the `events-mirror`
///   carries the topology difference.
pub(crate) fn events_stream_config(host: &str, federation: bool) -> stream::Config {
    let subjects = if federation {
        vec![format!("events.{host}.>")]
    } else {
        vec!["events.>".to_string()]
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
pub(crate) fn js_api_prefix(domain: &str) -> String {
    format!("$JS.{domain}.API")
}

/// Inverse of [`js_api_prefix`] — read a domain back out of a wire-format
/// `$JS.<domain>.API` string. Returns `None` for inputs that don't fit the
/// envelope (missing `$JS.` prefix, missing `.API` suffix, empty domain,
/// or anything else). Case-sensitive: `.api` is *not* accepted, matching
/// NATS's own behavior.
///
/// Used by the admin module to surface which leaf each source on the
/// hub's `events-agg` belongs to.
pub fn parse_js_api_prefix(s: &str) -> Option<String> {
    let inner = s.strip_prefix("$JS.")?.strip_suffix(".API")?;
    if inner.is_empty() {
        None
    } else {
        Some(inner.to_string())
    }
}

/// One row in the live JetStream fleet view — surfaces the configured
/// identity (`name`, `host`, `api_prefix`) paired with the runtime delivery
/// state (`lag`, `active_ms`) of a single source on a stream's `sources[]`.
///
/// `host` is `None` when the configured source has no `external` (a
/// same-domain local source — rare in our topology but legal) or when the
/// `api_prefix` doesn't parse as `$JS.<domain>.API` (a misconfiguration
/// the UI should surface).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct LiveSourceEntry {
    pub name: String,
    pub host: Option<String>,
    pub api_prefix: Option<String>,
    pub lag: u64,
    /// Milliseconds since the source was last seen active. `None` means
    /// "never seen" (the source exists but hasn't delivered yet).
    pub active_ms: Option<u64>,
}

/// Pair configured sources (`stream::Source`) with runtime source state
/// (`stream::SourceInfo`) into a flat fleet view.
///
/// **Pairing contract.** NATS preserves config-order in the response;
/// `config.sources[i]` and `state.sources[i]` describe the same source.
/// `SourceInfo` doesn't carry `external.api_prefix`, so we can't key by
/// it cross-side. We can, however, sanity-check that `name` matches at
/// each index — if it doesn't, the list semantics have drifted and we
/// degrade to config-only (lag=0, active_ms=None) rather than
/// misattribute runtime stats to the wrong leaf.
///
/// Length-mismatch (transient: config grew, runtime hasn't caught up) is
/// handled by `zip` — the shorter prefix is paired; later config entries
/// are dropped on the floor for THIS frame. The next pulse re-derives.
///
/// Pure: no I/O. Tested in isolation; the I/O wrapper that fetches both
/// halves from a live JetStream lives at the admin handler boundary.
pub fn derive_live_sources(
    config: &[stream::Source],
    runtime: &[async_nats::jetstream::stream::SourceInfo],
) -> Vec<LiveSourceEntry> {
    config
        .iter()
        .zip(runtime.iter())
        .map(|(c, r)| {
            let api_prefix = c.external.as_ref().map(|e| e.api_prefix.clone());
            let host = api_prefix.as_deref().and_then(parse_js_api_prefix);
            // Sanity-check: names must agree at the matching index. If
            // they don't, degrade — surface the config-side identity
            // (so the operator still sees which leaf was registered) but
            // suppress runtime stats to avoid the silently-wrong fleet
            // view the PR #58 reviewer was worried about.
            let names_aligned = c.name == r.name;
            LiveSourceEntry {
                name: c.name.clone(),
                host,
                api_prefix,
                lag: if names_aligned { r.lag } else { 0 },
                active_ms: if names_aligned {
                    r.active.map(|d| d.as_millis() as u64)
                } else {
                    None
                },
            }
        })
        .collect()
}

/// Mesh-mode mirror (T1 solo multi-device): no hub aggregate, instead one
/// source per peer's local `events` stream. Each peer publishes only into
/// `events.<peer>.>` (the spike's per-host namespace finding), so collecting
/// every peer's `events` stream into the mirror gives the full fleet view
/// without overlap. Self-origin loop prevention still excludes this node's
/// own events, so `events ∪ events-mirror` is the fleet without duplication.
pub(crate) fn events_mirror_mesh_config(peer_domains: &[String]) -> stream::Config {
    let sources: Vec<stream::Source> = peer_domains
        .iter()
        .map(|peer| stream::Source {
            name: "events".to_string(),
            external: Some(stream::External {
                api_prefix: js_api_prefix(peer),
                delivery_prefix: None,
            }),
            ..Default::default()
        })
        .collect();
    stream::Config {
        name: "events-mirror".to_string(),
        subjects: vec![],
        retention: stream::RetentionPolicy::Limits,
        max_bytes: EVENTS_MAX_BYTES,
        sources: Some(sources),
        ..Default::default()
    }
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

/// Idempotently add peer hubs' `events-agg` streams as sources on the
/// shared aggregate list, so a leaf attached to *this* hub also receives
/// events from leaves attached to peer hubs. Same merge semantics as
/// [`ensure_self_source`] (idempotent + additive), keyed by
/// `(name="events-agg", external.api="$JS.<peer>.API")`.
///
/// This is the T3 multi-hub mesh primitive. Combined with the per-host
/// subject namespacing (`events.{host}.>`) each leaf publishes, hub-to-hub
/// sourcing converges without double-counting: a given event has exactly
/// one origin host, so it appears once in each hub's events-agg regardless
/// of how many hops it took.
pub(crate) fn ensure_peer_hub_source(
    sources: &mut Vec<stream::Source>,
    peer_hub_domain: &str,
) -> bool {
    let want_prefix = js_api_prefix(peer_hub_domain);
    let already = sources.iter().any(|s| {
        s.name == "events-agg"
            && s.external.as_ref().map(|e| e.api_prefix.as_str()) == Some(want_prefix.as_str())
    });
    if already {
        return false;
    }
    sources.push(stream::Source {
        name: "events-agg".to_string(),
        external: Some(stream::External {
            api_prefix: want_prefix,
            delivery_prefix: None,
        }),
        ..Default::default()
    });
    true
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
/// Idempotently add each peer hub's `events-agg` as a source on **this**
/// hub's `events-agg`. The T3 multi-hub mesh setup step the hub runs on
/// boot. Uses the same get→merge→update→retry pattern as
/// [`register_self_with_hub`]; the merge is additive (peers preserved),
/// so a concurrent hub joining doesn't lose this hub's existing sources.
pub(crate) async fn register_peer_hubs(
    client: async_nats::Client,
    my_hub_domain: &str,
    peer_hub_domains: &[String],
) -> Result<()> {
    let my_js = jetstream::with_domain(client, my_hub_domain);
    let mut last_err: Option<anyhow::Error> = None;
    for attempt in 0..5u32 {
        let mut agg = match my_js.get_stream("events-agg").await {
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
        let mut any_added = false;
        for peer in peer_hub_domains {
            if ensure_peer_hub_source(&mut sources, peer) {
                any_added = true;
            }
        }
        cfg.sources = Some(sources);
        if !any_added {
            return Ok(()); // all peer hubs already registered
        }
        match my_js.update_stream(&cfg).await {
            Ok(_) => return Ok(()),
            Err(e) => {
                last_err = Some(anyhow::anyhow!("update_stream(events-agg) failed: {e}"));
                tokio::time::sleep(std::time::Duration::from_millis(50 * (1u64 << attempt))).await;
            }
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("peer hub registration failed: unknown")))
        .context("peer-hub registration on events-agg exhausted retries")
}

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
mod url_credential_tests {
    //! URL userinfo → ConnectOptions routing. async-nats ignores userinfo
    //! in the URL, so connect_inner must extract it: a lone segment is a
    //! token, `user:pass` is account credentials (multi-account mode).
    use super::*;

    #[test]
    fn user_pass_userinfo_is_extracted_for_account_auth() {
        let (user, pass) =
            NatsBus::extract_user_pass("nats://person-id:person-id-local-dev@localhost:4222")
                .expect("user:pass userinfo should be extracted");
        assert_eq!(user, "person-id");
        assert_eq!(pass, "person-id-local-dev");
        // And it must NOT be mistaken for a token.
        assert!(NatsBus::extract_token("nats://person-id:person-id-local-dev@localhost:4222")
            .is_none());
    }

    #[test]
    fn token_and_bare_urls_yield_no_user_pass() {
        assert!(NatsBus::extract_user_pass("nats://token@localhost:4222").is_none());
        assert!(NatsBus::extract_user_pass("nats://localhost:4222").is_none());
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
        // No federation → unchanged from today: capture `events.>`.
        let cfg = events_stream_config("maxs-air", false);
        assert_eq!(cfg.name, "events");
        assert_eq!(cfg.subjects, vec!["events.>".to_string()]);
        assert!(cfg.sources.is_none(), "solo events stream sources nothing");
        assert!(matches!(cfg.retention, stream::RetentionPolicy::Limits));
    }

    #[test]
    fn federation_events_stream_binds_only_own_host() {
        // Federation → bind ONLY this host's namespace so core leafnode
        // propagation can't cross-pollinate and peers can't double-count
        // (the load-bearing spike finding). Same shape for Hub + Mesh.
        let cfg = events_stream_config("maxs-air", true);
        assert_eq!(cfg.name, "events");
        assert_eq!(cfg.subjects, vec!["events.maxs-air.>".to_string()]);
        assert!(cfg.sources.is_none(), "local events stream is publish-only");
    }

    #[test]
    fn mesh_mirror_sources_each_peer_directly() {
        // T1 solo multi-device: no hub aggregate, one source per peer.
        // The mirror collects every peer's local `events` stream across
        // their own JetStream domain.
        let cfg = events_mirror_mesh_config(&[
            "laptop".to_string(),
            "phone".to_string(),
        ]);
        assert_eq!(cfg.name, "events-mirror");
        assert!(cfg.subjects.is_empty(), "mesh mirror is source-only");
        let sources = cfg.sources.as_ref().expect("mesh mirror must have sources");
        assert_eq!(sources.len(), 2, "one source per peer");
        let prefixes: Vec<_> = sources
            .iter()
            .filter_map(|s| s.external.as_ref().map(|e| e.api_prefix.as_str()))
            .collect();
        assert!(prefixes.contains(&"$JS.laptop.API"));
        assert!(prefixes.contains(&"$JS.phone.API"));
        for s in sources {
            assert_eq!(s.name, "events", "mesh sources each peer's local `events`");
        }
    }

    #[test]
    fn mesh_mirror_with_no_peers_has_empty_sources() {
        // A degenerate case: a single device in a "mesh" of one is just
        // a no-op for ingress (nothing to mirror). Should still be a
        // valid stream config — empty sources, not a panic.
        let cfg = events_mirror_mesh_config(&[]);
        let sources = cfg.sources.as_ref().expect("Some(empty) not None");
        assert!(sources.is_empty());
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

    // ── Inverse of js_api_prefix — read a domain back from the wire format ─
    // Admin v0.2 (live JetStream introspection): when we read a SourceInfo
    // off `events-agg` and want to surface "which leaf is this source from",
    // we parse `external.api_prefix` back into the leaf's domain. The
    // function MUST be a faithful inverse of `js_api_prefix(domain)`.

    #[test]
    fn parse_js_api_prefix_returns_domain_for_leaf_api() {
        assert_eq!(
            parse_js_api_prefix("$JS.node-0.API"),
            Some("node-0".to_string())
        );
    }

    #[test]
    fn parse_js_api_prefix_handles_hub_label() {
        assert_eq!(parse_js_api_prefix("$JS.hub.API"), Some("hub".to_string()));
    }

    #[test]
    fn parse_js_api_prefix_round_trips_with_js_api_prefix() {
        // The load-bearing property — any domain we emit must come back.
        for d in ["a1", "Maxs-Air", "node-99", "hub-b", "leaf-katies-mini"] {
            let emitted = js_api_prefix(d);
            assert_eq!(parse_js_api_prefix(&emitted), Some(d.to_string()), "round-trip {d}");
        }
    }

    #[test]
    fn parse_js_api_prefix_rejects_missing_prefix() {
        assert_eq!(parse_js_api_prefix("node-0"), None);
        assert_eq!(parse_js_api_prefix("JS.node-0.API"), None, "no leading $");
    }

    #[test]
    fn parse_js_api_prefix_rejects_missing_suffix() {
        // No `.API` tail — could be a cluster prefix or a typo. Refuse.
        assert_eq!(parse_js_api_prefix("$JS.node-0"), None);
        assert_eq!(parse_js_api_prefix("$JS.node-0.api"), None, "case-sensitive");
    }

    #[test]
    fn parse_js_api_prefix_rejects_empty_domain() {
        // `$JS..API` — a structurally valid envelope around an empty domain.
        // Treat as invalid; an empty domain can't be a routing key.
        assert_eq!(parse_js_api_prefix("$JS..API"), None);
        assert_eq!(parse_js_api_prefix(""), None);
    }

    #[test]
    fn parse_js_api_prefix_rejects_garbage() {
        assert_eq!(parse_js_api_prefix("garbage"), None);
        assert_eq!(parse_js_api_prefix("$"), None);
        assert_eq!(parse_js_api_prefix("$JS"), None);
    }

    // ── derive_live_sources: pair (config, runtime) into a fleet view ─────
    // Step 2 of Admin v0.2: the configured `Source` list tells us WHICH
    // leaves are registered (via external.api_prefix → domain); the runtime
    // `SourceInfo` list tells us their delivery state (active, lag).
    // Pairing is positional — the standard NATS layout. Pure function.

    use async_nats::jetstream::stream::SourceInfo;
    use std::time::Duration;

    fn cfg_src(name: &str, api: Option<&str>) -> stream::Source {
        stream::Source {
            name: name.to_string(),
            external: api.map(|p| stream::External {
                api_prefix: p.to_string(),
                delivery_prefix: None,
            }),
            ..Default::default()
        }
    }

    fn runtime_src(name: &str, lag: u64, active: Option<Duration>) -> SourceInfo {
        SourceInfo {
            name: name.to_string(),
            lag,
            active,
            filter_subject: None,
            subject_transform_dest: None,
            subject_transforms: vec![],
        }
    }

    #[test]
    fn derive_live_sources_returns_empty_for_empty_inputs() {
        let out = derive_live_sources(&[], &[]);
        assert!(out.is_empty());
    }

    #[test]
    fn derive_live_sources_pairs_config_and_runtime_positionally() {
        let cfg = vec![
            cfg_src("events", Some("$JS.node-0.API")),
            cfg_src("events", Some("$JS.node-1.API")),
        ];
        let rt = vec![
            runtime_src("events", 0, Some(Duration::from_millis(50))),
            runtime_src("events", 3, Some(Duration::from_secs(2))),
        ];
        let out = derive_live_sources(&cfg, &rt);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].name, "events");
        assert_eq!(out[0].host.as_deref(), Some("node-0"));
        assert_eq!(out[0].api_prefix.as_deref(), Some("$JS.node-0.API"));
        assert_eq!(out[0].lag, 0);
        assert_eq!(out[0].active_ms, Some(50));
        assert_eq!(out[1].host.as_deref(), Some("node-1"));
        assert_eq!(out[1].lag, 3);
        assert_eq!(out[1].active_ms, Some(2000));
    }

    #[test]
    fn derive_live_sources_handles_local_source_without_external() {
        // A source without `external` is a same-domain source (e.g. a hub
        // sourcing a local leaf in the rare cluster setup). Host is None;
        // api_prefix is None; rest passes through.
        let cfg = vec![cfg_src("events", None)];
        let rt = vec![runtime_src("events", 0, None)];
        let out = derive_live_sources(&cfg, &rt);
        assert_eq!(out.len(), 1);
        assert!(out[0].host.is_none());
        assert!(out[0].api_prefix.is_none());
        assert!(out[0].active_ms.is_none(), "Active=None means never seen");
    }

    #[test]
    fn derive_live_sources_tolerates_length_mismatch() {
        // Defensive: if the runtime hasn't caught up to the configured
        // sources (just-added, not yet started), zip-min the lists rather
        // than panic. The shorter list bounds the output.
        let cfg = vec![
            cfg_src("events", Some("$JS.node-0.API")),
            cfg_src("events", Some("$JS.node-1.API")),
            cfg_src("events", Some("$JS.node-2.API")),
        ];
        let rt = vec![runtime_src("events", 0, None)]; // only one runtime entry
        let out = derive_live_sources(&cfg, &rt);
        assert_eq!(out.len(), 1, "min(cfg.len(), rt.len())");
        assert_eq!(out[0].host.as_deref(), Some("node-0"));
    }

    /// Phase 4.5 RED — when runtime entries don't align with config by
    /// name at the same index, the function must NOT silently attribute
    /// the wrong runtime stats. NATS *currently* preserves config order in
    /// the response, but the reviewer's concern (PR #58 review, 🟠) is the
    /// kind of silent-wrongness that bites under concurrency or a future
    /// NATS behavioral change. Our sources all share the same name within
    /// a topology shape (every leaf source = `"events"`), so name alone
    /// can't disambiguate — but a positional MISMATCH on name is a clear
    /// signal that the list semantics drifted, and we should degrade
    /// gracefully (config-only view) rather than misattribute.
    #[test]
    fn derive_live_sources_degrades_when_names_misalign_positionally() {
        let cfg = vec![
            cfg_src("events", Some("$JS.node-0.API")),
            cfg_src("events-agg", Some("$JS.hub-b.API")),
        ];
        // Runtime returns them in REVERSED order — names don't align at
        // matching indexes. Currently we'd silently attribute hub-b's
        // lag=5 to node-0 and vice versa.
        let rt = vec![
            runtime_src("events-agg", 5, Some(Duration::from_millis(100))),
            runtime_src("events", 0, Some(Duration::from_millis(50))),
        ];
        let out = derive_live_sources(&cfg, &rt);
        assert_eq!(out.len(), 2);
        // Config-only degraded values — names + hosts preserved, but lag
        // and active_ms come back as the safe defaults rather than the
        // misattributed runtime numbers.
        assert_eq!(out[0].name, "events");
        assert_eq!(out[0].host.as_deref(), Some("node-0"));
        assert_eq!(
            out[0].lag, 0,
            "lag must be the safe default, NOT the misattributed runtime value (5)"
        );
        assert!(
            out[0].active_ms.is_none(),
            "active_ms must be None on degraded pair, NOT the misattributed runtime value (100ms)"
        );
        assert_eq!(out[1].name, "events-agg");
        assert_eq!(out[1].host.as_deref(), Some("hub-b"));
        assert_eq!(out[1].lag, 0);
        assert!(out[1].active_ms.is_none());
    }

    #[test]
    fn derive_live_sources_handles_unparseable_api_prefix() {
        // A source with a malformed api_prefix gets host=None but keeps
        // the raw api_prefix for the UI to surface (helpful for debugging
        // a misconfigured deployment).
        let cfg = vec![cfg_src("events", Some("not-a-valid-prefix"))];
        let rt = vec![runtime_src("events", 0, None)];
        let out = derive_live_sources(&cfg, &rt);
        assert_eq!(out.len(), 1);
        assert!(out[0].host.is_none());
        assert_eq!(out[0].api_prefix.as_deref(), Some("not-a-valid-prefix"));
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

    // ── T3 multi-hub mesh: hub-to-hub source registration ─────────────────

    #[test]
    fn peer_hub_register_adds_agg_source_to_empty_list() {
        let mut sources = vec![];
        let added = ensure_peer_hub_source(&mut sources, "hub-B");
        assert!(added);
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].name, "events-agg");
        let ext = sources[0].external.as_ref().expect("external set");
        assert_eq!(ext.api_prefix, "$JS.hub-B.API");
    }

    #[test]
    fn peer_hub_register_is_idempotent() {
        let mut sources = vec![];
        ensure_peer_hub_source(&mut sources, "hub-B");
        let again = ensure_peer_hub_source(&mut sources, "hub-B");
        assert!(!again);
        assert_eq!(sources.len(), 1);
    }

    #[test]
    fn peer_hub_and_leaf_sources_coexist() {
        // A hub aggregate carries BOTH leaf sources (name=events) and peer
        // hub sources (name=events-agg). The merge functions must not
        // collide with each other.
        let mut sources = vec![];
        ensure_self_source(&mut sources, "leaf-maxs-air");
        ensure_peer_hub_source(&mut sources, "hub-B");
        assert_eq!(sources.len(), 2);
        let names: Vec<_> = sources.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"events"));
        assert!(names.contains(&"events-agg"));
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
