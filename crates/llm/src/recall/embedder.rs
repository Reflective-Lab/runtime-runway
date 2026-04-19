// Copyright 2024-2026 Reflective Labs

//! Embedder trait, embedding result types, and determinism contracts.
//!
//! # Determinism Contract
//!
//! Embedders must declare their determinism level. The levels are:
//!
//! | Level | Guarantee | Use Case |
//! |-------|-----------|----------|
//! | `BitExact` | Identical bytes on same platform | Local inference (Candle, ONNX) |
//! | `SamePlatform` | Identical within platform/config | Local with potential floating-point variance |
//! | `ToleranceBased` | Same ranking within ε | Remote APIs, different hardware |
//! | `AuditOnly` | No reproducibility guarantee | External APIs with no temp=0 |
//!
//! # Verification
//!
//! Use `DeterminismContract::verify()` to check if two embeddings meet the contract:
//!
//! ```ignore
//! let contract = embedder.determinism_contract();
//! let result1 = embedder.embed("test")?;
//! let result2 = embedder.embed("test")?;
//!
//! match contract.verify(&result1, &result2) {
//!     DeterminismVerdict::Passed => println!("Contract satisfied"),
//!     DeterminismVerdict::Failed { reason } => panic!("Contract violated: {}", reason),
//! }
//! ```

use crate::error::LlmResult;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ============================================================================
// Determinism Contract Types
// ============================================================================

/// Level of determinism guaranteed by an embedder.
///
/// This categorization helps users understand what reproducibility
/// guarantees they can expect from a given embedder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DeterminismLevel {
    /// Bit-exact identical output on the same platform.
    ///
    /// Requires: same binary, same hardware, same inputs.
    /// Provides: `embedding_hash(run1) == embedding_hash(run2)`
    BitExact,

    /// Identical output within the same platform configuration.
    ///
    /// May vary across: different SIMD extensions, compiler flags.
    /// Provides: bit-exact on same machine, tolerance on different configs.
    SamePlatform,

    /// Rankings preserved within tolerance threshold.
    ///
    /// Individual float values may vary, but:
    /// - Cosine similarity between embeddings stays within ε
    /// - Ranking order of candidates is preserved
    ToleranceBased,

    /// No determinism guarantee; for audit purposes only.
    ///
    /// Remote APIs without temperature control or with stochastic layers.
    /// Trace captures inputs/outputs for audit but cannot replay.
    AuditOnly,
}

impl Default for DeterminismLevel {
    fn default() -> Self {
        Self::AuditOnly
    }
}

impl std::fmt::Display for DeterminismLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BitExact => write!(f, "bit-exact"),
            Self::SamePlatform => write!(f, "same-platform"),
            Self::ToleranceBased => write!(f, "tolerance-based"),
            Self::AuditOnly => write!(f, "audit-only"),
        }
    }
}

/// Determinism contract for an embedder.
///
/// Formalizes what reproducibility guarantees an embedder provides
/// and how to verify them.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeterminismContract {
    /// The level of determinism claimed
    pub level: DeterminismLevel,

    /// Maximum allowed cosine distance for ToleranceBased level.
    /// Default: 1e-5 (effectively requiring near-identical vectors)
    pub cosine_tolerance: f32,

    /// Maximum allowed absolute difference per element.
    /// Default: 1e-6 for BitExact/SamePlatform
    pub element_tolerance: f32,

    /// Whether ranking order must be preserved.
    /// True for ToleranceBased (similarity ranking stays same)
    pub preserve_ranking: bool,

    /// Platform identifier for SamePlatform verification.
    /// Format: "arch-os-simd" (e.g., "x86_64-linux-avx2")
    pub platform_id: Option<String>,
}

impl Default for DeterminismContract {
    fn default() -> Self {
        Self {
            level: DeterminismLevel::AuditOnly,
            cosine_tolerance: 1e-5,
            element_tolerance: 1e-6,
            preserve_ranking: false,
            platform_id: None,
        }
    }
}

