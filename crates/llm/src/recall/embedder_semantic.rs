// Copyright 2024-2026 Reflective Labs

//! Production-ready semantic embedder using fastembed.
//!
//! This module provides a real embedding implementation using the fastembed
//! library, which runs ONNX models locally for generating vector embeddings.
//!
//! # Features
//!
//! - Uses ONNX runtime for efficient inference
//! - Supports multiple embedding models
//! - Downloads models on first use (cached locally)
//! - Deterministic output for reproducibility
//!
//! # Usage
//!
//! Enable the `semantic-embedding` feature in Cargo.toml:
//!
//! ```toml
//! converge-llm = { version = "0.1", features = ["semantic-embedding"] }
//! ```
//!
//! Then create an embedder:
//!
//! ```ignore
//! use converge_llm::recall::SemanticEmbedder;
//!
//! let embedder = SemanticEmbedder::new()?;
//! let result = embedder.embed("deployment failure due to memory")?;
//!
//! println!("Embedding dimensions: {}", result.vector.len());
//! println!("Embedding hash: {}", result.embedding_hash());
//! ```

use crate::error::{LlmError, LlmResult};
use crate::recall::{DeterminismContract, EmbedderSettings, EmbeddingResult};
use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

/// Semantic embedder using fastembed with ONNX models.
///
/// This embedder provides production-quality semantic embeddings using
/// pre-trained models. It downloads models on first use and caches them
/// locally for subsequent runs.
///
/// # Model Selection
///
/// By default, uses `AllMiniLML6V2` which provides a good balance of:
/// - Speed: ~14ms per embedding
/// - Quality: Strong semantic understanding
/// - Size: 22M parameters, 384 dimensions
///
/// For higher quality (at cost of speed), consider `BGESmallENV15`.
pub struct SemanticEmbedder {
    model: Arc<TextEmbedding>,
    model_id: String,
    dimensions: usize,
}

impl SemanticEmbedder {
    /// Create a new semantic embedder with the default model (AllMiniLML6V2).
    ///
    /// # Errors
    ///
    /// Returns an error if the model fails to load.
    pub fn new() -> LlmResult<Self> {
        Self::with_model(EmbeddingModel::AllMiniLML6V2)
    }

    /// Create a semantic embedder with a specific model.
    ///
    /// # Model Options
    ///
    /// - `EmbeddingModel::AllMiniLML6V2` - 384 dims, fastest, good quality
    /// - `EmbeddingModel::BGESmallENV15` - 384 dims, slower, higher quality
    /// - `EmbeddingModel::BGEBaseENV15` - 768 dims, slowest, highest quality
    ///
    /// # Errors
    ///
    /// Returns an error if the model fails to load.
    pub fn with_model(model: EmbeddingModel) -> LlmResult<Self> {
        let model_id = format!("{:?}", model);

        tracing::info!("Loading semantic embedding model: {}", model_id);

        let text_embedding = TextEmbedding::try_new(InitOptions::new(model))
            .map_err(|e| LlmError::WeightLoadError(format!("Failed to load embedder: {}", e)))?;

        // Get dimensions from a test embedding
        let test_result = text_embedding.embed(vec!["test"], None).map_err(|e| {
            LlmError::InferenceError(format!("Failed to get embedding dimensions: {}", e))
        })?;

        let dimensions = test_result
            .first()
            .map(|v: &Vec<f32>| v.len())
            .ok_or_else(|| LlmError::InferenceError("Empty test embedding".to_string()))?;

        tracing::info!(
            "Semantic embedder loaded: {} ({} dimensions)",
            model_id,
            dimensions
        );

        Ok(Self {
            model: Arc::new(text_embedding),
            model_id,
            dimensions,
        })
    }

    /// Get the model identifier.
    #[must_use]
    pub fn model_id(&self) -> &str {
        &self.model_id
    }
}

impl super::Embedder for SemanticEmbedder {
    fn embedder_id(&self) -> &str {
        &self.model_id
    }

    fn dimensions(&self) -> usize {
        self.dimensions
    }

    fn determinism_contract(&self) -> DeterminismContract {
        // ONNX inference is deterministic on the same platform.
        // May have minor floating-point differences across different:
        // - CPU architectures (x86 vs ARM)
        // - SIMD instruction sets (AVX vs AVX2 vs AVX-512)
        // - Compiler optimizations
        //
        // We claim SamePlatform determinism - bit-exact on the same machine,
        // tolerance-based across different configurations.
        DeterminismContract::same_platform()
    }

