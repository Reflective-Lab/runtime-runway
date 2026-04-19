// Copyright 2024-2026 Reflective Labs

//! Configuration for LLM models and inference.

use serde::{Deserialize, Serialize};

/// Configuration for the LLM system.
///
/// These choices are architectural and should be frozen early:
/// - Tokenizer family determines compatibility across model swaps
/// - Context length affects memory footprint significantly
/// - Precision affects both speed and quality
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfig {
    /// Model identifier (e.g., "llama3-8b", "llama3-3b")
    pub model_id: String,

    /// Path to model weights (local or will be downloaded)
    pub weights_path: Option<String>,

    /// Maximum context length in tokens
    /// Rule: context_length × step_count matters more than param count
    pub max_context_length: usize,

    /// Precision for inference
    pub precision: Precision,

    /// Tokenizer configuration
    pub tokenizer: TokenizerConfig,

    /// Whether LoRA adapters are enabled
    pub lora_enabled: bool,

    /// Path to LoRA adapter weights (if any)
    pub lora_path: Option<String>,
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            model_id: "llama3-8b".to_string(),
            weights_path: None,
            max_context_length: 4096,
            precision: Precision::Fp16,
            tokenizer: TokenizerConfig::default(),
            lora_enabled: false,
            lora_path: None,
        }
    }
}

impl LlmConfig {
    /// Validate configuration for internal consistency.
    ///
    /// This checks that model, tokenizer, precision, and context length
    /// are mutually compatible. Call this early to fail fast on mismatches.
    ///
    /// # Errors
    ///
    /// Returns an error describing the incompatibility.
    pub fn validate(&self) -> Result<(), ConfigValidationError> {
        // Check tokenizer/model compatibility
        match (&self.tokenizer.tokenizer_type, self.model_id.as_str()) {
            // Llama 3 uses tiktoken
            (TokenizerType::Tiktoken, id) if id.contains("llama3") => {}
            // Gemma uses SentencePiece
            (TokenizerType::SentencePiece, id) if id.contains("gemma") => {}
            // Llama 2 uses SentencePiece
            (TokenizerType::SentencePiece, id) if id.contains("llama2") => {}
            // TinyLlama uses SentencePiece
            (TokenizerType::SentencePiece, id) if id.contains("tiny") => {}
            // Tiktoken with TinyLlama - warn but allow (some variants support it)
            (TokenizerType::Tiktoken, id) if id.contains("tiny") => {}
            // Mismatch
            (tok, model) => {
                return Err(ConfigValidationError::TokenizerModelMismatch {
                    tokenizer: format!("{tok:?}"),
                    model: model.to_string(),
                });
            }
        }

        // Check precision/model compatibility
        if self.precision == Precision::Int4 && self.lora_enabled {
            return Err(ConfigValidationError::IncompatiblePrecision {
                precision: "INT4".to_string(),
                reason: "LoRA requires at least INT8 precision for gradient flow".to_string(),
            });
        }

        // Check context length is reasonable for model size
        let param_billions = self.estimated_param_count();
        let max_reasonable_context = match param_billions {
            b if b < 2.0 => 2048,
            b if b < 4.0 => 4096,
            b if b < 10.0 => 8192,
            _ => 32768,
        };

        if self.max_context_length > max_reasonable_context {
            return Err(ConfigValidationError::ContextTooLarge {
                requested: self.max_context_length,
                recommended_max: max_reasonable_context,
                model: self.model_id.clone(),
            });
        }

        // Check special tokens are valid for tokenizer
        if self.tokenizer.tokenizer_type == TokenizerType::Tiktoken {
            // Llama 3 tiktoken vocab is 128256
            if self.tokenizer.bos_token_id >= 128256 || self.tokenizer.eos_token_id >= 128256 {
                return Err(ConfigValidationError::InvalidSpecialTokens {
                    reason: "Token IDs exceed tiktoken vocab size (128256)".to_string(),
                });
            }
        }

        Ok(())
    }

    /// Estimated parameter count in billions.
    fn estimated_param_count(&self) -> f64 {
        match self.model_id.as_str() {
            id if id.contains("1b") || id.contains("tiny") => 1.1,
            id if id.contains("3b") => 3.0,
            id if id.contains("7b") || id.contains("8b") => 8.0,
            id if id.contains("13b") => 13.0,
            id if id.contains("70b") => 70.0,
            _ => 8.0, // default assumption
        }
    }

    /// Create a minimal config for a 3B model (lower memory footprint).
    #[must_use]
    pub fn small() -> Self {
        Self {
            model_id: "llama3-3b".to_string(),
            max_context_length: 2048,
            precision: Precision::Int8,
            ..Default::default()
        }
    }

    /// Create a config for a 7B model with longer context.
    #[must_use]
    pub fn medium() -> Self {
        Self {
            model_id: "llama3-8b".to_string(),
            max_context_length: 8192,
            precision: Precision::Fp16,
            ..Default::default()
        }
    }

