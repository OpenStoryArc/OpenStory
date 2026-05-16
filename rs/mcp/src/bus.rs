//! Internal Bus abstraction for the MCP server.
//!
//! This is intentionally narrower than `open-story-bus`'s trait — we
//! only need subscribe/publish semantics. A real implementation will
//! wrap async-nats; for tests and early TDD slices, `InMemoryBus`
//! provides the same surface without external infrastructure.

use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};

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

struct CancelGuard {
    on_drop: Option<Box<dyn FnOnce() + Send>>,
}

impl Drop for CancelGuard {
    fn drop(&mut self) {
        if let Some(f) = self.on_drop.take() {
            f();
        }
    }
}

impl Subscription {
    pub async fn recv(&mut self) -> Option<StreamEvent> {
        self.rx.recv().await
    }

    /// Try to receive without blocking. Returns `None` if no event is ready.
    pub fn try_recv(&mut self) -> Option<StreamEvent> {
        self.rx.try_recv().ok()
    }
}

/// In-memory bus — events published to a session id are delivered to
/// every active subscription for that session id. Concurrent-safe
/// (multiple publishers, multiple subscribers).
#[derive(Clone, Default)]
pub struct InMemoryBus {
    inner: Arc<Mutex<BusInner>>,
}

#[derive(Default)]
struct BusInner {
    /// session_id → list of (subscription_id, sender, current_seq)
    routes: HashMap<String, Vec<Route>>,
}

struct Route {
    sub_id: uuid::Uuid,
    tx: mpsc::Sender<StreamEvent>,
    next_seq: u64,
}

impl InMemoryBus {
    pub fn new() -> Self {
        Self::default()
    }

    /// Open a new subscription to a session id. Returned `Subscription`
    /// receives `StreamEvent`s until dropped (= cancelled).
    pub async fn subscribe(&self, session_id: impl Into<String>) -> Subscription {
        let session_id = session_id.into();
        let sub_id = uuid::Uuid::new_v4();
        let (tx, rx) = mpsc::channel::<StreamEvent>(256);

        {
            let mut inner = self.inner.lock().await;
            inner
                .routes
                .entry(session_id.clone())
                .or_default()
                .push(Route { sub_id, tx, next_seq: 1 });
        }

        let bus = self.clone();
        let removal_session = session_id.clone();
        let cancel = CancelGuard {
            on_drop: Some(Box::new(move || {
                let bus = bus.clone();
                let sid = removal_session.clone();
                tokio::spawn(async move {
                    let mut inner = bus.inner.lock().await;
                    if let Some(routes) = inner.routes.get_mut(&sid) {
                        routes.retain(|r| r.sub_id != sub_id);
                        if routes.is_empty() {
                            inner.routes.remove(&sid);
                        }
                    }
                });
            })),
        };

        Subscription {
            stream_id: sub_id,
            session_id,
            rx,
            _cancel: cancel,
        }
    }

    /// Publish an event for `session_id`. Each active subscription
    /// for that session receives it (in publish order, with a
    /// monotonically increasing per-subscription seq).
    pub async fn publish(&self, session_id: &str, data: Value) {
        let mut inner = self.inner.lock().await;
        let Some(routes) = inner.routes.get_mut(session_id) else {
            return;
        };
        for route in routes.iter_mut() {
            let event = StreamEvent {
                seq: route.next_seq,
                session_id: session_id.to_string(),
                data: data.clone(),
            };
            route.next_seq += 1;
            // Best-effort send; if the channel is full or closed, the
            // subscriber is responsible for its own bounded behavior.
            let _ = route.tx.try_send(event);
        }
    }

    /// Number of active subscriptions for a given session id.
    /// Useful for assertions in cancellation tests.
    pub async fn subscription_count(&self, session_id: &str) -> usize {
        let inner = self.inner.lock().await;
        inner.routes.get(session_id).map(|r| r.len()).unwrap_or(0)
    }
}
