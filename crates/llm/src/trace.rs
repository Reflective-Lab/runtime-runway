// Copyright 2024-2026 Reflective Labs

//! Decision traces for multi-agent chain observability.
//!
//! This module provides instrumentation for debugging and analyzing
//! agent decision chains. It is NOT memory—it is debug infrastructure.
//!
//! # Purpose
//!
//! Decision traces let you:
//! - See exactly where decisions collapse
//! - Identify which contracts are too loose or too tight
//! - Extract training data for future LoRA
//! - Reproduce and bisect failures
//!
//! # Architecture
//!
//! ```text
//! DecisionChain (one per user request)
//!   └── DecisionTrace (one per agent step)
//!         ├── input_state: what the agent saw
//!         ├── raw_output: what the LLM produced
//!         ├── validation: did it pass the contract?
//!         └── envelope_id: how to reproduce
//! ```
//!
//! # Usage
//!
//! ```ignore
//! let mut chain = DecisionChain::new("req-001");
//!
//! // Run reasoning step
//! let trace = DecisionTrace::new(DecisionStep::Reasoning)
//!     .with_input_state(&state)
//!     .with_envelope(&envelope)
//!     .with_prompt_version(&stack.version);
//!
//! let result = engine.run(&stack, &envelope)?;
//! let validation = validate_output(&result.text, &contract);
//!
//! let trace = trace.complete(result.text, validation);
//! chain.add_trace(trace);
//!
//! if !chain.last_valid() {
//!     return chain.fail_at(DecisionStep::Reasoning);
//! }
//! ```

use crate::inference::InferenceEnvelope;
use crate::prompt::{PromptVersion, StateInjection};
use crate::validation::ValidationResult;
use serde::{Deserialize, Serialize};
use std::time::Instant;

// Re-export DecisionStep from converge-core for backward compatibility
pub use converge_core::DecisionStep;

/// A single step in a multi-agent decision chain.
///
/// Captures everything needed to understand and reproduce
/// what happened at one decision point.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionTrace {
    /// Unique identifier for this trace
    pub trace_id: String,

    /// Which step in the chain
    pub step: DecisionStep,

    /// Input state summary (what the agent saw)
    pub input_state: StateSummary,

    /// Raw LLM output (before validation)
    pub raw_output: String,

    /// Validation result
    pub validation: ValidationResult,

    /// The envelope ID used (for reproduction)
    pub envelope_id: String,

    /// Prompt version (for bisection)
    pub prompt_version: String,

    /// The contract type that was applied
    pub contract_type: String,

    /// When this step started (ISO 8601)
    pub started_at: String,

    /// When this step completed (ISO 8601)
    pub completed_at: String,

    /// Generation time in milliseconds
    pub generation_time_ms: u64,

    /// Recall metadata (if recall was used for this step)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recall_metadata: Option<RecallMetadata>,

    /// Adapter metadata (if a LoRA adapter was used for this step)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub adapter_metadata: Option<AdapterMetadata>,
}

/// Metadata about recall operations for a trace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecallMetadata {
    /// Whether recall was requested for this step
    pub recall_requested: bool,
    /// Whether recall was actually performed
    pub recall_performed: bool,
    /// Number of candidates returned
    pub candidates_returned: usize,
    /// Recall trace link for reproducibility (lightweight)
    pub trace_link: Option<crate::recall::RecallTraceLink>,
    /// IDs of candidates that were injected
    pub injected_candidate_ids: Vec<String>,
    /// Full provenance envelope for audit/replay
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance: Option<crate::recall::RecallProvenanceEnvelope>,
    /// Impact assessment of recall on this step (lightweight signal)
    ///
    /// This does NOT judge recall as "correct" - it only measures
    /// whether recall appears to help convergence control.
    #[serde(default)]
    pub impact: crate::kernel::RecallImpact,
    /// Whether the embedder used is bit-exact deterministic.
    ///
    /// If false, recall cannot be exactly replayed, which propagates
    /// to proposal replayability downgrade.
    #[serde(default)]
    pub embedder_deterministic: bool,
    /// Whether the corpus has content-derived hash.
    ///
    /// If false, we cannot verify exact corpus state at replay time.
    #[serde(default)]
    pub corpus_content_addressed: bool,
}

/// Metadata about LoRA adapter usage for a trace.
///
/// This captures everything needed to:
/// - Verify which adapter was used
/// - Reproduce the exact model configuration
/// - Audit adapter application
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdapterMetadata {
    /// The canonical adapter ID (namespace/name@version+sha256:hash)
    pub adapter_id: String,
    /// Base model identifier
    pub base_model_id: String,
    /// Whether weights were actually merged
    pub weights_merged: bool,
    /// List of tensors that were modified (sorted, deterministic order)
    pub affected_tensors: Vec<String>,
    /// Blake3 hashes of the delta values applied (for determinism verification)
    pub delta_hashes: Vec<String>,
    /// Composite hash of all deltas (single hash for quick comparison)
    pub merge_hash: String,
    /// LoRA rank used
    pub rank: usize,
    /// LoRA alpha used
    pub alpha: f32,
}

