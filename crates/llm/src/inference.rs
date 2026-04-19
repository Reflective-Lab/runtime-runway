// Copyright 2024-2026 Reflective Labs

//! Inference engine for text generation.
//!
//! Provides sampling strategies, generation loops, and output formatting
//! for autoregressive text generation with Llama models.

use crate::error::{LlmError, LlmResult};
use crate::model::LlamaModel;
use crate::tokenizer::Tokenizer;
use burn::tensor::backend::Backend;
use serde::{Deserialize, Serialize};

/// Parameters for text generation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerationParams {
    /// Maximum number of tokens to generate.
    pub max_new_tokens: usize,

    /// Temperature for sampling (0.0 = greedy, higher = more random).
    pub temperature: f32,

    /// Top-p (nucleus) sampling threshold.
    pub top_p: f32,

    /// Top-k sampling (0 = disabled).
    pub top_k: usize,

    /// Repetition penalty (1.0 = no penalty).
    pub repetition_penalty: f32,

    /// Stop sequences (generation stops when any is produced).
    pub stop_sequences: Vec<String>,

    /// Random seed for reproducibility.
    pub seed: Option<u64>,
}

impl Default for GenerationParams {
    fn default() -> Self {
        Self {
            max_new_tokens: 256,
            temperature: 0.7,
            top_p: 0.9,
            top_k: 40,
            repetition_penalty: 1.1,
            stop_sequences: vec![],
            seed: None,
        }
    }
}

impl GenerationParams {
    /// Create params for deterministic (greedy) generation.
    #[must_use]
    pub fn greedy() -> Self {
        Self {
            temperature: 0.0,
            top_p: 1.0,
            top_k: 1,
            ..Default::default()
        }
    }

    /// Create params for creative generation.
    #[must_use]
    pub fn creative() -> Self {
        Self {
            temperature: 1.0,
            top_p: 0.95,
            top_k: 0,
            ..Default::default()
        }
    }

    /// Create params for agent reasoning (balanced).
    #[must_use]
    pub fn agent() -> Self {
        Self {
            temperature: 0.3,
            top_p: 0.9,
            top_k: 40,
            max_new_tokens: 512,
            ..Default::default()
        }
    }

    /// Validate parameters.
    ///
    /// # Errors
    ///
    /// Returns an error if parameters are invalid.
    pub fn validate(&self) -> LlmResult<()> {
        if self.temperature < 0.0 {
            return Err(LlmError::InvalidParams(
                "temperature must be non-negative".to_string(),
            ));
        }
        if self.top_p <= 0.0 || self.top_p > 1.0 {
            return Err(LlmError::InvalidParams(
                "top_p must be in (0, 1]".to_string(),
            ));
        }
        if self.repetition_penalty < 1.0 {
            return Err(LlmError::InvalidParams(
                "repetition_penalty must be >= 1.0".to_string(),
            ));
        }
        Ok(())
    }
}

/// Inference engine that combines model and tokenizer.
pub struct InferenceEngine<B: Backend> {
    model: LlamaModel<B>,
    tokenizer: Tokenizer,
}

impl<B: Backend> InferenceEngine<B> {
    /// Create a new inference engine.
    pub fn new(model: LlamaModel<B>, tokenizer: Tokenizer) -> Self {
        Self { model, tokenizer }
    }

    /// Generate text from a prompt.
    ///
    /// # Arguments
    ///
    /// * `prompt` - The input prompt text
    /// * `params` - Generation parameters
    ///
    /// # Returns
    ///
    /// The generated text (not including the prompt).
    ///
    /// # Errors
    ///
    /// Returns an error if generation fails.
    pub fn generate(&self, prompt: &str, params: &GenerationParams) -> LlmResult<String> {
        params.validate()?;

        // Tokenize input
        let input_tokens = self.tokenizer.encode_with_special(prompt, true, false)?;
        tracing::debug!(
            prompt_tokens = input_tokens.len(),
            max_new = params.max_new_tokens,
            "Starting generation"
        );

        // Check context length
        let max_total = self.model.config().max_context_length;
        if input_tokens.len() + params.max_new_tokens > max_total {
            return Err(LlmError::ContextLengthExceeded {
                got: input_tokens.len() + params.max_new_tokens,
                max: max_total,
            });
        }

        // Generation loop
        let mut tokens = input_tokens.clone();
        let mut generated = Vec::new();

        for step in 0..params.max_new_tokens {
            // Get logits for next token
            let logits = self.model.next_token_logits(&tokens)?;

            // Sample next token
            let next_token = self.sample(&logits, params, &tokens)?;

            // Check for EOS
            if next_token == self.tokenizer.eos_token_id() {
                tracing::debug!(step, "EOS token generated");
                break;
            }

            // Check for stop sequences
            tokens.push(next_token);
            generated.push(next_token);

            let current_text = self.tokenizer.decode(&generated)?;
            if params
                .stop_sequences
                .iter()
                .any(|s| current_text.contains(s))
            {
                tracing::debug!(step, "Stop sequence found");
                break;
            }
        }

        // Decode generated tokens
        let output = self.tokenizer.decode(&generated)?;
        tracing::info!(
            input_tokens = input_tokens.len(),
            output_tokens = generated.len(),
            "Generation complete"
        );

        Ok(output)
    }

