//! Shapes consumer — deterministic per-event shape projections + live analysis.
//!
//! Actor contract:
//!   subscribes: events.>
//!   publishes:  BroadcastMessage::Shapes (via broadcast_tx, by the caller)
//!   owns:       the extractor set + per-session running `ShapeCounts`
//!
//! Each event runs through every extractor (bash / path / change). The resulting
//! `ShapeRow`s are written straight to the EventStore by the caller, and folded
//! into per-session running counts here — the live analysis the UI renders as a
//! sink. Extraction goes through the shared `shapes_for_session` (one source of
//! truth with the backfill CLI), called once.

use std::collections::HashMap;

use open_story_core::cloud_event::CloudEvent;
use open_story_shapes::{default_extractors, shapes_for_session, ShapeCounts, ShapeExtractor, ShapeRow};

/// State owned by the shapes consumer actor: extractors + per-session counts.
pub struct ShapesConsumer {
    extractors: Vec<Box<dyn ShapeExtractor>>,
    counts: HashMap<String, ShapeCounts>,
}

/// Output of processing one batch: the new rows (to persist) and the session's
/// updated running counts (to broadcast).
pub struct ShapesResult {
    pub rows: Vec<ShapeRow>,
    pub counts: ShapeCounts,
}

impl Default for ShapesConsumer {
    fn default() -> Self {
        Self::new()
    }
}

impl ShapesConsumer {
    pub fn new() -> Self {
        Self {
            extractors: default_extractors(),
            counts: HashMap::new(),
        }
    }

    /// Extract shape rows from a batch and fold them into the session's running
    /// counts. Keyed on the caller's `session_id` (the batch / subagent's own
    /// id) — never `event.data.session_id`, which is the parent. Mirrors how
    /// every other projection attributes.
    pub fn process_batch(&mut self, session_id: &str, events: &[CloudEvent]) -> ShapesResult {
        let rows = shapes_for_session(session_id, events, &self.extractors);
        let counts = self.counts.entry(session_id.to_string()).or_default();
        counts.ingest(&rows);
        ShapesResult {
            rows,
            counts: counts.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use open_story_core::event_data::{AgentPayload, ClaudeCodePayload, EventData};
    use serde_json::json;

    fn tool_event(event_id: &str, subtype: &str, tool: &str, args: serde_json::Value) -> CloudEvent {
        let mut p = ClaudeCodePayload::new();
        p.tool = Some(tool.to_string());
        p.args = Some(args);
        let data = EventData::with_payload(json!({}), 1, "parent".to_string(),
                                           AgentPayload::ClaudeCode(p));
        CloudEvent::new("test".into(), "io.arc.event".into(), data,
                        Some(subtype.to_string()), Some(event_id.into()),
                        Some("2026-01-01T00:00:00Z".into()), None, None, Some("claude-code".into()))
    }

    #[test]
    fn runs_all_extractors_stamps_session_and_folds_counts() {
        let mut consumer = ShapesConsumer::new();
        let events = vec![
            tool_event("e1", "message.assistant.tool_use", "Bash", json!({"command": "git status"})),
            tool_event("e2", "message.assistant.tool_use", "Write", json!({"file_path": "/x/a.rs", "content": "fn main(){}"})),
        ];
        let result = consumer.process_batch("own-sess", &events);
        // Bash → bash-shape; Write → path-shape + change-shape.
        let types: Vec<&str> = result.rows.iter().map(|r| r.shape_type.as_str()).collect();
        assert!(types.contains(&"bash-shape"));
        assert!(types.contains(&"path-shape"));
        assert!(types.contains(&"change-shape"));
        assert!(result.rows.iter().all(|r| r.session_id == "own-sess"));
        // counts folded
        assert_eq!(result.counts.bash, 1);
        assert_eq!(result.counts.path, 1);
        assert_eq!(result.counts.change, 1);
        assert_eq!(result.counts.programs.get("git"), Some(&1));
    }

    #[test]
    fn counts_accumulate_and_are_per_session_isolated() {
        let mut consumer = ShapesConsumer::new();
        let bash = |id: &str| tool_event(id, "message.assistant.tool_use", "Bash", json!({"command": "git log"}));
        consumer.process_batch("a", &[bash("a1")]);
        let a2 = consumer.process_batch("a", &[bash("a2")]);
        let b1 = consumer.process_batch("b", &[bash("b1")]);
        assert_eq!(a2.counts.bash, 2); // cumulative within session a
        assert_eq!(b1.counts.bash, 1); // session b is independent
    }

    #[test]
    fn skips_progress_and_hook_subtypes() {
        let mut consumer = ShapesConsumer::new();
        let ev = tool_event("e", "progress.bash", "Bash", json!({"command": "ls"}));
        assert!(consumer.process_batch("s", &[ev]).rows.is_empty());
    }
}
