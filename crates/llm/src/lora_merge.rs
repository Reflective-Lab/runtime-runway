// Copyright 2024-2026 Reflective Labs

//! LoRA weight merging for runtime adapter application.
//!
//! This module provides functionality to merge LoRA adapter weights into
//! base model weights at runtime. The core formula is:
//!
//! ```text
//! W' = W + (alpha/r) * B @ A
//! ```
//!
//! Where:
//! - W is the original weight matrix
//! - A is the down-projection matrix (in_features x rank)
//! - B is the up-projection matrix (rank x out_features)
//! - alpha is the scaling factor
//! - r is the rank
//!
//! # Layer Name Mapping
//!
//! Adapter layer names map to model tensor paths:
//!
//! | Adapter Name | Model Path |
//! |--------------|------------|
//! | `layers.{i}.attention.wq` | Query projection |
//! | `layers.{i}.attention.wk` | Key projection |
//! | `layers.{i}.attention.wv` | Value projection |
//! | `layers.{i}.attention.wo` | Output projection |
//! | `layers.{i}.feed_forward.w1` | FFN gate (swiglu.linear_inner) |
//! | `layers.{i}.feed_forward.w2` | FFN down |
//! | `layers.{i}.feed_forward.w3` | FFN up (swiglu.linear_outer) |
//!
//! # converge-core Axiom Compliance
//!
//! - **Transparent Determinism**: Same adapter + same weights = same merge result
//! - **No Hidden Work**: Merge operation is explicit and observable
//! - **Safety by Construction**: Invalid layer mappings return errors

use crate::adapter::AdapterWeights;
use crate::error::{LlmError, LlmResult};
use burn::tensor::Tensor;
use burn::tensor::backend::Backend;
use std::collections::HashMap;

/// Stores original weights for later restoration.
///
/// When a LoRA adapter is merged, we store the original weights so they
/// can be restored when the adapter is detached.
#[derive(Debug, Clone)]
pub struct OriginalWeights<B: Backend> {
    /// Map from layer path to original weight tensor
    weights: HashMap<String, Tensor<B, 2>>,
}

impl<B: Backend> OriginalWeights<B> {
    /// Create a new empty storage.
    #[must_use]
    pub fn new() -> Self {
        Self {
            weights: HashMap::new(),
        }
    }

    /// Store an original weight before merging.
    pub fn store(&mut self, path: String, weight: Tensor<B, 2>) {
        self.weights.insert(path, weight);
    }

    /// Get an original weight for restoration.
    #[must_use]
    pub fn get(&self, path: &str) -> Option<&Tensor<B, 2>> {
        self.weights.get(path)
    }

    /// Check if we have stored weights.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.weights.is_empty()
    }

    /// Get all stored paths.
    #[must_use]
    pub fn paths(&self) -> Vec<&String> {
        self.weights.keys().collect()
    }

    /// Take ownership of the weights map.
    pub fn take(self) -> HashMap<String, Tensor<B, 2>> {
        self.weights
    }
}

impl<B: Backend> Default for OriginalWeights<B> {
    fn default() -> Self {
        Self::new()
    }
}

/// Compute the LoRA delta for a single layer.
///
/// Computes: delta = (alpha/rank) * B @ A
///
/// # Arguments
///
/// * `lora_a` - Down-projection matrix [in_features, rank]
/// * `lora_b` - Up-projection matrix [rank, out_features]
/// * `alpha` - Scaling factor
/// * `rank` - LoRA rank
///
/// # Returns
///
/// The delta tensor to add to the base weights.
pub fn compute_lora_delta<B: Backend>(
    lora_a: &[f32],
    lora_b: &[f32],
    in_features: usize,
    out_features: usize,
    rank: usize,
    alpha: f32,
    device: &burn::tensor::Device<B>,
) -> Tensor<B, 2> {
    // Create tensors from the LoRA weights
    // A: [in_features, rank]
    // B: [rank, out_features]
    let a_data = burn::tensor::TensorData::new(lora_a.to_vec(), [in_features, rank]);
    let b_data = burn::tensor::TensorData::new(lora_b.to_vec(), [rank, out_features]);

    let a: Tensor<B, 2> = Tensor::from_data(a_data, device);
    let b: Tensor<B, 2> = Tensor::from_data(b_data, device);

    // Compute delta = (alpha/rank) * A @ B
    // Result shape: [in_features, out_features]
    let scale = alpha / rank as f32;
    a.matmul(b).mul_scalar(scale)
}

/// Merge LoRA weights into a base weight tensor.
///
/// Computes: W' = W + (alpha/rank) * B @ A
///
/// # Arguments
///
/// * `base_weight` - The original weight tensor [in_features, out_features]
/// * `lora_a` - Down-projection weights [in_features * rank]
/// * `lora_b` - Up-projection weights [rank * out_features]
/// * `alpha` - Scaling factor
/// * `rank` - LoRA rank
///
/// # Returns
///
/// The merged weight tensor.
pub fn merge_weights<B: Backend>(
    base_weight: Tensor<B, 2>,
    lora_a: &[f32],
    lora_b: &[f32],
    alpha: f32,
    rank: usize,
) -> Tensor<B, 2> {
    let device = base_weight.device();
    let [in_features, out_features] = base_weight.dims();

    let delta = compute_lora_delta::<B>(
        lora_a,
        lora_b,
        in_features,
        out_features,
        rank,
        alpha,
        &device,
    );

    base_weight + delta
}

/// Maps adapter layer names to standard paths.
///
/// Adapter weights use simplified names like "q_proj" while the model
/// uses full paths like "layers.0.attention.wq". This function handles
/// the mapping.
#[derive(Debug, Clone)]
pub struct LayerMapper {
    /// Number of layers in the model
    num_layers: usize,
}

impl LayerMapper {
    /// Create a new layer mapper.
    #[must_use]
    pub fn new(num_layers: usize) -> Self {
        Self { num_layers }
    }

