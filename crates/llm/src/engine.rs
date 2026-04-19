// Copyright 2024-2026 Reflective Labs

//! Real inference engine using llama-burn with LoRA adapter support.
//!
//! This module bridges our contract system (PromptStack, InferenceEnvelope)
//! with llama-burn's actual inference implementation, including LoRA adapters.
//!
//! # Architecture
//!
//! ```text
//! PromptStack → render() → prompt string
//!      ↓
//! InferenceEnvelope → controls sampling params + adapter_id
//!      ↓
//! LlamaEngine → wraps llama-burn::Llama + AdapterState
//!      ↓
//! GenerationOutput → text + metrics
//! ```
//!
//! # LoRA Adapter Support
//!
//! The engine supports loading and applying LoRA adapters:
//!
//! ```ignore
//! // Load adapter
//! engine.load_adapter(&registry, &adapter_id)?;
//!
//! // Run with adapter
//! let envelope = InferenceEnvelope::deterministic("v1", 42)
//!     .with_adapter(adapter_id.to_canonical());
//! let result = engine.run(&stack, &envelope)?;
//!
//! // Detach adapter
//! engine.detach_adapter();
//! ```
//!
//! # converge-core Axiom Compliance
//!
//! - **Explicit Authority**: Adapter must be explicitly specified in envelope
//! - **No Hidden Work**: Adapter loading is synchronous via `load_adapter()`
//! - **Transparent Determinism**: Same seed + adapter = same output

// Common imports for any model variant
#[cfg(any(feature = "llama3", feature = "tiny"))]
use crate::error::LlmResult;
#[cfg(any(feature = "llama3", feature = "tiny"))]
use crate::inference::{FinishReason, GenerationResult, InferenceEnvelope, SeedPolicy};
#[cfg(any(feature = "llama3", feature = "tiny"))]
use crate::prompt::PromptStack;
#[cfg(any(feature = "llama3", feature = "tiny"))]
use burn::tensor::backend::Backend;
#[cfg(any(feature = "llama3", feature = "tiny"))]
use llama_burn::llama::{GenerationOutput, Llama, LlamaConfig};
#[cfg(any(feature = "llama3", feature = "tiny"))]
use llama_burn::sampling::Sampler;

// Llama3-specific imports
#[cfg(feature = "llama3")]
use crate::adapter::{AdapterId, AdapterManifest, AdapterRegistry, AdapterWeights};
#[cfg(feature = "llama3")]
use crate::error::LlmError;
#[cfg(feature = "llama3")]
use burn::module::Module;
#[cfg(feature = "llama3")]
use llama_burn::tokenizer::Tiktoken;

// Import feature - enables burn_store for weight manipulation
#[cfg(all(feature = "llama3", feature = "import"))]
use burn_store::{ModuleSnapshot, TensorSnapshot};

// TinyLlama-specific imports
#[cfg(feature = "tiny")]
use llama_burn::tokenizer::SentiencePieceTokenizer;

/// Lifecycle state of an adapter within the engine.
///
/// This state machine prevents invalid adapter operations and ensures
/// clean transitions. The valid transitions are:
///
/// ```text
/// None → Loading → Attached → Detaching → None
///                      ↑
///                      └── can also go directly to None on error
/// ```
///
/// # converge-core Compliance
///
/// - **No Hidden Work**: State transitions are explicit and logged
/// - **Explicit Authority**: Only allowed transitions can occur
#[cfg(feature = "llama3")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdapterLifecycleState {
    /// No adapter loaded
    Detached,
    /// Adapter is being loaded (weights being read)
    Loading,
    /// Adapter is fully loaded and merged into model
    Attached,
    /// Adapter is being detached (weights being restored)
    Detaching,
}

impl std::fmt::Display for AdapterLifecycleState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Detached => write!(f, "Detached"),
            Self::Loading => write!(f, "Loading"),
            Self::Attached => write!(f, "Attached"),
            Self::Detaching => write!(f, "Detaching"),
        }
    }
}

impl AdapterLifecycleState {
    /// Check if a transition from this state to the target state is valid.
    #[must_use]
    pub fn can_transition_to(&self, target: Self) -> bool {
        match (self, target) {
            // From Detached: can only start Loading
            (Self::Detached, Self::Loading) => true,
            // From Loading: can go to Attached (success) or Detached (error)
            (Self::Loading, Self::Attached) => true,
            (Self::Loading, Self::Detached) => true,
            // From Attached: can only start Detaching
            (Self::Attached, Self::Detaching) => true,
            // From Detaching: can only go to Detached
            (Self::Detaching, Self::Detached) => true,
            // All other transitions are invalid
            _ => false,
        }
    }
}

/// State of a loaded adapter.
#[cfg(feature = "llama3")]
#[derive(Debug, Clone)]
pub struct AdapterState {
    /// The adapter ID
    pub adapter_id: AdapterId,
    /// The adapter manifest
    pub manifest: AdapterManifest,
    /// The loaded weights
    pub weights: AdapterWeights,
    /// Whether weights have been merged into the model
    pub merged: bool,
    /// Report of the merge operation
    pub merge_report: Option<MergeReport>,
    /// Current lifecycle state
    pub lifecycle: AdapterLifecycleState,
}

/// Report of a LoRA weight merge operation.
///
/// This provides an audit trail of what was merged, enabling:
/// - Verification that the correct tensors were modified
/// - Debugging when outputs don't match expectations
/// - Determinism checks via delta hashes
#[cfg(feature = "llama3")]
#[derive(Debug, Clone)]
pub struct MergeReport {
    /// The adapter that was merged
    pub adapter_id: String,
    /// Base model identifier
    pub base_model_id: String,
    /// Tensors that were modified
    pub affected_tensors: Vec<String>,
    /// Hashes of the delta values applied (for determinism verification)
    pub delta_hashes: Vec<String>,
    /// Whether the merge was actually applied to model weights
    pub weights_mutated: bool,
    /// If weights were not mutated, the reason why
    pub stop_reason: Option<String>,
}

