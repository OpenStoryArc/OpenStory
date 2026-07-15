//! `rebuild_session` — per-session reproject helper.
//!
//! Re-derives one session's `SessionProjection` purely from its stored
//! events (`EventStore::session_events` → `SessionProjection::append`).
//! Read-only with respect to the event log: it never drops, mutates, or
//! reorders events — SQLite stays the source of truth and this just
//! replays it. Shared by `reproject_all` (server crate), which loops this
//! over every session at boot.

use crate::event_store::EventStore;
use crate::projection::SessionProjection;

/// Rebuild a single session's projection from the durable event store.
///
/// Returns `None` if the session has no stored events (nothing to
/// project) or the store read fails.
pub async fn rebuild_session(
    store: &dyn EventStore,
    session_id: &str,
) -> Option<SessionProjection> {
    let events = store.session_events(session_id).await.unwrap_or_default();
    if events.is_empty() {
        return None;
    }
    let mut projection = SessionProjection::new(session_id);
    for event in &events {
        projection.append(event);
    }
    Some(projection)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sqlite_store::SqliteStore;
    use serde_json::{json, Value};
    use std::sync::Arc;

    /// A well-formed CloudEvent, matching the shape `sqlite_store.rs`'s own
    /// tests use (specversion/id/source/type/time/data all present — the
    /// fields `CloudEvent` requires to deserialize).
    fn test_event(id: &str, subtype: &str, time: &str, data: Value) -> Value {
        json!({
            "id": id,
            "specversion": "1.0",
            "datacontenttype": "application/json",
            "type": "io.arc.event",
            "subtype": subtype,
            "time": time,
            "source": "arc://test",
            "data": data,
        })
    }

    /// An in-memory SQLite-backed `EventStore` (real `EventStore`
    /// implementation, not a fake) seeded with `events` under `session_id`.
    async fn test_store_with_events(session_id: &str, events: &[Value]) -> Arc<dyn EventStore> {
        let store = SqliteStore::in_memory().expect("in-memory sqlite store");
        for event in events {
            store
                .insert_event(session_id, event)
                .await
                .expect("insert_event");
        }
        Arc::new(store)
    }

    #[tokio::test]
    async fn rebuild_session_replays_events_from_store() {
        let store = test_store_with_events(
            "s1",
            &[
                test_event(
                    "e1",
                    "message.user.prompt",
                    "2026-01-01T00:00:00Z",
                    json!({"text": "hi"}),
                ),
                test_event(
                    "e2",
                    "message.assistant.text",
                    "2026-01-01T00:00:01Z",
                    json!({"text": "yo"}),
                ),
            ],
        )
        .await;

        let p = rebuild_session(store.as_ref(), "s1").await.expect("some");
        assert_eq!(p.event_count(), 2);
    }

    #[tokio::test]
    async fn rebuild_session_returns_none_for_empty_session() {
        let store = test_store_with_events("s1", &[]).await;

        assert!(rebuild_session(store.as_ref(), "no-such-session")
            .await
            .is_none());
    }
}
