// Copyright 2024-2026 Reflective Labs

//! Recall provider trait and implementations.
//!
//! Defines the interface for recall providers (vector search backends)
//! and includes a mock implementation for testing.

use super::corpus::CorpusFingerprint;
use super::embedder::Embedder;
use super::normalizer::{RawRecallResult, RecallNormalizer};
use super::pii::embedding_input_hash;
use super::types::{
    CandidateProvenance, CandidateScore, CandidateSourceType, RecallCandidate, RecallPolicy,
    RecallProvenanceEnvelope, RecallQuery, RecallTraceLink, StopReason,
};
use crate::error::LlmResult;
use serde::{Deserialize, Serialize};
use std::time::Instant;

/// Response from a recall operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecallResponse {
    /// The returned candidates
    pub candidates: Vec<RecallCandidate>,
    /// Why recall stopped
    pub stop_reason: Option<StopReason>,
    /// Trace link for reproducibility (lightweight)
    pub trace_link: RecallTraceLink,
    /// Full provenance envelope for audit/replay
    pub provenance: Option<RecallProvenanceEnvelope>,
}

impl RecallResponse {
    /// Create an empty response.
    #[must_use]
    pub fn empty(trace_link: RecallTraceLink) -> Self {
        Self {
            candidates: vec![],
            stop_reason: Some(StopReason::BelowThreshold),
            trace_link,
            provenance: None,
        }
    }

    /// Check if the response has any candidates.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.candidates.is_empty()
    }

    /// Get the number of candidates.
    #[must_use]
    pub fn len(&self) -> usize {
        self.candidates.len()
    }

    /// Get the provenance envelope hash if available.
    #[must_use]
    pub fn provenance_hash(&self) -> Option<String> {
        self.provenance.as_ref().map(|p| p.envelope_hash())
    }
}

/// Trait for recall providers.
///
/// Recall providers implement semantic similarity search against
/// a corpus of past decisions, runbooks, etc.
pub trait RecallProvider: Send + Sync {
    /// Get the corpus fingerprint.
    fn corpus_fingerprint(&self) -> &CorpusFingerprint;

    /// Perform a recall query.
    fn recall(&self, query: &RecallQuery) -> LlmResult<RecallResponse>;

    /// Perform a recall query with policy enforcement.
    fn recall_with_policy(
        &self,
        query: &RecallQuery,
        policy: &RecallPolicy,
    ) -> LlmResult<RecallResponse> {
        let start = Instant::now();

        // Check if enabled
        if !policy.enabled {
            return Ok(RecallResponse::empty(RecallTraceLink {
                embedding_hash: String::new(),
                corpus_version: self.corpus_fingerprint().to_version_string(),
                embedder_id: String::new(),
                candidates_searched: 0,
                candidates_returned: 0,
                latency_ms: 0,
            }));
        }

        // Perform recall with adjusted top_k
        let adjusted_query = RecallQuery {
            top_k: query.top_k.min(policy.max_k_total),
            ..query.clone()
        };

        let mut response = self.recall(&adjusted_query)?;

        // Apply policy filters
        response
            .candidates
            .retain(|c| c.final_score >= policy.min_score_threshold);

        // Enforce max_k_total
        if response.candidates.len() > policy.max_k_total {
            response.candidates.truncate(policy.max_k_total);
            response.stop_reason = Some(StopReason::BudgetExhausted);
        }

        // Update provenance with policy hash
        if let Some(ref mut provenance) = response.provenance {
            provenance.policy_snapshot_hash = policy.snapshot_hash();
            // Update candidate scores after filtering
            provenance.candidate_scores = response
                .candidates
                .iter()
                .map(|c| CandidateScore {
                    id: c.id.clone(),
                    score: c.final_score,
                })
                .collect();
            provenance.candidates_returned = response.candidates.len();
            provenance.stop_reason = response.stop_reason;
        }

        // Check latency
        let elapsed = start.elapsed().as_millis() as u64;
        if elapsed > policy.budgets.max_latency_ms {
            response.stop_reason = Some(StopReason::LatencyExceeded);
        }

        // Update trace link
        response.trace_link.latency_ms = elapsed;
        response.trace_link.candidates_returned = response.candidates.len();

        Ok(response)
    }
}

/// Mock recall provider for testing.
///
/// Uses a HashEmbedder and in-memory storage for deterministic tests.
#[derive(Debug)]
pub struct MockRecallProvider<E: Embedder> {
    embedder: E,
    corpus_fingerprint: CorpusFingerprint,
    records: Vec<MockRecord>,
    normalizer: RecallNormalizer,
}

