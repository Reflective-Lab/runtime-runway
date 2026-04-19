// Copyright 2024-2026 Reflective Labs

//! Hash-based embedder for deterministic testing.
//!
//! This embedder creates stable pseudo-vectors from text hashes,
//! enabling reproducible tests without neural network dependencies.

use super::embedder::{DeterminismContract, Embedder, EmbedderSettings, EmbeddingResult};
use crate::error::LlmResult;
use blake3::Hasher;
use std::collections::HashMap;

/// Deterministic test embedder using blake3 hashing.
///
/// Same query → same embedding → same candidates (testable).
///
/// # Properties
///
/// - Deterministic: Same input always produces same output
/// - Fast: No neural network inference required
/// - Consistent: Useful for testing recall logic
///
/// # Example
///
/// ```ignore
/// let embedder = HashEmbedder::new(384);
/// let result = embedder.embed("deployment failure")?;
///
/// // Same input always gives same output
/// let result2 = embedder.embed("deployment failure")?;
/// assert_eq!(result.vector, result2.vector);
/// ```
#[derive(Debug, Clone)]
pub struct HashEmbedder {
    dimensions: usize,
}

impl HashEmbedder {
    /// Create a new hash embedder with the specified dimensions.
    #[must_use]
    pub fn new(dimensions: usize) -> Self {
        Self { dimensions }
    }

    /// Create a hash embedder with standard 384 dimensions.
    #[must_use]
    pub fn standard() -> Self {
        Self::new(384)
    }
}

impl Embedder for HashEmbedder {
    fn embedder_id(&self) -> &str {
        "hash-embedder-test"
    }

    fn dimensions(&self) -> usize {
        self.dimensions
    }

    fn determinism_contract(&self) -> DeterminismContract {
        // HashEmbedder is fully deterministic - same input always produces
        // bit-exact identical output regardless of platform
        DeterminismContract::bit_exact()
    }

    fn embed(&self, text: &str) -> LlmResult<EmbeddingResult> {
        let hash = blake3::hash(text.as_bytes());
        let hash_bytes = hash.as_bytes();

        // Expand hash to desired dimensions using HKDF-like expansion
        let mut vector = Vec::with_capacity(self.dimensions);
        let mut hasher = Hasher::new();

        for i in 0..self.dimensions {
            hasher.update(hash_bytes);
            hasher.update(&(i as u32).to_le_bytes());
            let chunk_hash = hasher.finalize();
            hasher.reset();

            // Convert first 4 bytes to f32 in [-1, 1]
            let bytes: [u8; 4] = chunk_hash.as_bytes()[0..4].try_into().unwrap();
            let u = u32::from_le_bytes(bytes);
            let f = (u as f64 / u32::MAX as f64) * 2.0 - 1.0;
            vector.push(f as f32);
        }

        // Normalize to unit length
        let norm: f32 = vector.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 0.0 {
            for v in &mut vector {
                *v /= norm;
            }
        }

        Ok(EmbeddingResult {
            vector,
            token_count: text.split_whitespace().count(),
            latency_ms: 0,
        })
    }

    fn settings_snapshot(&self) -> EmbedderSettings {
        EmbedderSettings {
            model_id: "hash-embedder-test".to_string(),
            model_revision: Some("1.0".to_string()),
            normalize: true,
            max_input_length: 8192,
            extra: HashMap::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_embedder_dimensions() {
        let embedder = HashEmbedder::new(384);
        let result = embedder.embed("test").unwrap();
        assert_eq!(result.vector.len(), 384);
    }

    #[test]
    fn test_hash_embedder_deterministic() {
        let embedder = HashEmbedder::new(128);
        let r1 = embedder.embed("hello world").unwrap();
        let r2 = embedder.embed("hello world").unwrap();
        assert_eq!(r1.vector, r2.vector);
    }

    #[test]
    fn test_hash_embedder_different_inputs() {
        let embedder = HashEmbedder::new(128);
        let r1 = embedder.embed("hello").unwrap();
        let r2 = embedder.embed("world").unwrap();
        assert_ne!(r1.vector, r2.vector);
    }

    #[test]
    fn test_hash_embedder_normalized() {
        let embedder = HashEmbedder::new(384);
        let result = embedder.embed("test query").unwrap();
        let norm: f32 = result.vector.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-5);
    }

    #[test]
    fn test_hash_embedder_token_count() {
        let embedder = HashEmbedder::new(128);
        let result = embedder.embed("one two three four five").unwrap();
        assert_eq!(result.token_count, 5);
    }

    #[test]
    fn test_hash_embedder_is_deterministic_flag() {
        let embedder = HashEmbedder::new(128);
        assert!(embedder.is_deterministic());
    }

    #[test]
    fn test_hash_embedder_batch() {
        let embedder = HashEmbedder::new(128);
        let results = embedder.embed_batch(&["one", "two", "three"]).unwrap();
        assert_eq!(results.len(), 3);
        assert_ne!(results[0].vector, results[1].vector);
    }

    #[test]
    fn test_similar_text_has_some_similarity() {
        let embedder = HashEmbedder::new(128);
        // Hash embedder won't produce semantically similar embeddings,
        // but this tests that the cosine similarity code works
        let r1 = embedder.embed("deployment failure").unwrap();
        let r2 = embedder.embed("deployment failure").unwrap();
        let r3 = embedder.embed("something else").unwrap();

        let sim_same = r1.cosine_similarity(&r2);
        let sim_diff = r1.cosine_similarity(&r3);

        // Same text should have similarity 1.0
        assert!((sim_same - 1.0).abs() < 1e-5);
        // Different text should have lower similarity
        assert!(sim_diff < sim_same);
    }

    #[test]
    fn test_embedding_hash_consistency() {
        let embedder = HashEmbedder::new(128);
        let r1 = embedder.embed("test").unwrap();
        let r2 = embedder.embed("test").unwrap();

        assert_eq!(r1.embedding_hash(), r2.embedding_hash());
    }
}
