// Copyright 2024-2026 Reflective Labs

//! Core types for semantic recall.
//!
//! ## Migration Note
//!
//! The portable recall types (RecallPolicy, RecallQuery, etc.) have been moved
//! to `converge_core::recall` and are re-exported here for backward compatibility.
//!
//! Types that remain local to converge-llm:
//! - RecallConfig, RecallPerStep, RecallTrigger (chain execution specific)
//! - RecallContext, RecallHint (prompt injection specific)
//! - DecisionOutcome, DecisionRecord (trace specific, uses PII redaction)

use crate::trace::DecisionStep;
use serde::{Deserialize, Serialize};

// ============================================================================
// Re-exports from converge-core (portable types)
// ============================================================================

pub use converge_core::recall::{
    CandidateProvenance,
    CandidateScore,
    CandidateSourceType,
    RecallBudgets,
    RecallCandidate,
    RecallConsumer,
    // Policy types
    RecallPolicy,
    RecallProvenanceEnvelope,
    // Query/Candidate types
    RecallQuery,
    // Provenance types
    RecallTraceLink,
    // Use/Consumer types (Recall ≠ Training boundary)
    RecallUse,
    RelevanceLevel,
    StopReason,
    // Functions
    recall_use_allowed,
};

// ============================================================================
// Local Types (chain execution specific)
// ============================================================================

/// Configuration for recall in chain execution.
///
/// Always present in `ChainConfig` regardless of feature flag.
/// The `Option<RecallConfig>` wrapping handles the disabled case.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RecallConfig {
    /// Policy controlling recall behavior
    pub policy: RecallPolicy,
    /// Per-step recall triggering
    pub per_step: RecallPerStep,
}

/// Per-step recall configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RecallPerStep {
    /// Recall trigger for reasoning step
    pub reasoning: RecallTrigger,
    /// Recall trigger for evaluation step
    pub evaluation: RecallTrigger,
    /// Recall trigger for planning step
    pub planning: RecallTrigger,
}

/// When to trigger recall for a step.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum RecallTrigger {
    /// Never use recall for this step
    #[default]
    Never,
    /// Always use recall
    Always,
    /// Use if explicitly requested in pack policy
    OnRequest,
}

// ============================================================================
// Local Types (prompt injection specific)
// ============================================================================

/// Context injected into prompts from recall results.
///
/// Explicitly separated from evidence - validators MUST reject
/// citations referencing this namespace.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RecallContext {
    /// Similar past failures
    pub similar_failures: Vec<RecallHint>,
    /// Similar past successes
    pub similar_successes: Vec<RecallHint>,
    /// Suggested runbooks
    pub suggested_runbooks: Vec<RecallHint>,
    /// Recommended adapter IDs
    pub recommended_adapter_ids: Vec<String>,
    /// Anti-patterns to avoid
    pub anti_patterns: Vec<RecallHint>,
    /// Trace link for reproducibility
    pub trace_link: Option<RecallTraceLink>,
}

impl RecallContext {
    /// Create an empty recall context.
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    /// Check if the context has any hints.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.similar_failures.is_empty()
            && self.similar_successes.is_empty()
            && self.suggested_runbooks.is_empty()
            && self.recommended_adapter_ids.is_empty()
            && self.anti_patterns.is_empty()
    }

    /// Get all recall IDs for validation.
    #[must_use]
    pub fn all_ids(&self) -> Vec<String> {
        let mut ids = Vec::new();
        ids.extend(self.similar_failures.iter().map(|h| h.id.clone()));
        ids.extend(self.similar_successes.iter().map(|h| h.id.clone()));
        ids.extend(self.suggested_runbooks.iter().map(|h| h.id.clone()));
        ids.extend(self.anti_patterns.iter().map(|h| h.id.clone()));
        ids
    }
}

/// A hint from recall (summary view of a candidate).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecallHint {
    /// Unique identifier
    pub id: String,
    /// Brief summary
    pub summary: String,
    /// Similarity score
    pub score: f64,
    /// Relevance level
    pub relevance: String,
}

impl RecallHint {
    /// Create from a recall candidate.
    #[must_use]
    pub fn from_candidate(candidate: &RecallCandidate) -> Self {
        Self {
            id: candidate.id.clone(),
            summary: candidate.summary.clone(),
            score: candidate.final_score,
            relevance: candidate.relevance.as_str().to_string(),
        }
    }
}

// ============================================================================
// Local Types (trace specific)
// ============================================================================

/// Decision outcome for indexing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DecisionOutcome {
    Success,
    Failure,
    Partial,
}

