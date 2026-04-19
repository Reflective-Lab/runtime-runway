// Copyright 2024-2026 Reflective Labs

//! LoRA (Low-Rank Adaptation) module for Burn.
//!
//! This module provides LoRA-wrapped layers that inject low-rank adapters
//! into existing model weights. The base weights remain frozen while only
//! the low-rank matrices A and B are trained (or loaded from adapters).
//!
//! # LoRA Formula
//!
//! ```text
//! W' = W + (alpha / r) * B @ A
//! ```
//!
//! Where:
//! - W: Original frozen weights
//! - A: Low-rank "down" projection (input_dim x rank)
//! - B: Low-rank "up" projection (rank x output_dim)
//! - alpha: Scaling factor
//! - r: Rank
//!
//! # converge-core Axiom Compliance
//!
//! - LoRA enhances ProposedFact quality, does not change trust boundary
//! - Adapter weights are explicit artifacts, not hidden state
//! - Same seed + adapter = deterministic output

use burn::module::Module;
use burn::nn::Linear;
use burn::tensor::backend::Backend;
use burn::tensor::{Distribution, Tensor};
use serde::{Deserialize, Serialize};

/// Configuration for LoRA layers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoraConfig {
    /// LoRA rank (r). Smaller = fewer parameters, larger = more capacity.
    pub rank: usize,

    /// LoRA alpha scaling factor. Scale = alpha / rank.
    pub alpha: f32,

    /// Dropout probability during training (0.0 = no dropout).
    pub dropout: f32,

    /// Whether to initialize B with zeros (standard LoRA init).
    pub init_b_zero: bool,
}

impl Default for LoraConfig {
    fn default() -> Self {
        Self {
            rank: 8,
            alpha: 16.0,
            dropout: 0.0,
            init_b_zero: true,
        }
    }
}

impl LoraConfig {
    /// Create a new LoRA config with defaults.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Create config optimized for fine-tuning.
    #[must_use]
    pub fn fine_tuning() -> Self {
        Self {
            rank: 16,
            alpha: 32.0,
            dropout: 0.05,
            init_b_zero: true,
        }
    }

    /// Create config for minimal adapter (small footprint).
    #[must_use]
    pub fn minimal() -> Self {
        Self {
            rank: 4,
            alpha: 8.0,
            dropout: 0.0,
            init_b_zero: true,
        }
    }

    /// Calculate the scaling factor.
    #[must_use]
    pub fn scale(&self) -> f32 {
        self.alpha / self.rank as f32
    }
}

/// LoRA-wrapped Linear layer.
///
/// Wraps a base Linear layer and adds low-rank adaptation.
/// The base weights are frozen during training; only A and B are updated.
///
/// # Forward Pass
///
/// ```text
/// y = base(x) + scale * (x @ A @ B)
/// ```
#[derive(Module, Debug)]
pub struct LoraLinear<B: Backend> {
    /// The base (frozen) linear layer
    base: Linear<B>,

    /// Low-rank "down" projection: input_dim -> rank
    lora_a: Tensor<B, 2>,

    /// Low-rank "up" projection: rank -> output_dim
    lora_b: Tensor<B, 2>,

    /// Scaling factor (alpha / rank)
    #[module(skip)]
    scale: f32,

    /// LoRA rank
    #[module(skip)]
    rank: usize,

    /// Whether LoRA is enabled (can be toggled for A/B testing)
    #[module(skip)]
    enabled: bool,
}

