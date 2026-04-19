// Copyright 2024-2026 Reflective Labs

//! Error types for the LLM module.

use thiserror::Error;

/// Errors that can occur during LLM operations.
#[derive(Error, Debug)]
pub enum LlmError {
    #[error("Model not loaded: {0}")]
    ModelNotLoaded(String),

    #[error("Tokenization failed: {0}")]
    TokenizationError(String),

    #[error("Inference failed: {0}")]
    InferenceError(String),

    #[error("Configuration error: {0}")]
    ConfigError(String),

    #[error("Weight loading failed: {0}")]
    WeightLoadError(String),

    #[error("Context length exceeded: got {got}, max {max}")]
    ContextLengthExceeded { got: usize, max: usize },

    #[error("Invalid generation parameters: {0}")]
    InvalidParams(String),

    // --- Adapter Errors ---
    #[error("Adapter error: {0}")]
    AdapterError(String),

    #[error("Adapter not found: {0}")]
    AdapterNotFound(String),

    #[error("Adapter incompatible: {reason}")]
    AdapterIncompatible { reason: String },

    #[error("Adapter policy violation: {0}")]
    AdapterPolicyViolation(String),

    #[error("Adapter loading failed: {0}")]
    AdapterLoadError(String),

    // --- Policy Enforcement Errors ---
    #[error("Policy violation: {reason}")]
    PolicyViolation { reason: String },

    #[error("Policy override attempted: {field} cannot be changed after compilation")]
    PolicyOverrideAttempted { field: String },

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

/// Result type for LLM operations.
pub type LlmResult<T> = Result<T, LlmError>;
