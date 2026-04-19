// Copyright 2024-2026 Reflective Labs

//! In-process Gemma inference using llama.cpp GGUF models.
//!
//! This module is intentionally narrow:
//! - Gemma-first
//! - GGUF-first
//! - laptop-first (especially Apple Silicon)
//!
//! It provides a pragmatic embedded path for `converge-llm` when the priority
//! is local inference on a MacBook rather than a model-generic architecture.

use crate::error::{LlmError, LlmResult};
use crate::inference::{FinishReason, GenerationResult, InferenceEnvelope, SeedPolicy};
use crate::prompt::PromptStack;
use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::{AddBos, LlamaChatMessage, LlamaModel};
use llama_cpp_2::sampling::LlamaSampler;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};

/// Mac-oriented runtime configuration for local Gemma GGUF inference.
#[derive(Debug, Clone)]
pub struct GemmaConfig {
    /// Local GGUF model path.
    pub model_path: PathBuf,
    /// Context window to allocate.
    pub context_len: usize,
    /// Batch size used when feeding prompt tokens to llama.cpp.
    pub batch_size: usize,
    /// Threads for prompt processing and token generation.
    pub threads: u32,
    /// Number of layers to offload to Metal. `u32::MAX` means "as many as possible".
    pub gpu_layers: u32,
    /// Whether to use the model's baked chat template when available.
    pub use_chat_template: bool,
}

impl GemmaConfig {
    /// Build a Gemma config for a local GGUF file.
    #[must_use]
    pub fn new(model_path: impl Into<PathBuf>) -> Self {
        let threads = std::thread::available_parallelism()
            .ok()
            .and_then(|value| u32::try_from(value.get()).ok())
            .unwrap_or(8);

        Self {
            model_path: model_path.into(),
            context_len: 4096,
            batch_size: 1024,
            threads,
            gpu_layers: u32::MAX,
            use_chat_template: true,
        }
    }

    /// Convenience for a Gemma 7B-style Q4 local profile.
    #[must_use]
    pub fn gemma_7b_q4(model_path: impl Into<PathBuf>) -> Self {
        Self::new(model_path)
    }

    /// Load config from environment for examples and local tooling.
    ///
    /// Required:
    /// - `CONVERGE_GEMMA_MODEL_PATH`
    ///
    /// Optional:
    /// - `CONVERGE_GEMMA_CONTEXT_LEN`
    /// - `CONVERGE_GEMMA_BATCH_SIZE`
    /// - `CONVERGE_GEMMA_THREADS`
    /// - `CONVERGE_GEMMA_GPU_LAYERS`
    ///
    /// # Errors
    ///
    /// Returns an error if the required model path is missing or an override
    /// cannot be parsed.
    pub fn from_env() -> LlmResult<Self> {
        let model_path = std::env::var("CONVERGE_GEMMA_MODEL_PATH").map_err(|_| {
            LlmError::ConfigError(
                "CONVERGE_GEMMA_MODEL_PATH must point to a local Gemma GGUF file".to_string(),
            )
        })?;

        let mut config = Self::gemma_7b_q4(model_path);

        if let Ok(value) = std::env::var("CONVERGE_GEMMA_CONTEXT_LEN") {
            config.context_len = parse_env_usize("CONVERGE_GEMMA_CONTEXT_LEN", &value)?;
        }

        if let Ok(value) = std::env::var("CONVERGE_GEMMA_BATCH_SIZE") {
            config.batch_size = parse_env_usize("CONVERGE_GEMMA_BATCH_SIZE", &value)?;
        }

        if let Ok(value) = std::env::var("CONVERGE_GEMMA_THREADS") {
            config.threads = parse_env_u32("CONVERGE_GEMMA_THREADS", &value)?;
        }

        if let Ok(value) = std::env::var("CONVERGE_GEMMA_GPU_LAYERS") {
            config.gpu_layers = parse_env_u32("CONVERGE_GEMMA_GPU_LAYERS", &value)?;
        }

        Ok(config)
    }

