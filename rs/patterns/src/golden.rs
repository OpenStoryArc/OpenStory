//! GoldenSpec — the typed, immutable description a story golden derives from.
//!
//! Memory hands (docs: openstory-research/memory/hands/REQUIREMENTS.md, group X).
//! A golden is never copied from a real session. It is *generated* from a
//! `GoldenSpec`, so the expected exchanges, arcs, and handles are known by
//! construction. Real sessions inform shape parameters by hand (how many
//! exchanges, how many injected, gap distribution) and nothing else.
//!
//! Everything here is plain data: serde for the committed `spec.json`,
//! schemars for `schemas/golden_spec.schema.json`. No behaviour lives in
//! this module; `generate` and `expect` are pure functions over it.

use serde::{Deserialize, Serialize};

/// Whether a user-role message was typed by the human or injected by the
/// harness (a skill body, a tool-loaded notice, a task notification).
/// Injected messages arrive while the assistant's turn is still open and
/// must not open an exchange (requirement A-03).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExchangeKind {
    Human,
    Injected,
}

/// Length class of the prompt text the generator writes. Content is
/// synthetic either way; the class only shapes byte counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PromptClass {
    Short,
    Long,
}

/// One assistant turn inside an exchange.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct TurnSpec {
    /// Sentence verb the turn should fold to (`read`, `edited`, `committed`, ...).
    pub verb: String,
    /// Objects the turn acts on; become entities in the expected output.
    pub objects: Vec<String>,
    /// Tool name and call count, in emission order.
    pub tools: Vec<(String, u32)>,
    /// Whether the turn is rich enough to produce a `turn.sentence`.
    /// `false` yields a thin turn (requirement A-04).
    pub has_sentence: bool,
}

/// One user-role message and the turns that answer it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ExchangeSpec {
    pub kind: ExchangeKind,
    pub prompt_class: PromptClass,
    /// Event-time gap between the previous exchange's last event and this
    /// exchange's first event. Compared against `gap_threshold_secs` to
    /// place arc boundaries (requirement A-06).
    pub gap_before_secs: u64,
    pub turns: Vec<TurnSpec>,
}

/// The whole spec for one synthetic session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct GoldenSpec {
    pub session_id: String,
    /// RFC 3339 timestamp of the first event.
    pub started_at: String,
    /// Arc gap threshold in seconds (default in production: 1800).
    pub gap_threshold_secs: u64,
    pub exchanges: Vec<ExchangeSpec>,
}