    /// Estimated memory footprint in GB.
    #[must_use]
    pub fn estimated_memory_gb(&self) -> f64 {
        let param_billions = match self.model_id.as_str() {
            id if id.contains("3b") => 3.0,
            id if id.contains("7b") || id.contains("8b") => 8.0,
            id if id.contains("13b") => 13.0,
            id if id.contains("70b") => 70.0,
            _ => 8.0, // default assumption
        };

        let bytes_per_param = match self.precision {
            Precision::Fp32 => 4.0,
            Precision::Fp16 | Precision::Bf16 => 2.0,
            Precision::Int8 => 1.0,
            Precision::Int4 => 0.5,
        };

        // Model weights + KV cache overhead
        let weights_gb = param_billions * bytes_per_param;
        let kv_cache_gb = (self.max_context_length as f64 / 1000.0) * 0.5; // rough estimate

        weights_gb + kv_cache_gb
    }
}

/// Precision for model weights and computation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Precision {
    Fp32,
    Fp16,
    Bf16,
    Int8,
    Int4,
}

impl Default for Precision {
    fn default() -> Self {
        Self::Fp16
    }
}

/// Tokenizer configuration.
///
/// CRITICAL: Choose one tokenizer family early and freeze it.
/// This is one of the biggest sources of accidental divergence.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenizerConfig {
    /// Tokenizer type
    pub tokenizer_type: TokenizerType,

    /// Path to tokenizer vocab (if custom)
    pub vocab_path: Option<String>,

    /// BOS token ID
    pub bos_token_id: u32,

    /// EOS token ID
    pub eos_token_id: u32,

    /// Pad token ID
    pub pad_token_id: u32,
}

impl Default for TokenizerConfig {
    fn default() -> Self {
        Self {
            tokenizer_type: TokenizerType::Tiktoken,
            vocab_path: None,
            bos_token_id: 128000, // Llama 3 default
            eos_token_id: 128001,
            pad_token_id: 128002,
        }
    }
}

/// Tokenizer type - choose one and stick with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TokenizerType {
    /// BPE tokenizer (tiktoken, used by Llama 3)
    Tiktoken,
    /// SentencePiece (used by older Llama models)
    SentencePiece,
}

impl Default for TokenizerType {
    fn default() -> Self {
        Self::Tiktoken
    }
}

/// Configuration validation errors.
///
/// These represent incompatible configuration combinations that should
/// fail fast at startup rather than cause subtle inference bugs later.
#[derive(Debug, Clone, thiserror::Error)]
pub enum ConfigValidationError {
    #[error("Tokenizer {tokenizer} is incompatible with model {model}")]
    TokenizerModelMismatch { tokenizer: String, model: String },

    #[error("Precision {precision} is incompatible: {reason}")]
    IncompatiblePrecision { precision: String, reason: String },

    #[error(
        "Context length {requested} is too large for {model} (recommended max: {recommended_max})"
    )]
    ContextTooLarge {
        requested: usize,
        recommended_max: usize,
        model: String,
    },

    #[error("Invalid special tokens: {reason}")]
    InvalidSpecialTokens { reason: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_estimation() {
        let small = LlmConfig::small();
        let medium = LlmConfig::medium();

        // 3B INT8 should be much smaller than 8B FP16
        assert!(small.estimated_memory_gb() < medium.estimated_memory_gb());
        assert!(small.estimated_memory_gb() < 5.0);
        assert!(medium.estimated_memory_gb() > 10.0);
    }

    #[test]
    fn test_default_config() {
        let config = LlmConfig::default();
        assert_eq!(config.max_context_length, 4096);
        assert_eq!(config.precision, Precision::Fp16);
        assert!(!config.lora_enabled);
    }

    #[test]
    fn test_validate_default_config() {
        let config = LlmConfig::default();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_validate_small_config() {
        let config = LlmConfig::small();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_validate_medium_config() {
        let config = LlmConfig::medium();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_validate_gemma_sentencepiece_config() {
        let mut config = LlmConfig::medium();
        config.model_id = "gemma-7b-it-q4".to_string();
        config.tokenizer.tokenizer_type = TokenizerType::SentencePiece;
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_validate_lora_with_int4_fails() {
        let mut config = LlmConfig::default();
        config.precision = Precision::Int4;
        config.lora_enabled = true;

        let result = config.validate();
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            ConfigValidationError::IncompatiblePrecision { .. }
        ));
    }

    #[test]
    fn test_validate_excessive_context_fails() {
        let mut config = LlmConfig::small(); // 3B model
        config.max_context_length = 32768; // Way too large for 3B

        let result = config.validate();
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            ConfigValidationError::ContextTooLarge { .. }
        ));
    }
}
