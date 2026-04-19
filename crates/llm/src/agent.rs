// Copyright 2024-2026 Reflective Labs

//! LLM Suggestor for the Converge framework.
//!
//! This module provides agents that use LLMs for reasoning, generation,
//! and decision-making within the Converge agent system.

use crate::config::LlmConfig;
use crate::error::LlmResult;
use crate::inference::{GenerationParams, InferenceEngine};
use crate::model::LlamaModel;
use crate::tokenizer::Tokenizer;
use burn::backend::NdArray;
use converge_core::{AgentEffect, ContextKey, ProposedFact, Suggestor};
use serde::{Deserialize, Serialize};

/// An agent that uses an LLM for reasoning and generation.
///
/// The LlmAgent observes context, constructs prompts, runs inference,
/// and emits generated content as facts or hypotheses.
pub struct LlmAgent {
    name: String,
    config: LlmConfig,
    generation_params: GenerationParams,
    prompt_template: PromptTemplate,
    output_key: ContextKey,
}

impl LlmAgent {
    /// Create a new LLM agent.
    #[must_use]
    pub fn new(name: impl Into<String>, config: LlmConfig) -> Self {
        Self {
            name: name.into(),
            config,
            generation_params: GenerationParams::agent(),
            prompt_template: PromptTemplate::default(),
            output_key: ContextKey::Hypotheses,
        }
    }

    /// Set generation parameters.
    #[must_use]
    pub fn with_params(mut self, params: GenerationParams) -> Self {
        self.generation_params = params;
        self
    }

    /// Set the prompt template.
    #[must_use]
    pub fn with_template(mut self, template: PromptTemplate) -> Self {
        self.prompt_template = template;
        self
    }

    /// Set the output context key.
    #[must_use]
    pub fn with_output_key(mut self, key: ContextKey) -> Self {
        self.output_key = key;
        self
    }

    /// Build prompt from context using the template.
    fn build_prompt(&self, ctx: &dyn converge_core::ContextView) -> String {
        let mut prompt = self.prompt_template.system.clone();
        prompt.push_str("\n\n");

        // Add context from specified keys
        for key in &self.prompt_template.context_keys {
            let facts = ctx.get(*key);
            if !facts.is_empty() {
                prompt.push_str(&format!("## {key:?}\n\n"));
                for fact in facts {
                    prompt.push_str(&format!("- {}: {}\n", fact.id, fact.content));
                }
                prompt.push('\n');
            }
        }

        // Add user instruction
        prompt.push_str(&self.prompt_template.instruction);

        prompt
    }

    /// Run inference and return generated text.
    fn run_inference(&self, prompt: &str) -> LlmResult<String> {
        let tokenizer = Tokenizer::llama3()?;
        let model: LlamaModel<NdArray> = LlamaModel::new(self.config.clone());

        // In production, model would be loaded once and reused
        // For now, this is a placeholder that shows the API
        let engine = InferenceEngine::new(model, tokenizer);

        // Note: This will fail with ModelNotLoaded in the placeholder impl
        // Real implementation would have the model loaded
        engine.generate(prompt, &self.generation_params)
    }
}

#[async_trait::async_trait]
impl Suggestor for LlmAgent {
    fn name(&self) -> &str {
        &self.name
    }

    fn dependencies(&self) -> &[ContextKey] {
        // LLM agent typically depends on seeds and signals
        &[ContextKey::Seeds, ContextKey::Signals]
    }

    fn accepts(&self, ctx: &dyn converge_core::ContextView) -> bool {
        // Check if we have input to process
        let has_seeds = !ctx.get(ContextKey::Seeds).is_empty();

        // AXIOM COMPLIANCE: Idempotency must check BOTH Proposals AND target_key
        // - Proposals: pending contributions awaiting validation
        // - target_key: validated contributions (promoted by engine)
        // See: converge-platform ENGINE_EXECUTION_MODEL.md
        let my_prefix = format!("{}-", self.name);

        // Check Proposals (pending before validation)
        let has_pending = ctx
            .get_proposals(self.output_key)
            .iter()
            .any(|proposal| proposal.id.contains(&my_prefix));

        // Check target_key (validated contributions)
        let has_validated = ctx
            .get(self.output_key)
            .iter()
            .any(|f| f.id.starts_with(&my_prefix));

        has_seeds && !has_pending && !has_validated
    }

