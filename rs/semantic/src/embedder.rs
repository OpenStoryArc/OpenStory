//! Embedding generation — Embedder trait with ONNX and Noop implementations.
//!
//! `OnnxEmbedder` loads all-MiniLM-L6-v2 at startup (~200MB RAM) and produces
//! 384-dimensional normalized vectors. `NoopEmbedder` returns zero vectors for tests.

use anyhow::Result;

/// Dimension of all-MiniLM-L6-v2 embeddings.
pub const EMBEDDING_DIM: usize = 384;

/// Embedding generation trait — sync because ONNX inference is CPU-bound.
pub trait Embedder: Send + Sync {
    /// Generate an embedding vector for the given text.
    fn embed(&self, text: &str) -> Result<Vec<f32>>;

    /// Generate embeddings for a batch of texts.
    fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        texts.iter().map(|t| self.embed(t)).collect()
    }

    /// The dimensionality of the embedding vectors.
    fn dimension(&self) -> usize;
}

// ---------------------------------------------------------------------------
// NoopEmbedder — zero vectors for tests
// ---------------------------------------------------------------------------

/// Returns zero vectors of the configured dimension. For tests that don't need real embeddings.
pub struct NoopEmbedder {
    dim: usize,
}

impl NoopEmbedder {
    pub fn new(dim: usize) -> Self {
        Self { dim }
    }
}

impl Default for NoopEmbedder {
    fn default() -> Self {
        Self { dim: EMBEDDING_DIM }
    }
}

impl Embedder for NoopEmbedder {
    fn embed(&self, _text: &str) -> Result<Vec<f32>> {
        Ok(vec![0.0; self.dim])
    }

    fn dimension(&self) -> usize {
        self.dim
    }
}

// ---------------------------------------------------------------------------
// OnnxEmbedder — real embeddings via ONNX Runtime
// ---------------------------------------------------------------------------

#[cfg(feature = "onnx")]
mod onnx_impl {
    use super::*;
    use std::sync::Mutex;
    use ort::session::Session;
    use tokenizers::Tokenizer;

    /// Produces 384-dimensional normalized embeddings using all-MiniLM-L6-v2.
    ///
    /// Loads the ONNX model and HuggingFace tokenizer at construction time.
    /// Inference is CPU-bound (~5ms/sentence on modern hardware).
    /// Session wrapped in Mutex because ort v2 run() takes &mut self.
    pub struct OnnxEmbedder {
        session: Mutex<Session>,
        tokenizer: Tokenizer,
    }

    impl OnnxEmbedder {
        /// Load model and tokenizer from the given directory.
        ///
        /// Expects `model.onnx` and `tokenizer.json` in the directory.
        pub fn new(model_dir: &std::path::Path) -> Result<Self> {
            let model_path = model_dir.join("model.onnx");
            let tokenizer_path = model_dir.join("tokenizer.json");

            anyhow::ensure!(
                model_path.exists(),
                "ONNX model not found: {}. Run `uv run python scripts/download_model.py`",
                model_path.display()
            );
            anyhow::ensure!(
                tokenizer_path.exists(),
                "Tokenizer not found: {}. Run `uv run python scripts/download_model.py`",
                tokenizer_path.display()
            );

            let session = Session::builder()
                .map_err(|e| anyhow::anyhow!("ONNX session builder error: {e}"))?
                .with_intra_threads(1)
                .map_err(|e| anyhow::anyhow!("ONNX thread config error: {e}"))?
                .commit_from_file(&model_path)
                .map_err(|e| anyhow::anyhow!("ONNX model load error: {e}"))?;

            let tokenizer = Tokenizer::from_file(&tokenizer_path)
                .map_err(|e| anyhow::anyhow!("Failed to load tokenizer: {e}"))?;

            Ok(Self { session: Mutex::new(session), tokenizer })
        }
    }

