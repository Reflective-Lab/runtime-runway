// Copyright 2024-2026 Reflective Labs
// SPDX-License-Identifier: MIT

//! Converge App - Distribution & Packaging Layer
//!
//! This is the generic converge distribution binary that:
//! - Manages the convergence engine lifecycle
//! - Provides CLI and TUI interfaces
//! - Supports pluggable domain packs
//!
//! Domain-specific agent packs (growth-strategy, patent, SDR, etc.)
//! are provided by organism-application, not this crate.

#![allow(dead_code)]
#![allow(unused_variables)]

mod agents;
mod config;
mod evals;
mod llm_backend;
mod packs;
mod streaming;
#[cfg(feature = "tui")]
mod ui;

use anyhow::Result;
use chrono::Utc;
use clap::{Parser, Subcommand};
#[cfg(feature = "tui")]
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
#[cfg(feature = "tui")]
use ratatui::{Terminal, backend::CrosstermBackend};
use serde::Serialize;
#[cfg(feature = "tui")]
use std::io;
#[cfg(feature = "tui")]
use std::panic;
use std::sync::Arc;
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

use converge_core::traits::DynChatBackend;
use converge_core::{Context, ContextKey, Engine, ExperienceStore};
use converge_experience::{InMemoryExperienceStore, StoreObserver};
use strum::IntoEnumIterator;

/// Converge - Semantic convergence engine for agentic workflows
#[derive(Parser)]
#[command(name = "converge")]
#[command(about = "Converge Suggestor OS - where agents propose and the engine decides")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Launch interactive TUI
    #[cfg(feature = "tui")]
    Tui,

    /// Manage domain packs
    Packs {
        #[command(subcommand)]
        command: PacksCommands,
    },

    /// Run a job from the command line
    Run {
        /// Template to use
        #[arg(short, long)]
        template: String,

        /// Seeds as JSON (or @file.json)
        #[arg(short, long)]
        seeds: Option<String>,

        /// Max cycles budget
        #[arg(long, default_value = "50")]
        max_cycles: u32,

        /// Run ID for traceability (auto-generated if not provided)
        #[arg(long)]
        run_id: Option<String>,

        /// Correlation ID to link related runs
        #[arg(long)]
        correlation_id: Option<String>,

        /// Use mock LLM for deterministic output
        #[arg(long)]
        mock: bool,

        /// Output as JSON
        #[arg(long)]
        json: bool,

        /// Stream facts as they arrive (real-time output)
        #[arg(long)]
        stream: bool,

        /// Quiet mode: exit code only, no output
        #[arg(long)]
        quiet: bool,
    },

    /// Run eval fixtures for reproducible testing
    Eval {
        #[command(subcommand)]
        command: EvalCommands,
    },
}

#[derive(Subcommand)]
enum EvalCommands {
    /// Run eval fixtures
    Run {
        /// Specific eval ID to run (runs all if not specified)
        eval_id: Option<String>,

        /// Directory containing eval fixtures
        #[arg(short, long, default_value = "evals")]
        dir: String,

        /// Use mock LLM for faster deterministic tests
        #[arg(long)]
        mock: bool,
    },
    /// List available eval fixtures
    List {
        /// Directory containing eval fixtures
        #[arg(short, long, default_value = "evals")]
        dir: String,
    },
}

#[derive(Subcommand)]
enum PacksCommands {
    /// List available domain packs
    List,
    /// Show details of a specific pack
    Info {
        /// Pack name
        name: String,
    },
}

/// JSON output format for run results (Cross-Platform Contract compliant)
#[derive(Debug, Serialize)]
struct RunOutput {
    run_id: String,
    correlation_id: String,
    timestamp: String,
    actor: ActorInfo,
    result: RunResultOutput,
    facts: Vec<FactOutput>,
}

#[derive(Debug, Serialize)]
struct ActorInfo {
    #[serde(rename = "type")]
    actor_type: String,
    device_id: String,
    cli_version: String,
}

#[derive(Debug, Serialize)]
struct RunResultOutput {
    converged: bool,
    cycles: u32,
    total_facts: usize,
}

#[derive(Debug, Serialize)]
struct FactOutput {
    sequence: usize,
    key: String,
    id: String,
    content: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenv::dotenv().ok();

    let cli = Cli::parse();

    let suppress_tracing = matches!(&cli.command, Commands::Run { quiet: true, .. });

    if !suppress_tracing {
        tracing_subscriber::fmt()
            .with_env_filter(
                EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
            )
            .with_target(false)
            .init();
    }