    async fn execute(&self, ctx: &dyn converge_core::ContextView) -> AgentEffect {
        let prompt = self.build_prompt(ctx);

        tracing::info!(
            agent = %self.name,
            prompt_len = prompt.len(),
            "Running LLM inference"
        );

        match self.run_inference(&prompt) {
            Ok(generated) => {
                // AXIOM COMPLIANCE: "Agents Suggest, Engines Decide"
                // LLM agents MUST emit ProposedFact, not Fact.
                // A separate ValidationAgent promotes proposals to facts.
                // See: converge-platform DECISIONS.md §3
                let proposal = ProposedFact {
                    key: self.output_key,
                    id: format!("{}-output", self.name),
                    content: generated,
                    confidence: 0.0, // To be set by evaluation/validation
                    provenance: format!("llm:{}", self.name),
                };
                AgentEffect::with_proposal(proposal)
            }
            Err(e) => {
                tracing::error!(
                    agent = %self.name,
                    error = %e,
                    "LLM inference failed"
                );
                AgentEffect::with_proposal(
                    ProposedFact::new(
                        ContextKey::Diagnostic,
                        format!("{}-error", self.name),
                        format!("LLM inference failed: {e}"),
                        format!("system:{}", self.name),
                    )
                    .with_confidence(1.0),
                )
            }
        }
    }
}

/// Template for constructing prompts from context.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptTemplate {
    /// System prompt / role description.
    pub system: String,
    /// Context keys to include in the prompt.
    pub context_keys: Vec<ContextKey>,
    /// User instruction / question.
    pub instruction: String,
}

impl Default for PromptTemplate {
    fn default() -> Self {
        Self {
            system: "You are a helpful assistant that reasons about facts and generates insights."
                .to_string(),
            context_keys: vec![ContextKey::Seeds, ContextKey::Signals],
            instruction: "Based on the context above, provide your analysis.".to_string(),
        }
    }
}

impl PromptTemplate {
    /// Create a reasoning-focused template.
    #[must_use]
    pub fn reasoning() -> Self {
        Self {
            system: "You are a reasoning agent. Analyze the given facts and derive logical conclusions. Be precise and concise.".to_string(),
            context_keys: vec![ContextKey::Seeds, ContextKey::Signals, ContextKey::Constraints],
            instruction: "What conclusions can be drawn from these facts? List them as numbered points.".to_string(),
        }
    }

    /// Create a planning template.
    #[must_use]
    pub fn planning() -> Self {
        Self {
            system: "You are a planning agent. Given the current state and goals, propose actionable steps.".to_string(),
            context_keys: vec![ContextKey::Seeds, ContextKey::Strategies],
            instruction: "What are the next steps to achieve the goal? Be specific and actionable.".to_string(),
        }
    }

    /// Create a scoring/evaluation template.
    #[must_use]
    pub fn evaluation() -> Self {
        Self {
            system: "You are an evaluation agent. Score and critique the proposed strategies."
                .to_string(),
            context_keys: vec![ContextKey::Hypotheses, ContextKey::Strategies],
            instruction:
                "Evaluate each strategy. Assign a score from 1-10 and explain your reasoning."
                    .to_string(),
        }
    }
}

/// Configuration for an LLM-based reasoning loop.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReasoningConfig {
    /// Maximum reasoning steps before stopping.
    pub max_steps: usize,
    /// Confidence threshold for accepting a conclusion.
    pub confidence_threshold: f64,
    /// Whether to emit intermediate reasoning as facts.
    pub emit_intermediate: bool,
}

