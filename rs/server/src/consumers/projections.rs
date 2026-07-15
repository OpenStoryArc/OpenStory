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
use open_story_store::event_store::EventStore;
use open_story_store::projection::SessionProjection;
use open_story_store::projection_cache::ProjectionCache;

/// State owned by the projections consumer actor.
pub struct ProjectionsConsumer {
    /// Durable event store — read to hydrate a cold session's full history
    /// before appending a live event, so a non-resident session never gets a
    /// partial projection built from only the new events.
    event_store: Arc<dyn EventStore>,
    /// Shared materialized view per session. Actor 3 is the sole writer;
    /// the API / WebSocket / other consumers read from the same bounded
    /// read-through cache without coordination.
    projections: Arc<ProjectionCache>,
    /// Session → project_id mapping (used when wired as independent consumer).
    #[allow(dead_code)]
    session_projects: HashMap<String, String>,
    /// Session → display name mapping (used when wired as independent consumer).
    #[allow(dead_code)]
    session_project_names: HashMap<String, String>,
    /// Subagent → parent session mapping (shared with StoreState).
    subagent_parents: Arc<DashMap<String, String>>,
    /// Parent → child session list (shared with StoreState).
    session_children: Arc<DashMap<String, Vec<String>>>,
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
    /// Construct a projections consumer backed by shared `StoreState` maps.
    /// Pass `state.store.event_store.clone()` / `state.store.projections.clone()`
    /// / `subagent_parents.clone()` / `session_children.clone()` — Arc clones
    /// are cheap (refcount only).
    pub fn new(
        event_store: Arc<dyn EventStore>,
        projections: Arc<ProjectionCache>,
        subagent_parents: Arc<DashMap<String, String>>,
        session_children: Arc<DashMap<String, Vec<String>>>,
    ) -> Self {
        Self {
            event_store,
            projections,
            session_projects: HashMap::new(),
            session_project_names: HashMap::new(),
            subagent_parents,
            session_children,
        }
    }

    /// Process a batch of CloudEvents — update projections.
    ///
    /// Async because a live event for a cold (evicted / never-seeded) session
    /// must hydrate that session's full durable history from SQLite BEFORE the
    /// in-place append (via `hydrate_and_append`), or the projection would be
    /// rebuilt from only the new events and lose prior token/event/filter
    /// totals. `seen_ids` dedups the reloaded history against re-delivery.
    pub async fn process_batch(
        &mut self,
        session_id: &str,
        events: &[CloudEvent],
    ) -> ProjectionsResult {
        let mut label_changed = false;

        for ce in events {
            let Ok(val) = serde_json::to_value(ce) else {
                continue;
            };

            // Track subagent → parent relationship (shared helper).
            open_story_store::state::detect_subagent_relationship(
                &val,
                session_id,
                &self.subagent_parents,
                &self.session_children,
            );

            // Hydrate cold history (if any) then append in place. The cache
            // accessor re-accounts bytes + recency and evicts to budget after
            // its shard-write guard is dropped. No `Ref` held across the await
            // or the append (deadlock-safe).
            let append_result = open_story_store::state::hydrate_and_append(
                self.event_store.as_ref(),
                &self.projections,
                session_id,
                |proj| proj.append(&val),
            )
            .await;

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

    /// How many sessions have projections resident today. Intended for tests.
    pub fn projection_count(&self) -> usize {
        self.projections.resident_sessions()
    }

    /// Get the parent session for a subagent.
    pub fn parent_session(&self, subagent_id: &str) -> Option<String> {
        self.subagent_parents
            .get(subagent_id)
            .map(|r| r.value().clone())
    }

    /// Get children (subagents) of a session (cloned snapshot).
    pub fn children(&self, session_id: &str) -> Vec<String> {
        self.session_children
            .get(session_id)
            .map(|r| r.value().clone())
            .unwrap_or_default()
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
            json!({}),
            0,
            session_id.to_string(),
            AgentPayload::ClaudeCode(payload),
        );
        CloudEvent::new(
            format!("arc://test/{session_id}"),
            "io.arc.event".into(),
            data,
            Some(subtype.into()),
            None,
            None,
            None,
            None,
            Some("claude-code".into()),
        )
    }

    fn empty_shared_map() -> Arc<ProjectionCache> {
        // Effectively unbounded (u64::MAX budget, no working-set window) so the
        // consumer tests see the old always-resident DashMap behavior.
        Arc::new(ProjectionCache::new(u64::MAX, 0))
    }

    fn empty_parents() -> Arc<DashMap<String, String>> {
        Arc::new(DashMap::new())
    }

    fn empty_children() -> Arc<DashMap<String, Vec<String>>> {
        Arc::new(DashMap::new())
    }

    /// An empty in-memory event store — hydration finds no prior history for
    /// these fresh sessions, so the consumer seeds the projection from the
    /// batch exactly as before the hydrate change.
    fn empty_event_store() -> Arc<dyn EventStore> {
        Arc::new(open_story_store::sqlite_store::SqliteStore::in_memory().unwrap())
    }

    fn make_consumer() -> ProjectionsConsumer {
        ProjectionsConsumer::new(
            empty_event_store(),
            empty_shared_map(),
            empty_parents(),
            empty_children(),
        )
    }

    #[test]
    fn new_consumer_has_empty_state() {
        let consumer = make_consumer();
        assert_eq!(consumer.projection_count(), 0);
    }

    #[tokio::test]
    async fn creates_projection_on_first_event() {
        let mut consumer = make_consumer();
        consumer
            .process_batch("sess-1", &[make_event("sess-1", "message.user.prompt")])
            .await;
        assert!(consumer.projection("sess-1").is_some());
    }

    #[tokio::test]
    async fn maintains_separate_projections_per_session() {
        let mut consumer = make_consumer();
        consumer
            .process_batch("sess-1", &[make_event("sess-1", "message.user.prompt")])
            .await;
        consumer
            .process_batch("sess-2", &[make_event("sess-2", "message.user.prompt")])
            .await;
        assert!(consumer.projection("sess-1").is_some());
        assert!(consumer.projection("sess-2").is_some());
    }

    /// Commit 1.3 landing test: the consumer writes into the caller's
    /// shared DashMap, so an externally-held `Arc` sees the projection
    /// without a sync step. Retires the previous "dead-state" tests.
    #[tokio::test]
    async fn writes_are_visible_via_shared_map() {
        let shared = empty_shared_map();
        let mut consumer = ProjectionsConsumer::new(
            empty_event_store(),
            shared.clone(),
            empty_parents(),
            empty_children(),
        );
        consumer
            .process_batch(
                "sess-shared",
                &[make_event("sess-shared", "message.user.prompt")],
            )
            .await;

        // The external holder of the Arc sees the same projection.
        assert!(
            shared.contains("sess-shared"),
            "external shared cache should see the consumer's write without any sync step"
        );
    }

    #[tokio::test]
    async fn processing_the_same_event_twice_is_deduped_internally_by_seen_ids() {
        // SessionProjection::append does dedup via its own seen_ids HashSet —
        // double-delivery from NATS at-least-once is absorbed transparently.
        let mut consumer = make_consumer();
        let ev = make_event("sess-dup", "message.user.prompt");
        consumer.process_batch("sess-dup", &[ev.clone()]).await;
        consumer.process_batch("sess-dup", &[ev]).await;

        let proj = consumer.projection("sess-dup").unwrap();
        assert_eq!(
            proj.event_count(),
            1,
            "SessionProjection.seen_ids dedups double-delivery internally"
        );
    }
}