    /// Validate local runtime configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if the config cannot be used for llama.cpp inference.
    pub fn validate(&self) -> LlmResult<()> {
        if !self.model_path.exists() {
            return Err(LlmError::ConfigError(format!(
                "Gemma model not found at {}",
                self.model_path.display()
            )));
        }

        if self
            .model_path
            .extension()
            .and_then(std::ffi::OsStr::to_str)
            != Some("gguf")
        {
            return Err(LlmError::ConfigError(format!(
                "Gemma model must be a GGUF file: {}",
                self.model_path.display()
            )));
        }

        if self.context_len == 0 {
            return Err(LlmError::ConfigError(
                "Gemma context length must be greater than zero".to_string(),
            ));
        }

        if self.batch_size == 0 {
            return Err(LlmError::ConfigError(
                "Gemma batch size must be greater than zero".to_string(),
            ));
        }

        if self.threads == 0 {
            return Err(LlmError::ConfigError(
                "Gemma thread count must be greater than zero".to_string(),
            ));
        }

        Ok(())
    }
}

/// Embedded Gemma inference engine backed by llama.cpp.
pub struct GemmaEngine {
    backend: LlamaBackend,
    model: LlamaModel,
    config: GemmaConfig,
}

impl GemmaEngine {
    /// Load a Gemma GGUF model into an embedded llama.cpp runtime.
    ///
    /// # Errors
    ///
    /// Returns an error if the config is invalid, the llama backend cannot be
    /// initialized, or the model cannot be loaded.
    pub fn load_from_gguf(config: GemmaConfig) -> LlmResult<Self> {
        config.validate()?;

        if !looks_like_q4_model(&config.model_path) {
            tracing::warn!(
                path = %config.model_path.display(),
                "Gemma model path does not look like a Q4 GGUF; local MacBook performance may be worse than expected"
            );
        }

        let backend = LlamaBackend::init().map_err(|error| {
            LlmError::InferenceError(format!("failed to initialize llama.cpp backend: {error}"))
        })?;

        let model_params = if config.gpu_layers == 0 {
            LlamaModelParams::default()
        } else {
            LlamaModelParams::default().with_n_gpu_layers(config.gpu_layers)
        };

        let model = LlamaModel::load_from_file(&backend, &config.model_path, &model_params)
            .map_err(|error| {
                LlmError::WeightLoadError(format!(
                    "failed to load Gemma GGUF model from {}: {error}",
                    config.model_path.display()
                ))
            })?;

        Ok(Self {
            backend,
            model,
            config,
        })
    }

    /// Access the current engine config.
    #[must_use]
    pub fn config(&self) -> &GemmaConfig {
        &self.config
    }

