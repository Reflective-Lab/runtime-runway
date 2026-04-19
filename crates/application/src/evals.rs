// Copyright 2024-2026 Reflective Labs
// SPDX-License-Identifier: MIT

//! Eval Fixtures for Converge
//!
//! This module implements reproducible evaluation testing based on the
//! cross-platform contract pattern from iOS/Android implementations.
//!
//! Domain-specific agent registration for evals should be provided by
//! organism-application or via a plugin mechanism.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::{Duration, Instant};
use uuid::Uuid;

use converge_core::{Context as ConvergeContext, ContextKey, Engine};
use strum::IntoEnumIterator;

use crate::packs::SeedFact;

/// Expected outcomes for an eval
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalExpectation {
    #[serde(default)]
    pub converged: Option<bool>,
    #[serde(default)]
    pub max_cycles: Option<u32>,
    #[serde(default)]
    pub min_facts: Option<usize>,
    #[serde(default)]
    pub must_contain_facts: Vec<String>,
    #[serde(default)]
    pub must_not_contain_facts: Vec<String>,
    #[serde(default)]
    pub min_strategies: Option<usize>,
    #[serde(default)]
    pub min_evaluations: Option<usize>,
    #[serde(default)]
    pub max_latency_ms: Option<u64>,
    #[serde(default)]
    pub required_context_keys: Vec<String>,
}

/// An eval fixture defining a test scenario
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalFixture {
    pub eval_id: String,
    pub description: String,
    pub pack: String,
    pub seeds: Vec<SeedFact>,
    pub expected: EvalExpectation,
    #[serde(default)]
    pub use_mock_llm: bool,
}

/// Result of running an eval
#[derive(Debug, Clone)]
pub struct EvalResult {
    pub eval_id: String,
    pub run_id: Uuid,
    pub passed: bool,
    pub checks: Vec<EvalCheck>,
    pub cycles: u32,
    pub fact_count: usize,
    pub converged: bool,
    pub duration: Duration,
    pub error: Option<String>,
}

/// Individual check within an eval
#[derive(Debug, Clone)]
pub struct EvalCheck {
    pub name: String,
    pub passed: bool,
    pub expected: String,
    pub actual: String,
}

impl EvalResult {
    pub fn error(eval_id: &str, run_id: Uuid, error: String, duration: Duration) -> Self {
        Self {
            eval_id: eval_id.to_string(),
            run_id,
            passed: false,
            checks: vec![],
            cycles: 0,
            fact_count: 0,
            converged: false,
            duration,
            error: Some(error),
        }
    }
}

/// Load an eval fixture from a JSON file
pub fn load_fixture(path: &Path) -> Result<EvalFixture> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read fixture file: {}", path.display()))?;

    let fixture: EvalFixture = serde_json::from_str(&content)
        .with_context(|| format!("Failed to parse fixture JSON: {}", path.display()))?;

    Ok(fixture)
}

/// Load all fixtures from a directory
pub fn load_fixtures_from_dir(dir: &Path) -> Result<Vec<EvalFixture>> {
    let mut fixtures = Vec::new();

    if !dir.exists() {
        return Ok(fixtures);
    }

    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.extension().is_some_and(|e| e == "json") {
            match load_fixture(&path) {
                Ok(fixture) => fixtures.push(fixture),
                Err(e) => {
                    tracing::warn!(path = %path.display(), error = %e, "Failed to load fixture");
                }
            }
        }
    }

    fixtures.sort_by(|a, b| a.eval_id.cmp(&b.eval_id));

    Ok(fixtures)
}