impl DeterminismContract {
    /// Create a contract for bit-exact determinism.
    #[must_use]
    pub fn bit_exact() -> Self {
        Self {
            level: DeterminismLevel::BitExact,
            cosine_tolerance: 0.0,
            element_tolerance: 0.0,
            preserve_ranking: true,
            platform_id: Some(Self::detect_platform()),
        }
    }

    /// Create a contract for same-platform determinism.
    #[must_use]
    pub fn same_platform() -> Self {
        Self {
            level: DeterminismLevel::SamePlatform,
            cosine_tolerance: 1e-7,
            element_tolerance: 1e-7,
            preserve_ranking: true,
            platform_id: Some(Self::detect_platform()),
        }
    }

    /// Create a contract for tolerance-based determinism.
    #[must_use]
    pub fn tolerance_based(cosine_tolerance: f32) -> Self {
        Self {
            level: DeterminismLevel::ToleranceBased,
            cosine_tolerance,
            element_tolerance: 1e-3, // Wider tolerance for individual elements
            preserve_ranking: true,
            platform_id: None,
        }
    }

    /// Create a contract for audit-only (no determinism guarantee).
    #[must_use]
    pub fn audit_only() -> Self {
        Self {
            level: DeterminismLevel::AuditOnly,
            cosine_tolerance: 1.0, // Accept any difference
            element_tolerance: 1.0,
            preserve_ranking: false,
            platform_id: None,
        }
    }

    /// Detect the current platform identifier.
    #[must_use]
    pub fn detect_platform() -> String {
        let arch = std::env::consts::ARCH;
        let os = std::env::consts::OS;
        // Note: SIMD detection would require runtime checks
        format!("{}-{}", arch, os)
    }

    /// Verify that two embeddings satisfy this contract.
    #[must_use]
    pub fn verify(&self, a: &EmbeddingResult, b: &EmbeddingResult) -> DeterminismVerdict {
        // Dimension mismatch is always a failure
        if a.vector.len() != b.vector.len() {
            return DeterminismVerdict::Failed {
                reason: format!(
                    "Dimension mismatch: {} vs {}",
                    a.vector.len(),
                    b.vector.len()
                ),
            };
        }

        match self.level {
            DeterminismLevel::BitExact => {
                // Require identical hashes
                if a.embedding_hash() == b.embedding_hash() {
                    DeterminismVerdict::Passed
                } else {
                    // Find first differing element for diagnostics
                    for (i, (x, y)) in a.vector.iter().zip(b.vector.iter()).enumerate() {
                        if x != y {
                            return DeterminismVerdict::Failed {
                                reason: format!(
                                    "Element {} differs: {} vs {} (hash mismatch)",
                                    i, x, y
                                ),
                            };
                        }
                    }
                    DeterminismVerdict::Failed {
                        reason: "Hash mismatch with identical elements (floating-point edge case)"
                            .to_string(),
                    }
                }
            }

            DeterminismLevel::SamePlatform => {
                // Check element-wise tolerance
                for (i, (x, y)) in a.vector.iter().zip(b.vector.iter()).enumerate() {
                    let diff = (x - y).abs();
                    if diff > self.element_tolerance {
                        return DeterminismVerdict::Failed {
                            reason: format!(
                                "Element {} exceeds tolerance: diff={} > {}",
                                i, diff, self.element_tolerance
                            ),
                        };
                    }
                }
                DeterminismVerdict::Passed
            }

            DeterminismLevel::ToleranceBased => {
                // Check cosine distance
                let cosine_sim = a.cosine_similarity(b);
                let cosine_dist = 1.0 - cosine_sim;
                if cosine_dist > self.cosine_tolerance {
                    return DeterminismVerdict::Failed {
                        reason: format!(
                            "Cosine distance {} exceeds tolerance {}",
                            cosine_dist, self.cosine_tolerance
                        ),
                    };
                }
                DeterminismVerdict::Passed
            }

            DeterminismLevel::AuditOnly => {
                // Always passes (no guarantee)
                DeterminismVerdict::Passed
            }
        }
    }

