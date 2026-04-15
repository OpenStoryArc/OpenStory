//! Projections consumer — maintains session metadata materialized views.
//!
//! Actor contract:
//!   subscribes: events.>
//!   publishes:  changes.{project}.{session}
//!   owns:       projections, session_projects, plan_store
//!
//! Responsibilities:
//!   1. Update SessionProjection (token counts, event counts, labels, branches)
//!   2. Track project → session mappings
//!   3. Track subagent → parent relationships
//!   4. Extract and store plans
//!   5. Publish session metadata changes for the broadcast consumer

use std::collections::HashMap;

use open_story_core::cloud_event::CloudEvent;
use open_story_store::projection::SessionProjection;

/// State owned by the projections consumer actor.
pub struct ProjectionsConsumer {
    /// Materialized view per session.
    projections: HashMap<String, SessionProjection>,
    /// Session → project_id mapping (used when wired as independent consumer).
    #[allow(dead_code)]
    session_projects: HashMap<String, String>,
    /// Session → display name mapping (used when wired as independent consumer).
    #[allow(dead_code)]
    session_project_names: HashMap<String, String>,
    /// Subagent → parent session mapping.
    subagent_parents: HashMap<String, String>,
    /// Parent → child session list.
    session_children: HashMap<String, Vec<String>>,
}

/// Result of processing one batch through projections.
pub struct ProjectionsResult {
    /// Whether the session label changed (triggers broadcast).
    pub label_changed: bool,
    /// Updated token counts (if changed).
    pub total_input_tokens: Option<u64>,
    pub total_output_tokens: Option<u64>,
}

impl Default for ProjectionsConsumer {
    fn default() -> Self {
        Self::new()
    }
}

impl ProjectionsConsumer {
    pub fn new() -> Self {
        Self {
            projections: HashMap::new(),
            session_projects: HashMap::new(),
            session_project_names: HashMap::new(),
            subagent_parents: HashMap::new(),
            session_children: HashMap::new(),
        }
    }

    /// Process a batch of CloudEvents — update projections.
    pub fn process_batch(
        &mut self,
        session_id: &str,
        events: &[CloudEvent],
    ) -> ProjectionsResult {
        let mut label_changed = false;

        for ce in events {
            let Ok(val) = serde_json::to_value(ce) else { continue };

            // Track subagent → parent relationship
            if let Some(data_sid) = val.get("data")
                .and_then(|d| d.get("session_id"))
                .and_then(|v| v.as_str())
            {
                if data_sid != session_id && !self.subagent_parents.contains_key(session_id) {
                    self.subagent_parents.insert(session_id.to_string(), data_sid.to_string());
                    self.session_children
                        .entry(data_sid.to_string())
                        .or_default()
                        .push(session_id.to_string());
                }
            }

            // Update projection
            let proj = self.projections
                .entry(session_id.to_string())
                .or_insert_with(|| SessionProjection::new(session_id));
            let append_result = proj.append(&val);

            if append_result.label_changed {
                label_changed = true;
            }
        }

        let proj = self.projections.get(session_id);
        ProjectionsResult {
            label_changed,
            total_input_tokens: proj.map(|p| p.total_input_tokens()),
            total_output_tokens: proj.map(|p| p.total_output_tokens()),
        }
    }

    /// Get the projection for a session.
    pub fn projection(&self, session_id: &str) -> Option<&SessionProjection> {
        self.projections.get(session_id)
    }

    /// Get all projections.
    pub fn all_projections(&self) -> &HashMap<String, SessionProjection> {
        &self.projections
    }

    /// Get the parent session for a subagent.
    pub fn parent_session(&self, subagent_id: &str) -> Option<&str> {
        self.subagent_parents.get(subagent_id).map(|s| s.as_str())
    }

    /// Get children (subagents) of a session.
    pub fn children(&self, session_id: &str) -> &[String] {
        self.session_children
            .get(session_id)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }
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
        let consumer = ProjectionsConsumer::new();
        assert!(consumer.all_projections().is_empty());
    }

    #[test]
    fn creates_projection_on_first_event() {
        let mut consumer = ProjectionsConsumer::new();
        consumer.process_batch("sess-1", &[make_event("sess-1", "message.user.prompt")]);
        assert!(consumer.projection("sess-1").is_some());
    }

    #[test]
    fn maintains_separate_projections_per_session() {
        let mut consumer = ProjectionsConsumer::new();
        consumer.process_batch("sess-1", &[make_event("sess-1", "message.user.prompt")]);
        consumer.process_batch("sess-2", &[make_event("sess-2", "message.user.prompt")]);
        assert!(consumer.projection("sess-1").is_some());
        assert!(consumer.projection("sess-2").is_some());
    }

    // ── Dead-state characterization (architecture audit) ──────────────
    //
    // ProjectionsConsumer builds its own SessionProjection on the NATS
    // subscription. Nothing in the live server reads from it — all
    // downstream consumers (wire-record enrichment, label/branch
    // lookups, session metadata endpoints) read from `state.store.
    // projections`, which is mutated by ingest_events on Actor 4's
    // path. So this consumer's state is dead work today.
    //
    // The T5 backlog entry "Wire ↔ Projection Sync When Decomposing
    // Broadcast" and the "Decompose Actor 4" entry both depend on
    // changing this. These tests lock in today's isolation so the
    // swap is a visible change.

    #[test]
    fn consumer_state_is_isolated_from_external_projection_map() {
        // ProjectionsConsumer has no API to accept or sync a foreign
        // projection map. Only its own internal state exists. If this
        // test ever needs to assert a bridge/sync method, that's the
        // signal that the decomposition is landing and T5 barriers
        // must go in place.
        let mut consumer = ProjectionsConsumer::new();
        consumer.process_batch("sess-x", &[make_event("sess-x", "message.user.prompt")]);

        // Sanity: no public methods named sync_with, bridge_to, etc.
        // The consumer's only outputs are projection() / all_projections() /
        // parent_session() / children() — all read-only accessors from
        // its own state. Nothing accepts an external store.
        assert!(consumer.projection("sess-x").is_some());
        assert_eq!(consumer.all_projections().len(), 1);
    }

    #[test]
    fn processing_the_same_event_twice_is_deduped_internally_by_seen_ids() {
        // CORRECTED from an initial wrong assumption: SessionProjection::
        // append does dedup via its own `seen_ids: HashSet<String>` — see
        // rs/store/src/projection.rs:220. So this consumer is internally
        // robust to double-delivery (NATS at-least-once is fine).
        //
        // The dead-state concern is therefore NOT "the consumer would
        // drift if re-delivered" — it's simply that its accurate
        // in-memory projection is never read by any caller today. The
        // decomposition work needs to make it read, not make it correct.
        let mut consumer = ProjectionsConsumer::new();
        let ev = make_event("sess-dup", "message.user.prompt");
        consumer.process_batch("sess-dup", &[ev.clone()]);
        consumer.process_batch("sess-dup", &[ev]);

        let proj = consumer.projection("sess-dup").unwrap();
        assert_eq!(
            proj.event_count(),
            1,
            "SessionProjection.seen_ids dedups double-delivery internally"
        );
    }
}