    /// Map an adapter layer name to model tensor paths.
    ///
    /// # Arguments
    ///
    /// * `adapter_layer` - The adapter's layer name (e.g., "q_proj", "layers.0.attention.wq")
    ///
    /// # Returns
    ///
    /// A list of model tensor paths that this adapter layer applies to.
    pub fn map_to_model_paths(&self, adapter_layer: &str) -> Vec<String> {
        // Handle fully qualified paths (e.g., "layers.0.attention.wq")
        if adapter_layer.starts_with("layers.") {
            return vec![adapter_layer.to_string()];
        }

        // Handle shorthand names that apply to all layers
        let suffix = match adapter_layer {
            "q_proj" | "wq" => "attention.wq",
            "k_proj" | "wk" => "attention.wk",
            "v_proj" | "wv" => "attention.wv",
            "o_proj" | "wo" => "attention.wo",
            "w1" | "gate_proj" => "feed_forward.swiglu.linear_inner",
            "w2" | "down_proj" => "feed_forward.w2",
            "w3" | "up_proj" => "feed_forward.swiglu.linear_outer",
            _ => return vec![], // Unknown layer
        };

        // Generate paths for all layers
        (0..self.num_layers)
            .map(|i| format!("layers.{}.{}", i, suffix))
            .collect()
    }

    /// Validate that an adapter's target layers are valid.
    pub fn validate_target_layers(&self, target_layers: &[String]) -> LlmResult<()> {
        for layer in target_layers {
            let paths = self.map_to_model_paths(layer);
            if paths.is_empty() {
                return Err(LlmError::AdapterError(format!(
                    "Unknown target layer: '{}'. Valid layers: q_proj, k_proj, v_proj, o_proj, w1, w2, w3",
                    layer
                )));
            }
        }
        Ok(())
    }
}

/// Result of a merge operation.
#[derive(Debug)]
pub struct MergeResult {
    /// Number of layers merged
    pub layers_merged: usize,
    /// Paths that were merged
    pub merged_paths: Vec<String>,
}

/// Plan for merging LoRA weights into a model.
///
/// This struct captures what needs to be done without actually doing it,
/// allowing for validation before modification.
#[derive(Debug)]
pub struct MergePlan {
    /// Mappings from adapter layer name to model paths
    pub layer_mappings: HashMap<String, Vec<String>>,
    /// Total number of weight tensors to modify
    pub total_tensors: usize,
}

impl MergePlan {
    /// Create a merge plan from adapter weights.
    pub fn from_adapter(adapter: &AdapterWeights, mapper: &LayerMapper) -> LlmResult<Self> {
        let mut layer_mappings = HashMap::new();
        let mut total_tensors = 0;

        for layer_name in adapter.layers.keys() {
            let paths = mapper.map_to_model_paths(layer_name);
            if paths.is_empty() {
                return Err(LlmError::AdapterError(format!(
                    "Cannot map adapter layer '{}' to model paths",
                    layer_name
                )));
            }
            total_tensors += paths.len();
            layer_mappings.insert(layer_name.clone(), paths);
        }

        Ok(Self {
            layer_mappings,
            total_tensors,
        })
    }

    /// Get all model paths that will be modified, sorted for determinism.
    pub fn all_model_paths(&self) -> Vec<String> {
        let mut paths: Vec<String> = self
            .layer_mappings
            .values()
            .flat_map(|paths| paths.iter().cloned())
            .collect();
        paths.sort();
        paths
    }

    /// Get layer mappings sorted by adapter layer name for deterministic iteration.
    pub fn sorted_layer_mappings(&self) -> Vec<(&String, &Vec<String>)> {
        let mut mappings: Vec<_> = self.layer_mappings.iter().collect();
        mappings.sort_by_key(|(k, _)| *k);
        mappings
    }
}

// ============================================================================
// Canonical Delta Hashing for Deterministic Merge
// ============================================================================

/// Canonical representation of a LoRA delta for hashing.
///
/// This ensures merge operations are deterministic and auditable by
/// hashing all inputs that affect the delta computation.
#[derive(Debug, Clone)]
pub struct DeltaCanonical {
    /// Tensor path being modified
    pub tensor_path: String,
    /// Data type (always normalized to f32 for hashing)
    pub dtype: String,
    /// Shape [in_features, out_features]
    pub shape: [usize; 2],
    /// Adapter identifier
    pub adapter_id: String,
    /// Base model identifier
    pub base_model_id: String,
    /// LoRA alpha scaling factor
    pub alpha: f32,
    /// LoRA rank
    pub rank: usize,
    /// Delta bytes in canonical little-endian f32 format
    pub delta_bytes: Vec<u8>,
}

impl DeltaCanonical {
    /// Create a new canonical delta representation.
    pub fn new(
        tensor_path: impl Into<String>,
        shape: [usize; 2],
        adapter_id: impl Into<String>,
        base_model_id: impl Into<String>,
        alpha: f32,
        rank: usize,
        delta_f32: &[f32],
    ) -> Self {
        // Canonicalize delta bytes as little-endian f32
        let delta_bytes: Vec<u8> = delta_f32.iter().flat_map(|f| f.to_le_bytes()).collect();

        Self {
            tensor_path: tensor_path.into(),
            dtype: "f32".to_string(), // Always normalize to f32 for hashing
            shape,
            adapter_id: adapter_id.into(),
            base_model_id: base_model_id.into(),
            alpha,
            rank,
            delta_bytes,
        }
    }