    /// Verify ranking preservation between two result sets.
    ///
    /// Returns true if the top-k ordering is preserved.
    #[must_use]
    pub fn verify_ranking(
        &self,
        query_a: &EmbeddingResult,
        candidates_a: &[EmbeddingResult],
        query_b: &EmbeddingResult,
        candidates_b: &[EmbeddingResult],
    ) -> bool {
        if !self.preserve_ranking {
            return true;
        }

        if candidates_a.len() != candidates_b.len() {
            return false;
        }

        // Compute rankings for both
        let mut scores_a: Vec<(usize, f32)> = candidates_a
            .iter()
            .enumerate()
            .map(|(i, c)| (i, query_a.cosine_similarity(c)))
            .collect();
        scores_a.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        let mut scores_b: Vec<(usize, f32)> = candidates_b
            .iter()
            .enumerate()
            .map(|(i, c)| (i, query_b.cosine_similarity(c)))
            .collect();
        scores_b.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        // Check if indices match (order preserved)
        scores_a
            .iter()
            .zip(scores_b.iter())
            .all(|(a, b)| a.0 == b.0)
    }
}

/// Result of verifying determinism contract compliance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeterminismVerdict {
    /// Contract requirements satisfied
    Passed,
    /// Contract requirements violated
    Failed {
        /// Description of what failed
        reason: String,
    },
}

impl DeterminismVerdict {
    /// Check if the verdict is a pass.
    #[must_use]
    pub fn is_passed(&self) -> bool {
        matches!(self, Self::Passed)
    }

    /// Check if the verdict is a failure.
    #[must_use]
    pub fn is_failed(&self) -> bool {
        matches!(self, Self::Failed { .. })
    }
}

// ============================================================================
// Embedder Trait
// ============================================================================

/// Trait for text embedding providers.
///
/// Implementations convert text to dense vector representations
/// for semantic similarity search.
///
/// # Determinism Contract
///
/// Every embedder must declare its determinism contract via
/// `determinism_contract()`. This enables the recall system to:
///
/// - Choose appropriate verification strategies
/// - Warn when replaying with different determinism guarantees
/// - Track which level of reproducibility was achieved
pub trait Embedder: Send + Sync {
    /// Get the unique identifier for this embedder.
    fn embedder_id(&self) -> &str;

    /// Get the number of dimensions in the output vectors.
    fn dimensions(&self) -> usize;

    /// Whether this embedder produces deterministic outputs.
    ///
    /// This is a convenience method; prefer using `determinism_contract()`
    /// for more detailed information.
    ///
    /// Returns true if `determinism_contract().level` is `BitExact` or `SamePlatform`.
    fn is_deterministic(&self) -> bool {
        matches!(
            self.determinism_contract().level,
            DeterminismLevel::BitExact | DeterminismLevel::SamePlatform
        )
    }

    /// Get the determinism contract for this embedder.
    ///
    /// The contract specifies what reproducibility guarantees the embedder
    /// provides and how to verify them.
    fn determinism_contract(&self) -> DeterminismContract;

    /// Embed a single text into a vector.
    fn embed(&self, text: &str) -> LlmResult<EmbeddingResult>;

    /// Embed multiple texts into vectors (batch operation).
    ///
    /// Default implementation calls `embed` for each text.
    /// Implementations may override for more efficient batching.
    fn embed_batch(&self, texts: &[&str]) -> LlmResult<Vec<EmbeddingResult>> {
        texts.iter().map(|t| self.embed(t)).collect()
    }

    /// Get a snapshot of the embedder settings.
    ///
    /// Used for corpus fingerprinting and reproducibility.
    fn settings_snapshot(&self) -> EmbedderSettings;

    /// Verify that a second embedding satisfies this embedder's determinism contract.
    ///
    /// Convenience method that uses the embedder's contract to verify.
    fn verify_determinism(&self, a: &EmbeddingResult, b: &EmbeddingResult) -> DeterminismVerdict {
        self.determinism_contract().verify(a, b)
    }
}

