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

impl Default for PatternsConsumer {
    fn default() -> Self {
        Self::new()
    }
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
            // Skip events that don't contribute to the eval-apply turn shape
            // (progress, hooks, queue lifecycle, file snapshots). See the
            // predicate's doc comment for why each is excluded.
            let subtype = ce.subtype.as_deref().unwrap_or("");
            if should_skip_pattern_detection(subtype) {
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
            // Metric: PatternsConsumer is the new home for patterns_detected_total.
            crate::metrics::record_patterns_detected(all_patterns.len() as u64);
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

/// True for subtypes that must not feed the eval-apply pattern detector.
///
/// This is NOT the same concept as `projection::is_ephemeral` or
/// `Subtype::is_ephemeral`, which both mean "not stored durably" and
/// cover only `progress.*`. This predicate is broader — it also filters
/// metadata events that are durable but do not contribute to the
/// structural turn shape, plus events that lack a stable `event.time`.
///
/// The axes diverged enough that sharing the name `is_ephemeral`
/// invited silent drift — see
/// `docs/research/architecture-audit/IS_EPHEMERAL_DIVERGENCE.md`.
/// Renamed 2026-04-15.
///
/// `file.snapshot` is the load-bearing member: file snapshots arrive
/// as metadata before/around tool calls and have no timestamp from the
/// source JSONL. Before this filter was added they were stamping turn
/// `start_ts` with wall-clock-at-detection-time, causing every boot
/// replay to emit a fresh sentence row for the same logical turn (the
/// H2 case in scripts/inspect_sentence_dedup.py — 1.50× ratio).
fn should_skip_pattern_detection(subtype: &str) -> bool {
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

    // ── Audit: divergence from Subtype::is_ephemeral ──────────────────
    // See docs/research/architecture-audit/IS_EPHEMERAL_DIVERGENCE.md.
    // This test locks in the intentional mismatch: the pattern-detection
    // filter is broader than the "not durably stored" predicate because
    // it also excludes events that ARE stored but shouldn't feed
    // eval-apply. If the two ever converge, this test is where to
    // read WHY they differed and decide whether convergence is safe.

    #[test]
    fn pattern_detection_filter_is_broader_than_subtype_is_ephemeral() {
        use open_story_core::subtype::Subtype;
        use std::str::FromStr;

        // Subtype::is_ephemeral covers only progress.*
        assert!(Subtype::from_str("progress.bash").unwrap().is_ephemeral());
        assert!(!Subtype::from_str("file.snapshot").unwrap().is_ephemeral());
        assert!(!Subtype::from_str("queue.remove").unwrap().is_ephemeral());
        assert!(!Subtype::from_str("system.hook").unwrap().is_ephemeral());

        // This predicate is broader.
        assert!(should_skip_pattern_detection("progress.bash"));
        assert!(should_skip_pattern_detection("file.snapshot"));
        assert!(should_skip_pattern_detection("queue.remove"));
        assert!(should_skip_pattern_detection("queue.popAll"));
        assert!(should_skip_pattern_detection("system.hook"));

        // And narrower too — user prompts and assistant messages pass
        // through here, then do their real lifting in eval-apply.
        assert!(!should_skip_pattern_detection("message.user.prompt"));
        assert!(!should_skip_pattern_detection("message.assistant.text"));
        assert!(!should_skip_pattern_detection("message.assistant.tool_use"));
    }
}