impl AdapterMetadata {
    /// Create adapter metadata from a merge report.
    #[cfg(feature = "llama3")]
    #[must_use]
    pub fn from_merge_report(
        adapter_id: &str,
        report: &crate::engine::MergeReport,
        rank: usize,
        alpha: f32,
    ) -> Self {
        // Compute composite merge hash from all delta hashes
        let merge_hash = if report.delta_hashes.is_empty() {
            "no-deltas".to_string()
        } else {
            use blake3::Hasher;
            let mut hasher = Hasher::new();
            for hash in &report.delta_hashes {
                hasher.update(hash.as_bytes());
            }
            hasher.finalize().to_hex()[..16].to_string()
        };

        Self {
            adapter_id: adapter_id.to_string(),
            base_model_id: report.base_model_id.clone(),
            weights_merged: report.weights_mutated,
            affected_tensors: report.affected_tensors.clone(),
            delta_hashes: report.delta_hashes.clone(),
            merge_hash,
            rank,
            alpha,
        }
    }

    /// Check if this adapter metadata matches another for replay purposes.
    #[must_use]
    pub fn matches_for_replay(&self, other: &Self) -> bool {
        self.adapter_id == other.adapter_id
            && self.base_model_id == other.base_model_id
            && self.merge_hash == other.merge_hash
    }
}

/// Builder for creating decision traces.
///
/// Use this to construct traces incrementally as you run inference.
#[derive(Debug)]
pub struct DecisionTraceBuilder {
    trace_id: String,
    step: DecisionStep,
    input_state: StateSummary,
    envelope_id: String,
    prompt_version: String,
    contract_type: String,
    started_at: Instant,
    started_at_iso: String,
    recall_metadata: Option<RecallMetadata>,
    adapter_metadata: Option<AdapterMetadata>,
}

impl DecisionTraceBuilder {
    /// Start building a new trace for the given step.
    #[must_use]
    pub fn new(step: DecisionStep) -> Self {
        Self {
            trace_id: generate_trace_id(),
            step,
            input_state: StateSummary::empty(),
            envelope_id: String::new(),
            prompt_version: String::new(),
            contract_type: String::new(),
            started_at: Instant::now(),
            started_at_iso: timestamp_now(),
            recall_metadata: None,
            adapter_metadata: None,
        }
    }

    /// Record what state the agent will see.
    #[must_use]
    pub fn with_input_state(mut self, state: &StateInjection) -> Self {
        self.input_state = StateSummary::from_state(state);
        self
    }

    /// Record which envelope is being used.
    #[must_use]
    pub fn with_envelope(mut self, envelope: &InferenceEnvelope) -> Self {
        self.envelope_id = format!(
            "{}:{}",
            envelope.prompt_version,
            match &envelope.seed_policy {
                crate::inference::SeedPolicy::Fixed(s) => format!("seed:{s}"),
                crate::inference::SeedPolicy::Random => "random".to_string(),
                crate::inference::SeedPolicy::InputDerived => "input-derived".to_string(),
            }
        );
        self
    }

    /// Record the prompt version.
    #[must_use]
    pub fn with_prompt_version(mut self, version: &PromptVersion) -> Self {
        self.prompt_version = version.to_string();
        self
    }

    /// Record the contract type being validated against.
    #[must_use]
    pub fn with_contract_type(mut self, contract_type: impl Into<String>) -> Self {
        self.contract_type = contract_type.into();
        self
    }

    /// Record recall metadata.
    #[must_use]
    pub fn with_recall_metadata(mut self, metadata: RecallMetadata) -> Self {
        self.recall_metadata = Some(metadata);
        self
    }

    /// Record adapter metadata for LoRA adapter usage.
    #[must_use]
    pub fn with_adapter_metadata(mut self, metadata: AdapterMetadata) -> Self {
        self.adapter_metadata = Some(metadata);
        self
    }

    /// Complete the trace with the output and validation result.
    #[must_use]
    pub fn complete(self, raw_output: String, validation: ValidationResult) -> DecisionTrace {
        let elapsed = self.started_at.elapsed();

        DecisionTrace {
            trace_id: self.trace_id,
            step: self.step,
            input_state: self.input_state,
            raw_output,
            validation,
            envelope_id: self.envelope_id,
            prompt_version: self.prompt_version,
            contract_type: self.contract_type,
            started_at: self.started_at_iso,
            completed_at: timestamp_now(),
            generation_time_ms: elapsed.as_millis() as u64,
            recall_metadata: self.recall_metadata,
            adapter_metadata: self.adapter_metadata,
        }
    }
}