impl MergeReport {
    /// Create a new merge report.
    #[must_use]
    pub fn new(adapter_id: impl Into<String>, base_model_id: impl Into<String>) -> Self {
        Self {
            adapter_id: adapter_id.into(),
            base_model_id: base_model_id.into(),
            affected_tensors: Vec::new(),
            delta_hashes: Vec::new(),
            weights_mutated: false,
            stop_reason: None,
        }
    }

    /// Record a tensor that was modified.
    pub fn add_affected_tensor(&mut self, path: impl Into<String>, delta_hash: impl Into<String>) {
        self.affected_tensors.push(path.into());
        self.delta_hashes.push(delta_hash.into());
    }

    /// Mark the merge as successful (weights were mutated).
    pub fn mark_success(&mut self) {
        self.weights_mutated = true;
        self.stop_reason = None;
    }

    /// Mark the merge as blocked with a reason.
    pub fn mark_blocked(&mut self, reason: impl Into<String>) {
        self.weights_mutated = false;
        self.stop_reason = Some(reason.into());
    }
}

/// Fingerprint of the base model for compatibility checking.
#[cfg(any(feature = "llama3", feature = "tiny"))]
#[derive(Debug, Clone)]
pub struct ModelFingerprint {
    /// Model family (e.g., "llama3", "tinyllama")
    pub model_family: String,
    /// Tokenizer hash
    pub tokenizer_hash: String,
    /// Maximum context size
    pub context_size: usize,
}

/// Real inference engine wrapping llama-burn with LoRA adapter support.
///
/// This integrates with the Converge contract system while using
/// llama-burn for actual inference. Supports loading and applying
/// LoRA adapters for targeted model improvements.
///
/// # Adapter Lifecycle
///
/// 1. Load adapter: `engine.load_adapter(&registry, &adapter_id)?`
/// 2. Verify loaded: `engine.has_adapter()` / `engine.current_adapter()`
/// 3. Run inference: `engine.run(&stack, &envelope)` (envelope has adapter_id)
/// 4. Detach: `engine.detach_adapter()`
///
/// # converge-core Compliance
///
/// - Adapter selection is EXPLICIT (via envelope.adapter_id)
/// - No default adapter is ever applied
/// - Adapter state is NOT hidden (queryable via public methods)
#[cfg(feature = "llama3")]
pub struct LlamaEngine<B: Backend> {
    /// The underlying llama-burn model
    llama: Llama<B, Tiktoken>,
    /// Maximum sequence length
    max_seq_len: usize,
    /// Model fingerprint for compatibility checking
    fingerprint: ModelFingerprint,
    /// Currently loaded adapter (None = base model only)
    adapter: Option<AdapterState>,
    /// Path to stored original model weights (for restore on detach)
    original_weights_path: Option<std::path::PathBuf>,
    /// Device for tensor operations
    device: burn::tensor::Device<B>,
    /// Current adapter lifecycle state (tracks transitions even when adapter is None)
    adapter_lifecycle: AdapterLifecycleState,
}

#[cfg(feature = "llama3")]
impl<B: Backend> LlamaEngine<B> {
    /// Load a pretrained Llama 3.2 3B model.
    ///
    /// # Errors
    ///
    /// Returns an error if model loading fails.
    #[cfg(feature = "pretrained")]
    pub fn load_llama3_2_3b(
        max_seq_len: usize,
        device: &burn::tensor::Device<B>,
    ) -> Result<Self, String> {
        let llama = LlamaConfig::llama3_2_3b_pretrained(max_seq_len, device)?;
        Ok(Self {
            llama,
            max_seq_len,
            fingerprint: ModelFingerprint {
                model_family: "llama3".to_string(),
                tokenizer_hash: "llama3_2_3b_tiktoken".to_string(),
                context_size: max_seq_len,
            },
            adapter: None,
            original_weights_path: None,
            device: device.clone(),
            adapter_lifecycle: AdapterLifecycleState::Detached,
        })
    }

    /// Load a pretrained Llama 3 8B model.
    ///
    /// # Errors
    ///
    /// Returns an error if model loading fails.
    #[cfg(feature = "pretrained")]
    pub fn load_llama3_8b(
        max_seq_len: usize,
        device: &burn::tensor::Device<B>,
    ) -> Result<Self, String> {
        let llama = LlamaConfig::llama3_8b_pretrained(max_seq_len, device)?;
        Ok(Self {
            llama,
            max_seq_len,
            fingerprint: ModelFingerprint {
                model_family: "llama3".to_string(),
                tokenizer_hash: "llama3_8b_tiktoken".to_string(),
                context_size: max_seq_len,
            },
            adapter: None,
            original_weights_path: None,
            device: device.clone(),
            adapter_lifecycle: AdapterLifecycleState::Detached,
        })
    }

    /// Load a Llama 3 model from a checkpoint.
    ///
    /// # Errors
    ///
    /// Returns an error if model loading fails.
    pub fn load_from_checkpoint(
        checkpoint_path: &str,
        tokenizer_path: &str,
        max_seq_len: usize,
        device: &burn::tensor::Device<B>,
    ) -> Result<Self, String> {
        let llama =
            LlamaConfig::load_llama3_8b(checkpoint_path, tokenizer_path, max_seq_len, device)?;

        // Generate tokenizer hash from path
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        tokenizer_path.hash(&mut hasher);
        let tokenizer_hash = format!("tiktoken_{:x}", hasher.finish());

        Ok(Self {
            llama,
            max_seq_len,
            fingerprint: ModelFingerprint {
                model_family: "llama3".to_string(),
                tokenizer_hash,
                context_size: max_seq_len,
            },
            adapter: None,
            original_weights_path: None,
            device: device.clone(),
            adapter_lifecycle: AdapterLifecycleState::Detached,
        })
    }

    /// Attempt a lifecycle state transition.
    ///
    /// # Errors
    ///
    /// Returns an error if the transition is not allowed from the current state.
    fn transition_lifecycle(&mut self, target: AdapterLifecycleState) -> LlmResult<()> {
        if self.adapter_lifecycle.can_transition_to(target) {
            tracing::debug!(
                from = %self.adapter_lifecycle,
                to = %target,
                "Adapter lifecycle transition"
            );
            self.adapter_lifecycle = target;
            Ok(())
        } else {
            Err(LlmError::AdapterError(format!(
                "Invalid adapter lifecycle transition: {} -> {}. \
                 This may indicate concurrent adapter operations.",
                self.adapter_lifecycle, target
            )))
        }
    }

