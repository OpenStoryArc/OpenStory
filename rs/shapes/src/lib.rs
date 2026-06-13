//! Deterministic shape-layer projections over the CloudEvent stream.
//!
//! A *shape* is a pure, per-event structural decomposition of what an agent
//! did — the inverse of the patterns crate, which is temporal (one pattern per
//! turn). Shapes are lexical/dimensional (one row per event, sometimes several):
//!
//!   - **bash-shape**  — tokenize a `Bash` command → program/subcommand/flags/args
//!   - **path-shape**  — decompose a file-touching tool's path → segments + naming tokens
//!   - **change-shape** — the delta of an Edit/Write/MultiEdit → lines/chars + excerpt
//!
//! Each layer is a [`ShapeExtractor`]: `(&CloudEvent, session_id) -> Vec<ShapeRow>`.
//! Pure, no I/O, agent-agnostic (reads via `AgentPayload::tool()/args()`). The
//! consumer keys rows on the *batch* session id (the subagent's own id), not
//! `event.data.session_id` (the parent) — so `session_id` is passed in, never
//! read from the event. Ported from the Python prototypes in `scripts/` (the
//! spec); see `docs/research/shape-layer-architecture.md`.

use open_story_core::cloud_event::CloudEvent;
use serde::{Deserialize, Serialize};

pub mod analysis;
pub mod backfill;
pub mod bash;
pub mod change;
pub mod path;

pub use analysis::ShapeCounts;
pub use backfill::{shapes_for_session, should_skip_shape_detection};
pub use bash::BashShape;
pub use change::ChangeShape;
pub use path::PathShape;

/// One deterministic shape projection of a single CloudEvent.
///
/// `id` is `{event_id}:{shape_type}:{ord}` — stable across replay/backfill
/// because `event.id` is the transcript message uuid and extraction is a pure,
/// ordered function. `ord` is 0 for single-row layers (bash, change) and the
/// match index for multi-row layers (path).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ShapeRow {
    pub id: String,
    pub session_id: String,
    pub shape_type: String,
    pub seq: u64,
    pub timestamp: String,
    pub event_id: String,
    /// Layer-specific decomposition (mirrors the prototype's DB columns).
    pub data: serde_json::Value,
}

impl ShapeRow {
    /// Construct a row, composing the deterministic dedup id.
    pub fn new(
        event: &CloudEvent,
        session_id: &str,
        shape_type: &str,
        ord: usize,
        data: serde_json::Value,
    ) -> Self {
        ShapeRow {
            id: format!("{}:{}:{}", event.id, shape_type, ord),
            session_id: session_id.to_string(),
            shape_type: shape_type.to_string(),
            seq: event.data.seq,
            timestamp: event.time.clone(),
            event_id: event.id.clone(),
            data,
        }
    }
}

/// A deterministic per-event projection. One implementor per shape layer.
pub trait ShapeExtractor: Send + Sync {
    /// Extract zero or more shape rows from one event. Pure; no I/O.
    /// `session_id` is the owning (batch) session — stamped onto every row.
    fn extract(&self, event: &CloudEvent, session_id: &str) -> Vec<ShapeRow>;

    /// The `shape_type` discriminator this extractor produces.
    fn shape_type(&self) -> &str;
}

/// The default MVP extractor set: the three stateless, NLP-free structural layers.
pub fn default_extractors() -> Vec<Box<dyn ShapeExtractor>> {
    vec![
        Box::new(BashShape),
        Box::new(PathShape),
        Box::new(ChangeShape),
    ]
}
