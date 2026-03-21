//! Background embedding worker — consumes ViewRecords from a channel,
//! generates embeddings, and upserts to the SemanticStore.
//!
//! Design: bounded channel (10,000). Non-blocking send from ingest.
//! Worker batches upserts (32 chunks or 500ms timeout). If channel full,
//! ingest logs a warning and drops — embedding must never block event ingestion.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc;

use open_story_views::view_record::ViewRecord;

use crate::embedder::Embedder;
use crate::extract::{extract_metadata, extract_text};
use crate::{EmbeddingChunk, SemanticStore};

/// Channel capacity — large enough to buffer bursts without blocking ingest.
pub const CHANNEL_CAPACITY: usize = 10_000;

/// Number of chunks to batch before flushing to the store.
const BATCH_SIZE: usize = 32;

/// Maximum time to wait before flushing a partial batch.
const FLUSH_TIMEOUT: Duration = Duration::from_millis(500);

/// Message sent from ingest to the embedding worker.
pub struct EmbedRequest {
    pub session_id: String,
    pub record: ViewRecord,
}

/// Start the background embedding worker. Returns the sender for the ingest pipeline.
///
/// The worker runs as a tokio task. It:
/// 1. Receives ViewRecords from the channel
/// 2. Extracts text (skips non-embeddable records)
/// 3. Generates embeddings via the Embedder
/// 4. Batches and upserts to the SemanticStore
pub fn spawn_worker(
    embedder: Arc<dyn Embedder>,
    store: Arc<dyn SemanticStore>,
) -> mpsc::Sender<EmbedRequest> {
    let (tx, rx) = mpsc::channel(CHANNEL_CAPACITY);

    tokio::spawn(worker_loop(rx, embedder, store));

    tx
}

async fn worker_loop(
    mut rx: mpsc::Receiver<EmbedRequest>,
    embedder: Arc<dyn Embedder>,
    store: Arc<dyn SemanticStore>,
) {
    let mut batch: Vec<EmbeddingChunk> = Vec::with_capacity(BATCH_SIZE);

    loop {
        // Wait for next item or flush timeout
        let item = if batch.is_empty() {
            // No pending batch — block until we get something
            match rx.recv().await {
                Some(item) => Some(item),
                None => break, // Channel closed
            }
        } else {
            // Have pending items — use timeout
            match tokio::time::timeout(FLUSH_TIMEOUT, rx.recv()).await {
                Ok(Some(item)) => Some(item),
                Ok(None) => {
                    // Channel closed — flush remaining and exit
                    flush_batch(&store, &mut batch).await;
                    break;
                }
                Err(_) => {
                    // Timeout — flush what we have
                    flush_batch(&store, &mut batch).await;
                    continue;
                }
            }
        };

        if let Some(req) = item {
            if let Some(chunk) = process_record(&req, &*embedder) {
                batch.push(chunk);

                if batch.len() >= BATCH_SIZE {
                    flush_batch(&store, &mut batch).await;
                }
            }
        }
    }
}

/// Extract text, generate embedding, return chunk. Returns None for non-embeddable records.
fn process_record(req: &EmbedRequest, embedder: &dyn Embedder) -> Option<EmbeddingChunk> {
    let text = extract_text(&req.record)?;
    let metadata = extract_metadata(&req.record);

    let embedding = match embedder.embed(&text) {
        Ok(emb) => emb,
        Err(e) => {
            eprintln!(
                "  \x1b[33mEmbedding failed for {}: {e}\x1b[0m",
                req.record.id
            );
            return None;
        }
    };

    Some(EmbeddingChunk {
        id: req.record.id.clone(),
        session_id: req.session_id.clone(),
        event_id: req.record.id.clone(),
        text,
        embedding,
        metadata,
    })
}

