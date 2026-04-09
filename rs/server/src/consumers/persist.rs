//! Persist consumer — stores every CloudEvent to durable storage.
//!
//! Actor contract:
//!   subscribes: events.>
//!   publishes:  nothing (pure sink)
//!   owns:       event_store, session_store, FTS index
//!
//! Responsibilities:
//!   1. Insert into the durable EventStore (SQLite or MongoDB).
//!      Dedup is the EventStore's PRIMARY KEY job — `insert_event`
//!      returns `Ok(false)` for duplicates and we treat that as "skipped"
//!      without further work. The legacy in-memory `seen_event_ids`
//!      HashSet was retired alongside the /hooks endpoint that needed it
//!      (the watcher is the sole ingestion source, so we no longer need
//!      to defend against the same event arriving twice via two paths).
//!   2. Append to JSONL backup (only on a successful insert — duplicates
//!      shouldn't pollute the sovereignty escape hatch).
//!   3. Index in FTS for full-text search (only on a successful insert).
//!
//! Cost note: relying on the EventStore PK means each duplicate now
//! costs one DB roundtrip rather than a HashMap lookup. In practice
//! duplicates are rare (one ingestion path), so the simplification wins.

use std::sync::Arc;

use open_story_core::cloud_event::CloudEvent;
use open_story_store::event_store::EventStore;
use open_story_store::persistence::SessionStore;
use open_story_views::from_cloud_event::from_cloud_event;

/// State owned by the persist consumer actor.
pub struct PersistConsumer {
    /// Shared event store (Arc — SQLite handles internal locking).
    event_store: Arc<dyn EventStore>,
    /// JSONL backup store (owned — only this actor writes).
    session_store: SessionStore,
}

/// Result of processing one batch of events.
pub struct PersistResult {
    /// Number of events persisted (after PK dedup).
    pub persisted: usize,
    /// Number of events skipped (PK collision — already persisted).
    pub skipped: usize,
}

impl PersistConsumer {
    /// Create a new persist consumer with owned state.
    pub fn new(event_store: Arc<dyn EventStore>, session_store: SessionStore) -> Self {
        Self {
            event_store,
            session_store,
        }
    }

    /// Process a batch of CloudEvents — persist + index.
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

            // EventStore PK is the dedup boundary. Returns Ok(false) when the
            // event_id already exists, Ok(true) on a fresh insert.
            let inserted = event_store
                .insert_event(session_id, &val)
                .await
                .unwrap_or(false);

            if !inserted {
                skipped += 1;
                continue;
            }

            // Append to JSONL backup only on a successful insert — duplicates
            // shouldn't pollute the sovereignty escape hatch.
            let _ = session_store.append(session_id, &val);

            // Index in the full-text index.
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
        // Use SqliteStore (not the JsonlStore fallback) so the EventStore PK
        // constraint is enforced — dedup tests need real PK rejection on
        // duplicate event_ids, not the JsonlStore's append-only "every insert
        // returns Ok(true)" behavior.
        let event_store: Arc<dyn EventStore> = Arc::new(
            open_story_store::sqlite_store::SqliteStore::new(tmp.path())
                .expect("create sqlite store"),
        );
        (PersistConsumer::new(event_store, session_store), tmp)
    }

    #[tokio::test]
    async fn dedup_skips_duplicate_event_ids_via_pk() {
        let (mut consumer, _tmp) = make_consumer();
        let e1 = test_event("evt-1");
        let e2 = test_event("evt-1"); // same ID
        let e3 = test_event("evt-2"); // different ID

        let result = consumer.process_batch("sess-1", &[e1, e2, e3]).await;
        assert_eq!(result.persisted, 2, "should persist 2 unique events");
        assert_eq!(result.skipped, 1, "should skip 1 duplicate via PK collision");
    }

    #[tokio::test]
    async fn dedup_state_persists_across_batches() {
        let (mut consumer, _tmp) = make_consumer();

        let e1 = test_event("evt-1");
        let result1 = consumer.process_batch("sess-1", &[e1]).await;
        assert_eq!(result1.persisted, 1);

        // Same event_id in a fresh batch — the EventStore PK still rejects it.
        let e1_again = test_event("evt-1");
        let result2 = consumer.process_batch("sess-1", &[e1_again]).await;
        assert_eq!(result2.persisted, 0);
        assert_eq!(result2.skipped, 1);
    }
}
