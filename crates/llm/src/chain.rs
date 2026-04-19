// Copyright 2024-2026 Reflective Labs

//! Decision chain execution for multi-step agent reasoning.
//!
//! This module orchestrates the three-step decision pipeline:
//!
//! ```text
//! StateInjection (from Polars)
//!       ↓
//! LlmAgent(reasoning) → validated Reasoning output
//!       ↓
//! LlmAgent(evaluation) → validated Evaluation output
//!       ↓
//! LlmAgent(planning) → validated Planning output
//!       ↓
//! Strategy
//! ```
//!
//! # Design Constraints (Phase-3)
//!
//! - No retries
//! - No self-reflection
//! - No tool calls
//! - Fail fast on validation errors
//! - Full observability via DecisionTrace
//!
//! # Usage
//!
//! ```ignore
//! let executor = ChainExecutor::new(engine);
//!
//! let chain = executor.run(
//!     &initial_state,
//!     &envelope,
//!     &ChainConfig::default(),
//! )?;
//!
//! if chain.completed {
//!     println!("Strategy: {}", chain.final_output.unwrap());
//! } else {
//!     println!("Failed at: {:?}", chain.failed_at);
//!     for failure in chain.failures() {
//!         println!("  {}: {:?}", failure.step, failure.validation.first_failure());
//!     }
//! }
//! ```

use crate::error::LlmResult;
use crate::execution_plan::{ExecutionPlan, StepPlan};
use crate::inference::InferenceEnvelope;
use crate::prompt::{
    OutputContract, PromptStack, PromptStackBuilder, StateInjection, TaskFrame, UserIntent,
};
use crate::trace::{DecisionChain, DecisionStep, DecisionTraceBuilder};
use crate::validation::validate_output;
use serde::{Deserialize, Serialize};

/// Configuration for chain execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainConfig {
    /// Intent for the reasoning step
    pub reasoning_intent: String,
    /// Intent for the evaluation step
    pub evaluation_intent: String,
    /// Intent for the planning step
    pub planning_intent: String,
    /// Criteria to apply (passed to all steps)
    pub criteria: Option<String>,
    /// Maximum tokens for reasoning output
    pub reasoning_max_tokens: usize,
    /// Maximum tokens for evaluation output
    pub evaluation_max_tokens: usize,
    /// Maximum tokens for planning output
    pub planning_max_tokens: usize,

    /// Recall configuration (always present, Option handles disabled case)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recall: Option<crate::recall::RecallConfig>,
}

impl Default for ChainConfig {
    fn default() -> Self {
        Self {
            reasoning_intent: "analyze_state".to_string(),
            evaluation_intent: "evaluate_options".to_string(),
            planning_intent: "create_action_plan".to_string(),
            criteria: None,
            reasoning_max_tokens: 512,
            evaluation_max_tokens: 256,
            planning_max_tokens: 512,
            recall: None,
        }
    }
}

impl ChainConfig {
    /// Create config for deployment decision chains.
    #[must_use]
    pub fn deployment() -> Self {
        Self {
            reasoning_intent: "analyze_metrics".to_string(),
            evaluation_intent: "evaluate_deployment_readiness".to_string(),
            planning_intent: "plan_deployment_actions".to_string(),
            criteria: Some("risk_adjusted".to_string()),
            ..Default::default()
        }
    }

    /// Create config for training decision chains.
    #[must_use]
    pub fn training() -> Self {
        Self {
            reasoning_intent: "analyze_training_progress".to_string(),
            evaluation_intent: "evaluate_convergence".to_string(),
            planning_intent: "plan_next_iteration".to_string(),
            criteria: Some("convergence_focused".to_string()),
            ..Default::default()
        }
    }

    /// Add recall configuration.
    #[must_use]
    pub fn with_recall(mut self, recall_config: crate::recall::RecallConfig) -> Self {
        self.recall = Some(recall_config);
        self
    }

