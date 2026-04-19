// Copyright 2024-2026 Reflective Labs

//! LoRA adapter lifecycle management.
//!
//! This module provides types and traits for managing LoRA adapters in converge-llm.
//!
//! # converge-core Axiom Compliance
//!
//! - **Explicit Authority**: `adapter_id` is explicit in requests, never implicit
//! - **No Hidden Work**: Adapter loading is synchronous and traceable
//! - **Safety by Construction**: Invalid adapter states are unrepresentable
//! - **Transparent Determinism**: Same adapter + seed = same output
//!
//! # Key Types
//!
//! - [`AdapterId`] - Unique identifier for an adapter (semantic versioning)
//! - [`AdapterManifest`] - Metadata describing an adapter's compatibility and purpose
//! - [`AdapterRegistry`] - Trait for adapter storage backends
//! - [`AdapterPolicy`] - Rules for which adapters are allowed
//!
//! # Governance Types
//!
//! Adapter lifecycle governance uses the portable types from converge-core:
//! - [`GovernedArtifactState`] - Lifecycle state machine (Draft → Approved → Active → ...)
//! - [`LifecycleEvent`] - Audit trail for state transitions
//! - [`RollbackSeverity`], [`RollbackImpact`] - Impact assessment

use crate::error::{LlmError, LlmResult};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// Re-export governance types from converge-core for backward compatibility
// These are the portable, capability-agnostic governance semantics
pub use converge_core::governed_artifact::{
    GovernedArtifactState, InvalidStateTransition, LifecycleEvent, RollbackImpact,
    RollbackSeverity, validate_transition,
};

// ============================================================================
// Adapter-Specific Types (Extensions to Core Governance)
// ============================================================================

/// Adapter-specific rollback record with merge context.
///
/// Extends the core RollbackRecord concept with adapter-specific fields
/// like merge hash and adapter ID.
///
/// Use this instead of the generic `RollbackRecord` from converge-core when
/// you need to track adapter-specific context like merge hashes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdapterRollbackRecord {
    /// Adapter that was rolled back
    pub adapter_id: AdapterId,
    /// Previous state before rollback
    pub previous_state: GovernedArtifactState,
    /// ISO 8601 timestamp of rollback
    pub rolled_back_at: String,
    /// Who initiated the rollback
    pub actor: String,
    /// Detailed reason for rollback
    pub reason: String,
    /// Impact assessment (from converge-core)
    pub impact: RollbackImpact,
    /// Adapter-specific: Merge artifact hash that was active at rollback time
    pub active_merge_hash: Option<String>,
    /// Optional: Link to incident ticket
    pub incident_ref: Option<String>,
}

/// Adapter-specific lifecycle event wrapper.
///
/// Wraps the core LifecycleEvent. For new code, consider using
/// core LifecycleEvent directly with GovernedArtifactState.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdapterLifecycleEvent {
    /// Core lifecycle event data
    #[serde(flatten)]
    pub core: LifecycleEvent,
}

/// Lifecycle-aware adapter record.
///
/// Wraps an AdapterManifest with lifecycle state and event history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdapterRecord {
    /// The adapter manifest
    pub manifest: AdapterManifest,
    /// Current lifecycle state (uses converge-core GovernedArtifactState)
    pub state: GovernedArtifactState,
    /// History of lifecycle events (uses converge-core LifecycleEvent)
    pub lifecycle_events: Vec<LifecycleEvent>,
    /// Adapter-specific rollback record if adapter was rolled back
    pub rollback: Option<AdapterRollbackRecord>,
}

impl AdapterRecord {
    /// Create a new adapter record in Draft state.
    pub fn new(manifest: AdapterManifest) -> Self {
        Self {
            manifest,
            state: GovernedArtifactState::Draft,
            lifecycle_events: Vec::new(),
            rollback: None,
        }
    }

    /// Get the adapter ID.
    #[must_use]
    pub fn adapter_id(&self) -> &AdapterId {
        &self.manifest.adapter_id
    }

    /// Transition to a new state with audit trail.
    ///
    /// # Errors
    ///
    /// Returns an error if the transition is invalid.
    pub fn transition(
        &mut self,
        to: GovernedArtifactState,
        actor: impl Into<String>,
        reason: impl Into<String>,
    ) -> Result<(), InvalidStateTransition> {
        validate_transition(self.state, to)?;

        let event = LifecycleEvent::new(self.state, to, actor, reason);
        self.lifecycle_events.push(event);
        self.state = to;

        Ok(())
    }

    /// Approve the adapter for production use.
    ///
    /// # Errors
    ///
    /// Returns an error if the adapter is not in Draft state.
    pub fn approve(
        &mut self,
        actor: impl Into<String>,
        reason: impl Into<String>,
    ) -> Result<(), InvalidStateTransition> {
        self.transition(GovernedArtifactState::Approved, actor, reason)
    }

