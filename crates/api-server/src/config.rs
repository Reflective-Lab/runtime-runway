//! Top-level runtime configuration for the api-server binary.
//!
//! Loaded once from environment variables in `main`, then passed by
//! field-value into the services that need them. Library crates never
//! read env directly.

use anyhow::Result;

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
}

impl RunwayConfig {
    pub fn from_env() -> Result<Self> {
        let local_dev = std::env::var("LOCAL_DEV").as_deref() == Ok("true");

        let allowed_origins = std::env::var("ALLOWED_ORIGINS").unwrap_or_default();

        if !local_dev {
            let stripe_secret = std::env::var("STRIPE_WEBHOOK_SECRET").unwrap_or_default();
            anyhow::ensure!(
                !stripe_secret.is_empty(),
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

        Ok(Self {
            local_dev,
            storage_path,
            firebase_project_id,
            route_prefix,
            app_url,
            allowed_origins,
        })
    }
}