impl DecisionTrace {
    /// Check if this trace passed validation.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.validation.valid
    }

    /// Get a short summary for logging.
    #[must_use]
    pub fn summary(&self) -> String {
        let status = if self.is_valid() { "✓" } else { "✗" };
        format!(
            "[{}] {:?} {} ({}ms)",
            status, self.step, self.contract_type, self.generation_time_ms
        )
    }
}

// Note: DecisionStep is now re-exported from converge_core at the top of this file

/// Compressed view of what the agent received.
///
/// This is NOT the full state—it's a summary for debugging.
/// The full state is reconstructable from the chain context.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateSummary {
    /// Names of scalar values provided
    pub scalar_keys: Vec<String>,

    /// Names of list values provided
    pub list_keys: Vec<String>,

    /// Number of records provided
    pub record_count: usize,

    /// Estimated token count (for budget tracking)
    pub estimated_tokens: usize,
}

impl StateSummary {
    /// Create an empty summary.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            scalar_keys: vec![],
            list_keys: vec![],
            record_count: 0,
            estimated_tokens: 0,
        }
    }

    /// Create a summary from a StateInjection.
    #[must_use]
    pub fn from_state(state: &StateInjection) -> Self {
        let scalar_keys: Vec<String> = state.scalars.keys().cloned().collect();
        let list_keys: Vec<String> = state.lists.keys().cloned().collect();
        let record_count = state.records.len();

        // Rough token estimate: ~4 chars per token
        let char_count = state.render().len();
        let estimated_tokens = (char_count + 3) / 4;

        Self {
            scalar_keys,
            list_keys,
            record_count,
            estimated_tokens,
        }
    }
}

/// A complete chain execution.
///
/// One chain = one user request = multiple agent steps.
/// Chains are immutable after completion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionChain {
    /// Chain execution ID
    pub chain_id: String,

    /// All steps in order
    pub traces: Vec<DecisionTrace>,

    /// Did the chain complete successfully?
    pub completed: bool,

    /// If failed, which step?
    pub failed_at: Option<DecisionStep>,

    /// Final output (if completed)
    pub final_output: Option<String>,

    /// When the chain started
    pub started_at: String,

    /// When the chain ended
    pub ended_at: Option<String>,
}

impl DecisionChain {
    /// Create a new chain.
    #[must_use]
    pub fn new(chain_id: impl Into<String>) -> Self {
        Self {
            chain_id: chain_id.into(),
            traces: vec![],
            completed: false,
            failed_at: None,
            final_output: None,
            started_at: timestamp_now(),
            ended_at: None,
        }
    }

    /// Add a trace to the chain.
    pub fn add_trace(&mut self, trace: DecisionTrace) {
        self.traces.push(trace);
    }

    /// Check if the last trace passed validation.
    #[must_use]
    pub fn last_valid(&self) -> bool {
        self.traces.last().is_some_and(|t| t.is_valid())
    }

    /// Mark the chain as failed at a specific step.
    pub fn fail_at(&mut self, step: DecisionStep) {
        self.completed = false;
        self.failed_at = Some(step);
        self.ended_at = Some(timestamp_now());
    }

    /// Mark the chain as successfully completed.
    pub fn complete_with(&mut self, final_output: String) {
        self.completed = true;
        self.failed_at = None;
        self.final_output = Some(final_output);
        self.ended_at = Some(timestamp_now());
    }

    /// Get total generation time across all steps.
    #[must_use]
    pub fn total_generation_time_ms(&self) -> u64 {
        self.traces.iter().map(|t| t.generation_time_ms).sum()
    }

    /// Get a summary of the chain for logging.
    #[must_use]
    pub fn summary(&self) -> String {
        let status = if self.completed {
            "COMPLETED"
        } else if self.failed_at.is_some() {
            "FAILED"
        } else {
            "IN_PROGRESS"
        };

        let steps: Vec<String> = self.traces.iter().map(|t| t.summary()).collect();

        format!(
            "Chain {} [{}] ({}ms total)\n  {}",
            self.chain_id,
            status,
            self.total_generation_time_ms(),
            steps.join("\n  ")
        )
    }

    /// Get all failed validations in the chain.
    #[must_use]
    pub fn failures(&self) -> Vec<&DecisionTrace> {
        self.traces.iter().filter(|t| !t.is_valid()).collect()
    }

    /// Check if the chain can be used for training data.
    ///
    /// Only completed chains with all valid steps are suitable.
    #[must_use]
    pub fn is_training_candidate(&self) -> bool {
        self.completed && self.traces.iter().all(|t| t.is_valid())
    }
}

/// Generate a unique trace ID.
fn generate_trace_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);

    let count = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("trace-{count:06}")
}