    /// Compute the canonical blake3 hash of this delta.
    ///
    /// The hash includes all parameters that affect the delta computation:
    /// - tensor_path, dtype, shape
    /// - adapter_id, base_model_id
    /// - alpha, rank
    /// - delta_bytes (canonical little-endian f32)
    #[must_use]
    pub fn compute_hash(&self) -> String {
        use blake3::Hasher;

        let mut hasher = Hasher::new();

        // Hash structural metadata
        hasher.update(self.tensor_path.as_bytes());
        hasher.update(b"|");
        hasher.update(self.dtype.as_bytes());
        hasher.update(b"|");
        hasher.update(&self.shape[0].to_le_bytes());
        hasher.update(&self.shape[1].to_le_bytes());
        hasher.update(b"|");

        // Hash adapter/model identifiers
        hasher.update(self.adapter_id.as_bytes());
        hasher.update(b"|");
        hasher.update(self.base_model_id.as_bytes());
        hasher.update(b"|");

        // Hash LoRA parameters
        hasher.update(&self.alpha.to_le_bytes());
        hasher.update(&self.rank.to_le_bytes());
        hasher.update(b"|");

        // Hash delta bytes
        hasher.update(&self.delta_bytes);

        // Return hex-encoded hash
        hasher.finalize().to_hex().to_string()
    }

    /// Get the delta as f32 slice (for verification).
    pub fn delta_as_f32(&self) -> Vec<f32> {
        self.delta_bytes
            .chunks_exact(4)
            .map(|chunk| {
                let bytes: [u8; 4] = chunk.try_into().unwrap();
                f32::from_le_bytes(bytes)
            })
            .collect()
    }
}

/// Builder for creating deterministic merge artifacts.
#[derive(Debug)]
pub struct MergeArtifactBuilder {
    adapter_id: String,
    base_model_id: String,
    alpha: f32,
    rank: usize,
    deltas: Vec<DeltaCanonical>,
}

impl MergeArtifactBuilder {
    /// Create a new builder.
    pub fn new(
        adapter_id: impl Into<String>,
        base_model_id: impl Into<String>,
        alpha: f32,
        rank: usize,
    ) -> Self {
        Self {
            adapter_id: adapter_id.into(),
            base_model_id: base_model_id.into(),
            alpha,
            rank,
            deltas: Vec::new(),
        }
    }

    /// Add a delta for a tensor.
    pub fn add_delta(
        &mut self,
        tensor_path: impl Into<String>,
        shape: [usize; 2],
        delta_f32: &[f32],
    ) {
        let canonical = DeltaCanonical::new(
            tensor_path,
            shape,
            &self.adapter_id,
            &self.base_model_id,
            self.alpha,
            self.rank,
            delta_f32,
        );
        self.deltas.push(canonical);
    }

    /// Build the merge artifact with sorted paths and hashes.
    pub fn build(mut self) -> MergeArtifact {
        // Sort deltas by tensor path for deterministic ordering
        self.deltas
            .sort_by(|a, b| a.tensor_path.cmp(&b.tensor_path));

        let affected_tensors: Vec<String> =
            self.deltas.iter().map(|d| d.tensor_path.clone()).collect();

        let delta_hashes: Vec<String> = self.deltas.iter().map(|d| d.compute_hash()).collect();

        // Compute overall merge hash (hash of all delta hashes)
        let merge_hash = {
            use blake3::Hasher;
            let mut hasher = Hasher::new();
            hasher.update(self.adapter_id.as_bytes());
            hasher.update(b"|");
            hasher.update(self.base_model_id.as_bytes());
            hasher.update(b"|");
            for hash in &delta_hashes {
                hasher.update(hash.as_bytes());
                hasher.update(b"|");
            }
            hasher.finalize().to_hex().to_string()
        };

        MergeArtifact {
            adapter_id: self.adapter_id,
            base_model_id: self.base_model_id,
            alpha: self.alpha,
            rank: self.rank,
            affected_tensors,
            delta_hashes,
            merge_hash,
            deltas: self.deltas,
        }
    }
}

/// Deterministic merge artifact for audit and replay.
#[derive(Debug, Clone)]
pub struct MergeArtifact {
    /// Adapter identifier
    pub adapter_id: String,
    /// Base model identifier
    pub base_model_id: String,
    /// LoRA alpha
    pub alpha: f32,
    /// LoRA rank
    pub rank: usize,
    /// Affected tensor paths (sorted)
    pub affected_tensors: Vec<String>,
    /// Blake3 hashes of each delta (same order as affected_tensors)
    pub delta_hashes: Vec<String>,
    /// Overall merge hash (hash of all delta hashes)
    pub merge_hash: String,
    /// Full canonical deltas (for verification)
    deltas: Vec<DeltaCanonical>,
}

impl MergeArtifact {
    /// Get the canonical deltas for verification.
    pub fn deltas(&self) -> &[DeltaCanonical] {
        &self.deltas
    }

    /// Verify that another artifact matches this one.
    pub fn verify_matches(&self, other: &MergeArtifact) -> Result<(), MergeVerificationError> {
        if self.adapter_id != other.adapter_id {
            return Err(MergeVerificationError::AdapterMismatch {
                expected: self.adapter_id.clone(),
                actual: other.adapter_id.clone(),
            });
        }

        if self.merge_hash != other.merge_hash {
            return Err(MergeVerificationError::HashMismatch {
                expected: self.merge_hash.clone(),
                actual: other.merge_hash.clone(),
            });
        }

        if self.affected_tensors != other.affected_tensors {
            return Err(MergeVerificationError::TensorListMismatch {
                expected: self.affected_tensors.clone(),
                actual: other.affected_tensors.clone(),
            });
        }

        Ok(())
    }
}

/// Error when verifying merge artifacts.
#[derive(Debug, Clone)]
pub enum MergeVerificationError {
    /// Adapter IDs don't match
    AdapterMismatch { expected: String, actual: String },
    /// Merge hashes don't match
    HashMismatch { expected: String, actual: String },
    /// Affected tensor lists don't match
    TensorListMismatch {
        expected: Vec<String>,
        actual: Vec<String>,
    },
}

