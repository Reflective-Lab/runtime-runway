//! Runtime configuration for the accounts crate.
//!
//! Populated by the binary at startup from env vars and passed into
//! `AccountsState::new`. Library code reads config from the struct,
//! never directly from the process environment.
//!
//! `AccountsConfig` intentionally has no `from_env` constructor — the
//! binary owns env reading and composes this struct from its own
//! top-level config (e.g. `RunwayConfig::accounts_config`).

use commerce_rails_stripe::CommerceRailsConfig;

#[derive(Debug, Clone)]
pub struct AccountsConfig {
    /// LOCAL_DEV mode: skip side-effects (e.g. Firebase custom claims).
    pub local_dev: bool,
    /// Base URL used when generating commerce checkout/portal return URLs.
    /// Production deployments must set this explicitly.
    pub app_url: String,
    /// Commerce Rails-owned commercial provider configuration.
    pub commerce: CommerceRailsConfig,
}

impl AccountsConfig {
    /// Convenience for tests and local development.
    pub fn local() -> Self {
        Self {
            local_dev: true,
            app_url: "http://localhost:3000".to_string(),
            commerce: CommerceRailsConfig::local(),
        }
    }
}