/// A record in the mock provider.
#[derive(Debug, Clone)]
struct MockRecord {
    id: String,
    summary: String,
    embedding: Vec<f32>,
    source_type: CandidateSourceType,
    provenance: CandidateProvenance,
}

impl<E: Embedder> MockRecallProvider<E> {
    /// Create a new mock provider with the given embedder.
    pub fn new(embedder: E, dataset_snapshot: impl Into<String>) -> Self {
        let fingerprint = CorpusFingerprint::new("mock-v1", &embedder, dataset_snapshot, None);

        Self {
            embedder,
            corpus_fingerprint: fingerprint,
            records: vec![],
            normalizer: RecallNormalizer::new().with_min_threshold(0.0),
        }
    }

    /// Add a record to the mock corpus.
    pub fn add_record(
        &mut self,
        id: impl Into<String>,
        summary: impl Into<String>,
        source_type: CandidateSourceType,
    ) -> LlmResult<()> {
        let id = id.into();
        let summary = summary.into();
        let embedding = self.embedder.embed(&summary)?.vector;

        self.records.push(MockRecord {
            id,
            summary,
            embedding,
            source_type,
            provenance: CandidateProvenance {
                created_at: "2026-01-16T00:00:00Z".to_string(),
                source_chain_id: None,
                source_step: None,
                corpus_version: self.corpus_fingerprint.to_version_string(),
            },
        });

        Ok(())
    }

    /// Create a mock provider with sample failure records.
    pub fn with_sample_failures(embedder: E) -> LlmResult<Self> {
        let mut provider = Self::new(embedder, "sample-v1");

        provider.add_record(
            "fail-001",
            "Deployment failed due to memory limit exceeded",
            CandidateSourceType::SimilarFailure,
        )?;

        provider.add_record(
            "fail-002",
            "Deployment failed due to timeout during health check",
            CandidateSourceType::SimilarFailure,
        )?;

        provider.add_record(
            "success-001",
            "Deployment succeeded with rolling update strategy",
            CandidateSourceType::SimilarSuccess,
        )?;

        provider.add_record(
            "runbook-001",
            "Runbook: How to handle memory limit failures",
            CandidateSourceType::Runbook,
        )?;

        Ok(provider)
    }

    /// Compute cosine similarity between two vectors.
    fn cosine_similarity(a: &[f32], b: &[f32]) -> f64 {
        if a.len() != b.len() {
            return 0.0;
        }

        let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
        let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
        let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();

        if norm_a == 0.0 || norm_b == 0.0 {
            0.0
        } else {
            f64::from(dot / (norm_a * norm_b))
        }
    }
}

impl<E: Embedder + Send + Sync> RecallProvider for MockRecallProvider<E> {
    fn corpus_fingerprint(&self) -> &CorpusFingerprint {
        &self.corpus_fingerprint
    }

    fn recall(&self, query: &RecallQuery) -> LlmResult<RecallResponse> {
        let start = Instant::now();

        // Embed the query
        let query_result = self.embedder.embed(&query.query_text)?;
        let query_embedding = &query_result.vector;

        // Score all records
        let mut scored: Vec<(f64, &MockRecord)> = self
            .records
            .iter()
            .map(|r| (Self::cosine_similarity(query_embedding, &r.embedding), r))
            .collect();

        // Sort by score descending
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

        // Take top_k
        let top_k = scored.into_iter().take(query.top_k);

        // Convert to raw results
        let raw_results: Vec<RawRecallResult> = top_k
            .map(|(score, record)| {
                RawRecallResult::new(
                    record.id.clone(),
                    record.summary.clone(),
                    score,
                    record.source_type,
                )
                .with_provenance(record.provenance.clone())
            })
            .collect();

        let elapsed = start.elapsed().as_millis() as u64;

        // Create trace link
        let trace_link = RecallTraceLink {
            embedding_hash: query_result.embedding_hash(),
            corpus_version: self.corpus_fingerprint.to_version_string(),
            embedder_id: self.embedder.embedder_id().to_string(),
            candidates_searched: self.records.len(),
            candidates_returned: raw_results.len(),
            latency_ms: elapsed,
        };

        // Normalize results
        let normalized =
            self.normalizer
                .normalize_batch(raw_results, trace_link.clone(), query.top_k);

        // Build candidate scores for provenance
        let candidate_scores: Vec<CandidateScore> = normalized
            .candidates
            .iter()
            .map(|c| CandidateScore {
                id: c.id.clone(),
                score: c.final_score,
            })
            .collect();

        // Create full provenance envelope
        let embedder_settings = self.embedder.settings_snapshot();
        let provenance = RecallProvenanceEnvelope {
            query_hash: query.query_hash(),
            embedding_input_hash: embedding_input_hash(&query.query_text),
            embedding_hash: query_result.embedding_hash(),
            embedder_id: self.embedder.embedder_id().to_string(),
            embedder_settings_hash: embedder_settings.settings_hash(),
            corpus_fingerprint: self.corpus_fingerprint.to_version_string(),
            policy_snapshot_hash: String::new(), // Policy applied in recall_with_policy
            // Runtime augmentation by default - kernel consumer
            purpose: super::types::RecallUse::RuntimeAugmentation,
            consumers: vec![super::types::RecallConsumer::Kernel],
            candidate_scores,
            candidates_searched: self.records.len(),
            candidates_returned: normalized.candidates.len(),
            stop_reason: normalized.stop_reason,
            latency_ms: elapsed,
            timestamp: timestamp_now(),
            signature: "unsigned".to_string(),
        };

        Ok(RecallResponse {
            candidates: normalized.candidates,
            stop_reason: normalized.stop_reason,
            trace_link,
            provenance: Some(provenance),
        })
    }
}