    /// Check if recall is enabled for a specific step.
    #[must_use]
    pub fn should_recall_for_step(&self, step: DecisionStep) -> bool {
        use crate::recall::RecallTrigger;

        let Some(recall_config) = &self.recall else {
            return false;
        };

        if !recall_config.policy.enabled {
            return false;
        }

        let trigger = match step {
            DecisionStep::Reasoning => recall_config.per_step.reasoning,
            DecisionStep::Evaluation => recall_config.per_step.evaluation,
            DecisionStep::Planning => recall_config.per_step.planning,
        };

        matches!(trigger, RecallTrigger::Always)
        // OnRequest would check pack policy, not implemented in MVP
    }
}

/// Result of a single chain step.
#[derive(Debug)]
pub struct StepResult {
    /// The raw output text
    pub output: String,
    /// Whether validation passed
    pub valid: bool,
    /// Extracted signals for the next step (if valid)
    pub signals: Option<StepSignals>,
}

/// Signals extracted from a step's output for the next step.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepSignals {
    /// Key observations/conclusions
    pub observations: Vec<String>,
    /// Numeric scores if available
    pub scores: Vec<(String, f64)>,
    /// Recommended actions
    pub actions: Vec<String>,
}

impl StepSignals {
    /// Create empty signals.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            observations: vec![],
            scores: vec![],
            actions: vec![],
        }
    }

    /// Convert to StateInjection for the next step.
    #[must_use]
    pub fn to_state_injection(&self) -> StateInjection {
        let mut state = StateInjection::new();

        // Add observations as a list
        if !self.observations.is_empty() {
            state = state.with_list(
                "observations",
                self.observations.iter().map(|s| s.clone().into()).collect(),
            );
        }

        // Add scores as scalars
        for (name, value) in &self.scores {
            state = state.with_scalar(name.clone(), *value);
        }

        // Add actions as a list
        if !self.actions.is_empty() {
            state = state.with_list(
                "recommended_actions",
                self.actions.iter().map(|s| s.clone().into()).collect(),
            );
        }

        state
    }
}

/// Executes decision chains.
///
/// This is the main orchestrator for multi-step agent reasoning.
/// It manages the flow: reasoning → evaluation → planning.
pub struct ChainExecutor<E> {
    engine: E,
}

impl<E> ChainExecutor<E> {
    /// Create a new chain executor with the given inference engine.
    pub fn new(engine: E) -> Self {
        Self { engine }
    }

    /// Get a reference to the underlying engine.
    pub fn engine(&self) -> &E {
        &self.engine
    }

    /// Get a mutable reference to the underlying engine.
    pub fn engine_mut(&mut self) -> &mut E {
        &mut self.engine
    }
}

/// Trait for engines that can run chain steps.
///
/// This allows the chain executor to work with different engine types
/// (real LlamaEngine or mock engines for testing).
pub trait ChainEngine {
    /// Generate output for a prompt stack with the given envelope.
    fn generate(&mut self, stack: &PromptStack, envelope: &InferenceEnvelope) -> LlmResult<String>;
}

#[cfg(feature = "llama3")]
impl<B: burn::tensor::backend::Backend> ChainEngine for crate::engine::LlamaEngine<B> {
    fn generate(&mut self, stack: &PromptStack, envelope: &InferenceEnvelope) -> LlmResult<String> {
        let result = self.run(stack, envelope)?;
        Ok(result.text)
    }
}

#[cfg(feature = "gemma")]
impl ChainEngine for crate::gemma::GemmaEngine {
    fn generate(&mut self, stack: &PromptStack, envelope: &InferenceEnvelope) -> LlmResult<String> {
        let result = self.run(stack, envelope)?;
        Ok(result.text)
    }
}

