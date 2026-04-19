// Copyright 2024-2026 Reflective Labs

//! Adversarial testing for decision chain contracts.
//!
//! This module generates edge cases, contradictions, and boundary conditions
//! to stress-test the contract system before introducing learning.
//!
//! # Purpose
//!
//! Phase-4A goal: Find where contracts break.
//!
//! We test:
//! - Contradictory metrics (good MAE + bad success_ratio)
//! - Boundary thresholds (0.749999 vs 0.750001)
//! - Underspecified states (missing critical fields)
//! - Semantically adversarial outputs (passes structure, fails logic)
//!
//! # Usage
//!
//! ```ignore
//! let harness = AdversarialHarness::new();
//!
//! for scenario in harness.scenarios() {
//!     let result = executor.run(&scenario.state, &envelope, &config)?;
//!     harness.record(scenario, result);
//! }
//!
//! let report = harness.report();
//! println!("{}", report.summary());
//! ```

use crate::chain::{ChainConfig, ChainEngine, ChainExecutor};
use crate::inference::InferenceEnvelope;
use crate::prompt::StateInjection;
use crate::trace::{DecisionChain, DecisionStep};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Categories of adversarial scenarios.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ScenarioCategory {
    /// Metrics that contradict each other
    Contradictory,
    /// Values exactly on decision boundaries
    Boundary,
    /// Missing critical information
    Underspecified,
    /// Technically valid but semantically problematic
    SemanticAdversarial,
    /// Extreme values (very high/low)
    Extreme,
    /// Normal baseline for comparison
    Baseline,
}

impl ScenarioCategory {
    /// All categories for iteration.
    pub fn all() -> &'static [ScenarioCategory] {
        &[
            Self::Contradictory,
            Self::Boundary,
            Self::Underspecified,
            Self::SemanticAdversarial,
            Self::Extreme,
            Self::Baseline,
        ]
    }
}

/// A single adversarial test scenario.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdversarialScenario {
    /// Unique identifier
    pub id: String,
    /// Human-readable name
    pub name: String,
    /// Category of adversarial condition
    pub category: ScenarioCategory,
    /// The state to inject
    pub state: StateInjection,
    /// What behavior we expect
    pub expected_behavior: ExpectedBehavior,
    /// Description of what this tests
    pub description: String,
}

/// Expected behavior for a scenario.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ExpectedBehavior {
    /// Should complete successfully
    Complete,
    /// Should fail at a specific step
    FailAt(DecisionStep),
    /// Should flag uncertainty
    FlagUncertainty,
    /// Behavior is undefined (we're probing)
    Probe,
}

/// Result of running an adversarial scenario.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioResult {
    /// The scenario that was run
    pub scenario_id: String,
    /// Whether it completed
    pub completed: bool,
    /// Where it failed (if applicable)
    pub failed_at: Option<DecisionStep>,
    /// Whether the result matched expectations
    pub matched_expectation: bool,
    /// The full decision chain
    pub chain: DecisionChain,
    /// Any anomalies detected
    pub anomalies: Vec<Anomaly>,
}

/// An anomaly detected during scenario execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Anomaly {
    /// Type of anomaly
    pub anomaly_type: AnomalyType,
    /// Which step it occurred at
    pub step: Option<DecisionStep>,
    /// Description
    pub description: String,
    /// Severity (1-5, 5 being most severe)
    pub severity: u8,
}

/// Types of anomalies we track.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AnomalyType {
    /// Contract passed but output is logically inconsistent
    LogicalInconsistency,
    /// Contract failed but output seems reasonable
    FalseNegative,
    /// Output contradicts input state
    StateContradiction,
    /// Confidence doesn't match evidence
    MiscalibratedConfidence,
    /// Missing expected content
    MissingContent,
    /// Unexpected content present
    UnexpectedContent,
}

/// Generates adversarial scenarios.
pub struct ScenarioGenerator {
    scenarios: Vec<AdversarialScenario>,
}

impl Default for ScenarioGenerator {
    fn default() -> Self {
        Self::new()
    }
}

