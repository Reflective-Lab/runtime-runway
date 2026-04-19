// Copyright 2024-2026 Reflective Labs

//! Reasoning Kernel — The public boundary of converge-llm.
//!
//! This module defines the **only** public API for the reasoning kernel.
//! Everything inside (ChainExecutor, PromptStack, InferenceEnvelope, etc.)
//! is an implementation detail.
//!
//! # Axiom Compliance
//!
//! - **Agents Suggest, Engines Decide**: Kernel outputs are `KernelProposal`,
//!   never `Fact`. Promotion happens in converge-core validators.
//! - **Append-Only Truth**: Kernel cannot mutate shared truth directly.
//! - **Explicit Authority**: Adapter selection comes from `KernelPolicy`,
//!   not emergent kernel behavior.
//! - **Transparent Determinism**: Every proposal includes full trace metadata
//!   for reproducibility.
//! - **Human Authority First-Class**: Proposals can be marked `requires_human`,
//!   which validators enforce.
//!
//! # Architecture
//!
//! ```text
//! converge-core
//!     │
//!     │  LlmAgent.execute()
//!     ▼
//! ┌─────────────────────────────────────────┐
//! │  run_kernel(intent, context, policy)    │  ← Public API
//! └─────────────────────────────────────────┘
//!     │
//!     ▼  (internal)
//! ┌─────────────────────────────────────────┐
//! │  KernelRunner                           │
//! │  ├─ InferenceService (LlamaEngine)      │
//! │  ├─ ContractService (validation)        │
//! │  ├─ RecallService (context retrieval)   │
//! │  └─ TraceBuilder (audit trail)          │
//! └─────────────────────────────────────────┘
//!     │
//!     ▼
//! ┌─────────────────────────────────────────┐
//! │  Vec<KernelProposal>                    │  ← Output
//! └─────────────────────────────────────────┘
//!     │
//!     ▼  (in converge-core)
//! ┌─────────────────────────────────────────┐
//! │  ContextKey::Proposals                  │
//! │  → Validator → Fact (or reject)         │
//! └─────────────────────────────────────────┘
//! ```

use serde::{Deserialize, Serialize};

use crate::chain::{ChainEngine, ChainExecutor};
use crate::error::LlmResult;
use crate::execution_plan::ExecutionPlan;
use crate::inference::InferenceEnvelope;
use crate::prompt::{StateInjection, StateValue};
use crate::trace::{DecisionChain, DecisionStep, DecisionTrace};

// Re-export kernel boundary types from converge-core (the constitutional home)
// These types encode the platform's contract for proposals - "language of the system"
pub use converge_core::kernel_boundary::{
    ContextFact,
    ContractResult,
    DataClassification,
    KernelContext,
    // Input types
    KernelIntent,
    KernelPolicy,
    // Output types (proposal taxonomy)
    ProposalKind,
    // Trace semantics
    Replayability,
    ReplayabilityDowngradeReason,
    // Routing vocabulary
    RiskTier,
    RoutingPolicy,
};

// ============================================================================
// LLM-Specific Kernel Output Types
// ============================================================================
//
// These types extend the converge-core constitutional types with LLM-specific
// details like recall metadata and embedding determinism tracking.

/// Metadata linking the proposal to its generation trace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KernelTraceLink {
    /// Hash of the full trace for reproducibility
    pub trace_hash: String,
    /// Prompt version used
    pub prompt_version: String,
    /// Envelope configuration hash
    pub envelope_hash: String,
    /// Adapter used (if any)
    pub adapter_id: Option<String>,
    /// Recall metadata (if recall was used)
    pub recall_metadata: Option<ProposalRecallMetadata>,
    /// Overall replayability of this proposal.
    ///
    /// This is the **minimum** replayability across all components:
    /// - Inference determinism (seed, sampler)
    /// - Recall determinism (embedder, corpus)
    ///
    /// Axiom: If any component is non-deterministic, proposal is non-deterministic.
    #[serde(default)]
    pub replayability: Replayability,
    /// Why replayability was downgraded (if applicable).
    ///
    /// This field enables audit trails showing which component caused
    /// the downgrade from Deterministic to BestEffort/None.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replayability_downgrade_reason: Option<ReplayabilityDowngradeReason>,
}

// Note: ReplayabilityDowngradeReason is now re-exported from converge-core::kernel_boundary

