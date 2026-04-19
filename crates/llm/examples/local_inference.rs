// Copyright 2024-2026 Reflective Labs

//! Local Inference Example for Apple Silicon (M1/M2/M3/M4)
//!
//! This example demonstrates running LLM inference locally on a Mac
//! using the Metal GPU backend via burn-wgpu.
//!
//! ## Requirements
//!
//! - Apple Silicon Mac (M1/M2/M3/M4)
//! - macOS 12.0+
//! - Rust 1.85+
//!
//! ## Running
//!
//! ```bash
//! # With Metal (GPU) - Recommended for Apple Silicon
//! cargo run --example local_inference --features "wgpu,llama3,pretrained" --release
//!
//! # With CPU only (slower but always works)
//! cargo run --example local_inference --features "ndarray,llama3,pretrained" --release
//!
//! # Quick test with tiny model (fast, for testing pipeline)
//! cargo run --example local_inference --features "wgpu,tiny,pretrained" --release
//! ```
//!
//! ## Expected Performance on M4 Mac
//!
//! - Tiny model: ~100+ tokens/sec
//! - Llama 3.2 3B (quantized): ~20-40 tokens/sec
//! - Llama 3 8B (quantized): ~10-20 tokens/sec
//!
//! Note: First run will download model weights (~2-6GB depending on model).

use converge_llm::{InferenceEnvelope, PromptStackBuilder, StateInjection, UserIntent};

fn main() {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    println!("═══════════════════════════════════════════════════════════════");
    println!("        Converge-LLM Local Inference (Apple Silicon)");
    println!("═══════════════════════════════════════════════════════════════");
    println!();

    // Show system info
    print_system_info();

    // Build a prompt using the contract system
    let stack = PromptStackBuilder::new()
        .state(
            StateInjection::new()
                .with_scalar("mae", 0.12)
                .with_scalar("success_ratio", 0.88)
                .with_list(
                    "top_features",
                    vec!["user_engagement".into(), "session_duration".into()],
                ),
        )
        .intent(
            UserIntent::new("analyze_model_performance")
                .with_criteria("identify areas for improvement"),
        )
        .build();

    println!("Prompt Stack (rendered):");
    println!("─────────────────────────────────────────────────────────────────");
    let rendered = stack.render();
    // Show first 500 chars to avoid flooding terminal
    if rendered.len() > 500 {
        println!("{}...", &rendered[..500]);
    } else {
        println!("{}", rendered);
    }
    println!("─────────────────────────────────────────────────────────────────");
    println!();

    // Create inference envelope (deterministic for reproducibility)
    let envelope = InferenceEnvelope::deterministic("local:v1", 42);

    println!("Inference Configuration:");
    println!("  Deterministic: {}", envelope.is_deterministic());
    println!("  Max tokens: {}", envelope.stopping.max_tokens);
    println!("  Temperature: {}", envelope.generation.temperature);
    println!();

    // Run inference based on available features
    run_inference(&stack, &envelope);
}

fn print_system_info() {
    println!("System Information:");

    #[cfg(feature = "gemma")]
    println!("  Backend: embedded llama.cpp (GGUF)");

    #[cfg(feature = "wgpu")]
    println!("  Backend: wgpu (Metal on macOS)");

    #[cfg(all(feature = "ndarray", not(feature = "wgpu")))]
    println!("  Backend: ndarray (CPU only)");

    #[cfg(feature = "tiny")]
    println!("  Model: Tiny (test model)");

    #[cfg(feature = "gemma")]
    println!("  Model: Gemma GGUF");

    #[cfg(all(feature = "llama3", not(feature = "tiny")))]
    println!("  Model: Llama 3");

    #[cfg(target_os = "macos")]
    println!("  Platform: macOS (Apple Silicon)");

    #[cfg(not(target_os = "macos"))]
    println!("  Platform: {}", std::env::consts::OS);

    println!();
}