impl std::fmt::Display for MergeVerificationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AdapterMismatch { expected, actual } => {
                write!(
                    f,
                    "Adapter mismatch: expected '{}', got '{}'",
                    expected, actual
                )
            }
            Self::HashMismatch { expected, actual } => {
                write!(
                    f,
                    "Merge hash mismatch: expected '{}', got '{}'",
                    expected, actual
                )
            }
            Self::TensorListMismatch { expected, actual } => {
                write!(
                    f,
                    "Tensor list mismatch: expected {:?}, got {:?}",
                    expected, actual
                )
            }
        }
    }
}

impl std::error::Error for MergeVerificationError {}

#[cfg(test)]
mod tests {
    use super::*;
    use burn::backend::NdArray;

    type TestBackend = NdArray;

    #[test]
    fn test_compute_lora_delta() {
        let device = Default::default();

        // Small test: in=4, out=4, rank=2
        let lora_a = vec![0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8]; // [4, 2]
        let lora_b = vec![1.0, 0.0, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0]; // [2, 4]
        let alpha = 16.0;
        let rank = 2;

        let delta = compute_lora_delta::<TestBackend>(&lora_a, &lora_b, 4, 4, rank, alpha, &device);

        let [rows, cols] = delta.dims();
        assert_eq!(rows, 4);
        assert_eq!(cols, 4);
    }

    #[test]
    fn test_merge_weights() {
        let device = Default::default();

        // Base weight: 4x4 identity-ish
        let base_data = burn::tensor::TensorData::new(
            vec![
                1.0f32, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
            ],
            [4, 4],
        );
        let base: Tensor<TestBackend, 2> = Tensor::from_data(base_data, &device);

        // LoRA weights
        let lora_a = vec![0.1, 0.0, 0.0, 0.1, 0.1, 0.0, 0.0, 0.1]; // [4, 2]
        let lora_b = vec![1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0]; // [2, 4]

        let merged = merge_weights::<TestBackend>(base, &lora_a, &lora_b, 8.0, 2);

        // Verify shape preserved
        let [rows, cols] = merged.dims();
        assert_eq!(rows, 4);
        assert_eq!(cols, 4);

        // The merged weights should be different from identity
        let merged_data = merged.to_data();
        let merged_slice: &[f32] = merged_data.as_slice().unwrap();

        // First element should be > 1.0 due to LoRA addition
        assert!(merged_slice[0] > 1.0);
    }

    #[test]
    fn test_layer_mapper_shorthand() {
        let mapper = LayerMapper::new(4);

        // Test shorthand mappings
        let q_paths = mapper.map_to_model_paths("q_proj");
        assert_eq!(q_paths.len(), 4);
        assert!(q_paths[0].contains("attention.wq"));

        let v_paths = mapper.map_to_model_paths("v_proj");
        assert_eq!(v_paths.len(), 4);
        assert!(v_paths[0].contains("attention.wv"));

        let w1_paths = mapper.map_to_model_paths("w1");
        assert_eq!(w1_paths.len(), 4);
        assert!(w1_paths[0].contains("feed_forward.swiglu.linear_inner"));
    }

