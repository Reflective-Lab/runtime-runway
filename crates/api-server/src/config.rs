//! Top-level runtime configuration for the api-server binary.
//!
//! Loaded once from environment variables in `main`, then passed by
//! field-value into the services that need them. Library crates never
//! read env directly.

use anyhow::Result;
use runway_accounts::AccountsConfig;

#[derive(Debug, Clone)]
pub struct RunwayConfig {
    pub local_dev: bool,
    pub storage_path: String,
    pub firebase_project_id: String,
    pub route_prefix: Option<String>,
    pub app_url: String,
    /// Comma-separated CORS allow-list. Required in production; empty in
    /// local_dev means "allow any origin".
    pub allowed_origins: String,
    /// Stripe webhook signing secret. Required in production.
    pub stripe_webhook_secret: String,
    /// Stripe API secret key. Optional — empty disables Stripe entirely
    /// (customer lookups, checkout, portal all degrade to no-ops).
    pub stripe_secret_key: String,
    /// Stripe price ID for the Team monthly plan. Optional.
    pub stripe_price_team_monthly: String,
    /// Stripe price ID for the Starter monthly plan. Optional.
    pub stripe_price_starter_monthly: String,
    /// TCP port to listen on. Cloud Run injects `PORT`; local dev
    /// defaults to 8080.
    pub port: u16,
}

impl RunwayConfig {
    pub fn from_env() -> Result<Self> {
        let local_dev = std::env::var("LOCAL_DEV").as_deref() == Ok("true");

        let allowed_origins = std::env::var("ALLOWED_ORIGINS").unwrap_or_default();
        let stripe_webhook_secret = std::env::var("STRIPE_WEBHOOK_SECRET").unwrap_or_default();
        let stripe_secret_key = std::env::var("STRIPE_SECRET_KEY").unwrap_or_default();
        let stripe_price_team_monthly =
            std::env::var("STRIPE_PRICE_TEAM_MONTHLY").unwrap_or_default();
        let stripe_price_starter_monthly =
            std::env::var("STRIPE_PRICE_STARTER_MONTHLY").unwrap_or_default();

        if !local_dev {
            anyhow::ensure!(
                !stripe_webhook_secret.is_empty(),
                "STRIPE_WEBHOOK_SECRET must be set in production (empty value disables HMAC verification)"
            );
            anyhow::ensure!(
                !allowed_origins.is_empty(),
                "ALLOWED_ORIGINS must be set in production (e.g. https://apps.reflective.se)"
            );
        }

        let storage_path =
            std::env::var("STORAGE_PATH").unwrap_or_else(|_| "/tmp/api-server".to_string());

        let firebase_project_id = std::env::var("FIREBASE_PROJECT_ID")
            .or_else(|_| std::env::var("GOOGLE_CLOUD_PROJECT"))
            .or_else(|_| std::env::var("GCP_PROJECT_ID"))
            .unwrap_or_else(|_| "dev-project".to_string());

        // ROUTE_PREFIX=/api-server mounts all routes under that path.
        // Firebase Hosting rewrites pass the full path through, so this lets
        // apps.reflective.se/api-server/** route to this service.
        // /health always stays at root for Cloud Run health checks.
        let route_prefix = match std::env::var("ROUTE_PREFIX") {
            Ok(prefix) => {
                let trimmed = prefix.trim();
                if trimmed.is_empty() || trimmed == "/" {
                    None
                } else if trimmed.starts_with('/') {
                    Some(trimmed.to_string())
                } else {
                    Some(format!("/{trimmed}"))
                }
            }
            Err(_) => None,
        };

        let app_url =
            std::env::var("APP_URL").unwrap_or_else(|_| "https://apps.reflective.se".to_string());

        let port: u16 = std::env::var("PORT")
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or(8080);

        Ok(Self {
            local_dev,
            storage_path,
            firebase_project_id,
            route_prefix,
            app_url,
            allowed_origins,
            stripe_webhook_secret,
            stripe_secret_key,
            stripe_price_team_monthly,
            stripe_price_starter_monthly,
            port,
        })
    }

    /// Project the runway-wide config onto the subset that `runway-accounts` needs.
    pub fn accounts_config(&self) -> AccountsConfig {
        AccountsConfig {
            local_dev: self.local_dev,
            app_url: self.app_url.clone(),
            stripe_webhook_secret: self.stripe_webhook_secret.clone(),
            stripe_secret_key: self.stripe_secret_key.clone(),
            stripe_price_team_monthly: self.stripe_price_team_monthly.clone(),
            stripe_price_starter_monthly: self.stripe_price_starter_monthly.clone(),
        }
    }
}
