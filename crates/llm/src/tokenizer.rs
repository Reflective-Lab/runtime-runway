// Copyright 2024-2026 Reflective Labs

//! Tokenizer abstraction for LLM inference.
//!
//! # CRITICAL: This module is correctness-critical
//!
//! Once wired to tiktoken-rs, this becomes one of the most important files
//! in the repo. Tokenization ambiguity means undefined inference correctness.
//!
//! ## Design Decisions (frozen early)
//!
//! - **Tokenizer family**: tiktoken (BPE) for Llama 3
//! - **BOS/EOS handling**: Manual (caller controls via `encode_with_special`)
//! - **Truncation**: NOT allowed (caller must check length pre-inference)
//! - **Special tokens**: Surfaced via `is_special_token()`, filtered in decode
//! - **Token counts**: Enforced by `InferenceEngine` before generation
//!
//! ## Correctness Guarantees
//!
//! 1. **Roundtrip safety**: `decode(encode(text))` preserves semantic content
//! 2. **Determinism**: Same input always produces same tokens
//! 3. **No silent truncation**: Overlength input returns error, never truncates
//! 4. **Special token transparency**: BOS/EOS/PAD are explicit, never hidden
//!
//! ## Why This Matters
//!
//! In Ollama, tokenization is "free" (hidden in the runtime).
//! In Burn, tokenization is part of your correctness surface.
//!
//! A tokenizer bug will NOT cause a crash. It will cause:
//! - Subtly wrong outputs
//! - Non-reproducible behavior
//! - Prompt injection vulnerabilities
//! - Context window miscalculations

use crate::config::{TokenizerConfig, TokenizerType};
use crate::error::{LlmError, LlmResult};
use serde::{Deserialize, Serialize};

/// Tokenizer for converting text to/from token IDs.
///
/// This is a first-class correctness concern:
/// - BPE vs SentencePiece behavior differs
/// - Unicode normalization rules matter
/// - BOS/EOS semantics must match model training
/// - Special token alignment across models
pub struct Tokenizer {
    config: TokenizerConfig,
    // In production, this would hold the actual tokenizer instance
    // For now, we'll use a placeholder that demonstrates the API
}

impl Tokenizer {
    /// Create a new tokenizer from configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if the tokenizer cannot be initialized.
    pub fn new(config: TokenizerConfig) -> LlmResult<Self> {
        // Validate config
        if config.tokenizer_type == TokenizerType::SentencePiece && config.vocab_path.is_none() {
            return Err(LlmError::ConfigError(
                "SentencePiece requires vocab_path".to_string(),
            ));
        }

        Ok(Self { config })
    }

    /// Create a default Llama 3 tokenizer.
    ///
    /// # Errors
    ///
    /// Returns an error if the tokenizer cannot be initialized.
    pub fn llama3() -> LlmResult<Self> {
        Self::new(TokenizerConfig::default())
    }

    /// Encode text to token IDs.
    ///
    /// # Errors
    ///
    /// Returns an error if tokenization fails.
    pub fn encode(&self, text: &str) -> LlmResult<Vec<u32>> {
        // In production, this would call tiktoken-rs or tokenizers
        // For now, demonstrate the API with a simple byte-based encoding
        match self.config.tokenizer_type {
            TokenizerType::Tiktoken => self.encode_tiktoken(text),
            TokenizerType::SentencePiece => self.encode_sentencepiece(text),
        }
    }

    /// Encode with special tokens (BOS/EOS).
    ///
    /// # Errors
    ///
    /// Returns an error if tokenization fails.
    pub fn encode_with_special(
        &self,
        text: &str,
        add_bos: bool,
        add_eos: bool,
    ) -> LlmResult<Vec<u32>> {
        let mut tokens = Vec::new();

        if add_bos {
            tokens.push(self.config.bos_token_id);
        }

        tokens.extend(self.encode(text)?);

        if add_eos {
            tokens.push(self.config.eos_token_id);
        }

        Ok(tokens)
    }

    /// Decode token IDs back to text.
    ///
    /// # Errors
    ///
    /// Returns an error if decoding fails.
    pub fn decode(&self, tokens: &[u32]) -> LlmResult<String> {
        // Filter out special tokens
        let filtered: Vec<u32> = tokens
            .iter()
            .copied()
            .filter(|&t| {
                t != self.config.bos_token_id
                    && t != self.config.eos_token_id
                    && t != self.config.pad_token_id
            })
            .collect();

        match self.config.tokenizer_type {
            TokenizerType::Tiktoken => self.decode_tiktoken(&filtered),
            TokenizerType::SentencePiece => self.decode_sentencepiece(&filtered),
        }
    }

