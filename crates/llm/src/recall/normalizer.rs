// Copyright 2024-2026 Reflective Labs

//! Recall result normalization.
//!
//! Normalizes raw recall results into a consistent format with
//! provenance and relevance scoring.

use super::types::{
    CandidateProvenance, CandidateSourceType, RecallCandidate, RecallTraceLink, RelevanceLevel,
    StopReason,
};
use serde::{Deserialize, Serialize};

/// Normalized recall results with metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NormalizedRecall {
    /// Normalized candidates
    pub candidates: Vec<RecallCandidate>,
    /// Why recall stopped
    pub stop_reason: Option<StopReason>,
    /// Total candidates searched
    pub total_searched: usize,
    /// Trace link for reproducibility
    pub trace_link: RecallTraceLink,
}

impl NormalizedRecall {
    /// Check if any candidates were found.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.candidates.is_empty()
    }

    /// Get the number of candidates returned.
    #[must_use]
    pub fn len(&self) -> usize {
        self.candidates.len()
    }

    /// Get candidates above a score threshold.
    #[must_use]
    pub fn above_threshold(&self, threshold: f64) -> Vec<&RecallCandidate> {
        self.candidates
            .iter()
            .filter(|c| c.final_score >= threshold)
            .collect()
    }

    /// Get candidates by source type.
    #[must_use]
    pub fn by_source_type(&self, source_type: CandidateSourceType) -> Vec<&RecallCandidate> {
        self.candidates
            .iter()
            .filter(|c| c.source_type == source_type)
            .collect()
    }
}

/// Normalizer for recall results.
///
/// Converts raw vector search results into normalized candidates
/// with consistent scoring and metadata.
#[derive(Debug, Clone)]
pub struct RecallNormalizer {
    /// Minimum score threshold
    min_threshold: f64,
    /// Score scaling factor (raw scores may have different ranges)
    score_scale: f64,
    /// Whether to apply sigmoid normalization
    use_sigmoid: bool,
}

impl Default for RecallNormalizer {
    fn default() -> Self {
        Self {
            min_threshold: 0.0,
            score_scale: 1.0,
            use_sigmoid: false,
        }
    }
}

impl RecallNormalizer {
    /// Create a new normalizer with default settings.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the minimum score threshold.
    #[must_use]
    pub fn with_min_threshold(mut self, threshold: f64) -> Self {
        self.min_threshold = threshold;
        self
    }

    /// Set the score scaling factor.
    #[must_use]
    pub fn with_score_scale(mut self, scale: f64) -> Self {
        self.score_scale = scale;
        self
    }

    /// Enable sigmoid normalization.
    #[must_use]
    pub fn with_sigmoid(mut self) -> Self {
        self.use_sigmoid = true;
        self
    }

    /// Normalize a raw score.
    #[must_use]
    pub fn normalize_score(&self, raw_score: f64) -> f64 {
        let scaled = raw_score * self.score_scale;

        if self.use_sigmoid {
            // Sigmoid normalization: maps (-inf, inf) to (0, 1)
            1.0 / (1.0 + (-scaled).exp())
        } else {
            // Clamp to [0, 1]
            scaled.clamp(0.0, 1.0)
        }
    }

    /// Create a normalized candidate from raw search result data.
    #[must_use]
    pub fn create_candidate(
        &self,
        id: String,
        summary: String,
        raw_score: f64,
        source_type: CandidateSourceType,
        provenance: CandidateProvenance,
    ) -> Option<RecallCandidate> {
        let final_score = self.normalize_score(raw_score);

        if final_score < self.min_threshold {
            return None;
        }

        Some(RecallCandidate {
            id,
            summary,
            raw_score,
            final_score,
            relevance: RelevanceLevel::from_score(final_score),
            source_type,
            provenance,
        })
    }

