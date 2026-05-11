mod document;
mod event;
mod object;
mod vector;

use std::sync::Arc;

use anyhow::Result;

use crate::{StorageKit, embedding::vertex::VertexEmbedder};

#[derive(Clone)]
pub struct RemoteConfig {
    pub project_id: String,
    pub region: String,
    /// GCS bucket for this app's artifacts (e.g. "reflective-prod-wolfgang")
    pub bucket: String,
    /// Bearer token source: "metadata" (Cloud Run) or an explicit token string (dev/test)
    pub token_source: TokenSource,
}

#[derive(Clone)]
pub enum TokenSource {
    /// Fetch from GCE metadata server — the normal Cloud Run case.
    Metadata,
    /// Hard-coded token (for tests / local dev with a service account key).
    Static(String),
}

impl RemoteConfig {
    pub fn from_env() -> anyhow::Result<Self> {
        Ok(Self {
            project_id: std::env::var("GOOGLE_CLOUD_PROJECT")?,
            region: std::env::var("GOOGLE_CLOUD_REGION").unwrap_or_else(|_| "europe-west1".into()),
            bucket: std::env::var("GCS_BUCKET")?,
            token_source: TokenSource::Metadata,
        })
    }
}

pub struct RemoteStorageKit;

impl RemoteStorageKit {
    pub async fn build(config: RemoteConfig) -> Result<StorageKit> {
        let token = GcpToken::new(config.token_source.clone());

        Ok(StorageKit {
            documents: Arc::new(document::FirestoreDocumentStore::new(
                config.project_id.clone(),
                token.clone(),
            )),
            vectors: Arc::new(vector::VertexVectorStore::new(
                config.project_id.clone(),
                config.region.clone(),
                token.clone(),
            )),
            objects: Arc::new(object::GcsObjectStore::new(
                config.bucket.clone(),
                token.clone(),
            )),
            events: Arc::new(event::FirestoreEventLog::new(
                config.project_id.clone(),
                token.clone(),
            )),
            embeddings: Arc::new(VertexEmbedder::new(
                config.project_id.clone(),
                config.region.clone(),
                token,
            )),
        })
    }
}

/// Shared GCP bearer token accessor. Fetches from metadata server or returns static token.
#[derive(Clone)]
pub struct GcpToken {
    source: TokenSource,
}

impl GcpToken {
    pub fn new(source: TokenSource) -> Self {
        Self { source }
    }

    pub async fn get(&self) -> anyhow::Result<String> {
        match &self.source {
            TokenSource::Static(t) => Ok(t.clone()),
            TokenSource::Metadata => {
                let resp = reqwest::Client::new()
                    .get("http://metadata.google.internal/computeMetadata/v1/instance/service-accounts/default/token")
                    .header("Metadata-Flavor", "Google")
                    .send()
                    .await?
                    .json::<serde_json::Value>()
                    .await?;
                Ok(resp["access_token"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string())
            }
        }
    }
}
