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
pub mod eval_apply;
pub mod sentence;

// Re-export detectors for convenience
pub use turn_phase::TurnPhaseDetector;
pub use git_flow::GitFlowDetector;
pub use test_cycle::TestCycleDetector;
pub use error_recovery::ErrorRecoveryDetector;
pub use agent_delegation::AgentDelegationDetector;
pub use eval_apply::EvalApplyDetector;
pub use sentence::SentenceDetector;

use open_story_core::cloud_event::CloudEvent;
use open_story_views::view_record::ViewRecord;

pub use eval_apply::StructuralTurn;

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
// TurnDetector — consumes StructuralTurns (output of eval-apply)
// ═══════════════════════════════════════════════════════════════════

/// A detector that consumes StructuralTurns rather than raw records.
/// Runs in phase 2 of the pipeline, after eval-apply produces turns.
pub trait TurnDetector: Send + Sync {
    /// Process one completed turn. Returns any patterns detected.
    fn feed_turn(&mut self, turn: &StructuralTurn) -> Vec<PatternEvent>;

    /// Flush at end of stream.
    fn flush(&mut self) -> Vec<PatternEvent>;

    /// Detector name.
    fn name(&self) -> &str;
}

// ═══════════════════════════════════════════════════════════════════
// PatternPipeline — wires detectors together
// ═══════════════════════════════════════════════════════════════════

/// Two-phase streaming pipeline:
///   Phase 1: CloudEvent → EvalApplyDetector → StructuralTurns + PatternEvents
///   Phase 2: StructuralTurn → TurnDetectors → PatternEvents
///   Legacy:  ViewRecord → record Detectors → PatternEvents
pub struct PatternPipeline {
    /// Phase 1: CloudEvent consumer. Produces StructuralTurns.
    eval_apply: EvalApplyDetector,
    /// Phase 2: StructuralTurn consumers (SentenceDetector, etc.)
    turn_detectors: Vec<Box<dyn TurnDetector>>,
    /// Legacy: ViewRecord consumers (TestCycle, GitFlow, etc.)
    record_detectors: Vec<Box<dyn Detector>>,
}

impl PatternPipeline {
    /// Create a pipeline with the default detector set.
    pub fn new() -> Self {
        PatternPipeline {
            eval_apply: EvalApplyDetector::new(),
            turn_detectors: vec![
                Box::new(SentenceDetector::new()),
            ],
            record_detectors: vec![
                Box::new(TestCycleDetector::new()),
                Box::new(GitFlowDetector::new()),
                Box::new(ErrorRecoveryDetector::new()),
                Box::new(AgentDelegationDetector::new()),
                Box::new(TurnPhaseDetector::new()),
            ],
        }
    }

    /// Create a pipeline with custom detectors.
    pub fn with_detectors(
        record_detectors: Vec<Box<dyn Detector>>,
        turn_detectors: Vec<Box<dyn TurnDetector>>,
    ) -> Self {
        PatternPipeline {
            eval_apply: EvalApplyDetector::new(),
            turn_detectors,
            record_detectors,
        }
    }

    /// Phase 1: Feed a CloudEvent to eval-apply, collect turns, feed to turn detectors.
    /// Returns (PatternEvents, completed StructuralTurns).
    pub fn feed_event(&mut self, event: &CloudEvent) -> (Vec<PatternEvent>, Vec<StructuralTurn>) {
        let mut emitted = Vec::new();

        // Phase 1: eval-apply consumes the CloudEvent
        emitted.extend(self.eval_apply.feed_cloud_event(event));

        // Collect completed turns and feed to phase 2 detectors
        let turns = self.eval_apply.take_completed_turns();
        for turn in &turns {
            for td in &mut self.turn_detectors {
                emitted.extend(td.feed_turn(turn));
            }
        }

        (emitted, turns)
    }

    /// Legacy: Feed one ViewRecord to record-based detectors.
    pub fn feed(&mut self, ctx: &FeedContext) -> Vec<PatternEvent> {
        let mut emitted = Vec::new();
        // Feed to eval-apply via legacy path too (for backward compat)
        emitted.extend(self.eval_apply.feed(ctx));
        for d in &mut self.record_detectors {
            emitted.extend(d.feed(ctx));
        }
        emitted
    }

    /// Flush all detectors across both phases.
    pub fn flush(&mut self) -> Vec<PatternEvent> {
        let mut emitted = Vec::new();

        // Flush eval-apply (PatternEvents)
        emitted.extend(self.eval_apply.flush());

        // Flush incomplete turns through turn detectors
        let turns = self.eval_apply.flush_turns();
        for turn in &turns {
            for td in &mut self.turn_detectors {
                emitted.extend(td.feed_turn(turn));
            }
        }
        for td in &mut self.turn_detectors {
            emitted.extend(td.flush());
        }

        // Flush legacy record detectors
        for d in &mut self.record_detectors {
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