impl ScenarioGenerator {
    /// Create a new generator with all built-in scenarios.
    #[must_use]
    pub fn new() -> Self {
        let mut scenarios = Vec::new();

        // Baseline scenarios (for comparison)
        scenarios.extend(Self::baseline_scenarios());

        // Contradictory scenarios
        scenarios.extend(Self::contradictory_scenarios());

        // Boundary scenarios
        scenarios.extend(Self::boundary_scenarios());

        // Underspecified scenarios
        scenarios.extend(Self::underspecified_scenarios());

        // Semantic adversarial scenarios
        scenarios.extend(Self::semantic_adversarial_scenarios());

        // Extreme value scenarios
        scenarios.extend(Self::extreme_scenarios());

        Self { scenarios }
    }

    /// Get all scenarios.
    #[must_use]
    pub fn scenarios(&self) -> &[AdversarialScenario] {
        &self.scenarios
    }

    /// Get scenarios by category.
    #[must_use]
    pub fn by_category(&self, category: ScenarioCategory) -> Vec<&AdversarialScenario> {
        self.scenarios
            .iter()
            .filter(|s| s.category == category)
            .collect()
    }

    fn baseline_scenarios() -> Vec<AdversarialScenario> {
        vec![
            AdversarialScenario {
                id: "baseline-good".into(),
                name: "Good metrics baseline".into(),
                category: ScenarioCategory::Baseline,
                state: StateInjection::new()
                    .with_scalar("mae", 0.08)
                    .with_scalar("success_ratio", 0.92)
                    .with_scalar("val_rows", 1000.0)
                    .with_flag("meets_threshold", true)
                    .with_flag("has_nulls", false),
                expected_behavior: ExpectedBehavior::Complete,
                description: "Standard good metrics - should complete successfully".into(),
            },
            AdversarialScenario {
                id: "baseline-bad".into(),
                name: "Bad metrics baseline".into(),
                category: ScenarioCategory::Baseline,
                state: StateInjection::new()
                    .with_scalar("mae", 0.45)
                    .with_scalar("success_ratio", 0.55)
                    .with_scalar("val_rows", 1000.0)
                    .with_flag("meets_threshold", false)
                    .with_flag("has_nulls", false),
                expected_behavior: ExpectedBehavior::Complete,
                description: "Standard bad metrics - should complete with negative assessment"
                    .into(),
            },
        ]
    }

    fn contradictory_scenarios() -> Vec<AdversarialScenario> {
        vec![
            AdversarialScenario {
                id: "contradict-mae-success".into(),
                name: "Good MAE, bad success ratio".into(),
                category: ScenarioCategory::Contradictory,
                state: StateInjection::new()
                    .with_scalar("mae", 0.05) // Excellent
                    .with_scalar("success_ratio", 0.30) // Terrible
                    .with_flag("meets_threshold", true), // Contradicts success_ratio
                expected_behavior: ExpectedBehavior::FlagUncertainty,
                description: "MAE suggests good fit, success_ratio suggests failure. \
                              Model should recognize contradiction."
                    .into(),
            },
            AdversarialScenario {
                id: "contradict-flag-metric".into(),
                name: "Flag contradicts metric".into(),
                category: ScenarioCategory::Contradictory,
                state: StateInjection::new()
                    .with_scalar("success_ratio", 0.95)
                    .with_flag("meets_threshold", false), // Contradicts high success
                expected_behavior: ExpectedBehavior::FlagUncertainty,
                description: "High success ratio but threshold flag is false. \
                              Should question the data consistency."
                    .into(),
            },
            AdversarialScenario {
                id: "contradict-drift-stable".into(),
                name: "Drift detected but metrics stable".into(),
                category: ScenarioCategory::Contradictory,
                state: StateInjection::new()
                    .with_scalar("mae", 0.10)
                    .with_scalar("success_ratio", 0.85)
                    .with_flag("drift_detected", true) // Drift!
                    .with_flag("metrics_stable", true), // But stable?
                expected_behavior: ExpectedBehavior::FlagUncertainty,
                description: "Drift detected contradicts stable metrics flag.".into(),
            },
        ]
    }