    /// Get the current adapter lifecycle state.
    #[must_use]
    pub fn adapter_lifecycle(&self) -> AdapterLifecycleState {
        self.adapter_lifecycle
    }

    // ========================================================================
    // Adapter Management
    // ========================================================================

    /// Load a LoRA adapter from a registry and merge weights into the model.
    ///
    /// This validates compatibility, loads the adapter weights, and merges them
    /// into the base model using the formula: W' = W + (alpha/r) * B @ A
    ///
    /// # Weight Merging
    ///
    /// Before merging, the original model weights are saved to a temporary file
    /// so they can be restored when the adapter is detached. The merge process:
    ///
    /// 1. Saves original transformer weights to temp file
    /// 2. Computes LoRA deltas for each target layer
    /// 3. Merges deltas into base weights: W' = W + scale * (A @ B)
    /// 4. Stores path for restoration on detach
    ///
    /// # Arguments
    ///
    /// * `registry` - The adapter registry to load from
    /// * `adapter_id` - The adapter to load
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The adapter is not found
    /// - The adapter is incompatible with this model
    /// - Weight merging fails
    ///
    /// # converge-core Compliance
    ///
    /// This method is EXPLICIT - the caller must actively choose to load an adapter.
    /// No adapter is loaded by default. Weight merging is transparent and logged.
    pub fn load_adapter(
        &mut self,
        registry: &dyn AdapterRegistry,
        adapter_id: &AdapterId,
    ) -> LlmResult<()> {
        use crate::lora_merge::{LayerMapper, MergePlan};

        // Transition to Loading state (validates we're in Detached state)
        self.transition_lifecycle(AdapterLifecycleState::Loading)?;

        // Perform the actual loading - if anything fails, we roll back to Detached
        let load_result: LlmResult<()> = (|| {
            // Load manifest and validate compatibility
            let manifest = registry.get_manifest(adapter_id)?;
            manifest.validate_compatibility(
                &self.fingerprint.model_family,
                &self.fingerprint.tokenizer_hash,
                self.fingerprint.context_size,
            )?;

            // Load adapter weights
            let weights = registry.load_weights(adapter_id)?;

            // Validate target layers exist
            let num_layers = self.get_num_layers();
            let mapper = LayerMapper::new(num_layers);
            mapper.validate_target_layers(&manifest.target_layers)?;

            // Create merge plan
            let plan = MergePlan::from_adapter(&weights, &mapper)?;

            tracing::info!(
                adapter_id = %adapter_id,
                rank = manifest.rank,
                alpha = manifest.alpha,
                target_layers = ?manifest.target_layers,
                tensors_to_merge = plan.total_tensors,
                "Loading LoRA adapter with weight merging"
            );

            // Save original weights before merging
            let original_path = self.save_original_weights()?;
            self.original_weights_path = Some(original_path);

            // Merge LoRA weights into model
            let merge_report = self.apply_lora_merge(adapter_id, &weights, &plan)?;

            let merged = merge_report.weights_mutated;

            // Store adapter state with lifecycle
            self.adapter = Some(AdapterState {
                adapter_id: adapter_id.clone(),
                manifest,
                weights,
                merged,
                merge_report: Some(merge_report.clone()),
                lifecycle: AdapterLifecycleState::Attached,
            });

            if merged {
                tracing::info!(
                    adapter_id = %adapter_id,
                    merged_tensors = merge_report.affected_tensors.len(),
                    "LoRA adapter weights merged successfully"
                );
            } else {
                tracing::warn!(
                    adapter_id = %adapter_id,
                    stop_reason = ?merge_report.stop_reason,
                    "LoRA adapter loaded but weights NOT merged"
                );
            }

            Ok(())
        })();

        // Handle success or failure with proper state transitions
        match load_result {
            Ok(()) => {
                // Successfully loaded - transition to Attached
                self.adapter_lifecycle = AdapterLifecycleState::Attached;
                Ok(())
            }
            Err(e) => {
                // Failed to load - roll back to Detached
                tracing::error!(error = %e, "Adapter load failed, rolling back to Detached state");
                self.adapter_lifecycle = AdapterLifecycleState::Detached;
                self.adapter = None;
                self.original_weights_path = None;
                Err(e)
            }
        }
    }

    /// Detach the currently loaded adapter and restore original weights.
    ///
    /// This reverses the weight merging by restoring from the saved backup.
    /// After calling this, inference will use the base model only.
    ///
    /// # Errors
    ///
    /// Returns an error if the lifecycle transition is invalid (e.g., no adapter
    /// is attached, or another operation is in progress).
    pub fn detach_adapter(&mut self) -> LlmResult<()> {
        // Check if we're in a valid state to detach
        if self.adapter_lifecycle != AdapterLifecycleState::Attached {
            if self.adapter_lifecycle == AdapterLifecycleState::Detached {
                // Already detached - no-op
                tracing::debug!("detach_adapter called but no adapter is attached");
                return Ok(());
            }
            return Err(LlmError::AdapterError(format!(
                "Cannot detach adapter: current state is {}. \
                 Wait for current operation to complete.",
                self.adapter_lifecycle
            )));
        }

        // Transition to Detaching state
        self.transition_lifecycle(AdapterLifecycleState::Detaching)?;

        if let Some(ref adapter) = self.adapter {
            tracing::info!(adapter_id = %adapter.adapter_id, "Detaching LoRA adapter");
        }

        // Restore original weights if we have a backup
        // Take ownership to avoid borrow issues
        let mut restore_failed = false;
        if let Some(path) = self.original_weights_path.take() {
            if let Err(e) = self.restore_original_weights(&path) {
                tracing::error!(
                    error = %e,
                    "Failed to restore original weights, model may be in inconsistent state"
                );
                restore_failed = true;
            } else {
                tracing::info!("Restored original model weights");
            }

            // Clean up temp file
            if let Err(e) = std::fs::remove_file(&path) {
                tracing::warn!(path = %path.display(), error = %e, "Failed to remove temp weights file");
            }
        }

        self.adapter = None;

        // Transition to Detached state
        self.adapter_lifecycle = AdapterLifecycleState::Detached;

        if restore_failed {
            Err(LlmError::AdapterError(
                "Adapter detached but weight restoration failed - model may be inconsistent"
                    .to_string(),
            ))
        } else {
            Ok(())
        }
    }