    /// Activate the adapter (deploy to production).
    ///
    /// # Errors
    ///
    /// Returns an error if the adapter is not in Approved state.
    pub fn activate(
        &mut self,
        actor: impl Into<String>,
        reason: impl Into<String>,
    ) -> Result<(), InvalidStateTransition> {
        self.transition(GovernedArtifactState::Active, actor, reason)
    }

    /// Deprecate the adapter (superseded by newer version).
    ///
    /// # Errors
    ///
    /// Returns an error if the adapter is not in Active state.
    pub fn deprecate(
        &mut self,
        actor: impl Into<String>,
        reason: impl Into<String>,
    ) -> Result<(), InvalidStateTransition> {
        self.transition(GovernedArtifactState::Deprecated, actor, reason)
    }

    /// Roll back the adapter due to issues.
    ///
    /// This is a more complex operation that captures full rollback context.
    ///
    /// # Errors
    ///
    /// Returns an error if the adapter is in a state that cannot be rolled back.
    pub fn rollback(
        &mut self,
        actor: impl Into<String>,
        reason: impl Into<String>,
        impact: RollbackImpact,
        active_merge_hash: Option<String>,
    ) -> Result<(), InvalidStateTransition> {
        let actor_str = actor.into();
        let reason_str = reason.into();

        // Validate the transition first
        validate_transition(self.state, GovernedArtifactState::RolledBack)?;

        // Create rollback record
        self.rollback = Some(AdapterRollbackRecord {
            adapter_id: self.manifest.adapter_id.clone(),
            previous_state: self.state,
            rolled_back_at: chrono::Utc::now().to_rfc3339(),
            actor: actor_str.clone(),
            reason: reason_str.clone(),
            impact,
            active_merge_hash,
            incident_ref: None,
        });

        // Record the transition
        let event = LifecycleEvent::new(
            self.state,
            GovernedArtifactState::RolledBack,
            actor_str,
            reason_str,
        );
        self.lifecycle_events.push(event);
        self.state = GovernedArtifactState::RolledBack;

        Ok(())
    }

    /// Check if this adapter can be used in production.
    #[must_use]
    pub fn can_use_in_production(&self) -> bool {
        self.state.allows_production_use()
    }

    /// Check if this adapter was rolled back.
    #[must_use]
    pub fn was_rolled_back(&self) -> bool {
        self.rollback.is_some()
    }

    /// Get the rollback reason if rolled back.
    #[must_use]
    pub fn rollback_reason(&self) -> Option<&str> {
        self.rollback.as_ref().map(|r| r.reason.as_str())
    }
}

/// Unique identifier for a LoRA adapter.
///
/// Format: `{namespace}/{name}@{version}+{content_hash}`
/// Example: `llm/grounded-answering@1.0.0+sha256:abc123...`
///
/// The content hash ensures that two adapters with the same version
/// but different weights are distinguishable.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AdapterId {
    /// Namespace (e.g., "llm", "domain")
    pub namespace: String,
    /// Adapter name (e.g., "grounded-answering")
    pub name: String,
    /// Semantic version
    pub version: String,
    /// Content hash of adapter weights (sha256, first 12 chars)
    pub content_hash: String,
}

impl AdapterId {
    /// Create a new adapter ID.
    #[must_use]
    pub fn new(
        namespace: impl Into<String>,
        name: impl Into<String>,
        version: impl Into<String>,
        content_hash: impl Into<String>,
    ) -> Self {
        Self {
            namespace: namespace.into(),
            name: name.into(),
            version: version.into(),
            content_hash: content_hash.into(),
        }
    }

    /// Parse an adapter ID from string format.
    ///
    /// Format: `{namespace}/{name}@{version}+sha256:{hash}`
    ///
    /// # Errors
    ///
    /// Returns an error if the format is invalid.
    pub fn parse(s: &str) -> LlmResult<Self> {
        // Split by '@' to get name part and version+hash
        let (name_part, version_hash) = s
            .split_once('@')
            .ok_or_else(|| LlmError::AdapterError("Missing '@' in adapter ID".to_string()))?;

        // Split name_part by '/' to get namespace and name
        let (namespace, name) = name_part
            .split_once('/')
            .ok_or_else(|| LlmError::AdapterError("Missing '/' in adapter ID".to_string()))?;

        // Split version_hash by '+sha256:' to get version and hash
        let (version, hash) = version_hash.split_once("+sha256:").ok_or_else(|| {
            LlmError::AdapterError("Missing '+sha256:' in adapter ID".to_string())
        })?;

        Ok(Self {
            namespace: namespace.to_string(),
            name: name.to_string(),
            version: version.to_string(),
            content_hash: hash.to_string(),
        })
    }