impl<E: ChainEngine> ChainExecutor<E> {
    /// Execute a full decision chain.
    ///
    /// This runs through all three steps (reasoning → evaluation → planning),
    /// validating output at each step and failing fast on errors.
    ///
    /// # Arguments
    ///
    /// * `initial_state` - The starting state (typically from Polars metrics)
    /// * `envelope` - The inference envelope (controls sampling/determinism)
    /// * `config` - Chain configuration (intents, criteria, token limits)
    ///
    /// # Returns
    ///
    /// A `DecisionChain` containing traces for all steps, whether successful or not.
    pub fn run(
        &mut self,
        initial_state: &StateInjection,
        envelope: &InferenceEnvelope,
        config: &ChainConfig,
    ) -> LlmResult<DecisionChain> {
        let mut chain = DecisionChain::new(generate_chain_id());

        // Step 1: Reasoning
        let reasoning_result =
            self.run_reasoning_step(initial_state, envelope, config, &mut chain)?;

        if !reasoning_result.valid {
            chain.fail_at(DecisionStep::Reasoning);
            return Ok(chain);
        }

        // Extract signals for next step
        let reasoning_signals = reasoning_result.signals.unwrap_or_else(StepSignals::empty);

        // Step 2: Evaluation
        let evaluation_state = initial_state
            .clone()
            .merge(&reasoning_signals.to_state_injection());

        let evaluation_result =
            self.run_evaluation_step(&evaluation_state, envelope, config, &mut chain)?;

        if !evaluation_result.valid {
            chain.fail_at(DecisionStep::Evaluation);
            return Ok(chain);
        }

        // Extract signals for next step
        let evaluation_signals = evaluation_result.signals.unwrap_or_else(StepSignals::empty);

        // Step 3: Planning
        let planning_state = evaluation_state.merge(&evaluation_signals.to_state_injection());

        let planning_result =
            self.run_planning_step(&planning_state, envelope, config, &mut chain)?;

        if !planning_result.valid {
            chain.fail_at(DecisionStep::Planning);
            return Ok(chain);
        }

        // Chain completed successfully
        chain.complete_with(planning_result.output);
        Ok(chain)
    }

    /// Execute a full decision chain using a compiled ExecutionPlan.
    ///
    /// This is the **preferred** entry point. The ExecutionPlan is created from
    /// `ExecutionPlan::compile(intent, policy)` and cannot be constructed directly,
    /// ensuring policy cannot be bypassed.
    ///
    /// # Arguments
    ///
    /// * `initial_state` - The starting state (typically from Polars metrics)
    /// * `plan` - The compiled execution plan (from `ExecutionPlan::compile`)
    ///
    /// # Returns
    ///
    /// A `DecisionChain` containing traces for all steps, whether successful or not.
    ///
    /// # Policy Enforcement
    ///
    /// The plan's fields are private and cannot be modified after compilation.
    /// This ensures that policy toggles (recall enabled, adapter, etc.) cannot
    /// be overridden downstream.
    pub fn run_with_plan(
        &mut self,
        initial_state: &StateInjection,
        plan: &ExecutionPlan,
    ) -> LlmResult<DecisionChain> {
        let mut chain = DecisionChain::new(generate_chain_id());
        let envelope = plan.envelope();

        // Get steps from plan (guaranteed 3 steps: Reasoning, Evaluation, Planning)
        let steps = plan.steps();
        assert!(steps.len() >= 3, "ExecutionPlan must have at least 3 steps");

        // Step 1: Reasoning
        let reasoning_step = &steps[0];
        let reasoning_result =
            self.run_step_with_plan(initial_state, envelope, reasoning_step, &mut chain)?;

        if !reasoning_result.valid {
            chain.fail_at(DecisionStep::Reasoning);
            return Ok(chain);
        }

        let reasoning_signals = reasoning_result.signals.unwrap_or_else(StepSignals::empty);

        // Step 2: Evaluation
        let evaluation_step = &steps[1];
        let evaluation_state = initial_state
            .clone()
            .merge(&reasoning_signals.to_state_injection());

        let evaluation_result =
            self.run_step_with_plan(&evaluation_state, envelope, evaluation_step, &mut chain)?;

        if !evaluation_result.valid {
            chain.fail_at(DecisionStep::Evaluation);
            return Ok(chain);
        }

        let evaluation_signals = evaluation_result.signals.unwrap_or_else(StepSignals::empty);

        // Step 3: Planning
        let planning_step = &steps[2];
        let planning_state = evaluation_state.merge(&evaluation_signals.to_state_injection());

        let planning_result =
            self.run_step_with_plan(&planning_state, envelope, planning_step, &mut chain)?;

        if !planning_result.valid {
            chain.fail_at(DecisionStep::Planning);
            return Ok(chain);
        }

        // Chain completed successfully
        chain.complete_with(planning_result.output);
        Ok(chain)
    }