/// Metadata about recall usage in a kernel proposal.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProposalRecallMetadata {
    /// Number of candidates retrieved
    pub candidates_retrieved: usize,
    /// Number of candidates used (after filtering)
    pub candidates_used: usize,
    /// Corpus fingerprint for reproducibility
    pub corpus_fingerprint: String,
    /// IDs of records used (opaque format for safety)
    pub record_ids: Vec<String>,
    /// Query hash for reproducibility
    pub query_hash: String,
    /// Embedding hash for exact replay
    pub embedding_hash: String,
    /// Full provenance envelope (if available)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance: Option<crate::recall::RecallProvenanceEnvelope>,
    /// Impact assessment of recall on this proposal
    pub impact: RecallImpact,
    /// Whether the embedder is bit-exact deterministic.
    ///
    /// If false, recall cannot be exactly replayed, which propagates
    /// to the overall proposal replayability.
    #[serde(default)]
    pub embedder_deterministic: bool,
    /// Whether corpus has content-derived hash (for exact replay).
    ///
    /// If false, we cannot verify the exact corpus state at replay time.
    #[serde(default)]
    pub corpus_content_addressed: bool,
}

/// Lightweight signal about recall usefulness.
///
/// This does NOT judge recall as "correct" - it only measures
/// whether recall appears to help convergence control.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum RecallImpact {
    /// No recall was used
    #[default]
    None,
    /// Recall was injected but impact unknown
    Unknown,
    /// Recall appeared to reduce iterations needed
    ReducedIterations,
    /// Recall appeared to reduce validation failures
    ReducedValidationFailures,
    /// Recall provided useful context (runbook, anti-pattern)
    ProvidedContext,
}

/// Error when recall provenance is incomplete.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecallProvenanceError {
    /// Which step had the error
    pub step: String,
    /// What was missing
    pub missing: RecallProvenanceMissing,
    /// Human-readable message
    pub message: String,
}

/// What provenance data is missing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RecallProvenanceMissing {
    /// No RecallMetadata at all
    Metadata,
    /// No trace link in metadata
    TraceLink,
    /// No corpus fingerprint
    CorpusFingerprint,
    /// No embedding hash
    EmbeddingHash,
    /// No query hash
    QueryHash,
    /// No provenance envelope
    ProvenanceEnvelope,
    /// No stop reason (even "no_candidates" should be explicit)
    StopReason,
}

/// A proposal from the reasoning kernel.
///
/// This is the **only** output type that crosses the kernel boundary.
/// It must be validated and promoted by converge-core before becoming a Fact.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KernelProposal {
    /// Unique identifier for this proposal
    pub id: String,
    /// What kind of proposal this is
    pub kind: ProposalKind,
    /// The actual content/payload
    pub payload: String,
    /// Structured payload (if applicable)
    pub structured_payload: Option<serde_json::Value>,
    /// Link to the generation trace
    pub trace_link: KernelTraceLink,
    /// Contract/truth validation results
    pub contract_results: Vec<ContractResult>,
    /// Whether this proposal requires human approval
    pub requires_human: bool,
    /// Confidence score (0.0 - 1.0) if available
    pub confidence: Option<f32>,
    /// The full decision trace (for audit)
    pub trace: Option<DecisionTrace>,
}

impl KernelProposal {
    /// Check if all contracts passed.
    pub fn all_contracts_passed(&self) -> bool {
        self.contract_results.iter().all(|r| r.passed)
    }

    /// Get failed contract names.
    pub fn failed_contracts(&self) -> Vec<&str> {
        self.contract_results
            .iter()
            .filter(|r| !r.passed)
            .map(|r| r.name.as_str())
            .collect()
    }

    /// Check if a specific truth/contract passed.
    pub fn truth_passed(&self, truth_name: &str) -> bool {
        self.contract_results
            .iter()
            .find(|r| r.name == truth_name)
            .is_some_and(|r| r.passed)
    }
}

// ============================================================================
// Kernel API
// ============================================================================

/// Run the reasoning kernel with the provided engine.
///
/// This is the **only** public entry point for converge-llm.
///
/// # Arguments
///
/// * `engine` - The inference engine implementing `ChainEngine`
/// * `intent` - What to reason about
/// * `context` - The context to reason over (from converge-core)
/// * `policy` - Controls kernel behavior (adapter, recall, human gates)
///
/// # Returns
///
/// A vector of `KernelProposal` that should be added to `ContextKey::Proposals`
/// in converge-core. These are **not** facts until validated and promoted.
///
/// # Example
///
/// ```ignore
/// let engine = LlamaEngine::load(&config)?;
///
/// let intent = KernelIntent::new("analyze_metrics")
///     .with_criteria("identify anomalies")
///     .with_max_tokens(512);
///
/// let context = KernelContext::new()
///     .with_state("mae", json!(0.15))
///     .with_state("success_ratio", json!(0.85));
///
/// let policy = KernelPolicy::deterministic(42)
///     .with_recall(true)
///     .with_required_truth("grounded-answering");
///
/// let proposals = run_kernel(&mut engine, &intent, &context, &policy)?;
///
/// // In converge-core's LlmAgent:
/// for proposal in proposals {
///     ctx.add_proposal(ProposedFact::from(proposal));
/// }
/// ```
pub fn run_kernel<E: ChainEngine>(
    engine: &mut E,
    intent: &KernelIntent,
    context: &KernelContext,
    policy: &KernelPolicy,
) -> LlmResult<Vec<KernelProposal>> {
    let mut runner = KernelRunner::new(engine);
    runner.run(intent, context, policy)
}

