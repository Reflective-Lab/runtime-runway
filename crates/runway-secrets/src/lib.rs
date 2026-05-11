use std::collections::HashMap;

use secrecy::{ExposeSecret, SecretString};
use tracing::instrument;

pub use secrecy::SecretString as Secret;

#[derive(Debug, thiserror::Error)]
pub enum SecretsError {
    #[error("secret not found: {0}")]
    NotFound(String),
    #[error("network error fetching secret {0}: {1}")]
    Network(String, String),
    #[error("auth error: {0}")]
    Auth(String),
}

/// GCP Secret Manager client.
///
/// Secrets are named: `{env}-{app}-{name}` or `{env}-platform-{name}`.
/// All fetched values are held as `SecretString` (zeroized on drop).
pub struct Secrets {
    project_id: String,
    env: String,
    app: String,
    client: reqwest::Client,
}

impl Secrets {
    pub fn new(
        project_id: impl Into<String>,
        env: impl Into<String>,
        app: impl Into<String>,
    ) -> Self {
        Self {
            project_id: project_id.into(),
            env: env.into(),
            app: app.into(),
            client: reqwest::Client::new(),
        }
    }

    /// Build from environment variables: GCP_PROJECT, ENV, APP_NAME.
    pub fn from_env() -> anyhow::Result<Self> {
        Ok(Self::new(
            std::env::var("GCP_PROJECT").or_else(|_| std::env::var("GOOGLE_CLOUD_PROJECT"))?,
            std::env::var("ENV").unwrap_or_else(|_| "dev".into()),
            std::env::var("APP_NAME")?,
        ))
    }

    fn secret_name(&self, key: &str) -> String {
        format!("{}-{}-{}", self.env, self.app, key)
    }

    fn platform_name(&self, key: &str) -> String {
        format!("{}-platform-{}", self.env, key)
    }

    async fn gcp_token(&self) -> Result<String, SecretsError> {
        let resp = self.client
            .get("http://metadata.google.internal/computeMetadata/v1/instance/service-accounts/default/token")
            .header("Metadata-Flavor", "Google")
            .send()
            .await
            .map_err(|e| SecretsError::Auth(e.to_string()))?
            .json::<serde_json::Value>()
            .await
            .map_err(|e| SecretsError::Auth(e.to_string()))?;
        Ok(resp["access_token"]
            .as_str()
            .unwrap_or_default()
            .to_string())
    }

    /// Fetch a single app-scoped secret: `{env}-{app}-{key}`.
    #[instrument(skip(self), fields(key))]
    pub async fn get(&self, key: &str) -> Result<SecretString, SecretsError> {
        self.fetch_raw(&self.secret_name(key)).await
    }

    /// Fetch a platform-scoped secret: `{env}-platform-{key}`.
    pub async fn get_platform(&self, key: &str) -> Result<SecretString, SecretsError> {
        self.fetch_raw(&self.platform_name(key)).await
    }

    /// Fetch multiple secrets at startup and return a typed map.
    /// Fails fast if any secret is missing.
    pub async fn load_all(
        &self,
        app_keys: &[&str],
        platform_keys: &[&str],
    ) -> Result<SecretMap, SecretsError> {
        let mut map = HashMap::new();
        for &key in app_keys {
            map.insert(key.to_string(), self.get(key).await?);
        }
        for &key in platform_keys {
            map.insert(format!("platform/{key}"), self.get_platform(key).await?);
        }
        Ok(SecretMap(map))
    }

    async fn fetch_raw(&self, full_name: &str) -> Result<SecretString, SecretsError> {
        let url = format!(
            "https://secretmanager.googleapis.com/v1/projects/{}/secrets/{}/versions/latest:access",
            self.project_id, full_name
        );
        let token = self.gcp_token().await?;
        let resp: serde_json::Value = self
            .client
            .get(&url)
            .bearer_auth(&token)
            .send()
            .await
            .map_err(|e| SecretsError::Network(full_name.to_string(), e.to_string()))?
            .error_for_status()
            .map_err(|e| {
                if e.status() == Some(reqwest::StatusCode::NOT_FOUND) {
                    SecretsError::NotFound(full_name.to_string())
                } else {
                    SecretsError::Network(full_name.to_string(), e.to_string())
                }
            })?
            .json()
            .await
            .map_err(|e| SecretsError::Network(full_name.to_string(), e.to_string()))?;

        let encoded = resp["payload"]["data"]
            .as_str()
            .ok_or_else(|| SecretsError::NotFound(full_name.to_string()))?;
        let decoded =
            base64_decode(encoded).map_err(|e| SecretsError::Network(full_name.to_string(), e))?;

        Ok(SecretString::new(decoded))
    }
}

/// Map of loaded secrets. Access values via `.get(key)`.
pub struct SecretMap(HashMap<String, SecretString>);

impl SecretMap {
    pub fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).map(|s| s.expose_secret().as_str())
    }

    pub fn require(&self, key: &str) -> anyhow::Result<&str> {
        self.get(key)
            .ok_or_else(|| anyhow::anyhow!("secret not loaded: {key}"))
    }
}

fn base64_decode(s: &str) -> Result<String, String> {
    let bytes = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, s)
        .map_err(|e| e.to_string())?;
    String::from_utf8(bytes).map_err(|e| e.to_string())
}