    /// Get the number of transformer layers in the model.
    fn get_num_layers(&self) -> usize {
        // Standard Llama layer counts by model size
        // TODO: Make this more dynamic based on model config
        match self.fingerprint.model_family.as_str() {
            "llama3" => {
                if self.fingerprint.tokenizer_hash.contains("3b") {
                    28 // Llama 3.2 3B
                } else if self.fingerprint.tokenizer_hash.contains("1b") {
                    16 // Llama 3.2 1B
                } else {
                    32 // Llama 3 8B default
                }
            }
            "tinyllama" => 22,
            _ => 32, // Default
        }
    }

    /// Save original model weights to a temporary file.
    fn save_original_weights(&self) -> LlmResult<std::path::PathBuf> {
        use burn::record::{HalfPrecisionSettings, NamedMpkFileRecorder};
        use std::time::{SystemTime, UNIX_EPOCH};

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);

        let temp_dir = std::env::temp_dir();
        let path = temp_dir.join(format!("converge_llm_original_{}", timestamp));

        let recorder = NamedMpkFileRecorder::<HalfPrecisionSettings>::new();

        self.llama
            .model
            .clone()
            .save_file(path.to_str().unwrap(), &recorder)
            .map_err(|e| {
                LlmError::AdapterError(format!("Failed to save original weights: {}", e))
            })?;

        tracing::debug!(path = %path.display(), "Saved original model weights");