/// Result of embedding a text.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingResult {
    /// The embedding vector
    pub vector: Vec<f32>,
    /// Estimated token count of the input
    pub token_count: usize,
    /// Latency in milliseconds
    pub latency_ms: u64,
}

impl EmbeddingResult {
    /// Compute deterministic hash of the entire embedding vector.
    ///
    /// Uses blake3 for cryptographic-quality hashing.
    #[must_use]
    pub fn embedding_hash(&self) -> String {
        // Convert to little-endian bytes
        let bytes: Vec<u8> = self.vector.iter().flat_map(|f| f.to_le_bytes()).collect();

        let hash = blake3::hash(&bytes);
        hash.to_hex().to_string()
    }

    /// Get the L2 norm of the vector.
    #[must_use]
    pub fn norm(&self) -> f32 {
        self.vector.iter().map(|x| x * x).sum::<f32>().sqrt()
    }

    /// Check if the vector is normalized (unit length).
    #[must_use]
    pub fn is_normalized(&self) -> bool {
        let norm = self.norm();
        (norm - 1.0).abs() < 1e-5
    }

    /// Compute cosine similarity with another embedding.
    #[must_use]
    pub fn cosine_similarity(&self, other: &EmbeddingResult) -> f32 {
        if self.vector.len() != other.vector.len() {
            return 0.0;
        }

        let dot: f32 = self
            .vector
            .iter()
            .zip(other.vector.iter())
            .map(|(a, b)| a * b)
            .sum();

        let norm_a = self.norm();
        let norm_b = other.norm();

        if norm_a == 0.0 || norm_b == 0.0 {
            0.0
        } else {
            dot / (norm_a * norm_b)
        }
    }
}

/// Settings snapshot for an embedder.
///
/// Used for corpus fingerprinting to ensure reproducibility.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbedderSettings {
    /// Model identifier
    pub model_id: String,
    /// Model revision/version
    pub model_revision: Option<String>,
    /// Whether outputs are normalized
    pub normalize: bool,
    /// Maximum input length in tokens
    pub max_input_length: usize,
    /// Additional settings
    #[serde(default)]
    pub extra: HashMap<String, String>,
}

