// Copyright 2024-2026 Reflective Labs

//! Phase-4A: Complete adversarial analysis.
//!
//! Run with: cargo run --example phase4a_analysis --no-default-features --features ndarray

use converge_llm::LlmResult;
use converge_llm::adversarial::{AdversarialHarness, ScenarioCategory};
use converge_llm::chain::{ChainConfig, ChainEngine, ChainExecutor};
use converge_llm::contract_stress::run_output_stress_tests;
use converge_llm::inference::InferenceEnvelope;
use converge_llm::prompt::PromptStack;

/// Mock engine for contract testing (deterministic outputs based on input patterns)
struct MockContractEngine;

impl ChainEngine for MockContractEngine {
    fn generate(
        &mut self,
        stack: &PromptStack,
        _envelope: &InferenceEnvelope,
    ) -> LlmResult<String> {
        let rendered = stack.render();

        // Analyze the state injection to determine response
        let is_contradictory = rendered.contains("contradictory")
            || (rendered.contains("mae")
                && rendered.contains("success_ratio")
                && rendered.contains("0.01")
                && rendered.contains("0.1"));

        let is_underspecified = rendered.contains("underspecified") || rendered.contains("missing");

        let is_boundary = rendered.contains("boundary")
            || rendered.contains("0.7499")
            || rendered.contains("0.7500");

        let is_extreme =
            rendered.contains("extreme") || rendered.contains("-0.5") || rendered.contains("999");

        let is_semantic_adversarial = rendered.contains("semantic")
            || rendered.contains("negative") && rendered.contains("mae");

        // Generate appropriate responses based on task
        if rendered.contains("Reasoning") || rendered.contains("reason") {
            if is_contradictory {
                Ok("Step 1: MAE is 0.01 which is excellent. Step 2: Success ratio is 0.1 which is poor. \
                    Step 3: These metrics contradict each other - low error but low success. \
                    UNCERTAIN: Cannot determine deployment readiness due to conflicting signals.".into())
            } else if is_underspecified {
                Ok(
                    "UNCERTAIN: Insufficient data to complete analysis. Missing critical metrics."
                        .into(),
                )
            } else if is_extreme {
                Ok("Step 1: Detected extreme values in input. Step 2: Values outside normal operating range. \
                    UNCERTAIN: Cannot reason about extreme inputs without calibration data.".into())
            } else if is_semantic_adversarial {
                Ok("Step 1: MAE is negative which is impossible. Step 2: This indicates data corruption. \
                    CONCLUSION: Reject input as semantically invalid.".into())
            } else if is_boundary {
                Ok("Step 1: Value is exactly at decision boundary. Step 2: Small perturbation could flip decision. \
                    CONCLUSION: Recommend conservative approach due to boundary proximity.".into())
            } else {
                Ok(
                    "Step 1: Analyzing provided metrics. Step 2: All values within normal ranges. \
                    CONCLUSION: System is operating normally."
                        .into(),
                )
            }
        } else if rendered.contains("Evaluation") || rendered.contains("evaluate") {
            if is_contradictory {
                Ok(
                    "Score: 0.5 (confidence: 0.3). The conflicting metrics make it difficult to \
                    assess overall system health. Low confidence due to contradictory signals \
                    between error metrics and success ratios."
                        .into(),
                )
            } else if is_underspecified {
                Ok(
                    "Score: 0.0 (confidence: 0.1). Cannot evaluate without required metrics. \
                    Insufficient data prevents meaningful assessment of deployment readiness."
                        .into(),
                )
            } else if is_extreme {
                Ok("Score: 0.2 (confidence: 0.4). Extreme values detected suggest system anomaly. \
                    Cannot provide high confidence evaluation with outlier data present.".into())
            } else if is_semantic_adversarial {
                Ok(
                    "Score: 0.0 (confidence: 0.9). Invalid input data detected (negative MAE). \
                    High confidence that this data should not be used for deployment decisions."
                        .into(),
                )
            } else if is_boundary {
                Ok(
                    "Score: 0.75 (confidence: 0.6). Value is at decision boundary threshold. \
                    Moderate confidence due to proximity to critical threshold value."
                        .into(),
                )
            } else {
                Ok(
                    "Score: 0.85 (confidence: 0.9). Metrics indicate healthy system operation. \
                    High confidence in positive assessment based on consistent indicators."
                        .into(),
                )
            }
        } else if rendered.contains("Planning") || rendered.contains("plan") {
            if is_contradictory {
                Ok("1. Investigate conflicting metrics before proceeding\n\
                    2. Run diagnostic checks on data pipeline\n\
                    3. Delay deployment decision until signals align"
                    .into())
            } else if is_underspecified {
                Ok("1. Gather missing metric data\n\
                    2. Re-evaluate once data is complete\n\
                    3. Do not proceed without full information"
                    .into())
            } else if is_extreme || is_semantic_adversarial {
                Ok("1. Flag input as anomalous\n\
                    2. Escalate to human review\n\
                    3. Do not automate decision on this input"
                    .into())
            } else if is_boundary {
                Ok("1. Apply conservative threshold interpretation\n\
                    2. Request additional validation data\n\
                    3. Proceed with caution if confirmed"
                    .into())
            } else {
                Ok("1. Proceed with standard deployment\n\
                    2. Monitor for 24 hours\n\
                    3. Confirm stable operation"
                    .into())
            }
        } else {
            Ok("Step 1: Processing. CONCLUSION: Complete.".into())
        }
    }
}

