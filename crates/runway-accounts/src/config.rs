//! Runtime configuration for the accounts crate.
//!
//! Populated by the binary at startup from env vars and passed into
//! `AccountsState::new`. Library code reads config from the struct,
//! never directly from the process environment.
//!
//! `AccountsConfig` intentionally has no `from_env` constructor — the
//! binary owns env reading and composes this struct from its own
//! top-level config (e.g. `RunwayConfig::accounts_config`).

#[derive(Debug, Clone)]
pub struct AccountsConfig {
    /// LOCAL_DEV mode: skip side-effects (e.g. Firebase custom claims).
    pub local_dev: bool,
    /// Base URL used when generating Stripe checkout/portal return URLs.
    /// Production deployments must set this explicitly.
    pub app_url: String,
    /// Stripe webhook signing secret. Empty disables HMAC verification —
    /// acceptable in local development, but `RunwayConfig::from_env`
    /// rejects an empty value in production.
    pub stripe_webhook_secret: String,
    /// Stripe API secret key. Empty disables Stripe (customer lookups,
    /// checkout, portal, and subscription queries return None / error
    /// out cleanly).
    pub stripe_secret_key: String,
    /// Stripe price ID for the Team monthly plan. Empty means the Team
    /// plan is unavailable; subscriptions matching no configured price
    /// fall back to `Plan::Free`.
    pub stripe_price_team_monthly: String,
    /// Stripe price ID for the Starter monthly plan. Empty means the
    /// Starter plan is unavailable.
    pub stripe_price_starter_monthly: String,
}

impl AccountsConfig {
    /// Convenience for tests and local development.
    pub fn local() -> Self {
        Self {
            local_dev: true,
            app_url: "http://localhost:3000".to_string(),
            stripe_webhook_secret: String::new(),
            stripe_secret_key: String::new(),
            stripe_price_team_monthly: String::new(),
            stripe_price_starter_monthly: String::new(),
        }
    }
}