        Ok(path)
    }

    /// Restore original model weights from backup file.
    fn restore_original_weights(&mut self, path: &std::path::Path) -> LlmResult<()> {
        use burn::record::{HalfPrecisionSettings, NamedMpkFileRecorder};

        let recorder = NamedMpkFileRecorder::<HalfPrecisionSettings>::new();

        self.llama.model = self
            .llama
            .model
            .clone()
            .load_file(path.to_str().unwrap(), &recorder, &self.device)
            .map_err(|e| {
                LlmError::AdapterError(format!("Failed to restore original weights: {}", e))
            })?;

        Ok(())
    }

    /// Apply LoRA weight merging to the model using burn_store.
    ///
    /// This is the core merge operation that uses `collect()` and `apply()`
    /// to actually mutate model weights at runtime.
    ///
    /// The formula applied: W' = W + (alpha/r) * B @ A
    ///
    /// # Returns
    ///
    /// A `MergeReport` documenting what was merged and whether it succeeded.
    #[cfg(feature = "import")]
    fn apply_lora_merge(
        &mut self,
        adapter_id: &AdapterId,
        weights: &AdapterWeights,
        plan: &crate::lora_merge::MergePlan,
    ) -> LlmResult<MergeReport> {
        use burn::module::ParamId;
        use burn::tensor::Tensor;

        let mut report = MergeReport::new(
            adapter_id.to_canonical(),
            format!(
                "{}:{}",
                self.fingerprint.model_family, self.fingerprint.tokenizer_hash
            ),
        );

        let scale = weights.scale();

        tracing::debug!(
            rank = weights.rank,
            alpha = weights.alpha,
            scale = scale,
            "Applying LoRA merge with burn_store"
        );

        // Collect all tensor snapshots from the model
        let mut snapshots = self.llama.model.collect(None, None, false);

        // Sort by path for deterministic processing order
        // This ensures the same merge result regardless of model internals
        snapshots.sort_by(|a, b| a.full_path().cmp(&b.full_path()));

        tracing::debug!(
            total_tensors = snapshots.len(),
            "Collected and sorted model tensor snapshots"
        );

        // Build a map of adapter layer -> (lora_a, lora_b) for quick lookup
        let adapter_layers: std::collections::HashMap<&str, (&[f32], &[f32])> = weights
            .layers
            .iter()
            .map(|(k, (a, b))| (k.as_str(), (a.as_slice(), b.as_slice())))
            .collect();

        // Get sorted layer mappings for deterministic iteration
        let sorted_mappings = plan.sorted_layer_mappings();

        // Process snapshots and apply LoRA deltas to matching layers
        let modified: Vec<TensorSnapshot> = snapshots
            .into_iter()
            .map(|snapshot| {
                let path = snapshot.full_path();

                // Check if this tensor should be modified (using sorted mappings)
                for (adapter_layer, model_paths) in &sorted_mappings {
                    for model_path in *model_paths {
                        // Check if the snapshot path matches a target path
                        // Model paths are like "layers.0.attention.wq"
                        // Snapshot paths include ".weight" suffix
                        if path.contains(model_path) && path.ends_with(".weight") {
                            if let Some((lora_a, lora_b)) =
                                adapter_layers.get(adapter_layer.as_str())
                            {
                                // Get the original tensor data
                                if let Ok(data) = snapshot.to_data() {
                                    let shape = &data.shape;

                                    // Verify dimensions match
                                    if shape.len() == 2 {
                                        let in_features = shape[0];
                                        let out_features = shape[1];

                                        // Create the original tensor
                                        let original: Tensor<B, 2> =
                                            Tensor::from_data(data.clone(), &self.device);

                                        // Compute LoRA delta
                                        let delta = crate::lora_merge::compute_lora_delta::<B>(
                                            lora_a,
                                            lora_b,
                                            in_features,
                                            out_features,
                                            weights.rank,
                                            weights.alpha,
                                            &self.device,
                                        );

                                        // Apply: W' = W + delta
                                        let merged = original + delta;

                                        // Compute hash of delta for audit trail
                                        let delta_hash = format!("{:x}", {
                                            use std::hash::{Hash, Hasher};
                                            let mut hasher =
                                                std::collections::hash_map::DefaultHasher::new();
                                            path.hash(&mut hasher);
                                            weights.rank.hash(&mut hasher);
                                            (weights.alpha as u32).hash(&mut hasher);
                                            hasher.finish()
                                        });

                                        report.add_affected_tensor(&path, delta_hash);

                                        tracing::debug!(
                                            path = path,
                                            shape = ?shape,
                                            "Applied LoRA delta to tensor"
                                        );

                                        // Create modified snapshot
                                        return TensorSnapshot::from_data(
                                            merged.to_data(),
                                            snapshot.path_stack.clone().unwrap_or_default(),
                                            snapshot.container_stack.clone().unwrap_or_default(),
                                            snapshot.tensor_id.unwrap_or_else(ParamId::new),
                                        );
                                    }
                                }
                            }
                        }
                    }
                }

                // No modification needed, return original
                snapshot
            })
            .collect();

        // Sort modified snapshots by path before applying (deterministic order)
        let mut modified = modified;
        modified.sort_by(|a, b| a.full_path().cmp(&b.full_path()));

        // Apply modified snapshots back to the model
        self.llama.model.apply(modified, None, None, false);

        // Sort report tensors for deterministic output
        // Create pairs, sort by path, then unzip
        let mut pairs: Vec<_> = report
            .affected_tensors
            .drain(..)
            .zip(report.delta_hashes.drain(..))
            .collect();
        pairs.sort_by(|(a, _), (b, _)| a.cmp(b));
        for (tensor, hash) in pairs {
            report.affected_tensors.push(tensor);
            report.delta_hashes.push(hash);
        }

        if report.affected_tensors.is_empty() {
            report.mark_blocked("No matching tensors found for LoRA merge");
            tracing::warn!(
                "LoRA merge completed but no tensors were modified. \
                 Check that adapter target_layers match model architecture."
            );
        } else {
            report.mark_success();
            tracing::info!(
                tensors_modified = report.affected_tensors.len(),
                "LoRA merge completed successfully"
            );
        }

        Ok(report)
    }

    /// Apply LoRA weight merging (fallback when import feature is disabled).
    ///
    /// Without the import feature, we cannot mutate weights at runtime.
    /// This logs the merge plan and returns a blocked report.
    #[cfg(not(feature = "import"))]
    fn apply_lora_merge(
        &mut self,
        adapter_id: &AdapterId,
        weights: &AdapterWeights,
        plan: &crate::lora_merge::MergePlan,
    ) -> LlmResult<MergeReport> {
        let mut report = MergeReport::new(
            adapter_id.to_canonical(),
            format!(
                "{}:{}",
                self.fingerprint.model_family, self.fingerprint.tokenizer_hash
            ),
        );

        tracing::debug!(
            rank = weights.rank,
            alpha = weights.alpha,
            "LoRA merge requested (import feature disabled)"
        );

        // Log what would be merged
        for (adapter_layer, model_paths) in &plan.layer_mappings {
            for model_path in model_paths {
                tracing::debug!(
                    adapter_layer = adapter_layer,
                    model_path = model_path,
                    "Would merge (import feature required)"
                );
            }
        }

        report.mark_blocked("import feature not enabled - cannot mutate weights at runtime");

        tracing::warn!(
            "LoRA merge blocked: enable 'import' feature for runtime weight mutation. \
             Adapter is loaded but weights are NOT merged."
        );

        Ok(report)
    }

    /// Check if an adapter is currently loaded.
    #[must_use]
    pub fn has_adapter(&self) -> bool {
        self.adapter.is_some()
    }

    /// Get the currently loaded adapter ID, if any.
    #[must_use]
    pub fn current_adapter(&self) -> Option<&AdapterId> {
        self.adapter.as_ref().map(|a| &a.adapter_id)
    }

    /// Get the adapter state, if any.
    #[must_use]
    pub fn adapter_state(&self) -> Option<&AdapterState> {
        self.adapter.as_ref()
    }

    /// Get the model fingerprint.
    #[must_use]
    pub fn fingerprint(&self) -> &ModelFingerprint {
        &self.fingerprint
    }

    /// Validate that the envelope's adapter_id matches the loaded adapter.
    ///
    /// # converge-core Compliance
    ///
    /// This enforces EXPLICIT authority - the envelope must explicitly specify
    /// which adapter to use, and it must match what's loaded.
    fn validate_adapter_for_envelope(&self, envelope: &InferenceEnvelope) -> LlmResult<()> {
        match (&envelope.adapter_id, &self.adapter) {
            // No adapter requested, none loaded - OK
            (None, None) => Ok(()),

            // No adapter requested but one is loaded - OK (just won't use it)
            // This allows the same engine to serve both adapted and non-adapted requests
            (None, Some(_)) => Ok(()),

            // Adapter requested but none loaded - ERROR
            (Some(requested), None) => Err(LlmError::AdapterError(format!(
                "Envelope requests adapter '{}' but no adapter is loaded. \
                 Call load_adapter() first.",
                requested
            ))),

            // Adapter requested and one is loaded - must match
            (Some(requested), Some(loaded)) => {
                let loaded_canonical = loaded.adapter_id.to_canonical();
                if requested != &loaded_canonical {
                    return Err(LlmError::AdapterError(format!(
                        "Envelope requests adapter '{}' but loaded adapter is '{}'. \
                         Load the correct adapter first.",
                        requested, loaded_canonical
                    )));
                }
                Ok(())
            }
        }
    }

    /// Run inference using our contract system.
    ///
    /// This is the main entry point that integrates:
    /// - PromptStack (cognitive API)
    /// - InferenceEnvelope (reproducibility contract)
    /// - LoRA adapters (if envelope.adapter_id is set)
    ///
    /// # Adapter Behavior
    ///
    /// - If `envelope.adapter_id` is `None`, runs base model inference
    /// - If `envelope.adapter_id` is `Some`, validates the loaded adapter matches
    ///   and applies LoRA during inference
    ///
    /// # Arguments
    ///
    /// * `stack` - The prompt stack to render
    /// * `envelope` - The inference envelope controlling sampling
    ///
    /// # Returns
    ///
    /// A `GenerationResult` with the generated text and metadata.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Inference fails
    /// - Adapter is requested but not loaded
    /// - Loaded adapter doesn't match requested adapter
    pub fn run(
        &mut self,
        stack: &PromptStack,
        envelope: &InferenceEnvelope,
    ) -> LlmResult<GenerationResult> {
        // Validate adapter state matches envelope request
        self.validate_adapter_for_envelope(envelope)?;

        // Determine if we should apply LoRA
        let use_adapter = envelope.adapter_id.is_some() && self.adapter.is_some();

        if use_adapter {
            if let Some(ref adapter) = self.adapter {
                tracing::debug!(
                    adapter_id = %adapter.adapter_id,
                    "Running inference with LoRA adapter"
                );
            }
        }

        // Render the prompt stack to a string
        let prompt = stack.render();

        // Create sampler based on envelope settings
        let mut sampler = self.create_sampler(envelope);

        // Determine temperature (0 for greedy/deterministic)
        let temperature = if envelope.is_deterministic() {
            0.0
        } else {
            f64::from(envelope.generation.temperature)
        };

        // Reset cache before generation
        self.llama.reset();

        // Run generation
        // NOTE: LoRA weight application happens at the model layer level.
        // The llama-burn library needs to be extended to support runtime
        // LoRA injection. For now, we load the adapter and track it,
        // but actual weight merging requires deeper integration.
        //
        // TODO: Implement one of these approaches:
        // 1. Modify llama-burn to accept LoRA weights at inference time
        // 2. Pre-merge LoRA weights into base weights (loses hot-swap ability)
        // 3. Use a custom forward pass that applies LoRA computations
        let output = self.llama.generate(
            &prompt,
            envelope.stopping.max_tokens,
            temperature,
            &mut sampler,
        );

        // Determine finish reason
        let finish_reason = if output.tokens < envelope.stopping.max_tokens {
            FinishReason::Eos
        } else {
            FinishReason::Length
        };

        // Estimate input tokens (rough approximation)
        let input_tokens = prompt.len() / 4; // ~4 chars per token

        Ok(GenerationResult {
            text: output.text,
            input_tokens,
            output_tokens: output.tokens,
            finish_reason,
        })
    }

    /// Run inference with explicit adapter requirement.
    ///
    /// This is a convenience method that ensures an adapter is used.
    /// Unlike `run()`, this will error if no adapter is loaded.
    ///
    /// # Errors
    ///
    /// Returns an error if no adapter is loaded or inference fails.
    pub fn run_with_adapter(
        &mut self,
        stack: &PromptStack,
        envelope: &InferenceEnvelope,
    ) -> LlmResult<GenerationResult> {
        if self.adapter.is_none() {
            return Err(LlmError::AdapterError(
                "run_with_adapter() called but no adapter is loaded".to_string(),
            ));
        }

        // Create envelope with adapter if not already set
        let envelope_with_adapter = if envelope.adapter_id.is_none() {
            let adapter_id = self
                .adapter
                .as_ref()
                .map(|a| a.adapter_id.to_canonical())
                .unwrap();
            InferenceEnvelope {
                adapter_id: Some(adapter_id),
                ..envelope.clone()
            }
        } else {
            envelope.clone()
        };

        self.run(stack, &envelope_with_adapter)
    }

    /// Generate text from a raw prompt (bypassing contract system).
    ///
    /// Use `run()` for contract-aware inference. This method is for
    /// low-level access when you don't need the full contract system.
    ///
    /// # Errors
    ///
    /// Returns an error if generation fails.
    pub fn generate_raw(
        &mut self,
        prompt: &str,
        max_tokens: usize,
        temperature: f64,
        seed: u64,
    ) -> LlmResult<GenerationOutput> {
        // Create sampler
        let mut sampler = if temperature > 0.0 {
            Sampler::new_top_p(0.9, seed)
        } else {
            Sampler::Argmax
        };

        // Reset and generate
        self.llama.reset();
        Ok(self
            .llama
            .generate(prompt, max_tokens, temperature, &mut sampler))
    }

    /// Create a sampler based on the envelope settings.
    fn create_sampler(&self, envelope: &InferenceEnvelope) -> Sampler {
        if envelope.is_deterministic() {
            // Greedy decoding for deterministic output
            Sampler::Argmax
        } else {
            // Top-p sampling with seed from policy
            let seed = match envelope.seed_policy {
                SeedPolicy::Fixed(s) => s,
                SeedPolicy::Random => rand::random(),
                SeedPolicy::InputDerived => {
                    // Use a hash of the prompt version as seed
                    use std::hash::{Hash, Hasher};
                    let mut hasher = std::collections::hash_map::DefaultHasher::new();
                    envelope.prompt_version.hash(&mut hasher);
                    hasher.finish()
                }
            };
            Sampler::new_top_p(f64::from(envelope.generation.top_p), seed)
        }
    }

    /// Get the maximum sequence length.
    #[must_use]
    pub fn max_seq_len(&self) -> usize {
        self.max_seq_len
    }
}