    /// Run inference using the crate's contract-driven prompt stack.
    ///
    /// # Errors
    ///
    /// Returns an error if prompt rendering, tokenization, decode, or sampling fails.
    pub fn run(
        &mut self,
        stack: &PromptStack,
        envelope: &InferenceEnvelope,
    ) -> LlmResult<GenerationResult> {
        let (prompt, add_bos) = self.render_prompt(stack);
        let prompt_tokens = self.model.str_to_token(&prompt, add_bos).map_err(|error| {
            LlmError::TokenizationError(format!("failed to tokenize Gemma prompt: {error}"))
        })?;

        let max_new_tokens = envelope.stopping.max_tokens;
        let max_context = self.config.context_len;
        if prompt_tokens.len() + max_new_tokens > max_context {
            return Err(LlmError::ContextLengthExceeded {
                got: prompt_tokens.len() + max_new_tokens,
                max: max_context,
            });
        }

        let threads = i32::try_from(self.config.threads).map_err(|_| {
            LlmError::ConfigError("Gemma thread count does not fit into i32".to_string())
        })?;

        // Set n_batch large enough to hold the full prompt so get_one works
        // in a single decode pass (positions are auto-assigned correctly).
        let effective_batch = prompt_tokens
            .len()
            .max(self.config.batch_size)
            .min(max_context);
        let ctx_params = LlamaContextParams::default()
            .with_n_ctx(Some(non_zero_u32(
                "context length",
                self.config.context_len,
            )?))
            .with_n_batch(u32::try_from(effective_batch).map_err(|_| {
                LlmError::ConfigError("Gemma batch size does not fit into u32".to_string())
            })?)
            .with_n_threads(threads)
            .with_n_threads_batch(threads);

        let mut context = self
            .model
            .new_context(&self.backend, ctx_params)
            .map_err(|error| {
                LlmError::InferenceError(format!("failed to create Gemma context: {error}"))
            })?;

        let mut prompt_batch = LlamaBatch::get_one(&prompt_tokens).map_err(|error| {
            LlmError::InferenceError(format!("failed to prepare prompt batch: {error}"))
        })?;
        context.decode(&mut prompt_batch).map_err(|error| {
            LlmError::InferenceError(format!("failed to decode Gemma prompt: {error}"))
        })?;

        let mut sampler = self.create_sampler(envelope);
        sampler.accept_many(prompt_tokens.iter());

        let mut decode_batch = LlamaBatch::new(1, 1);
        let mut decoder = encoding_rs::UTF_8.new_decoder();
        let mut output = String::new();
        let mut generated_tokens = 0usize;
        let mut finish_reason = FinishReason::Length;
        let mut position = i32::try_from(prompt_tokens.len()).map_err(|_| {
            LlmError::InferenceError("prompt token length does not fit into i32".to_string())
        })?;

        // After prompt decode with get_one, logits are only at the last token.
        // First sample uses that index; subsequent samples use index 0 (single-token batch).
        let prompt_logits_idx = i32::try_from(prompt_tokens.len().saturating_sub(1)).unwrap_or(0);
        let mut sample_idx = prompt_logits_idx;

        for _ in 0..max_new_tokens {
            let token = sampler.sample(&context, sample_idx);
            sample_idx = 0; // subsequent iterations sample from single-token decode batches
            sampler.accept(token);

            if self.model.is_eog_token(token) {
                finish_reason = FinishReason::Eos;
                break;
            }

            let piece = self
                .model
                .token_to_piece(token, &mut decoder, true, None)
                .map_err(|error| {
                    LlmError::InferenceError(format!("failed to decode Gemma token: {error}"))
                })?;
            output.push_str(&piece);
            generated_tokens += 1;

            if let Some(stop_at) =
                first_stop_sequence_index(&output, &envelope.generation.stop_sequences)
            {
                output.truncate(stop_at);
                finish_reason = FinishReason::StopSequence;
                break;
            }

            decode_batch.clear();
            decode_batch
                .add(token, position, &[0], true)
                .map_err(|error| {
                    LlmError::InferenceError(format!(
                        "failed to extend Gemma decode batch: {error}"
                    ))
                })?;
            context.decode(&mut decode_batch).map_err(|error| {
                LlmError::InferenceError(format!("failed to decode Gemma token batch: {error}"))
            })?;
            position += 1;
        }

        Ok(GenerationResult {
            text: output,
            input_tokens: prompt_tokens.len(),
            output_tokens: generated_tokens,
            finish_reason,
        })
    }

    fn create_sampler(&self, envelope: &InferenceEnvelope) -> LlamaSampler {
        let params = &envelope.generation;
        let penalty_last_n = i32::try_from(self.config.context_len).unwrap_or(i32::MAX);

        if envelope.is_deterministic() {
            return LlamaSampler::chain_simple([
                LlamaSampler::penalties(penalty_last_n, params.repetition_penalty, 0.0, 0.0),
                LlamaSampler::greedy(),
            ]);
        }

        let seed = seed_from_envelope(envelope);
        let mut samplers = vec![
            LlamaSampler::penalties(penalty_last_n, params.repetition_penalty, 0.0, 0.0),
            LlamaSampler::temp(params.temperature),
        ];
        if params.top_k > 0 {
            samplers.push(LlamaSampler::top_k(
                i32::try_from(params.top_k).unwrap_or(i32::MAX),
            ));
        }
        if params.top_p < 1.0 {
            samplers.push(LlamaSampler::top_p(params.top_p, 1));
        }
        samplers.push(LlamaSampler::dist(seed));
        LlamaSampler::chain_simple(samplers)
    }