fn main() {
    println!("═══════════════════════════════════════════════════════════════");
    println!("              PHASE-4A: ADVERSARIAL ANALYSIS");
    println!("═══════════════════════════════════════════════════════════════\n");

    // Part 1: Run output contract stress tests
    println!("▶ PART 1: OUTPUT CONTRACT STRESS TESTS\n");
    let stress_report = run_output_stress_tests();
    println!("{}\n", stress_report.summary());

    // Part 2: Run adversarial scenarios with mock engine
    println!("▶ PART 2: ADVERSARIAL SCENARIO ANALYSIS\n");

    let mut harness = AdversarialHarness::new();
    let mut executor = ChainExecutor::new(MockContractEngine);
    let envelope = InferenceEnvelope::deterministic("phase4a:v1", 42);
    let config = ChainConfig::deployment();

    // Run all scenarios
    if let Err(e) = harness.run_all(&mut executor, &envelope, &config) {
        println!("Error running scenarios: {}", e);
    }

    let report = harness.report();
    println!("{}\n", report.summary());

    // Part 3: Build failure taxonomy
    println!("═══════════════════════════════════════════════════════════════");
    println!("                    FAILURE TAXONOMY");
    println!("═══════════════════════════════════════════════════════════════\n");

    build_failure_taxonomy(&report, &stress_report);

    // Part 4: Surprising behaviors
    println!("\n═══════════════════════════════════════════════════════════════");
    println!("                  SURPRISING BEHAVIORS");
    println!("═══════════════════════════════════════════════════════════════\n");

    analyze_surprising_behaviors(&report, &stress_report);

    // Part 5: Contract change recommendations
    println!("\n═══════════════════════════════════════════════════════════════");
    println!("              CONTRACT CHANGE RECOMMENDATIONS");
    println!("═══════════════════════════════════════════════════════════════\n");

    recommend_contract_changes(&report, &stress_report);
}

fn build_failure_taxonomy(
    adv_report: &converge_llm::adversarial::AdversarialReport,
    _stress_report: &converge_llm::contract_stress::OutputStressReport,
) {
    println!("┌─────────────────────────────────────────────────────────────┐");
    println!("│ CATEGORY 1: STRUCTURAL FAILURES (Contract Rejects)         │");
    println!("└─────────────────────────────────────────────────────────────┘");
    println!("  These are failures where the contract correctly rejects output.\n");

    println!("  • Missing conclusion marker (Reasoning)");
    println!("  • Missing reasoning steps before conclusion (Reasoning) [NEWLY ADDED]");
    println!("  • Score out of range (Evaluation)");
    println!("  • Missing confidence value (Evaluation)");
    println!("  • Missing justification text (Evaluation) [NEWLY TIGHTENED]");
    println!("  • Missing numbered steps (Planning)");
    println!("  • Too many steps (Planning/Reasoning)");
    println!("  • Missing required fields (Extraction)");

    println!("\n┌─────────────────────────────────────────────────────────────┐");
    println!("│ CATEGORY 2: SEMANTIC FAILURES (Contract Passes, Logic Fails)│");
    println!("└─────────────────────────────────────────────────────────────┘");
    println!("  These are KNOWN LIMITATIONS - contract passes but output is wrong.\n");

    println!("  • Hallucinated facts in reasoning (passes structure check)");
    println!("  • Miscalibrated confidence (high confidence, weak evidence)");
    println!("  • Hallucinated capabilities in planning (no registry check)");
    println!("  • Contradictory reasoning that reaches confident conclusion");

    println!("\n┌─────────────────────────────────────────────────────────────┐");
    println!("│ CATEGORY 3: INPUT-DRIVEN FAILURES (Bad State → Bad Output)  │");
    println!("└─────────────────────────────────────────────────────────────┘");
    println!("  These originate from adversarial input states.\n");

    // Analyze adversarial results by category
    for category in ScenarioCategory::all() {
        if let Some(stats) = adv_report.by_category.get(category) {
            println!("  {:?}:", category);
            println!("    - Total scenarios: {}", stats.total);
            println!("    - Completed: {}", stats.completed);
            println!("    - Matched expectations: {}", stats.matched_expectation);
            println!("    - Anomalies detected: {}", stats.anomaly_count);
        }
    }

    println!("\n┌─────────────────────────────────────────────────────────────┐");
    println!("│ CATEGORY 4: BOUNDARY AMBIGUITIES                            │");
    println!("└─────────────────────────────────────────────────────────────┘");
    println!("  Behaviors at decision thresholds.\n");

    println!("  • 0.749999 vs 0.750001: Should produce DIFFERENT decisions");
    println!("  • Words-per-score threshold: Currently 10 (could be 8 or 12)");
    println!("  • Step count: 'Step 1' vs 'First' vs 'Initially' all count");
}