    impl Embedder for OnnxEmbedder {
        fn embed(&self, text: &str) -> Result<Vec<f32>> {
            let encoding = self
                .tokenizer
                .encode(text, true)
                .map_err(|e| anyhow::anyhow!("Tokenization failed: {e}"))?;

            let ids = encoding.get_ids();
            let attention_mask = encoding.get_attention_mask();
            let token_type_ids = encoding.get_type_ids();
            let seq_len = ids.len();

            // Create input tensors [1, seq_len]
            let input_ids_data: Vec<i64> = ids.iter().map(|&x| x as i64).collect();
            let attention_data: Vec<i64> = attention_mask.iter().map(|&x| x as i64).collect();
            let type_ids_data: Vec<i64> = token_type_ids.iter().map(|&x| x as i64).collect();

            let input_ids_tensor = ort::value::Tensor::from_array(
                ([1usize, seq_len], input_ids_data.into_boxed_slice()),
            ).map_err(|e| anyhow::anyhow!("Tensor creation error: {e}"))?;
            let attention_tensor = ort::value::Tensor::from_array(
                ([1usize, seq_len], attention_data.into_boxed_slice()),
            ).map_err(|e| anyhow::anyhow!("Tensor creation error: {e}"))?;
            let type_ids_tensor = ort::value::Tensor::from_array(
                ([1usize, seq_len], type_ids_data.into_boxed_slice()),
            ).map_err(|e| anyhow::anyhow!("Tensor creation error: {e}"))?;

            // Run inference
            let mut session = self.session.lock()
                .map_err(|e| anyhow::anyhow!("Session lock poisoned: {e}"))?;
            let outputs = session.run(ort::inputs![
                input_ids_tensor,
                attention_tensor,
                type_ids_tensor,
            ]).map_err(|e| anyhow::anyhow!("ONNX inference error: {e}"))?;

            // Extract token embeddings from first output
            // Output shape: [1, seq_len, 384]
            let token_embeddings = outputs[0].try_extract_array::<f32>()
                .map_err(|e| anyhow::anyhow!("Tensor extraction error: {e}"))?;
            let shape = token_embeddings.shape();
            let embedding_dim = shape.last().copied().unwrap_or(EMBEDDING_DIM);
            // Flatten to a raw slice for indexed access: [1 * seq_len * dim]
            let raw_data = token_embeddings.as_slice()
                .ok_or_else(|| anyhow::anyhow!("Non-contiguous tensor output"))?;

            // Mean pooling: sum(embeddings * attention_mask) / sum(attention_mask)
            let mut pooled = vec![0.0f32; embedding_dim];
            let mask_sum: f32 = attention_mask.iter().map(|&x| x as f32).sum();

            if mask_sum > 0.0 {
                for (tok_idx, &mask_val) in attention_mask.iter().enumerate() {
                    if mask_val > 0 {
                        let offset = tok_idx * embedding_dim;
                        for dim_idx in 0..embedding_dim {
                            pooled[dim_idx] += raw_data[offset + dim_idx];
                        }
                    }
                }
                for val in &mut pooled {
                    *val /= mask_sum;
                }
            }

            // L2 normalize
            let norm: f32 = pooled.iter().map(|x| x * x).sum::<f32>().sqrt();
            if norm > 0.0 {
                for val in &mut pooled {
                    *val /= norm;
                }
            }

            Ok(pooled)
        }

        fn dimension(&self) -> usize {
            EMBEDDING_DIM
        }
    }
}

#[cfg(feature = "onnx")]
pub use onnx_impl::OnnxEmbedder;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // describe("NoopEmbedder")
    mod noop_embedder {
        use super::*;

        #[test]
        fn returns_zero_vector_of_configured_dimension() {
            let embedder = NoopEmbedder::new(384);
            let embedding = embedder.embed("hello world").unwrap();
            assert_eq!(embedding.len(), 384);
            assert!(embedding.iter().all(|&v| v == 0.0));
        }

        #[test]
        fn default_dimension_is_384() {
            let embedder = NoopEmbedder::default();
            assert_eq!(embedder.dimension(), 384);
            let embedding = embedder.embed("test").unwrap();
            assert_eq!(embedding.len(), 384);
        }

        #[test]
        fn custom_dimension() {
            let embedder = NoopEmbedder::new(128);
            assert_eq!(embedder.dimension(), 128);
            let embedding = embedder.embed("test").unwrap();
            assert_eq!(embedding.len(), 128);
        }

        #[test]
        fn handles_empty_text() {
            let embedder = NoopEmbedder::default();
            let embedding = embedder.embed("").unwrap();
            assert_eq!(embedding.len(), 384);
        }

        #[test]
        fn embed_batch_returns_correct_count() {
            let embedder = NoopEmbedder::default();
            let texts = vec!["hello", "world", "foo"];
            let embeddings = embedder.embed_batch(&texts).unwrap();
            assert_eq!(embeddings.len(), 3);
            for emb in &embeddings {
                assert_eq!(emb.len(), 384);
            }
        }
    }

    // describe("Embedder trait")
    mod embedder_trait {
        use super::*;

        #[test]
        fn noop_embedder_is_send_sync() {
            fn assert_send_sync<T: Send + Sync>() {}
            assert_send_sync::<NoopEmbedder>();
        }
    }
}
