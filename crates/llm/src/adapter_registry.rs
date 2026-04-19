// Copyright 2024-2026 Reflective Labs

//! Filesystem-based adapter registry.
//!
//! This module provides a concrete implementation of [`AdapterRegistry`] that
//! stores adapters on the local filesystem.
//!
//! # Directory Structure
//!
//! ```text
//! adapters/
//! ├── llm/
//! │   └── grounded-answering/
//! │       └── 1.0.0+sha256:abc123/
//! │           ├── manifest.toml
//! │           └── weights.bin
//! └── domain/
//!     └── invoice-classifier/
//!         └── 1.0.0+sha256:def456/
//!             ├── manifest.toml
//!             └── weights.bin
//! ```
//!
//! # converge-core Axiom Compliance
//!
//! - **No Hidden Work**: All I/O is synchronous and explicit
//! - **Transparent Determinism**: Adapters are content-addressed by hash
//! - **Safety by Construction**: Invalid paths cannot be constructed

use crate::adapter::{AdapterId, AdapterManifest, AdapterRegistry, AdapterWeights};
use crate::error::{LlmError, LlmResult};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

/// Filesystem-based adapter registry.
///
/// Stores and retrieves adapters from a local directory.
#[derive(Debug, Clone)]
pub struct FilesystemRegistry {
    /// Root directory for adapter storage
    root: PathBuf,
}

impl FilesystemRegistry {
    /// Create a new filesystem registry.
    ///
    /// # Arguments
    ///
    /// * `root` - Root directory for adapter storage
    ///
    /// # Errors
    ///
    /// Returns an error if the directory cannot be created.
    pub fn new(root: impl Into<PathBuf>) -> LlmResult<Self> {
        let root = root.into();

        // Create root directory if it doesn't exist
        if !root.exists() {
            fs::create_dir_all(&root)?;
        }

        Ok(Self { root })
    }

    /// Get the path to an adapter's directory.
    fn adapter_path(&self, id: &AdapterId) -> PathBuf {
        self.root
            .join(&id.namespace)
            .join(&id.name)
            .join(format!("{}+sha256:{}", id.version, id.content_hash))
    }

    /// Get the path to an adapter's manifest file.
    fn manifest_path(&self, id: &AdapterId) -> PathBuf {
        self.adapter_path(id).join("manifest.toml")
    }

    /// Get the path to an adapter's weights file.
    fn weights_path(&self, id: &AdapterId) -> PathBuf {
        self.adapter_path(id).join("weights.bin")
    }

    /// Save an adapter to the registry.
    ///
    /// # Arguments
    ///
    /// * `manifest` - Adapter manifest
    /// * `weights` - Adapter weights
    ///
    /// # Errors
    ///
    /// Returns an error if the adapter cannot be saved.
    pub fn save(&self, manifest: &AdapterManifest, weights: &AdapterWeights) -> LlmResult<()> {
        let id = &manifest.adapter_id;
        let adapter_dir = self.adapter_path(id);

        // Create adapter directory
        fs::create_dir_all(&adapter_dir)?;

        // Write manifest
        let manifest_toml = toml::to_string_pretty(manifest)
            .map_err(|e| LlmError::AdapterError(format!("Failed to serialize manifest: {}", e)))?;
        fs::write(self.manifest_path(id), manifest_toml)?;

        // Write weights (simple binary format for now)
        let weights_data = serialize_weights(weights)?;
        fs::write(self.weights_path(id), weights_data)?;

        tracing::info!(
            adapter_id = %id,
            path = %adapter_dir.display(),
            "Saved adapter to filesystem"
        );

        Ok(())
    }

    /// Delete an adapter from the registry.
    ///
    /// # Errors
    ///
    /// Returns an error if the adapter cannot be deleted.
    pub fn delete(&self, id: &AdapterId) -> LlmResult<()> {
        let adapter_dir = self.adapter_path(id);

        if adapter_dir.exists() {
            fs::remove_dir_all(&adapter_dir)?;
            tracing::info!(adapter_id = %id, "Deleted adapter from filesystem");
        }

        Ok(())
    }
}

impl AdapterRegistry for FilesystemRegistry {
    fn get_manifest(&self, id: &AdapterId) -> LlmResult<AdapterManifest> {
        let manifest_path = self.manifest_path(id);

        if !manifest_path.exists() {
            return Err(LlmError::AdapterNotFound(id.to_canonical()));
        }

        let manifest_str = fs::read_to_string(&manifest_path)?;
        let manifest: AdapterManifest = toml::from_str(&manifest_str)
            .map_err(|e| LlmError::AdapterError(format!("Failed to parse manifest: {}", e)))?;

        Ok(manifest)
    }

    fn load_weights(&self, id: &AdapterId) -> LlmResult<AdapterWeights> {
        let weights_path = self.weights_path(id);

        if !weights_path.exists() {
            return Err(LlmError::AdapterNotFound(format!(
                "{} (weights file missing)",
                id.to_canonical()
            )));
        }

        let weights_data = fs::read(&weights_path)?;
        let weights = deserialize_weights(&weights_data)?;

        tracing::debug!(adapter_id = %id, "Loaded adapter weights");

        Ok(weights)
    }

