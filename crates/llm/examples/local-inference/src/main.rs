// Copyright 2024-2026 Reflective Labs
// SPDX-License-Identifier: MIT

//! Local Inference — run LLM inference on Apple Silicon.
//!
//! See README.md for feature flags and expected performance.

use converge_llm::{InferenceEnvelope, PromptStackBuilder, StateInjection, UserIntent};

#[cfg(any(feature = "llama3", feature = "tiny"))]
#[cfg(feature = "wgpu")]
type Backend = burn::backend::Wgpu;

#[cfg(any(feature = "llama3", feature = "tiny"))]
#[cfg(all(feature = "ndarray", not(feature = "wgpu")))]
type Backend = burn::backend::NdArray;

fn main() {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    println!("=== Converge Local Inference ===\n");

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

    let rendered = stack.render();
    println!("Prompt ({} chars):", rendered.len());
    println!("{}\n", &rendered[..rendered.len().min(300)]);

    // Create deterministic envelope for reproducibility
    let envelope = InferenceEnvelope::deterministic("local:v1", 42);
    println!(
        "Envelope: deterministic={}, max_tokens={}\n",
        envelope.is_deterministic(),
        envelope.stopping.max_tokens
    );

    run_inference(&stack, &envelope);
}

#[cfg(feature = "gemma")]
fn run_inference(stack: &converge_llm::PromptStack, envelope: &converge_llm::InferenceEnvelope) {
    use converge_llm::{GemmaConfig, GemmaEngine};
    use std::time::Instant;

    println!("Loading Gemma GGUF via embedded llama.cpp...");
    let start = Instant::now();

    let config = match GemmaConfig::from_env() {
        Ok(config) => config,
        Err(error) => {
            println!("{error}");
            println!("Set CONVERGE_GEMMA_MODEL_PATH to a local Gemma Q4 GGUF file.");
            println!(
                "Example: CONVERGE_GEMMA_MODEL_PATH=~/models/gemma-7b-it-Q4_K_M.gguf cargo run -p example-local-inference --features gemma --release"
            );
            return;
        }
    };

    match GemmaEngine::load_from_gguf(config) {
        Ok(mut engine) => {
            println!("Loaded in {:.2}s\n", start.elapsed().as_secs_f64());
            let gen_start = Instant::now();
            match engine.run(stack, envelope) {
                Ok(result) => {
                    let secs = gen_start.elapsed().as_secs_f64();
                    println!("Output: {}\n", result.text);
                    println!(
                        "Tokens: {} in / {} out ({:.1} tok/s)",
                        result.input_tokens,
                        result.output_tokens,
                        result.output_tokens as f64 / secs,
                    );
                    println!("Finish reason: {:?}", result.finish_reason);
                }
                Err(error) => println!("Generation failed: {error:?}"),
            }
        }
        Err(error) => println!("Model load failed: {error}"),
    }
}

#[cfg(all(feature = "tiny", feature = "pretrained"))]
fn run_inference(stack: &converge_llm::PromptStack, envelope: &converge_llm::InferenceEnvelope) {
    use converge_llm::TinyLlamaEngine;
    use std::time::Instant;

    println!("Loading TinyLlama 1.1B...");
    let device = burn::tensor::Device::<Backend>::default();
    let start = Instant::now();

    match TinyLlamaEngine::<Backend>::load_pretrained(2048, &device) {
        Ok(mut engine) => {
            println!("Loaded in {:.2}s\n", start.elapsed().as_secs_f64());
            let gen_start = Instant::now();
            match engine.run(stack, envelope) {
                Ok(result) => {
                    let secs = gen_start.elapsed().as_secs_f64();
                    println!("Output: {}\n", result.text);
                    println!(
                        "Tokens: {} in / {} out ({:.1} tok/s)",
                        result.input_tokens,
                        result.output_tokens,
                        result.output_tokens as f64 / secs,
                    );
                }
                Err(e) => println!("Generation failed: {e:?}"),
            }
        }
        Err(e) => println!("Model load failed: {e}"),
    }
}

#[cfg(all(feature = "llama3", feature = "pretrained", not(feature = "tiny")))]
fn run_inference(stack: &converge_llm::PromptStack, envelope: &converge_llm::InferenceEnvelope) {
    use converge_llm::LlamaEngine;
    use std::time::Instant;

    println!("Loading Llama 3.2 3B...");
    let device = burn::tensor::Device::<Backend>::default();
    let start = Instant::now();

    match LlamaEngine::<Backend>::load_llama3_2_3b(2048, &device) {
        Ok(mut engine) => {
            println!("Loaded in {:.2}s\n", start.elapsed().as_secs_f64());
            let gen_start = Instant::now();
            match engine.run(stack, envelope) {
                Ok(result) => {
                    let secs = gen_start.elapsed().as_secs_f64();
                    println!("Output: {}\n", result.text);
                    println!(
                        "Tokens: {} in / {} out ({:.1} tok/s)",
                        result.input_tokens,
                        result.output_tokens,
                        result.output_tokens as f64 / secs,
                    );
                }
                Err(e) => println!("Generation failed: {e:?}"),
            }
        }
        Err(e) => println!("Model load failed: {e}"),
    }
}

#[cfg(not(any(
    feature = "gemma",
    all(feature = "tiny", feature = "pretrained"),
    all(feature = "llama3", feature = "pretrained", not(feature = "tiny"))
)))]
fn run_inference(stack: &converge_llm::PromptStack, _envelope: &converge_llm::InferenceEnvelope) {
    println!("No model backend enabled.");
    println!(
        "Run with: CONVERGE_GEMMA_MODEL_PATH=/path/to/gemma-7b-it-Q4_K_M.gguf cargo run -p example-local-inference --features \"gemma\" --release"
    );
    println!("\nPrompt stack validated: version={}", stack.version);
}