    /// Convert to canonical string format.
    #[must_use]
    pub fn to_canonical(&self) -> String {
        format!(
            "{}/{}@{}+sha256:{}",
            self.namespace, self.name, self.version, self.content_hash
        )
    }

    /// Get a short display form (name@version).
    #[must_use]
    pub fn short(&self) -> String {
        format!("{}@{}", self.name, self.version)
    }
}

impl std::fmt::Display for AdapterId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_canonical())
    }
}

/// Metadata describing a LoRA adapter.
///
/// The manifest captures everything needed to:
/// 1. Validate compatibility with a base model
/// 2. Understand the adapter's purpose and training data
/// 3. Reproduce the adapter's behavior
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdapterManifest {
    /// Unique identifier
    pub adapter_id: AdapterId,

    // --- Compatibility ---
    /// Base model this adapter was trained for
    pub base_model_id: String,
    /// Model family (e.g., "llama3")
    pub model_family: String,
    /// Hash of the tokenizer configuration
    pub tokenizer_hash: String,
    /// Maximum context size the adapter supports
    pub context_size: usize,
    /// Quantization mode (e.g., "none", "int8", "int4")
    pub quantization_mode: Option<String>,

    // --- LoRA Configuration ---
    /// LoRA rank (r)
    pub rank: usize,
    /// LoRA alpha scaling factor
    pub alpha: f32,
    /// Target layers (e.g., ["q_proj", "v_proj"])
    pub target_layers: Vec<String>,
    /// Dropout used during training
    pub dropout: f32,

    // --- Converge Metadata ---
    /// Truth IDs this adapter targets
    pub truth_targets: Vec<String>,
    /// Dataset manifest used for training
    pub dataset_manifest_id: Option<String>,
    /// Creation timestamp (ISO 8601)
    pub created_at: String,
    /// Training configuration hash
    pub training_config_hash: Option<String>,

    // --- Provenance ---
    /// Who/what created this adapter
    pub author: Option<String>,
    /// Description
    pub description: Option<String>,
}

impl AdapterManifest {
    /// Validate that this adapter is compatible with a given model configuration.
    ///
    /// This performs basic compatibility checks. For production use, consider
    /// `validate_compatibility_strict` which also verifies quantization and
    /// base model hash.
    ///
    /// # Errors
    ///
    /// Returns an error describing the incompatibility.
    pub fn validate_compatibility(
        &self,
        model_family: &str,
        tokenizer_hash: &str,
        context_size: usize,
    ) -> LlmResult<()> {
        // Model family must match
        if self.model_family != model_family {
            return Err(LlmError::AdapterIncompatible {
                reason: format!(
                    "model family mismatch: adapter requires '{}', got '{}'",
                    self.model_family, model_family
                ),
            });
        }

        // Tokenizer hash must match
        if self.tokenizer_hash != tokenizer_hash {
            return Err(LlmError::AdapterIncompatible {
                reason: format!(
                    "tokenizer mismatch: adapter hash '{}', model hash '{}'",
                    self.tokenizer_hash, tokenizer_hash
                ),
            });
        }

        // Context size: adapter must fit within model's context
        if self.context_size > context_size {
            return Err(LlmError::AdapterIncompatible {
                reason: format!(
                    "context size mismatch: adapter requires {}, model supports {}",
                    self.context_size, context_size
                ),
            });
        }

        Ok(())
    }

