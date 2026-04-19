// Copyright 2024-2026 Reflective Labs

//! Example: LLM Suggestor Integration
//!
//! Demonstrates how to use the LlmAgent within a Converge context.
//! This example shows the agent API without requiring actual model weights.

use converge_core::{Context, ContextKey, Engine, Suggestor};
use converge_llm::{GenerationParams, LlmAgent, LlmConfig, PromptTemplate};

fn main() {
    // Initialize tracing for observability
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    println!("=== Converge LLM Suggestor Example ===\n");

    // Create an LLM agent with small config (for demo)
    let config = LlmConfig::small();
    println!("Model config:");
    println!("  - Model: {}", config.model_id);
    println!("  - Context: {} tokens", config.max_context_length);
    println!("  - Precision: {:?}", config.precision);
    println!("  - Est. memory: {:.1} GB\n", config.estimated_memory_gb());

    // Configure the agent
    let agent = LlmAgent::new("reasoning-agent", config)
        .with_params(GenerationParams::agent())
        .with_template(PromptTemplate::reasoning())
        .with_output_key(ContextKey::Hypotheses);

    println!("Suggestor: {}", agent.name());
    println!("Dependencies: {:?}", agent.dependencies());
    println!();

    // Create a context with some seed facts
    let mut staged = Context::new();

    // Add seed facts (simulating data from analytics pipeline)
    let _ = staged.add_input(
        ContextKey::Seeds,
        "metric-1",
        "Monthly active users increased by 15% in December",
    );

    let _ = staged.add_input(
        ContextKey::Seeds,
        "metric-2",
        "Customer churn rate decreased from 5% to 3.5%",
    );

    let _ = staged.add_input(
        ContextKey::Signals,
        "signal-1",
        "New onboarding flow launched November 15th",
    );

    let mut ctx = Engine::new()
        .run(staged)
        .expect("seed inputs should promote")
        .context;

    println!(
        "Context prepared with {} seeds and {} signals\n",
        ctx.get(ContextKey::Seeds).len(),
        ctx.get(ContextKey::Signals).len()
    );

    // Check if agent accepts this context
    println!("Suggestor accepts context: {}", agent.accepts(&ctx));

    // Execute the agent (this will fail gracefully without loaded model)
    println!("\nExecuting agent...");
    let effect = agent.execute(&ctx);

    // Examine the effect
    println!(
        "\nSuggestor produced {} proposal(s):",
        effect.proposals.len()
    );
    for proposal in &effect.proposals {
        println!(
            "  [{:?}] {}: {}",
            proposal.key,
            proposal.id,
            if proposal.content.len() > 80 {
                format!("{}...", &proposal.content[..80])
            } else {
                proposal.content.clone()
            }
        );
    }

    // Demonstrate that agent won't run twice (already has output)
    let _ = ctx.add_proposal(effect.proposals.into_iter().next().unwrap());
    println!("\nAfter adding output to context:");
    println!("Suggestor accepts context: {}", agent.accepts(&ctx));

    println!("\n=== Example Complete ===");
    println!("\nNote: To run with actual inference, load model weights first.");
    println!("See README.md for instructions on downloading pretrained models.");
}