    /// Generate with streaming callback.
    ///
    /// # Errors
    ///
    /// Returns an error if generation fails.
    pub fn generate_streaming<F>(
        &self,
        prompt: &str,
        params: &GenerationParams,
        mut callback: F,
    ) -> LlmResult<String>
    where
        F: FnMut(&str),
    {
        params.validate()?;

        let input_tokens = self.tokenizer.encode_with_special(prompt, true, false)?;
        let mut tokens = input_tokens.clone();
        let mut generated = Vec::new();

        for _ in 0..params.max_new_tokens {
            let logits = self.model.next_token_logits(&tokens)?;
            let next_token = self.sample(&logits, params, &tokens)?;

            if next_token == self.tokenizer.eos_token_id() {
                break;
            }

            tokens.push(next_token);
            generated.push(next_token);

            // Decode and emit the new token
            if let Ok(text) = self.tokenizer.decode(&[next_token]) {
                callback(&text);
            }
        }

        self.tokenizer.decode(&generated)
    }

    /// Sample next token from logits using temperature, top-k, and top-p sampling.
    ///
    /// Sampling pipeline:
    /// 1. Apply repetition penalty to tokens in context
    /// 2. Apply temperature scaling
    /// 3. Apply top-k filtering (keep top k highest probability tokens)
    /// 4. Apply top-p (nucleus) filtering (keep smallest set with cumulative prob >= p)
    /// 5. Sample from filtered distribution
    fn sample(&self, logits: &[f32], params: &GenerationParams, context: &[u32]) -> LlmResult<u32> {
        if logits.is_empty() {
            return Err(LlmError::InferenceError("Empty logits".to_string()));
        }

        let mut scaled = logits.to_vec();

        // Step 1: Apply repetition penalty
        if params.repetition_penalty > 1.0 {
            apply_repetition_penalty(&mut scaled, context, params.repetition_penalty);
        }

        // Step 2: Apply temperature scaling
        if params.temperature > 0.0 && params.temperature != 1.0 {
            for logit in &mut scaled {
                *logit /= params.temperature;
            }
        }

        // Greedy decoding: return argmax
        if params.temperature == 0.0 || params.top_k == 1 {
            return Ok(argmax(&scaled) as u32);
        }

        // Step 3: Convert to probabilities (softmax)
        let probs = softmax(&scaled);

        // Step 4: Create sorted indices by probability (descending)
        let mut indexed: Vec<(usize, f32)> = probs.iter().copied().enumerate().collect();
        indexed.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        // Step 5: Apply top-k filtering
        let top_k_filtered = if params.top_k > 0 && params.top_k < indexed.len() {
            &indexed[..params.top_k]
        } else {
            &indexed[..]
        };

        // Step 6: Apply top-p (nucleus) filtering
        let filtered = apply_top_p(top_k_filtered, params.top_p);

        if filtered.is_empty() {
            // Fallback to argmax if filtering removed everything
            return Ok(argmax(&scaled) as u32);
        }

        // Step 7: Renormalize probabilities
        let sum: f32 = filtered.iter().map(|(_, p)| p).sum();
        let normalized: Vec<(usize, f32)> = if sum > 0.0 {
            filtered.iter().map(|(i, p)| (*i, p / sum)).collect()
        } else {
            filtered.to_vec()
        };

        // Step 8: Sample from filtered distribution
        let sampled_idx = sample_from_distribution(&normalized, params.seed);

        Ok(sampled_idx as u32)
    }