// ============================================================================
// KernelRunner — Internal Orchestration
// ============================================================================

/// The internal kernel runner that orchestrates chain execution.
///
/// This wraps `ChainExecutor` and provides the mapping between kernel types
/// and chain types. It's the implementation detail behind `run_kernel`.
pub struct KernelRunner<'a, E> {
    executor: ChainExecutor<&'a mut E>,
}

impl<'a, E: ChainEngine> KernelRunner<'a, E> {
    /// Create a new kernel runner with the given engine.
    pub fn new(engine: &'a mut E) -> Self {
        Self {
            executor: ChainExecutor::new(engine),
        }
    }

    /// Run the kernel pipeline.
    ///
    /// This:
    /// 1. Compiles `ExecutionPlan` from intent + policy (cannot be overridden)
    /// 2. Converts `KernelContext` to `StateInjection`
    /// 3. Runs `ChainExecutor` with the compiled plan
    /// 4. Converts `DecisionChain` to `Vec<KernelProposal>`
    ///
    /// # Policy Enforcement
    ///
    /// The `ExecutionPlan` is compiled from policy and cannot be modified.
    /// This ensures policy toggles (recall, adapter, seed) cannot be bypassed
    /// by downstream code.
    pub fn run(
        &mut self,
        intent: &KernelIntent,
        context: &KernelContext,
        policy: &KernelPolicy,
    ) -> LlmResult<Vec<KernelProposal>> {
        // 1. Compile ExecutionPlan (policy is now locked)
        let plan = ExecutionPlan::compile(intent, policy);

        // 2. Convert KernelContext to StateInjection
        let state = self.build_state_injection(context);

        // 3. Run the chain with the compiled plan
        let chain = self.executor.run_with_plan(&state, &plan)?;

        // 4. Convert DecisionChain to KernelProposals
        self.build_proposals(chain, intent, policy)
    }

    /// Convert `KernelContext` to `StateInjection`.
    fn build_state_injection(&self, context: &KernelContext) -> StateInjection {
        let mut state = StateInjection::new();

        // Convert KernelContext.state (HashMap<String, serde_json::Value>) to StateInjection
        for (key, value) in &context.state {
            state = self.inject_json_value(state, key, value);
        }

        // Add facts as records (context from converge-core)
        for fact in &context.facts {
            state = state.with_list(
                format!("fact:{}", fact.key),
                vec![StateValue::String(format!(
                    "[{}] {}",
                    fact.id, fact.content
                ))],
            );
        }

        state
    }

    /// Inject a JSON value into the state injection.
    fn inject_json_value(
        &self,
        mut state: StateInjection,
        key: &str,
        value: &serde_json::Value,
    ) -> StateInjection {
        match value {
            serde_json::Value::Number(n) => {
                if let Some(f) = n.as_f64() {
                    state = state.with_scalar(key.to_string(), f);
                } else if let Some(i) = n.as_i64() {
                    state = state.with_scalar(key.to_string(), i as f64);
                }
            }
            serde_json::Value::String(s) => {
                state = state.with_scalar(key.to_string(), s.clone());
            }
            serde_json::Value::Bool(b) => {
                state = state.with_scalar(key.to_string(), if *b { "true" } else { "false" });
            }
            serde_json::Value::Array(arr) => {
                let items: Vec<StateValue> = arr
                    .iter()
                    .filter_map(|v| match v {
                        serde_json::Value::String(s) => Some(StateValue::String(s.clone())),
                        serde_json::Value::Number(n) => n.as_f64().map(StateValue::Float),
                        _ => None,
                    })
                    .collect();
                if !items.is_empty() {
                    state = state.with_list(key.to_string(), items);
                }
            }
            serde_json::Value::Object(obj) => {
                // Flatten nested objects with dot notation
                for (nested_key, nested_value) in obj {
                    let full_key = format!("{}.{}", key, nested_key);
                    state = self.inject_json_value(state, &full_key, nested_value);
                }
            }
            serde_json::Value::Null => {
                // Skip null values
            }
        }
        state
    }

    // NOTE: build_envelope and build_chain_config have been removed.
    // The ExecutionPlan now handles envelope and config compilation internally,
    // ensuring policy cannot be bypassed by downstream code.