#[cfg(feature = "gemma")]
fn run_inference(stack: &converge_llm::PromptStack, envelope: &converge_llm::InferenceEnvelope) {
    use converge_llm::{GemmaConfig, GemmaEngine};
    use std::time::Instant;

    println!("Loading Gemma GGUF (embedded llama.cpp)...");
    let start = Instant::now();

    let config = match GemmaConfig::from_env() {
        Ok(config) => config,
        Err(error) => {
            println!("✗ Gemma configuration failed: {error}");
            println!();
            println!("Set CONVERGE_GEMMA_MODEL_PATH to a local Gemma Q4 GGUF file.");
            println!(
                "Example: CONVERGE_GEMMA_MODEL_PATH=~/models/gemma-7b-it-Q4_K_M.gguf cargo run --example local_inference --features \"gemma\" --release"
            );
            return;
        }
    };

    match GemmaEngine::load_from_gguf(config) {
        Ok(mut engine) => {
            let load_time = start.elapsed();
            println!("✓ Gemma loaded in {:.2}s", load_time.as_secs_f64());
            println!();

            println!("Generating response...");
            let gen_start = Instant::now();

            match engine.run(stack, envelope) {
                Ok(result) => {
                    let gen_time = gen_start.elapsed();

                    println!();
                    println!("Generated Output:");
                    println!("─────────────────────────────────────────────────────────────────");
                    println!("{}", result.text);
                    println!("─────────────────────────────────────────────────────────────────");
                    println!();
                    println!("Performance Metrics:");
                    println!("  Input tokens:  {}", result.input_tokens);
                    println!("  Output tokens: {}", result.output_tokens);
                    println!("  Generation time: {:.2}s", gen_time.as_secs_f64());
                    println!(
                        "  Tokens/second: {:.1}",
                        result.output_tokens as f64 / gen_time.as_secs_f64()
                    );
                    println!("  Finish reason: {:?}", result.finish_reason);
                }
                Err(error) => {
                    println!("✗ Generation failed: {:?}", error);
                }
            }
        }
        Err(error) => {
            println!("✗ Model loading failed: {}", error);
            println!();
            println!("Troubleshooting:");
            println!("  1. Download a local Gemma GGUF file");
            println!("  2. Prefer a Q4 variant for MacBook local inference");
            println!("  3. Set CONVERGE_GEMMA_MODEL_PATH to that file");
        }
    }
}

// TinyLlama inference (smallest model, ideal for laptops)
#[cfg(all(feature = "tiny", feature = "pretrained"))]
fn run_inference(stack: &converge_llm::PromptStack, envelope: &converge_llm::InferenceEnvelope) {
    use converge_llm::TinyLlamaEngine;
    use std::time::Instant;

    println!("Loading TinyLlama 1.1B (this may download ~2GB on first run)...");
    let start = Instant::now();

    // Get device for the backend
    let device = burn::tensor::Device::<Backend>::default();

    // Load TinyLlama (smallest model)
    match TinyLlamaEngine::<Backend>::load_pretrained(2048, &device) {
        Ok(mut engine) => {
            let load_time = start.elapsed();
            println!("✓ TinyLlama loaded in {:.2}s", load_time.as_secs_f64());
            println!();

            // Run inference
            println!("Generating response...");
            let gen_start = Instant::now();

            match engine.run(stack, envelope) {
                Ok(result) => {
                    let gen_time = gen_start.elapsed();

                    println!();
                    println!("Generated Output:");
                    println!("─────────────────────────────────────────────────────────────────");
                    println!("{}", result.text);
                    println!("─────────────────────────────────────────────────────────────────");
                    println!();
                    println!("Performance Metrics:");
                    println!("  Input tokens:  ~{}", result.input_tokens);
                    println!("  Output tokens: {}", result.output_tokens);
                    println!("  Generation time: {:.2}s", gen_time.as_secs_f64());
                    println!(
                        "  Tokens/second: {:.1}",
                        result.output_tokens as f64 / gen_time.as_secs_f64()
                    );
                    println!("  Finish reason: {:?}", result.finish_reason);
                }
                Err(e) => {
                    println!("✗ Generation failed: {:?}", e);
                }
            }
        }
        Err(e) => {
            println!("✗ Model loading failed: {}", e);
            println!();
            println!("Troubleshooting:");
            println!("  1. Ensure you have the 'pretrained' feature enabled");
            println!("  2. Check you have ~2GB free disk space for TinyLlama weights");
            println!("  3. Check network connectivity (weights downloaded from HuggingFace)");
        }
    }
}

