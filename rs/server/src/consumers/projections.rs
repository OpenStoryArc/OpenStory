//! Projections consumer — maintains session metadata materialized views.
//!
//! Actor contract:
//!   subscribes: events.>
//!   publishes:  changes.{project}.{session}
//!   owns:       projections (via shared DashMap), session_projects, plan_store
//!
//! Responsibilities:
//!   1. Update SessionProjection (token counts, event counts, labels, branches)
//!   2. Track project → session mappings
//!   3. Track subagent → parent relationships
//!   4. Extract and store plans
//!   5. Publish session metadata changes for the broadcast consumer
//!
//! The `projections` field is an `Arc<DashMap>` shared with `AppState.store`.
//! Actor 3 is the sole **writer**; the API, WebSocket, and other consumers
//! read from the same map. This replaces the previous dead-code pattern
//! where ProjectionsConsumer maintained its own internal HashMap that
//! nothing read.

use std::collections::HashMap;
use std::sync::Arc;

use dashmap::DashMap;
use open_story_core::cloud_event::CloudEvent;
use open_story_store::projection::SessionProjection;

/// State owned by the projections consumer actor.
pub struct ProjectionsConsumer {
    /// Shared materialized view per session. Actor 3 is the sole writer;
    /// the API / WebSocket / other consumers read from the same map
    /// without coordination.
    projections: Arc<DashMap<String, SessionProjection>>,
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

impl ProjectionsConsumer {
    /// Construct a projections consumer backed by the shared projection
    /// DashMap from `StoreState`. Pass `state.store.projections.clone()`
    /// (the `Arc` is cheap to clone — refcount only).
    pub fn new(projections: Arc<DashMap<String, SessionProjection>>) -> Self {
        Self {
            projections,
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

            // Track subagent → parent relationship (shared helper).
            open_story_store::state::detect_subagent_relationship(
                &val,
                session_id,
                &mut self.subagent_parents,
                &mut self.session_children,
            );

            // Update projection (DashMap: entry().or_insert_with — same shape
            // as HashMap, different guard type).
            let mut proj = self
                .projections
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
            total_input_tokens: proj.as_ref().map(|p| p.total_input_tokens()),
            total_output_tokens: proj.as_ref().map(|p| p.total_output_tokens()),
        }
    }

    /// Get a snapshot of the projection for a session (clone).
    /// Callers that want a long-lived borrow should use the shared
    /// `state.store.projections` directly and hold a `DashMap::Ref`.
    pub fn projection(&self, session_id: &str) -> Option<SessionProjection> {
        self.projections.get(session_id).map(|r| r.value().clone())
    }

    /// How many sessions have projections today. Intended for tests.
    pub fn projection_count(&self) -> usize {
        self.projections.len()
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

    fn empty_shared_map() -> Arc<DashMap<String, SessionProjection>> {
        Arc::new(DashMap::new())
    }

    #[test]
    fn new_consumer_has_empty_state() {
        let consumer = ProjectionsConsumer::new(empty_shared_map());
        assert_eq!(consumer.projection_count(), 0);
    }

    #[test]
    fn creates_projection_on_first_event() {
        let mut consumer = ProjectionsConsumer::new(empty_shared_map());
        consumer.process_batch("sess-1", &[make_event("sess-1", "message.user.prompt")]);
        assert!(consumer.projection("sess-1").is_some());
    }

    #[test]
    fn maintains_separate_projections_per_session() {
        let mut consumer = ProjectionsConsumer::new(empty_shared_map());
        consumer.process_batch("sess-1", &[make_event("sess-1", "message.user.prompt")]);
        consumer.process_batch("sess-2", &[make_event("sess-2", "message.user.prompt")]);
        assert!(consumer.projection("sess-1").is_some());
        assert!(consumer.projection("sess-2").is_some());
    }

    /// Commit 1.3 landing test: the consumer writes into the caller's
    /// shared DashMap, so an externally-held `Arc` sees the projection
    /// without a sync step. Retires the previous "dead-state" tests.
    #[test]
    fn writes_are_visible_via_shared_map() {
        let shared = empty_shared_map();
        let mut consumer = ProjectionsConsumer::new(shared.clone());
        consumer.process_batch("sess-shared", &[make_event("sess-shared", "message.user.prompt")]);

        // The external holder of the Arc sees the same projection.
        assert!(
            shared.contains_key("sess-shared"),
            "external shared map should see the consumer's write without any sync step"
        );
    }

    #[test]
    fn processing_the_same_event_twice_is_deduped_internally_by_seen_ids() {
        // SessionProjection::append does dedup via its own seen_ids HashSet —
        // double-delivery from NATS at-least-once is absorbed transparently.
        let mut consumer = ProjectionsConsumer::new(empty_shared_map());
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