    /// Get the underlying model.
    #[must_use]
    pub fn model(&self) -> &LlamaModel<B> {
        &self.model
    }

    /// Get the tokenizer.
    #[must_use]
    pub fn tokenizer(&self) -> &Tokenizer {
        &self.tokenizer
    }
}

// ============================================================================
// Sampling Helper Functions
// ============================================================================

/// Find the index of the maximum value in a slice.
fn argmax(values: &[f32]) -> usize {
    values
        .iter()
        .enumerate()
        .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(i, _)| i)
        .unwrap_or(0)
}

/// Apply softmax to convert logits to probabilities.
fn softmax(logits: &[f32]) -> Vec<f32> {
    // Find max for numerical stability
    let max_logit = logits.iter().copied().fold(f32::NEG_INFINITY, f32::max);

    // Compute exp(logit - max)
    let exp_values: Vec<f32> = logits.iter().map(|l| (l - max_logit).exp()).collect();

    // Normalize
    let sum: f32 = exp_values.iter().sum();
    if sum > 0.0 {
        exp_values.iter().map(|e| e / sum).collect()
    } else {
        // Fallback: uniform distribution
        let uniform = 1.0 / logits.len() as f32;
        vec![uniform; logits.len()]
    }
}

/// Apply repetition penalty to logits for tokens that appear in context.
///
/// Tokens that appear in the context have their logits divided by the penalty
/// (if positive) or multiplied (if negative), reducing their probability.
fn apply_repetition_penalty(logits: &mut [f32], context: &[u32], penalty: f32) {
    for &token in context {
        let idx = token as usize;
        if idx < logits.len() {
            // Apply penalty: divide positive logits, multiply negative logits
            if logits[idx] > 0.0 {
                logits[idx] /= penalty;
            } else {
                logits[idx] *= penalty;
            }
        }
    }
}

/// Apply top-p (nucleus) filtering to a sorted probability distribution.
///
/// Keeps the smallest set of tokens whose cumulative probability >= top_p.
/// Input must be sorted in descending order by probability.
fn apply_top_p(sorted_probs: &[(usize, f32)], top_p: f32) -> Vec<(usize, f32)> {
    if top_p >= 1.0 {
        return sorted_probs.to_vec();
    }

    let mut cumulative = 0.0;
    let mut result = Vec::new();

    for &(idx, prob) in sorted_probs {
        cumulative += prob;
        result.push((idx, prob));

        if cumulative >= top_p {
            break;
        }
    }

    // Always include at least one token
    if result.is_empty() && !sorted_probs.is_empty() {
        result.push(sorted_probs[0]);
    }

    result
}

/// Sample from a discrete probability distribution.
///
/// Uses the seed if provided for reproducibility.
fn sample_from_distribution(distribution: &[(usize, f32)], seed: Option<u64>) -> usize {
    use rand::prelude::*;

    if distribution.is_empty() {
        return 0;
    }

    if distribution.len() == 1 {
        return distribution[0].0;
    }

    // Create RNG (seeded or random)
    let mut rng: Box<dyn RngCore> = match seed {
        Some(s) => Box::new(rand::rngs::StdRng::seed_from_u64(s)),
        None => Box::new(rand::thread_rng()),
    };

    // Generate random value in [0, 1)
    let r: f32 = rng.r#gen();

    // Find the token corresponding to this random value
    let mut cumulative = 0.0;
    for &(idx, prob) in distribution {
        cumulative += prob;
        if r < cumulative {
            return idx;
        }
    }

    // Fallback to last token (shouldn't happen with proper normalization)
    distribution.last().map(|(i, _)| *i).unwrap_or(0)
}

/// Result of a generation request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerationResult {
    /// The generated text.
    pub text: String,
    /// Number of input tokens.
    pub input_tokens: usize,
    /// Number of generated tokens.
    pub output_tokens: usize,
    /// Reason generation stopped.
    pub finish_reason: FinishReason,
}

/// Reason why generation stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FinishReason {
    /// Reached max_new_tokens limit.
    Length,
    /// Generated EOS token.
    Eos,
    /// Hit a stop sequence.
    StopSequence,
    /// Other/unknown reason.
    Other,
}

