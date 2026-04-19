// Copyright 2024-2026 Reflective Labs
// SPDX-License-Identifier: MIT

//! Object-store-backed adapter registry.
//!
//! Wraps a local [`FilesystemRegistry`] with an async sync step that
//! pulls adapter artifacts from any `converge-storage` backend. The
//! [`AdapterRegistry`] trait is sync, so all reads hit the local cache
//! after the initial sync.
//!
//! ## Usage
//!
//! ```ignore
//! let store = converge_storage::build_store(&config)?;
//! let registry = ObjectStoreRegistry::new(cache_dir, store);
//! registry.sync().await?;  // pull from remote → local cache
//! // Now use as a normal AdapterRegistry (sync)
//! let manifest = registry.get_manifest(&id)?;
//! ```

use std::path::PathBuf;
use std::sync::Arc;

use converge_storage::object_store::ObjectStoreExt;
use converge_storage::{ObjectPath, ObjectStore};

use crate::adapter::{AdapterId, AdapterManifest, AdapterRegistry, AdapterWeights};
use crate::adapter_registry::FilesystemRegistry;
use crate::error::{LlmError, LlmResult};

/// Adapter registry backed by a remote object store with local cache.
pub struct ObjectStoreRegistry {
    local: FilesystemRegistry,
    store: Arc<dyn ObjectStore>,
    prefix: String,
}

impl ObjectStoreRegistry {
    /// Create a new storage-backed registry.
    ///
    /// `cache_dir` is used as the local filesystem cache for downloaded artifacts.
    /// `prefix` is the object key prefix under which adapters are stored (e.g., `"adapters/"`).
    ///
    /// # Errors
    ///
    /// Returns an error if the local cache directory cannot be created.
    pub fn new(
        cache_dir: PathBuf,
        store: Arc<dyn ObjectStore>,
        prefix: impl Into<String>,
    ) -> LlmResult<Self> {
        let local = FilesystemRegistry::new(&cache_dir)?;
        Ok(Self {
            local,
            store,
            prefix: prefix.into(),
        })
    }

    /// Sync a specific adapter from the remote store to the local cache.
    ///
    /// Downloads the manifest and weights if not already cached locally.
    ///
    /// # Errors
    ///
    /// Returns an error if the remote adapter cannot be fetched.
    pub async fn sync_adapter(&self, id: &AdapterId) -> LlmResult<()> {
        if self.local.exists(id) {
            tracing::debug!(adapter_id = %id, "adapter already cached locally");
            return Ok(());
        }

        let remote_dir = format!(
            "{}{}/{}/{}+sha256:{}",
            self.prefix, id.namespace, id.name, id.version, id.content_hash
        );

        // Download manifest
        let manifest_key = ObjectPath::from(format!("{remote_dir}/manifest.toml"));
        let manifest_bytes = self
            .store
            .get(&manifest_key)
            .await
            .map_err(|e| LlmError::AdapterError(format!("failed to fetch manifest: {e}")))?
            .bytes()
            .await
            .map_err(|e| LlmError::AdapterError(format!("failed to read manifest bytes: {e}")))?;

        let manifest: AdapterManifest = toml::from_str(
            std::str::from_utf8(&manifest_bytes)
                .map_err(|e| LlmError::AdapterError(format!("manifest is not utf-8: {e}")))?,
        )
        .map_err(|e| LlmError::AdapterError(format!("failed to parse manifest: {e}")))?;

        // Download weights
        let weights_key = ObjectPath::from(format!("{remote_dir}/weights.bin"));
        let weights_bytes = self
            .store
            .get(&weights_key)
            .await
            .map_err(|e| LlmError::AdapterError(format!("failed to fetch weights: {e}")))?
            .bytes()
            .await
            .map_err(|e| LlmError::AdapterError(format!("failed to read weights bytes: {e}")))?;

        let weights = crate::adapter_registry::deserialize_weights(&weights_bytes)?;

        // Save to local cache
        self.local.save(&manifest, &weights)?;
        tracing::info!(adapter_id = %id, "synced adapter from remote storage");

        Ok(())
    }
}

impl AdapterRegistry for ObjectStoreRegistry {
    fn get_manifest(&self, id: &AdapterId) -> LlmResult<AdapterManifest> {
        self.local.get_manifest(id)
    }

    fn load_weights(&self, id: &AdapterId) -> LlmResult<AdapterWeights> {
        self.local.load_weights(id)
    }

    fn exists(&self, id: &AdapterId) -> bool {
        self.local.exists(id)
    }

    fn list(&self) -> LlmResult<Vec<AdapterId>> {
        self.local.list()
    }
}