    fn exists(&self, id: &AdapterId) -> bool {
        self.manifest_path(id).exists()
    }

    fn list(&self) -> LlmResult<Vec<AdapterId>> {
        let mut adapters = Vec::new();

        // Iterate through namespaces
        for ns_entry in fs::read_dir(&self.root)? {
            let ns_entry = ns_entry?;
            if !ns_entry.file_type()?.is_dir() {
                continue;
            }
            let namespace = ns_entry.file_name().to_string_lossy().to_string();

            // Iterate through adapter names
            for name_entry in fs::read_dir(ns_entry.path())? {
                let name_entry = name_entry?;
                if !name_entry.file_type()?.is_dir() {
                    continue;
                }
                let name = name_entry.file_name().to_string_lossy().to_string();

                // Iterate through versions
                for version_entry in fs::read_dir(name_entry.path())? {
                    let version_entry = version_entry?;
                    if !version_entry.file_type()?.is_dir() {
                        continue;
                    }

                    let version_hash = version_entry.file_name().to_string_lossy().to_string();

                    // Parse version+hash
                    if let Some((version, hash)) = version_hash.split_once("+sha256:") {
                        adapters.push(AdapterId {
                            namespace: namespace.clone(),
                            name: name.clone(),
                            version: version.to_string(),
                            content_hash: hash.to_string(),
                        });
                    }
                }
            }
        }

        Ok(adapters)
    }
}

/// In-memory adapter registry for testing.
///
/// Useful for unit tests where filesystem access is not desired.
#[derive(Debug, Default)]
pub struct InMemoryRegistry {
    manifests: HashMap<String, AdapterManifest>,
    weights: HashMap<String, AdapterWeights>,
}

impl InMemoryRegistry {
    /// Create a new empty in-memory registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add an adapter to the registry.
    pub fn add(&mut self, manifest: AdapterManifest, weights: AdapterWeights) {
        let key = manifest.adapter_id.to_canonical();
        self.manifests.insert(key.clone(), manifest);
        self.weights.insert(key, weights);
    }
}

impl AdapterRegistry for InMemoryRegistry {
    fn get_manifest(&self, id: &AdapterId) -> LlmResult<AdapterManifest> {
        self.manifests
            .get(&id.to_canonical())
            .cloned()
            .ok_or_else(|| LlmError::AdapterNotFound(id.to_canonical()))
    }

    fn load_weights(&self, id: &AdapterId) -> LlmResult<AdapterWeights> {
        self.weights
            .get(&id.to_canonical())
            .cloned()
            .ok_or_else(|| LlmError::AdapterNotFound(id.to_canonical()))
    }

    fn exists(&self, id: &AdapterId) -> bool {
        self.manifests.contains_key(&id.to_canonical())
    }

    fn list(&self) -> LlmResult<Vec<AdapterId>> {
        self.manifests
            .values()
            .map(|m| Ok(m.adapter_id.clone()))
            .collect()
    }
}

/// Serialize adapter weights to bytes.
///
/// Uses a simple format: JSON header + raw floats.
fn serialize_weights(weights: &AdapterWeights) -> LlmResult<Vec<u8>> {
    let mut data = Vec::new();

    // Header: JSON with metadata
    let header = serde_json::json!({
        "version": 1,
        "rank": weights.rank,
        "alpha": weights.alpha,
        "layers": weights.layers.keys().collect::<Vec<_>>(),
    });
    let header_bytes = serde_json::to_vec(&header)?;
    let header_len = header_bytes.len() as u32;

    // Write header length (4 bytes) + header
    data.extend_from_slice(&header_len.to_le_bytes());
    data.extend_from_slice(&header_bytes);

    // Write each layer's weights
    for (name, (a, b)) in &weights.layers {
        // Layer name length + name
        let name_bytes = name.as_bytes();
        let name_len = name_bytes.len() as u32;
        data.extend_from_slice(&name_len.to_le_bytes());
        data.extend_from_slice(name_bytes);

        // A matrix: length + floats
        let a_len = a.len() as u32;
        data.extend_from_slice(&a_len.to_le_bytes());
        for val in a {
            data.extend_from_slice(&val.to_le_bytes());
        }

        // B matrix: length + floats
        let b_len = b.len() as u32;
        data.extend_from_slice(&b_len.to_le_bytes());
        for val in b {
            data.extend_from_slice(&val.to_le_bytes());
        }
    }

    Ok(data)
}