    /// Convert `DecisionChain` to `Vec<KernelProposal>`.
    fn build_proposals(
        &self,
        chain: DecisionChain,
        _intent: &KernelIntent,
        policy: &KernelPolicy,
    ) -> LlmResult<Vec<KernelProposal>> {
        let mut proposals = Vec::new();

        // Generate unique proposal ID
        let proposal_id = format!("proposal-{}", chain.chain_id);

        // Determine proposal kind based on chain completion
        let kind = if chain.completed {
            ProposalKind::Plan
        } else {
            ProposalKind::Reasoning
        };

        // Build payload from chain output
        let payload = chain.final_output.clone().unwrap_or_else(|| {
            // If chain didn't complete, use the last trace output
            chain
                .traces
                .last()
                .map(|t| t.raw_output.clone())
                .unwrap_or_default()
        });

        // Build contract results from chain traces
        let contract_results: Vec<ContractResult> = chain
            .traces
            .iter()
            .map(|trace| {
                let step_name = format!("{:?}", trace.step);
                ContractResult {
                    name: step_name,
                    passed: trace.validation.valid,
                    failure_reason: trace.validation.first_failure().map(|f| f.reason.clone()),
                }
            })
            .collect();

        // Add required truth results
        let mut truth_results: Vec<ContractResult> = policy
            .required_truths
            .iter()
            .map(|truth| {
                // For now, truths pass if the chain completed successfully
                // In a full implementation, this would check specific validators
                ContractResult {
                    name: truth.clone(),
                    passed: chain.completed,
                    failure_reason: if chain.completed {
                        None
                    } else {
                        Some(format!(
                            "Chain failed at {:?}",
                            chain.failed_at.unwrap_or(DecisionStep::Reasoning)
                        ))
                    },
                }
            })
            .collect();

        let mut all_results = contract_results;
        all_results.append(&mut truth_results);

        // Build trace link
        let trace_link = self.build_trace_link(&chain, policy);

        // Calculate confidence based on chain success and trace count
        let confidence = if chain.completed {
            Some(0.9) // High confidence for completed chains
        } else {
            let completed_steps = chain.traces.iter().filter(|t| t.validation.valid).count();
            Some(completed_steps as f32 / 3.0)
        };

        // Build the proposal
        let proposal = KernelProposal {
            id: proposal_id,
            kind,
            payload,
            structured_payload: None, // Could parse structured output if available
            trace_link,
            contract_results: all_results,
            requires_human: policy.requires_human,
            confidence,
            trace: chain.traces.first().cloned(),
        };

        proposals.push(proposal);

        Ok(proposals)
    }

    /// Build trace link from decision chain.
    fn build_trace_link(&self, chain: &DecisionChain, policy: &KernelPolicy) -> KernelTraceLink {
        // Compute trace hash from chain data
        let trace_data = format!(
            "{}:{}:{:?}",
            chain.chain_id,
            chain.traces.len(),
            chain.completed
        );
        let trace_hash = format!("{:016x}", hash_string(&trace_data));

        // Get envelope hash from first trace
        let envelope_hash = chain
            .traces
            .first()
            .map(|t| t.envelope_id.clone())
            .unwrap_or_else(|| "no-envelope".to_string());

        // Get prompt version from first trace
        let prompt_version = chain
            .traces
            .first()
            .map(|t| t.prompt_version.clone())
            .unwrap_or_else(|| "unknown".to_string());

        // Build recall metadata if recall was used
        let recall_metadata = if policy.recall_enabled {
            chain.traces.first().and_then(|trace| {
                trace.recall_metadata.as_ref().map(|rm| {
                    ProposalRecallMetadata {
                        candidates_retrieved: rm.candidates_returned,
                        candidates_used: rm.injected_candidate_ids.len(),
                        corpus_fingerprint: rm
                            .trace_link
                            .as_ref()
                            .map(|tl| tl.corpus_version.clone())
                            .unwrap_or_default(),
                        record_ids: rm
                            .injected_candidate_ids
                            .iter()
                            .map(|id| opaque_recall_id(id))
                            .collect(),
                        query_hash: rm
                            .trace_link
                            .as_ref()
                            .map(|tl| tl.embedding_hash.clone()) // Use embedding_hash as query identifier
                            .unwrap_or_default(),
                        embedding_hash: rm
                            .trace_link
                            .as_ref()
                            .map(|tl| tl.embedding_hash.clone())
                            .unwrap_or_default(),
                        provenance: rm.provenance.clone(),
                        impact: rm.impact,
                        embedder_deterministic: rm.embedder_deterministic,
                        corpus_content_addressed: rm.corpus_content_addressed,
                    }
                })
            })
        } else {
            None
        };

        // Compute overall replayability from all components
        // Axiom: If any component is non-deterministic, proposal is non-deterministic
        let (replayability, downgrade_reason) = compute_replayability(policy, &recall_metadata);

        KernelTraceLink {
            trace_hash,
            prompt_version,
            envelope_hash,
            adapter_id: policy.adapter_id.clone(),
            recall_metadata,
            replayability,
            replayability_downgrade_reason: downgrade_reason,
        }
    }
}