/// A record of a past decision for recall indexing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionRecord {
    /// Unique identifier
    pub id: String,
    /// Which step this record is from
    pub step: DecisionStep,
    /// Outcome of the decision
    pub outcome: DecisionOutcome,
    /// Contract type that was validated
    pub contract_type: String,
    /// Summary of the input state
    pub input_summary: String,
    /// Summary of the output
    pub output_summary: String,
    /// Chain ID this record came from
    pub chain_id: String,
    /// When this record was created
    pub created_at: String,
    /// Optional tenant scope
    pub tenant_scope: Option<String>,
}

impl DecisionRecord {
    /// Convert to embedding text with PII redaction.
    #[must_use]
    pub fn to_embedding_text(&self) -> String {
        use super::pii::redact_pii;

        let input_clean = redact_pii(&self.input_summary);
        let output_clean = redact_pii(&self.output_summary);

        format!(
            "Step: {:?} | Outcome: {:?} | Contract: {} | Input: {} | Output: {}",
            self.step, self.outcome, self.contract_type, input_clean, output_clean
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_recall_policy_enabled() {
        let policy = RecallPolicy::enabled();
        assert!(policy.enabled);
    }

    #[test]
    fn test_recall_policy_disabled() {
        let policy = RecallPolicy::disabled();
        assert!(!policy.enabled);
    }

    #[test]
    fn test_relevance_from_score() {
        assert_eq!(RelevanceLevel::from_score(0.9), RelevanceLevel::High);
        assert_eq!(RelevanceLevel::from_score(0.6), RelevanceLevel::Medium);
        assert_eq!(RelevanceLevel::from_score(0.3), RelevanceLevel::Low);
    }

    #[test]
    fn test_recall_query_builder() {
        let query = RecallQuery::new("test", 5)
            .with_step_context(DecisionStep::Reasoning)
            .with_tenant_scope("tenant-1");

        assert_eq!(query.query_text, "test");
        assert_eq!(query.top_k, 5);
        assert_eq!(query.step_context, Some(DecisionStep::Reasoning));
        assert_eq!(query.tenant_scope, Some("tenant-1".to_string()));
    }

    #[test]
    fn test_recall_context_all_ids() {
        let context = RecallContext {
            similar_failures: vec![RecallHint {
                id: "fail-1".to_string(),
                summary: "test".to_string(),
                score: 0.9,
                relevance: "high".to_string(),
            }],
            similar_successes: vec![RecallHint {
                id: "success-1".to_string(),
                summary: "test".to_string(),
                score: 0.8,
                relevance: "high".to_string(),
            }],
            ..Default::default()
        };

        let ids = context.all_ids();
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&"fail-1".to_string()));
        assert!(ids.contains(&"success-1".to_string()));
    }

    #[test]
    fn test_decision_record_to_embedding_text() {
        let record = DecisionRecord {
            id: "rec-1".to_string(),
            step: DecisionStep::Reasoning,
            outcome: DecisionOutcome::Success,
            contract_type: "Reasoning".to_string(),
            input_summary: "User john@example.com reported issue".to_string(),
            output_summary: "Analyzed successfully".to_string(),
            chain_id: "chain-1".to_string(),
            created_at: "2026-01-16".to_string(),
            tenant_scope: None,
        };

        let text = record.to_embedding_text();
        assert!(!text.contains("john@example.com"));
        assert!(text.contains("<EMAIL>"));
        assert!(text.contains("Step: Reasoning"));
    }

    // ========================================================================
    // RecallUse / RecallConsumer Tests (Recall ≠ Training boundary)
    // ========================================================================

    /// AXIOM: Policy defaults to runtime-only.
    /// Training candidate selection must be explicitly enabled.
    #[test]
    fn test_recall_policy_defaults_to_runtime_only() {
        let policy = RecallPolicy::default();
        assert!(
            policy
                .allowed_uses
                .contains(&RecallUse::RuntimeAugmentation),
            "Default policy must allow RuntimeAugmentation"
        );
        assert!(
            !policy
                .allowed_uses
                .contains(&RecallUse::TrainingCandidateSelection),
            "Default policy must NOT allow TrainingCandidateSelection"
        );
    }

    /// AXIOM: Training use is blocked in kernel by default.
    /// This is the primary enforcement test for "Recall ≠ Training".
    #[test]
    fn test_recall_training_purpose_is_blocked_in_kernel() {
        let policy = RecallPolicy {
            allowed_uses: vec![RecallUse::RuntimeAugmentation],
            ..Default::default()
        };

        // Runtime use should be allowed
        assert!(
            recall_use_allowed(&policy, RecallUse::RuntimeAugmentation),
            "RuntimeAugmentation must be allowed"
        );

        // Training use should be blocked
        assert!(
            !recall_use_allowed(&policy, RecallUse::TrainingCandidateSelection),
            "TrainingCandidateSelection must be blocked when not in allowed_uses"
        );
    }

