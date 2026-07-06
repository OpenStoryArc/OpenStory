//! Subscription mechanics for the MCP server.
//!
//! This module defines what MCP's transport layer needs from a bus:
//! a way to open a subscription for a session id and read events
//! arriving in sequence. The `Subscribe` trait names that contract.
//!
//! `NatsBus` (production) impls `Subscribe` by wrapping
//! `open_story_bus::NatsBus`. `LoopbackSubscriber` (tests, in
//! `tests/common/`) impls it in memory.

use anyhow::Result;
use async_trait::async_trait;
use open_story_bus::IngestBatch;
use serde_json::Value;
use tokio::sync::mpsc;

/// One delivered event on a subscription stream.
#[derive(Debug, Clone)]
pub struct StreamEvent {
    pub seq: u64,
    pub session_id: String,
    pub data: Value,
}

/// Handle the subscriber holds. Drop it to cancel.
pub struct Subscription {
    pub stream_id: uuid::Uuid,
    pub session_id: String,
    rx: mpsc::Receiver<StreamEvent>,
    _cancel: CancelGuard,
}

/// A drop-guard that runs a cancel callback when dropped.
pub struct CancelGuard {
    on_drop: Option<Box<dyn FnOnce() + Send>>,
}

impl CancelGuard {
    pub fn from_fn<F: FnOnce() + Send + 'static>(f: F) -> Self {
        Self {
            on_drop: Some(Box::new(f)),
        }
    }
}

impl Drop for CancelGuard {
    fn drop(&mut self) {
        if let Some(f) = self.on_drop.take() {
            f();
        }
    }
}

impl Subscription {
    pub fn from_parts(
        stream_id: uuid::Uuid,
        session_id: String,
        rx: mpsc::Receiver<StreamEvent>,
        cancel: CancelGuard,
    ) -> Self {
        Self {
            stream_id,
            session_id,
            rx,
            _cancel: cancel,
        }
    }

    pub async fn recv(&mut self) -> Option<StreamEvent> {
        self.rx.recv().await
    }

    pub fn try_recv(&mut self) -> Option<StreamEvent> {
        self.rx.try_recv().ok()
    }
}

/// What MCP needs from a bus: open a subscription for a session id.
///
/// Production impl: `NatsBus` (wraps `open_story_bus::NatsBus`).
/// Test impl: `LoopbackSubscriber` in `tests/common/mod.rs`.
#[async_trait]
pub trait Subscribe: Clone + Send + Sync + 'static {
    async fn subscribe(&self, session_id: &str) -> Result<Subscription>;

    /// Subscribe to the AUTHORED `ui.*` stream — live-follow of the user's
    /// interactions (the READ half of the agent-in-UI seam). Default:
    /// unsupported, so test subscribers don't have to implement it;
    /// production `NatsBus` overrides with a real `ui` JetStream subscription.
    async fn subscribe_ui(&self) -> Result<Subscription> {
        anyhow::bail!("this subscriber does not support ui.* streaming")
    }
}

/// Pure transform: read `IngestBatch`es from a source channel, wrap each
/// in a `StreamEvent` with a monotonically increasing seq, and forward
/// to a sink channel. Terminates when either channel closes.
///
/// This is the one bit of subscription mechanics that belongs to MCP —
/// the bus delivers `IngestBatch`es, MCP attaches a per-subscription
/// seq counter and adapts the wire shape.
pub async fn pump_subscription(
    mut source: mpsc::Receiver<IngestBatch>,
    sink: mpsc::Sender<StreamEvent>,
    session_id: String,
) {
    let mut seq: u64 = 1;
    while let Some(batch) = source.recv().await {
        let data = serde_json::to_value(&batch).unwrap_or(Value::Null);
        let event = StreamEvent {
            seq,
            session_id: session_id.clone(),
            data,
        };
        seq += 1;
        if sink.send(event).await.is_err() {
            break;
        }
    }
}