impl Default for ReasoningConfig {
    fn default() -> Self {
        Self {
            max_steps: 5,
            confidence_threshold: 0.8,
            emit_intermediate: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use converge_core::{Context, Engine};

    fn promoted_context(entries: &[(ContextKey, &str, &str)]) -> Context {
        let mut ctx = Context::new();
        for (key, id, content) in entries {
            ctx.add_input(*key, *id, *content).unwrap();
        }
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(Engine::new().run(ctx)).unwrap().context
    }

    #[test]
    fn test_agent_creation() {
        let config = LlmConfig::default();
        let agent = LlmAgent::new("test-agent", config);
        assert_eq!(agent.name(), "test-agent");
    }

    #[test]
    fn test_agent_accepts_with_seeds() {
        let config = LlmConfig::default();
        let agent = LlmAgent::new("test-agent", config);

        let mut ctx = Context::new();
        assert!(!agent.accepts(&ctx)); // No seeds

        ctx = promoted_context(&[(ContextKey::Seeds, "seed-1", "test seed")]);
        assert!(agent.accepts(&ctx)); // Has seeds
    }

    #[test]
    fn test_prompt_building() {
        let config = LlmConfig::default();
        let agent = LlmAgent::new("test-agent", config);

        let ctx = promoted_context(&[(ContextKey::Seeds, "seed-1", "test content")]);

        let prompt = agent.build_prompt(&ctx);
        assert!(prompt.contains("test content"));
        assert!(prompt.contains("Seeds"));
    }

    #[test]
    fn test_prompt_templates() {
        let reasoning = PromptTemplate::reasoning();
        assert!(reasoning.system.contains("reasoning"));

        let planning = PromptTemplate::planning();
        assert!(planning.system.contains("planning"));

        let eval = PromptTemplate::evaluation();
        assert!(eval.system.contains("evaluation"));
    }

    // ========================================================================
    // AXIOM COMPLIANCE TESTS
    // ========================================================================

    /// AXIOM: "Agents Suggest, Engines Decide"
    /// LlmAgent idempotency must check BOTH Proposals AND target_key.
    /// If a pending proposal exists, accepts() must return false.
    #[test]
    fn test_idempotency_checks_proposals() {
        let config = LlmConfig::default();
        let agent = LlmAgent::new("test-agent", config);

        let mut ctx = promoted_context(&[(ContextKey::Seeds, "seed-1", "test seed")]);
        assert!(agent.accepts(&ctx)); // Should accept (no pending proposal)

        // Add a pending proposal from this agent
        let _ = ctx.add_proposal(ProposedFact::new(
            ContextKey::Hypotheses,
            "test-agent-output",
            "pending proposal",
            "test-agent",
        ));

        // Now accepts() should return false (pending proposal exists)
        assert!(
            !agent.accepts(&ctx),
            "VIOLATION: accepts() must check Proposals for idempotency"
        );
    }

    /// AXIOM: Idempotency must also check validated contributions in target_key.
    #[test]
    fn test_idempotency_checks_target_key() {
        let config = LlmConfig::default();
        let agent = LlmAgent::new("test-agent", config);

        let mut ctx = promoted_context(&[(ContextKey::Seeds, "seed-1", "test seed")]);
        assert!(agent.accepts(&ctx));

        // Add validated output in target_key (Hypotheses by default)
        ctx = promoted_context(&[
            (ContextKey::Seeds, "seed-1", "test seed"),
            (
                ContextKey::Hypotheses,
                "test-agent-output",
                "validated output",
            ),
        ]);

        // Now accepts() should return false (validated contribution exists)
        assert!(
            !agent.accepts(&ctx),
            "VIOLATION: accepts() must check target_key for idempotency"
        );
    }

    /// AXIOM: Two executes with same context must produce exactly one proposal.
    /// This tests the combined idempotency check.
    #[test]
    fn test_two_executes_produce_one_proposal() {
        let config = LlmConfig::default();
        let agent = LlmAgent::new("test-agent", config);

        let mut ctx = promoted_context(&[(ContextKey::Seeds, "seed-1", "test seed")]);

        // First check: should accept
        assert!(agent.accepts(&ctx), "First execution should be accepted");

        // Simulate what happens after first execution:
        // A proposal is added to the context
        let _ = ctx.add_proposal(ProposedFact::new(
            ContextKey::Hypotheses,
            "test-agent-output",
            "generated content",
            "test-agent",
        ));

        // Second check: should NOT accept (idempotency)
        assert!(
            !agent.accepts(&ctx),
            "Second execution must be rejected (idempotency)"
        );
    }
}