    /// Test that enabling training explicitly works.
    #[test]
    fn test_recall_training_can_be_explicitly_enabled() {
        let policy = RecallPolicy {
            allowed_uses: vec![
                RecallUse::RuntimeAugmentation,
                RecallUse::TrainingCandidateSelection,
            ],
            ..Default::default()
        };

        assert!(recall_use_allowed(&policy, RecallUse::RuntimeAugmentation));
        assert!(recall_use_allowed(
            &policy,
            RecallUse::TrainingCandidateSelection
        ));
    }

    /// AXIOM: Provenance captures purpose and consumers deterministically.
    /// Same envelope → same hash.
    #[test]
    fn test_recall_provenance_includes_purpose_and_consumers() {
        let env = RecallProvenanceEnvelope {
            query_hash: "query-hash".to_string(),
            embedding_input_hash: "embed-input-hash".to_string(),
            embedding_hash: "embed-hash".to_string(),
            embedder_id: "hash-v1".to_string(),
            embedder_settings_hash: "settings-hash".to_string(),
            corpus_fingerprint: "corpus-fp".to_string(),
            policy_snapshot_hash: "policy-hash".to_string(),
            purpose: RecallUse::RuntimeAugmentation,
            consumers: vec![RecallConsumer::Kernel],
            candidate_scores: vec![],
            candidates_searched: 100,
            candidates_returned: 5,
            stop_reason: Some(StopReason::ReachedTopK),
            latency_ms: 42,
            timestamp: "2026-01-18T12:00:00Z".to_string(),
            signature: "unsigned".to_string(),
        };

        let hash1 = env.envelope_hash();
        let hash2 = env.envelope_hash();
        assert_eq!(hash1, hash2, "Same envelope must produce same hash");

        // Changing purpose changes the hash
        let mut env_training = env.clone();
        env_training.purpose = RecallUse::TrainingCandidateSelection;
        let hash3 = env_training.envelope_hash();
        assert_ne!(
            hash1, hash3,
            "Different purpose must produce different hash"
        );

        // Changing consumers changes the hash
        let mut env_trainer = env.clone();
        env_trainer.consumers = vec![RecallConsumer::Trainer];
        let hash4 = env_trainer.envelope_hash();
        assert_ne!(
            hash1, hash4,
            "Different consumers must produce different hash"
        );
    }

    /// Test replay matching includes purpose/consumers.
    #[test]
    fn test_replay_matching_includes_purpose_and_consumers() {
        let env1 = RecallProvenanceEnvelope {
            query_hash: "q".to_string(),
            embedding_input_hash: "e".to_string(),
            embedding_hash: "h".to_string(),
            embedder_id: "id".to_string(),
            embedder_settings_hash: "s".to_string(),
            corpus_fingerprint: "c".to_string(),
            policy_snapshot_hash: "p".to_string(),
            purpose: RecallUse::RuntimeAugmentation,
            consumers: vec![RecallConsumer::Kernel],
            candidate_scores: vec![],
            candidates_searched: 10,
            candidates_returned: 2,
            stop_reason: None,
            latency_ms: 10,
            timestamp: "t".to_string(),
            signature: "unsigned".to_string(),
        };

        // Same envelope matches
        assert!(env1.matches_for_replay(&env1.clone()));

        // Different purpose does not match
        let mut env2 = env1.clone();
        env2.purpose = RecallUse::TrainingCandidateSelection;
        assert!(
            !env1.matches_for_replay(&env2),
            "Different purpose must not match"
        );

        // Different consumers does not match
        let mut env3 = env1.clone();
        env3.consumers = vec![RecallConsumer::Trainer];
        assert!(
            !env1.matches_for_replay(&env3),
            "Different consumers must not match"
        );
    }

    /// Test that policy snapshot hash changes when allowed_uses changes.
    #[test]
    fn test_policy_hash_includes_allowed_uses() {
        let policy1 = RecallPolicy::default();
        let policy2 = RecallPolicy {
            allowed_uses: vec![
                RecallUse::RuntimeAugmentation,
                RecallUse::TrainingCandidateSelection,
            ],
            ..Default::default()
        };

        let hash1 = policy1.snapshot_hash();
        let hash2 = policy2.snapshot_hash();

        assert_ne!(
            hash1, hash2,
            "Different allowed_uses must produce different policy hash"
        );
    }
}