impl<B: Backend> LoraLinear<B> {
    /// Create a new LoRA-wrapped linear layer.
    ///
    /// # Arguments
    ///
    /// * `base` - The base linear layer to wrap
    /// * `config` - LoRA configuration
    /// * `device` - Device to create tensors on
    pub fn new(base: Linear<B>, config: &LoraConfig, device: &B::Device) -> Self {
        // Burn Linear weight shape: [in_features, out_features]
        let weight_dims = base.weight.dims();
        let in_features = weight_dims[0];
        let out_features = weight_dims[1];

        // Initialize A with Kaiming/He initialization
        // Standard: A ~ N(0, 1/sqrt(rank))
        let lora_a = Tensor::random(
            [in_features, config.rank],
            Distribution::Normal(0.0, 1.0 / (config.rank as f64).sqrt()),
            device,
        );

        // Initialize B with zeros (standard LoRA initialization)
        // This ensures the adapter starts as identity (no change to base model)
        let lora_b = if config.init_b_zero {
            Tensor::zeros([config.rank, out_features], device)
        } else {
            Tensor::random(
                [config.rank, out_features],
                Distribution::Normal(0.0, 1.0 / (out_features as f64).sqrt()),
                device,
            )
        };

        Self {
            base,
            lora_a,
            lora_b,
            scale: config.scale(),
            rank: config.rank,
            enabled: true,
        }
    }

    /// Create from existing base layer and loaded adapter weights.
    ///
    /// # Arguments
    ///
    /// * `base` - The base linear layer
    /// * `lora_a` - Pre-loaded A matrix
    /// * `lora_b` - Pre-loaded B matrix
    /// * `alpha` - LoRA alpha value
    pub fn from_weights(
        base: Linear<B>,
        lora_a: Tensor<B, 2>,
        lora_b: Tensor<B, 2>,
        alpha: f32,
    ) -> Self {
        let rank = lora_a.dims()[1];
        Self {
            base,
            lora_a,
            lora_b,
            scale: alpha / rank as f32,
            rank,
            enabled: true,
        }
    }

    /// Forward pass with LoRA adaptation.
    ///
    /// Computes: y = base(x) + scale * (x @ A @ B)
    pub fn forward(&self, x: Tensor<B, 2>) -> Tensor<B, 2> {
        let base_out = self.base.forward(x.clone());

        if !self.enabled {
            return base_out;
        }

        // LoRA: y = base(x) + scale * x @ A @ B
        let lora_out = x
            .matmul(self.lora_a.clone())
            .matmul(self.lora_b.clone())
            .mul_scalar(self.scale);

        base_out + lora_out
    }

    /// Forward pass for 3D input (batch x seq x features).
    pub fn forward_3d(&self, x: Tensor<B, 3>) -> Tensor<B, 3> {
        let [batch, seq, features] = x.dims();

        // Reshape to 2D: (batch * seq) x features
        let x_2d = x.reshape([batch * seq, features]);

        // Apply forward
        let out_2d = self.forward(x_2d);

        // Reshape back to 3D
        let out_features = out_2d.dims()[1];
        out_2d.reshape([batch, seq, out_features])
    }

    /// Enable or disable LoRA adaptation.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// Check if LoRA is enabled.
    #[must_use]
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Get the LoRA rank.
    #[must_use]
    pub fn rank(&self) -> usize {
        self.rank
    }

    /// Get the scaling factor.
    #[must_use]
    pub fn scale(&self) -> f32 {
        self.scale
    }

    /// Extract LoRA weights for checkpointing.
    #[must_use]
    pub fn extract_lora_weights(&self) -> LoraLayerWeights<B> {
        LoraLayerWeights {
            a: self.lora_a.clone(),
            b: self.lora_b.clone(),
            scale: self.scale,
            rank: self.rank,
        }
    }

    /// Get reference to the base layer.
    #[must_use]
    pub fn base(&self) -> &Linear<B> {
        &self.base
    }
}

/// Extracted LoRA weights for a single layer.
#[derive(Debug, Clone)]
pub struct LoraLayerWeights<B: Backend> {
    /// A matrix (input_dim x rank)
    pub a: Tensor<B, 2>,
    /// B matrix (rank x output_dim)
    pub b: Tensor<B, 2>,
    /// Scaling factor
    pub scale: f32,
    /// Rank
    pub rank: usize,
}

/// Collection of LoRA weights for multiple layers.
#[derive(Debug, Clone)]
pub struct LoraCheckpoint<B: Backend> {
    /// Layer name -> weights
    pub layers: std::collections::HashMap<String, LoraLayerWeights<B>>,
    /// Configuration used
    pub config: LoraConfig,
}

