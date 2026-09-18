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

    /// Subscribe to closed story arcs and exchanges (memory hands, group C).
    /// `session_id: None` follows every session; `from_seq` resumes from a
    /// stored batch sequence (the lazy-list cursor). Default: unsupported.
    async fn subscribe_arcs(
        &self,
        _session_id: Option<&str>,
        _from_seq: Option<u64>,
    ) -> Result<Subscription> {
        anyhow::bail!("this subscriber does not support story arc streaming")
    }
}

/// Pure: what a host receives for one story pattern, or `None` for any
/// other pattern type. `needs` says what judgment the skeleton is waiting
/// for: an untitled arc needs `enrich`, an arc with ambiguous seams needs
/// `adjudicate`, a closed exchange feeds the streaming `read`.
pub fn arc_closed(session_id: &str, pattern: &Value) -> Option<Value> {
    let kind = match pattern.get("pattern_type")?.as_str()? {
        "story.arc" => "arc",
        "story.exchange" => "exchange",
        _ => return None,
    };
    let meta = pattern.get("metadata")?;
    let handle = meta.get("handle")?.as_str()?;
    let needs: Vec<&str> = if kind == "arc" {
        let mut n = Vec::new();
        if meta.get("title").is_none() {
            n.push("enrich");
        }
        let ambiguous = meta
            .get("ambiguous_seams")
            .and_then(|v| v.as_array())
            .map(|a| !a.is_empty())
            .unwrap_or(false);
        if ambiguous {
            n.push("adjudicate");
        }
        n
    } else {
        vec!["read"]
    };
    // E-02: the prompt to run for each need — a ready-made prompts/get
    // call, so a host with nothing but the wire can act on the notification.
    let args = |extra: Option<(&str, Value)>| {
        let mut a = serde_json::json!({ "handle": handle, "session_id": session_id });
        if let Some((k, v)) = extra {
            a[k] = v;
        }
        a
    };
    let mut prompts: Vec<Value> = Vec::new();
    for need in &needs {
        match *need {
            "enrich" => {
                prompts.push(serde_json::json!({ "name": "narrate_arc", "arguments": args(None) }));
                prompts.push(serde_json::json!({ "name": "segment_arc", "arguments": args(None) }));
            }
            "adjudicate" => {
                let seams = meta
                    .get("ambiguous_seams")
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default();
                for seam in seams {
                    prompts.push(serde_json::json!({
                        "name": "adjudicate_seam",
                        "arguments": args(Some(("seam", seam)))
                    }));
                }
            }
            "read" => {
                prompts
                    .push(serde_json::json!({ "name": "read_exchange", "arguments": args(None) }));
            }
            _ => {}
        }
    }
    Some(serde_json::json!({
        "kind": kind,
        "handle": handle,
        "session_id": session_id,
        "pattern_type": pattern.get("pattern_type"),
        "skeleton": pattern,
        "needs": needs,
        "prompts": prompts,
    }))
}

/// Pure transform for pattern batches: each batch is a JSON array of
/// PatternEvents tagged with its stored sequence; every story pattern in
/// it becomes one `StreamEvent` carrying `arc_closed` data plus
/// `batch_seq` (the resume cursor) and `index` within the batch. The
/// per-subscription `seq` counts delivered events, like `pump_subscription`.
pub async fn pump_patterns(
    mut source: mpsc::Receiver<(u64, Value)>,
    sink: mpsc::Sender<StreamEvent>,
    session_filter: Option<String>,
) {
    let mut seq: u64 = 1;
    while let Some((batch_seq, batch)) = source.recv().await {
        let Some(items) = batch.as_array() else {
            continue;
        };
        for (index, p) in items.iter().enumerate() {
            let sid = p.get("session_id").and_then(|v| v.as_str()).unwrap_or("");
            if session_filter.as_deref().map(|f| f != sid).unwrap_or(false) {
                continue;
            }
            let Some(mut data) = arc_closed(sid, p) else {
                continue;
            };
            data["batch_seq"] = serde_json::json!(batch_seq);
            data["index"] = serde_json::json!(index);
            let event = StreamEvent {
                seq,
                session_id: sid.to_string(),
                data,
            };
            seq += 1;
            if sink.send(event).await.is_err() {
                return;
            }
        }
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