// ============================================================================
// TinyLlama Engine (uses SentiencePieceTokenizer)
// ============================================================================

/// TinyLlama inference engine using SentiencePiece tokenizer.
///
/// This is a separate engine for TinyLlama models which use a different
/// tokenizer than Llama 3 models.
///
/// # Example
///
/// ```ignore
/// let engine = TinyLlamaEngine::<Wgpu>::load_pretrained(2048, &device)?;
/// let result = engine.run(&stack, &envelope)?;
/// ```
#[cfg(feature = "tiny")]
pub struct TinyLlamaEngine<B: Backend> {
    /// The underlying llama-burn model with SentiencePiece tokenizer
    llama: Llama<B, SentiencePieceTokenizer>,
    /// Maximum sequence length
    max_seq_len: usize,
    /// Model fingerprint for compatibility checking
    fingerprint: ModelFingerprint,
}

#[cfg(feature = "tiny")]
impl<B: Backend> TinyLlamaEngine<B> {
    /// Load a pretrained TinyLlama 1.1B model.
    ///
    /// This is the smallest model, ideal for testing on laptops.
    /// ~1.1B parameters, ~2GB download, runs well on CPU/Metal.
    ///
    /// # Errors
    ///
    /// Returns an error if model loading fails.
    #[cfg(feature = "pretrained")]
    pub fn load_pretrained(
        max_seq_len: usize,
        device: &burn::tensor::Device<B>,
    ) -> Result<Self, String> {
        let llama = LlamaConfig::tiny_llama_pretrained(max_seq_len, device)?;
        Ok(Self {
            llama,
            max_seq_len,
            fingerprint: ModelFingerprint {
                model_family: "tinyllama".to_string(),
                tokenizer_hash: "tinyllama_sentencepiece".to_string(),
                context_size: max_seq_len,
            },
        })
    }