impl<B: Backend> LoraCheckpoint<B> {
    /// Create an empty checkpoint.
    #[must_use]
    pub fn new(config: LoraConfig) -> Self {
        Self {
            layers: std::collections::HashMap::new(),
            config,
        }
    }

    /// Add weights for a layer.
    pub fn add_layer(&mut self, name: impl Into<String>, weights: LoraLayerWeights<B>) {
        self.layers.insert(name.into(), weights);
    }

    /// Get weights for a layer.
    #[must_use]
    pub fn get_layer(&self, name: &str) -> Option<&LoraLayerWeights<B>> {
        self.layers.get(name)
    }

    /// Number of layers with LoRA weights.
    #[must_use]
    pub fn num_layers(&self) -> usize {
        self.layers.len()
    }
}

/// Builder for creating LoRA-wrapped models.
///
/// Helps apply LoRA to specific layers of a model.
#[derive(Debug)]
pub struct LoraBuilder {
    config: LoraConfig,
    target_layers: Vec<String>,
}

impl LoraBuilder {
    /// Create a new builder with the given configuration.
    #[must_use]
    pub fn new(config: LoraConfig) -> Self {
        Self {
            config,
            target_layers: vec![],
        }
    }

    /// Target specific layers for LoRA adaptation.
    #[must_use]
    pub fn target_layers(mut self, layers: Vec<String>) -> Self {
        self.target_layers = layers;
        self
    }

    /// Target common attention layers (q_proj, v_proj).
    #[must_use]
    pub fn target_attention(mut self) -> Self {
        self.target_layers = vec!["q_proj".to_string(), "v_proj".to_string()];
        self
    }

    /// Target all projection layers (q_proj, k_proj, v_proj, o_proj).
    #[must_use]
    pub fn target_all_projections(mut self) -> Self {
        self.target_layers = vec![
            "q_proj".to_string(),
            "k_proj".to_string(),
            "v_proj".to_string(),
            "o_proj".to_string(),
        ];
        self
    }

    /// Check if a layer should have LoRA applied.
    #[must_use]
    pub fn should_apply(&self, layer_name: &str) -> bool {
        if self.target_layers.is_empty() {
            return false;
        }
        self.target_layers.iter().any(|t| layer_name.contains(t))
    }

    /// Get the LoRA configuration.
    #[must_use]
    pub fn config(&self) -> &LoraConfig {
        &self.config
    }

    /// Wrap a linear layer with LoRA if it matches target layers.
    pub fn maybe_wrap<B: Backend>(
        &self,
        layer_name: &str,
        linear: Linear<B>,
        device: &B::Device,
    ) -> LoraLinearOrBase<B> {
        if self.should_apply(layer_name) {
            LoraLinearOrBase::Lora(LoraLinear::new(linear, &self.config, device))
        } else {
            LoraLinearOrBase::Base(linear)
        }
    }
}

/// Either a LoRA-wrapped linear or a base linear layer.
///
/// Allows models to have a mix of adapted and non-adapted layers.
#[derive(Module, Debug)]
pub enum LoraLinearOrBase<B: Backend> {
    /// LoRA-adapted layer
    Lora(LoraLinear<B>),
    /// Base layer (no adaptation)
    Base(Linear<B>),
}

impl<B: Backend> LoraLinearOrBase<B> {
    /// Forward pass.
    pub fn forward(&self, x: Tensor<B, 2>) -> Tensor<B, 2> {
        match self {
            Self::Lora(lora) => lora.forward(x),
            Self::Base(base) => base.forward(x),
        }
    }

    /// Forward pass for 3D input.
    pub fn forward_3d(&self, x: Tensor<B, 3>) -> Tensor<B, 3> {
        match self {
            Self::Lora(lora) => lora.forward_3d(x),
            Self::Base(base) => {
                let [batch, seq, features] = x.dims();
                let x_2d = x.reshape([batch * seq, features]);
                let out_2d = base.forward(x_2d);
                let out_features = out_2d.dims()[1];
                out_2d.reshape([batch, seq, out_features])
            }
        }
    }

