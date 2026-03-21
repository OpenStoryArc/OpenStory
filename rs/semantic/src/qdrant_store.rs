//! QdrantStore — SemanticStore implementation backed by Qdrant vector database.
//!
//! Requires the `qdrant` feature flag. Uses gRPC via `qdrant-client`.

#[cfg(feature = "qdrant")]
mod qdrant_impl {
    use anyhow::Result;
    use async_trait::async_trait;

    use qdrant_client::qdrant::{
        Condition, CreateCollectionBuilder, DeletePointsBuilder, Distance, Filter,
        PointStruct, SearchPointsBuilder, UpsertPointsBuilder, VectorParamsBuilder,
    };
    use qdrant_client::Qdrant;

    use crate::{ChunkMetadata, EmbeddingChunk, SearchResult, SemanticStore};

    /// Collection name in Qdrant.
    const COLLECTION_NAME: &str = "open_story_events";

    /// QdrantStore — connects to a Qdrant instance for vector search.
    pub struct QdrantStore {
        client: Qdrant,
    }

    impl QdrantStore {
        /// Connect to Qdrant and ensure the collection exists.
        pub async fn new(url: &str, dimension: u64) -> Result<Self> {
            let client = Qdrant::from_url(url).build()?;

            // Create collection if it doesn't exist
            let collections = client.list_collections().await?;
            let exists = collections
                .collections
                .iter()
                .any(|c| c.name == COLLECTION_NAME);

            if !exists {
                client
                    .create_collection(
                        CreateCollectionBuilder::new(COLLECTION_NAME)
                            .vectors_config(VectorParamsBuilder::new(dimension, Distance::Cosine)),
                    )
                    .await?;
            }

            Ok(Self { client })
        }
    }

    #[async_trait]
    impl SemanticStore for QdrantStore {
        async fn upsert(&self, chunks: &[EmbeddingChunk]) -> Result<()> {
            if chunks.is_empty() {
                return Ok(());
            }

            let points: Vec<PointStruct> = chunks
                .iter()
                .map(|chunk| {
                    let payload = serde_json::json!({
                        "session_id": chunk.session_id,
                        "event_id": chunk.event_id,
                        "text": chunk.text,
                        "record_type": chunk.metadata.record_type,
                        "timestamp": chunk.metadata.timestamp,
                        "tool_name": chunk.metadata.tool_name,
                        "session_label": chunk.metadata.session_label,
                    });

                    // Use a deterministic point ID from the chunk ID hash
                    let point_id = uuid_from_chunk_id(&chunk.id);

                    PointStruct::new(
                        point_id,
                        chunk.embedding.clone(),
                        payload.as_object().unwrap().clone().into_iter().map(|(k, v)| {
                            (k, qdrant_client::qdrant::Value::from(v.to_string()))
                        }).collect::<std::collections::HashMap<_, _>>(),
                    )
                })
                .collect();

            self.client
                .upsert_points(UpsertPointsBuilder::new(COLLECTION_NAME, points))
                .await?;

            Ok(())
        }

        async fn search(
            &self,
            query_embedding: &[f32],
            limit: usize,
            session_filter: Option<&str>,
        ) -> Result<Vec<SearchResult>> {
            let mut builder = SearchPointsBuilder::new(
                COLLECTION_NAME,
                query_embedding.to_vec(),
                limit as u64,
            )
            .with_payload(true);

            if let Some(sid) = session_filter {
                builder = builder.filter(Filter::must([Condition::matches(
                    "session_id",
                    sid.to_string(),
                )]));
            }

            let results = self.client.search_points(builder).await?;

            Ok(results
                .result
                .into_iter()
                .map(|point| {
                    let payload = &point.payload;
                    let get_str = |key: &str| -> String {
                        payload
                            .get(key)
                            .and_then(|v| v.as_str())
                            .map(|s| s.trim_matches('"').to_string())
                            .unwrap_or_default()
                    };

                    SearchResult {
                        event_id: get_str("event_id"),
                        session_id: get_str("session_id"),
                        score: point.score,
                        text_snippet: {
                            let text = get_str("text");
                            if text.len() > 200 {
                                format!("{}...", &text[..200])
                            } else {
                                text
                            }
                        },
                        metadata: ChunkMetadata {
                            record_type: get_str("record_type"),
                            timestamp: get_str("timestamp"),
                            tool_name: {
                                let name = get_str("tool_name");
                                if name.is_empty() || name == "null" {
                                    None
                                } else {
                                    Some(name)
                                }
                            },
                            session_label: {
                                let label = get_str("session_label");
                                if label.is_empty() || label == "null" {
                                    None
                                } else {
                                    Some(label)
                                }
                            },
                        },
                    }
                })
                .collect())
        }

        async fn delete_session(&self, session_id: &str) -> Result<()> {
            self.client
                .delete_points(
                    DeletePointsBuilder::new(COLLECTION_NAME)
                        .points(Filter::must([Condition::matches(
                            "session_id",
                            session_id.to_string(),
                        )])),
                )
                .await?;

            Ok(())
        }

        fn is_active(&self) -> bool {
            true
        }
    }

    /// Generate a deterministic UUID from a chunk ID string.
    fn uuid_from_chunk_id(chunk_id: &str) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        chunk_id.hash(&mut hasher);
        let hash = hasher.finish();

        // Format as UUID-like string using the hash
        format!(
            "{:08x}-{:04x}-{:04x}-{:04x}-{:012x}",
            (hash >> 32) as u32,
            (hash >> 16) as u16 & 0xffff,
            hash as u16 & 0xffff,
            ((hash >> 48) as u16) & 0x0fff | 0x4000,
            hash & 0xffffffffffff
        )
    }
}

#[cfg(feature = "qdrant")]
pub use qdrant_impl::QdrantStore;
