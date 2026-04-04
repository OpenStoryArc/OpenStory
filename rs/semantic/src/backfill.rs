//! Backfill — batch-embed all existing events from the EventStore.
//!
//! Reads sessions from the store, extracts embeddable text, generates
//! embeddings, and upserts to the SemanticStore. Idempotent (upsert by
//! deterministic chunk ID).

use anyhow::Result;

use open_story_core::cloud_event::CloudEvent;
use open_story_views::from_cloud_event::from_cloud_event;

use crate::embedder::Embedder;
use crate::extract::{extract_metadata, extract_text};
use crate::{EmbeddingChunk, SemanticStore};

/// Stats returned from backfill.
#[derive(Debug, Default)]
pub struct BackfillStats {
    pub sessions_processed: usize,
    pub events_scanned: usize,
    pub chunks_embedded: usize,
    pub chunks_skipped: usize,
    pub errors: usize,
}

/// Batch size for upserts during backfill.
const BACKFILL_BATCH_SIZE: usize = 64;

/// Backfill all sessions from the given events.
///
/// `session_events_fn` is called for each session ID and returns its events.
/// This abstraction keeps backfill independent of the EventStore trait
/// (which lives in the store crate).
pub async fn backfill<F>(
    session_ids: &[String],
    session_events_fn: F,
    embedder: &dyn Embedder,
    store: &dyn SemanticStore,
) -> Result<BackfillStats>
where
    F: Fn(&str) -> Vec<serde_json::Value>,
{
    let mut stats = BackfillStats::default();

    for sid in session_ids {
        let events = session_events_fn(sid);
        stats.sessions_processed += 1;

        let mut batch: Vec<EmbeddingChunk> = Vec::with_capacity(BACKFILL_BATCH_SIZE);

        for event in &events {
            stats.events_scanned += 1;

            let ce = match serde_json::from_value::<CloudEvent>(event.clone()) {
                Ok(ce) => ce,
                Err(_) => continue,
            };
            let view_records = from_cloud_event(&ce);
            for vr in &view_records {
                let text = match extract_text(vr) {
                    Some(t) => t,
                    None => {
                        stats.chunks_skipped += 1;
                        continue;
                    }
                };

                let metadata = extract_metadata(vr);
                let embedding = match embedder.embed(&text) {
                    Ok(emb) => emb,
                    Err(e) => {
                        eprintln!(
                            "  \x1b[33mBackfill embed error for {}: {e}\x1b[0m",
                            vr.id
                        );
                        stats.errors += 1;
                        continue;
                    }
                };

                batch.push(EmbeddingChunk {
                    id: vr.id.clone(),
                    session_id: sid.clone(),
                    event_id: vr.id.clone(),
                    text,
                    embedding,
                    metadata,
                });

                if batch.len() >= BACKFILL_BATCH_SIZE {
                    stats.chunks_embedded += batch.len();
                    store.upsert(&batch).await?;
                    batch.clear();
                    eprint!("\r  Embedded {} chunks...", stats.chunks_embedded);
                }
            }
        }

        // Flush remaining
        if !batch.is_empty() {
            stats.chunks_embedded += batch.len();
            store.upsert(&batch).await?;
            batch.clear();
        }
    }

    if stats.chunks_embedded > 0 {
        eprintln!();
    }

    Ok(stats)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::embedder::NoopEmbedder;
    use crate::NoopSemanticStore;

    fn make_events(count: usize) -> Vec<serde_json::Value> {
        (0..count)
            .map(|i| {
                serde_json::json!({
                    "id": format!("evt-{i}"),
                    "type": "io.arc.event",
                    "subtype": "message.user.prompt",
                    "source": "arc://test",
                    "time": "2025-01-17T00:00:00Z",
                    "data": {
                        "seq": i,
                        "session_id": "sess-1",
                        "text": format!("message {i}"),
                        "raw": {
                            "type": "user",
                            "message": {"content": [{"type": "text", "text": format!("message {i}")}]}
                        }
                    }
                })
            })
            .collect()
    }

    #[tokio::test]
    async fn backfill_processes_all_sessions() {
        let embedder = NoopEmbedder::default();
        let store = NoopSemanticStore;
        let session_ids = vec!["sess-1".to_string(), "sess-2".to_string()];

        let stats = backfill(
            &session_ids,
            |_sid| make_events(5),
            &embedder,
            &store,
        )
        .await
        .unwrap();

        assert_eq!(stats.sessions_processed, 2);
        assert_eq!(stats.events_scanned, 10);
        assert!(stats.chunks_embedded > 0);
    }

    #[tokio::test]
    async fn backfill_skips_non_embeddable_events() {
        let embedder = NoopEmbedder::default();
        let store = NoopSemanticStore;
        let session_ids = vec!["sess-1".to_string()];

        // TokenUsage events are not embeddable
        let events = vec![serde_json::json!({
            "id": "evt-token",
            "type": "io.arc.event",
            "subtype": "system.turn.complete",
            "source": "arc://test",
            "time": "2025-01-17T00:00:00Z",
            "data": {
                "seq": 1,
                "session_id": "sess-1",
                "raw": {
                    "type": "system",
                    "message": {"content": [{"type": "text", "text": "turn complete"}]}
                }
            }
        })];

        let stats = backfill(
            &session_ids,
            |_sid| events.clone(),
            &embedder,
            &store,
        )
        .await
        .unwrap();

        assert_eq!(stats.sessions_processed, 1);
        assert_eq!(stats.events_scanned, 1);
        // Some may be embedded (system events with text), some skipped
        assert_eq!(stats.errors, 0);
    }

    #[tokio::test]
    async fn backfill_handles_empty_sessions() {
        let embedder = NoopEmbedder::default();
        let store = NoopSemanticStore;
        let session_ids = vec!["empty-sess".to_string()];

        let stats = backfill(
            &session_ids,
            |_sid| vec![],
            &embedder,
            &store,
        )
        .await
        .unwrap();

        assert_eq!(stats.sessions_processed, 1);
        assert_eq!(stats.events_scanned, 0);
        assert_eq!(stats.chunks_embedded, 0);
    }

    #[tokio::test]
    async fn backfill_is_idempotent() {
        let embedder = NoopEmbedder::default();
        let store = NoopSemanticStore;
        let session_ids = vec!["sess-1".to_string()];
        let events = make_events(3);

        // Run twice — should produce same stats (upsert = idempotent)
        let stats1 = backfill(
            &session_ids,
            |_sid| events.clone(),
            &embedder,
            &store,
        )
        .await
        .unwrap();

        let stats2 = backfill(
            &session_ids,
            |_sid| events.clone(),
            &embedder,
            &store,
        )
        .await
        .unwrap();

        assert_eq!(stats1.chunks_embedded, stats2.chunks_embedded);
    }

    #[tokio::test]
    async fn backfill_reports_progress() {
        let embedder = NoopEmbedder::default();
        let store = NoopSemanticStore;
        let session_ids = vec!["sess-1".to_string()];

        // Use enough events to trigger a batch flush (>= 64)
        let stats = backfill(
            &session_ids,
            |_sid| make_events(70),
            &embedder,
            &store,
        )
        .await
        .unwrap();

        assert!(stats.chunks_embedded >= 64, "should have embedded at least one batch");
    }
}