fn analyze_surprising_behaviors(
    _adv_report: &converge_llm::adversarial::AdversarialReport,
    _stress_report: &converge_llm::contract_stress::OutputStressReport,
) {
    println!("1. UNCERTAIN WITHOUT STEPS IS VALID");
    println!("   ─────────────────────────────────");
    println!("   Discovery: When reasoning allows uncertainty, 'UNCERTAIN: reason'");
    println!("   is valid WITHOUT preceding reasoning steps.");
    println!("   ");
    println!("   Rationale: UNCERTAIN means 'I cannot complete reasoning', so");
    println!("   requiring steps before UNCERTAIN would force fake reasoning.");
    println!("   ");
    println!("   Impact: This is CORRECT behavior, not a bug.\n");

    println!("2. JUSTIFICATION LENGTH IS WORD-COUNT BASED");
    println!("   ────────────────────────────────────────");
    println!("   Discovery: Justification validation uses words-per-score (≥10),");
    println!("   not semantic analysis.");
    println!("   ");
    println!("   Example: 'Score: 0.5. Lorem ipsum dolor sit amet consectetur");
    println!("   adipiscing elit sed do eiusmod tempor.' passes justification check");
    println!("   despite being meaningless.");
    println!("   ");
    println!("   Impact: This is a KNOWN LIMITATION. Semantic justification");
    println!("   quality is a Phase-4B LoRA target.\n");

    println!("3. STEP DETECTION IS KEYWORD-BASED");
    println!("   ───────────────────────────────");
    println!("   Discovery: Reasoning steps are detected by keywords:");
    println!("   'step ', 'first,', 'second,', 'then,', 'finally,', 'therefore,'");
    println!("   ");
    println!("   Example: 'Step by step, I conclude X' counts as 1 step.");
    println!("   'Stepping forward, we see...' also triggers step detection.");
    println!("   ");
    println!("   Impact: Could cause false positives. Consider requiring");
    println!("   'Step N:' format for stricter detection.");
}

fn recommend_contract_changes(
    _adv_report: &converge_llm::adversarial::AdversarialReport,
    _stress_report: &converge_llm::contract_stress::OutputStressReport,
) {
    println!("CHANGE 1: ADD STRICT STEP FORMAT OPTION");
    println!("────────────────────────────────────────");
    println!("Current:  Step detection uses loose keyword matching");
    println!("Proposed: Add `strict_step_format: bool` to Reasoning contract");
    println!("");
    println!("When true, require 'Step N:' format (not 'first,' or 'then,')");
    println!("");
    println!("Rationale:");
    println!("  - Current detection is too permissive for formal reasoning");
    println!("  - But loose matching is useful for natural language output");
    println!("  - Make it configurable per-contract");
    println!("");
    println!("Implementation:");
    println!("  OutputContract::Reasoning {{");
    println!("      requires_conclusion: true,");
    println!("      allows_uncertainty: true,");
    println!("      max_steps: Some(5),");
    println!("      strict_step_format: true,  // NEW");
    println!("  }}");
    println!("");

    println!("CHANGE 2: ADD MINIMUM SCORE COUNT TO EVALUATION");
    println!("────────────────────────────────────────────────");
    println!("Current:  Any score in range passes (even 0 scores with text)");
    println!("Proposed: Add `min_scores: usize` to Evaluation contract");
    println!("");
    println!("Rationale:");
    println!("  - Some evaluation tasks require exactly 1 score");
    println!("  - Others require multiple scores (A vs B comparison)");
    println!("  - Currently no way to enforce this");
    println!("");
    println!("Implementation:");
    println!("  OutputContract::Evaluation {{");
    println!("      score_range: (0.0, 1.0),");
    println!("      confidence_required: true,");
    println!("      justification_required: true,");
    println!("      min_scores: 1,  // NEW - require at least 1 score");
    println!("  }}");
}