    fn boundary_scenarios() -> Vec<AdversarialScenario> {
        vec![
            AdversarialScenario {
                id: "boundary-just-under".into(),
                name: "Just under threshold".into(),
                category: ScenarioCategory::Boundary,
                state: StateInjection::new()
                    .with_scalar("success_ratio", 0.749999)
                    .with_flag("meets_threshold", false),
                expected_behavior: ExpectedBehavior::Probe,
                description: "Success ratio epsilon below 0.75 threshold. \
                              Tests boundary sensitivity."
                    .into(),
            },
            AdversarialScenario {
                id: "boundary-just-over".into(),
                name: "Just over threshold".into(),
                category: ScenarioCategory::Boundary,
                state: StateInjection::new()
                    .with_scalar("success_ratio", 0.750001)
                    .with_flag("meets_threshold", true),
                expected_behavior: ExpectedBehavior::Probe,
                description: "Success ratio epsilon above 0.75 threshold. \
                              Tests boundary sensitivity."
                    .into(),
            },
            AdversarialScenario {
                id: "boundary-exact".into(),
                name: "Exactly at threshold".into(),
                category: ScenarioCategory::Boundary,
                state: StateInjection::new()
                    .with_scalar("success_ratio", 0.75)
                    .with_flag("meets_threshold", true), // Edge case: true or false?
                expected_behavior: ExpectedBehavior::Probe,
                description: "Exactly at 0.75 threshold. How does model handle equality?".into(),
            },
            AdversarialScenario {
                id: "boundary-zero-mae".into(),
                name: "Zero MAE".into(),
                category: ScenarioCategory::Boundary,
                state: StateInjection::new()
                    .with_scalar("mae", 0.0)
                    .with_scalar("success_ratio", 1.0)
                    .with_flag("meets_threshold", true),
                expected_behavior: ExpectedBehavior::Probe,
                description: "Perfect metrics. Should this raise suspicion of overfitting?".into(),
            },
        ]
    }

    fn underspecified_scenarios() -> Vec<AdversarialScenario> {
        vec![
            AdversarialScenario {
                id: "underspec-no-metrics".into(),
                name: "No actual metrics".into(),
                category: ScenarioCategory::Underspecified,
                state: StateInjection::new()
                    .with_scalar("iteration", 5.0)
                    .with_scalar("epoch", 10.0),
                // No MAE, no success_ratio, no flags
                expected_behavior: ExpectedBehavior::FlagUncertainty,
                description: "Only iteration info, no performance metrics. \
                              Should refuse to make deployment decision."
                    .into(),
            },
            AdversarialScenario {
                id: "underspec-single-metric".into(),
                name: "Single metric only".into(),
                category: ScenarioCategory::Underspecified,
                state: StateInjection::new().with_scalar("mae", 0.12),
                expected_behavior: ExpectedBehavior::FlagUncertainty,
                description: "Only MAE provided. Insufficient for deployment decision.".into(),
            },
            AdversarialScenario {
                id: "underspec-empty".into(),
                name: "Empty state".into(),
                category: ScenarioCategory::Underspecified,
                state: StateInjection::new(),
                expected_behavior: ExpectedBehavior::FailAt(DecisionStep::Reasoning),
                description: "Completely empty state. Should fail early.".into(),
            },
        ]
    }

    fn semantic_adversarial_scenarios() -> Vec<AdversarialScenario> {
        vec![
            AdversarialScenario {
                id: "semantic-negative-mae".into(),
                name: "Negative MAE".into(),
                category: ScenarioCategory::SemanticAdversarial,
                state: StateInjection::new()
                    .with_scalar("mae", -0.15) // Invalid: MAE can't be negative
                    .with_scalar("success_ratio", 0.85),
                expected_behavior: ExpectedBehavior::FlagUncertainty,
                description: "Negative MAE is mathematically impossible. \
                              Model should recognize data error."
                    .into(),
            },
            AdversarialScenario {
                id: "semantic-ratio-over-one".into(),
                name: "Success ratio over 100%".into(),
                category: ScenarioCategory::SemanticAdversarial,
                state: StateInjection::new()
                    .with_scalar("mae", 0.10)
                    .with_scalar("success_ratio", 1.15), // Invalid: ratio can't exceed 1.0
                expected_behavior: ExpectedBehavior::FlagUncertainty,
                description: "Success ratio > 1.0 is impossible. Data error.".into(),
            },
            AdversarialScenario {
                id: "semantic-zero-samples".into(),
                name: "Zero validation samples".into(),
                category: ScenarioCategory::SemanticAdversarial,
                state: StateInjection::new()
                    .with_scalar("mae", 0.10)
                    .with_scalar("success_ratio", 0.85)
                    .with_scalar("val_rows", 0.0), // No samples!
                expected_behavior: ExpectedBehavior::FlagUncertainty,
                description: "Metrics from zero samples are meaningless.".into(),
            },
        ]
    }

