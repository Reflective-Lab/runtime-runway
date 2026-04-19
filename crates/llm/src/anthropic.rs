// Copyright 2024-2026 Reflective Labs

//! Anthropic Claude Backend — Re-exported from converge-provider.
//!
//! This module re-exports `AnthropicBackend` from `converge-provider::llm`
//! for backward compatibility. The implementation lives in converge-provider
//! where it belongs with other remote provider implementations.
//!
//! # Usage
//!
//! ```ignore
//! use converge_llm::anthropic::AnthropicBackend;
//!
//! let backend = AnthropicBackend::new("your-api-key")
//!     .with_model("claude-sonnet-4-20250514");
//!
//! let response = backend.execute(&request)?;
//! // response.trace_link is RemoteTraceLink (audit-eligible only)
//! ```
//!
//! # Note
//!
//! This is a re-export. The canonical implementation is in:
//! `converge-provider::llm::anthropic`

pub use converge_provider::llm::AnthropicBackend;
