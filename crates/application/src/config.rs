// Copyright 2024-2026 Reflective Labs
// SPDX-License-Identifier: MIT
// See LICENSE file in the project root for full license information.

//! Application configuration for Converge distribution.
//!
//! This module handles deployment-level configuration:
//! - Server binding (host, port)
//! - Enabled domain packs
//! - Provider configuration
//! - Auth and tenancy settings
//!
//! Note: This is **wiring configuration**, not business semantics.

use serde::{Deserialize, Serialize};

/// Application configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    /// Host to bind to.
    pub host: String,

    /// Port to bind to.
    pub port: u16,

    /// Enabled domain packs.
    pub enabled_packs: Vec<String>,

    /// Provider configuration.
    pub providers: ProviderConfig,

    /// Auth configuration.
    pub auth: AuthConfig,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            host: "0.0.0.0".to_string(),
            port: 8080,
            enabled_packs: vec!["growth-strategy".to_string(), "ask-converge".to_string()],
            providers: ProviderConfig::default(),
            auth: AuthConfig::default(),
        }
    }
}

/// Provider configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProviderConfig {
    /// Preferred providers in order.
    pub prefer: Vec<String>,

    /// Providers to exclude.
    pub exclude: Vec<String>,

    /// Per-provider overrides.
    pub overrides: std::collections::HashMap<String, ProviderOverride>,
}

/// Per-provider configuration override.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderOverride {
    /// Override the default model.
    pub model: Option<String>,

    /// Rate limit (requests per minute).
    pub rate_limit: Option<u32>,

    /// Timeout in milliseconds.
    pub timeout_ms: Option<u64>,
}

/// Authentication configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthConfig {
    /// Whether auth is enabled.
    pub enabled: bool,

    /// Auth provider type.
    pub provider: AuthProvider,
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            provider: AuthProvider::None,
        }
    }
}

/// Auth provider types.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthProvider {
    /// No authentication.
    None,
    /// API key authentication.
    ApiKey,
    /// JWT/OAuth.
    Jwt,
}