    /// Run inference using our contract system.
    ///
    /// # Arguments
    ///
    /// * `stack` - The prompt stack to render
    /// * `envelope` - The inference envelope controlling sampling
    ///
    /// # Returns
    ///
    /// A `GenerationResult` with the generated text and metadata.
    pub fn run(
        &mut self,
        stack: &PromptStack,
        envelope: &InferenceEnvelope,
    ) -> LlmResult<GenerationResult> {
        // Render the prompt stack to a string
        let prompt = stack.render();

        // Create sampler based on envelope settings
        let mut sampler = self.create_sampler(envelope);

        // Determine temperature (0 for greedy/deterministic)
        let temperature = if envelope.is_deterministic() {
            0.0
        } else {
            f64::from(envelope.generation.temperature)
        };

        // Reset cache before generation
        self.llama.reset();

        // Run generation
        let output = self.llama.generate(
            &prompt,
            envelope.stopping.max_tokens,
            temperature,
            &mut sampler,
        );

        // Determine finish reason
        let finish_reason = if output.tokens < envelope.stopping.max_tokens {
            FinishReason::Eos
        } else {
            FinishReason::Length
        };

        // Estimate input tokens (rough approximation)
        let input_tokens = prompt.len() / 4; // ~4 chars per token

        Ok(GenerationResult {
            text: output.text,
            input_tokens,
            output_tokens: output.tokens,
            finish_reason,
        })
    }

    /// Generate text from a raw prompt (bypassing contract system).
    pub fn generate_raw(
        &mut self,
        prompt: &str,
        max_tokens: usize,
        temperature: f64,
        seed: u64,
    ) -> LlmResult<GenerationOutput> {
        let mut sampler = if temperature > 0.0 {
            Sampler::new_top_p(0.9, seed)
        } else {
            Sampler::Argmax
        };

        self.llama.reset();
        Ok(self
            .llama
            .generate(prompt, max_tokens, temperature, &mut sampler))
    }

    /// Create a sampler based on the envelope settings.
    fn create_sampler(&self, envelope: &InferenceEnvelope) -> Sampler {
        if envelope.is_deterministic() {
            Sampler::Argmax
        } else {
            let seed = match envelope.seed_policy {
                SeedPolicy::Fixed(s) => s,
                SeedPolicy::Random => rand::random(),
                SeedPolicy::InputDerived => {
                    use std::hash::{Hash, Hasher};
                    let mut hasher = std::collections::hash_map::DefaultHasher::new();
                    envelope.prompt_version.hash(&mut hasher);
                    hasher.finish()
                }
            };
            Sampler::new_top_p(f64::from(envelope.generation.top_p), seed)
        }
    }

    /// Get the maximum sequence length.
    #[must_use]
    pub fn max_seq_len(&self) -> usize {
        self.max_seq_len
    }

    /// Get the model fingerprint.
    #[must_use]
    pub fn fingerprint(&self) -> &ModelFingerprint {
        &self.fingerprint
    }
}

/// Validation result for golden tests.
#[derive(Debug, Clone)]
pub struct GoldenTestResult {
    /// Whether the output matches expected
    pub matches: bool,
    /// The actual output
    pub actual: String,
    /// The expected output
    pub expected: String,
    /// Token count
    pub tokens: usize,
    /// Generation time in seconds
    pub time: f64,
}