/// Run a single eval fixture
pub async fn run_eval(fixture: &EvalFixture) -> EvalResult {
    let run_id = Uuid::new_v4();
    let start = Instant::now();

    tracing::info!(
        eval_id = %fixture.eval_id,
        run_id = %run_id,
        pack = %fixture.pack,
        "Starting eval run"
    );

    let mut context = ConvergeContext::new();
    for seed in &fixture.seeds {
        if let Err(e) = context.add_input(ContextKey::Seeds, &seed.id, &seed.content) {
            return EvalResult::error(
                &fixture.eval_id,
                run_id,
                format!("Failed to add seed: {e}"),
                start.elapsed(),
            );
        }
    }

    // Create engine - domain-specific agent registration should be provided
    // by organism-application or via a plugin mechanism
    let mut engine = Engine::new();

    let result = match engine.run(context).await {
        Ok(r) => r,
        Err(e) => {
            return EvalResult::error(
                &fixture.eval_id,
                run_id,
                format!("Engine run failed: {e}"),
                start.elapsed(),
            );
        }
    };

    let duration = start.elapsed();

    let all_facts: Vec<_> = ContextKey::iter()
        .flat_map(|key| result.context.get(key).to_vec())
        .collect();

    let fact_count = all_facts.len();
    let strategy_count = result.context.get(ContextKey::Strategies).len();
    let evaluation_count = result.context.get(ContextKey::Evaluations).len();

    let mut checks = Vec::new();
    let expected = &fixture.expected;

    if let Some(expected_converged) = expected.converged {
        checks.push(EvalCheck {
            name: "converged".to_string(),
            passed: result.converged == expected_converged,
            expected: expected_converged.to_string(),
            actual: result.converged.to_string(),
        });
    }

    if let Some(max_cycles) = expected.max_cycles {
        checks.push(EvalCheck {
            name: "max_cycles".to_string(),
            passed: result.cycles <= max_cycles,
            expected: format!("<= {max_cycles}"),
            actual: result.cycles.to_string(),
        });
    }

    if let Some(min_facts) = expected.min_facts {
        checks.push(EvalCheck {
            name: "min_facts".to_string(),
            passed: fact_count >= min_facts,
            expected: format!(">= {min_facts}"),
            actual: fact_count.to_string(),
        });
    }

    if let Some(min_strategies) = expected.min_strategies {
        checks.push(EvalCheck {
            name: "min_strategies".to_string(),
            passed: strategy_count >= min_strategies,
            expected: format!(">= {min_strategies}"),
            actual: strategy_count.to_string(),
        });
    }

    if let Some(min_evaluations) = expected.min_evaluations {
        checks.push(EvalCheck {
            name: "min_evaluations".to_string(),
            passed: evaluation_count >= min_evaluations,
            expected: format!(">= {min_evaluations}"),
            actual: evaluation_count.to_string(),
        });
    }

    if let Some(max_latency_ms) = expected.max_latency_ms {
        let actual_ms = u64::try_from(duration.as_millis()).unwrap_or(u64::MAX);
        checks.push(EvalCheck {
            name: "max_latency_ms".to_string(),
            passed: actual_ms <= max_latency_ms,
            expected: format!("<= {max_latency_ms}ms"),
            actual: format!("{actual_ms}ms"),
        });
    }

    for fact_prefix in &expected.must_contain_facts {
        let found = all_facts.iter().any(|f| f.id.starts_with(fact_prefix));
        checks.push(EvalCheck {
            name: format!("contains:{fact_prefix}"),
            passed: found,
            expected: format!("fact with prefix '{fact_prefix}'"),
            actual: if found {
                "found".to_string()
            } else {
                "not found".to_string()
            },
        });
    }

    for fact_prefix in &expected.must_not_contain_facts {
        let found = all_facts.iter().any(|f| f.id.starts_with(fact_prefix));
        checks.push(EvalCheck {
            name: format!("excludes:{fact_prefix}"),
            passed: !found,
            expected: format!("no fact with prefix '{fact_prefix}'"),
            actual: if found {
                "found (unexpected)".to_string()
            } else {
                "not found (good)".to_string()
            },
        });
    }

    for key_name in &expected.required_context_keys {
        let key = match key_name.as_str() {
            "Seeds" => Some(ContextKey::Seeds),
            "Signals" => Some(ContextKey::Signals),
            "Competitors" => Some(ContextKey::Competitors),
            "Strategies" => Some(ContextKey::Strategies),
            "Evaluations" => Some(ContextKey::Evaluations),
            "Hypotheses" => Some(ContextKey::Hypotheses),
            "Constraints" => Some(ContextKey::Constraints),
            _ => None,
        };

        if let Some(context_key) = key {
            let has_facts = !result.context.get(context_key).is_empty();
            checks.push(EvalCheck {
                name: format!("has_key:{key_name}"),
                passed: has_facts,
                expected: format!("{key_name} has facts"),
                actual: if has_facts {
                    "has facts".to_string()
                } else {
                    "empty".to_string()
                },
            });
        }
    }

    let passed = checks.iter().all(|c| c.passed);

    tracing::info!(
        eval_id = %fixture.eval_id,
        run_id = %run_id,
        passed = passed,
        cycles = result.cycles,
        facts = fact_count,
        duration_ms = duration.as_millis(),
        "Eval run completed"
    );

    EvalResult {
        eval_id: fixture.eval_id.clone(),
        run_id,
        passed,
        checks,
        cycles: result.cycles,
        fact_count,
        converged: result.converged,
        duration,
        error: None,
    }
}

