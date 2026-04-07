//! Persist consumer — stores every CloudEvent to durable storage.
//!
//! Actor contract:
//!   subscribes: events.>
//!   publishes:  nothing (pure sink)
//!   owns:       seen_event_ids, event_store, session_store, FTS index
//!
//! Responsibilities:
//!   1. Dedup by event ID (skip already-seen events)
//!   2. Insert into SQLite (event_store)
//!   3. Append to JSONL (session_store)
//!   4. Index in FTS5 for full-text search

use std::collections::HashSet;
use std::sync::Arc;

use open_story_core::cloud_event::CloudEvent;
use open_story_store::event_store::EventStore;
use open_story_store::persistence::SessionStore;
use open_story_views::from_cloud_event::from_cloud_event;

/// State owned by the persist consumer actor.
pub struct PersistConsumer {
    /// Event IDs already seen — for dedup.
    seen_event_ids: HashSet<String>,
    /// Shared event store (Arc — SQLite handles internal locking).
    event_store: Arc<dyn EventStore>,
    /// JSONL backup store (owned — only this actor writes).
    session_store: SessionStore,
}

/// Result of processing one batch of events.
pub struct PersistResult {
    /// Number of events persisted (after dedup).
    pub persisted: usize,
    /// Number of events skipped (dedup).
    pub skipped: usize,
}

impl PersistConsumer {
    /// Create a new persist consumer with owned state.
    pub fn new(event_store: Arc<dyn EventStore>, session_store: SessionStore) -> Self {
        Self {
            seen_event_ids: HashSet::new(),
            event_store,
            session_store,
        }
    }

    /// Process a batch of CloudEvents — dedup, persist, index.
    pub async fn process_batch(
        &mut self,
        session_id: &str,
        events: &[CloudEvent],
    ) -> PersistResult {
        let event_store = &*self.event_store;
        let session_store = &self.session_store;
        let mut persisted = 0;
        let mut skipped = 0;

        for ce in events {
            let Ok(val) = serde_json::to_value(ce) else {
                continue;
            };

            // Dedup: skip events we've already seen
            let event_id = val.get("id").and_then(|v| v.as_str()).unwrap_or("");
            if !event_id.is_empty() && !self.seen_event_ids.insert(event_id.to_string()) {
                skipped += 1;
                continue;
            }

            // Persist to JSONL backup
            let _ = session_store.append(session_id, &val);

            // Persist to the durable event store
            let _ = event_store.insert_event(session_id, &val).await;

            // Index in the full-text index
            let view_records = from_cloud_event(ce);
            for vr in &view_records {
                if let Some(text) = open_story_store::extract::extract_text(vr) {
                    let record_type = open_story_store::extract::record_type_str(&vr.body);
                    let _ = event_store.index_fts(&vr.id, session_id, record_type, &text).await;
                }
            }

            persisted += 1;
        }

        PersistResult { persisted, skipped }
    }

    /// Check if an event ID has already been seen.
    pub fn is_duplicate(&self, event_id: &str) -> bool {
        self.seen_event_ids.contains(event_id)
    }

    /// Number of unique events seen.
    pub fn seen_count(&self) -> usize {
        self.seen_event_ids.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use open_story_core::event_data::{AgentPayload, ClaudeCodePayload, EventData};
    use serde_json::json;

    fn test_event(id: &str) -> CloudEvent {
        let mut payload = ClaudeCodePayload::new();
        payload.text = Some("test content".to_string());
        let data = EventData::with_payload(
            json!({}), 0, "sess-1".to_string(),
            AgentPayload::ClaudeCode(payload),
        );
        CloudEvent::new(
            "arc://test/sess-1".into(),
            "io.arc.event".into(),
            data,
            Some("message.user.prompt".into()),
            Some(id.to_string()),
            None, None, None, None,
        )
    }

    fn make_consumer() -> (PersistConsumer, tempfile::TempDir) {
        let tmp = tempfile::tempdir().expect("create temp dir");
        let session_store = SessionStore::new(tmp.path()).expect("create session store");
        let event_log = open_story_store::persistence::EventLog::new(tmp.path()).expect("create event log");
        let event_store: Arc<dyn EventStore> = Arc::new(
            open_story_store::jsonl_store::JsonlStore::new(
                SessionStore::new(tmp.path()).expect("create session store"),
                event_log,
            ),
        );
        (PersistConsumer::new(event_store, session_store), tmp)
    }

    #[tokio::test]
    async fn dedup_tracks_event_ids() {
        let (mut consumer, _tmp) = make_consumer();
        assert!(!consumer.is_duplicate("evt-1"));

        // Manually insert
        consumer.seen_event_ids.insert("evt-1".to_string());
        assert!(consumer.is_duplicate("evt-1"));
        assert!(!consumer.is_duplicate("evt-2"));
    }

    #[tokio::test]
    async fn dedup_skips_duplicate_event_ids() {
        let (mut consumer, _tmp) = make_consumer();
        let e1 = test_event("evt-1");
        let e2 = test_event("evt-1"); // same ID
        let e3 = test_event("evt-2"); // different ID

        let result = consumer.process_batch("sess-1", &[e1, e2, e3]).await;
        assert_eq!(result.persisted, 2, "should persist 2 unique events");
        assert_eq!(result.skipped, 1, "should skip 1 duplicate");
        assert_eq!(consumer.seen_count(), 2);
    }

    #[tokio::test]
    async fn dedup_state_persists_across_batches() {
        let (mut consumer, _tmp) = make_consumer();

        let e1 = test_event("evt-1");
        let result1 = consumer.process_batch("sess-1", &[e1]).await;
        assert_eq!(result1.persisted, 1);

        let e1_again = test_event("evt-1");
        let result2 = consumer.process_batch("sess-1", &[e1_again]).await;
        assert_eq!(result2.persisted, 0);
        assert_eq!(result2.skipped, 1);
    }
}