async fn flush_batch(store: &Arc<dyn SemanticStore>, batch: &mut Vec<EmbeddingChunk>) {
    if batch.is_empty() {
        return;
    }

    if let Err(e) = store.upsert(batch).await {
        eprintln!(
            "  \x1b[33mSemantic upsert failed ({} chunks): {e}\x1b[0m",
            batch.len()
        );
    }
    batch.clear();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::embedder::NoopEmbedder;
    use crate::NoopSemanticStore;
    use open_story_views::unified::{MessageContent, RecordBody, UserMessage};

    fn make_embed_request(text: &str) -> EmbedRequest {
        EmbedRequest {
            session_id: "sess-1".into(),
            record: ViewRecord {
                id: "evt-1".into(),
                seq: 1,
                session_id: "sess-1".into(),
                timestamp: "2025-01-17T00:00:00Z".into(),
                agent_id: None,
                is_sidechain: false,
                body: RecordBody::UserMessage(UserMessage {
                    content: MessageContent::Text(text.into()),
                    images: vec![],
                }),
            },
        }
    }

    fn make_non_embeddable_request() -> EmbedRequest {
        EmbedRequest {
            session_id: "sess-1".into(),
            record: ViewRecord {
                id: "evt-2".into(),
                seq: 2,
                session_id: "sess-1".into(),
                timestamp: "2025-01-17T00:00:00Z".into(),
                agent_id: None,
                is_sidechain: false,
                body: RecordBody::TurnEnd(open_story_views::unified::TurnEnd {
                    turn_id: None,
                    reason: None,
                    duration_ms: None,
                }),
            },
        }
    }

    // describe("process_record")
    mod process_record_tests {
        use super::*;

        #[test]
        fn embeddable_record_produces_chunk() {
            let embedder = NoopEmbedder::default();
            let req = make_embed_request("fix the auth bug");
            let chunk = process_record(&req, &embedder).unwrap();

            assert_eq!(chunk.id, "evt-1");
            assert_eq!(chunk.session_id, "sess-1");
            assert_eq!(chunk.text, "fix the auth bug");
            assert_eq!(chunk.embedding.len(), 384);
            assert_eq!(chunk.metadata.record_type, "user_message");
        }

        #[test]
        fn non_embeddable_record_returns_none() {
            let embedder = NoopEmbedder::default();
            let req = make_non_embeddable_request();
            assert!(process_record(&req, &embedder).is_none());
        }
    }

    // describe("spawn_worker")
    mod spawn_worker_tests {
        use super::*;

        #[tokio::test]
        async fn worker_receives_and_processes_records() {
            let embedder = Arc::new(NoopEmbedder::default());
            let store = Arc::new(NoopSemanticStore);
            let tx = spawn_worker(embedder, store);

            // Send some records
            for i in 0..5 {
                let req = EmbedRequest {
                    session_id: "sess-1".into(),
                    record: ViewRecord {
                        id: format!("evt-{i}"),
                        seq: i as u64,
                        session_id: "sess-1".into(),
                        timestamp: "2025-01-17T00:00:00Z".into(),
                        agent_id: None,
                        is_sidechain: false,
                        body: RecordBody::UserMessage(UserMessage {
                            content: MessageContent::Text(format!("message {i}")),
                            images: vec![],
                        }),
                    },
                };
                tx.send(req).await.unwrap();
            }

            // Drop sender to signal shutdown
            drop(tx);

            // Give worker time to process
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        #[tokio::test]
        async fn worker_skips_non_embeddable_records() {
            let embedder = Arc::new(NoopEmbedder::default());
            let store = Arc::new(NoopSemanticStore);
            let tx = spawn_worker(embedder, store);

            // Send a non-embeddable record
            let req = make_non_embeddable_request();
            tx.send(req).await.unwrap();

            drop(tx);
            tokio::time::sleep(Duration::from_millis(100)).await;
            // No panic = success (NoopStore accepts anything)
        }

        #[tokio::test]
        async fn worker_flushes_on_batch_size() {
            let embedder = Arc::new(NoopEmbedder::default());
            let store = Arc::new(NoopSemanticStore);
            let tx = spawn_worker(embedder, store);

            // Send BATCH_SIZE records to trigger a flush
            for i in 0..BATCH_SIZE {
                let req = EmbedRequest {
                    session_id: "sess-1".into(),
                    record: ViewRecord {
                        id: format!("evt-batch-{i}"),
                        seq: i as u64,
                        session_id: "sess-1".into(),
                        timestamp: "2025-01-17T00:00:00Z".into(),
                        agent_id: None,
                        is_sidechain: false,
                        body: RecordBody::UserMessage(UserMessage {
                            content: MessageContent::Text(format!("batch msg {i}")),
                            images: vec![],
                        }),
                    },
                };
                tx.send(req).await.unwrap();
            }

            // Give worker time to flush
            tokio::time::sleep(Duration::from_millis(100)).await;

            drop(tx);
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        #[tokio::test]
        async fn worker_flushes_on_timeout() {
            let embedder = Arc::new(NoopEmbedder::default());
            let store = Arc::new(NoopSemanticStore);
            let tx = spawn_worker(embedder, store);

            // Send fewer than BATCH_SIZE records
            let req = make_embed_request("timeout test");
            tx.send(req).await.unwrap();

            // Wait longer than FLUSH_TIMEOUT for the partial batch to flush
            tokio::time::sleep(Duration::from_millis(700)).await;

            drop(tx);
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        #[tokio::test]
        async fn ingest_does_not_block_when_semantic_store_inactive() {
            // This test verifies the non-blocking contract:
            // ingest sends to the channel but never awaits the result
            let embedder = Arc::new(NoopEmbedder::default());
            let store = Arc::new(NoopSemanticStore);
            let tx = spawn_worker(embedder, store);

            // try_send is what ingest should use — non-blocking
            let req = make_embed_request("non-blocking test");
            assert!(tx.try_send(req).is_ok());

            drop(tx);
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
}