    fn embed(&self, text: &str) -> LlmResult<EmbeddingResult> {
        let start = Instant::now();

        let embeddings = self
            .model
            .embed(vec![text], None)
            .map_err(|e| LlmError::InferenceError(format!("Embedding failed: {}", e)))?;

        let vector = embeddings
            .into_iter()
            .next()
            .ok_or_else(|| LlmError::InferenceError("No embedding returned".to_string()))?;

        let latency_ms = start.elapsed().as_millis() as u64;

        // Estimate token count (rough approximation)
        let token_count = (text.len() / 4).max(1);

        Ok(EmbeddingResult {
            vector,
            token_count,
            latency_ms,
        })
    }

    fn embed_batch(&self, texts: &[&str]) -> LlmResult<Vec<EmbeddingResult>> {
        let start = Instant::now();

        let texts_owned: Vec<String> = texts.iter().map(|&s| s.to_string()).collect();
        let embeddings = self
            .model
            .embed(texts_owned, None)
            .map_err(|e| LlmError::InferenceError(format!("Batch embedding failed: {}", e)))?;

        let total_latency_ms = start.elapsed().as_millis() as u64;
        let per_text_latency = if texts.is_empty() {
            0
        } else {
            total_latency_ms / texts.len() as u64
        };

        let results = texts
            .iter()
            .zip(embeddings)
            .map(|(text, vector)| {
                let token_count = (text.len() / 4).max(1);
                EmbeddingResult {
                    vector,
                    token_count,
                    latency_ms: per_text_latency,
                }
            })
            .collect();

        Ok(results)
    }

    fn settings_snapshot(&self) -> EmbedderSettings {
        let mut extra = HashMap::new();
        extra.insert("runtime".to_string(), "onnx".to_string());

        EmbedderSettings {
            model_id: self.model_id.clone(),
            model_revision: None, // fastembed uses pinned versions
            normalize: true,      // fastembed normalizes by default
            max_input_length: 512,
            extra,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recall::Embedder;

    #[test]
    #[ignore = "requires model download"]
    fn test_semantic_embedder_creation() {
        let embedder = SemanticEmbedder::new().expect("Failed to create embedder");
        assert_eq!(embedder.dimensions(), 384);
        assert!(embedder.is_deterministic());
    }

    #[test]
    #[ignore = "requires model download"]
    fn test_semantic_embedding() {
        let embedder = SemanticEmbedder::new().expect("Failed to create embedder");
        let result = embedder
            .embed("deployment failure due to memory")
            .expect("Failed to embed");

        assert_eq!(result.vector.len(), 384);
        assert!(result.is_normalized());
        assert!(result.latency_ms > 0);
    }

    #[test]
    #[ignore = "requires model download"]
    fn test_semantic_similarity() {
        let embedder = SemanticEmbedder::new().expect("Failed to create embedder");

        let result1 = embedder
            .embed("deployment failure due to memory")
            .expect("Failed to embed");
        let result2 = embedder
            .embed("deployment failed because of memory issues")
            .expect("Failed to embed");
        let result3 = embedder
            .embed("the weather is sunny today")
            .expect("Failed to embed");

        // Similar texts should have high similarity
        let sim_similar = result1.cosine_similarity(&result2);
        // Unrelated texts should have low similarity
        let sim_unrelated = result1.cosine_similarity(&result3);

        assert!(
            sim_similar > 0.7,
            "Similar texts should have high similarity: {}",
            sim_similar
        );
        assert!(
            sim_unrelated < 0.5,
            "Unrelated texts should have low similarity: {}",
            sim_unrelated
        );
    }

    #[test]
    #[ignore = "requires model download"]
    fn test_semantic_batch_embedding() {
        let embedder = SemanticEmbedder::new().expect("Failed to create embedder");

        let texts = [
            "deployment failure",
            "memory issues",
            "successful deployment",
        ];

        let results = embedder.embed_batch(&texts).expect("Failed to batch embed");

        assert_eq!(results.len(), 3);
        for result in &results {
            assert_eq!(result.vector.len(), 384);
            assert!(result.is_normalized());
        }
    }

    #[test]
    #[ignore = "requires model download"]
    fn test_semantic_determinism() {
        let embedder = SemanticEmbedder::new().expect("Failed to create embedder");

        let text = "test text for determinism";
        let result1 = embedder.embed(text).expect("Failed to embed");
        let result2 = embedder.embed(text).expect("Failed to embed");

        // Same text should produce identical embeddings
        assert_eq!(result1.embedding_hash(), result2.embedding_hash());
    }
}