    /// Normalize a batch of raw results.
    #[must_use]
    pub fn normalize_batch(
        &self,
        raw_results: Vec<RawRecallResult>,
        trace_link: RecallTraceLink,
        max_candidates: usize,
    ) -> NormalizedRecall {
        let total_searched = raw_results.len();

        let mut candidates: Vec<RecallCandidate> = raw_results
            .into_iter()
            .filter_map(|r| {
                self.create_candidate(r.id, r.summary, r.raw_score, r.source_type, r.provenance)
            })
            .collect();

        // Sort by final score descending
        candidates.sort_by(|a, b| {
            b.final_score
                .partial_cmp(&a.final_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // Determine stop reason
        let stop_reason = if candidates.len() > max_candidates {
            candidates.truncate(max_candidates);
            Some(StopReason::ReachedTopK)
        } else if candidates.is_empty() && total_searched > 0 {
            Some(StopReason::BelowThreshold)
        } else {
            None
        };

        NormalizedRecall {
            candidates,
            stop_reason,
            total_searched,
            trace_link,
        }
    }
}

/// Raw recall result from vector search.
#[derive(Debug, Clone)]
pub struct RawRecallResult {
    /// Unique identifier
    pub id: String,
    /// Summary text
    pub summary: String,
    /// Raw similarity score from vector search
    pub raw_score: f64,
    /// Source type
    pub source_type: CandidateSourceType,
    /// Provenance
    pub provenance: CandidateProvenance,
}

impl RawRecallResult {
    /// Create a new raw result.
    #[must_use]
    pub fn new(
        id: impl Into<String>,
        summary: impl Into<String>,
        raw_score: f64,
        source_type: CandidateSourceType,
    ) -> Self {
        Self {
            id: id.into(),
            summary: summary.into(),
            raw_score,
            source_type,
            provenance: CandidateProvenance {
                created_at: String::new(),
                source_chain_id: None,
                source_step: None,
                corpus_version: String::new(),
            },
        }
    }

    /// Add provenance information.
    #[must_use]
    pub fn with_provenance(mut self, provenance: CandidateProvenance) -> Self {
        self.provenance = provenance;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_trace_link() -> RecallTraceLink {
        RecallTraceLink {
            embedding_hash: "abc123".to_string(),
            corpus_version: "v1".to_string(),
            embedder_id: "test".to_string(),
            candidates_searched: 0,
            candidates_returned: 0,
            latency_ms: 0,
        }
    }

    #[test]
    fn test_normalize_score_default() {
        let normalizer = RecallNormalizer::new();
        assert!((normalizer.normalize_score(0.5) - 0.5).abs() < 1e-6);
        assert!((normalizer.normalize_score(1.5) - 1.0).abs() < 1e-6);
        assert!((normalizer.normalize_score(-0.5) - 0.0).abs() < 1e-6);
    }

    #[test]
    fn test_normalize_score_with_scale() {
        let normalizer = RecallNormalizer::new().with_score_scale(2.0);
        assert!((normalizer.normalize_score(0.25) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn test_normalize_score_sigmoid() {
        let normalizer = RecallNormalizer::new().with_sigmoid();
        let score = normalizer.normalize_score(0.0);
        assert!((score - 0.5).abs() < 1e-6); // sigmoid(0) = 0.5
    }

    #[test]
    fn test_create_candidate_above_threshold() {
        let normalizer = RecallNormalizer::new().with_min_threshold(0.5);

        let provenance = CandidateProvenance {
            created_at: "2026-01-16".to_string(),
            source_chain_id: None,
            source_step: None,
            corpus_version: "v1".to_string(),
        };

        let candidate = normalizer.create_candidate(
            "test-1".to_string(),
            "Test summary".to_string(),
            0.8,
            CandidateSourceType::SimilarFailure,
            provenance,
        );

        assert!(candidate.is_some());
        let c = candidate.unwrap();
        assert_eq!(c.id, "test-1");
        assert_eq!(c.relevance, RelevanceLevel::High);
    }

    #[test]
    fn test_create_candidate_below_threshold() {
        let normalizer = RecallNormalizer::new().with_min_threshold(0.5);

        let provenance = CandidateProvenance {
            created_at: "2026-01-16".to_string(),
            source_chain_id: None,
            source_step: None,
            corpus_version: "v1".to_string(),
        };

        let candidate = normalizer.create_candidate(
            "test-1".to_string(),
            "Test summary".to_string(),
            0.3,
            CandidateSourceType::SimilarFailure,
            provenance,
        );

        assert!(candidate.is_none());
    }

    #[test]
    fn test_normalize_batch() {
        let normalizer = RecallNormalizer::new();

        let results = vec![
            RawRecallResult::new(
                "id-1",
                "Summary 1",
                0.9,
                CandidateSourceType::SimilarFailure,
            ),
            RawRecallResult::new(
                "id-2",
                "Summary 2",
                0.7,
                CandidateSourceType::SimilarSuccess,
            ),
            RawRecallResult::new("id-3", "Summary 3", 0.5, CandidateSourceType::Runbook),
        ];

        let normalized = normalizer.normalize_batch(results, make_trace_link(), 10);

        assert_eq!(normalized.len(), 3);
        assert_eq!(normalized.candidates[0].id, "id-1"); // Highest score first
        assert_eq!(normalized.total_searched, 3);
    }

    #[test]
    fn test_normalize_batch_truncation() {
        let normalizer = RecallNormalizer::new();

        let results = vec![
            RawRecallResult::new(
                "id-1",
                "Summary 1",
                0.9,
                CandidateSourceType::SimilarFailure,
            ),
            RawRecallResult::new(
                "id-2",
                "Summary 2",
                0.8,
                CandidateSourceType::SimilarFailure,
            ),
            RawRecallResult::new(
                "id-3",
                "Summary 3",
                0.7,
                CandidateSourceType::SimilarFailure,
            ),
        ];

        let normalized = normalizer.normalize_batch(results, make_trace_link(), 2);

        assert_eq!(normalized.len(), 2);
        assert_eq!(normalized.stop_reason, Some(StopReason::ReachedTopK));
    }

    #[test]
    fn test_normalized_recall_by_source_type() {
        let normalizer = RecallNormalizer::new();

        let results = vec![
            RawRecallResult::new(
                "id-1",
                "Summary 1",
                0.9,
                CandidateSourceType::SimilarFailure,
            ),
            RawRecallResult::new(
                "id-2",
                "Summary 2",
                0.8,
                CandidateSourceType::SimilarSuccess,
            ),
            RawRecallResult::new(
                "id-3",
                "Summary 3",
                0.7,
                CandidateSourceType::SimilarFailure,
            ),
        ];

        let normalized = normalizer.normalize_batch(results, make_trace_link(), 10);
        let failures = normalized.by_source_type(CandidateSourceType::SimilarFailure);

        assert_eq!(failures.len(), 2);
    }

    #[test]
    fn test_normalized_recall_above_threshold() {
        let normalizer = RecallNormalizer::new();

        let results = vec![
            RawRecallResult::new(
                "id-1",
                "Summary 1",
                0.9,
                CandidateSourceType::SimilarFailure,
            ),
            RawRecallResult::new(
                "id-2",
                "Summary 2",
                0.6,
                CandidateSourceType::SimilarFailure,
            ),
            RawRecallResult::new(
                "id-3",
                "Summary 3",
                0.3,
                CandidateSourceType::SimilarFailure,
            ),
        ];

        let normalized = normalizer.normalize_batch(results, make_trace_link(), 10);
        let high_quality = normalized.above_threshold(0.5);

        assert_eq!(high_quality.len(), 2);
    }
}