/// Generate current timestamp in ISO 8601 format.
fn timestamp_now() -> String {
    // Placeholder - in production use chrono
    "2026-01-18T00:00:00Z".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recall::HashEmbedder;

    #[test]
    fn test_mock_provider_basic() {
        let embedder = HashEmbedder::new(128);
        let mut provider = MockRecallProvider::new(embedder, "test-v1");

        provider
            .add_record(
                "test-1",
                "Test record one",
                CandidateSourceType::SimilarFailure,
            )
            .unwrap();

        let query = RecallQuery::new("Test record", 5);
        let response = provider.recall(&query).unwrap();

        assert_eq!(response.len(), 1);
        assert_eq!(response.candidates[0].id, "test-1");
    }

    #[test]
    fn test_mock_provider_deterministic() {
        let embedder = HashEmbedder::new(128);
        let provider = MockRecallProvider::with_sample_failures(embedder).unwrap();

        let query = RecallQuery::new("deployment failure", 3);

        let r1 = provider.recall(&query).unwrap();
        let r2 = provider.recall(&query).unwrap();

        assert_eq!(r1.candidates.len(), r2.candidates.len());
        assert_eq!(r1.trace_link.embedding_hash, r2.trace_link.embedding_hash);

        for (c1, c2) in r1.candidates.iter().zip(r2.candidates.iter()) {
            assert_eq!(c1.id, c2.id);
            assert!((c1.final_score - c2.final_score).abs() < 1e-6);
        }
    }

    #[test]
    fn test_mock_provider_with_policy() {
        let embedder = HashEmbedder::new(128);
        let provider = MockRecallProvider::with_sample_failures(embedder).unwrap();

        let query = RecallQuery::new("deployment failure", 10);
        let policy = RecallPolicy {
            enabled: true,
            max_k_total: 2,
            min_score_threshold: 0.0,
            ..Default::default()
        };

        let response = provider.recall_with_policy(&query, &policy).unwrap();

        assert!(response.len() <= 2);
    }

    #[test]
    fn test_mock_provider_disabled_policy() {
        let embedder = HashEmbedder::new(128);
        let provider = MockRecallProvider::with_sample_failures(embedder).unwrap();

        let query = RecallQuery::new("deployment failure", 10);
        let policy = RecallPolicy::disabled();

        let response = provider.recall_with_policy(&query, &policy).unwrap();

        assert!(response.is_empty());
    }

    #[test]
    fn test_corpus_fingerprint() {
        let embedder = HashEmbedder::new(128);
        let provider = MockRecallProvider::new(embedder, "test-snapshot");

        let fp = provider.corpus_fingerprint();
        assert_eq!(fp.schema_version, "mock-v1");
        assert_eq!(fp.dataset_snapshot, "test-snapshot");
    }

    #[test]
    fn test_recall_response_empty() {
        let trace_link = RecallTraceLink {
            embedding_hash: "test".to_string(),
            corpus_version: "v1".to_string(),
            embedder_id: "test".to_string(),
            candidates_searched: 0,
            candidates_returned: 0,
            latency_ms: 0,
        };

        let response = RecallResponse::empty(trace_link);
        assert!(response.is_empty());
        assert_eq!(response.stop_reason, Some(StopReason::BelowThreshold));
    }
}