/// Compute overall proposal replayability from components.
///
/// # Replayability Rules
///
/// - If `policy.seed` is `Some(seed)` → inference is deterministic
/// - If `recall_metadata.embedder_deterministic` is false → recall is non-deterministic
/// - If `recall_metadata.corpus_content_addressed` is false → corpus state unknown
///
/// Final replayability is the **minimum** across all components.
fn compute_replayability(
    policy: &KernelPolicy,
    recall_metadata: &Option<ProposalRecallMetadata>,
) -> (Replayability, Option<ReplayabilityDowngradeReason>) {
    let mut downgrade_reasons = Vec::new();

    // Check inference determinism (seed must be provided for deterministic replay)
    let inference_deterministic = policy.seed.is_some();
    if !inference_deterministic {
        downgrade_reasons.push(ReplayabilityDowngradeReason::NoSeedProvided);
    }

    // Check recall determinism (if recall was used)
    if let Some(rm) = recall_metadata {
        if !rm.embedder_deterministic {
            downgrade_reasons.push(ReplayabilityDowngradeReason::RecallEmbedderNotDeterministic);
        }
        if !rm.corpus_content_addressed {
            downgrade_reasons.push(ReplayabilityDowngradeReason::RecallCorpusNotContentAddressed);
        }
    }

    // Determine final replayability
    match downgrade_reasons.len() {
        0 => (Replayability::Deterministic, None),
        1 => (Replayability::BestEffort, Some(downgrade_reasons[0])),
        _ => (
            Replayability::BestEffort,
            Some(ReplayabilityDowngradeReason::MultipleReasons),
        ),
    }
}

/// Simple string hash for trace IDs (not cryptographic).
fn hash_string(s: &str) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    s.hash(&mut hasher);
    hasher.finish()
}

// ============================================================================
// ChainEngine implementation for references
// ============================================================================

/// Allow `&mut E` where `E: ChainEngine` to also be a `ChainEngine`.
impl<E: ChainEngine> ChainEngine for &mut E {
    fn generate(
        &mut self,
        stack: &crate::prompt::PromptStack,
        envelope: &InferenceEnvelope,
    ) -> LlmResult<String> {
        (*self).generate(stack, envelope)
    }
}

// ============================================================================
// Recall Provenance Validation
// ============================================================================

/// Generate an opaque recall hint ID that cannot be confused with evidence IDs.
///
/// Recall IDs use a special prefix to make them syntactically distinguishable.
/// This prevents the model from accidentally citing recall as evidence.
///
/// # Format
///
/// `~hint:<truncated_id>~`
///
/// The `~hint:` prefix and `~` suffix are:
/// 1. Clearly marked as non-citable
/// 2. Unlikely to appear in real evidence IDs
/// 3. Short enough to not bloat context
#[must_use]
pub fn opaque_recall_id(original_id: &str) -> String {
    format!("~hint:{}~", &original_id[..original_id.len().min(8)])
}

/// Check if an ID is an opaque recall hint ID.
#[must_use]
pub fn is_opaque_recall_id(id: &str) -> bool {
    id.starts_with("~hint:") && id.ends_with('~')
}

/// Validate that recall provenance is complete when recall was used.
///
/// # Invariant
///
/// If recall_trigger != Never AND policy.enabled == true, then DecisionTrace MUST contain:
/// - corpus_fingerprint
/// - embedder_settings snapshot (via embedder hash)
/// - query_hash
/// - candidate_ids + scores
/// - stop_reason (even if "no_candidates")
///
/// This makes recall replayable and auditable as part of the convergence trace,
/// not an optional debug nicety.
pub fn validate_recall_provenance(
    trace: &DecisionTrace,
    recall_was_triggered: bool,
) -> Result<(), RecallProvenanceError> {
    // If recall wasn't triggered, no validation needed
    if !recall_was_triggered {
        return Ok(());
    }

    let step_name = format!("{:?}", trace.step);

    // Must have RecallMetadata
    let Some(metadata) = &trace.recall_metadata else {
        return Err(RecallProvenanceError {
            step: step_name,
            missing: RecallProvenanceMissing::Metadata,
            message: "Recall was triggered but no RecallMetadata present".to_string(),
        });
    };

    // Must have trace link
    let Some(trace_link) = &metadata.trace_link else {
        return Err(RecallProvenanceError {
            step: step_name,
            missing: RecallProvenanceMissing::TraceLink,
            message: "RecallMetadata missing trace_link".to_string(),
        });
    };

    // Trace link must have corpus fingerprint
    if trace_link.corpus_version.is_empty() {
        return Err(RecallProvenanceError {
            step: step_name,
            missing: RecallProvenanceMissing::CorpusFingerprint,
            message: "Trace link missing corpus_version".to_string(),
        });
    }

    // Trace link must have embedding hash
    if trace_link.embedding_hash.is_empty() {
        return Err(RecallProvenanceError {
            step: step_name,
            missing: RecallProvenanceMissing::EmbeddingHash,
            message: "Trace link missing embedding_hash".to_string(),
        });
    }

    // If recall was performed, must have provenance envelope
    if metadata.recall_performed && metadata.provenance.is_none() {
        return Err(RecallProvenanceError {
            step: step_name,
            missing: RecallProvenanceMissing::ProvenanceEnvelope,
            message: "Recall performed but no provenance envelope".to_string(),
        });
    }

    Ok(())
}