    /// Strict compatibility validation including quantization and base model hash.
    ///
    /// This is the recommended validation for production use. It ensures:
    /// - Model family matches
    /// - Tokenizer hash matches
    /// - Context size is sufficient
    /// - Quantization mode is compatible
    /// - Base model hash matches (if provided)
    ///
    /// # Arguments
    ///
    /// * `model_family` - The model family (e.g., "llama3")
    /// * `tokenizer_hash` - Hash of the tokenizer configuration
    /// * `context_size` - Maximum context size the model supports
    /// * `quantization_mode` - Current model quantization (None = full precision)
    /// * `base_model_hash` - Optional hash of the base model weights
    ///
    /// # Errors
    ///
    /// Returns an error describing the incompatibility.
    pub fn validate_compatibility_strict(
        &self,
        model_family: &str,
        tokenizer_hash: &str,
        context_size: usize,
        quantization_mode: Option<&str>,
        base_model_hash: Option<&str>,
    ) -> LlmResult<()> {
        // First, run basic validation
        self.validate_compatibility(model_family, tokenizer_hash, context_size)?;

        // Quantization compatibility check
        // Rules:
        // - Adapter trained on full precision (None) can be applied to full precision model
        // - Adapter trained on quantized model must match that quantization
        // - Adapter trained on full precision CAN be applied to quantized model (with warning)
        // - Adapter trained on INT8 CANNOT be applied to INT4 model (or vice versa)
        match (&self.quantization_mode, quantization_mode) {
            // Both full precision - OK
            (None, None) => {}

            // Adapter full precision, model quantized - Warning but OK
            // (The adapter deltas will be applied in the quantized space)
            (None, Some(_)) => {
                tracing::warn!(
                    adapter_id = %self.adapter_id,
                    model_quant = quantization_mode,
                    "Applying full-precision adapter to quantized model - quality may degrade"
                );
            }

            // Adapter quantized, model full precision - Error
            // (The adapter was trained in a different weight space)
            (Some(adapter_quant), None) => {
                return Err(LlmError::AdapterIncompatible {
                    reason: format!(
                        "quantization mismatch: adapter trained on {} cannot be applied to full-precision model",
                        adapter_quant
                    ),
                });
            }

            // Both quantized - must match exactly
            (Some(adapter_quant), Some(model_quant)) => {
                if adapter_quant != model_quant {
                    return Err(LlmError::AdapterIncompatible {
                        reason: format!(
                            "quantization mismatch: adapter trained on '{}', model is '{}'",
                            adapter_quant, model_quant
                        ),
                    });
                }
            }
        }

        // Base model hash check (if provided)
        if let Some(expected_hash) = base_model_hash {
            // Extract the hash from base_model_id if it contains one
            // Format could be "llama3-8b" or "llama3-8b+sha256:abc123"
            if let Some(adapter_base_hash) = self.base_model_id.split("+sha256:").nth(1) {
                if adapter_base_hash != expected_hash {
                    return Err(LlmError::AdapterIncompatible {
                        reason: format!(
                            "base model hash mismatch: adapter trained on '{}', got '{}'",
                            adapter_base_hash, expected_hash
                        ),
                    });
                }
            }
            // If adapter doesn't have a hash, we can't verify - that's OK
        }

        Ok(())
    }

    /// Validate layer shapes against expected dimensions.
    ///
    /// This checks that the adapter's LoRA matrices have shapes compatible
    /// with the target model layers.
    ///
    /// # Arguments
    ///
    /// * `weights` - The adapter weights to validate
    /// * `expected_shapes` - Map of layer name -> (input_dim, output_dim)
    ///
    /// # Errors
    ///
    /// Returns an error if any layer shape mismatches.
    pub fn validate_layer_shapes(
        &self,
        weights: &AdapterWeights,
        expected_shapes: &std::collections::HashMap<String, (usize, usize)>,
    ) -> LlmResult<()> {
        for layer_name in &self.target_layers {
            // Check if we have weights for this layer
            if let Some((lora_a, lora_b)) = weights.layers.get(layer_name) {
                // Check if we have expected shapes
                if let Some(&(expected_in, expected_out)) = expected_shapes.get(layer_name) {
                    // LoRA A: input_dim x rank
                    // LoRA B: rank x output_dim
                    let actual_in = lora_a.len() / self.rank;
                    let actual_out = lora_b.len() / self.rank;

                    if actual_in != expected_in {
                        return Err(LlmError::AdapterIncompatible {
                            reason: format!(
                                "layer '{}' input dimension mismatch: expected {}, got {}",
                                layer_name, expected_in, actual_in
                            ),
                        });
                    }

                    if actual_out != expected_out {
                        return Err(LlmError::AdapterIncompatible {
                            reason: format!(
                                "layer '{}' output dimension mismatch: expected {}, got {}",
                                layer_name, expected_out, actual_out
                            ),
                        });
                    }
                }
            }
        }

        Ok(())
    }

    /// Check if this adapter targets a specific Truth.
    #[must_use]
    pub fn targets_truth(&self, truth_id: &str) -> bool {
        self.truth_targets.iter().any(|t| t == truth_id)
    }
}

/// LoRA adapter weights.
///
/// These are the trainable parameters injected into the model.
/// The base model weights remain frozen.
#[derive(Debug, Clone)]
pub struct AdapterWeights {
    /// Layer name -> (A matrix, B matrix)
    /// A: input_dim x rank
    /// B: rank x output_dim
    pub layers: HashMap<String, (Vec<f32>, Vec<f32>)>,
    /// LoRA rank
    pub rank: usize,
    /// LoRA alpha
    pub alpha: f32,
}

impl AdapterWeights {
    /// Create empty adapter weights.
    #[must_use]
    pub fn empty(rank: usize, alpha: f32) -> Self {
        Self {
            layers: HashMap::new(),
            rank,
            alpha,
        }
    }