/// Inference envelope for deterministic, reproducible agent behavior.
///
/// This captures everything needed to reproduce an inference run exactly.
/// Without this, debugging agent behavior becomes extremely difficult.
///
/// # Determinism Guarantees
///
/// When `seed` is set and `temperature` is 0:
/// - Same envelope + same input = same output
/// - Regression tests become meaningful
/// - Failures are reproducible
///
/// # LoRA Adapter Support
///
/// The `adapter_id` field explicitly specifies which LoRA adapter to use.
/// This is OPTIONAL and EXPLICIT per converge-core axioms:
/// - No default adapter selection (violates Explicit Authority)
/// - Adapter must be specified in the request
///
/// # Usage
///
/// ```ignore
/// let envelope = InferenceEnvelope::deterministic(config, prompt_stack);
/// let result = engine.run_with_envelope(&envelope, input)?;
///
/// // Later: reproduce exactly
/// let same_result = engine.run_with_envelope(&envelope, input)?;
/// assert_eq!(result.tokens, same_result.tokens);
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InferenceEnvelope {
    /// Version identifier for this envelope format
    pub version: u32,

    /// Tokenizer configuration snapshot
    pub tokenizer_config: TokenizerSnapshot,

    /// Generation parameters
    pub generation: GenerationParams,

    /// Prompt version (for co-versioning)
    pub prompt_version: String,

    /// Seed policy
    pub seed_policy: SeedPolicy,

    /// Stopping criteria
    pub stopping: StoppingCriteria,

    /// Created timestamp (for audit)
    pub created_at: String,

    /// Optional LoRA adapter ID (explicit, not default)
    ///
    /// IMPORTANT: This field is Optional because adapters are OPTIONAL.
    /// When None, the base model runs without adaptation.
    /// When Some, the specified adapter is loaded and applied.
    ///
    /// Per converge-core axioms, adapter selection is EXPLICIT:
    /// - No default adapter is ever applied
    /// - The caller must explicitly choose to use an adapter
    #[serde(skip_serializing_if = "Option::is_none")]
    pub adapter_id: Option<String>,
}

impl InferenceEnvelope {
    /// Create a deterministic envelope (greedy, fixed seed).
    ///
    /// Use this for regression tests and reproducible agent behavior.
    #[must_use]
    pub fn deterministic(prompt_version: impl Into<String>, seed: u64) -> Self {
        Self {
            version: 1,
            tokenizer_config: TokenizerSnapshot::llama3_default(),
            generation: GenerationParams::greedy(),
            prompt_version: prompt_version.into(),
            seed_policy: SeedPolicy::Fixed(seed),
            stopping: StoppingCriteria::default(),
            created_at: timestamp_placeholder(),
            adapter_id: None, // No adapter by default (Explicit Authority)
        }
    }

    /// Create an envelope for agent reasoning (balanced sampling).
    #[must_use]
    pub fn agent_reasoning(prompt_version: impl Into<String>) -> Self {
        Self {
            version: 1,
            tokenizer_config: TokenizerSnapshot::llama3_default(),
            generation: GenerationParams::agent(),
            prompt_version: prompt_version.into(),
            seed_policy: SeedPolicy::Random,
            stopping: StoppingCriteria::default(),
            created_at: timestamp_placeholder(),
            adapter_id: None, // No adapter by default (Explicit Authority)
        }
    }

    /// Attach a LoRA adapter to this envelope.
    ///
    /// This explicitly sets the adapter to use for inference.
    /// Per converge-core axioms, adapter selection is explicit.
    #[must_use]
    pub fn with_adapter(mut self, adapter_id: impl Into<String>) -> Self {
        self.adapter_id = Some(adapter_id.into());
        self
    }

    /// Check if this envelope has an adapter attached.
    #[must_use]
    pub fn has_adapter(&self) -> bool {
        self.adapter_id.is_some()
    }

    /// Get the adapter ID if one is attached.
    #[must_use]
    pub fn adapter(&self) -> Option<&str> {
        self.adapter_id.as_deref()
    }

    /// Check if this envelope produces deterministic output.
    #[must_use]
    pub fn is_deterministic(&self) -> bool {
        matches!(self.seed_policy, SeedPolicy::Fixed(_)) && self.generation.temperature == 0.0
    }
}

/// Tokenizer configuration snapshot for reproducibility.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenizerSnapshot {
    /// Tokenizer type identifier
    pub tokenizer_type: String,
    /// BOS token ID
    pub bos_token_id: u32,
    /// EOS token ID
    pub eos_token_id: u32,
    /// Vocab size
    pub vocab_size: usize,
}

