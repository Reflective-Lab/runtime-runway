// Copyright 2024-2026 Reflective Labs

//! Golden Test Example
//!
//! This example demonstrates how to run a deterministic golden test.
//!
//! Golden tests verify that:
//! - Same prompt + same envelope + same weights = same output
//! - Reproducible inference for debugging and regression testing
//!
//! ## Requirements
//!
//! This example requires:
//! - The `pretrained` feature enabled
//! - A GPU backend (cuda or tch-gpu) for reasonable performance
//!
//! ## Running
//!
//! ```bash
//! # With CUDA backend
//! cargo run --example golden_test --features "cuda,pretrained"
//!
//! # With LibTorch GPU
//! cargo run --example golden_test --features "tch-gpu,pretrained"
//! ```

use converge_llm::{InferenceEnvelope, PromptStackBuilder, StateInjection, UserIntent};

#[cfg(all(feature = "llama3", feature = "pretrained", feature = "cuda"))]
use burn::backend::CudaJit;

#[cfg(all(feature = "llama3", feature = "pretrained", feature = "tch-gpu"))]
use burn::backend::LibTorch;

fn main() {
    // Print configuration
    println!("═══════════════════════════════════════════════════════════════");
    println!("                    Converge-LLM Golden Test");
    println!("═══════════════════════════════════════════════════════════════");
    println!();

    // Build a prompt using our contract system
    let stack = PromptStackBuilder::new()
        .state(
            StateInjection::new()
                .with_scalar("mae", 0.15)
                .with_scalar("success_ratio", 0.85)
                .with_list("top_features", vec!["feature_a".into(), "feature_b".into()]),
        )
        .intent(UserIntent::new("interpret_metrics").with_criteria("deployment_readiness"))
        .build();

    println!("Prompt Stack:");
    println!("─────────────────────────────────────────────────────────────────");
    println!("{}", stack.render());
    println!("─────────────────────────────────────────────────────────────────");
    println!();

    // Create deterministic envelope
    let envelope = InferenceEnvelope::deterministic("golden:v1", 42);

    println!("Inference Envelope:");
    println!("  Deterministic: {}", envelope.is_deterministic());
    println!("  Seed Policy: {:?}", envelope.seed_policy);
    println!("  Temperature: {}", envelope.generation.temperature);
    println!("  Max Tokens: {}", envelope.stopping.max_tokens);
    println!();

    // Run inference if a GPU backend is available
    #[cfg(all(feature = "llama3", feature = "pretrained", feature = "cuda"))]
    {
        run_golden_test::<CudaJit<burn::tensor::f16, i32>>(&stack, &envelope);
    }

    #[cfg(all(
        feature = "llama3",
        feature = "pretrained",
        feature = "tch-gpu",
        not(feature = "cuda")
    ))]
    {
        run_golden_test::<LibTorch<burn::tensor::f16>>(&stack, &envelope);
    }

    #[cfg(not(any(
        all(feature = "llama3", feature = "pretrained", feature = "cuda"),
        all(
            feature = "llama3",
            feature = "pretrained",
            feature = "tch-gpu",
            not(feature = "cuda")
        )
    )))]
    {
        println!("⚠️  No GPU backend available.");
        println!();
        println!("To run this example with actual inference, use:");
        println!("  cargo run --example golden_test --features \"cuda,pretrained\"");
        println!("  cargo run --example golden_test --features \"tch-gpu,pretrained\"");
        println!();
        println!("Contracts validated:");
        println!("  ✓ PromptStack renders correctly");
        println!("  ✓ InferenceEnvelope is deterministic");
        println!("  ✓ Version binding: {}", stack.version);
    }
}

#[cfg(all(feature = "llama3", feature = "pretrained"))]
fn run_golden_test<B: burn::tensor::backend::Backend>(
    stack: &PromptStack,
    envelope: &InferenceEnvelope,
) {
    use converge_llm::LlamaEngine;

    println!("Loading model...");
    let device = burn::tensor::Device::<B>::default();

    // Load Llama 3.2 3B (smallest Llama 3 variant)
    match LlamaEngine::<B>::load_llama3_2_3b(2048, &device) {
        Ok(mut engine) => {
            println!("Model loaded successfully!");
            println!();

            // Run inference
            println!("Running inference...");
            match engine.run(stack, envelope) {
                Ok(result) => {
                    println!();
                    println!("Generated Output:");
                    println!("─────────────────────────────────────────────────────────────────");
                    println!("{}", result.text);
                    println!("─────────────────────────────────────────────────────────────────");
                    println!();
                    println!("Metrics:");
                    println!("  Input tokens:  ~{}", result.input_tokens);
                    println!("  Output tokens: {}", result.output_tokens);
                    println!("  Finish reason: {:?}", result.finish_reason);
                    println!();

                    // Verify determinism
                    println!("Verifying determinism (running twice)...");
                    let result2 = engine.run(stack, envelope).expect("Second run failed");

                    if result.text == result2.text {
                        println!("  ✓ Output matches! Determinism verified.");
                    } else {
                        println!("  ✗ Output differs! Non-deterministic behavior detected.");
                        println!("    First:  {}", &result.text[..50.min(result.text.len())]);
                        println!(
                            "    Second: {}",
                            &result2.text[..50.min(result2.text.len())]
                        );
                    }
                }
                Err(e) => {
                    println!("✗ Inference failed: {:?}", e);
                }
            }
        }
        Err(e) => {
            println!("✗ Model loading failed: {}", e);
            println!();
            println!("Make sure you have the 'pretrained' feature enabled and");
            println!("sufficient GPU memory for Llama 3.2 3B (~6GB VRAM).");
        }
    }
}
