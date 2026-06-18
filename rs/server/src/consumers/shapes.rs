//! Shapes consumer — deterministic per-event shape projections.
//!
//! Actor contract:
//!   subscribes: events.>
//!   publishes:  nothing (shapes are a pure function of events; any node
//!               re-derives them by running its own consumer, so there's no
//!               NATS `shapes` stream)
//!   owns:       the set of stateless `ShapeExtractor`s
//!
//! Each event is run through every extractor (bash / path / change for the
//! MVP). The resulting `ShapeRow`s are written straight to the EventStore by
//! the caller via `insert_shapes_batch`. Stateless — no per-session state
//! (snapshot-shape, which diffs against the previous snapshot, will add that
//! when it lands).

use open_story_core::cloud_event::CloudEvent;
use open_story_shapes::{default_extractors, shapes_for_session, ShapeExtractor, ShapeRow};

/// State owned by the shapes consumer actor: the extractor set.
pub struct ShapesConsumer {
    extractors: Vec<Box<dyn ShapeExtractor>>,
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
        }
    }

    /// Extract shape rows from a batch of events. Keyed on the caller's
    /// `session_id` (the batch / subagent's own id) — never `event.data.session_id`,
    /// which is the parent. Mirrors how every other projection attributes.
    pub fn process_batch(&self, session_id: &str, events: &[CloudEvent]) -> Vec<ShapeRow> {
        // Same extraction path as the backfill CLI — one source of truth for
        // the skip predicate and the extractor loop.
        shapes_for_session(session_id, events, &self.extractors)
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
    fn runs_all_extractors_and_stamps_session() {
        let consumer = ShapesConsumer::new();
        let events = vec![
            tool_event("e1", "message.assistant.tool_use", "Bash", json!({"command": "git status"})),
            tool_event("e2", "message.assistant.tool_use", "Write", json!({"file_path": "/x/a.rs", "content": "fn main(){}"})),
        ];
        let rows = consumer.process_batch("own-sess", &events);
        // Bash → bash-shape; Write → path-shape + change-shape.
        let types: Vec<&str> = rows.iter().map(|r| r.shape_type.as_str()).collect();
        assert!(types.contains(&"bash-shape"));
        assert!(types.contains(&"path-shape"));
        assert!(types.contains(&"change-shape"));
        assert!(rows.iter().all(|r| r.session_id == "own-sess"));
    }

    #[test]
    fn skips_progress_and_hook_subtypes() {
        let consumer = ShapesConsumer::new();
        let ev = tool_event("e", "progress.bash", "Bash", json!({"command": "ls"}));
        assert!(consumer.process_batch("s", &[ev]).is_empty());
    }
}