    fn render_prompt(&self, stack: &PromptStack) -> (String, AddBos) {
        let rendered = stack.render();

        if self.config.use_chat_template {
            match self.try_render_chat_prompt(&rendered) {
                Ok(prompt) => return (prompt, AddBos::Never),
                Err(error) => {
                    tracing::warn!(
                        error = %error,
                        "Model chat template failed, using built-in Gemma turn format"
                    );
                }
            }
        }

        // Standard Gemma instruction format (works for Gemma 2 and 4).
        // BOS is prepended by llama.cpp via AddBos::Always.
        let prompt =
            format!("<start_of_turn>user\n{rendered}<end_of_turn>\n<start_of_turn>model\n");
        (prompt, AddBos::Always)
    }

    fn try_render_chat_prompt(&self, rendered: &str) -> LlmResult<String> {
        let template = self.model.chat_template(None).map_err(|error| {
            LlmError::ConfigError(format!(
                "Gemma model does not expose a chat template: {error}"
            ))
        })?;

        let message =
            LlamaChatMessage::new("user".to_string(), rendered.to_string()).map_err(|error| {
                LlmError::ConfigError(format!("failed to build Gemma chat message: {error}"))
            })?;

        self.model
            .apply_chat_template(&template, &[message], true)
            .map_err(|error| {
                LlmError::ConfigError(format!("failed to apply Gemma chat template: {error}"))
            })
    }
}

fn seed_from_envelope(envelope: &InferenceEnvelope) -> u32 {
    match envelope.seed_policy {
        SeedPolicy::Fixed(seed) => u32::try_from(seed).unwrap_or(u32::MAX),
        SeedPolicy::Random => rand::random(),
        SeedPolicy::InputDerived => {
            let mut hasher = DefaultHasher::new();
            envelope.prompt_version.hash(&mut hasher);
            u32::try_from(hasher.finish()).unwrap_or(u32::MAX)
        }
    }
}

fn looks_like_q4_model(path: &Path) -> bool {
    path.file_name()
        .and_then(std::ffi::OsStr::to_str)
        .map(|file_name| file_name.to_ascii_lowercase().contains("q4"))
        .unwrap_or(false)
}

fn first_stop_sequence_index(output: &str, stop_sequences: &[String]) -> Option<usize> {
    stop_sequences
        .iter()
        .filter(|sequence| !sequence.is_empty())
        .filter_map(|sequence| output.find(sequence))
        .min()
}

fn non_zero_u32(label: &str, value: usize) -> LlmResult<NonZeroU32> {
    let converted = u32::try_from(value)
        .map_err(|_| LlmError::ConfigError(format!("{label} does not fit into u32")))?;
    NonZeroU32::new(converted)
        .ok_or_else(|| LlmError::ConfigError(format!("{label} must be greater than zero")))
}

fn parse_env_usize(name: &str, value: &str) -> LlmResult<usize> {
    value
        .parse::<usize>()
        .map_err(|error| LlmError::ConfigError(format!("failed to parse {name} as usize: {error}")))
}

fn parse_env_u32(name: &str, value: &str) -> LlmResult<u32> {
    value
        .parse::<u32>()
        .map_err(|error| LlmError::ConfigError(format!("failed to parse {name} as u32: {error}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn q4_detector_handles_common_filenames() {
        assert!(looks_like_q4_model(Path::new("gemma-7b-it-Q4_K_M.gguf")));
        assert!(looks_like_q4_model(Path::new("gemma-7b-q4.gguf")));
        assert!(!looks_like_q4_model(Path::new("gemma-7b-f16.gguf")));
    }

    #[test]
    fn stop_sequence_detection_returns_earliest_hit() {
        let output = "hello stop there done";
        let stop_sequences = vec!["done".to_string(), "stop".to_string()];
        assert_eq!(first_stop_sequence_index(output, &stop_sequences), Some(6));
    }
}