    fn extreme_scenarios() -> Vec<AdversarialScenario> {
        vec![
            AdversarialScenario {
                id: "extreme-perfect".into(),
                name: "Suspiciously perfect".into(),
                category: ScenarioCategory::Extreme,
                state: StateInjection::new()
                    .with_scalar("mae", 0.001)
                    .with_scalar("success_ratio", 0.999)
                    .with_flag("meets_threshold", true),
                expected_behavior: ExpectedBehavior::Probe,
                description: "Near-perfect metrics. Overfitting or data leakage?".into(),
            },
            AdversarialScenario {
                id: "extreme-terrible".into(),
                name: "Catastrophically bad".into(),
                category: ScenarioCategory::Extreme,
                state: StateInjection::new()
                    .with_scalar("mae", 5.0)
                    .with_scalar("success_ratio", 0.01)
                    .with_flag("meets_threshold", false),
                expected_behavior: ExpectedBehavior::Complete,
                description: "Extremely bad metrics. Should recommend not deploying.".into(),
            },
            AdversarialScenario {
                id: "extreme-high-variance".into(),
                name: "High variance indicators".into(),
                category: ScenarioCategory::Extreme,
                state: StateInjection::new()
                    .with_scalar("mae", 0.15)
                    .with_scalar("mae_std", 0.50) // Std > mean
                    .with_scalar("success_ratio", 0.75),
                expected_behavior: ExpectedBehavior::FlagUncertainty,
                description: "Standard deviation exceeds mean. Unstable model.".into(),
            },
        ]
    }
}

/// Harness for running adversarial tests.
pub struct AdversarialHarness {
    generator: ScenarioGenerator,
    results: Vec<ScenarioResult>,
}

impl Default for AdversarialHarness {
    fn default() -> Self {
        Self::new()
    }
}

impl AdversarialHarness {
    /// Create a new harness.
    #[must_use]
    pub fn new() -> Self {
        Self {
            generator: ScenarioGenerator::new(),
            results: Vec::new(),
        }
    }

    /// Get all scenarios to run.
    #[must_use]
    pub fn scenarios(&self) -> &[AdversarialScenario] {
        self.generator.scenarios()
    }

    /// Run all scenarios against an executor.
    pub fn run_all<E: ChainEngine>(
        &mut self,
        executor: &mut ChainExecutor<E>,
        envelope: &InferenceEnvelope,
        config: &ChainConfig,
    ) -> Result<(), crate::error::LlmError> {
        for scenario in self.generator.scenarios().to_vec() {
            let chain = executor.run(&scenario.state, envelope, config)?;
            let result = self.analyze_result(&scenario, chain);
            self.results.push(result);
        }
        Ok(())
    }

    /// Record a single result.
    pub fn record(&mut self, scenario: &AdversarialScenario, chain: DecisionChain) {
        let result = self.analyze_result(scenario, chain);
        self.results.push(result);
    }

    /// Analyze a chain result for anomalies.
    fn analyze_result(
        &self,
        scenario: &AdversarialScenario,
        chain: DecisionChain,
    ) -> ScenarioResult {
        let mut anomalies = Vec::new();

        // Check if result matched expectation
        let matched = match &scenario.expected_behavior {
            ExpectedBehavior::Complete => chain.completed,
            ExpectedBehavior::FailAt(step) => chain.failed_at == Some(*step),
            ExpectedBehavior::FlagUncertainty => {
                // Check if any trace mentions uncertainty
                chain.traces.iter().any(|t| {
                    t.raw_output.to_lowercase().contains("uncertain")
                        || t.raw_output.to_lowercase().contains("insufficient")
                        || t.raw_output.to_lowercase().contains("cannot determine")
                })
            }
            ExpectedBehavior::Probe => true, // Probes always "match" - we're exploring
        };

        // Detect anomalies
        for trace in &chain.traces {
            // Check for logical inconsistencies
            if trace.is_valid() && self.detect_logical_inconsistency(scenario, &trace.raw_output) {
                anomalies.push(Anomaly {
                    anomaly_type: AnomalyType::LogicalInconsistency,
                    step: Some(trace.step),
                    description: "Output passed validation but contains logical errors".into(),
                    severity: 4,
                });
            }

            // Check for state contradictions
            if self.detect_state_contradiction(scenario, &trace.raw_output) {
                anomalies.push(Anomaly {
                    anomaly_type: AnomalyType::StateContradiction,
                    step: Some(trace.step),
                    description: "Output contradicts input state".into(),
                    severity: 3,
                });
            }
        }

        ScenarioResult {
            scenario_id: scenario.id.clone(),
            completed: chain.completed,
            failed_at: chain.failed_at,
            matched_expectation: matched,
            chain,
            anomalies,
        }
    }

