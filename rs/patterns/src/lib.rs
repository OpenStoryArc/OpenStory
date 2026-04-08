//! Streaming pattern detection pipeline.
//!
//! Eval-apply is the source of truth. It consumes CloudEvents and produces
//! `StructuralTurn`s; the sentence detector consumes turns and produces
//! `turn.sentence` patterns. Anything else (turn phase classification,
//! agent delegation, etc.) is a runtime projection of `StructuralTurn`,
//! computed at the rendering boundary, not persisted as its own pattern type.
//!
//! Historical note: a `Detector` trait + 5 record-based detectors
//! (TurnPhaseDetector, AgentDelegationDetector, ErrorRecoveryDetector,
//! TestCycleDetector, GitFlowDetector) lived alongside this pipeline through
//! the `feat/mongodb-sink` work. They were cut in `chore/cut-legacy-detectors`
//! after the data showed they produced ~127 turn.phase patterns and 0–13 of
//! everything else across nine sessions while the new pipeline produced
//! ~3000 eval_apply patterns. Two were dead-by-data, three were retired in
//! favor of derive-on-the-fly. See git log + docs/BACKLOG.md for the
//! "Subagent Task Labels — Restore After Cut" follow-up.

use serde::{Deserialize, Serialize};

pub mod eval_apply;
pub mod sentence;

// Re-export the only detectors that survived the cut.
pub use eval_apply::{EvalApplyDetector, StructuralTurn};
pub use sentence::SentenceDetector;

use open_story_core::cloud_event::CloudEvent;

// ═══════════════════════════════════════════════════════════════════
// PatternEvent — the output of all detectors
// ═══════════════════════════════════════════════════════════════════

/// Higher-order event emitted by pattern detectors.
/// Represents a recognized behavioral pattern spanning multiple events.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatternEvent {
    /// Pattern type identifier (e.g., "eval_apply.eval", "turn.sentence").
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
// TurnDetector — consumes StructuralTurns produced by eval_apply
// ═══════════════════════════════════════════════════════════════════

/// A detector that consumes `StructuralTurn`s rather than raw CloudEvents.
/// Runs in phase 2 of the pipeline, after eval-apply produces turns.
pub trait TurnDetector: Send + Sync {
    /// Process one completed turn. Returns any patterns detected.
    fn feed_turn(&mut self, turn: &StructuralTurn) -> Vec<PatternEvent>;

    /// Flush at end of stream.
    fn flush(&mut self) -> Vec<PatternEvent>;

    /// Detector name (used for logging/debugging).
    fn name(&self) -> &str;
}

// ═══════════════════════════════════════════════════════════════════
// PatternPipeline — wires the eval-apply + turn-detector chain together
// ═══════════════════════════════════════════════════════════════════

/// Two-phase streaming pipeline:
///   Phase 1: CloudEvent → EvalApplyDetector → StructuralTurns + PatternEvents
///   Phase 2: StructuralTurn → TurnDetectors → PatternEvents
pub struct PatternPipeline {
    /// Phase 1: CloudEvent consumer. Produces StructuralTurns.
    eval_apply: EvalApplyDetector,
    /// Phase 2: StructuralTurn consumers (SentenceDetector, etc.)
    turn_detectors: Vec<Box<dyn TurnDetector>>,
}

impl PatternPipeline {
    /// Create a pipeline with the default detector set.
    pub fn new() -> Self {
        PatternPipeline {
            eval_apply: EvalApplyDetector::new(),
            turn_detectors: vec![Box::new(SentenceDetector::new())],
        }
    }

    /// Create a pipeline with custom turn detectors. Used in tests.
    pub fn with_turn_detectors(turn_detectors: Vec<Box<dyn TurnDetector>>) -> Self {
        PatternPipeline {
            eval_apply: EvalApplyDetector::new(),
            turn_detectors,
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

    /// Flush both phases. The eval-apply detector emits patterns inline as
    /// CloudEvents arrive (no buffering), so flush only collects any
    /// in-flight `StructuralTurn`s and routes them through the turn
    /// detectors.
    pub fn flush(&mut self) -> (Vec<PatternEvent>, Vec<StructuralTurn>) {
        let mut emitted = Vec::new();

        let turns = self.eval_apply.flush_turns();
        for turn in &turns {
            for td in &mut self.turn_detectors {
                emitted.extend(td.feed_turn(turn));
            }
        }
        for td in &mut self.turn_detectors {
            emitted.extend(td.flush());
        }

        (emitted, turns)
    }
}

impl Default for PatternPipeline {
    fn default() -> Self {
        Self::new()
    }
}
