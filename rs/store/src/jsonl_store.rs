//! JsonlStore — fallback EventStore backed by JSONL files.
//!
//! Wraps the existing SessionStore + EventLog. Degrades gracefully:
//! pattern and plan storage are no-ops, cross-session queries return empty.

use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;

use open_story_patterns::PatternEvent;

use crate::event_store::{EventStore, SessionRow};
use crate::persistence::{EventLog, SessionStore};

/// JSONL-backed event store. Fallback when SQLite is unavailable.
pub struct JsonlStore {
    session_store: SessionStore,
    event_log: EventLog,
}

impl JsonlStore {
    pub fn new(session_store: SessionStore, event_log: EventLog) -> Self {
        Self {
            session_store,
            event_log,
        }
    }
}

#[async_trait]
impl EventStore for JsonlStore {
    async fn insert_event(&self, session_id: &str, event: &Value) -> Result<bool> {
        self.session_store.append(session_id, event)?;
        self.event_log.append(event)?;
        // JSONL can't cheaply dedup — always returns true.
        // Dedup responsibility stays with the caller.
        Ok(true)
    }

    async fn insert_batch(&self, session_id: &str, events: &[Value]) -> Result<usize> {
        for event in events {
            self.session_store.append(session_id, event)?;
            self.event_log.append(event)?;
        }
        Ok(events.len())
    }

    async fn session_events(&self, session_id: &str) -> Result<Vec<Value>> {
        Ok(self.session_store.load_session(session_id))
    }

    async fn list_sessions(&self) -> Result<Vec<SessionRow>> {
        Ok(self
            .session_store
            .list_sessions()
            .into_iter()
            .map(|id| SessionRow {
                id,
                project_id: None,
                project_name: None,
                label: None,
                custom_label: None,
                branch: None,
                event_count: 0,
                first_event: None,
                last_event: None,
            })
            .collect())
    }

    async fn upsert_session(&self, _session: &SessionRow) -> Result<()> {
        // No-op: JSONL has no session metadata table
        Ok(())
    }

    async fn insert_pattern(&self, _session_id: &str, _pattern: &PatternEvent) -> Result<()> {
        // No-op: patterns not persisted in JSONL mode
        Ok(())
    }

    async fn session_patterns(
        &self,
        _session_id: &str,
        _pattern_type: Option<&str>,
    ) -> Result<Vec<PatternEvent>> {
        // Not supported in JSONL mode
        Ok(vec![])
    }

    async fn insert_turn(&self, _session_id: &str, _turn: &open_story_patterns::StructuralTurn) -> Result<()> {
        Ok(())
    }

    async fn session_turns(&self, _session_id: &str) -> Result<Vec<open_story_patterns::StructuralTurn>> {
        Ok(vec![])
    }

    async fn upsert_plan(
        &self,
        _plan_id: &str,
        _session_id: &str,
        _content: &str,
    ) -> Result<()> {
        // No-op: plans not persisted in JSONL mode
        Ok(())
    }

    async fn full_payload(&self, _event_id: &str) -> Result<Option<String>> {
        // Not supported: would require scanning all JSONL files
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::TempDir;

    fn setup() -> (TempDir, JsonlStore) {
        let tmp = TempDir::new().unwrap();
        let session_store = SessionStore::new(tmp.path()).unwrap();
        let event_log = EventLog::new(tmp.path()).unwrap();
        let store = JsonlStore::new(session_store, event_log);
        (tmp, store)
    }

    #[tokio::test]
    async fn insert_event_appends_and_returns_true() {
        let (_tmp, store) = setup();
        let event = json!({"id": "evt-1", "type": "io.arc.event", "time": "2025-01-14T00:00:00Z"});
        assert!(store.insert_event("sess-1", &event).await.unwrap());
    }

    #[tokio::test]
    async fn insert_event_duplicate_still_returns_true() {
        let (_tmp, store) = setup();
        let event = json!({"id": "evt-1", "type": "io.arc.event", "time": "2025-01-14T00:00:00Z"});
        store.insert_event("sess-1", &event).await.unwrap();
        // JSONL can't dedup — caller is responsible
        assert!(store.insert_event("sess-1", &event).await.unwrap());
    }

    #[tokio::test]
    async fn session_events_reads_from_jsonl() {
        let (_tmp, store) = setup();
        let event = json!({"id": "evt-1", "type": "io.arc.event", "time": "2025-01-14T00:00:00Z"});
        store.insert_event("sess-1", &event).await.unwrap();

        let events = store.session_events("sess-1").await.unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["id"], "evt-1");
    }

    #[tokio::test]
    async fn list_sessions_delegates() {
        let (_tmp, store) = setup();
        store
            .insert_event("alpha", &json!({"id": "e1", "type": "test", "time": "t1"}))
            .await
            .unwrap();
        store
            .insert_event("beta", &json!({"id": "e2", "type": "test", "time": "t2"}))
            .await
            .unwrap();

        let sessions = store.list_sessions().await.unwrap();
        let ids: Vec<&str> = sessions.iter().map(|s| s.id.as_str()).collect();
        assert!(ids.contains(&"alpha"));
        assert!(ids.contains(&"beta"));
    }

    #[tokio::test]
    async fn session_patterns_returns_empty() {
        let (_tmp, store) = setup();
        assert!(store.session_patterns("sess-1", None).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn full_payload_returns_none() {
        let (_tmp, store) = setup();
        assert!(store.full_payload("evt-1").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn upsert_session_is_noop() {
        let (_tmp, store) = setup();
        let row = SessionRow {
            id: "s1".into(),
            project_id: None,
            project_name: None,
            label: None,
            custom_label: None,
            branch: None,
            event_count: 0,
            first_event: None,
            last_event: None,
        };
        // Should not error
        store.upsert_session(&row).await.unwrap();
    }
}
