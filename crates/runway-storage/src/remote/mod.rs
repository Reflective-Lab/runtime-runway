mod document;
mod event;
mod object;
mod vector;

use std::sync::Arc;

use anyhow::Result;

use crate::{EmbeddingProvider, StorageKit, embedding::vertex::VertexEmbedder};

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
    /// Default constructor — uses the Vertex AI embedder.
    pub async fn build(config: RemoteConfig) -> Result<StorageKit> {
        Self::build_with_embedder(config, None).await
    }

    /// Constructor with an optional embedding-provider override. Used by the
    /// contract emulator entry point to inject fastembed since there is no
    /// Vertex AI emulator.
    pub async fn build_with_embedder(
        config: RemoteConfig,
        embedder_override: Option<Arc<dyn EmbeddingProvider>>,
    ) -> Result<StorageKit> {
        let token = GcpToken::new(config.token_source.clone());

        let embeddings: Arc<dyn EmbeddingProvider> = match embedder_override {
            Some(e) => e,
            None => Arc::new(VertexEmbedder::new(
                config.project_id.clone(),
                config.region.clone(),
                token.clone(),
            )),
        };

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
            embeddings,
            syncable_events: None,
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
                let client = reqwest::Client::new();
                runway_secrets::metadata::fetch_access_token(&client).await
            }
        }
    }
}

/// Apply a bearer token only if it is non-empty.
///
/// The GCP emulators reject `Authorization: Bearer ` (empty token) with
/// 500 UNKNOWN, but accept the request when the header is absent entirely.
/// Tests against the emulator stack use `TokenSource::Static(String::new())`,
/// so call sites must drop the header in that case.
pub trait BearerAuthExt {
    fn bearer_auth_if_set(self, token: &str) -> Self;
}

impl BearerAuthExt for reqwest::RequestBuilder {
    fn bearer_auth_if_set(self, token: &str) -> Self {
        if token.is_empty() {
            self
        } else {
            self.bearer_auth(token)
        }
    }
}
