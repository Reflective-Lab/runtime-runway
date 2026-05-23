use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header};
use serde::Deserialize;
use tokio::sync::RwLock;

const JWKS_URL: &str =
    "https://www.googleapis.com/service_accounts/v1/jwk/securetoken@system.gserviceaccount.com";
const JWKS_TTL: Duration = Duration::from_secs(3600);

/// Custom claims expected in Firebase ID tokens issued by the backend after org creation.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct FirebaseClaims {
    pub uid: String,
    pub email: Option<String>,
    pub org_id: Option<String>,
    #[serde(default)]
    pub apps: Vec<String>,
    pub role: Option<String>,
}

impl FirebaseClaims {
    pub fn has_app(&self, app: &str) -> bool {
        self.apps.iter().any(|a| a == app)
    }
}

#[derive(Debug, Deserialize, Clone)]
struct Jwk {
    kid: String,
    n: String,
    e: String,
}

#[derive(Deserialize)]
struct JwksResponse {
    keys: Vec<Jwk>,
}

struct JwksCache {
    keys: Vec<Jwk>,
    fetched_at: Instant,
}

/// Full JWT payload — `sub` maps to uid, custom claims sit at the top level.
#[derive(Deserialize)]
struct JwtPayload {
    sub: String,
    email: Option<String>,
    #[serde(default)]
    org_id: Option<String>,
    #[serde(default)]
    apps: Vec<String>,
    #[serde(default)]
    role: Option<String>,
}

pub struct FirebaseAuth {
    project_id: String,
    client: reqwest::Client,
    cache: Arc<RwLock<Option<JwksCache>>>,
}

impl FirebaseAuth {
    pub fn new(project_id: impl Into<String>) -> Self {
        Self {
            project_id: project_id.into(),
            client: reqwest::Client::new(),
            cache: Arc::new(RwLock::new(None)),
        }
    }

    /// Verify a Firebase ID token offline via JWKS + RS256 signature check.
    /// JWKS is cached for 1 hour; re-fetched when a new `kid` is seen.
    pub async fn verify(&self, id_token: &str) -> anyhow::Result<FirebaseClaims> {
        let header = decode_header(id_token)?;
        let kid = header
            .kid
            .ok_or_else(|| anyhow::anyhow!("JWT missing kid"))?;

        let key = self.decoding_key(&kid).await?;

        let mut validation = Validation::new(Algorithm::RS256);
        validation.set_issuer(&[format!(
            "https://securetoken.google.com/{}",
            self.project_id
        )]);
        validation.set_audience(&[self.project_id.as_str()]);

        let payload = decode::<JwtPayload>(id_token, &key, &validation)?.claims;

        Ok(FirebaseClaims {
            uid: payload.sub,
            email: payload.email,
            org_id: payload.org_id,
            apps: payload.apps,
            role: payload.role,
        })
    }

    async fn decoding_key(&self, kid: &str) -> anyhow::Result<DecodingKey> {
        // Fast path: cache hit under read lock.
        {
            let guard = self.cache.read().await;
            if let Some(c) = guard.as_ref()
                && c.fetched_at.elapsed() < JWKS_TTL
                && let Some(jwk) = c.keys.iter().find(|k| k.kid == kid)
            {
                return Ok(DecodingKey::from_rsa_components(&jwk.n, &jwk.e)?);
            }
        }

        // Cache miss or expired — fetch under write lock.
        let mut guard = self.cache.write().await;
        let keys = self.fetch_jwks().await?;
        let jwk = keys
            .iter()
            .find(|k| k.kid == kid)
            .ok_or_else(|| anyhow::anyhow!("no JWK for kid={kid}"))?;
        let decoding_key = DecodingKey::from_rsa_components(&jwk.n, &jwk.e)?;
        *guard = Some(JwksCache {
            keys,
            fetched_at: Instant::now(),
        });
        Ok(decoding_key)
    }

    async fn fetch_jwks(&self) -> anyhow::Result<Vec<Jwk>> {
        let resp: JwksResponse = self
            .client
            .get(JWKS_URL)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        tracing::debug!(count = resp.keys.len(), "JWKS refreshed");
        Ok(resp.keys)
    }
}
