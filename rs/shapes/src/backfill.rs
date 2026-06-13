//! Shared extraction over a session's events — used by both the live
//! `ShapesConsumer` and the on-demand backfill CLI, so they apply identical
//! rules. Pure: events in, rows out, no I/O. Persistence lives at the edges
//! (the consumer writes via the store; the CLI loops sessions and writes).
//!
//! Backfill is needed because the live bus subscription uses
//! `DeliverPolicy::New` — a freshly-deployed `ShapesConsumer` only sees events
//! published after it starts, never the historical corpus. The CLI replays
//! stored events through `shapes_for_session`; the stable row id makes re-runs
//! idempotent.

use open_story_core::cloud_event::CloudEvent;

use crate::{ShapeExtractor, ShapeRow};

/// Run every extractor over one session's events, applying the shapes skip
/// predicate. `session_id` is the owning id — stamped onto every row.
pub fn shapes_for_session(
    session_id: &str,
    events: &[CloudEvent],
    extractors: &[Box<dyn ShapeExtractor>],
) -> Vec<ShapeRow> {
    let mut rows = Vec::new();
    for ce in events {
        let subtype = ce.subtype.as_deref().unwrap_or("");
        if should_skip_shape_detection(subtype) {
            continue;
        }
        for extractor in extractors {
            rows.extend(extractor.extract(ce, session_id));
        }
    }
    rows
}

/// True for subtypes that can't carry a tool call worth shaping.
///
/// Matters for events that DO carry a tool but on a non-substantive subtype —
/// e.g. `progress.bash` carries a command but is an intermediate progress
/// frame, not the durable tool call, so shaping it would double-count. The
/// durable `message.assistant.tool_use` event is the one that counts.
pub fn should_skip_shape_detection(subtype: &str) -> bool {
    subtype.starts_with("progress.")
        || subtype == "system.hook"
        || subtype.starts_with("queue.")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::default_extractors;
    use open_story_core::event_data::{AgentPayload, ClaudeCodePayload, EventData};
    use serde_json::json;

    fn ev(event_id: &str, subtype: &str, tool: &str, args: serde_json::Value) -> CloudEvent {
        let mut p = ClaudeCodePayload::new();
        p.tool = Some(tool.to_string());
        p.args = Some(args);
        let data = EventData::with_payload(json!({}), 1, "parent".to_string(),
                                           AgentPayload::ClaudeCode(p));
        CloudEvent::new("test".into(), "io.arc.event".into(), data,
                        Some(subtype.into()), Some(event_id.into()),
                        Some("2026-01-01T00:00:00Z".into()), None, None, Some("claude-code".into()))
    }

    #[test]
    fn extracts_across_events_and_skips_progress_frames() {
        let extractors = default_extractors();
        let events = vec![
            ev("e1", "message.assistant.tool_use", "Bash", json!({"command": "git status"})),
            // progress frame carrying a command — must be skipped (no double-count)
            ev("e2", "progress.bash", "Bash", json!({"command": "git status"})),
        ];
        let rows = shapes_for_session("own", &events, &extractors);
        assert_eq!(rows.iter().filter(|r| r.shape_type == "bash-shape").count(), 1);
        assert!(rows.iter().all(|r| r.session_id == "own"));
    }
}
