pub mod embedding;
pub mod endpoints;
pub mod local;
pub mod remote;
pub mod traits;

pub use traits::{
    document::{Document, DocumentStore, Filter, Order, Query},
    embedding::EmbeddingProvider,
    event::{EventLog, StoredEvent},
    object::ObjectStore,
    vector::{Match, VectorStore},
};

use std::{path::Path, sync::Arc};

use anyhow::Result;

/// Composed storage kit — one instance per app, injected at startup.
///
/// Tauri apps call `StorageKit::local()`; Cloud Run apps call `StorageKit::remote()`.
/// All Converge loops, Suggestors, and domain logic receive this and never care which backend is live.
#[derive(Clone)]
pub struct StorageKit {
    pub documents: Arc<dyn DocumentStore>,
    pub vectors: Arc<dyn VectorStore>,
    pub objects: Arc<dyn ObjectStore>,
    pub events: Arc<dyn EventLog>,
    pub embeddings: Arc<dyn EmbeddingProvider>,
}

impl StorageKit {
    /// Local storage for Tauri desktop apps. Uses SQLite + LanceDB + local FS.
    /// `base` is the root directory (e.g. `~/.inkling` or `~/.wolfgang`).
    pub async fn local(base: impl AsRef<Path>) -> Result<Self> {
        local::LocalStorageKit::build(base.as_ref()).await
    }

    /// Remote storage for Cloud Run backends. Uses Firestore + GCS + Vertex AI.
    pub async fn remote(config: remote::RemoteConfig) -> Result<Self> {
        remote::RemoteStorageKit::build(config).await
    }
}
