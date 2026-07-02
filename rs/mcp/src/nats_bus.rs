//! `NatsBus` — thin wrapper over `open_story_bus::NatsBus`.
//!
//! MCP doesn't need a parallel NATS implementation; it needs to consume
//! the same JetStream the rest of OpenStory publishes to. This wrapper
//! adapts the workspace's bus into MCP's `Subscribe` trait — that's it.

use crate::subscription::{pump_subscription, CancelGuard, Subscribe, Subscription};
use anyhow::Result;
use async_trait::async_trait;
use open_story_bus::nats_bus::NatsBus as InnerNatsBus;
use open_story_bus::Bus;
use std::sync::Arc;
use tokio::sync::mpsc;

#[derive(Clone)]
pub struct NatsBus {
    inner: Arc<InnerNatsBus>,
}

impl NatsBus {
    /// Connect to NATS and ensure the JetStream `events` stream exists.
    ///
    /// `ensure_streams()` is idempotent — safe to call even if the
    /// OpenStory server has already declared the streams. Calling it
    /// from MCP defensively means the binary works whether or not the
    /// server boots first.
    pub async fn connect(url: &str) -> Result<Self> {
        let inner = InnerNatsBus::connect(url).await?;
        inner.ensure_streams().await?;
        Ok(Self {
            inner: Arc::new(inner),
        })
    }
}

#[async_trait]
impl Subscribe for NatsBus {
    async fn subscribe(&self, session_id: &str) -> Result<Subscription> {
        // Subject convention: events.{host}.{project}.{session}.{main|agent.id}
        // (see open_story_core::paths::nats_subject_from_path).
        //
        // Each `*` matches a single token: first `*` = host, second = project.
        // The `>` matches trailing tokens = `main` or `agent.{id}`.
        //
        // If the subject scheme grows further (e.g., person_id / principal_id),
        // pull subject construction into open_story_bus and call a typed helper
        // so subscribe and publish can't drift again.
        let pattern = format!("events.*.*.{}.>", session_id);
        let bus_sub = self.inner.subscribe(&pattern).await?;

        let sub_id = uuid::Uuid::new_v4();
        let (tx, rx) = mpsc::channel(256);
        let pump = tokio::spawn(pump_subscription(
            bus_sub.receiver,
            tx,
            session_id.to_string(),
        ));

        let cancel = CancelGuard::from_fn(move || {
            pump.abort();
        });

        Ok(Subscription::from_parts(
            sub_id,
            session_id.to_string(),
            rx,
            cancel,
        ))
    }

    /// Live-follow the AUTHORED `ui.*` stream (interactions/control/annotations)
    /// — the READ half of the agent-in-UI seam. Now that authored events are
    /// proper CloudEvents published as IngestBatches, this consumes the `ui`
    /// JetStream stream filtered `ui.>` through the SAME typed pump as observed
    /// events — no raw-bytes special-case. Strictly the authored namespace;
    /// never `events.*`.
    async fn subscribe_ui(&self) -> Result<Subscription> {
        let bus_rx = self.inner.subscribe_typed("ui", "ui.>").await?;

        let sub_id = uuid::Uuid::new_v4();
        let session_id = "openstory-ui".to_string();
        let (tx, rx) = mpsc::channel(256);
        let pump = tokio::spawn(pump_subscription(bus_rx, tx, session_id.clone()));

        let cancel = CancelGuard::from_fn(move || {
            pump.abort();
        });

        Ok(Subscription::from_parts(sub_id, session_id, rx, cancel))
    }
}