    match cli.command {
        #[cfg(feature = "tui")]
        Commands::Tui => {
            run_tui().await?;
        }

        Commands::Packs { command } => match command {
            PacksCommands::List => {
                println!("Available domain packs:\n");
                for pack in packs::available_packs() {
                    let info = packs::pack_info(&pack);
                    println!("  {} - {}", pack, info.description);
                }
            }
            PacksCommands::Info { name } => {
                let info = packs::pack_info(&name);
                println!("Pack: {name}");
                println!("Description: {}", info.description);
                println!("Version: {}", info.version);
                println!("\nTemplates:");
                for template in &info.templates {
                    println!("  - {template}");
                }
                println!("\nInvariants:");
                for invariant in &info.invariants {
                    println!("  - {invariant}");
                }
            }
        },

        Commands::Run {
            template,
            seeds,
            max_cycles,
            run_id,
            correlation_id,
            mock,
            json,
            stream,
            quiet,
        } => {
            let run_id = run_id.unwrap_or_else(|| format!("run_{}", uuid::Uuid::new_v4()));
            let correlation_id =
                correlation_id.unwrap_or_else(|| format!("cor_{}", uuid::Uuid::new_v4()));

            let hostname = hostname::get().map_or_else(
                |_| "unknown".to_string(),
                |h| h.to_string_lossy().to_string(),
            );
            let username = std::env::var("USER").unwrap_or_else(|_| "unknown".to_string());
            let device_id = format!("cli:{hostname}:{username}");

            if !json && !stream && !quiet {
                info!(
                    template = %template,
                    run_id = %run_id,
                    correlation_id = %correlation_id,
                    "Running job from CLI"
                );
            }

            let enabled_packs = packs::available_packs();
            let registry = packs::load_templates(&enabled_packs)?;

            let _template_arc = registry.get(&template).ok_or_else(|| {
                anyhow::anyhow!("Template '{template}' not found in any enabled pack")
            })?;

            let mut context = Context::new();
            if let Some(seeds_raw) = seeds {
                let seeds_json = if let Some(path) = seeds_raw.strip_prefix('@') {
                    std::fs::read_to_string(path)
                        .map_err(|e| anyhow::anyhow!("Failed to read seed file '{path}': {e}"))?
                } else {
                    seeds_raw
                };

                let seed_facts: Vec<crate::packs::SeedFact> = serde_json::from_str(&seeds_json)
                    .map_err(|e| anyhow::anyhow!("Failed to parse seeds JSON: {e}"))?;

                for seed in seed_facts {
                    context
                        .add_input(ContextKey::Seeds, seed.id, seed.content)
                        .map_err(|e| anyhow::anyhow!("Failed to add seed fact: {e}"))?;
                }
            }

            let total_facts: usize = ContextKey::iter().map(|key| context.get(key).len()).sum();
            if !json && !stream && !quiet {
                info!(facts = total_facts, "Context initialized with seeds");
            }

            let mut engine = Engine::new();

            // Wire experience event observer for audit trail capture
            let experience_store = Arc::new(InMemoryExperienceStore::new());
            let observer = Arc::new(StoreObserver::new(experience_store.clone()));
            engine.set_event_observer(observer);

            // Domain-specific agent registration should be provided by
            // organism-application or via a plugin mechanism.
            warn!(
                template = %template,
                "No domain agents registered. Use organism-application for domain-specific packs."
            );

            let streaming_handler = if stream {
                use crate::streaming::{OutputFormat, StreamingHandler};
                let format = if json {
                    OutputFormat::Json
                } else {
                    OutputFormat::Human
                };
                let handler = Arc::new(StreamingHandler::new(format));
                engine.set_streaming(handler.clone());
                Some(handler)
            } else {
                None
            };

            if !stream && !quiet {
                info!("Starting convergence loop...");
            }

            let result = if quiet {
                match engine.run(context).await {
                    Ok(r) => r,
                    Err(e) => {
                        let exit_code = if e.to_string().contains("invariant") {
                            1
                        } else {
                            3
                        };
                        std::process::exit(exit_code);
                    }
                }
            } else {
                engine.run(context).await?
            };

            // Report experience events captured during the run
            if let Ok(events) = experience_store.query_events(&converge_core::EventQuery::default())
            {
                if !events.is_empty() && !quiet {
                    info!(events = events.len(), "Experience events captured");
                }
            }

            if !stream && !quiet {
                if result.converged {
                    info!(cycles = result.cycles, "Job reached fixed point");
                } else {
                    warn!(
                        cycles = result.cycles,
                        "Job halted without reaching fixed point (budget exhausted)"
                    );
                }
            }

            if quiet {
                let exit_code = if result.converged { 0 } else { 2 };
                std::process::exit(exit_code);
            } else if let Some(handler) = streaming_handler {
                handler.emit_final_status(result.converged, result.cycles);
            } else if json {
                let final_facts: usize = ContextKey::iter()
                    .map(|key| result.context.get(key).len())
                    .sum();

                let mut facts: Vec<FactOutput> = Vec::new();
                let mut sequence = 0usize;
                for key in ContextKey::iter() {
                    for fact in result.context.get(key) {
                        sequence += 1;
                        facts.push(FactOutput {
                            sequence,
                            key: format!("{key:?}"),
                            id: fact.id.clone(),
                            content: fact.content.clone(),
                        });
                    }
                }

                let output = RunOutput {
                    run_id: run_id.clone(),
                    correlation_id: correlation_id.clone(),
                    timestamp: Utc::now().to_rfc3339(),
                    actor: ActorInfo {
                        actor_type: "system".to_string(),
                        device_id: device_id.clone(),
                        cli_version: env!("CARGO_PKG_VERSION").to_string(),
                    },
                    result: RunResultOutput {
                        converged: result.converged,
                        cycles: result.cycles,
                        total_facts: final_facts,
                    },
                    facts,
                };

                println!("{}", serde_json::to_string_pretty(&output)?);
            } else {
                let final_facts: usize = ContextKey::iter()
                    .map(|key| result.context.get(key).len())
                    .sum();

                println!("\n=== Convergence Result ===");
                println!("Run ID: {run_id}");
                println!("Correlation ID: {correlation_id}");
                println!("Converged: {}", result.converged);
                println!("Total Cycles: {}", result.cycles);
                println!("Total Facts: {final_facts}");
                println!("==========================\n");

                println!("=== Generated Facts ===\n");
                for key in ContextKey::iter() {
                    let facts = result.context.get(key);
                    if !facts.is_empty() {
                        println!("[{key:?}]");
                        for fact in facts {
                            println!("  {} | {}", fact.id, fact.content);
                        }
                        println!();
                    }
                }
                println!("=======================");
            }
        }

        Commands::Eval { command } => match command {
            EvalCommands::Run { eval_id, dir, mock } => {
                let dir_path = std::path::Path::new(&dir);

                let mut fixtures = evals::load_fixtures_from_dir(dir_path)?;

                if fixtures.is_empty() {
                    println!("No eval fixtures found in '{dir}'");
                    println!("Create JSON fixture files in the evals/ directory.");
                    return Ok(());
                }

                if let Some(ref id) = eval_id {
                    fixtures.retain(|f| f.eval_id == *id);
                    if fixtures.is_empty() {
                        println!("Eval '{id}' not found in '{dir}'");
                        return Ok(());
                    }
                }

                if mock {
                    for fixture in &mut fixtures {
                        fixture.use_mock_llm = true;
                    }
                }

                info!(count = fixtures.len(), "Running eval fixtures");

                let results = evals::run_evals(&fixtures).await;
                evals::print_results(&results);

                let all_passed = results.iter().all(|r| r.passed);
                if !all_passed {
                    std::process::exit(1);
                }
            }
            EvalCommands::List { dir } => {
                let dir_path = std::path::Path::new(&dir);
                let fixtures = evals::load_fixtures_from_dir(dir_path)?;

                if fixtures.is_empty() {
                    println!("No eval fixtures found in '{dir}'");
                    return Ok(());
                }

                println!("\nAvailable eval fixtures:\n");
                for fixture in fixtures {
                    println!("  {} - {}", fixture.eval_id, fixture.description);
                    println!("    Pack: {}", fixture.pack);
                    println!("    Seeds: {}", fixture.seeds.len());
                    println!("    Mock LLM: {}", fixture.use_mock_llm);
                    println!();
                }
            }
        },
    }

    Ok(())
}

#[cfg(feature = "tui")]
/// Cleanup terminal on exit or panic
fn cleanup_terminal() {
    let _ = disable_raw_mode();
    let _ = execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture);
}

#[cfg(feature = "tui")]
/// Run the TUI application with proper terminal lifecycle management
async fn run_tui() -> Result<()> {
    let original_hook = panic::take_hook();
    panic::set_hook(Box::new(move |panic_info| {
        cleanup_terminal();
        original_hook(panic_info);
    }));

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let app = ui::App::new();
    let res = ui::run_app(&mut terminal, app).await;

    cleanup_terminal();
    terminal.show_cursor()?;

    if let Err(err) = res {
        eprintln!("Error: {err:?}");
    }

    Ok(())
}

/// Creates a chat backend from environment variables.
fn create_chat_backend() -> Arc<dyn DynChatBackend> {
    llm_backend::create_chat_backend_or_mock()
}