// Llama 3 inference (larger models)
#[cfg(all(feature = "llama3", feature = "pretrained", not(feature = "tiny")))]
fn run_inference(stack: &converge_llm::PromptStack, envelope: &converge_llm::InferenceEnvelope) {
    use converge_llm::LlamaEngine;
    use std::time::Instant;

    println!("Loading Llama 3.2 3B (this may download ~6GB on first run)...");
    let start = Instant::now();

    // Get device for the backend
    let device = burn::tensor::Device::<Backend>::default();

    // Try to load Llama 3.2 3B
    match LlamaEngine::<Backend>::load_llama3_2_3b(2048, &device) {
        Ok(mut engine) => {
            let load_time = start.elapsed();
            println!("✓ Llama 3.2 3B loaded in {:.2}s", load_time.as_secs_f64());
            println!();

            // Run inference
            println!("Generating response...");
            let gen_start = Instant::now();

            match engine.run(stack, envelope) {
                Ok(result) => {
                    let gen_time = gen_start.elapsed();

                    println!();
                    println!("Generated Output:");
                    println!("─────────────────────────────────────────────────────────────────");
                    println!("{}", result.text);
                    println!("─────────────────────────────────────────────────────────────────");
                    println!();
                    println!("Performance Metrics:");
                    println!("  Input tokens:  ~{}", result.input_tokens);
                    println!("  Output tokens: {}", result.output_tokens);
                    println!("  Generation time: {:.2}s", gen_time.as_secs_f64());
                    println!(
                        "  Tokens/second: {:.1}",
                        result.output_tokens as f64 / gen_time.as_secs_f64()
                    );
                    println!("  Finish reason: {:?}", result.finish_reason);
                }
                Err(e) => {
                    println!("✗ Generation failed: {:?}", e);
                }
            }
        }
        Err(e) => {
            println!("✗ Model loading failed: {}", e);
            println!();
            println!("Troubleshooting:");
            println!("  1. Ensure you have the 'pretrained' feature enabled");
            println!("  2. Check you have ~6GB free RAM for Llama 3.2 3B");
            println!("  3. Try the tiny model: --features \"wgpu,tiny,pretrained\"");
        }
    }
}

// No model features enabled
#[cfg(not(any(
    feature = "gemma",
    all(feature = "tiny", feature = "pretrained"),
    all(feature = "llama3", feature = "pretrained", not(feature = "tiny"))
)))]
fn run_inference(stack: &converge_llm::PromptStack, _envelope: &converge_llm::InferenceEnvelope) {
    println!("⚠️  Model loading not available with current features.");
    println!();
    println!("To run actual inference, use:");
    println!(
        "  CONVERGE_GEMMA_MODEL_PATH=/path/to/gemma-7b-it-Q4_K_M.gguf cargo run --example local_inference --features \"gemma\" --release"
    );
    println!();
    println!("Or for quick testing with tiny model:");
    println!("  cargo run --example local_inference --features \"wgpu,tiny,pretrained\" --release");
    println!();
    println!("Contracts validated successfully:");
    println!("  ✓ PromptStack built and rendered");
    println!("  ✓ InferenceEnvelope configured");
    println!("  ✓ Version binding: {}", stack.version);
}
