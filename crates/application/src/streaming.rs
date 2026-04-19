// Copyright 2024-2026 Reflective Labs
// SPDX-License-Identifier: MIT
// See LICENSE file in the project root for full license information.

//! Streaming output handler for CLI.
//!
//! Implements the `StreamingCallback` trait to emit facts in real-time
//! as they are produced during convergence.
//!
//! # Output Formats
//!
//! ## Human-readable (default)
//! ```text
//! [cycle:1] fact:Seeds:seed-1 | Initial market data
//! [cycle:2] fact:Strategies:strategy-smb | Target SMB segment
//! [cycle:3] converged | 3 cycles, 5 facts
//! ```
//!
//! ## JSON (one object per line)
//! ```json
//! {"cycle":1,"type":"fact","key":"Seeds","id":"seed-1","content":"Initial market data"}
//! {"cycle":3,"type":"status","converged":true,"cycles":3,"facts":5}
//! ```

use std::io::{self, Write};
use std::sync::atomic::{AtomicUsize, Ordering};

use converge_core::{Fact, StreamingCallback};
use serde::Serialize;

/// Output format for streaming.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    /// Human-readable format with cycle prefixes.
    Human,
    /// JSON Lines format (one JSON object per line).
    Json,
}

/// Streaming output handler that implements `StreamingCallback`.
///
/// Writes facts to stdout as they arrive during convergence.
pub struct StreamingHandler {
    format: OutputFormat,
    fact_count: AtomicUsize,
}

impl StreamingHandler {
    /// Creates a new streaming handler with the specified output format.
    pub fn new(format: OutputFormat) -> Self {
        Self {
            format,
            fact_count: AtomicUsize::new(0),
        }
    }

    /// Creates a handler for human-readable output.
    pub fn human() -> Self {
        Self::new(OutputFormat::Human)
    }

    /// Creates a handler for JSON output.
    pub fn json() -> Self {
        Self::new(OutputFormat::Json)
    }

    /// Returns the total number of facts emitted.
    pub fn fact_count(&self) -> usize {
        self.fact_count.load(Ordering::SeqCst)
    }

    /// Emits the final status line.
    pub fn emit_final_status(&self, converged: bool, cycles: u32) {
        let facts = self.fact_count();
        match self.format {
            OutputFormat::Human => {
                let status = if converged { "converged" } else { "halted" };
                println!("[cycle:{cycles}] {status} | {cycles} cycles, {facts} facts");
            }
            OutputFormat::Json => {
                let status = StreamingStatus {
                    cycle: cycles,
                    event_type: "status".to_string(),
                    converged,
                    cycles,
                    facts,
                };
                if let Ok(json) = serde_json::to_string(&status) {
                    println!("{json}");
                }
            }
        }
    }
}

impl StreamingCallback for StreamingHandler {
    fn on_cycle_start(&self, _cycle: u32) {
        // Optionally emit cycle start marker
        // For now, we only emit facts and final status
    }

    fn on_fact(&self, cycle: u32, fact: &Fact) {
        self.fact_count.fetch_add(1, Ordering::SeqCst);

        match self.format {
            OutputFormat::Human => {
                // Format: [cycle:N] fact:Key:id | content
                let key_str = format!("{:?}", fact.key());
                println!(
                    "[cycle:{}] fact:{}:{} | {}",
                    cycle, key_str, fact.id, fact.content
                );
            }
            OutputFormat::Json => {
                let event = StreamingFact {
                    cycle,
                    event_type: "fact".to_string(),
                    key: format!("{:?}", fact.key()),
                    id: fact.id.clone(),
                    content: fact.content.clone(),
                };
                if let Ok(json) = serde_json::to_string(&event) {
                    println!("{json}");
                }
            }
        }

        // Flush to ensure immediate output
        let _ = io::stdout().flush();
    }

    fn on_cycle_end(&self, _cycle: u32, _facts_added: usize) {
        // Optionally emit cycle end marker
        // For now, we rely on emit_final_status for the summary
    }
}

/// JSON structure for fact events.
#[derive(Debug, Serialize)]
struct StreamingFact {
    cycle: u32,
    #[serde(rename = "type")]
    event_type: String,
    key: String,
    id: String,
    content: String,
}

/// JSON structure for status events.
#[derive(Debug, Serialize)]
struct StreamingStatus {
    cycle: u32,
    #[serde(rename = "type")]
    event_type: String,
    converged: bool,
    cycles: u32,
    facts: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use converge_core::{Context, ContextKey, Engine};

    fn promoted_fact(key: ContextKey, id: &str, content: &str) -> Fact {
        let mut ctx = Context::new();
        let _ = ctx.add_input(key, id, content);
        let rt = tokio::runtime::Runtime::new().expect("create runtime");
        rt.block_on(async {
            Engine::new()
                .run(ctx)
                .await
                .expect("should promote test input")
                .context
        })
        .get(key)
        .first()
        .expect("promoted fact should exist")
        .clone()
    }

    #[test]
    fn streaming_handler_counts_facts() {
        let handler = StreamingHandler::human();
        assert_eq!(handler.fact_count(), 0);

        let fact = promoted_fact(ContextKey::Seeds, "test", "test content");

        handler.on_fact(1, &fact);
        assert_eq!(handler.fact_count(), 1);

        handler.on_fact(2, &fact);
        assert_eq!(handler.fact_count(), 2);
    }
}