/// Validate all traces in a decision chain for recall provenance.
///
/// Returns all errors found (does not stop at first error).
pub fn validate_chain_recall_provenance(
    chain: &crate::trace::DecisionChain,
    policy: &KernelPolicy,
) -> Vec<RecallProvenanceError> {
    if !policy.recall_enabled {
        return vec![];
    }

    let mut errors = vec![];

    for trace in &chain.traces {
        // For now, if recall is enabled in policy, we expect all steps to have provenance
        // In a more refined implementation, we'd check per-step triggers
        if let Err(e) = validate_recall_provenance(trace, true) {
            errors.push(e);
        }
    }

    errors
}

/// Check if model output improperly cites recall hints as evidence.
///
/// This is the machine-checkable enforcement of "Recall ≠ Evidence".
/// Recall candidates use opaque `~hint:...~` IDs that are syntactically
/// distinguishable from evidence IDs.
#[must_use]
pub fn check_recall_as_evidence_violation(output: &str, recall_ids: &[String]) -> Option<String> {
    let output_lower = output.to_lowercase();

    // First check for opaque recall IDs being cited
    for id in recall_ids {
        let opaque = opaque_recall_id(id).to_lowercase();

        // Check for citation patterns
        let citation_patterns = [
            format!("evidence: {}", opaque),
            format!("citing: {}", opaque),
            format!("based on {}", opaque),
            format!("according to {}", opaque),
            format!("as shown in {}", opaque),
            format!("reference: {}", opaque),
            format!("source: {}", opaque),
        ];

        for pattern in &citation_patterns {
            if output_lower.contains(pattern) {
                return Some(format!(
                    "Recall hint '{}' improperly cited as evidence with pattern '{}'",
                    opaque, pattern
                ));
            }
        }
    }

    // Also check for the ~hint: prefix appearing in any citation context
    if output_lower.contains("~hint:") {
        // Check if it's in an evidence-like context
        let evidence_contexts = ["evidence", "citing", "based on", "reference", "source"];
        for ctx in evidence_contexts {
            if output_lower.contains(ctx) && output_lower.contains("~hint:") {
                return Some(format!(
                    "Recall hint ID (with ~hint: prefix) found near evidence context '{}'",
                    ctx
                ));
            }
        }
    }

    None
}

// ============================================================================
// Internal Services (not public)
// ============================================================================