    fn detect_logical_inconsistency(&self, scenario: &AdversarialScenario, output: &str) -> bool {
        let output_lower = output.to_lowercase();

        // If scenario has contradictory inputs but output doesn't acknowledge it
        if scenario.category == ScenarioCategory::Contradictory {
            let acknowledges_contradiction = output_lower.contains("contradict")
                || output_lower.contains("inconsistent")
                || output_lower.contains("conflict")
                || output_lower.contains("uncertain");

            // If good conclusion without acknowledging contradiction = inconsistency
            if output_lower.contains("recommend deploy") && !acknowledges_contradiction {
                return true;
            }
        }

        // If semantically invalid inputs but output treats them as valid
        if scenario.category == ScenarioCategory::SemanticAdversarial {
            let acknowledges_error = output_lower.contains("invalid")
                || output_lower.contains("error")
                || output_lower.contains("impossible")
                || output_lower.contains("cannot");

            if !acknowledges_error && output_lower.contains("conclusion") {
                return true;
            }
        }

        false
    }

    fn detect_state_contradiction(&self, scenario: &AdversarialScenario, output: &str) -> bool {
        let output_lower = output.to_lowercase();

        // Check if output claims something opposite to input
        if let Some(success_ratio) = scenario.state.scalars.get("success_ratio") {
            if let crate::prompt::StateValue::Float(ratio) = success_ratio {
                // Output says "high success" but ratio is low
                if *ratio < 0.5 && output_lower.contains("high success") {
                    return true;
                }
                // Output says "low success" but ratio is high
                if *ratio > 0.8 && output_lower.contains("low success") {
                    return true;
                }
            }
        }

        false
    }

    /// Generate a report of all results.
    #[must_use]
    pub fn report(&self) -> AdversarialReport {
        let mut by_category: HashMap<ScenarioCategory, CategoryStats> = HashMap::new();

        for result in &self.results {
            let scenario = self
                .generator
                .scenarios()
                .iter()
                .find(|s| s.id == result.scenario_id)
                .unwrap();

            let stats = by_category
                .entry(scenario.category)
                .or_insert_with(CategoryStats::default);

            stats.total += 1;
            if result.completed {
                stats.completed += 1;
            }
            if result.matched_expectation {
                stats.matched_expectation += 1;
            }
            stats.anomaly_count += result.anomalies.len();
        }

        AdversarialReport {
            total_scenarios: self.results.len(),
            total_completed: self.results.iter().filter(|r| r.completed).count(),
            total_matched: self
                .results
                .iter()
                .filter(|r| r.matched_expectation)
                .count(),
            total_anomalies: self.results.iter().map(|r| r.anomalies.len()).sum(),
            by_category,
            results: self.results.clone(),
        }
    }
}

/// Statistics for a category.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct CategoryStats {
    pub total: usize,
    pub completed: usize,
    pub matched_expectation: usize,
    pub anomaly_count: usize,
}

/// Complete adversarial test report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdversarialReport {
    pub total_scenarios: usize,
    pub total_completed: usize,
    pub total_matched: usize,
    pub total_anomalies: usize,
    pub by_category: HashMap<ScenarioCategory, CategoryStats>,
    pub results: Vec<ScenarioResult>,
}