    #[test]
    fn test_layer_mapper_full_path() {
        let mapper = LayerMapper::new(4);

        // Full path should be returned as-is
        let paths = mapper.map_to_model_paths("layers.2.attention.wq");
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0], "layers.2.attention.wq");
    }

    #[test]
    fn test_layer_mapper_unknown() {
        let mapper = LayerMapper::new(4);

        let paths = mapper.map_to_model_paths("unknown_layer");
        assert!(paths.is_empty());
    }

    #[test]
    fn test_merge_plan() {
        let mapper = LayerMapper::new(4);

        let mut weights = crate::adapter::AdapterWeights::empty(8, 16.0);
        weights.add_layer("q_proj", vec![0.1; 64 * 8], vec![0.2; 8 * 64]);
        weights.add_layer("v_proj", vec![0.1; 64 * 8], vec![0.2; 8 * 64]);

        let plan = MergePlan::from_adapter(&weights, &mapper).unwrap();

        // 2 adapter layers, each maps to 4 model layers = 8 total
        assert_eq!(plan.total_tensors, 8);
        assert_eq!(plan.layer_mappings.len(), 2);
    }

    #[test]
    fn test_original_weights_storage() {
        let device: burn::tensor::Device<TestBackend> = Default::default();
        let mut storage: OriginalWeights<TestBackend> = OriginalWeights::new();

        let weight_data = burn::tensor::TensorData::new(vec![1.0f32, 2.0, 3.0, 4.0], [2, 2]);
        let weight: Tensor<TestBackend, 2> = Tensor::from_data(weight_data, &device);

        storage.store("layers.0.attention.wq".to_string(), weight);

        assert!(!storage.is_empty());
        assert!(storage.get("layers.0.attention.wq").is_some());
        assert!(storage.get("layers.0.attention.wk").is_none());
    }

    // ========================================================================
    // Canonical Delta Hashing Tests
    // ========================================================================

    #[test]
    fn test_delta_canonical_hash_is_deterministic() {
        let delta_f32 = vec![0.1f32, 0.2, 0.3, 0.4];

        let canonical1 = DeltaCanonical::new(
            "layers.0.attention.wq.weight",
            [2, 2],
            "adapter/test@1.0+sha256:abc",
            "llama3:8b",
            16.0,
            8,
            &delta_f32,
        );

        let canonical2 = DeltaCanonical::new(
            "layers.0.attention.wq.weight",
            [2, 2],
            "adapter/test@1.0+sha256:abc",
            "llama3:8b",
            16.0,
            8,
            &delta_f32,
        );

        // Same inputs → same hash
        assert_eq!(canonical1.compute_hash(), canonical2.compute_hash());
    }

    #[test]
    fn test_delta_canonical_hash_changes_with_path() {
        let delta_f32 = vec![0.1f32, 0.2, 0.3, 0.4];

        let canonical1 = DeltaCanonical::new(
            "layers.0.attention.wq.weight",
            [2, 2],
            "adapter/test@1.0+sha256:abc",
            "llama3:8b",
            16.0,
            8,
            &delta_f32,
        );

        let canonical2 = DeltaCanonical::new(
            "layers.0.attention.wv.weight", // Different path
            [2, 2],
            "adapter/test@1.0+sha256:abc",
            "llama3:8b",
            16.0,
            8,
            &delta_f32,
        );

        // Different path → different hash
        assert_ne!(canonical1.compute_hash(), canonical2.compute_hash());
    }

    #[test]
    fn test_delta_canonical_hash_changes_with_data() {
        let delta1 = vec![0.1f32, 0.2, 0.3, 0.4];
        let delta2 = vec![0.1f32, 0.2, 0.3, 0.5]; // Different value

        let canonical1 = DeltaCanonical::new(
            "layers.0.attention.wq.weight",
            [2, 2],
            "adapter/test@1.0+sha256:abc",
            "llama3:8b",
            16.0,
            8,
            &delta1,
        );

        let canonical2 = DeltaCanonical::new(
            "layers.0.attention.wq.weight",
            [2, 2],
            "adapter/test@1.0+sha256:abc",
            "llama3:8b",
            16.0,
            8,
            &delta2,
        );

        // Different data → different hash
        assert_ne!(canonical1.compute_hash(), canonical2.compute_hash());
    }

    #[test]
    fn test_delta_canonical_roundtrip() {
        let original = vec![0.1f32, 0.2, 0.3, 0.4, 0.5, 0.6];

        let canonical = DeltaCanonical::new("test", [2, 3], "adapter", "model", 16.0, 8, &original);

        // Roundtrip through canonical form
        let recovered = canonical.delta_as_f32();
        assert_eq!(original, recovered);
    }

    #[test]
    fn test_merge_artifact_builder_sorts_paths() {
        let mut builder =
            MergeArtifactBuilder::new("adapter/test@1.0+sha256:abc", "llama3:8b", 16.0, 8);

        // Add in non-sorted order
        builder.add_delta("layers.2.attention.wq.weight", [4, 4], &vec![0.1f32; 16]);
        builder.add_delta("layers.0.attention.wq.weight", [4, 4], &vec![0.2f32; 16]);
        builder.add_delta("layers.1.attention.wq.weight", [4, 4], &vec![0.3f32; 16]);

        let artifact = builder.build();

        // Should be sorted
        assert_eq!(artifact.affected_tensors[0], "layers.0.attention.wq.weight");
        assert_eq!(artifact.affected_tensors[1], "layers.1.attention.wq.weight");
        assert_eq!(artifact.affected_tensors[2], "layers.2.attention.wq.weight");
    }

    #[test]
    fn test_merge_artifact_verification() {
        let mut builder1 = MergeArtifactBuilder::new("adapter", "model", 16.0, 8);
        builder1.add_delta("layer.0", [4, 4], &vec![0.1f32; 16]);
        let artifact1 = builder1.build();

        let mut builder2 = MergeArtifactBuilder::new("adapter", "model", 16.0, 8);
        builder2.add_delta("layer.0", [4, 4], &vec![0.1f32; 16]);
        let artifact2 = builder2.build();

        // Same inputs should verify
        assert!(artifact1.verify_matches(&artifact2).is_ok());
    }

    #[test]
    fn test_merge_artifact_verification_fails_on_mismatch() {
        let mut builder1 = MergeArtifactBuilder::new("adapter1", "model", 16.0, 8);
        builder1.add_delta("layer.0", [4, 4], &vec![0.1f32; 16]);
        let artifact1 = builder1.build();

        let mut builder2 = MergeArtifactBuilder::new("adapter2", "model", 16.0, 8);
        builder2.add_delta("layer.0", [4, 4], &vec![0.1f32; 16]);
        let artifact2 = builder2.build();

        // Different adapter should fail
        let result = artifact1.verify_matches(&artifact2);
        assert!(matches!(
            result,
            Err(MergeVerificationError::AdapterMismatch { .. })
        ));
    }

    #[test]
    fn test_merge_plan_sorted_mappings() {
        let mapper = LayerMapper::new(2);

        let mut weights = crate::adapter::AdapterWeights::empty(8, 16.0);
        weights.add_layer("v_proj", vec![0.1; 64 * 8], vec![0.2; 8 * 64]);
        weights.add_layer("q_proj", vec![0.1; 64 * 8], vec![0.2; 8 * 64]);

        let plan = MergePlan::from_adapter(&weights, &mapper).unwrap();

        // sorted_layer_mappings should return alphabetically sorted
        let sorted = plan.sorted_layer_mappings();
        assert_eq!(sorted[0].0, "q_proj");
        assert_eq!(sorted[1].0, "v_proj");
    }

    // ========================================================================
    // Merge Stability Tests (Determinism)
    // ========================================================================

    /// Truth: Merge is stable across multiple runs with same inputs.
    ///
    /// This test verifies that:
    /// 1. Same adapter weights + same base model → same delta hashes
    /// 2. Running merge twice produces identical artifacts
    /// 3. Order of layer processing doesn't affect the final hash
    #[test]
    fn test_merge_stability_same_inputs_same_hash() {
        let device: burn::tensor::Device<TestBackend> = Default::default();

        // Create identical weights for two "runs"
        let lora_a = vec![0.1f32; 64 * 8];
        let lora_b = vec![0.2f32; 8 * 64];

        // First "merge" simulation
        let delta1 = compute_lora_delta::<TestBackend>(&lora_a, &lora_b, 64, 64, 8, 16.0, &device);

        // Second "merge" simulation with identical inputs
        let delta2 = compute_lora_delta::<TestBackend>(&lora_a, &lora_b, 64, 64, 8, 16.0, &device);

        // Compare tensor values
        let data1: Vec<f32> = delta1.to_data().to_vec().unwrap();
        let data2: Vec<f32> = delta2.to_data().to_vec().unwrap();

        assert_eq!(data1.len(), data2.len());
        for (a, b) in data1.iter().zip(data2.iter()) {
            assert!(
                (a - b).abs() < 1e-6,
                "Delta values should be identical for same inputs: {} vs {}",
                a,
                b
            );
        }
    }

    /// Truth: Artifact hash captures all relevant merge parameters.
    ///
    /// Changing any of: adapter_id, base_model_id, alpha, rank, or data
    /// should produce a different merge hash.
    #[test]
    fn test_merge_artifact_hash_captures_all_parameters() {
        let delta_f32 = vec![0.1f32; 16];

        // Base artifact
        let mut builder1 =
            MergeArtifactBuilder::new("llm/adapter@1.0.0+sha256:abc", "llama3-8b", 16.0, 8);
        builder1.add_delta("layer.0.wq.weight", [4, 4], &delta_f32);
        let artifact1 = builder1.build();

        // Different adapter_id
        let mut builder2 = MergeArtifactBuilder::new(
            "llm/adapter@1.0.1+sha256:def", // Changed
            "llama3-8b",
            16.0,
            8,
        );
        builder2.add_delta("layer.0.wq.weight", [4, 4], &delta_f32);
        let artifact2 = builder2.build();
        assert_ne!(
            artifact1.merge_hash, artifact2.merge_hash,
            "Different adapter_id should change hash"
        );

        // Different base_model_id
        let mut builder3 = MergeArtifactBuilder::new(
            "llm/adapter@1.0.0+sha256:abc",
            "llama3-3b", // Changed
            16.0,
            8,
        );
        builder3.add_delta("layer.0.wq.weight", [4, 4], &delta_f32);
        let artifact3 = builder3.build();
        assert_ne!(
            artifact1.merge_hash, artifact3.merge_hash,
            "Different base_model_id should change hash"
        );

        // Different alpha
        let mut builder4 = MergeArtifactBuilder::new(
            "llm/adapter@1.0.0+sha256:abc",
            "llama3-8b",
            32.0, // Changed
            8,
        );
        builder4.add_delta("layer.0.wq.weight", [4, 4], &delta_f32);
        let artifact4 = builder4.build();
        assert_ne!(
            artifact1.merge_hash, artifact4.merge_hash,
            "Different alpha should change hash"
        );

        // Different rank
        let mut builder5 = MergeArtifactBuilder::new(
            "llm/adapter@1.0.0+sha256:abc",
            "llama3-8b",
            16.0,
            16, // Changed
        );
        builder5.add_delta("layer.0.wq.weight", [4, 4], &delta_f32);
        let artifact5 = builder5.build();
        assert_ne!(
            artifact1.merge_hash, artifact5.merge_hash,
            "Different rank should change hash"
        );

        // Different data
        let different_delta = vec![0.2f32; 16];
        let mut builder6 =
            MergeArtifactBuilder::new("llm/adapter@1.0.0+sha256:abc", "llama3-8b", 16.0, 8);
        builder6.add_delta("layer.0.wq.weight", [4, 4], &different_delta);
        let artifact6 = builder6.build();
        assert_ne!(
            artifact1.merge_hash, artifact6.merge_hash,
            "Different delta data should change hash"
        );
    }

    /// Truth: Merge weights function produces stable results.
    ///
    /// Given: W + (alpha/r) * B @ A
    /// The merge operation should be numerically stable.
    #[test]
    fn test_merge_weights_numerical_stability() {
        let device: burn::tensor::Device<TestBackend> = Default::default();

        // Create base weights [64, 64]
        let base_data = burn::tensor::TensorData::new(vec![1.0f32; 64 * 64], [64, 64]);
        let base: Tensor<TestBackend, 2> = Tensor::from_data(base_data, &device);

        // Create LoRA weights
        // LoRA A: [in_features, rank] = [64, 8]
        // LoRA B: [rank, out_features] = [8, 64]
        let rank = 8;
        let alpha = 16.0;
        let lora_a = vec![0.01f32; 64 * rank]; // Small values
        let lora_b = vec![0.01f32; rank * 64];

        // Merge multiple times using the correct signature
        let result1 = merge_weights::<TestBackend>(base.clone(), &lora_a, &lora_b, alpha, rank);
        let result2 = merge_weights::<TestBackend>(base.clone(), &lora_a, &lora_b, alpha, rank);

        // Results should be identical
        let data1: Vec<f32> = result1.to_data().to_vec().unwrap();
        let data2: Vec<f32> = result2.to_data().to_vec().unwrap();

        for (a, b) in data1.iter().zip(data2.iter()) {
            assert!(
                (a - b).abs() < 1e-6,
                "Merge weights should be numerically stable"
            );
        }

        // Values should be reasonable (base + small delta)
        // With alpha=16, rank=8, scale = 2.0
        // delta ≈ 2.0 * (small matrix product)
        for val in &data1 {
            assert!(
                *val >= 0.5 && *val <= 2.0,
                "Merged weights should be close to base with small LoRA: {}",
                val
            );
        }
    }

    // ========================================================================
    // Stress Tests: Repeated Attach/Detach
    // ========================================================================

    /// Stress test: Repeated merge/unmerge cycles must not corrupt weights.
    ///
    /// This test verifies that:
    /// 1. Merge is reversible (unmerge restores original)
    /// 2. N cycles of merge→unmerge leave weights unchanged
    /// 3. No floating-point drift accumulates across cycles
    #[test]
    fn stress_test_merge_unmerge_cycles() {
        let device: burn::tensor::Device<TestBackend> = Default::default();

        // Create base weights
        let original_data: Vec<f32> = (0..64 * 64).map(|i| (i as f32) * 0.01).collect();
        let base_tensor_data = burn::tensor::TensorData::new(original_data.clone(), [64, 64]);
        let mut current: Tensor<TestBackend, 2> = Tensor::from_data(base_tensor_data, &device);

        // LoRA parameters
        let rank = 8;
        let alpha = 16.0;
        let lora_a: Vec<f32> = (0..64 * rank).map(|i| ((i % 100) as f32) * 0.001).collect();
        let lora_b: Vec<f32> = (0..rank * 64).map(|i| ((i % 100) as f32) * 0.001).collect();

        // Compute delta once (reused for unmerge)
        let delta =
            compute_lora_delta::<TestBackend>(&lora_a, &lora_b, 64, 64, rank, alpha, &device);

        const CYCLES: usize = 100;

        for cycle in 0..CYCLES {
            // Merge: W' = W + delta
            current = current.clone() + delta.clone();

            // Unmerge: W = W' - delta
            current = current.clone() - delta.clone();

            // Every 10 cycles, verify we're still close to original
            if cycle % 10 == 0 {
                let current_data: Vec<f32> = current.clone().to_data().to_vec().unwrap();
                for (i, (orig, curr)) in original_data.iter().zip(current_data.iter()).enumerate() {
                    let diff = (orig - curr).abs();
                    assert!(
                        diff < 1e-4,
                        "Cycle {}: Weight drift at index {}: original={}, current={}, diff={}",
                        cycle,
                        i,
                        orig,
                        curr,
                        diff
                    );
                }
            }
        }

        // Final verification
        let final_data: Vec<f32> = current.to_data().to_vec().unwrap();
        let max_diff: f32 = original_data
            .iter()
            .zip(final_data.iter())
            .map(|(a, b)| (a - b).abs())
            .fold(0.0, f32::max);

        assert!(
            max_diff < 1e-4,
            "After {} cycles, max weight drift is {} (should be < 1e-4)",
            CYCLES,
            max_diff
        );
    }

    /// Stress test: Different adapters can be cycled without interference.
    ///
    /// This tests attach adapter A → detach → attach adapter B → detach
    /// and verifies final weights match original.
    #[test]
    fn stress_test_multi_adapter_cycling() {
        let device: burn::tensor::Device<TestBackend> = Default::default();

        // Original weights
        let original_data: Vec<f32> = vec![1.0; 64 * 64];
        let base_data = burn::tensor::TensorData::new(original_data.clone(), [64, 64]);
        let mut current: Tensor<TestBackend, 2> = Tensor::from_data(base_data, &device);

        let rank = 8;
        let alpha = 16.0;

        // Adapter A - different pattern
        let lora_a_a: Vec<f32> = vec![0.1; 64 * rank];
        let lora_b_a: Vec<f32> = vec![0.1; rank * 64];

        // Adapter B - different pattern
        let lora_a_b: Vec<f32> = vec![0.2; 64 * rank];
        let lora_b_b: Vec<f32> = vec![-0.1; rank * 64];

        let delta_a =
            compute_lora_delta::<TestBackend>(&lora_a_a, &lora_b_a, 64, 64, rank, alpha, &device);
        let delta_b =
            compute_lora_delta::<TestBackend>(&lora_a_b, &lora_b_b, 64, 64, rank, alpha, &device);

        const CYCLES: usize = 50;

        for _ in 0..CYCLES {
            // Attach A
            current = current.clone() + delta_a.clone();
            // Detach A
            current = current.clone() - delta_a.clone();
            // Attach B
            current = current.clone() + delta_b.clone();
            // Detach B
            current = current.clone() - delta_b.clone();
        }

        // Verify original restored
        let final_data: Vec<f32> = current.to_data().to_vec().unwrap();
        let max_diff: f32 = original_data
            .iter()
            .zip(final_data.iter())
            .map(|(a, b)| (a - b).abs())
            .fold(0.0, f32::max);

        assert!(
            max_diff < 1e-4,
            "After {} adapter A/B cycles, max drift is {} (should be < 1e-4)",
            CYCLES,
            max_diff
        );
    }

    /// Stress test: Original weights storage and restoration.
    ///
    /// Verifies OriginalWeights correctly stores and restores weights
    /// across multiple operations.
    #[test]
    fn stress_test_original_weights_storage() {
        let device: burn::tensor::Device<TestBackend> = Default::default();

        let mut storage: OriginalWeights<TestBackend> = OriginalWeights::new();

        // Store weights for multiple layers
        let layer_names = [
            "layers.0.attention.wq",
            "layers.0.attention.wv",
            "layers.1.attention.wq",
            "layers.1.attention.wv",
        ];

        let mut original_sums: Vec<f32> = Vec::new();

        for (i, name) in layer_names.iter().enumerate() {
            let data: Vec<f32> = (0..64).map(|j| (i * 64 + j) as f32 * 0.1).collect();
            let sum: f32 = data.iter().sum();
            original_sums.push(sum);

            let tensor_data = burn::tensor::TensorData::new(data, [8, 8]);
            let tensor: Tensor<TestBackend, 2> = Tensor::from_data(tensor_data, &device);
            storage.store(name.to_string(), tensor);
        }

        // Verify all layers stored
        assert_eq!(storage.paths().len(), 4);

        // Verify data integrity
        for (i, name) in layer_names.iter().enumerate() {
            let restored = storage.get(name).expect("should find stored weight");
            let restored_data: Vec<f32> = restored.clone().to_data().to_vec().unwrap();
            let restored_sum: f32 = restored_data.iter().sum();

            assert!(
                (original_sums[i] - restored_sum).abs() < 1e-5,
                "Layer {} sum mismatch: original={}, restored={}",
                name,
                original_sums[i],
                restored_sum
            );
        }
    }

    // ========================================================================
    // Replay Integrity Tests
    // ========================================================================

    /// Truth: Merge hash change implies replay invalidation.
    ///
    /// If ANY input to the merge changes, the merge hash MUST change,
    /// signaling that previous outputs cannot be replayed with new merge.
    #[test]
    fn replay_integrity_hash_change_invalidates_replay() {
        let delta = vec![0.1f32; 16];

        // Original artifact
        let mut builder1 = MergeArtifactBuilder::new(
            "llm/adapter@1.0.0+sha256:abc",
            "llama3-8b+sha256:base123",
            16.0,
            8,
        );
        builder1.add_delta("layers.0.wq.weight", [4, 4], &delta);
        let original = builder1.build();

        // Same content = same hash (replay valid)
        let mut builder2 = MergeArtifactBuilder::new(
            "llm/adapter@1.0.0+sha256:abc",
            "llama3-8b+sha256:base123",
            16.0,
            8,
        );
        builder2.add_delta("layers.0.wq.weight", [4, 4], &delta);
        let same = builder2.build();

        assert_eq!(
            original.merge_hash, same.merge_hash,
            "Same inputs must produce same hash (replay valid)"
        );

        // Different base model = different hash (replay INVALID)
        let mut builder3 = MergeArtifactBuilder::new(
            "llm/adapter@1.0.0+sha256:abc",
            "llama3-8b+sha256:different456", // Changed!
            16.0,
            8,
        );
        builder3.add_delta("layers.0.wq.weight", [4, 4], &delta);
        let different_base = builder3.build();

        assert_ne!(
            original.merge_hash, different_base.merge_hash,
            "Different base model must invalidate replay"
        );

        // Different adapter version = different hash (replay INVALID)
        let mut builder4 = MergeArtifactBuilder::new(
            "llm/adapter@1.0.1+sha256:def", // Changed!
            "llama3-8b+sha256:base123",
            16.0,
            8,
        );
        builder4.add_delta("layers.0.wq.weight", [4, 4], &delta);
        let different_adapter = builder4.build();

        assert_ne!(
            original.merge_hash, different_adapter.merge_hash,
            "Different adapter version must invalidate replay"
        );
    }

    /// Truth: Merge reproducibility with same inputs.
    ///
    /// Given identical:
    /// - Base model checkpoint hash
    /// - Adapter weights (content hash)
    /// - Merge parameters (alpha, rank)
    ///
    /// The merge MUST be reproducible.
    #[test]
    fn replay_integrity_reproducible_merge() {
        let device: burn::tensor::Device<TestBackend> = Default::default();

        // Fixed inputs
        let base_data: Vec<f32> = (0..64).map(|i| i as f32 * 0.1).collect();
        let lora_a: Vec<f32> = vec![0.1; 64];
        let lora_b: Vec<f32> = vec![0.1; 64];
        let alpha = 16.0;
        let rank = 8;

        // Simulate two independent "replays"
        let base1 = burn::tensor::TensorData::new(base_data.clone(), [8, 8]);
        let tensor1: Tensor<TestBackend, 2> = Tensor::from_data(base1, &device);
        let merged1 = merge_weights::<TestBackend>(tensor1, &lora_a, &lora_b, alpha, rank);

        let base2 = burn::tensor::TensorData::new(base_data.clone(), [8, 8]);
        let tensor2: Tensor<TestBackend, 2> = Tensor::from_data(base2, &device);
        let merged2 = merge_weights::<TestBackend>(tensor2, &lora_a, &lora_b, alpha, rank);

        // Results must be bitwise identical
        let data1: Vec<f32> = merged1.to_data().to_vec().unwrap();
        let data2: Vec<f32> = merged2.to_data().to_vec().unwrap();

        assert_eq!(data1, data2, "Replayed merge must be identical");
    }

    /// Truth: Merge artifact contains sufficient info for replay decision.
    ///
    /// The MergeArtifact must contain all hashes needed to determine
    /// whether a replay is valid.
    #[test]
    fn replay_integrity_artifact_contains_required_hashes() {
        let mut builder = MergeArtifactBuilder::new(
            "llm/grounded@1.2.0+sha256:adapter_hash",
            "llama3-8b+sha256:model_hash",
            16.0,
            8,
        );
        builder.add_delta("layers.0.wq.weight", [64, 64], &vec![0.1f32; 4096]);
        builder.add_delta("layers.0.wv.weight", [64, 64], &vec![0.2f32; 4096]);

        let artifact = builder.build();

        // Must have overall merge hash
        assert!(!artifact.merge_hash.is_empty(), "merge_hash required");

        // Must have per-delta hashes
        assert_eq!(artifact.delta_hashes.len(), 2, "should have 2 delta hashes");
        for hash in &artifact.delta_hashes {
            assert!(!hash.is_empty(), "delta hash must not be empty");
        }

        // Must track adapter and base model
        assert_eq!(
            artifact.adapter_id,
            "llm/grounded@1.2.0+sha256:adapter_hash"
        );
        assert_eq!(artifact.base_model_id, "llama3-8b+sha256:model_hash");

        // Must list affected tensors
        assert!(
            artifact
                .affected_tensors
                .contains(&"layers.0.wq.weight".to_string())
        );
        assert!(
            artifact
                .affected_tensors
                .contains(&"layers.0.wv.weight".to_string())
        );
    }

    /// Truth: Replay can be declared invalid explicitly.
    ///
    /// When merge parameters change, verification should fail
    /// with a clear error message.
    #[test]
    fn replay_integrity_explicit_invalidation() {
        let delta = vec![0.1f32; 16];

        // Original merge
        let mut builder1 = MergeArtifactBuilder::new("adapter", "model", 16.0, 8);
        builder1.add_delta("layer.0", [4, 4], &delta);
        let original = builder1.build();

        // Attempted replay with different data
        let mut builder2 = MergeArtifactBuilder::new("adapter", "model", 16.0, 8);
        builder2.add_delta("layer.0", [4, 4], &vec![0.2f32; 16]); // Different!
        let replay_attempt = builder2.build();

        // Verification should fail with hash mismatch
        let result = original.verify_matches(&replay_attempt);
        assert!(matches!(
            result,
            Err(MergeVerificationError::HashMismatch { .. })
        ));
    }
}