impl TokenizerSnapshot {
    /// Default snapshot for Llama 3 tiktoken.
    #[must_use]
    pub fn llama3_default() -> Self {
        Self {
            tokenizer_type: "tiktoken".to_string(),
            bos_token_id: 128000,
            eos_token_id: 128001,
            vocab_size: 128256,
        }
    }
}

/// Seed policy for sampling.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SeedPolicy {
    /// Fixed seed for reproducibility
    Fixed(u64),
    /// Random seed per run
    Random,
    /// Seed derived from input hash (same input = same seed)
    InputDerived,
}

/// Stopping criteria for generation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoppingCriteria {
    /// Maximum tokens to generate
    pub max_tokens: usize,
    /// Stop on EOS token
    pub stop_on_eos: bool,
    /// Stop sequences
    pub stop_sequences: Vec<String>,
    /// Timeout in milliseconds (0 = no timeout)
    pub timeout_ms: u64,
}

impl Default for StoppingCriteria {
    fn default() -> Self {
        Self {
            max_tokens: 256,
            stop_on_eos: true,
            stop_sequences: vec![],
            timeout_ms: 30000, // 30 seconds
        }
    }
}

fn timestamp_placeholder() -> String {
    "2026-01-16T00:00:00Z".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_params_validation() {
        let valid = GenerationParams::default();
        assert!(valid.validate().is_ok());

        let invalid = GenerationParams {
            temperature: -1.0,
            ..Default::default()
        };
        assert!(invalid.validate().is_err());

        let invalid_top_p = GenerationParams {
            top_p: 0.0,
            ..Default::default()
        };
        assert!(invalid_top_p.validate().is_err());
    }

    #[test]
    fn test_preset_params() {
        let greedy = GenerationParams::greedy();
        assert_eq!(greedy.temperature, 0.0);

        let creative = GenerationParams::creative();
        assert_eq!(creative.temperature, 1.0);

        let agent = GenerationParams::agent();
        assert!(agent.temperature > 0.0 && agent.temperature < 1.0);
    }

    #[test]
    fn test_deterministic_envelope() {
        let envelope = InferenceEnvelope::deterministic("reasoning:v1", 42);

        assert!(envelope.is_deterministic());
        assert_eq!(envelope.generation.temperature, 0.0);
        assert!(matches!(envelope.seed_policy, SeedPolicy::Fixed(42)));
        assert!(!envelope.has_adapter()); // No adapter by default
    }

    #[test]
    fn test_agent_envelope_not_deterministic() {
        let envelope = InferenceEnvelope::agent_reasoning("reasoning:v1");

        assert!(!envelope.is_deterministic());
        assert!(matches!(envelope.seed_policy, SeedPolicy::Random));
        assert!(!envelope.has_adapter()); // No adapter by default
    }

    #[test]
    fn test_envelope_with_adapter() {
        let envelope = InferenceEnvelope::deterministic("reasoning:v1", 42)
            .with_adapter("llm/grounded-answering@1.0.0+sha256:abc123");

        assert!(envelope.has_adapter());
        assert_eq!(
            envelope.adapter(),
            Some("llm/grounded-answering@1.0.0+sha256:abc123")
        );
        // Still deterministic even with adapter
        assert!(envelope.is_deterministic());
    }

    // =========================================================================
    // Sampling Function Tests
    // =========================================================================

    #[test]
    fn test_argmax() {
        assert_eq!(argmax(&[1.0, 3.0, 2.0]), 1);
        assert_eq!(argmax(&[5.0, 1.0, 2.0]), 0);
        assert_eq!(argmax(&[1.0, 2.0, 5.0]), 2);
        assert_eq!(argmax(&[1.0]), 0);
    }

    #[test]
    fn test_softmax_basic() {
        let logits = vec![1.0, 2.0, 3.0];
        let probs = softmax(&logits);

        // Sum should be 1.0
        let sum: f32 = probs.iter().sum();
        assert!((sum - 1.0).abs() < 1e-5);

        // Higher logit = higher probability
        assert!(probs[2] > probs[1]);
        assert!(probs[1] > probs[0]);
    }

    #[test]
    fn test_softmax_numerical_stability() {
        // Large logits shouldn't overflow
        let logits = vec![1000.0, 1001.0, 1002.0];
        let probs = softmax(&logits);

        let sum: f32 = probs.iter().sum();
        assert!((sum - 1.0).abs() < 1e-5);
        assert!(probs[2] > probs[1]);
    }

    #[test]
    fn test_softmax_uniform_input() {
        let logits = vec![1.0, 1.0, 1.0, 1.0];
        let probs = softmax(&logits);

        // All should be equal (0.25 each)
        for p in &probs {
            assert!((*p - 0.25).abs() < 1e-5);
        }
    }

    #[test]
    fn test_repetition_penalty() {
        let mut logits = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let context = vec![1, 3]; // Penalize tokens 1 and 3

        apply_repetition_penalty(&mut logits, &context, 2.0);

        // Token 1: 2.0 / 2.0 = 1.0
        assert!((logits[1] - 1.0).abs() < 1e-5);
        // Token 3: 4.0 / 2.0 = 2.0
        assert!((logits[3] - 2.0).abs() < 1e-5);
        // Other tokens unchanged
        assert!((logits[0] - 1.0).abs() < 1e-5);
        assert!((logits[2] - 3.0).abs() < 1e-5);
        assert!((logits[4] - 5.0).abs() < 1e-5);
    }

    #[test]
    fn test_repetition_penalty_negative_logits() {
        let mut logits = vec![-2.0, 1.0];
        let context = vec![0]; // Penalize token 0 (negative logit)

        apply_repetition_penalty(&mut logits, &context, 2.0);

        // Negative logits are multiplied (making them more negative)
        assert!((logits[0] - (-4.0)).abs() < 1e-5);
        // Positive logit unchanged
        assert!((logits[1] - 1.0).abs() < 1e-5);
    }

    #[test]
    fn test_top_p_filtering() {
        // Sorted by probability descending
        let sorted_probs = vec![
            (0, 0.5),  // cumulative: 0.5
            (1, 0.3),  // cumulative: 0.8
            (2, 0.15), // cumulative: 0.95
            (3, 0.05), // cumulative: 1.0
        ];

        // top_p = 0.8 should keep tokens 0 and 1
        let filtered = apply_top_p(&sorted_probs, 0.8);
        assert_eq!(filtered.len(), 2);
        assert_eq!(filtered[0].0, 0);
        assert_eq!(filtered[1].0, 1);

        // top_p = 0.5 should keep only token 0
        let filtered = apply_top_p(&sorted_probs, 0.5);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].0, 0);

        // top_p = 1.0 should keep all
        let filtered = apply_top_p(&sorted_probs, 1.0);
        assert_eq!(filtered.len(), 4);
    }

    #[test]
    fn test_top_p_always_keeps_one() {
        let sorted_probs = vec![(0, 0.1), (1, 0.1)];

        // Even with very low top_p, at least one token is kept
        let filtered = apply_top_p(&sorted_probs, 0.01);
        assert!(!filtered.is_empty());
    }

    #[test]
    fn test_sample_from_distribution_deterministic() {
        let dist = vec![(0, 0.1), (1, 0.2), (2, 0.7)];

        // Same seed should give same result
        let sample1 = sample_from_distribution(&dist, Some(42));
        let sample2 = sample_from_distribution(&dist, Some(42));
        assert_eq!(sample1, sample2);
    }

    #[test]
    fn test_sample_from_distribution_single_token() {
        let dist = vec![(5, 1.0)];
        let sample = sample_from_distribution(&dist, Some(42));
        assert_eq!(sample, 5);
    }

    #[test]
    fn test_sample_from_distribution_empty() {
        let dist: Vec<(usize, f32)> = vec![];
        let sample = sample_from_distribution(&dist, Some(42));
        assert_eq!(sample, 0); // Fallback
    }

    #[test]
    fn test_sample_distribution_over_many_runs() {
        // With probability 0.9 for token 1, it should be selected most of the time
        let dist = vec![(0, 0.05), (1, 0.9), (2, 0.05)];

        let mut counts = [0usize; 3];
        for seed in 0..1000 {
            let sample = sample_from_distribution(&dist, Some(seed));
            if sample < 3 {
                counts[sample] += 1;
            }
        }

        // Token 1 should be selected roughly 900 times (allow some variance)
        assert!(counts[1] > 800, "Token 1 count: {}", counts[1]);
        assert!(counts[0] < 100, "Token 0 count: {}", counts[0]);
        assert!(counts[2] < 100, "Token 2 count: {}", counts[2]);
    }
}