/// Generate current timestamp in ISO 8601 format.
fn timestamp_now() -> String {
    // Placeholder - in production use chrono
    "2026-01-16T00:00:00Z".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prompt::{OutputContract, ScoreCardinality, StepFormat};
    use crate::validation::validate_output;

    #[test]
    fn test_decision_step_contract_mapping() {
        assert_eq!(DecisionStep::Reasoning.expected_contract(), "Reasoning");
        assert_eq!(DecisionStep::Evaluation.expected_contract(), "Evaluation");
        assert_eq!(DecisionStep::Planning.expected_contract(), "Planning");
    }

    #[test]
    fn test_state_summary_from_state() {
        let state = StateInjection::new()
            .with_scalar("mae", 0.15)
            .with_scalar("success_ratio", 0.85)
            .with_list("features", vec!["a".into(), "b".into()]);

        let summary = StateSummary::from_state(&state);

        assert_eq!(summary.scalar_keys.len(), 2);
        assert_eq!(summary.list_keys.len(), 1);
        assert!(summary.estimated_tokens > 0);
    }

    #[test]
    fn test_decision_chain_lifecycle() {
        let mut chain = DecisionChain::new("test-chain-001");

        // Simulate a successful reasoning step
        let contract = OutputContract::Reasoning {
            requires_conclusion: true,
            allows_uncertainty: true,
            max_steps: Some(5),
            step_format: StepFormat::Loose,
        };

        let output = "Step 1: Based on the data analysis. Step 2: The metrics show improvement. CONCLUSION: The trend is positive.";
        let validation = validate_output(output, &contract);

        let trace = DecisionTraceBuilder::new(DecisionStep::Reasoning)
            .with_contract_type("Reasoning")
            .complete(output.to_string(), validation);

        chain.add_trace(trace);

        assert!(chain.last_valid());
        assert_eq!(chain.traces.len(), 1);
    }

    #[test]
    fn test_chain_failure_tracking() {
        let mut chain = DecisionChain::new("test-chain-002");

        // Simulate a failed evaluation step
        let contract = OutputContract::Evaluation {
            score_range: (0.0, 1.0),
            confidence_required: true,
            justification_required: false,
            cardinality: ScoreCardinality::AtLeast(1),
            grounding_refs: vec![],
        };

        let output = "This is not a valid evaluation output";
        let validation = validate_output(output, &contract);

        let trace = DecisionTraceBuilder::new(DecisionStep::Evaluation)
            .with_contract_type("Evaluation")
            .complete(output.to_string(), validation);

        chain.add_trace(trace);

        assert!(!chain.last_valid());

        chain.fail_at(DecisionStep::Evaluation);

        assert!(!chain.completed);
        assert_eq!(chain.failed_at, Some(DecisionStep::Evaluation));
        assert!(!chain.is_training_candidate());
    }

    #[test]
    fn test_complete_chain_is_training_candidate() {
        let mut chain = DecisionChain::new("test-chain-003");

        // Add valid reasoning
        let contract = OutputContract::Reasoning {
            requires_conclusion: true,
            allows_uncertainty: false,
            max_steps: None,
            step_format: StepFormat::Loose,
        };
        let output = "Step 1: Reviewing all available data. Step 2: No anomalies detected. CONCLUSION: Analysis complete.";
        let validation = validate_output(output, &contract);
        let trace = DecisionTraceBuilder::new(DecisionStep::Reasoning)
            .with_contract_type("Reasoning")
            .complete(output.to_string(), validation);
        chain.add_trace(trace);

        // Add valid evaluation
        let contract = OutputContract::Evaluation {
            score_range: (0.0, 1.0),
            confidence_required: true,
            justification_required: false,
            cardinality: ScoreCardinality::AtLeast(1),
            grounding_refs: vec![],
        };
        let output = "Score: 0.85 with confidence 0.9";
        let validation = validate_output(output, &contract);
        let trace = DecisionTraceBuilder::new(DecisionStep::Evaluation)
            .with_contract_type("Evaluation")
            .complete(output.to_string(), validation);
        chain.add_trace(trace);

        // Complete the chain
        chain.complete_with("Final strategy output".to_string());

        assert!(chain.completed);
        assert!(chain.is_training_candidate());
        assert!(chain.failures().is_empty());
    }

    #[test]
    fn test_trace_summary() {
        let contract = OutputContract::Reasoning {
            requires_conclusion: true,
            allows_uncertainty: false,
            max_steps: None,
            step_format: StepFormat::Loose,
        };
        let output = "Step 1: Processed all inputs successfully. CONCLUSION: Done.";
        let validation = validate_output(output, &contract);

        let trace = DecisionTraceBuilder::new(DecisionStep::Reasoning)
            .with_contract_type("Reasoning")
            .complete(output.to_string(), validation);

        let summary = trace.summary();
        assert!(summary.contains("Reasoning"));
        assert!(summary.contains("✓"));
    }
}
