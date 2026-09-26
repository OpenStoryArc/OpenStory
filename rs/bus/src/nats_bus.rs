//! NatsBus — default Bus implementation backed by NATS JetStream.
//!
//! Provides durable event streams, multi-store fan-out, and replay from
//! stream history for boot recovery.

use anyhow::{Context, Result};
use async_nats::jetstream::context::ConsumerInfoErrorKind;
use async_nats::jetstream::{self, stream};
use async_trait::async_trait;
use futures::StreamExt;
use tokio::sync::mpsc;

use crate::{
    Acker, Bus, BusSubscription, Delivery, DurableSpec, DurableSubscription, IngestBatch, StartFrom,
};

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
    /// The events cap in bytes (K-08), from OPEN_STORY_EVENTS_MAX_BYTES.
    events_cap: i64,
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
    /// The highest stream sequence each durable consumer of this process has
    /// acknowledged, by `{stream}/{durable}` (B-11). When the server loses a
    /// durable (deleted, or its stream recreated), the recreated consumer
    /// starts after this instead of at its first-start policy.
    acked: std::sync::Arc<
        std::sync::Mutex<
            std::collections::HashMap<String, std::sync::Arc<std::sync::atomic::AtomicU64>>,
        >,
    >,
    /// Each stream's last sequence when `ensure_streams` ran — before this
    /// process published anything (B-11: where `StartFrom::SinceBoot`
    /// starts a durable that does not exist yet).
    boot_marks: std::sync::Mutex<std::collections::HashMap<String, u64>>,
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
            acked: Default::default(),
            boot_marks: Default::default(),
            // K-08: the cap is a knob at the edge; the builders stay pure.
            events_cap: events_cap_from(
                std::env::var("OPEN_STORY_EVENTS_MAX_BYTES").ok().as_deref(),
            ),
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
            .get_or_create_stream(events_stream_config(host, fed_active, self.events_cap))
            .await
            .context("failed to create/get 'events' JetStream stream")?;

        // Local stream — own sessions a node chooses NOT to publish
        // (`publish_sessions = false`). Bound to `local.>`, it is NEVER a
        // federation source (the hub/peers source the `events` stream by
        // name), so these events are stored + visible locally but never
        // leave the machine. Always created so subscribers can read it
        // regardless of the publish switch.
        self.jetstream
            .get_or_create_stream(local_stream_config(self.events_cap))
            .await
            .context("failed to create/get 'local' JetStream stream")?;

        // Ops stream (M-06): proposals and commands from the tier-1 hands,
        // `ops.>`. Authored ops history, never agent history; a month of it.
        self.jetstream
            .get_or_create_stream(stream::Config {
                name: "ops".to_string(),
                subjects: vec!["ops.>".to_string()],
                retention: stream::RetentionPolicy::Limits,
                max_bytes: 67_108_864,
                max_age: std::time::Duration::from_secs(30 * 24 * 3600),
                ..Default::default()
            })
            .await
            .context("failed to create/get 'ops' JetStream stream")?;

        // Presence stream (P-01, P-04): each node's heartbeat on
        // `presence.{host}.{principal}`. Its own observed family: never
        // `events.*` (it is not agent history) and never `ui.*` (nobody
        // authored it). A week of history so DORA reads (group D) have
        // something to read. Same host rule as events under federation.
        self.jetstream
            .get_or_create_stream(presence_stream_config(host, fed_active))
            .await
            .context("failed to create/get 'presence' JetStream stream")?;

        if let Some(fed) = &self.federation {
            match &fed.peers {
                FederationPeers::Hub { hub_domain } => {
                    self.jetstream
                        .get_or_create_stream(events_mirror_config(hub_domain, self.events_cap))
                        .await
                        .context("failed to create/get 'events-mirror' (Hub) stream")?;
                    self.jetstream
                        .get_or_create_stream(presence_mirror_config(hub_domain))
                        .await
                        .context("failed to create/get 'presence-mirror' (Hub) stream")?;
                    register_self_with_hub(self.client.clone(), hub_domain, &fed.host)
                        .await
                        .context("failed to self-register as source on hub events-agg")?;
                }
                FederationPeers::Mesh { peer_domains } => {
                    self.jetstream
                        .get_or_create_stream(events_mirror_mesh_config(peer_domains))
                        .await
                        .context("failed to create/get 'events-mirror' (Mesh) stream")?;
                    self.jetstream
                        .get_or_create_stream(presence_mirror_mesh_config(peer_domains))
                        .await
                        .context("failed to create/get 'presence-mirror' (Mesh) stream")?;
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

        // UI stream — the AUTHORED agent-in-UI namespace (interactions, control,
        // annotations published to `ui.*` by the server). Interest-based (kept
        // only while a subscriber — e.g. the MCP's subscribe_ui_state — is
        // attached), so it's a light live-follow channel; the durable copy of
        // interactions already lives in SQLite. STRICTLY separate from the
        // observed, read-only `events.*` stream — the sovereignty partition,
        // enforced at the subject layer (server: ui_events::ui_subject, which
        // only ever emits `ui.*`). Needed so `publish_bytes` to `ui.>` doesn't
        // error at the broker. (Durable/limits retention is the future
        // enhancement for resumable replay — see the NATS-unification backlog.)
        self.jetstream
            .get_or_create_stream(stream::Config {
                name: "ui".to_string(),
                subjects: vec!["ui.>".to_string()],
                retention: stream::RetentionPolicy::Interest,
                ..Default::default()
            })
            .await
            .context("failed to create/get 'ui' JetStream stream")?;

        // B-11: where "since boot" is, per stream a durable may read.
        for name in [
            "events",
            "local",
            "presence",
            "events-mirror",
            "presence-mirror",
        ] {
            if let Ok(mut st) = self.jetstream.get_stream(name).await {
                if let Ok(info) = st.info().await {
                    let seq = info.state.last_sequence;
                    self.boot_marks
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .insert(name.to_string(), seq);
                }
            }
        }

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
        self.jetstream
            .get_or_create_stream(presence_aggregate_config())
            .await
            .context("failed to create/get 'presence-agg' JetStream stream")?;
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
    ) -> Result<tokio::task::JoinHandle<()>> {
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
        Ok(tokio::spawn(async move {
            loop {
                // The server deletes an ephemeral push consumer whose
                // connection lost interest past its inactivity threshold (a
                // stall, a slow-consumer disconnect). The subscription then
                // just goes quiet, forever, and the actor looks alive. So
                // after a quiet spell ask whether the consumer still exists;
                // if the server says it is gone, end, and the actor's
                // supervisor subscribes afresh. A transient error (NATS
                // reconnecting) keeps waiting: re-delivering the stream is
                // not free.
                let next = match tokio::time::timeout(CONSUMER_QUIET_PROBE, messages.next()).await {
                    Ok(next) => next,
                    Err(_quiet) => match consumer.get_info().await {
                        Err(e)
                            if matches!(
                                e.kind(),
                                ConsumerInfoErrorKind::NotFound
                                    | ConsumerInfoErrorKind::StreamNotFound
                            ) =>
                        {
                            eprintln!(
                                "bus[{label}]: event=bus_consumer_lost error={e}; ending the subscription"
                            );
                            break;
                        }
                        _ => continue,
                    },
                };
                let msg = match next {
                    Some(Ok(msg)) => msg,
                    Some(Err(e)) => {
                        eprintln!(
                            "bus[{label}]: event=bus_consumer_lost error={e}; ending the subscription"
                        );
                        break;
                    }
                    None => break,
                };
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
        }))
    }

    /// Subscribe to a named stream filtered by `pattern`, yielding typed
    /// `IngestBatch`es (the same decode as `Bus::subscribe`, which is hardwired
    /// to the `events` stream). For the authored `ui` stream
    /// (`subscribe_typed("ui", "ui.>")`) — now that authored events are proper
    /// CloudEvents published as IngestBatches, the ui stream uses the SAME typed
    /// path as observed events; no raw-bytes special-case remains. Sovereignty
    /// note: this only consumes the named stream; the observed read-only
    /// `events.*` path is untouched.
    pub async fn subscribe_typed(
        &self,
        stream_name: &str,
        pattern: &str,
    ) -> Result<mpsc::Receiver<IngestBatch>> {
        let (tx, rx) = mpsc::channel(256);
        let _forwarder = self.spawn_consumer(tx, stream_name, pattern).await?;
        Ok(rx)
    }
}

impl NatsBus {
    /// B-11: a durable pull consumer named `spec.name` on `stream_name`,
    /// filtered by `pattern`, pumping each message into `tx` with its
    /// acknowledgement. The actor acks after handling, so a restart resumes
    /// from what it had not acknowledged; the server lets at most
    /// `CONSUMER_MAX_ACK_PENDING` messages be in flight, and each pull asks
    /// for at most `PULL_BATCH` messages and `PULL_MAX_BYTES`, so a backlog
    /// of any size drains at bounded memory.
    async fn spawn_durable(
        &self,
        tx: mpsc::Sender<Delivery>,
        stream_name: &str,
        pattern: &str,
        spec: &DurableSpec,
    ) -> Result<tokio::task::JoinHandle<()>> {
        let stream = self
            .jetstream
            .get_stream(stream_name)
            .await
            .with_context(|| format!("failed to get '{stream_name}' stream"))?;
        let floor = {
            let mut acked = self.acked.lock().unwrap_or_else(|e| e.into_inner());
            acked
                .entry(format!("{stream_name}/{}", spec.name))
                .or_default()
                .clone()
        };

        // An existing durable resumes where it is — unless messages a dead
        // run was handed still wait for their ack (they would hold the
        // in-flight window until the ack wait expires) or its shape is not
        // ours: then it is recreated from its ack floor.
        let boot_mark = self
            .boot_marks
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(stream_name)
            .copied();
        let mut start = first_start_policy(
            spec.first_start,
            floor.load(std::sync::atomic::Ordering::Relaxed),
            boot_mark,
        );
        match stream.consumer_info(&spec.name).await {
            Ok(info) => {
                if needs_reset(&info, pattern) {
                    start = jetstream::consumer::DeliverPolicy::ByStartSequence {
                        start_sequence: info.ack_floor.stream_sequence + 1,
                    };
                    eprintln!(
                        "bus[{stream_name}]: event=bus_consumer_reset consumer={} \
                         ack_pending={} ack_floor={}; recreating from the ack floor",
                        spec.name, info.num_ack_pending, info.ack_floor.stream_sequence
                    );
                    stream
                        .delete_consumer(&spec.name)
                        .await
                        .with_context(|| format!("failed to reset consumer {}", spec.name))?;
                }
            }
            Err(e)
                if matches!(
                    e.kind(),
                    ConsumerInfoErrorKind::NotFound | ConsumerInfoErrorKind::StreamNotFound
                ) =>
            {
                eprintln!(
                    "bus[{stream_name}]: event=bus_consumer_created consumer={} start={start:?}",
                    spec.name
                );
            }
            Err(e) => {
                return Err(anyhow::anyhow!(e))
                    .with_context(|| format!("failed to read consumer {}", spec.name))
            }
        }
        let consumer = stream
            .get_or_create_consumer(&spec.name, durable_config(&spec.name, pattern, start))
            .await
            .with_context(|| {
                format!("failed to create durable {} on '{stream_name}'", spec.name)
            })?;
        let mut messages = consumer
            .stream()
            .max_messages_per_batch(PULL_BATCH)
            .max_bytes_per_batch(PULL_MAX_BYTES)
            .expires(std::time::Duration::from_secs(30))
            .heartbeat(std::time::Duration::from_secs(5))
            .messages()
            .await
            .with_context(|| format!("failed to pull from {} on '{stream_name}'", spec.name))?;

        let label = format!("{stream_name}/{}", spec.name);
        let client = self.client.clone();
        Ok(tokio::spawn(async move {
            loop {
                // As in `spawn_consumer`: after a quiet spell, or a missed
                // heartbeat, ask whether the consumer still exists; if the
                // server lost it, end, and the actor's supervisor
                // resubscribes (and recreates it after the last ack).
                let next = match tokio::time::timeout(CONSUMER_QUIET_PROBE, messages.next()).await {
                    Ok(Some(Err(e)))
                        if matches!(
                            e.kind(),
                            jetstream::consumer::pull::MessagesErrorKind::MissingHeartbeat
                        ) =>
                    {
                        None
                    }
                    Ok(next) => Some(next),
                    Err(_quiet) => None,
                };
                let next = match next {
                    Some(next) => next,
                    None => match consumer.get_info().await {
                        Err(e)
                            if matches!(
                                e.kind(),
                                ConsumerInfoErrorKind::NotFound
                                    | ConsumerInfoErrorKind::StreamNotFound
                            ) =>
                        {
                            eprintln!(
                                "bus[{label}]: event=bus_consumer_lost error={e}; ending the subscription"
                            );
                            break;
                        }
                        _ => continue,
                    },
                };
                let msg = match next {
                    Some(Ok(msg)) => msg,
                    Some(Err(e)) => {
                        eprintln!(
                            "bus[{label}]: event=bus_consumer_lost error={e}; ending the subscription"
                        );
                        break;
                    }
                    None => break,
                };
                let seq = msg.info().map(|i| i.stream_sequence).unwrap_or(0);
                let ack = NatsAck {
                    client: client.clone(),
                    reply: msg.reply.clone(),
                    seq,
                    floor: floor.clone(),
                };
                match serde_json::from_slice::<IngestBatch>(&msg.payload) {
                    Ok(batch) => {
                        let d = Delivery {
                            batch,
                            ack: Acker::nats(ack),
                        };
                        if tx.send(d).await.is_err() {
                            break; // receiver dropped
                        }
                    }
                    Err(e) => {
                        // Nothing an actor can handle: acknowledge it so it
                        // does not come back on every restart.
                        eprintln!("bus[{label}]: failed to deserialize IngestBatch: {e}");
                        ack.ack().await;
                    }
                }
            }
        }))
    }
}

/// The acknowledgement of one JetStream message, held by the actor until it
/// has handled the batch (B-11). Acking publishes to the message's reply
/// subject, as `jetstream::Message::ack` does, without keeping the payload
/// alive, and records the sequence as this process's resume point.
pub struct NatsAck {
    client: async_nats::Client,
    reply: Option<async_nats::Subject>,
    seq: u64,
    floor: std::sync::Arc<std::sync::atomic::AtomicU64>,
}

impl NatsAck {
    pub(crate) async fn ack(self) {
        let Some(reply) = self.reply else {
            return;
        };
        match self.client.publish(reply, "".into()).await {
            Ok(()) => {
                self.floor
                    .fetch_max(self.seq, std::sync::atomic::Ordering::Relaxed);
            }
            Err(e) => eprintln!("bus: failed to ack message {}: {e}", self.seq),
        }
    }
}

/// Messages one durable consumer may have in flight, unacknowledged (B-11):
/// handed to the actor, queued for it, or buffered by the client. A batch
/// is up to a few MB decoded, so this is the whole memory bound of a drain.
pub const CONSUMER_MAX_ACK_PENDING: i64 = 4;
/// Messages one pull asks for.
pub const PULL_BATCH: usize = 4;
/// Bytes one pull asks for: one maximal publish.
pub const PULL_MAX_BYTES: usize = PUBLISH_MAX_BYTES;
/// Deliveries queued between the forwarders and the actor.
pub const DELIVERY_QUEUE: usize = 2;
/// How long a handed-out message may go unacknowledged before the server
/// delivers it again. The in-flight window is small, so a minute is ample;
/// a redelivery is harmless (every actor is idempotent by event id).
pub const CONSUMER_ACK_WAIT: std::time::Duration = std::time::Duration::from_secs(60);

/// A durable nobody has pulled from for this long is removed by the server,
/// so a retired node, or a host renamed, leaves no consumer behind for
/// good. A node down longer than this starts again at its first-start
/// policy (persist and patterns re-read the stream once, bounded).
pub const CONSUMER_INACTIVE_THRESHOLD: std::time::Duration =
    std::time::Duration::from_secs(7 * 24 * 3600);

/// Where a durable that does not exist yet starts: after `acked` when this
/// process already acknowledged up to it (the server lost the consumer
/// mid-run), else the actor's first-start policy; `SinceBoot` starts after
/// `boot_mark`, the stream's last sequence before this process published
/// (from now when it is unknown). Pure.
pub fn first_start_policy(
    first: StartFrom,
    acked: u64,
    boot_mark: Option<u64>,
) -> jetstream::consumer::DeliverPolicy {
    use jetstream::consumer::DeliverPolicy;
    if acked > 0 {
        return DeliverPolicy::ByStartSequence {
            start_sequence: acked + 1,
        };
    }
    match first {
        StartFrom::All => DeliverPolicy::All,
        StartFrom::SinceBoot => match boot_mark {
            Some(mark) => DeliverPolicy::ByStartSequence {
                start_sequence: mark + 1,
            },
            None => DeliverPolicy::New,
        },
        StartFrom::New => DeliverPolicy::New,
        StartFrom::LastPerSubject => DeliverPolicy::LastPerSubject,
    }
}

/// The durable pull consumer config (B-11). Pure.
pub fn durable_config(
    name: &str,
    pattern: &str,
    start: jetstream::consumer::DeliverPolicy,
) -> jetstream::consumer::pull::Config {
    jetstream::consumer::pull::Config {
        durable_name: Some(name.to_string()),
        filter_subject: pattern.to_string(),
        deliver_policy: start,
        ack_policy: jetstream::consumer::AckPolicy::Explicit,
        ack_wait: CONSUMER_ACK_WAIT,
        max_ack_pending: CONSUMER_MAX_ACK_PENDING,
        inactive_threshold: CONSUMER_INACTIVE_THRESHOLD,
        ..Default::default()
    }
}

/// Whether an existing durable must be recreated from its ack floor before
/// this run reads it: messages a previous run was handed still wait for an
/// ack (they would fill the in-flight window until the ack wait expires),
/// or it is not the consumer this code creates (another filter, another
/// in-flight bound, not a pull consumer). Pure.
pub fn needs_reset(info: &jetstream::consumer::Info, pattern: &str) -> bool {
    info.num_ack_pending > 0
        || info.config.filter_subject != pattern
        || info.config.max_ack_pending != CONSUMER_MAX_ACK_PENDING
        || info.config.deliver_subject.is_some()
}

/// How long a forwarder's subscription may stay quiet before it asks the
/// server whether its consumer still exists (one API call per quiet spell).
const CONSUMER_QUIET_PROBE: std::time::Duration = std::time::Duration::from_secs(5);

/// One subscription's forwarders share one channel: when any of them ends
/// (its consumer was deleted, the receiver dropped), stop the rest, so the
/// channel closes and the actor sees the subscription end instead of
/// waiting on the survivors forever.
fn link_forwarders(forwarders: Vec<tokio::task::JoinHandle<()>>) {
    tokio::spawn(async move {
        let (_, _, rest) = futures::future::select_all(forwarders).await;
        for f in rest {
            f.abort();
        }
    });
}

/// A failed spawn part-way through a subscription stops the forwarders
/// already started for it, so no orphan keeps reading for nobody.
fn abort_on_err<T>(spawned: Result<T>, started: &[tokio::task::JoinHandle<()>]) -> Result<T> {
    if spawned.is_err() {
        for f in started {
            f.abort();
        }
    }
    spawned
}

/// Streams that have a `{name}-mirror` twin under federation (P-04).
const MIRRORED_STREAMS: [&str; 2] = ["events", "presence"];

/// Bytes per JetStream publish, under the 8 MB `max_payload` with room for
/// headers and the subject. Larger batches are split (see `split_batch`).
pub const PUBLISH_MAX_BYTES: usize = 4 * 1024 * 1024;

/// The bytes one publish may carry on a server whose max_payload is
/// `server_max`: nine tenths of it, for headers and the subject, capped at
/// `PUBLISH_MAX_BYTES`; the cap when the limit is unknown. A managed
/// standalone nats-server runs the 1 MiB default. Pure.
pub fn publish_budget(server_max: usize) -> usize {
    if server_max == 0 {
        return PUBLISH_MAX_BYTES;
    }
    (server_max * 9 / 10).min(PUBLISH_MAX_BYTES)
}

impl NatsBus {
    async fn publish_one(&self, subject: &str, payload: Vec<u8>) -> Result<()> {
        self.jetstream
            .publish(subject.to_string(), payload.into())
            .await
            .with_context(|| format!("failed to publish to {subject}"))?
            .await
            .with_context(|| format!("failed to confirm publish to {subject}"))?;
        Ok(())
    }
}

#[async_trait]
impl Bus for NatsBus {
    /// The real client state (H-01). `Connected` only; `Pending` and
    /// `Disconnected` both mean a publish would not reach the server now.
    fn is_active(&self) -> bool {
        self.client.connection_state() == async_nats::connection::State::Connected
    }

    /// Bytes, messages, and cap for every stream this node declares (H-04).
    /// A stream that does not exist on this server (federation-only ones on
    /// a solo node) is simply absent.
    async fn stream_stats(&self) -> Vec<crate::StreamStats> {
        let mut out = Vec::new();
        for name in [
            "events",
            "local",
            "patterns",
            "ui",
            "changes",
            "presence",
            "ops",
            "events-mirror",
            "events-agg",
            "presence-mirror",
            "presence-agg",
        ] {
            let Ok(mut stream) = self.jetstream.get_stream(name).await else {
                continue;
            };
            let Ok(info) = stream.info().await else {
                continue;
            };
            out.push(crate::StreamStats::new(
                name,
                info.state.bytes,
                info.state.messages,
                info.config.max_bytes,
            ));
        }
        out
    }

    async fn publish(&self, subject: &str, batch: &IngestBatch) -> Result<()> {
        let payload = serde_json::to_vec(batch).context("failed to serialize IngestBatch")?;

        // E-05: the watcher batches by count; NATS caps by bytes (8 MB
        // max_payload). A batch over the budget is split in order and
        // published piece by piece, so a hundred-event Grok batch of 1 MB
        // lines no longer fails whole.
        // The server says how much one publish may carry; read it now, not at
        // connect, because the INFO frame can land after connect returns.
        let budget = publish_budget(self.client.server_info().max_payload);
        if payload.len() > budget && batch.events.len() > 1 {
            for piece in crate::split::split_batch(batch.clone(), budget) {
                let bytes =
                    serde_json::to_vec(&piece).context("failed to serialize IngestBatch")?;
                self.publish_one(subject, bytes).await?;
            }
            return Ok(());
        }
        self.publish_one(subject, payload).await
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
        let mut forwarders = vec![self
            .spawn_consumer(tx.clone(), "events", pattern)
            .await
            .context("failed to spawn 'events' consumer")?];
        // Own local-only events (`publish_sessions = false`) live in the
        // `local` stream, which federation never sources — read it too so a
        // node always sees its own sessions, published or not. Filter to
        // `local.>` so the `events.>`-shaped `pattern` doesn't exclude them.
        let local = self
            .spawn_consumer(tx.clone(), "local", "local.>")
            .await
            .context("failed to spawn 'local' consumer");
        forwarders.push(abort_on_err(local, &forwarders)?);
        if self.federation.is_some() {
            let mirror = self
                .spawn_consumer(tx, "events-mirror", pattern)
                .await
                .context("failed to spawn 'events-mirror' consumer");
            forwarders.push(abort_on_err(mirror, &forwarders)?);
        }
        link_forwarders(forwarders);
        Ok(BusSubscription { receiver: rx })
    }

    async fn subscribe_stream(&self, stream: &str, pattern: &str) -> Result<BusSubscription> {
        let (tx, rx) = mpsc::channel(256);
        let mut forwarders = vec![self
            .spawn_consumer(tx.clone(), stream, pattern)
            .await
            .with_context(|| format!("failed to spawn '{stream}' consumer"))?];
        // P-04: a mirrored family reads the fleet's copy too when federated,
        // so the presence table sees every node, not just this one.
        if self.federation.is_some() && MIRRORED_STREAMS.contains(&stream) {
            let mirror = format!("{stream}-mirror");
            let spawned = self
                .spawn_consumer(tx, &mirror, pattern)
                .await
                .with_context(|| format!("failed to spawn '{mirror}' consumer"));
            forwarders.push(abort_on_err(spawned, &forwarders)?);
        }
        link_forwarders(forwarders);
        Ok(BusSubscription { receiver: rx })
    }

    async fn subscribe_durable(
        &self,
        stream: &str,
        pattern: &str,
        spec: &DurableSpec,
    ) -> Result<DurableSubscription> {
        // The same streams `subscribe` / `subscribe_stream` read, each through
        // its own durable of the same name.
        let mut sources: Vec<(String, String)> = vec![(stream.to_string(), pattern.to_string())];
        if stream == "events" {
            sources.push(("local".to_string(), "local.>".to_string()));
        }
        if self.federation.is_some() && MIRRORED_STREAMS.contains(&stream) {
            sources.push((format!("{stream}-mirror"), pattern.to_string()));
        }
        let (tx, rx) = mpsc::channel(DELIVERY_QUEUE);
        let mut forwarders = Vec::new();
        for (name, filter) in &sources {
            let spawned = self
                .spawn_durable(tx.clone(), name, filter, spec)
                .await
                .with_context(|| format!("failed to spawn '{name}' durable {}", spec.name));
            forwarders.push(abort_on_err(spawned, &forwarders)?);
        }
        link_forwarders(forwarders);
        Ok(DurableSubscription { receiver: rx })
    }

    async fn replay(&self, pattern: &str) -> Result<Vec<IngestBatch>> {
        // Solo: replay `events`. Federation: replay BOTH `events`
        // (own host's namespace) and `events-mirror` (fleet sourced from hub
        // aggregate). Order within each stream is preserved; cross-stream
        // ordering is not guaranteed (and event-ID dedup downstream makes
        // ordering irrelevant for correctness).
        let mut all = replay_one(&self.jetstream, "events", pattern)
            .await
            .context("replay 'events' failed")?;
        if self.federation.is_some() {
            let mirror = replay_one(&self.jetstream, "events-mirror", pattern)
                .await
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

    let info = stream
        .info()
        .await
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
/// The smallest events cap the knob accepts (K-08): below this a stream
/// cannot hold one batch.
pub(crate) const EVENTS_CAP_FLOOR: i64 = 65_536;

/// The events cap from `OPEN_STORY_EVENTS_MAX_BYTES`: the default when
/// unset or unreadable, the floor when too small. Pure.
pub(crate) fn events_cap_from(env: Option<&str>) -> i64 {
    match env.and_then(|s| s.trim().parse::<i64>().ok()) {
        Some(n) if n >= EVENTS_CAP_FLOOR => n,
        Some(_) => EVENTS_CAP_FLOOR,
        None => EVENTS_MAX_BYTES,
    }
}
/// P-04: presence beats keep a week of history under a 64 MB cap, on the
/// node's own stream, its mirror, and the hub aggregate alike.
pub(crate) const PRESENCE_MAX_BYTES: i64 = 67_108_864;
pub(crate) const PRESENCE_MAX_AGE: std::time::Duration =
    std::time::Duration::from_secs(7 * 24 * 3600);

/// The local `events` stream this node publishes into.
///
/// - **Solo** (`federation = false`): binds `events.>` — captures everything,
///   unchanged from pre-federation behavior.
/// - **Federation** (`federation = true`): binds ONLY `events.{host}.>` so
///   the racy core leafnode propagation can't cross-pollinate streams and
///   peers can't double-count (the load-bearing spike finding). Publish-only,
///   no sources. Same shape for Hub (T2/T3) and Mesh (T1) — the `events-mirror`
///   carries the topology difference.
pub(crate) fn events_stream_config(host: &str, federation: bool, cap: i64) -> stream::Config {
    let subjects = if federation {
        vec![format!("events.{host}.>")]
    } else {
        vec!["events.>".to_string()]
    };
    stream::Config {
        name: "events".to_string(),
        subjects,
        retention: stream::RetentionPolicy::Limits,
        max_bytes: cap,
        ..Default::default()
    }
}

/// The `local` stream — own sessions a node keeps private to itself
/// (`publish_sessions = false`). Bound to `local.>` and deliberately NOT a
/// federation source (peers/hubs source the `events` stream by name), so its
/// events are persisted and visible on this node but never propagate. Host is
/// irrelevant — a node only ever publishes its OWN local events here.
pub(crate) fn local_stream_config(cap: i64) -> stream::Config {
    stream::Config {
        name: "local".to_string(),
        subjects: vec!["local.>".to_string()],
        retention: stream::RetentionPolicy::Limits,
        max_bytes: cap,
        ..Default::default()
    }
}

/// The `presence` stream (P-01, P-04): each node's beats. Solo it binds
/// `presence.>`; federated it binds only `presence.{host}.>`, the same rule
/// as events, so leafnode propagation cannot double-count a beat that also
/// arrives through the mirror.
pub(crate) fn presence_stream_config(host: &str, federation: bool) -> stream::Config {
    let subjects = if federation {
        vec![format!("presence.{host}.>")]
    } else {
        vec!["presence.>".to_string()]
    };
    stream::Config {
        name: "presence".to_string(),
        subjects,
        retention: stream::RetentionPolicy::Limits,
        max_bytes: PRESENCE_MAX_BYTES,
        max_age: PRESENCE_MAX_AGE,
        ..Default::default()
    }
}

/// The source-only presence mirror (P-04): everyone else's beats, pulled
/// from the hub's `presence-agg` across the domain boundary. The fleet's
/// presence on a node is `presence ∪ presence-mirror`.
pub(crate) fn presence_mirror_config(hub_domain: &str) -> stream::Config {
    stream::Config {
        name: "presence-mirror".to_string(),
        subjects: vec![],
        retention: stream::RetentionPolicy::Limits,
        max_bytes: PRESENCE_MAX_BYTES,
        max_age: PRESENCE_MAX_AGE,
        sources: Some(vec![external_source("presence-agg", hub_domain)]),
        ..Default::default()
    }
}

/// Mesh form of the presence mirror: one source per peer's own `presence`.
pub(crate) fn presence_mirror_mesh_config(peer_domains: &[String]) -> stream::Config {
    stream::Config {
        name: "presence-mirror".to_string(),
        subjects: vec![],
        retention: stream::RetentionPolicy::Limits,
        max_bytes: PRESENCE_MAX_BYTES,
        max_age: PRESENCE_MAX_AGE,
        sources: Some(
            peer_domains
                .iter()
                .map(|peer| external_source("presence", peer))
                .collect(),
        ),
        ..Default::default()
    }
}

/// The hub's presence aggregate (P-04): source-only, leaves register their
/// `presence` streams into it the way they do on `events-agg`.
pub(crate) fn presence_aggregate_config() -> stream::Config {
    stream::Config {
        name: "presence-agg".to_string(),
        subjects: vec![],
        retention: stream::RetentionPolicy::Limits,
        max_bytes: PRESENCE_MAX_BYTES,
        max_age: PRESENCE_MAX_AGE,
        ..Default::default()
    }
}

/// A stream source in another JetStream domain, by stream name.
pub(crate) fn external_source(name: &str, domain: &str) -> stream::Source {
    stream::Source {
        name: name.to_string(),
        external: Some(stream::External {
            api_prefix: js_api_prefix(domain),
            delivery_prefix: None,
        }),
        ..Default::default()
    }
}

/// The source-only fleet mirror: pulls everyone else's events down from the
/// hub aggregate across the JetStream domain boundary. No subjects → no
/// publishers → structurally cannot loop (and JetStream self-origin loop
/// prevention excludes this leaf's own events). The complete fleet view on a
/// node is therefore `events ∪ events-mirror`.
pub(crate) fn events_mirror_config(hub_domain: &str, cap: i64) -> stream::Config {
    stream::Config {
        name: "events-mirror".to_string(),
        subjects: vec![],
        retention: stream::RetentionPolicy::Limits,
        max_bytes: cap,
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

/// Idempotently add the stream `name` in `domain` as a source. Keyed by
/// (name, domain); additive, never drops a peer. Serves `events` and
/// `presence` on their aggregates and peer hubs' aggregates alike.
pub(crate) fn ensure_named_source(
    sources: &mut Vec<stream::Source>,
    name: &str,
    domain: &str,
) -> bool {
    let want_prefix = js_api_prefix(domain);
    let already = sources.iter().any(|s| {
        s.name == name
            && s.external.as_ref().map(|e| e.api_prefix.as_str()) == Some(want_prefix.as_str())
    });
    if already {
        return false;
    }
    sources.push(external_source(name, domain));
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
    // P-04: peer hubs' presence aggregates are sourced the same way.
    for agg in ["events-agg", "presence-agg"] {
        register_peer_hubs_on(&my_js, agg, peer_hub_domains).await?;
    }
    Ok(())
}

async fn register_peer_hubs_on(
    my_js: &jetstream::Context,
    agg_name: &str,
    peer_hub_domains: &[String],
) -> Result<()> {
    let mut last_err: Option<anyhow::Error> = None;
    for attempt in 0..5u32 {
        let mut agg = match my_js.get_stream(agg_name).await {
            Ok(s) => s,
            Err(e) => {
                last_err = Some(anyhow::anyhow!("get_stream({agg_name}) failed: {e}"));
                tokio::time::sleep(std::time::Duration::from_millis(100 * (1u64 << attempt))).await;
                continue;
            }
        };
        let mut cfg = agg
            .info()
            .await
            .with_context(|| format!("info({agg_name}) failed"))?
            .config
            .clone();
        let mut sources = cfg.sources.unwrap_or_default();
        let mut any_added = false;
        for peer in peer_hub_domains {
            if ensure_named_source(&mut sources, agg_name, peer) {
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
                last_err = Some(anyhow::anyhow!("update_stream({agg_name}) failed: {e}"));
                tokio::time::sleep(std::time::Duration::from_millis(50 * (1u64 << attempt))).await;
            }
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("peer hub registration failed: unknown")))
        .with_context(|| format!("peer-hub registration on {agg_name} exhausted retries"))
}

pub(crate) async fn register_self_with_hub(
    client: async_nats::Client,
    hub_domain: &str,
    my_domain: &str,
) -> Result<()> {
    let hub_js = jetstream::with_domain(client, hub_domain);
    // P-04: the leaf's `presence` registers on `presence-agg` the same way
    // its `events` registers on `events-agg`.
    for (agg, stream) in [("events-agg", "events"), ("presence-agg", "presence")] {
        register_source_on_hub(&hub_js, agg, stream, my_domain).await?;
    }
    Ok(())
}

async fn register_source_on_hub(
    hub_js: &jetstream::Context,
    agg_name: &str,
    stream_name: &str,
    my_domain: &str,
) -> Result<()> {
    let mut last_err: Option<anyhow::Error> = None;
    for attempt in 0..5u32 {
        let mut agg = match hub_js.get_stream(agg_name).await {
            Ok(s) => s,
            Err(e) => {
                last_err = Some(anyhow::anyhow!("get_stream({agg_name}) failed: {e}"));
                tokio::time::sleep(std::time::Duration::from_millis(100 * (1u64 << attempt))).await;
                continue;
            }
        };
        let mut cfg = agg
            .info()
            .await
            .with_context(|| format!("info({agg_name}) failed"))?
            .config
            .clone();
        let mut sources = cfg.sources.unwrap_or_default();
        let added = ensure_named_source(&mut sources, stream_name, my_domain);
        cfg.sources = Some(sources);
        if !added {
            return Ok(());
        }
        match hub_js.update_stream(&cfg).await {
            Ok(_) => return Ok(()),
            Err(e) => {
                last_err = Some(anyhow::anyhow!("update_stream({agg_name}) failed: {e}"));
                tokio::time::sleep(std::time::Duration::from_millis(50 * (1u64 << attempt))).await;
            }
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("hub registration failed: unknown")))
        .with_context(|| format!("hub {agg_name} self-registration exhausted retries"))
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
        assert!(
            NatsBus::extract_token("nats://person-id:person-id-local-dev@localhost:4222").is_none()
        );
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
        let cfg = events_stream_config("maxs-air", false, EVENTS_MAX_BYTES);
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
        let cfg = events_stream_config("maxs-air", true, EVENTS_MAX_BYTES);
        assert_eq!(cfg.name, "events");
        assert_eq!(cfg.subjects, vec!["events.maxs-air.>".to_string()]);
        assert!(cfg.sources.is_none(), "local events stream is publish-only");
    }

    #[test]
    fn local_stream_binds_local_subject_and_is_never_a_source() {
        // The `local` stream holds own events a node keeps to itself
        // (`publish_sessions = false`). It binds `local.>` and has NO sources
        // — and crucially nothing ever sources IT (peers source the `events`
        // stream by name), so these events never leave the machine.
        let cfg = local_stream_config(EVENTS_MAX_BYTES);
        assert_eq!(cfg.name, "local");
        assert_eq!(cfg.subjects, vec!["local.>".to_string()]);
        assert!(cfg.sources.is_none());
    }

    #[test]
    fn mesh_mirror_sources_each_peer_directly() {
        // T1 solo multi-device: no hub aggregate, one source per peer.
        // The mirror collects every peer's local `events` stream across
        // their own JetStream domain.
        let cfg = events_mirror_mesh_config(&["laptop".to_string(), "phone".to_string()]);
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
        let cfg = events_mirror_config("hub", EVENTS_MAX_BYTES);
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
        let external = sources[0]
            .external
            .as_ref()
            .expect("must use external for cross-domain");
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
            assert_eq!(
                parse_js_api_prefix(&emitted),
                Some(d.to_string()),
                "round-trip {d}"
            );
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
        assert_eq!(
            parse_js_api_prefix("$JS.node-0.api"),
            None,
            "case-sensitive"
        );
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
        let added = ensure_named_source(&mut sources, "events", "leaf-maxs-air");
        assert!(added, "first registration adds a source");
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].name, "events");
        let ext = sources[0].external.as_ref().expect("external set");
        assert_eq!(ext.api_prefix, "$JS.leaf-maxs-air.API");
    }

    #[test]
    fn self_register_is_idempotent() {
        let mut sources = vec![];
        ensure_named_source(&mut sources, "events", "leaf-maxs-air");
        let added_again = ensure_named_source(&mut sources, "events", "leaf-maxs-air");
        assert!(!added_again, "re-registration is a no-op");
        assert_eq!(sources.len(), 1, "no duplicate source for the same domain");
    }

    // ── T3 multi-hub mesh: hub-to-hub source registration ─────────────────

    #[test]
    fn peer_hub_register_adds_agg_source_to_empty_list() {
        let mut sources = vec![];
        let added = ensure_named_source(&mut sources, "events-agg", "hub-B");
        assert!(added);
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].name, "events-agg");
        let ext = sources[0].external.as_ref().expect("external set");
        assert_eq!(ext.api_prefix, "$JS.hub-B.API");
    }

    #[test]
    fn peer_hub_register_is_idempotent() {
        let mut sources = vec![];
        ensure_named_source(&mut sources, "events-agg", "hub-B");
        let again = ensure_named_source(&mut sources, "events-agg", "hub-B");
        assert!(!again);
        assert_eq!(sources.len(), 1);
    }

    #[test]
    fn peer_hub_and_leaf_sources_coexist() {
        // A hub aggregate carries BOTH leaf sources (name=events) and peer
        // hub sources (name=events-agg). The merge functions must not
        // collide with each other.
        let mut sources = vec![];
        ensure_named_source(&mut sources, "events", "leaf-maxs-air");
        ensure_named_source(&mut sources, "events-agg", "hub-B");
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
        let added = ensure_named_source(&mut sources, "events", "leaf-maxs-air");
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

#[cfg(test)]
mod presence_federation_tests {
    //! P-04: presence crosses the leaf and the hub the way events do. The
    //! leaf template names no subjects; federation is JetStream sourcing,
    //! so the truth is in these pure config builders.
    use super::*;

    #[test]
    fn solo_presence_stream_binds_everything() {
        let cfg = presence_stream_config("maxs-air", false);
        assert_eq!(cfg.name, "presence");
        assert_eq!(cfg.subjects, vec!["presence.>".to_string()]);
        assert!(cfg.sources.is_none());
        assert!(matches!(cfg.retention, stream::RetentionPolicy::Limits));
        assert_eq!(cfg.max_bytes, PRESENCE_MAX_BYTES);
        assert_eq!(
            cfg.max_age, PRESENCE_MAX_AGE,
            "a week of beats for DORA reads"
        );
    }

    #[test]
    fn federation_presence_stream_binds_only_own_host() {
        // Same rule as events: own namespace only, so leafnode propagation
        // cannot double-count a beat that also arrives through the mirror.
        let cfg = presence_stream_config("maxs-air", true);
        assert_eq!(cfg.subjects, vec!["presence.maxs-air.>".to_string()]);
        assert!(cfg.sources.is_none());
    }

    #[test]
    fn presence_mirror_sources_the_hub_presence_aggregate() {
        let cfg = presence_mirror_config("hub");
        assert_eq!(cfg.name, "presence-mirror");
        assert!(cfg.subjects.is_empty(), "the mirror is source-only");
        let sources = cfg.sources.as_ref().expect("sources");
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].name, "presence-agg");
        assert_eq!(
            sources[0].external.as_ref().map(|e| e.api_prefix.as_str()),
            Some("$JS.hub.API")
        );
        assert_eq!(cfg.max_age, PRESENCE_MAX_AGE);
    }

    #[test]
    fn presence_aggregate_is_source_only() {
        let cfg = presence_aggregate_config();
        assert_eq!(cfg.name, "presence-agg");
        assert!(cfg.subjects.is_empty());
        assert_eq!(cfg.max_bytes, PRESENCE_MAX_BYTES);
        assert_eq!(cfg.max_age, PRESENCE_MAX_AGE);
    }

    #[test]
    fn mesh_presence_mirror_sources_each_peer() {
        let cfg = presence_mirror_mesh_config(&["laptop".to_string(), "phone".to_string()]);
        assert_eq!(cfg.name, "presence-mirror");
        let sources = cfg.sources.as_ref().expect("sources");
        assert_eq!(sources.len(), 2);
        for s in sources {
            assert_eq!(
                s.name, "presence",
                "mesh sources each peer's own presence stream"
            );
        }
    }

    #[test]
    fn self_registration_is_keyed_by_stream_name_and_domain() {
        // One helper serves both aggregates: the leaf's `events` on
        // `events-agg`, the leaf's `presence` on `presence-agg`.
        let mut sources = vec![];
        assert!(ensure_named_source(&mut sources, "presence", "leaf-1"));
        assert!(
            !ensure_named_source(&mut sources, "presence", "leaf-1"),
            "idempotent"
        );
        assert!(
            ensure_named_source(&mut sources, "events", "leaf-1"),
            "a different stream is a different source"
        );
        assert_eq!(sources.len(), 2);
        assert_eq!(sources[0].name, "presence");
        assert_eq!(
            sources[0].external.as_ref().map(|e| e.api_prefix.as_str()),
            Some("$JS.leaf-1.API")
        );
    }
}

#[cfg(test)]
mod events_cap_tests {
    //! K-08: the events cap is a knob, so a test can flood a tiny stream and
    //! watch health flip before anything wedges.
    use super::*;

    #[test]
    fn the_cap_reads_from_the_env_with_a_default_and_a_floor() {
        assert_eq!(events_cap_from(None), EVENTS_MAX_BYTES, "default 1 GiB");
        assert_eq!(events_cap_from(Some("524288")), 524_288);
        assert_eq!(
            events_cap_from(Some("not a number")),
            EVENTS_MAX_BYTES,
            "garbage is the default"
        );
        assert_eq!(
            events_cap_from(Some("10")),
            EVENTS_CAP_FLOOR,
            "below the floor is the floor"
        );
    }

    #[test]
    fn the_events_streams_take_the_cap() {
        assert_eq!(events_stream_config("h", false, 524_288).max_bytes, 524_288);
        assert_eq!(events_stream_config("h", true, 524_288).max_bytes, 524_288);
        assert_eq!(local_stream_config(524_288).max_bytes, 524_288);
        assert_eq!(events_mirror_config("hub", 524_288).max_bytes, 524_288);
    }
}

#[cfg(test)]
mod publish_budget_tests {
    //! The split budget follows the server's advertised max_payload. A
    //! managed standalone NATS runs the 1 MiB default; the E-05 split
    //! assumed 8 MB and let 2 MB batches fail whole (found on the demo
    //! node, 2026-09-24).
    use super::*;

    #[test]
    fn the_budget_is_nine_tenths_of_the_server_limit_capped_at_four_mib() {
        assert_eq!(
            publish_budget(1_048_576),
            943_718,
            "a default server: under 1 MiB"
        );
        assert_eq!(
            publish_budget(8_388_608),
            PUBLISH_MAX_BYTES,
            "an 8 MB server: the 4 MiB cap"
        );
        assert_eq!(
            publish_budget(0),
            PUBLISH_MAX_BYTES,
            "an unknown limit reads as the cap"
        );
    }
}

#[cfg(test)]
mod durable_consumer_tests {
    //! B-11: the pure pieces of the durable consumers — where a new one
    //! starts, what it is configured with, and when an existing one is
    //! recreated from its ack floor.
    use super::*;
    use jetstream::consumer::DeliverPolicy;

    #[test]
    fn a_new_durable_starts_at_the_actors_first_start() {
        let mark = Some(9);
        assert_eq!(
            first_start_policy(StartFrom::All, 0, mark),
            DeliverPolicy::All
        );
        assert_eq!(
            first_start_policy(StartFrom::New, 0, mark),
            DeliverPolicy::New
        );
        assert_eq!(
            first_start_policy(StartFrom::LastPerSubject, 0, mark),
            DeliverPolicy::LastPerSubject
        );
    }

    #[test]
    fn since_boot_starts_after_the_streams_mark_at_boot() {
        assert_eq!(
            first_start_policy(StartFrom::SinceBoot, 0, Some(9)),
            DeliverPolicy::ByStartSequence { start_sequence: 10 }
        );
        assert_eq!(
            first_start_policy(StartFrom::SinceBoot, 0, Some(0)),
            DeliverPolicy::ByStartSequence { start_sequence: 1 },
            "an empty stream at boot: everything published since"
        );
        assert_eq!(
            first_start_policy(StartFrom::SinceBoot, 0, None),
            DeliverPolicy::New,
            "no mark: from now"
        );
    }

    #[test]
    fn a_durable_the_server_lost_mid_run_starts_after_the_last_ack() {
        for first in [
            StartFrom::All,
            StartFrom::SinceBoot,
            StartFrom::New,
            StartFrom::LastPerSubject,
        ] {
            assert_eq!(
                first_start_policy(first, 41, Some(3)),
                DeliverPolicy::ByStartSequence { start_sequence: 42 }
            );
        }
    }

    #[test]
    fn the_config_is_a_bounded_explicit_ack_pull_durable() {
        let c = durable_config("os-persist-h", "events.>", DeliverPolicy::All);
        assert_eq!(c.durable_name.as_deref(), Some("os-persist-h"));
        assert_eq!(c.filter_subject, "events.>");
        assert_eq!(c.deliver_policy, DeliverPolicy::All);
        assert_eq!(c.ack_policy, jetstream::consumer::AckPolicy::Explicit);
        assert_eq!(c.max_ack_pending, CONSUMER_MAX_ACK_PENDING);
        assert_eq!(c.ack_wait, CONSUMER_ACK_WAIT);
        assert_eq!(c.inactive_threshold, CONSUMER_INACTIVE_THRESHOLD);
        assert!(
            CONSUMER_MAX_ACK_PENDING as usize >= PULL_BATCH,
            "one pull fits in the in-flight window"
        );
    }

    fn info(ack_pending: usize, filter: &str, max_ack_pending: i64) -> jetstream::consumer::Info {
        let mut v = serde_json::json!({
            "stream_name": "events",
            "name": "os-persist-h",
            "created": "2026-09-26T00:00:00Z",
            "config": {
                "durable_name": "os-persist-h",
                "deliver_policy": "all",
                "ack_policy": "explicit",
                "filter_subject": filter,
                "max_ack_pending": max_ack_pending,
            },
            "delivered": {"consumer_seq": 10, "stream_seq": 10},
            "ack_floor": {"consumer_seq": 8, "stream_seq": 8},
            "num_ack_pending": ack_pending,
            "num_redelivered": 0,
            "num_waiting": 0,
            "num_pending": 0,
        });
        v["cluster"] = serde_json::Value::Null;
        serde_json::from_value(v).expect("consumer info")
    }

    #[test]
    fn an_idle_durable_of_our_shape_is_resumed_as_it_is() {
        assert!(!needs_reset(
            &info(0, "events.>", CONSUMER_MAX_ACK_PENDING),
            "events.>"
        ));
    }

    #[test]
    fn a_durable_still_holding_a_dead_runs_messages_is_recreated() {
        assert!(needs_reset(
            &info(3, "events.>", CONSUMER_MAX_ACK_PENDING),
            "events.>"
        ));
    }

    #[test]
    fn a_durable_of_another_shape_is_recreated() {
        assert!(needs_reset(
            &info(0, "events.a.>", CONSUMER_MAX_ACK_PENDING),
            "events.>"
        ));
        assert!(needs_reset(&info(0, "events.>", 1000), "events.>"));
    }
}
