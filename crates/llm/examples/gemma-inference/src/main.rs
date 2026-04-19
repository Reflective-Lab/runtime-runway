// Copyright 2024-2026 Reflective Labs

//! Gemma GGUF interactive chat via embedded llama.cpp.
//!
//! ## Running
//!
//! ```bash
//! CONVERGE_GEMMA_MODEL_PATH=~/models/gemma-2b-it-Q4_K_M.gguf \
//!     cargo run -p example-gemma-inference --release
//! ```
//!
//! Optional environment overrides:
//! - `CONVERGE_GEMMA_CONTEXT_LEN` (default: 4096)
//! - `CONVERGE_GEMMA_BATCH_SIZE` (default: 1024)
//! - `CONVERGE_GEMMA_THREADS` (default: available parallelism)
//! - `CONVERGE_GEMMA_GPU_LAYERS` (default: u32::MAX = all to Metal)

use converge_llm::{
    GemmaConfig, GemmaEngine, InferenceEnvelope, PromptStackBuilder, StoppingCriteria, UserIntent,
};
use std::io::{self, BufRead, Write};
use std::time::Instant;

fn main() {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    println!("=== Converge Gemma Chat ===\n");

    let config = match GemmaConfig::from_env() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Configuration error: {e}");
            eprintln!("\nSet CONVERGE_GEMMA_MODEL_PATH to a local Gemma GGUF file.");
            eprintln!("Example:");
            eprintln!("  CONVERGE_GEMMA_MODEL_PATH=~/models/gemma-2b-it-Q4_K_M.gguf \\");
            eprintln!("      cargo run -p example-gemma-inference --release");
            std::process::exit(1);
        }
    };

    println!(
        "Model:   {}",
        config
            .model_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
    );
    println!("Context: {} tokens", config.context_len);
    println!("Threads: {}", config.threads);
    println!("GPU layers: {}\n", config.gpu_layers);

    println!("Loading model...");
    let load_start = Instant::now();

    let mut engine = match GemmaEngine::load_from_gguf(config) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("Failed to load model: {e}");
            std::process::exit(1);
        }
    };

    println!("Loaded in {:.2}s", load_start.elapsed().as_secs_f64());
    println!("Type a message and press Enter. Ctrl-D to quit.\n");

    let stdin = io::stdin();
    loop {
        print!("> ");
        io::stdout().flush().ok();

        let mut line = String::new();
        if stdin.lock().read_line(&mut line).unwrap_or(0) == 0 {
            println!("\nBye!");
            break;
        }

        let input = line.trim();
        if input.is_empty() {
            continue;
        }

        let stack = PromptStackBuilder::new()
            .intent(UserIntent::new(input))
            .build();

        let mut envelope = InferenceEnvelope::agent_reasoning("gemma:chat");
        envelope.stopping = StoppingCriteria {
            max_tokens: 512,
            stop_on_eos: true,
            stop_sequences: vec![],
            timeout_ms: 60000,
        };

        let gen_start = Instant::now();
        match engine.run(&stack, &envelope) {
            Ok(result) => {
                let secs = gen_start.elapsed().as_secs_f64();
                println!("\n{}", result.text.trim());
                println!(
                    "\n[{} tok in {:.1}s — {:.1} tok/s, {:?}]\n",
                    result.output_tokens,
                    secs,
                    result.output_tokens as f64 / secs,
                    result.finish_reason,
                );
            }
            Err(e) => {
                eprintln!("Error: {e:?}\n");
            }
        }
    }
}