impl AdversarialReport {
    /// Generate a summary string.
    #[must_use]
    pub fn summary(&self) -> String {
        let mut lines = vec![
            "═══════════════════════════════════════════════════════════════".to_string(),
            "                 ADVERSARIAL TEST REPORT".to_string(),
            "═══════════════════════════════════════════════════════════════".to_string(),
            format!(
                "Total: {} scenarios | {} completed | {} matched | {} anomalies",
                self.total_scenarios,
                self.total_completed,
                self.total_matched,
                self.total_anomalies
            ),
            "───────────────────────────────────────────────────────────────".to_string(),
        ];

        for category in ScenarioCategory::all() {
            if let Some(stats) = self.by_category.get(category) {
                let match_rate = if stats.total > 0 {
                    (stats.matched_expectation as f64 / stats.total as f64) * 100.0
                } else {
                    0.0
                };

                lines.push(format!(
                    "{:?}: {}/{} matched ({:.0}%), {} anomalies",
                    category,
                    stats.matched_expectation,
                    stats.total,
                    match_rate,
                    stats.anomaly_count
                ));
            }
        }

        // List anomalies
        let anomalies: Vec<_> = self
            .results
            .iter()
            .flat_map(|r| {
                r.anomalies
                    .iter()
                    .map(|a| (r.scenario_id.clone(), a.clone()))
            })
            .collect();

        if !anomalies.is_empty() {
            lines.push(
                "───────────────────────────────────────────────────────────────".to_string(),
            );
            lines.push("ANOMALIES:".to_string());
            for (scenario_id, anomaly) in anomalies {
                lines.push(format!(
                    "  [{scenario_id}] {:?} (severity {}): {}",
                    anomaly.anomaly_type, anomaly.severity, anomaly.description
                ));
            }
        }

        lines.push("═══════════════════════════════════════════════════════════════".to_string());
        lines.join("\n")
    }

    /// Get scenarios that didn't match expectations.
    #[must_use]
    pub fn failures(&self) -> Vec<&ScenarioResult> {
        self.results
            .iter()
            .filter(|r| !r.matched_expectation)
            .collect()
    }

    /// Get all anomalies.
    #[must_use]
    pub fn all_anomalies(&self) -> Vec<(&str, &Anomaly)> {
        self.results
            .iter()
            .flat_map(|r| r.anomalies.iter().map(|a| (r.scenario_id.as_str(), a)))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scenario_generator() {
        let generator = ScenarioGenerator::new();
        let scenarios = generator.scenarios();

        assert!(!scenarios.is_empty());

        // Check we have scenarios in each category
        for category in ScenarioCategory::all() {
            let count = generator.by_category(*category).len();
            assert!(count > 0, "No scenarios for {:?}", category);
        }
    }

    #[test]
    fn test_contradictory_scenarios() {
        let generator = ScenarioGenerator::new();
        let contradictory = generator.by_category(ScenarioCategory::Contradictory);

        assert!(contradictory.len() >= 3);

        // All should expect uncertainty
        for scenario in contradictory {
            assert!(
                matches!(
                    scenario.expected_behavior,
                    ExpectedBehavior::FlagUncertainty
                ),
                "Contradictory scenario {} should expect uncertainty",
                scenario.id
            );
        }
    }

    #[test]
    fn test_boundary_scenarios() {
        let generator = ScenarioGenerator::new();
        let boundary = generator.by_category(ScenarioCategory::Boundary);

        // Should have at least just-under, just-over, and exact
        assert!(boundary.len() >= 3);
    }

    #[test]
    fn test_report_generation() {
        let harness = AdversarialHarness::new();
        let report = harness.report();

        // Empty report should have zero totals
        assert_eq!(report.total_scenarios, 0);
        assert_eq!(report.total_anomalies, 0);

        // Summary should be valid
        let summary = report.summary();
        assert!(summary.contains("ADVERSARIAL TEST REPORT"));
    }

    #[test]
    fn test_semantic_adversarial_scenarios() {
        let generator = ScenarioGenerator::new();
        let semantic = generator.by_category(ScenarioCategory::SemanticAdversarial);

        // Check for negative MAE scenario
        let negative_mae = semantic.iter().find(|s| s.id == "semantic-negative-mae");
        assert!(negative_mae.is_some());

        // The state should have negative MAE
        if let Some(scenario) = negative_mae {
            let mae = scenario.state.scalars.get("mae");
            assert!(mae.is_some());
        }
    }
}