    /// Get the vocabulary size.
    #[must_use]
    pub fn vocab_size(&self) -> usize {
        match self.config.tokenizer_type {
            TokenizerType::Tiktoken => 128256,     // Llama 3 vocab size
            TokenizerType::SentencePiece => 32000, // Llama 2 default
        }
    }

    /// Check if a token ID is a special token.
    #[must_use]
    pub fn is_special_token(&self, token_id: u32) -> bool {
        token_id == self.config.bos_token_id
            || token_id == self.config.eos_token_id
            || token_id == self.config.pad_token_id
    }

    /// Get the BOS token ID.
    #[must_use]
    pub fn bos_token_id(&self) -> u32 {
        self.config.bos_token_id
    }

    /// Get the EOS token ID.
    #[must_use]
    pub fn eos_token_id(&self) -> u32 {
        self.config.eos_token_id
    }

    /// Encode and validate against a maximum length.
    ///
    /// This is the PREFERRED method for inference. It guarantees:
    /// - No silent truncation
    /// - Clear error on overlength
    /// - Correct special token handling
    ///
    /// # Errors
    ///
    /// Returns `ContextLengthExceeded` if the encoded length exceeds `max_tokens`.
    pub fn encode_validated(
        &self,
        text: &str,
        add_bos: bool,
        add_eos: bool,
        max_tokens: usize,
    ) -> LlmResult<Vec<u32>> {
        let tokens = self.encode_with_special(text, add_bos, add_eos)?;

        if tokens.len() > max_tokens {
            return Err(LlmError::ContextLengthExceeded {
                got: tokens.len(),
                max: max_tokens,
            });
        }

        Ok(tokens)
    }

    /// Estimate token count without full encoding.
    ///
    /// Useful for budget checks before expensive operations.
    /// Returns a conservative estimate (may overestimate).
    #[must_use]
    pub fn estimate_tokens(&self, text: &str) -> usize {
        // Conservative estimate: ~4 chars per token for English
        // This is intentionally pessimistic to avoid context overflows
        (text.len() + 3) / 4
    }

    // Private implementation methods

    fn encode_tiktoken(&self, text: &str) -> LlmResult<Vec<u32>> {
        // TODO: Integrate with tiktoken-rs
        // For now, simple placeholder that converts to bytes
        // This is NOT production-ready - just demonstrates the API
        Ok(text.bytes().map(u32::from).collect())
    }

    fn encode_sentencepiece(&self, _text: &str) -> LlmResult<Vec<u32>> {
        Err(LlmError::TokenizationError(
            "SentencePiece not yet implemented".to_string(),
        ))
    }

    fn decode_tiktoken(&self, tokens: &[u32]) -> LlmResult<String> {
        // TODO: Integrate with tiktoken-rs
        // For now, simple placeholder
        let bytes: Vec<u8> = tokens
            .iter()
            .filter_map(|&t| if t < 256 { Some(t as u8) } else { None })
            .collect();

        String::from_utf8(bytes).map_err(|e| LlmError::TokenizationError(e.to_string()))
    }

    fn decode_sentencepiece(&self, _tokens: &[u32]) -> LlmResult<String> {
        Err(LlmError::TokenizationError(
            "SentencePiece not yet implemented".to_string(),
        ))
    }
}

/// Token sequence with metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenSequence {
    /// The token IDs.
    pub tokens: Vec<u32>,
    /// Whether BOS was added.
    pub has_bos: bool,
    /// Whether EOS was added.
    pub has_eos: bool,
    /// Original text length (characters).
    pub original_length: usize,
}

impl TokenSequence {
    /// Create a new token sequence.
    #[must_use]
    pub fn new(tokens: Vec<u32>, has_bos: bool, has_eos: bool, original_length: usize) -> Self {
        Self {
            tokens,
            has_bos,
            has_eos,
            original_length,
        }
    }

    /// Get the number of tokens.
    #[must_use]
    pub fn len(&self) -> usize {
        self.tokens.len()
    }

    /// Check if the sequence is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tokens.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tokenizer_creation() {
        let tokenizer = Tokenizer::llama3().unwrap();
        assert_eq!(tokenizer.vocab_size(), 128256);
    }

    #[test]
    fn test_special_tokens() {
        let tokenizer = Tokenizer::llama3().unwrap();
        assert!(tokenizer.is_special_token(128000)); // BOS
        assert!(tokenizer.is_special_token(128001)); // EOS
        assert!(!tokenizer.is_special_token(100)); // Regular token
    }

    #[test]
    fn test_encode_with_special() {
        let tokenizer = Tokenizer::llama3().unwrap();
        let tokens = tokenizer.encode_with_special("hi", true, true).unwrap();
        assert_eq!(tokens[0], 128000); // BOS
        assert_eq!(*tokens.last().unwrap(), 128001); // EOS
    }
}