    /// Add weights for a layer.
    pub fn add_layer(&mut self, name: impl Into<String>, a: Vec<f32>, b: Vec<f32>) {
        self.layers.insert(name.into(), (a, b));
    }

    /// Get the scaling factor for LoRA.
    #[must_use]
    pub fn scale(&self) -> f32 {
        self.alpha / self.rank as f32
    }
}

/// Trait for adapter storage backends.
///
/// Implementations handle fetching and storing adapter artifacts.
/// The filesystem backend is provided; other backends (S3, GCS, etc.)
/// can be implemented for production use.
pub trait AdapterRegistry: Send + Sync {
    /// Get the manifest for an adapter.
    ///
    /// # Errors
    ///
    /// Returns an error if the adapter is not found or cannot be read.
    fn get_manifest(&self, id: &AdapterId) -> LlmResult<AdapterManifest>;

    /// Load adapter weights.
    ///
    /// # Errors
    ///
    /// Returns an error if the weights cannot be loaded.
    fn load_weights(&self, id: &AdapterId) -> LlmResult<AdapterWeights>;

    /// Check if an adapter exists.
    fn exists(&self, id: &AdapterId) -> bool;

    /// List all available adapters.
    ///
    /// # Errors
    ///
    /// Returns an error if the registry cannot be enumerated.
    fn list(&self) -> LlmResult<Vec<AdapterId>>;

    /// Validate compatibility with a model configuration.
    ///
    /// # Errors
    ///
    /// Returns an error describing any incompatibility.
    fn validate_compatibility(
        &self,
        id: &AdapterId,
        model_family: &str,
        tokenizer_hash: &str,
        context_size: usize,
    ) -> LlmResult<()> {
        let manifest = self.get_manifest(id)?;
        manifest.validate_compatibility(model_family, tokenizer_hash, context_size)
    }
}

/// Policy for adapter usage.
///
/// Controls which adapters are allowed for a given context.
/// This is NOT a default adapter selector (that would violate Explicit Authority).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AdapterPolicy {
    /// Adapters that are explicitly allowed (empty = all allowed)
    pub allowed: Vec<AdapterId>,
    /// Adapters that are explicitly blocked
    pub blocked: Vec<AdapterId>,
    /// Require adapters to be signed
    pub require_signed: bool,
    /// Per-Truth adapter preferences (for documentation, not auto-selection)
    pub truth_preferences: HashMap<String, AdapterId>,
}

impl AdapterPolicy {
    /// Create an empty (permissive) policy.
    #[must_use]
    pub fn permissive() -> Self {
        Self::default()
    }

    /// Create a restrictive policy that only allows specific adapters.
    #[must_use]
    pub fn allow_only(adapters: Vec<AdapterId>) -> Self {
        Self {
            allowed: adapters,
            ..Default::default()
        }
    }

    /// Check if an adapter is allowed by this policy.
    #[must_use]
    pub fn is_allowed(&self, id: &AdapterId) -> bool {
        // Check blocked list first
        if self.blocked.contains(id) {
            return false;
        }

        // If allowed list is empty, allow all (not blocked)
        if self.allowed.is_empty() {
            return true;
        }

        // Otherwise, must be in allowed list
        self.allowed.contains(id)
    }