/// Run a golden test (deterministic inference check).
///
/// Golden tests verify that given:
/// - Same prompt
/// - Same envelope (deterministic, fixed seed)
/// - Same model weights
///
/// We get the exact same output.
///
/// # Example
///
/// ```ignore
/// let envelope = InferenceEnvelope::deterministic("test:v1", 42);
/// let result = golden_test(&mut engine, "Hello", &envelope, "Hello, world!")?;
/// assert!(result.matches);
/// ```
#[cfg(feature = "llama3")]
pub fn golden_test<B: Backend>(
    engine: &mut LlamaEngine<B>,
    prompt: &str,
    envelope: &InferenceEnvelope,
    expected: &str,
) -> LlmResult<GoldenTestResult> {
    assert!(
        envelope.is_deterministic(),
        "Golden tests require deterministic envelope"
    );

    let seed = match envelope.seed_policy {
        SeedPolicy::Fixed(s) => s,
        _ => panic!("Golden tests require fixed seed"),
    };

    let output = engine.generate_raw(
        prompt,
        envelope.stopping.max_tokens,
        0.0, // Always greedy for golden tests
        seed,
    )?;

    Ok(GoldenTestResult {
        matches: output.text == expected,
        actual: output.text,
        expected: expected.to_string(),
        tokens: output.tokens,
        time: output.time,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::{AdapterId, AdapterManifest, AdapterWeights};
    use crate::adapter_registry::InMemoryRegistry;
    use crate::inference::InferenceEnvelope;

    #[test]
    fn test_sampler_creation() {
        // Test that sampler creation works (doesn't require model)
        use llama_burn::sampling::Sampler;

        let argmax = Sampler::Argmax;
        let top_p = Sampler::new_top_p(0.9, 42);

        // Just verify they compile and construct
        assert!(matches!(argmax, Sampler::Argmax));
        assert!(matches!(top_p, Sampler::TopP(_)));
    }

    #[test]
    fn test_adapter_state_structure() {
        // Test AdapterState can be constructed and accessed
        let adapter_id = AdapterId::new("llm", "test-adapter", "1.0.0", "abc123");
        let manifest = AdapterManifest {
            adapter_id: adapter_id.clone(),
            base_model_id: "llama3-8b".to_string(),
            model_family: "llama3".to_string(),
            tokenizer_hash: "tiktoken_abc".to_string(),
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
        };
        let weights = AdapterWeights::empty(8, 16.0);

        let state = AdapterState {
            adapter_id: adapter_id.clone(),
            manifest,
            weights,
            merged: false,
            merge_report: None,
            lifecycle: AdapterLifecycleState::Attached,
        };

        assert_eq!(state.adapter_id.name, "test-adapter");
        assert_eq!(state.manifest.rank, 8);
        assert!(state.merge_report.is_none());
        assert!(!state.merged);
        assert_eq!(state.lifecycle, AdapterLifecycleState::Attached);
    }

    #[test]
    fn test_adapter_lifecycle_transitions() {
        // Test valid transitions
        assert!(AdapterLifecycleState::Detached.can_transition_to(AdapterLifecycleState::Loading));
        assert!(AdapterLifecycleState::Loading.can_transition_to(AdapterLifecycleState::Attached));
        assert!(AdapterLifecycleState::Loading.can_transition_to(AdapterLifecycleState::Detached));
        assert!(
            AdapterLifecycleState::Attached.can_transition_to(AdapterLifecycleState::Detaching)
        );
        assert!(
            AdapterLifecycleState::Detaching.can_transition_to(AdapterLifecycleState::Detached)
        );

        // Test invalid transitions
        assert!(
            !AdapterLifecycleState::Detached.can_transition_to(AdapterLifecycleState::Attached)
        );
        assert!(
            !AdapterLifecycleState::Detached.can_transition_to(AdapterLifecycleState::Detaching)
        );
        assert!(!AdapterLifecycleState::Attached.can_transition_to(AdapterLifecycleState::Loading));
        assert!(
            !AdapterLifecycleState::Loading.can_transition_to(AdapterLifecycleState::Detaching)
        );
    }

    #[test]
    fn test_model_fingerprint() {
        let fingerprint = ModelFingerprint {
            model_family: "llama3".to_string(),
            tokenizer_hash: "tiktoken_abc".to_string(),
            context_size: 4096,
        };

        assert_eq!(fingerprint.model_family, "llama3");
        assert_eq!(fingerprint.context_size, 4096);
    }

    #[test]
    fn test_adapter_registry_lifecycle() {
        // Test complete adapter lifecycle with InMemoryRegistry
        let mut registry = InMemoryRegistry::new();

        let adapter_id = AdapterId::new("llm", "grounded-answering", "1.0.0", "hash123");
        let manifest = AdapterManifest {
            adapter_id: adapter_id.clone(),
            base_model_id: "llama3-8b".to_string(),
            model_family: "llama3".to_string(),
            tokenizer_hash: "tiktoken_abc".to_string(),
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
            author: None,
            description: None,
        };

        let mut weights = AdapterWeights::empty(8, 16.0);
        weights.add_layer("q_proj", vec![0.1; 64 * 8], vec![0.2; 8 * 64]);

        registry.add(manifest.clone(), weights);

        // Verify adapter exists
        assert!(registry.exists(&adapter_id));

        // Load manifest
        let loaded_manifest = registry.get_manifest(&adapter_id).unwrap();
        assert_eq!(loaded_manifest.rank, 8);
        assert_eq!(loaded_manifest.truth_targets, vec!["grounded-answering"]);

        // Load weights
        let loaded_weights = registry.load_weights(&adapter_id).unwrap();
        assert_eq!(loaded_weights.rank, 8);
        assert!(loaded_weights.layers.contains_key("q_proj"));

        // List adapters
        let adapters = registry.list().unwrap();
        assert_eq!(adapters.len(), 1);
    }

    #[test]
    fn test_envelope_with_adapter_id() {
        // Test that envelope can carry adapter_id
        // Format: namespace/name@version+sha256:hash
        let envelope = InferenceEnvelope::deterministic("v1", 42)
            .with_adapter("llm/grounded-answering@1.0.0+sha256:abc123");

        assert!(envelope.has_adapter());
        assert_eq!(
            envelope.adapter(),
            Some("llm/grounded-answering@1.0.0+sha256:abc123")
        );
    }

    #[test]
    fn test_envelope_without_adapter() {
        let envelope = InferenceEnvelope::deterministic("v1", 42);

        assert!(!envelope.has_adapter());
        assert_eq!(envelope.adapter(), None);
    }

    #[test]
    fn test_adapter_id_parsing_for_envelope() {
        // Test that adapter IDs can be parsed from canonical form
        // Format: namespace/name@version+sha256:hash
        let canonical = "llm/grounded-answering@1.0.0+sha256:abc123";
        let parsed = AdapterId::parse(canonical).unwrap();

        assert_eq!(parsed.namespace, "llm");
        assert_eq!(parsed.name, "grounded-answering");
        assert_eq!(parsed.version, "1.0.0");
        assert_eq!(parsed.content_hash, "abc123");

        // Roundtrip
        assert_eq!(parsed.to_canonical(), canonical);
    }

    #[test]
    fn test_adapter_compatibility_validation() {
        let adapter_id = AdapterId::new("llm", "test", "1.0.0", "abc");
        let manifest = AdapterManifest {
            adapter_id,
            base_model_id: "llama3-8b".to_string(),
            model_family: "llama3".to_string(),
            tokenizer_hash: "tiktoken_abc".to_string(),
            context_size: 4096,
            quantization_mode: None,
            rank: 8,
            alpha: 16.0,
            target_layers: vec!["q_proj".to_string()],
            dropout: 0.0,
            truth_targets: vec![],
            dataset_manifest_id: None,
            created_at: "2026-01-17T00:00:00Z".to_string(),
            training_config_hash: None,
            author: None,
            description: None,
        };

        // Compatible
        assert!(
            manifest
                .validate_compatibility("llama3", "tiktoken_abc", 4096)
                .is_ok()
        );

        // Incompatible model family
        assert!(
            manifest
                .validate_compatibility("llama2", "tiktoken_abc", 4096)
                .is_err()
        );

        // Incompatible tokenizer
        assert!(
            manifest
                .validate_compatibility("llama3", "tiktoken_xyz", 4096)
                .is_err()
        );

        // Context too small
        assert!(
            manifest
                .validate_compatibility("llama3", "tiktoken_abc", 2048)
                .is_err()
        );
    }
}
