//! Patterns consumer — detects structural patterns in event streams.
//!
//! Actor contract:
//!   subscribes: events.>
//!   publishes:  patterns.{project}.{session}
//!   owns:       pattern_pipelines (one per session), detected_patterns cache
//!
//! This is the only consumer that is ALSO a producer. It reads CloudEvents,
//! feeds them through the eval-apply detector, and publishes detected
//! PatternEvents back to the bus for other consumers (broadcast, persist).
//!
//! The eval-apply detector is a pure fold:
//!   (accumulator, CloudEvent) → (new_accumulator, [StructuralTurn])
//! Each completed turn is fed to the SentenceDetector:
//!   StructuralTurn → [PatternEvent]

use std::collections::HashMap;

use open_story_core::cloud_event::CloudEvent;
use open_story_patterns::{PatternEvent, PatternPipeline, StructuralTurn};

/// State owned by the patterns consumer actor.
pub struct PatternsConsumer {
    /// One pipeline per session — maintains eval-apply accumulator state.
    pipelines: HashMap<String, PatternPipeline>,
    /// Detected patterns cache (for initial_state on WebSocket connect).
    detected: HashMap<String, Vec<PatternEvent>>,
}

/// Result of processing one batch of events through the pattern pipeline.
pub struct PatternsResult {
    /// Detected patterns from this batch.
    pub patterns: Vec<PatternEvent>,
    /// Completed structural turns from this batch.
    pub turns: Vec<StructuralTurn>,
}

impl PatternsConsumer {
    pub fn new() -> Self {
        Self {
            pipelines: HashMap::new(),
            detected: HashMap::new(),
        }
    }

    /// Process a batch of CloudEvents through the pattern detection pipeline.
    ///
    /// Returns detected patterns and completed turns. The caller is responsible
    /// for publishing PatternEvents to the bus (patterns.{project}.{session}).
    pub fn process_batch(
        &mut self,
        session_id: &str,
        events: &[CloudEvent],
    ) -> PatternsResult {
        let pipeline = self.pipelines
            .entry(session_id.to_string())
            .or_default();

        let mut all_patterns = Vec::new();
        let mut all_turns = Vec::new();

        for ce in events {
            // Skip ephemeral events (progress, hooks)
            let subtype = ce.subtype.as_deref().unwrap_or("");
            if is_ephemeral(subtype) {
                continue;
            }

            // Feed to eval-apply → sentence pipeline
            let (patterns, turns) = pipeline.feed_event(ce);
            all_patterns.extend(patterns);
            all_turns.extend(turns);
        }

        // Cache detected patterns for initial_state
        if !all_patterns.is_empty() {
            self.detected
                .entry(session_id.to_string())
                .or_default()
                .extend(all_patterns.clone());
        }

        PatternsResult {
            patterns: all_patterns,
            turns: all_turns,
        }
    }

    /// Get all detected patterns for a session (for initial_state).
    pub fn session_patterns(&self, session_id: &str) -> &[PatternEvent] {
        self.detected
            .get(session_id)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Get all detected patterns across all sessions.
    pub fn all_patterns(&self) -> impl Iterator<Item = &PatternEvent> {
        self.detected.values().flat_map(|v| v.iter())
    }

    /// Flush the pipeline for a session (produces any pending turns).
    pub fn flush(&mut self, session_id: &str) -> PatternsResult {
        if let Some(pipeline) = self.pipelines.get_mut(session_id) {
            let (patterns, turns) = pipeline.flush();
            if !patterns.is_empty() {
                self.detected
                    .entry(session_id.to_string())
                    .or_default()
                    .extend(patterns.clone());
            }
            PatternsResult { patterns, turns }
        } else {
            PatternsResult { patterns: Vec::new(), turns: Vec::new() }
        }
    }
}

/// Check if a subtype is ephemeral (not fed to pattern detection).
///
/// Ephemeral events are observation metadata that don't represent agent
/// reasoning or action — the eval_apply detector should never see them
/// because (a) they don't contribute to the structural turn shape and
/// (b) some of them lack a stable `event.time`, which would corrupt
/// `start_ts` if they were picked as the first event of a turn.
///
/// `file.snapshot` is the load-bearing one: file snapshots arrive as
/// metadata before/around tool calls and have no timestamp from the
/// source JSONL. Before this filter was added, they were stamping
/// turn `start_ts` with wall-clock-at-detection-time, causing every
/// boot replay to emit a fresh sentence row for the same logical turn
/// (the H2 case in scripts/inspect_sentence_dedup.py — 1.50× ratio).
fn is_ephemeral(subtype: &str) -> bool {
    subtype.starts_with("progress.")
        || subtype == "system.hook"
        || subtype == "queue.enqueue"
        || subtype == "queue.dequeue"
        || subtype == "queue.remove"
        || subtype == "queue.popAll"
        || subtype == "file.snapshot"
}

#[cfg(test)]
mod tests {
    use super::*;
    use open_story_core::event_data::{AgentPayload, ClaudeCodePayload, EventData};
    use serde_json::json;

    fn make_event(session_id: &str, subtype: &str) -> CloudEvent {
        let mut payload = ClaudeCodePayload::new();
        payload.text = Some("test".to_string());
        let data = EventData::with_payload(
            json!({}), 0, session_id.to_string(),
            AgentPayload::ClaudeCode(payload),
        );
        CloudEvent::new(
            format!("arc://test/{session_id}"),
            "io.arc.event".into(),
            data,
            Some(subtype.into()),
            None, None, None, None,
            Some("claude-code".into()),
        )
    }

    #[test]
    fn new_consumer_has_empty_state() {
        let consumer = PatternsConsumer::new();
        assert_eq!(consumer.all_patterns().count(), 0);
    }

    #[test]
    fn skips_ephemeral_events() {
        let mut consumer = PatternsConsumer::new();
        let events = vec![
            make_event("sess-1", "progress.bash"),
            make_event("sess-1", "system.hook"),
            make_event("sess-1", "queue.enqueue"),
        ];
        let result = consumer.process_batch("sess-1", &events);
        assert_eq!(result.patterns.len(), 0);
        assert_eq!(result.turns.len(), 0);
    }

    #[test]
    fn processes_durable_events() {
        let mut consumer = PatternsConsumer::new();
        // A single prompt won't produce a complete turn, but it should be processed
        let events = vec![
            make_event("sess-1", "message.user.prompt"),
        ];
        let result = consumer.process_batch("sess-1", &events);
        // No turn completed yet (need assistant response + turn_end)
        assert_eq!(result.turns.len(), 0);
        // But the pipeline accepted the event (no panic)
    }

    #[test]
    fn maintains_separate_pipelines_per_session() {
        let mut consumer = PatternsConsumer::new();

        consumer.process_batch("sess-1", &[make_event("sess-1", "message.user.prompt")]);
        consumer.process_batch("sess-2", &[make_event("sess-2", "message.user.prompt")]);

        // Each session has its own pipeline
        assert!(consumer.pipelines.contains_key("sess-1"));
        assert!(consumer.pipelines.contains_key("sess-2"));
    }
}