    /// Run a single step using the compiled StepPlan.
    fn run_step_with_plan(
        &mut self,
        state: &StateInjection,
        envelope: &InferenceEnvelope,
        step_plan: &StepPlan,
        chain: &mut DecisionChain,
    ) -> LlmResult<StepResult> {
        let task_frame = match step_plan.step {
            DecisionStep::Reasoning => TaskFrame::reason(),
            DecisionStep::Evaluation => TaskFrame::evaluate(),
            DecisionStep::Planning => TaskFrame::plan(),
        };

        let intent = UserIntent::new(&step_plan.intent);

        let stack = PromptStackBuilder::new()
            .task_frame(task_frame)
            .state(state.clone())
            .intent(intent)
            .build();

        // Build trace
        let trace_builder = DecisionTraceBuilder::new(step_plan.step)
            .with_input_state(state)
            .with_envelope(envelope)
            .with_prompt_version(&stack.version)
            .with_contract_type(&format!("{:?}", step_plan.step));

        // Run inference
        let output = self.engine.generate(&stack, envelope)?;

        // Validate against the step's contract
        let validation = validate_output(&output, &step_plan.contract);
        let valid = validation.valid;

        // Complete trace
        let trace = trace_builder.complete(output.clone(), validation);
        chain.add_trace(trace);

        // Extract signals based on step type
        let signals = if valid {
            Some(match step_plan.step {
                DecisionStep::Reasoning => extract_reasoning_signals(&output),
                DecisionStep::Evaluation => extract_evaluation_signals(&output),
                DecisionStep::Planning => StepSignals::empty(),
            })
        } else {
            None
        };

        Ok(StepResult {
            output,
            valid,
            signals,
        })
    }

    /// Run the reasoning step.
    fn run_reasoning_step(
        &mut self,
        state: &StateInjection,
        envelope: &InferenceEnvelope,
        config: &ChainConfig,
        chain: &mut DecisionChain,
    ) -> LlmResult<StepResult> {
        let contract = OutputContract::reasoning();

        let task_frame = TaskFrame::reason();
        let mut intent = UserIntent::new(&config.reasoning_intent);
        if let Some(criteria) = &config.criteria {
            intent = intent.with_criteria(criteria);
        }

        let stack = PromptStackBuilder::new()
            .task_frame(task_frame)
            .state(state.clone())
            .intent(intent)
            .build();

        // Build trace
        let trace_builder = DecisionTraceBuilder::new(DecisionStep::Reasoning)
            .with_input_state(state)
            .with_envelope(envelope)
            .with_prompt_version(&stack.version)
            .with_contract_type("Reasoning");

        // Run inference
        let output = self.engine.generate(&stack, envelope)?;

        // Validate
        let validation = validate_output(&output, &contract);
        let valid = validation.valid;

        // Complete trace
        let trace = trace_builder.complete(output.clone(), validation);
        chain.add_trace(trace);

        // Extract signals if valid
        let signals = if valid {
            Some(extract_reasoning_signals(&output))
        } else {
            None
        };

        Ok(StepResult {
            output,
            valid,
            signals,
        })
    }

    /// Run the evaluation step.
    fn run_evaluation_step(
        &mut self,
        state: &StateInjection,
        envelope: &InferenceEnvelope,
        config: &ChainConfig,
        chain: &mut DecisionChain,
    ) -> LlmResult<StepResult> {
        let contract = OutputContract::evaluation();

        let task_frame = TaskFrame::evaluate();
        let mut intent = UserIntent::new(&config.evaluation_intent);
        if let Some(criteria) = &config.criteria {
            intent = intent.with_criteria(criteria);
        }

        let stack = PromptStackBuilder::new()
            .task_frame(task_frame)
            .state(state.clone())
            .intent(intent)
            .build();

        // Build trace
        let trace_builder = DecisionTraceBuilder::new(DecisionStep::Evaluation)
            .with_input_state(state)
            .with_envelope(envelope)
            .with_prompt_version(&stack.version)
            .with_contract_type("Evaluation");

        // Run inference
        let output = self.engine.generate(&stack, envelope)?;

        // Validate
        let validation = validate_output(&output, &contract);
        let valid = validation.valid;

        // Complete trace
        let trace = trace_builder.complete(output.clone(), validation);
        chain.add_trace(trace);

        // Extract signals if valid
        let signals = if valid {
            Some(extract_evaluation_signals(&output))
        } else {
            None
        };

        Ok(StepResult {
            output,
            valid,
            signals,
        })
    }