/// Run multiple eval fixtures
pub async fn run_evals(fixtures: &[EvalFixture]) -> Vec<EvalResult> {
    let mut results = Vec::with_capacity(fixtures.len());
    for fixture in fixtures {
        results.push(run_eval(fixture).await);
    }
    results
}

/// Print eval results in a formatted way
pub fn print_results(results: &[EvalResult]) {
    let total = results.len();
    let passed = results.iter().filter(|r| r.passed).count();
    let failed = total - passed;

    println!("\n=== Eval Results ===\n");

    for result in results {
        let status = if result.passed { "PASS" } else { "FAIL" };
        let status_color = if result.passed {
            "\x1b[32m"
        } else {
            "\x1b[31m"
        };
        let reset = "\x1b[0m";

        println!(
            "[{}{}{}] {} ({}ms, {} cycles, {} facts)",
            status_color,
            status,
            reset,
            result.eval_id,
            result.duration.as_millis(),
            result.cycles,
            result.fact_count,
        );

        if let Some(ref error) = result.error {
            println!("      Error: {error}");
        }

        for check in &result.checks {
            if !check.passed {
                println!(
                    "      \x1b[31mFAIL{}: {} - expected {}, got {}",
                    reset, check.name, check.expected, check.actual
                );
            }
        }
    }

    println!("\n===================");
    println!(
        "Total: {} | \x1b[32mPassed: {}\x1b[0m | {}Failed: {}\x1b[0m",
        total,
        passed,
        if failed > 0 { "\x1b[31m" } else { "\x1b[0m" },
        failed
    );
    println!("===================\n");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fixture_parsing() {
        let json = r#"{
            "eval_id": "test_001",
            "description": "Test fixture",
            "pack": "growth-strategy",
            "seeds": [
                {"id": "seed1", "content": "Test seed"}
            ],
            "expected": {
                "converged": true,
                "max_cycles": 10
            },
            "use_mock_llm": true
        }"#;

        let fixture: EvalFixture = serde_json::from_str(json).unwrap();
        assert_eq!(fixture.eval_id, "test_001");
        assert_eq!(fixture.seeds.len(), 1);
        assert_eq!(fixture.expected.converged, Some(true));
        assert!(fixture.use_mock_llm);
    }

    #[test]
    fn test_eval_check_logic() {
        let check = EvalCheck {
            name: "test".to_string(),
            passed: true,
            expected: "foo".to_string(),
            actual: "foo".to_string(),
        };
        assert!(check.passed);
    }
}