impl EmbedderSettings {
    /// Compute a hash of the settings.
    #[must_use]
    pub fn settings_hash(&self) -> String {
        let json = serde_json::to_string(self).unwrap_or_default();
        blake3::hash(json.as_bytes()).to_hex().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_embedding_hash_deterministic() {
        let result = EmbeddingResult {
            vector: vec![0.1, 0.2, 0.3, 0.4],
            token_count: 4,
            latency_ms: 0,
        };

        let hash1 = result.embedding_hash();
        let hash2 = result.embedding_hash();
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_embedding_different_vectors_different_hash() {
        let result1 = EmbeddingResult {
            vector: vec![0.1, 0.2, 0.3, 0.4],
            token_count: 4,
            latency_ms: 0,
        };
        let result2 = EmbeddingResult {
            vector: vec![0.1, 0.2, 0.3, 0.5],
            token_count: 4,
            latency_ms: 0,
        };

        assert_ne!(result1.embedding_hash(), result2.embedding_hash());
    }

    #[test]
    fn test_cosine_similarity() {
        let a = EmbeddingResult {
            vector: vec![1.0, 0.0, 0.0],
            token_count: 1,
            latency_ms: 0,
        };
        let b = EmbeddingResult {
            vector: vec![1.0, 0.0, 0.0],
            token_count: 1,
            latency_ms: 0,
        };

        let sim = a.cosine_similarity(&b);
        assert!((sim - 1.0).abs() < 1e-5);
    }

    #[test]
    fn test_cosine_similarity_orthogonal() {
        let a = EmbeddingResult {
            vector: vec![1.0, 0.0, 0.0],
            token_count: 1,
            latency_ms: 0,
        };
        let b = EmbeddingResult {
            vector: vec![0.0, 1.0, 0.0],
            token_count: 1,
            latency_ms: 0,
        };

        let sim = a.cosine_similarity(&b);
        assert!(sim.abs() < 1e-5);
    }

    #[test]
    fn test_normalized_check() {
        let normalized = EmbeddingResult {
            vector: vec![0.6, 0.8, 0.0],
            token_count: 1,
            latency_ms: 0,
        };
        assert!(normalized.is_normalized());

        let not_normalized = EmbeddingResult {
            vector: vec![1.0, 1.0, 1.0],
            token_count: 1,
            latency_ms: 0,
        };
        assert!(!not_normalized.is_normalized());
    }

    #[test]
    fn test_embedder_settings_hash() {
        let settings = EmbedderSettings {
            model_id: "test-model".to_string(),
            model_revision: Some("1.0".to_string()),
            normalize: true,
            max_input_length: 512,
            extra: HashMap::new(),
        };

        let hash1 = settings.settings_hash();
        let hash2 = settings.settings_hash();
        assert_eq!(hash1, hash2);
    }

    // ========================================================================
    // Determinism Contract Tests
    // ========================================================================

    #[test]
    fn test_determinism_level_display() {
        assert_eq!(format!("{}", DeterminismLevel::BitExact), "bit-exact");
        assert_eq!(
            format!("{}", DeterminismLevel::SamePlatform),
            "same-platform"
        );
        assert_eq!(
            format!("{}", DeterminismLevel::ToleranceBased),
            "tolerance-based"
        );
        assert_eq!(format!("{}", DeterminismLevel::AuditOnly), "audit-only");
    }

    #[test]
    fn test_bit_exact_contract_passes_identical() {
        let contract = DeterminismContract::bit_exact();
        let a = EmbeddingResult {
            vector: vec![0.1, 0.2, 0.3],
            token_count: 3,
            latency_ms: 0,
        };
        let b = a.clone();

        assert!(contract.verify(&a, &b).is_passed());
    }

    #[test]
    fn test_bit_exact_contract_fails_different() {
        let contract = DeterminismContract::bit_exact();
        let a = EmbeddingResult {
            vector: vec![0.1, 0.2, 0.3],
            token_count: 3,
            latency_ms: 0,
        };
        let b = EmbeddingResult {
            vector: vec![0.1, 0.2, 0.31], // Clearly different value
            token_count: 3,
            latency_ms: 0,
        };

        let verdict = contract.verify(&a, &b);
        assert!(verdict.is_failed());
    }

    #[test]
    fn test_same_platform_contract_passes_within_tolerance() {
        let contract = DeterminismContract::same_platform();
        let a = EmbeddingResult {
            vector: vec![0.1, 0.2, 0.3],
            token_count: 3,
            latency_ms: 0,
        };
        let b = EmbeddingResult {
            vector: vec![0.1, 0.2, 0.3 + 1e-8], // Within tolerance
            token_count: 3,
            latency_ms: 0,
        };

        assert!(contract.verify(&a, &b).is_passed());
    }

    #[test]
    fn test_same_platform_contract_fails_exceeds_tolerance() {
        let contract = DeterminismContract::same_platform();
        let a = EmbeddingResult {
            vector: vec![0.1, 0.2, 0.3],
            token_count: 3,
            latency_ms: 0,
        };
        let b = EmbeddingResult {
            vector: vec![0.1, 0.2, 0.3 + 1e-5], // Exceeds tolerance
            token_count: 3,
            latency_ms: 0,
        };

        assert!(contract.verify(&a, &b).is_failed());
    }

    #[test]
    fn test_tolerance_based_contract_passes_similar_vectors() {
        let contract = DeterminismContract::tolerance_based(0.01); // 1% tolerance
        // Two normalized vectors with high cosine similarity
        let a = EmbeddingResult {
            vector: vec![0.6, 0.8, 0.0],
            token_count: 3,
            latency_ms: 0,
        };
        let b = EmbeddingResult {
            vector: vec![0.61, 0.79, 0.01], // Slightly different but still similar
            token_count: 3,
            latency_ms: 0,
        };

        assert!(contract.verify(&a, &b).is_passed());
    }

    #[test]
    fn test_tolerance_based_contract_fails_dissimilar_vectors() {
        let contract = DeterminismContract::tolerance_based(0.001); // 0.1% tolerance
        let a = EmbeddingResult {
            vector: vec![1.0, 0.0, 0.0],
            token_count: 3,
            latency_ms: 0,
        };
        let b = EmbeddingResult {
            vector: vec![0.0, 1.0, 0.0], // Orthogonal - very different
            token_count: 3,
            latency_ms: 0,
        };

        assert!(contract.verify(&a, &b).is_failed());
    }

    #[test]
    fn test_audit_only_contract_always_passes() {
        let contract = DeterminismContract::audit_only();
        let a = EmbeddingResult {
            vector: vec![1.0, 0.0, 0.0],
            token_count: 3,
            latency_ms: 0,
        };
        let b = EmbeddingResult {
            vector: vec![0.0, 1.0, 0.0], // Completely different
            token_count: 3,
            latency_ms: 0,
        };

        // Audit-only always passes - no determinism guarantee
        assert!(contract.verify(&a, &b).is_passed());
    }

    #[test]
    fn test_contract_fails_dimension_mismatch() {
        let contract = DeterminismContract::bit_exact();
        let a = EmbeddingResult {
            vector: vec![0.1, 0.2, 0.3],
            token_count: 3,
            latency_ms: 0,
        };
        let b = EmbeddingResult {
            vector: vec![0.1, 0.2], // Different dimensions
            token_count: 2,
            latency_ms: 0,
        };

        let verdict = contract.verify(&a, &b);
        assert!(verdict.is_failed());
        if let DeterminismVerdict::Failed { reason } = verdict {
            assert!(reason.contains("Dimension mismatch"));
        }
    }

    #[test]
    fn test_detect_platform() {
        let platform = DeterminismContract::detect_platform();
        // Should contain arch and os
        assert!(!platform.is_empty());
        assert!(platform.contains('-'));
    }

    #[test]
    fn test_ranking_preservation() {
        let contract = DeterminismContract::tolerance_based(0.01);

        // Query
        let query_a = EmbeddingResult {
            vector: vec![1.0, 0.0, 0.0],
            token_count: 1,
            latency_ms: 0,
        };
        let query_b = query_a.clone();

        // Candidates - same order expected
        let candidates_a = vec![
            EmbeddingResult {
                vector: vec![0.9, 0.1, 0.0],
                token_count: 1,
                latency_ms: 0,
            },
            EmbeddingResult {
                vector: vec![0.5, 0.5, 0.0],
                token_count: 1,
                latency_ms: 0,
            },
            EmbeddingResult {
                vector: vec![0.0, 1.0, 0.0],
                token_count: 1,
                latency_ms: 0,
            },
        ];
        let candidates_b = candidates_a.clone();

        assert!(contract.verify_ranking(&query_a, &candidates_a, &query_b, &candidates_b));
    }

    #[test]
    fn test_ranking_preservation_fails_different_order() {
        let contract = DeterminismContract::tolerance_based(0.01);

        let query_a = EmbeddingResult {
            vector: vec![1.0, 0.0, 0.0],
            token_count: 1,
            latency_ms: 0,
        };
        let query_b = EmbeddingResult {
            vector: vec![0.0, 1.0, 0.0], // Different query changes ranking
            token_count: 1,
            latency_ms: 0,
        };

        let candidates_a = vec![
            EmbeddingResult {
                vector: vec![0.9, 0.1, 0.0],
                token_count: 1,
                latency_ms: 0,
            },
            EmbeddingResult {
                vector: vec![0.1, 0.9, 0.0],
                token_count: 1,
                latency_ms: 0,
            },
        ];
        let candidates_b = candidates_a.clone();

        // Different queries will produce different rankings
        assert!(!contract.verify_ranking(&query_a, &candidates_a, &query_b, &candidates_b));
    }
}