    /// Run the planning step.
    fn run_planning_step(
        &mut self,
        state: &StateInjection,
        envelope: &InferenceEnvelope,
        config: &ChainConfig,
        chain: &mut DecisionChain,
    ) -> LlmResult<StepResult> {
        let contract = OutputContract::planning();

        let task_frame = TaskFrame::plan();
        let mut intent = UserIntent::new(&config.planning_intent);
        if let Some(criteria) = &config.criteria {
            intent = intent.with_criteria(criteria);
        }

        let stack = PromptStackBuilder::new()
            .task_frame(task_frame)
            .state(state.clone())
            .intent(intent)
            .build();

        // Build trace
        let trace_builder = DecisionTraceBuilder::new(DecisionStep::Planning)
            .with_input_state(state)
            .with_envelope(envelope)
            .with_prompt_version(&stack.version)
            .with_contract_type("Planning");

        // Run inference
        let output = self.engine.generate(&stack, envelope)?;

        // Validate
        let validation = validate_output(&output, &contract);
        let valid = validation.valid;

        // Complete trace
        let trace = trace_builder.complete(output.clone(), validation);
        chain.add_trace(trace);

        // No signals needed from planning (it's the final step)
        let signals = if valid {
            Some(StepSignals::empty())
        } else {
            None
        };

        Ok(StepResult {
            output,
            valid,
            signals,
        })
    }
}

/// Extract signals from reasoning output.
fn extract_reasoning_signals(output: &str) -> StepSignals {
    let mut signals = StepSignals::empty();

    // Extract conclusion
    let output_upper = output.to_uppercase();
    if let Some(idx) = output_upper.find("CONCLUSION:") {
        let conclusion_start = idx + "CONCLUSION:".len();
        let conclusion = output[conclusion_start..]
            .lines()
            .next()
            .unwrap_or("")
            .trim();
        if !conclusion.is_empty() {
            signals.observations.push(conclusion.to_string());
        }
    }

    // Extract any uncertainty markers
    if output_upper.contains("UNCERTAIN") {
        signals.observations.push("uncertainty_flagged".to_string());
    }

    signals
}

/// Extract signals from evaluation output.
fn extract_evaluation_signals(output: &str) -> StepSignals {
    let mut signals = StepSignals::empty();

    // Extract numeric scores (simple heuristic)
    for word in output.split_whitespace() {
        let cleaned = word.trim_matches(|c: char| !c.is_numeric() && c != '.' && c != '-');
        if let Ok(score) = cleaned.parse::<f64>() {
            if (0.0..=1.0).contains(&score) {
                signals.scores.push(("score".to_string(), score));
                break; // Take first valid score
            }
        }
    }

    // Extract confidence mentions
    let output_lower = output.to_lowercase();
    if output_lower.contains("high confidence") {
        signals.observations.push("high_confidence".to_string());
    } else if output_lower.contains("low confidence") {
        signals.observations.push("low_confidence".to_string());
    }

    signals
}

/// Generate a unique chain ID.
fn generate_chain_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);

    let count = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("chain-{count:06}")
}

// Extension trait for StateInjection merging
trait StateInjectionExt {
    fn merge(self, other: &StateInjection) -> StateInjection;
}

impl StateInjectionExt for StateInjection {
    fn merge(mut self, other: &StateInjection) -> StateInjection {
        // Merge scalars
        for (key, value) in &other.scalars {
            self.scalars.insert(key.clone(), value.clone());
        }
        // Merge lists
        for (key, value) in &other.lists {
            self.lists.insert(key.clone(), value.clone());
        }
        // Merge records
        for record in &other.records {
            self.records.push(record.clone());
        }
        self
    }
}

/// Mock engine for testing chains without a real model.
#[cfg(test)]
pub struct MockEngine {
    responses: std::collections::VecDeque<String>,
}

#[cfg(test)]
impl MockEngine {
    pub fn new(responses: Vec<String>) -> Self {
        Self {
            responses: responses.into(),
        }
    }
}