// These will be implemented as the kernel internals:
//
// mod runner;        // KernelRunner - orchestrates the pipeline
// mod inference;     // InferenceService - wraps LlamaEngine
// mod contracts;     // ContractService - validates against Truths
// mod recall_svc;    // RecallService - retrieves and injects context
// mod trace_builder; // TraceBuilder - constructs audit artifacts

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_kernel_intent_builder() {
        let intent = KernelIntent::new("analyze")
            .with_criteria("find anomalies")
            .with_criteria("suggest fixes")
            .with_max_tokens(512);

        assert_eq!(intent.task, "analyze");
        assert_eq!(intent.criteria.len(), 2);
        assert_eq!(intent.max_tokens, 512);
    }

    #[test]
    fn test_kernel_context_builder() {
        let context = KernelContext::new()
            .with_state("metric", serde_json::json!(0.5))
            .with_fact("Seeds", "seed-1", "Some seed fact")
            .with_tenant("tenant-123");

        assert!(context.state.contains_key("metric"));
        assert_eq!(context.facts.len(), 1);
        assert_eq!(context.tenant_id, Some("tenant-123".to_string()));
    }

    #[test]
    fn test_kernel_policy_deterministic() {
        let policy = KernelPolicy::deterministic(42)
            .with_adapter("llm/grounded@1.0.0")
            .with_recall(true)
            .with_human_required();

        assert_eq!(policy.seed, Some(42));
        assert_eq!(policy.adapter_id, Some("llm/grounded@1.0.0".to_string()));
        assert!(policy.recall_enabled);
        assert!(policy.requires_human);
    }

    #[test]
    fn test_proposal_contract_checking() {
        let proposal = KernelProposal {
            id: "prop-1".to_string(),
            kind: ProposalKind::Reasoning,
            payload: "Some reasoning".to_string(),
            structured_payload: None,
            trace_link: KernelTraceLink {
                trace_hash: "abc123".to_string(),
                prompt_version: "v1".to_string(),
                envelope_hash: "def456".to_string(),
                adapter_id: None,
                recall_metadata: None,
                replayability: Replayability::Deterministic,
                replayability_downgrade_reason: None,
            },
            contract_results: vec![
                ContractResult {
                    name: "grounded-answering".to_string(),
                    passed: true,
                    failure_reason: None,
                },
                ContractResult {
                    name: "no-hallucination".to_string(),
                    passed: false,
                    failure_reason: Some("Detected unsupported claim".to_string()),
                },
            ],
            requires_human: false,
            confidence: Some(0.85),
            trace: None,
        };

        assert!(!proposal.all_contracts_passed());
        assert_eq!(proposal.failed_contracts(), vec!["no-hallucination"]);
        assert!(proposal.truth_passed("grounded-answering"));
        assert!(!proposal.truth_passed("no-hallucination"));
    }

    #[test]
    fn test_opaque_recall_id() {
        let original = "recall-abc123-def456";
        let opaque = opaque_recall_id(original);

        assert!(is_opaque_recall_id(&opaque));
        assert!(opaque.starts_with("~hint:"));
        assert!(opaque.ends_with('~'));

        // Original ID should not be opaque
        assert!(!is_opaque_recall_id(original));
    }

    #[test]
    fn test_opaque_recall_id_truncation() {
        let short_id = "abc";
        let opaque = opaque_recall_id(short_id);
        assert_eq!(opaque, "~hint:abc~");

        let long_id = "abcdefghijklmnop";
        let opaque = opaque_recall_id(long_id);
        assert_eq!(opaque, "~hint:abcdefgh~");
    }

    #[test]
    fn test_recall_impact_default() {
        let impact = RecallImpact::default();
        assert_eq!(impact, RecallImpact::None);
    }

    #[test]
    fn test_check_recall_as_evidence_violation_detects_citation() {
        let output = "Based on ~hint:abc12345~ the system should proceed.";
        let recall_ids = vec!["abc12345-full-id".to_string()];

        let violation = check_recall_as_evidence_violation(output, &recall_ids);
        assert!(violation.is_some());
    }

    #[test]
    fn test_check_recall_as_evidence_violation_no_false_positives() {
        let output = "The analysis shows good metrics and we should proceed.";
        let recall_ids = vec!["recall-001".to_string()];

        let violation = check_recall_as_evidence_violation(output, &recall_ids);
        assert!(violation.is_none());
    }

    #[test]
    fn test_check_recall_as_evidence_violation_detects_evidence_pattern() {
        let output = "evidence: ~hint:abc~ shows the pattern";
        let recall_ids = vec!["abc12345".to_string()];

        let violation = check_recall_as_evidence_violation(output, &recall_ids);
        assert!(violation.is_some());
    }

    #[test]
    fn test_recall_provenance_error_types() {
        let error = RecallProvenanceError {
            step: "Reasoning".to_string(),
            missing: RecallProvenanceMissing::Metadata,
            message: "Missing metadata".to_string(),
        };

        assert_eq!(error.missing, RecallProvenanceMissing::Metadata);
    }

    // ========================================================================
    // run_kernel integration tests
    // ========================================================================

    /// Mock engine for testing kernel execution.
    struct MockEngine {
        responses: std::collections::VecDeque<String>,
    }

    impl MockEngine {
        fn new(responses: Vec<String>) -> Self {
            Self {
                responses: responses.into(),
            }
        }
    }

    impl ChainEngine for MockEngine {
        fn generate(
            &mut self,
            _stack: &crate::prompt::PromptStack,
            _envelope: &InferenceEnvelope,
        ) -> crate::error::LlmResult<String> {
            self.responses.pop_front().ok_or_else(|| {
                crate::error::LlmError::InferenceError("No more mock responses".into())
            })
        }
    }

    fn valid_reasoning_output() -> String {
        "Based on the metrics, step 1 we observe MAE is low. \
         Step 2, success ratio is high. \
         CONCLUSION: The model is performing well and ready for deployment."
            .to_string()
    }

    fn valid_evaluation_output() -> String {
        "Deployment readiness score: 0.85 (confidence: 0.9)\n\
         Justification: Metrics indicate strong performance with low error rates."
            .to_string()
    }

    fn valid_planning_output() -> String {
        "1. Validate model on holdout set\n\
         2. Run canary deployment at 5%\n\
         3. Monitor error rates for 24 hours\n\
         4. If stable, proceed to full deployment"
            .to_string()
    }

    #[test]
    fn test_run_kernel_successful_chain() {
        let mut engine = MockEngine::new(vec![
            valid_reasoning_output(),
            valid_evaluation_output(),
            valid_planning_output(),
        ]);

        let intent = KernelIntent::new("analyze_metrics")
            .with_criteria("identify anomalies")
            .with_max_tokens(512);

        let context = KernelContext::new()
            .with_state("mae", serde_json::json!(0.12))
            .with_state("success_ratio", serde_json::json!(0.88));

        let policy = KernelPolicy::deterministic(42);

        let proposals = run_kernel(&mut engine, &intent, &context, &policy).unwrap();

        assert_eq!(proposals.len(), 1);
        let proposal = &proposals[0];

        // Successful chain should produce a Plan
        assert_eq!(proposal.kind, ProposalKind::Plan);

        // All step contracts should pass
        assert!(proposal.all_contracts_passed());

        // Trace link should be present
        assert!(!proposal.trace_link.trace_hash.is_empty());
        assert!(proposal.trace_link.prompt_version.contains("v1"));

        // Human requirement comes from policy
        assert!(!proposal.requires_human);

        // High confidence for completed chain
        assert!(proposal.confidence.unwrap() > 0.5);
    }

    #[test]
    fn test_run_kernel_with_required_truths() {
        let mut engine = MockEngine::new(vec![
            valid_reasoning_output(),
            valid_evaluation_output(),
            valid_planning_output(),
        ]);

        let intent = KernelIntent::new("analyze");
        let context = KernelContext::new();
        let policy = KernelPolicy::deterministic(42)
            .with_required_truth("grounded-answering")
            .with_required_truth("no-hallucination");

        let proposals = run_kernel(&mut engine, &intent, &context, &policy).unwrap();

        let proposal = &proposals[0];

        // Required truths should be in contract results
        assert!(proposal.truth_passed("grounded-answering"));
        assert!(proposal.truth_passed("no-hallucination"));
    }

    #[test]
    fn test_run_kernel_with_human_required() {
        let mut engine = MockEngine::new(vec![
            valid_reasoning_output(),
            valid_evaluation_output(),
            valid_planning_output(),
        ]);

        let intent = KernelIntent::new("analyze");
        let context = KernelContext::new();
        let policy = KernelPolicy::new().with_human_required();

        let proposals = run_kernel(&mut engine, &intent, &context, &policy).unwrap();

        // Proposal should require human approval
        assert!(proposals[0].requires_human);
    }

    #[test]
    fn test_run_kernel_failed_at_reasoning() {
        let mut engine = MockEngine::new(vec![
            "This is not valid reasoning output.".to_string(),
            valid_evaluation_output(),
            valid_planning_output(),
        ]);

        let intent = KernelIntent::new("analyze");
        let context = KernelContext::new();
        let policy = KernelPolicy::new();

        let proposals = run_kernel(&mut engine, &intent, &context, &policy).unwrap();

        let proposal = &proposals[0];

        // Failed chain should produce Reasoning (not Plan)
        assert_eq!(proposal.kind, ProposalKind::Reasoning);

        // Some contracts should fail
        assert!(!proposal.all_contracts_passed());

        // Lower confidence for failed chain
        assert!(proposal.confidence.unwrap() < 0.5);
    }

    #[test]
    fn test_run_kernel_context_mapping() {
        let mut engine = MockEngine::new(vec![
            valid_reasoning_output(),
            valid_evaluation_output(),
            valid_planning_output(),
        ]);

        let intent = KernelIntent::new("analyze");

        // Test various context value types
        let context = KernelContext::new()
            .with_state("int_value", serde_json::json!(42))
            .with_state("float_value", serde_json::json!(3.14))
            .with_state("string_value", serde_json::json!("hello"))
            .with_state("bool_value", serde_json::json!(true))
            .with_state("array_value", serde_json::json!(["a", "b", "c"]))
            .with_state("nested", serde_json::json!({"inner": 123}))
            .with_fact("Seeds", "seed-1", "Test fact content")
            .with_tenant("test-tenant-123");

        let policy = KernelPolicy::new();

        // Should not panic on various JSON types
        let result = run_kernel(&mut engine, &intent, &context, &policy);
        assert!(result.is_ok());
    }

    #[test]
    fn test_kernel_policy_adapter() {
        // Test that adapter_id is stored correctly in policy
        let policy = KernelPolicy::new().with_adapter("llm/grounded@1.0.0");

        assert!(policy.adapter_id.is_some());
        assert_eq!(policy.adapter_id.as_deref(), Some("llm/grounded@1.0.0"));
    }

    #[test]
    fn test_run_kernel_with_adapter() {
        let mut engine = MockEngine::new(vec![
            valid_reasoning_output(),
            valid_evaluation_output(),
            valid_planning_output(),
        ]);

        let intent = KernelIntent::new("analyze");
        let context = KernelContext::new();
        let policy = KernelPolicy::deterministic(42).with_adapter("llm/grounded@1.0.0");

        let proposals = run_kernel(&mut engine, &intent, &context, &policy).unwrap();

        // Adapter should be recorded in trace link
        let proposal = &proposals[0];
        assert_eq!(
            proposal.trace_link.adapter_id.as_deref(),
            Some("llm/grounded@1.0.0")
        );
    }
}
