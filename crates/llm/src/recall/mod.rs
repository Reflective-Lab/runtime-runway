// Copyright 2024-2026 Reflective Labs

//! Semantic recall for decision chains.
//!
//! This module provides semantic similarity search as a Provider for the
//! Converge decision system. It enables finding similar past failures,
//! runbooks, and adapter configurations.
//!
//! # Design Principles
//!
//! 1. **Recall is a Provider** - Separate, replaceable implementation
//! 2. **Deterministic/Replayable** - Full embedding hash via blake3
//! 3. **Process Control Focus** - Find similar failures, runbooks, adapters
//! 4. **Recall ≠ Evidence** - Structurally enforced separation
//! 5. **Explicit Recall Only** - Requires config + policy + step request
//!
//! # Usage
//!
//! ```ignore
//! use converge_llm::recall::{RecallConfig, RecallPolicy, HashEmbedder};
//!
//! // Create config with recall enabled
//! let recall_config = RecallConfig {
//!     policy: RecallPolicy::enabled(),
//!     per_step: RecallPerStep::default(),
//! };
//!
//! // Use hash embedder for deterministic tests
//! let embedder = HashEmbedder::new(384);
//! let result = embedder.embed("deployment failure")?;
//!
//! // Embedding hash enables reproducibility
//! println!("Hash: {}", result.embedding_hash());
//! ```

mod corpus;
mod embedder;
mod embedder_hash;
mod normalizer;
mod pii;
mod provider;
mod types;

// Semantic embedder (requires fastembed)
#[cfg(feature = "semantic-embedding")]
mod embedder_semantic;

// Persistent provider (file-based storage)
#[cfg(feature = "persistent-recall")]
mod provider_persistent;

// Core types (always available)
pub use corpus::{CorpusFingerprint, CorpusFingerprintBuilder, TenantPolicy};
pub use embedder::{
    // Determinism contract types
    DeterminismContract,
    DeterminismLevel,
    DeterminismVerdict,
    // Embedder trait and result types
    Embedder,
    EmbedderSettings,
    EmbeddingResult,
};
pub use embedder_hash::HashEmbedder;

// Semantic embedder (requires semantic-embedding feature)
#[cfg(feature = "semantic-embedding")]
pub use embedder_semantic::SemanticEmbedder;

// Persistent provider (requires persistent-recall feature)
pub use normalizer::{NormalizedRecall, RawRecallResult, RecallNormalizer};
pub use pii::{
    canonicalize_for_embedding, contains_pii, count_pii_patterns, embedding_input_hash, redact_pii,
};
pub use provider::{MockRecallProvider, RecallProvider, RecallResponse};
#[cfg(feature = "persistent-recall")]
pub use provider_persistent::{CorpusMetadata, PersistentRecallProvider, RecallFilter};
pub use types::{
    CandidateScore,
    CandidateSourceType,
    DecisionOutcome,
    DecisionRecord,
    RecallBudgets,
    RecallCandidate,
    RecallConfig,
    RecallConsumer,
    RecallContext,
    RecallHint,
    RecallPerStep,
    RecallPolicy,
    RecallProvenanceEnvelope,
    RecallQuery,
    RecallTraceLink,
    RecallTrigger,
    // Recall Use/Consumer types (Recall ≠ Training boundary)
    RecallUse,
    StopReason,
    recall_use_allowed,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_embedder_determinism() {
        let embedder = HashEmbedder::new(384);
        let v1 = embedder.embed("test query").unwrap();
        let v2 = embedder.embed("test query").unwrap();
        assert_eq!(v1.vector, v2.vector);
        assert_eq!(v1.embedding_hash(), v2.embedding_hash());
    }

    #[test]
    fn test_hash_embedder_different_queries_different_vectors() {
        let embedder = HashEmbedder::new(384);
        let v1 = embedder.embed("query one").unwrap();
        let v2 = embedder.embed("query two").unwrap();
        assert_ne!(v1.vector, v2.vector);
    }

    #[test]
    fn test_corpus_fingerprint_equality() {
        let embedder = HashEmbedder::new(384);
        let fp1 = CorpusFingerprint::new("v1", &embedder, "abc123", None);
        let fp2 = CorpusFingerprint::new("v1", &embedder, "abc123", None);
        assert_eq!(fp1, fp2);
    }

    #[test]
    fn test_pii_redaction_stable() {
        let input = "Contact john@example.com or call +1-555-123-4567";
        let output1 = redact_pii(input);
        let output2 = redact_pii(input);
        assert_eq!(output1, output2);
        assert_eq!(output1, "Contact <EMAIL> or call <PHONE>");
    }

    #[test]
    fn test_recall_config_serialization() {
        let config = RecallConfig::default();
        let json = serde_json::to_string(&config).unwrap();
        let parsed: RecallConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(config.policy.enabled, parsed.policy.enabled);
    }
}