#[cfg(test)]
impl ChainEngine for MockEngine {
    fn generate(
        &mut self,
        _stack: &PromptStack,
        _envelope: &InferenceEnvelope,
    ) -> LlmResult<String> {
        self.responses
            .pop_front()
            .ok_or_else(|| crate::error::LlmError::InferenceError("No more mock responses".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inference::InferenceEnvelope;

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

    fn invalid_reasoning_output() -> String {
        "The data looks interesting but I'm not sure what to make of it.".to_string()
    }

    #[test]
    fn test_successful_chain_execution() {
        let engine = MockEngine::new(vec![
            valid_reasoning_output(),
            valid_evaluation_output(),
            valid_planning_output(),
        ]);

        let mut executor = ChainExecutor::new(engine);

        let state = StateInjection::new()
            .with_scalar("mae", 0.12)
            .with_scalar("success_ratio", 0.88);

        let envelope = InferenceEnvelope::deterministic("test:v1", 42);
        let config = ChainConfig::default();

        let chain = executor.run(&state, &envelope, &config).unwrap();

        assert!(chain.completed);
        assert!(chain.failed_at.is_none());
        assert_eq!(chain.traces.len(), 3);
        assert!(chain.is_training_candidate());
    }

    #[test]
    fn test_chain_fails_at_reasoning() {
        let engine = MockEngine::new(vec![
            invalid_reasoning_output(),
            // These won't be called because we fail fast
            valid_evaluation_output(),
            valid_planning_output(),
        ]);

        let mut executor = ChainExecutor::new(engine);

        let state = StateInjection::new().with_scalar("mae", 0.12);
        let envelope = InferenceEnvelope::deterministic("test:v1", 42);
        let config = ChainConfig::default();

        let chain = executor.run(&state, &envelope, &config).unwrap();

        assert!(!chain.completed);
        assert_eq!(chain.failed_at, Some(DecisionStep::Reasoning));
        assert_eq!(chain.traces.len(), 1); // Only reasoning was attempted
        assert!(!chain.is_training_candidate());
    }

    #[test]
    fn test_chain_fails_at_evaluation() {
        let engine = MockEngine::new(vec![
            valid_reasoning_output(),
            "This is not a valid evaluation".to_string(), // Invalid
            valid_planning_output(),
        ]);

        let mut executor = ChainExecutor::new(engine);

        let state = StateInjection::new().with_scalar("mae", 0.12);
        let envelope = InferenceEnvelope::deterministic("test:v1", 42);
        let config = ChainConfig::default();

        let chain = executor.run(&state, &envelope, &config).unwrap();

        assert!(!chain.completed);
        assert_eq!(chain.failed_at, Some(DecisionStep::Evaluation));
        assert_eq!(chain.traces.len(), 2); // Reasoning + Evaluation
    }

    #[test]
    fn test_chain_fails_at_planning() {
        let engine = MockEngine::new(vec![
            valid_reasoning_output(),
            valid_evaluation_output(),
            "No numbered steps here, just prose.".to_string(), // Invalid
        ]);

        let mut executor = ChainExecutor::new(engine);

        let state = StateInjection::new().with_scalar("mae", 0.12);
        let envelope = InferenceEnvelope::deterministic("test:v1", 42);
        let config = ChainConfig::default();

        let chain = executor.run(&state, &envelope, &config).unwrap();

        assert!(!chain.completed);
        assert_eq!(chain.failed_at, Some(DecisionStep::Planning));
        assert_eq!(chain.traces.len(), 3); // All three attempted
    }

    #[test]
    fn test_chain_config_presets() {
        let deployment = ChainConfig::deployment();
        assert!(deployment.criteria.is_some());
        assert!(deployment.reasoning_intent.contains("metrics"));

        let training = ChainConfig::training();
        assert!(training.criteria.is_some());
        assert!(training.reasoning_intent.contains("training"));
    }

    #[test]
    fn test_signal_extraction() {
        let reasoning_output = "Analysis shows CONCLUSION: Model is ready.";
        let signals = extract_reasoning_signals(reasoning_output);
        assert!(!signals.observations.is_empty());

        let eval_output = "Score: 0.85 with high confidence";
        let signals = extract_evaluation_signals(eval_output);
        assert!(!signals.scores.is_empty());
        assert!(
            signals
                .observations
                .contains(&"high_confidence".to_string())
        );
    }

    #[test]
    fn test_step_signals_to_state() {
        let signals = StepSignals {
            observations: vec!["obs1".to_string()],
            scores: vec![("score".to_string(), 0.85)],
            actions: vec!["action1".to_string()],
        };

        let state = signals.to_state_injection();
        assert!(state.lists.contains_key("observations"));
        assert!(state.scalars.contains_key("score"));
        assert!(state.lists.contains_key("recommended_actions"));
    }

    #[test]
    fn test_chain_summary() {
        let engine = MockEngine::new(vec![
            valid_reasoning_output(),
            valid_evaluation_output(),
            valid_planning_output(),
        ]);

        let mut executor = ChainExecutor::new(engine);

        let state = StateInjection::new().with_scalar("mae", 0.12);
        let envelope = InferenceEnvelope::deterministic("test:v1", 42);
        let config = ChainConfig::default();

        let chain = executor.run(&state, &envelope, &config).unwrap();

        let summary = chain.summary();
        assert!(summary.contains("COMPLETED"));
        assert!(summary.contains("Reasoning"));
        assert!(summary.contains("Evaluation"));
        assert!(summary.contains("Planning"));
    }

    // =========================================================================
    // ExecutionPlan Integration Tests
    // =========================================================================

    #[test]
    fn test_run_with_plan_successful() {
        use crate::kernel::{KernelIntent, KernelPolicy};

        let engine = MockEngine::new(vec![
            valid_reasoning_output(),
            valid_evaluation_output(),
            valid_planning_output(),
        ]);

        let mut executor = ChainExecutor::new(engine);

        let intent = KernelIntent::new("analyze deployment readiness");
        let policy = KernelPolicy::deterministic(42);
        let plan = ExecutionPlan::compile(&intent, &policy);

        let state = StateInjection::new()
            .with_scalar("mae", 0.12)
            .with_scalar("success_ratio", 0.88);

        let chain = executor.run_with_plan(&state, &plan).unwrap();

        assert!(chain.completed);
        assert!(chain.failed_at.is_none());
        assert_eq!(chain.traces.len(), 3);
    }

    #[test]
    fn test_run_with_plan_uses_compiled_envelope() {
        use crate::kernel::{KernelIntent, KernelPolicy};

        let engine = MockEngine::new(vec![
            valid_reasoning_output(),
            valid_evaluation_output(),
            valid_planning_output(),
        ]);

        let mut executor = ChainExecutor::new(engine);

        // Compile with deterministic seed
        let intent = KernelIntent::new("test");
        let policy = KernelPolicy::deterministic(123);
        let plan = ExecutionPlan::compile(&intent, &policy);

        // The plan's envelope should have the seed from policy
        assert_eq!(plan.determinism().seed, Some(123));

        let state = StateInjection::new().with_scalar("x", 1.0);
        let chain = executor.run_with_plan(&state, &plan).unwrap();

        assert!(chain.completed);
    }

    /// Negative test: run_with_plan respects policy even if ChainConfig would allow more.
    ///
    /// This demonstrates that using ExecutionPlan prevents policy bypass.
    #[test]
    fn test_run_with_plan_policy_enforcement() {
        use crate::kernel::{KernelIntent, KernelPolicy};

        let engine = MockEngine::new(vec![
            valid_reasoning_output(),
            valid_evaluation_output(),
            valid_planning_output(),
        ]);

        let mut executor = ChainExecutor::new(engine);

        // Policy with recall DISABLED
        let intent = KernelIntent::new("test");
        let policy = KernelPolicy::new(); // recall_enabled = false by default
        let plan = ExecutionPlan::compile(&intent, &policy);

        // Verify the plan has recall disabled
        assert!(!plan.recall_enabled());
        assert!(plan.recall().is_none());

        // All steps should have recall_enabled = false
        for step in plan.steps() {
            assert!(!step.recall_enabled);
        }

        let state = StateInjection::new().with_scalar("x", 1.0);
        let chain = executor.run_with_plan(&state, &plan).unwrap();

        // Chain completes, but recall was never used because policy disabled it
        // (In a real scenario, recall would be skipped in the step execution)
        assert!(chain.completed);
    }
}