/// Deserialize adapter weights from bytes.
pub(crate) fn deserialize_weights(data: &[u8]) -> LlmResult<AdapterWeights> {
    if data.len() < 4 {
        return Err(LlmError::AdapterLoadError(
            "Invalid weights file".to_string(),
        ));
    }

    let mut cursor = 0;

    // Read header length
    let header_len = u32::from_le_bytes(data[cursor..cursor + 4].try_into().unwrap()) as usize;
    cursor += 4;

    // Read header
    let header: serde_json::Value = serde_json::from_slice(&data[cursor..cursor + header_len])?;
    cursor += header_len;

    let rank = header["rank"].as_u64().unwrap_or(8) as usize;
    let alpha = header["alpha"].as_f64().unwrap_or(16.0) as f32;

    let mut weights = AdapterWeights::empty(rank, alpha);

    // Read layers
    while cursor < data.len() {
        // Layer name
        let name_len = u32::from_le_bytes(data[cursor..cursor + 4].try_into().unwrap()) as usize;
        cursor += 4;
        let name = String::from_utf8_lossy(&data[cursor..cursor + name_len]).to_string();
        cursor += name_len;

        // A matrix
        let a_len = u32::from_le_bytes(data[cursor..cursor + 4].try_into().unwrap()) as usize;
        cursor += 4;
        let mut a = Vec::with_capacity(a_len);
        for _ in 0..a_len {
            let val = f32::from_le_bytes(data[cursor..cursor + 4].try_into().unwrap());
            cursor += 4;
            a.push(val);
        }

        // B matrix
        let b_len = u32::from_le_bytes(data[cursor..cursor + 4].try_into().unwrap()) as usize;
        cursor += 4;
        let mut b = Vec::with_capacity(b_len);
        for _ in 0..b_len {
            let val = f32::from_le_bytes(data[cursor..cursor + 4].try_into().unwrap());
            cursor += 4;
            b.push(val);
        }

        weights.add_layer(name, a, b);
    }

    Ok(weights)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn test_manifest() -> AdapterManifest {
        AdapterManifest {
            adapter_id: AdapterId::new("llm", "test-adapter", "1.0.0", "abc123"),
            base_model_id: "llama3-8b".to_string(),
            model_family: "llama3".to_string(),
            tokenizer_hash: "tok123".to_string(),
            context_size: 4096,
            quantization_mode: None,
            rank: 8,
            alpha: 16.0,
            target_layers: vec!["q_proj".to_string(), "v_proj".to_string()],
            dropout: 0.0,
            truth_targets: vec!["grounded-answering".to_string()],
            dataset_manifest_id: None,
            created_at: "2026-01-17T00:00:00Z".to_string(),
            training_config_hash: None,
            author: Some("test".to_string()),
            description: Some("Test adapter".to_string()),
        }
    }

    fn test_weights() -> AdapterWeights {
        let mut weights = AdapterWeights::empty(8, 16.0);
        weights.add_layer("q_proj", vec![0.1; 64 * 8], vec![0.2; 8 * 64]);
        weights.add_layer("v_proj", vec![0.3; 64 * 8], vec![0.4; 8 * 64]);
        weights
    }

    #[test]
    fn test_weights_serialization_roundtrip() {
        let original = test_weights();
        let serialized = serialize_weights(&original).unwrap();
        let deserialized = deserialize_weights(&serialized).unwrap();

        assert_eq!(deserialized.rank, original.rank);
        assert_eq!(deserialized.alpha, original.alpha);
        assert_eq!(deserialized.layers.len(), original.layers.len());
    }

    #[test]
    fn test_filesystem_registry_save_load() {
        let dir = tempdir().unwrap();
        let registry = FilesystemRegistry::new(dir.path()).unwrap();

        let manifest = test_manifest();
        let weights = test_weights();
        let id = manifest.adapter_id.clone();

        // Save
        registry.save(&manifest, &weights).unwrap();

        // Check exists
        assert!(registry.exists(&id));

        // Load manifest
        let loaded_manifest = registry.get_manifest(&id).unwrap();
        assert_eq!(loaded_manifest.adapter_id, id);
        assert_eq!(loaded_manifest.model_family, "llama3");

        // Load weights
        let loaded_weights = registry.load_weights(&id).unwrap();
        assert_eq!(loaded_weights.rank, weights.rank);
    }

    #[test]
    fn test_filesystem_registry_list() {
        let dir = tempdir().unwrap();
        let registry = FilesystemRegistry::new(dir.path()).unwrap();

        // Save two adapters
        let manifest1 = test_manifest();
        let mut manifest2 = test_manifest();
        manifest2.adapter_id = AdapterId::new("llm", "other-adapter", "2.0.0", "def456");

        registry.save(&manifest1, &test_weights()).unwrap();
        registry.save(&manifest2, &test_weights()).unwrap();

        // List
        let adapters = registry.list().unwrap();
        assert_eq!(adapters.len(), 2);
    }

    #[test]
    fn test_filesystem_registry_not_found() {
        let dir = tempdir().unwrap();
        let registry = FilesystemRegistry::new(dir.path()).unwrap();

        let id = AdapterId::new("llm", "nonexistent", "1.0.0", "xxx");
        assert!(!registry.exists(&id));
        assert!(registry.get_manifest(&id).is_err());
    }

    #[test]
    fn test_in_memory_registry() {
        let mut registry = InMemoryRegistry::new();

        let manifest = test_manifest();
        let weights = test_weights();
        let id = manifest.adapter_id.clone();

        registry.add(manifest, weights);

        assert!(registry.exists(&id));
        assert!(registry.get_manifest(&id).is_ok());
        assert!(registry.load_weights(&id).is_ok());
    }
}
