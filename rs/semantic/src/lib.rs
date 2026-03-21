//! Semantic search for open-story — trait, text extraction, and embedding.
//!
//! `SemanticStore` defines the search boundary (like `Bus` for event transport).
//! `NoopSemanticStore` silently does nothing when Qdrant is unavailable.
//! `extract` module provides pure text extraction from ViewRecords.
//! `embedder` module provides the Embedder trait with ONNX and Noop implementations.
//! `worker` module runs background embedding as a tokio task.
//! `backfill` module batch-embeds existing events.

pub mod backfill;
pub mod embedder;
pub mod extract;
pub mod qdrant_store;
pub mod worker;

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A chunk of text with its embedding vector, ready for storage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingChunk {
    /// Unique ID: event_id or event_id:chunk_idx for multi-chunk records.
    pub id: String,
    pub session_id: String,
    pub event_id: String,
    /// The text that was embedded.
    pub text: String,
    /// The embedding vector (384 dims for all-MiniLM-L6-v2).
    pub embedding: Vec<f32>,
    pub metadata: ChunkMetadata,
}

/// Metadata stored alongside each embedding for filtering and display.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChunkMetadata {
    pub record_type: String,
    pub timestamp: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_label: Option<String>,
}

/// A search result returned from the semantic store.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub event_id: String,
    pub session_id: String,
    pub score: f32,
    pub text_snippet: String,
    pub metadata: ChunkMetadata,
}

// ---------------------------------------------------------------------------
// SemanticStore trait
// ---------------------------------------------------------------------------

/// The semantic search boundary — analogous to `Bus` for event transport.
///
/// Implementations:
/// - `NoopSemanticStore` — silent no-op (default when Qdrant unavailable)
/// - `QdrantStore` — real vector search (Phase 4)
#[async_trait]
pub trait SemanticStore: Send + Sync + 'static {
    /// Insert or update embedding chunks. Upsert semantics (idempotent by chunk ID).
    async fn upsert(&self, chunks: &[EmbeddingChunk]) -> Result<()>;

    /// Search for similar vectors. Returns results ranked by score (descending).
    /// Optional session_filter restricts results to a single session.
    async fn search(
        &self,
        query_embedding: &[f32],
        limit: usize,
        session_filter: Option<&str>,
    ) -> Result<Vec<SearchResult>>;

    /// Delete all vectors for a session (e.g., when session is deleted).
    async fn delete_session(&self, session_id: &str) -> Result<()>;

    /// Whether this store is active (connected to real backend).
    /// Returns false for NoopSemanticStore.
    fn is_active(&self) -> bool;
}

// ---------------------------------------------------------------------------
// NoopSemanticStore
// ---------------------------------------------------------------------------

/// Silent no-op implementation — used when Qdrant is not configured.
/// Same pattern as `NoopBus`.
pub struct NoopSemanticStore;

#[async_trait]
impl SemanticStore for NoopSemanticStore {
    async fn upsert(&self, _chunks: &[EmbeddingChunk]) -> Result<()> {
        Ok(())
    }

    async fn search(
        &self,
        _query_embedding: &[f32],
        _limit: usize,
        _session_filter: Option<&str>,
    ) -> Result<Vec<SearchResult>> {
        Ok(vec![])
    }

    async fn delete_session(&self, _session_id: &str) -> Result<()> {
        Ok(())
    }

    fn is_active(&self) -> bool {
        false
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // describe("NoopSemanticStore")
    mod noop_semantic_store {
        use super::*;

        #[tokio::test]
        async fn upsert_succeeds_silently() {
            let store = NoopSemanticStore;
            let chunk = EmbeddingChunk {
                id: "evt-1".into(),
                session_id: "sess-1".into(),
                event_id: "evt-1".into(),
                text: "hello".into(),
                embedding: vec![0.1; 384],
                metadata: ChunkMetadata {
                    record_type: "user_message".into(),
                    timestamp: "2025-01-17T00:00:00Z".into(),
                    tool_name: None,
                    session_label: None,
                },
            };
            let result = store.upsert(&[chunk]).await;
            assert!(result.is_ok());
        }

        #[tokio::test]
        async fn search_returns_empty_vec() {
            let store = NoopSemanticStore;
            let results = store.search(&[0.1; 384], 10, None).await.unwrap();
            assert!(results.is_empty());
        }

        #[tokio::test]
        async fn search_with_session_filter_returns_empty_vec() {
            let store = NoopSemanticStore;
            let results = store.search(&[0.1; 384], 10, Some("sess-1")).await.unwrap();
            assert!(results.is_empty());
        }

        #[tokio::test]
        async fn delete_session_succeeds_silently() {
            let store = NoopSemanticStore;
            let result = store.delete_session("sess-1").await;
            assert!(result.is_ok());
        }

        #[test]
        fn is_active_returns_false() {
            let store = NoopSemanticStore;
            assert!(!store.is_active());
        }
    }

    // describe("EmbeddingChunk serialization")
    mod embedding_chunk {
        use super::*;

        #[test]
        fn serializes_and_deserializes() {
            let chunk = EmbeddingChunk {
                id: "evt-1:0".into(),
                session_id: "sess-1".into(),
                event_id: "evt-1".into(),
                text: "fix the auth bug".into(),
                embedding: vec![0.5, -0.3, 0.1],
                metadata: ChunkMetadata {
                    record_type: "user_message".into(),
                    timestamp: "2025-01-17T00:00:00Z".into(),
                    tool_name: None,
                    session_label: Some("fix auth".into()),
                },
            };

            let json = serde_json::to_string(&chunk).unwrap();
            let deserialized: EmbeddingChunk = serde_json::from_str(&json).unwrap();

            assert_eq!(deserialized.id, "evt-1:0");
            assert_eq!(deserialized.session_id, "sess-1");
            assert_eq!(deserialized.event_id, "evt-1");
            assert_eq!(deserialized.text, "fix the auth bug");
            assert_eq!(deserialized.embedding, vec![0.5, -0.3, 0.1]);
            assert_eq!(deserialized.metadata.record_type, "user_message");
            assert_eq!(deserialized.metadata.session_label, Some("fix auth".into()));
        }

        #[test]
        fn metadata_skips_none_fields() {
            let meta = ChunkMetadata {
                record_type: "tool_call".into(),
                timestamp: "2025-01-17T00:00:00Z".into(),
                tool_name: Some("Bash".into()),
                session_label: None,
            };
            let json = serde_json::to_value(&meta).unwrap();
            assert_eq!(json["tool_name"], "Bash");
            assert!(json.get("session_label").is_none());
        }
    }

    // describe("SearchResult serialization")
    mod search_result {
        use super::*;

        #[test]
        fn serializes_and_deserializes() {
            let result = SearchResult {
                event_id: "evt-1".into(),
                session_id: "sess-1".into(),
                score: 0.95,
                text_snippet: "fix the auth bug".into(),
                metadata: ChunkMetadata {
                    record_type: "user_message".into(),
                    timestamp: "2025-01-17T00:00:00Z".into(),
                    tool_name: None,
                    session_label: None,
                },
            };

            let json = serde_json::to_string(&result).unwrap();
            let deserialized: SearchResult = serde_json::from_str(&json).unwrap();

            assert_eq!(deserialized.event_id, "evt-1");
            assert_eq!(deserialized.score, 0.95);
            assert_eq!(deserialized.text_snippet, "fix the auth bug");
        }
    }
}