    /// Get the preferred adapter for a Truth (informational only).
    ///
    /// Note: This does NOT auto-select adapters. The caller must explicitly
    /// choose to use this preference.
    #[must_use]
    pub fn preferred_for_truth(&self, truth_id: &str) -> Option<&AdapterId> {
        self.truth_preferences.get(truth_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_adapter_id_parse() {
        let id = AdapterId::parse("llm/grounded-answering@1.0.0+sha256:abc123").unwrap();
        assert_eq!(id.namespace, "llm");
        assert_eq!(id.name, "grounded-answering");
        assert_eq!(id.version, "1.0.0");
        assert_eq!(id.content_hash, "abc123");
    }

    #[test]
    fn test_adapter_id_canonical() {
        let id = AdapterId::new("llm", "test", "1.0.0", "abc123");
        assert_eq!(id.to_canonical(), "llm/test@1.0.0+sha256:abc123");
    }

    #[test]
    fn test_adapter_id_roundtrip() {
        let original = "llm/grounded-answering@2.1.0+sha256:deadbeef";
        let id = AdapterId::parse(original).unwrap();
        assert_eq!(id.to_canonical(), original);
    }

    #[test]
    fn test_manifest_compatibility_ok() {
        let manifest = AdapterManifest {
            adapter_id: AdapterId::new("llm", "test", "1.0.0", "abc"),
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
            author: None,
            description: None,
        };

        // Compatible
        assert!(
            manifest
                .validate_compatibility("llama3", "tok123", 8192)
                .is_ok()
        );

        // Model family mismatch
        assert!(
            manifest
                .validate_compatibility("llama2", "tok123", 8192)
                .is_err()
        );

        // Tokenizer mismatch
        assert!(
            manifest
                .validate_compatibility("llama3", "different", 8192)
                .is_err()
        );

        // Context too small
        assert!(
            manifest
                .validate_compatibility("llama3", "tok123", 2048)
                .is_err()
        );
    }

    #[test]
    fn test_adapter_policy() {
        let allowed_id = AdapterId::new("llm", "allowed", "1.0.0", "aaa");
        let blocked_id = AdapterId::new("llm", "blocked", "1.0.0", "bbb");
        let other_id = AdapterId::new("llm", "other", "1.0.0", "ccc");

        let policy = AdapterPolicy {
            allowed: vec![allowed_id.clone()],
            blocked: vec![blocked_id.clone()],
            ..Default::default()
        };

        assert!(policy.is_allowed(&allowed_id));
        assert!(!policy.is_allowed(&blocked_id));
        assert!(!policy.is_allowed(&other_id)); // Not in allowed list

        // Permissive policy
        let permissive = AdapterPolicy::permissive();
        assert!(permissive.is_allowed(&other_id));
    }

    #[test]
    fn test_adapter_weights_scale() {
        let weights = AdapterWeights::empty(8, 16.0);
        assert_eq!(weights.scale(), 2.0); // 16.0 / 8 = 2.0
    }

    #[test]
    fn test_strict_compatibility_quantization() {
        let manifest = AdapterManifest {
            adapter_id: AdapterId::new("llm", "test", "1.0.0", "abc"),
            base_model_id: "llama3-8b".to_string(),
            model_family: "llama3".to_string(),
            tokenizer_hash: "tok123".to_string(),
            context_size: 4096,
            quantization_mode: None, // Full precision adapter
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

        // Full precision adapter on full precision model - OK
        assert!(
            manifest
                .validate_compatibility_strict("llama3", "tok123", 8192, None, None)
                .is_ok()
        );

        // Full precision adapter on quantized model - OK (with warning)
        assert!(
            manifest
                .validate_compatibility_strict("llama3", "tok123", 8192, Some("int8"), None)
                .is_ok()
        );

        // Now test quantized adapter
        let quant_manifest = AdapterManifest {
            quantization_mode: Some("int8".to_string()),
            ..manifest.clone()
        };

        // Quantized adapter on full precision model - ERROR
        assert!(
            quant_manifest
                .validate_compatibility_strict("llama3", "tok123", 8192, None, None)
                .is_err()
        );

        // Quantized adapter on matching quantized model - OK
        assert!(
            quant_manifest
                .validate_compatibility_strict("llama3", "tok123", 8192, Some("int8"), None)
                .is_ok()
        );

        // Quantized adapter on different quantized model - ERROR
        assert!(
            quant_manifest
                .validate_compatibility_strict("llama3", "tok123", 8192, Some("int4"), None)
                .is_err()
        );
    }

    #[test]
    fn test_strict_compatibility_base_hash() {
        let manifest = AdapterManifest {
            adapter_id: AdapterId::new("llm", "test", "1.0.0", "abc"),
            base_model_id: "llama3-8b+sha256:expected123".to_string(),
            model_family: "llama3".to_string(),
            tokenizer_hash: "tok123".to_string(),
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

        // Matching base model hash - OK
        assert!(
            manifest
                .validate_compatibility_strict("llama3", "tok123", 8192, None, Some("expected123"))
                .is_ok()
        );

        // Mismatched base model hash - ERROR
        assert!(
            manifest
                .validate_compatibility_strict("llama3", "tok123", 8192, None, Some("different456"))
                .is_err()
        );

        // No base model hash provided - OK (can't verify)
        assert!(
            manifest
                .validate_compatibility_strict("llama3", "tok123", 8192, None, None)
                .is_ok()
        );
    }

    #[test]
    fn test_layer_shape_validation() {
        let manifest = AdapterManifest {
            adapter_id: AdapterId::new("llm", "test", "1.0.0", "abc"),
            base_model_id: "llama3-8b".to_string(),
            model_family: "llama3".to_string(),
            tokenizer_hash: "tok123".to_string(),
            context_size: 4096,
            quantization_mode: None,
            rank: 8,
            alpha: 16.0,
            target_layers: vec!["q_proj".to_string(), "v_proj".to_string()],
            dropout: 0.0,
            truth_targets: vec![],
            dataset_manifest_id: None,
            created_at: "2026-01-17T00:00:00Z".to_string(),
            training_config_hash: None,
            author: None,
            description: None,
        };

        // Create weights with correct shapes
        // rank=8, so:
        // lora_a: 64 * 8 = 512 elements (input_dim=64)
        // lora_b: 8 * 64 = 512 elements (output_dim=64)
        let mut weights = AdapterWeights::empty(8, 16.0);
        weights.add_layer("q_proj", vec![0.1; 512], vec![0.2; 512]);
        weights.add_layer("v_proj", vec![0.1; 512], vec![0.2; 512]);

        let mut expected_shapes = std::collections::HashMap::new();
        expected_shapes.insert("q_proj".to_string(), (64, 64));
        expected_shapes.insert("v_proj".to_string(), (64, 64));

        // Matching shapes - OK
        assert!(
            manifest
                .validate_layer_shapes(&weights, &expected_shapes)
                .is_ok()
        );

        // Wrong expected shape
        let mut wrong_shapes = std::collections::HashMap::new();
        wrong_shapes.insert("q_proj".to_string(), (128, 64)); // Wrong input dim

        assert!(
            manifest
                .validate_layer_shapes(&weights, &wrong_shapes)
                .is_err()
        );
    }

    // ========================================================================
    // Lifecycle State Tests
    // ========================================================================

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
            truth_targets: vec![],
            dataset_manifest_id: None,
            created_at: "2026-01-19T00:00:00Z".to_string(),
            training_config_hash: None,
            author: None,
            description: None,
        }
    }

    #[test]
    fn test_lifecycle_state_default() {
        assert_eq!(
            GovernedArtifactState::default(),
            GovernedArtifactState::Draft
        );
    }

    #[test]
    fn test_lifecycle_state_allows_production() {
        assert!(!GovernedArtifactState::Draft.allows_production_use());
        assert!(GovernedArtifactState::Approved.allows_production_use());
        assert!(GovernedArtifactState::Active.allows_production_use());
        assert!(!GovernedArtifactState::Deprecated.allows_production_use());
        assert!(!GovernedArtifactState::RolledBack.allows_production_use());
    }

    #[test]
    fn test_lifecycle_state_is_terminal() {
        assert!(!GovernedArtifactState::Draft.is_terminal());
        assert!(!GovernedArtifactState::Approved.is_terminal());
        assert!(!GovernedArtifactState::Active.is_terminal());
        assert!(GovernedArtifactState::Deprecated.is_terminal());
        assert!(GovernedArtifactState::RolledBack.is_terminal());
    }

    #[test]
    fn test_lifecycle_valid_transitions() {
        // Draft → Approved
        assert!(
            validate_transition(
                GovernedArtifactState::Draft,
                GovernedArtifactState::Approved
            )
            .is_ok()
        );
        // Draft → Deprecated (abandoned)
        assert!(
            validate_transition(
                GovernedArtifactState::Draft,
                GovernedArtifactState::Deprecated
            )
            .is_ok()
        );
        // Approved → Active
        assert!(
            validate_transition(
                GovernedArtifactState::Approved,
                GovernedArtifactState::Active
            )
            .is_ok()
        );
        // Approved → RolledBack
        assert!(
            validate_transition(
                GovernedArtifactState::Approved,
                GovernedArtifactState::RolledBack
            )
            .is_ok()
        );
        // Active → Deprecated
        assert!(
            validate_transition(
                GovernedArtifactState::Active,
                GovernedArtifactState::Deprecated
            )
            .is_ok()
        );
        // Active → RolledBack
        assert!(
            validate_transition(
                GovernedArtifactState::Active,
                GovernedArtifactState::RolledBack
            )
            .is_ok()
        );
    }

    #[test]
    fn test_lifecycle_invalid_transitions() {
        // Cannot skip states
        assert!(
            validate_transition(GovernedArtifactState::Draft, GovernedArtifactState::Active)
                .is_err()
        );
        // Cannot go backwards
        assert!(
            validate_transition(
                GovernedArtifactState::Active,
                GovernedArtifactState::Approved
            )
            .is_err()
        );
        // Cannot transition from terminal states
        assert!(
            validate_transition(
                GovernedArtifactState::Deprecated,
                GovernedArtifactState::Active
            )
            .is_err()
        );
        assert!(
            validate_transition(
                GovernedArtifactState::RolledBack,
                GovernedArtifactState::Draft
            )
            .is_err()
        );
    }

    #[test]
    fn test_adapter_record_lifecycle() {
        let mut record = AdapterRecord::new(test_manifest());

        // Starts in Draft
        assert_eq!(record.state, GovernedArtifactState::Draft);
        assert!(!record.can_use_in_production());

        // Approve
        record
            .approve("reviewer@example.com", "Passed quality review")
            .unwrap();
        assert_eq!(record.state, GovernedArtifactState::Approved);
        assert!(record.can_use_in_production());
        assert_eq!(record.lifecycle_events.len(), 1);

        // Activate
        record
            .activate("deploy-system", "Production deployment v1.0")
            .unwrap();
        assert_eq!(record.state, GovernedArtifactState::Active);
        assert!(record.can_use_in_production());
        assert_eq!(record.lifecycle_events.len(), 2);

        // Deprecate
        record.deprecate("admin", "Superseded by v1.1").unwrap();
        assert_eq!(record.state, GovernedArtifactState::Deprecated);
        assert!(!record.can_use_in_production());
        assert_eq!(record.lifecycle_events.len(), 3);
    }

    #[test]
    fn test_adapter_record_rollback() {
        let mut record = AdapterRecord::new(test_manifest());

        // Progress to Active
        record.approve("reviewer", "Approved").unwrap();
        record.activate("system", "Deployed").unwrap();

        // Rollback with impact assessment
        let impact = RollbackImpact {
            affected_count: Some(1500),
            quality_issues: vec!["Incorrect grounding".to_string()],
            invalidates_outputs: true,
            severity: RollbackSeverity::High,
            affected_tenants: vec![],
        };

        record
            .rollback(
                "incident-commander@example.com",
                "Grounding failure detected in production",
                impact,
                Some("merge-hash-abc123".to_string()),
            )
            .unwrap();

        assert_eq!(record.state, GovernedArtifactState::RolledBack);
        assert!(record.was_rolled_back());
        assert_eq!(
            record.rollback_reason(),
            Some("Grounding failure detected in production")
        );

        // Verify rollback record details
        let rollback = record.rollback.as_ref().unwrap();
        assert_eq!(rollback.previous_state, GovernedArtifactState::Active);
        assert_eq!(rollback.impact.severity, RollbackSeverity::High);
        assert_eq!(
            rollback.active_merge_hash,
            Some("merge-hash-abc123".to_string())
        );
    }

    #[test]
    fn test_adapter_record_cannot_rollback_from_draft() {
        let mut record = AdapterRecord::new(test_manifest());

        let impact = RollbackImpact::default();
        let result = record.rollback("admin", "Testing", impact, None);

        assert!(result.is_err());
    }

    #[test]
    fn test_lifecycle_event_audit_trail() {
        let mut record = AdapterRecord::new(test_manifest());

        record
            .approve("alice@example.com", "Initial review complete")
            .unwrap();
        record
            .activate("deploy-bot", "Canary deployment successful")
            .unwrap();

        // Verify audit trail
        assert_eq!(record.lifecycle_events.len(), 2);

        let event1 = &record.lifecycle_events[0];
        assert_eq!(event1.from_state, GovernedArtifactState::Draft);
        assert_eq!(event1.to_state, GovernedArtifactState::Approved);
        assert_eq!(event1.actor, "alice@example.com");
        assert_eq!(event1.reason, "Initial review complete");

        let event2 = &record.lifecycle_events[1];
        assert_eq!(event2.from_state, GovernedArtifactState::Approved);
        assert_eq!(event2.to_state, GovernedArtifactState::Active);
        assert_eq!(event2.actor, "deploy-bot");
    }

    #[test]
    fn test_lifecycle_state_serialization_stable() {
        // Verify enum serialization is stable for audit trails
        let draft = serde_json::to_string(&GovernedArtifactState::Draft).unwrap();
        let approved = serde_json::to_string(&GovernedArtifactState::Approved).unwrap();
        let active = serde_json::to_string(&GovernedArtifactState::Active).unwrap();
        let deprecated = serde_json::to_string(&GovernedArtifactState::Deprecated).unwrap();
        let rolled_back = serde_json::to_string(&GovernedArtifactState::RolledBack).unwrap();

        assert_eq!(draft, "\"Draft\"");
        assert_eq!(approved, "\"Approved\"");
        assert_eq!(active, "\"Active\"");
        assert_eq!(deprecated, "\"Deprecated\"");
        assert_eq!(rolled_back, "\"RolledBack\"");
    }

    #[test]
    fn test_rollback_severity_serialization_stable() {
        let low = serde_json::to_string(&RollbackSeverity::Low).unwrap();
        let medium = serde_json::to_string(&RollbackSeverity::Medium).unwrap();
        let high = serde_json::to_string(&RollbackSeverity::High).unwrap();
        let critical = serde_json::to_string(&RollbackSeverity::Critical).unwrap();

        assert_eq!(low, "\"Low\"");
        assert_eq!(medium, "\"Medium\"");
        assert_eq!(high, "\"High\"");
        assert_eq!(critical, "\"Critical\"");
    }
}