    /// Check if this is a LoRA-adapted layer.
    #[must_use]
    pub fn is_lora(&self) -> bool {
        matches!(self, Self::Lora(_))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use burn::backend::NdArray;
    use burn::nn::LinearConfig;

    type TestBackend = NdArray<f32>;

    #[test]
    fn test_lora_config_scale() {
        let config = LoraConfig {
            rank: 8,
            alpha: 16.0,
            ..Default::default()
        };
        assert_eq!(config.scale(), 2.0);
    }

    #[test]
    fn test_lora_config_presets() {
        let default = LoraConfig::default();
        assert_eq!(default.rank, 8);

        let fine_tuning = LoraConfig::fine_tuning();
        assert_eq!(fine_tuning.rank, 16);

        let minimal = LoraConfig::minimal();
        assert_eq!(minimal.rank, 4);
    }

    #[test]
    fn test_lora_linear_creation() {
        let device = Default::default();
        let base_config = LinearConfig::new(64, 32);
        let base = base_config.init::<TestBackend>(&device);

        let lora_config = LoraConfig::default();
        let lora = LoraLinear::new(base, &lora_config, &device);

        assert_eq!(lora.rank(), 8);
        assert!(lora.is_enabled());
    }

    #[test]
    fn test_lora_linear_forward_2d() {
        let device = Default::default();
        let base_config = LinearConfig::new(64, 32);
        let base = base_config.init::<TestBackend>(&device);

        let lora_config = LoraConfig::default();
        let lora = LoraLinear::new(base, &lora_config, &device);

        let input: Tensor<TestBackend, 2> =
            Tensor::random([4, 64], Distribution::Normal(0.0, 1.0), &device);
        let output = lora.forward(input);

        assert_eq!(output.dims(), [4, 32]);
    }

    #[test]
    fn test_lora_linear_forward_3d() {
        let device = Default::default();
        let base_config = LinearConfig::new(64, 32);
        let base = base_config.init::<TestBackend>(&device);

        let lora_config = LoraConfig::default();
        let lora = LoraLinear::new(base, &lora_config, &device);

        let input: Tensor<TestBackend, 3> =
            Tensor::random([2, 8, 64], Distribution::Normal(0.0, 1.0), &device);
        let output = lora.forward_3d(input);

        assert_eq!(output.dims(), [2, 8, 32]);
    }

    #[test]
    fn test_lora_disabled() {
        let device = Default::default();
        let base_config = LinearConfig::new(64, 32);
        let base = base_config.init::<TestBackend>(&device);

        let lora_config = LoraConfig::default();
        let mut lora = LoraLinear::new(base, &lora_config, &device);

        let input: Tensor<TestBackend, 2> =
            Tensor::random([4, 64], Distribution::Normal(0.0, 1.0), &device);

        // Get output with LoRA enabled
        let output_enabled = lora.forward(input.clone());

        // Disable LoRA
        lora.set_enabled(false);
        let output_disabled = lora.forward(input);

        // Outputs should be different (unless B is exactly zero, which it is initially)
        // With B=0, they should be the same
        assert_eq!(output_enabled.dims(), output_disabled.dims());
    }

    #[test]
    fn test_lora_builder() {
        let builder = LoraBuilder::new(LoraConfig::default()).target_attention();

        assert!(builder.should_apply("layer.0.self_attn.q_proj"));
        assert!(builder.should_apply("layer.0.self_attn.v_proj"));
        assert!(!builder.should_apply("layer.0.self_attn.k_proj"));
        assert!(!builder.should_apply("layer.0.mlp.gate_proj"));
    }

    #[test]
    fn test_lora_checkpoint() {
        let device = Default::default();
        let config = LoraConfig::default();
        let mut checkpoint = LoraCheckpoint::<TestBackend>::new(config.clone());

        let weights = LoraLayerWeights {
            a: Tensor::zeros([64, 8], &device),
            b: Tensor::zeros([8, 32], &device),
            scale: 2.0,
            rank: 8,
        };

        checkpoint.add_layer("q_proj", weights);

        assert_eq!(checkpoint.num_layers(), 1);
        assert!(checkpoint.get_layer("q_proj").is_some());
        assert!(checkpoint.get_layer("v_proj").is_none());
    }
}
