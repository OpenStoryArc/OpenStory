//! Streaming pattern detection pipeline.
//!
//! Each detector is a pure fold: (state, event) -> (new_state, [outputs])
//! Detectors run independently and emit PatternEvents when they recognize
//! higher-order behavioral patterns in the event stream.
//!
//! Ported from `scripts/streaming_patterns.py` (28 BDD tests).

use serde::{Deserialize, Serialize};

pub mod turn_phase;
pub mod git_flow;
pub mod test_cycle;
pub mod error_recovery;
pub mod agent_delegation;

// Re-export detectors for convenience
pub use turn_phase::TurnPhaseDetector;
pub use git_flow::GitFlowDetector;
pub use test_cycle::TestCycleDetector;
pub use error_recovery::ErrorRecoveryDetector;
pub use agent_delegation::AgentDelegationDetector;

use open_story_views::view_record::ViewRecord;

// ═══════════════════════════════════════════════════════════════════
// PatternEvent — the output of all detectors
// ═══════════════════════════════════════════════════════════════════

/// Higher-order event emitted by pattern detectors.
/// Represents a recognized behavioral pattern spanning multiple events.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatternEvent {
    /// Pattern type identifier (e.g., "test.cycle", "git.workflow").
    pub pattern_type: String,
    /// Session this pattern belongs to.
    pub session_id: String,
    /// Event IDs that compose this pattern.
    pub event_ids: Vec<String>,
    /// Timestamp of the first event in the pattern.
    pub started_at: String,
    /// Timestamp of the last event in the pattern.
    pub ended_at: String,
    /// Human-readable one-line summary.
    pub summary: String,
    /// Detector-specific metadata.
    #[serde(default)]
    pub metadata: serde_json::Value,
}

// ═══════════════════════════════════════════════════════════════════
// FeedContext — what detectors receive alongside each ViewRecord
// ═══════════════════════════════════════════════════════════════════

/// Context passed to detectors on each feed call.
/// Provides tree metadata that ViewRecord doesn't carry.
pub struct FeedContext<'a> {
    /// The ViewRecord being processed.
    pub record: &'a ViewRecord,
    /// Depth in the session tree (0 = root).
    pub depth: u16,
    /// Parent event UUID (None = root event).
    pub parent_uuid: Option<&'a str>,
}

// ═══════════════════════════════════════════════════════════════════
// Detector trait
// ═══════════════════════════════════════════════════════════════════

/// A streaming pattern detector.
///
/// Each detector maintains its own state machine and emits PatternEvents
/// when it recognizes a pattern in the event stream. Detectors are pure folds:
/// they process events one at a time and produce outputs without side effects.
pub trait Detector: Send + Sync {
    /// Process one event. Returns any patterns detected.
    fn feed(&mut self, ctx: &FeedContext) -> Vec<PatternEvent>;

    /// Flush at end of stream. Returns any incomplete patterns.
    fn flush(&mut self) -> Vec<PatternEvent>;

    /// Detector name (used for logging/debugging).
    fn name(&self) -> &str;
}

// ═══════════════════════════════════════════════════════════════════
// PatternPipeline — wires detectors together
// ═══════════════════════════════════════════════════════════════════

/// Streaming pipeline: feed events to all detectors, collect pattern outputs.
pub struct PatternPipeline {
    detectors: Vec<Box<dyn Detector>>,
}

impl PatternPipeline {
    /// Create a pipeline with the default set of 5 detectors.
    pub fn new() -> Self {
        PatternPipeline {
            detectors: vec![
                Box::new(TestCycleDetector::new()),
                Box::new(GitFlowDetector::new()),
                Box::new(ErrorRecoveryDetector::new()),
                Box::new(AgentDelegationDetector::new()),
                Box::new(TurnPhaseDetector::new()),
            ],
        }
    }

    /// Create a pipeline with a custom set of detectors.
    pub fn with_detectors(detectors: Vec<Box<dyn Detector>>) -> Self {
        PatternPipeline { detectors }
    }

    /// Feed one event (with tree context) to all detectors.
    pub fn feed(&mut self, ctx: &FeedContext) -> Vec<PatternEvent> {
        let mut emitted = Vec::new();
        for d in &mut self.detectors {
            emitted.extend(d.feed(ctx));
        }
        emitted
    }

    /// Flush all detectors (end of stream).
    pub fn flush(&mut self) -> Vec<PatternEvent> {
        let mut emitted = Vec::new();
        for d in &mut self.detectors {
            emitted.extend(d.flush());
        }
        emitted
    }
}

impl Default for PatternPipeline {
    fn default() -> Self {
        Self::new()
    }
}
