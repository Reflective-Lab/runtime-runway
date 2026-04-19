// Copyright 2024-2026 Reflective Labs

//! Output contract stress tests.
//!
//! These tests probe the OUTPUT side of contracts, not input states.
//! They answer: "Does the contract catch this malformed output?"
//!
//! # Categories
//!
//! 1. **Reasoning stress**: Concludes without reasoning
//! 2. **Evaluation stress**: Scores without justification
//! 3. **Planning stress**: References non-existent capabilities
//!
//! # Interpretation
//!
//! If a test PASSES validation when it shouldn't:
//! → The contract is under-specified (tighten it)
//!
//! If a test FAILS validation when it shouldn't:
//! → The contract is over-specified (loosen it)

use crate::prompt::{OutputContract, ScoreCardinality, StepFormat};
use crate::validation::validate_output;
use serde::{Deserialize, Serialize};

/// A single output stress test case.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputStressCase {
    /// Unique identifier
    pub id: String,
    /// Human-readable name
    pub name: String,
    /// The malformed output to test
    pub output: String,
    /// The contract to test against
    pub contract: OutputContract,
    /// Should this pass or fail validation?
    pub expected_valid: bool,
    /// What this tests
    pub tests: String,
    /// What it means if expectation is wrong
    pub interpretation: String,
}

/// Results of running output stress tests.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputStressReport {
    /// Total cases run
    pub total: usize,
    /// Cases that matched expectation
    pub matched: usize,
    /// Cases where contract was too weak (passed when should fail)
    pub contract_too_weak: Vec<OutputStressCase>,
    /// Cases where contract was too strict (failed when should pass)
    pub contract_too_strict: Vec<OutputStressCase>,
}

impl OutputStressReport {
    /// Check if all tests passed as expected.
    #[must_use]
    pub fn all_passed(&self) -> bool {
        self.matched == self.total
    }

    /// Generate summary.
    #[must_use]
    pub fn summary(&self) -> String {
        let mut lines = vec![
            "═══════════════════════════════════════════════════════════════".to_string(),
            "              OUTPUT CONTRACT STRESS REPORT".to_string(),
            "═══════════════════════════════════════════════════════════════".to_string(),
            format!(
                "Total: {} | Matched: {} | Mismatched: {}",
                self.total,
                self.matched,
                self.total - self.matched
            ),
        ];

        if !self.contract_too_weak.is_empty() {
            lines.push(
                "───────────────────────────────────────────────────────────────".to_string(),
            );
            lines.push("CONTRACT TOO WEAK (passed when should fail):".to_string());
            for case in &self.contract_too_weak {
                lines.push(format!("  [{}] {}", case.id, case.name));
                lines.push(format!("    → {}", case.interpretation));
            }
        }

        if !self.contract_too_strict.is_empty() {
            lines.push(
                "───────────────────────────────────────────────────────────────".to_string(),
            );
            lines.push("CONTRACT TOO STRICT (failed when should pass):".to_string());
            for case in &self.contract_too_strict {
                lines.push(format!("  [{}] {}", case.id, case.name));
                lines.push(format!("    → {}", case.interpretation));
            }
        }

        lines.push("═══════════════════════════════════════════════════════════════".to_string());
        lines.join("\n")
    }
}

/// Run all output stress tests.
#[must_use]
pub fn run_output_stress_tests() -> OutputStressReport {
    let cases = all_stress_cases();
    let mut matched = 0;
    let mut too_weak = Vec::new();
    let mut too_strict = Vec::new();

    for case in cases {
        let result = validate_output(&case.output, &case.contract);
        let actual_valid = result.valid;

        if actual_valid == case.expected_valid {
            matched += 1;
        } else if actual_valid && !case.expected_valid {
            // Passed when should fail = contract too weak
            too_weak.push(case);
        } else {
            // Failed when should pass = contract too strict
            too_strict.push(case);
        }
    }

    OutputStressReport {
        total: all_stress_cases().len(),
        matched,
        contract_too_weak: too_weak,
        contract_too_strict: too_strict,
    }
}

/// Get all stress test cases.
#[must_use]
pub fn all_stress_cases() -> Vec<OutputStressCase> {
    let mut cases = Vec::new();
    cases.extend(reasoning_stress_cases());
    cases.extend(evaluation_stress_cases());
    cases.extend(planning_stress_cases());
    cases
}

/// Reasoning contract stress cases.
fn reasoning_stress_cases() -> Vec<OutputStressCase> {
    vec![
        // Should FAIL: Conclusion without reasoning
        OutputStressCase {
            id: "reasoning-no-steps".into(),
            name: "Conclusion without reasoning steps".into(),
            output: "CONCLUSION: Deploy the model.".into(),
            contract: OutputContract::Reasoning {
                requires_conclusion: true,
                allows_uncertainty: false,
                max_steps: Some(5),
                step_format: StepFormat::Loose,
            },
            expected_valid: false, // Should fail - no reasoning shown
            tests: "Does contract require reasoning steps, not just conclusion?".into(),
            interpretation: "Contract only checks for CONCLUSION marker, not reasoning process. \
                            Need to add step requirement.".into(),
        },

        // Should FAIL: Reasoning that hallucinates facts
        OutputStressCase {
            id: "reasoning-hallucinated-facts".into(),
            name: "Reasoning with invented metrics".into(),
            output: "Step 1: The accuracy is 99.5% (excellent). \
                    Step 2: The F1 score is 0.98. \
                    CONCLUSION: Model is production-ready.".into(),
            contract: OutputContract::Reasoning {
                requires_conclusion: true,
                allows_uncertainty: false,
                max_steps: Some(5),
                step_format: StepFormat::Loose,
            },
            expected_valid: true, // Will pass structurally - THIS IS A KNOWN LIMITATION
            tests: "Can contract detect hallucinated facts?".into(),
            interpretation: "Structural contracts cannot detect hallucination. \
                            This requires state-aware validation or learning.".into(),
        },

        // Should PASS: Proper reasoning with uncertainty
        OutputStressCase {
            id: "reasoning-proper-uncertainty".into(),
            name: "Proper reasoning with uncertainty flag".into(),
            output: "Step 1: MAE is 0.05 (good). \
                    Step 2: Success ratio is 0.3 (bad). \
                    Step 3: These metrics contradict each other. \
                    UNCERTAIN: Cannot determine deployment readiness due to conflicting signals.".into(),
            contract: OutputContract::Reasoning {
                requires_conclusion: true,
                allows_uncertainty: true,
                max_steps: Some(5),
                step_format: StepFormat::Loose,
            },
            expected_valid: true, // Should pass - proper uncertainty handling
            tests: "Does contract accept explicit uncertainty?".into(),
            interpretation: "Contract correctly allows uncertainty as valid conclusion.".into(),
        },

        // Should FAIL: Too many reasoning steps
        OutputStressCase {
            id: "reasoning-too-many-steps".into(),
            name: "Exceeds maximum reasoning steps".into(),
            output: "Step 1: First. Step 2: Second. Step 3: Third. Step 4: Fourth. \
                    Step 5: Fifth. Step 6: Sixth. CONCLUSION: Done.".into(),
            contract: OutputContract::Reasoning {
                requires_conclusion: true,
                allows_uncertainty: false,
                max_steps: Some(5),
                step_format: StepFormat::Loose,
            },
            expected_valid: false, // Should fail - too many steps
            tests: "Does contract enforce max_steps?".into(),
            interpretation: "Contract should limit reasoning depth.".into(),
        },

        // Should FAIL: Refuses to engage with insufficient data
        OutputStressCase {
            id: "reasoning-refuses-insufficient".into(),
            name: "Refuses without flagging uncertainty".into(),
            output: "I cannot make a determination without more data.".into(),
            contract: OutputContract::Reasoning {
                requires_conclusion: true,
                allows_uncertainty: true,
                max_steps: Some(5),
                step_format: StepFormat::Loose,
            },
            expected_valid: false, // Should fail - no CONCLUSION or UNCERTAIN marker
            tests: "Does contract require structured refusal?".into(),
            interpretation: "Refusals must use UNCERTAIN: marker, not free-form text.".into(),
        },

        // === NEW: StepFormat::StepNColon tests ===

        // Should PASS: Valid StepNColon format
        OutputStressCase {
            id: "reasoning-step-n-colon-valid".into(),
            name: "Valid Step N: format".into(),
            output: "Step 1: Analyze the metrics.\nStep 2: Compare against baseline.\nCONCLUSION: Proceed with deployment.".into(),
            contract: OutputContract::Reasoning {
                requires_conclusion: true,
                allows_uncertainty: false,
                max_steps: Some(5),
                step_format: StepFormat::StepNColon,
            },
            expected_valid: true,
            tests: "Does StepNColon accept valid step format?".into(),
            interpretation: "Contract correctly validates Step N: format.".into(),
        },

        // Should FAIL: Invalid step format for StepNColon
        OutputStressCase {
            id: "reasoning-step-n-colon-invalid".into(),
            name: "Invalid format for StepNColon".into(),
            output: "First, analyze the metrics. Second, compare against baseline. CONCLUSION: Proceed.".into(),
            contract: OutputContract::Reasoning {
                requires_conclusion: true,
                allows_uncertainty: false,
                max_steps: Some(5),
                step_format: StepFormat::StepNColon,
            },
            expected_valid: false,
            tests: "Does StepNColon reject non-Step N: format?".into(),
            interpretation: "Contract enforces strict Step N: format.".into(),
        },

        // Should PASS: Valid NumberedList format
        OutputStressCase {
            id: "reasoning-numbered-list-valid".into(),
            name: "Valid numbered list format".into(),
            output: "1. Analyze the metrics.\n2. Compare against baseline.\n3. Check for anomalies.\nCONCLUSION: Proceed with deployment.".into(),
            contract: OutputContract::Reasoning {
                requires_conclusion: true,
                allows_uncertainty: false,
                max_steps: Some(5),
                step_format: StepFormat::NumberedList,
            },
            expected_valid: true,
            tests: "Does NumberedList accept valid numbered format?".into(),
            interpretation: "Contract correctly validates numbered list format.".into(),
        },

        // Should FAIL: Invalid format for NumberedList
        OutputStressCase {
            id: "reasoning-numbered-list-invalid".into(),
            name: "Invalid format for NumberedList".into(),
            output: "Step 1: Analyze the metrics. Step 2: Compare. CONCLUSION: Proceed.".into(),
            contract: OutputContract::Reasoning {
                requires_conclusion: true,
                allows_uncertainty: false,
                max_steps: Some(5),
                step_format: StepFormat::NumberedList,
            },
            expected_valid: false,
            tests: "Does NumberedList reject Step N: format?".into(),
            interpretation: "Contract enforces strict numbered list format.".into(),
        },

        // === NEW: UNCERTAIN reason requirement tests ===

        // Should PASS: UNCERTAIN with reason
        OutputStressCase {
            id: "reasoning-uncertain-with-reason".into(),
            name: "UNCERTAIN with reason code".into(),
            output: "Step 1: Metrics show mixed signals.\nUNCERTAIN: Conflicting data between MAE and success ratio.".into(),
            contract: OutputContract::Reasoning {
                requires_conclusion: true,
                allows_uncertainty: true,
                max_steps: Some(5),
                step_format: StepFormat::Loose,
            },
            expected_valid: true,
            tests: "Does contract accept UNCERTAIN with reason?".into(),
            interpretation: "Contract correctly validates UNCERTAIN with reason text.".into(),
        },

        // Should FAIL: UNCERTAIN without reason
        OutputStressCase {
            id: "reasoning-uncertain-no-reason".into(),
            name: "UNCERTAIN without reason".into(),
            output: "Step 1: Metrics analyzed.\nUNCERTAIN".into(),
            contract: OutputContract::Reasoning {
                requires_conclusion: true,
                allows_uncertainty: true,
                max_steps: Some(5),
                step_format: StepFormat::Loose,
            },
            expected_valid: false,
            tests: "Does contract require UNCERTAIN to have a reason?".into(),
            interpretation: "Contract enforces UNCERTAIN must have a reason.".into(),
        },
    ]
}

/// Evaluation contract stress cases.
fn evaluation_stress_cases() -> Vec<OutputStressCase> {
    vec![
        // Should FAIL: Score without justification
        OutputStressCase {
            id: "eval-no-justification".into(),
            name: "Score without justification".into(),
            output: "Deployment readiness: 0.85 (confidence: 0.9)".into(),
            contract: OutputContract::Evaluation {
                score_range: (0.0, 1.0),
                confidence_required: true,
                justification_required: true,
                cardinality: ScoreCardinality::AtLeast(1),
                grounding_refs: vec![],
            },
            expected_valid: false, // Should fail - no justification
            tests: "Does contract enforce justification requirement?".into(),
            interpretation: "Contract must require justification text, not just scores.".into(),
        },
        // Should FAIL: Score without confidence
        OutputStressCase {
            id: "eval-no-confidence".into(),
            name: "Score without confidence".into(),
            output: "Deployment readiness: 0.85. The model performs well on validation data."
                .into(),
            contract: OutputContract::Evaluation {
                score_range: (0.0, 1.0),
                confidence_required: true,
                justification_required: true,
                cardinality: ScoreCardinality::AtLeast(1),
                grounding_refs: vec![],
            },
            expected_valid: false, // Should fail - no confidence
            tests: "Does contract enforce confidence requirement?".into(),
            interpretation: "Contract must detect missing confidence values.".into(),
        },
        // Should PASS: Complete evaluation
        OutputStressCase {
            id: "eval-complete".into(),
            name: "Complete evaluation with all components".into(),
            output: "Deployment readiness: 0.85 (confidence: 0.9). \
                    The model shows strong performance on validation metrics with \
                    low error rates and high success ratio."
                .into(),
            contract: OutputContract::Evaluation {
                score_range: (0.0, 1.0),
                confidence_required: true,
                justification_required: true,
                cardinality: ScoreCardinality::AtLeast(1),
                grounding_refs: vec![],
            },
            expected_valid: true, // Should pass
            tests: "Does contract accept complete evaluations?".into(),
            interpretation: "Contract correctly validates complete output.".into(),
        },
        // Should FAIL: Confidence higher than evidence warrants
        OutputStressCase {
            id: "eval-overconfident".into(),
            name: "High confidence with weak justification".into(),
            output: "Score: 0.5 (confidence: 0.99). Might work.".into(),
            contract: OutputContract::Evaluation {
                score_range: (0.0, 1.0),
                confidence_required: true,
                justification_required: true,
                cardinality: ScoreCardinality::AtLeast(1),
                grounding_refs: vec![],
            },
            expected_valid: true, // Will pass structurally - THIS IS A KNOWN LIMITATION
            tests: "Can contract detect miscalibrated confidence?".into(),
            interpretation: "Structural contracts cannot detect calibration. \
                            This is the primary LoRA target for Phase-4B."
                .into(),
        },
        // Should FAIL: Score outside valid range
        OutputStressCase {
            id: "eval-out-of-range".into(),
            name: "Score outside valid range".into(),
            output: "Score: 1.5 (confidence: 0.8). Excellent performance.".into(),
            contract: OutputContract::Evaluation {
                score_range: (0.0, 1.0),
                confidence_required: true,
                justification_required: true,
                cardinality: ScoreCardinality::AtLeast(1),
                grounding_refs: vec![],
            },
            expected_valid: false, // Should fail - score > 1.0
            tests: "Does contract enforce score bounds?".into(),
            interpretation: "Contract must reject out-of-range scores.".into(),
        },
        // === NEW: ScoreCardinality tests ===

        // Should PASS: AtLeast(2) with 3 scores
        OutputStressCase {
            id: "eval-cardinality-atleast-pass".into(),
            name: "AtLeast(2) with 3 scores".into(),
            output: "Accuracy: 0.85 (confidence: 0.9). Precision: 0.82 (confidence: 0.85). \
                    Recall: 0.78 (confidence: 0.8). All metrics show strong performance."
                .into(),
            contract: OutputContract::Evaluation {
                score_range: (0.0, 1.0),
                confidence_required: true,
                justification_required: true,
                cardinality: ScoreCardinality::AtLeast(2),
                grounding_refs: vec![],
            },
            expected_valid: true,
            tests: "Does AtLeast(n) accept n or more scores?".into(),
            interpretation: "Contract correctly validates score cardinality.".into(),
        },
        // Should FAIL: AtLeast(3) with only 2 scores
        OutputStressCase {
            id: "eval-cardinality-atleast-fail".into(),
            name: "AtLeast(3) with only 2 scores".into(),
            output: "Accuracy: 0.85 (confidence: 0.9). Precision: 0.82 (confidence: 0.85). \
                    Model shows promise."
                .into(),
            contract: OutputContract::Evaluation {
                score_range: (0.0, 1.0),
                confidence_required: true,
                justification_required: true,
                cardinality: ScoreCardinality::AtLeast(3),
                grounding_refs: vec![],
            },
            expected_valid: false,
            tests: "Does AtLeast(n) reject fewer than n scores?".into(),
            interpretation: "Contract enforces minimum score count.".into(),
        },
        // Should PASS: Exactly(2) with 2 scores
        OutputStressCase {
            id: "eval-cardinality-exactly-pass".into(),
            name: "Exactly(2) with 2 scores".into(),
            output: "Accuracy: 0.85 (confidence: 0.9). Precision: 0.82 (confidence: 0.85). \
                    Both metrics meet threshold."
                .into(),
            contract: OutputContract::Evaluation {
                score_range: (0.0, 1.0),
                confidence_required: true,
                justification_required: true,
                cardinality: ScoreCardinality::Exactly(2),
                grounding_refs: vec![],
            },
            expected_valid: true,
            tests: "Does Exactly(n) accept exactly n scores?".into(),
            interpretation: "Contract correctly validates exact score count.".into(),
        },
        // Should FAIL: Exactly(2) with 3 scores
        OutputStressCase {
            id: "eval-cardinality-exactly-fail".into(),
            name: "Exactly(2) with 3 scores".into(),
            output: "Accuracy: 0.85 (confidence: 0.9). Precision: 0.82 (confidence: 0.85). \
                    Recall: 0.78 (confidence: 0.8). Extra score provided."
                .into(),
            contract: OutputContract::Evaluation {
                score_range: (0.0, 1.0),
                confidence_required: true,
                justification_required: true,
                cardinality: ScoreCardinality::Exactly(2),
                grounding_refs: vec![],
            },
            expected_valid: false,
            tests: "Does Exactly(n) reject more than n scores?".into(),
            interpretation: "Contract enforces exact score count.".into(),
        },
        // === NEW: Grounding reference tests ===

        // Should PASS: Evaluation with grounding refs present
        OutputStressCase {
            id: "eval-grounding-refs-pass".into(),
            name: "Evaluation references required terms".into(),
            output: "Score: 0.85 (confidence: 0.9). The MAE of 0.05 indicates low error, \
                    and the success_ratio of 0.92 shows strong performance."
                .into(),
            contract: OutputContract::Evaluation {
                score_range: (0.0, 1.0),
                confidence_required: true,
                justification_required: true,
                cardinality: ScoreCardinality::AtLeast(1),
                grounding_refs: vec!["MAE".into(), "success_ratio".into()],
            },
            expected_valid: true,
            tests: "Does contract accept justification with grounding refs?".into(),
            interpretation: "Contract validates presence of grounding references.".into(),
        },
        // Should FAIL: Evaluation missing grounding refs
        OutputStressCase {
            id: "eval-grounding-refs-fail".into(),
            name: "Evaluation missing required terms".into(),
            output: "Score: 0.85 (confidence: 0.9). The model performs well overall.".into(),
            contract: OutputContract::Evaluation {
                score_range: (0.0, 1.0),
                confidence_required: true,
                justification_required: true,
                cardinality: ScoreCardinality::AtLeast(1),
                grounding_refs: vec!["MAE".into(), "success_ratio".into()],
            },
            expected_valid: false,
            tests: "Does contract reject justification missing grounding refs?".into(),
            interpretation: "Contract enforces grounding reference requirement.".into(),
        },
    ]
}

/// Planning contract stress cases.
fn planning_stress_cases() -> Vec<OutputStressCase> {
    vec![
        // Should FAIL: Plan without numbered steps
        OutputStressCase {
            id: "plan-no-numbers".into(),
            name: "Plan without numbered steps".into(),
            output: "First, validate the model. Then deploy to staging. Finally, monitor.".into(),
            contract: OutputContract::Planning {
                requires_ordered_steps: true,
                max_steps: 10,
                requires_capability_refs: false,
                allowed_capabilities: vec![],
            },
            expected_valid: false, // Should fail - no numbered steps
            tests: "Does contract require explicit numbering?".into(),
            interpretation: "Contract must enforce step numbering for ordered plans.".into(),
        },
        // Should PASS: Proper numbered plan
        OutputStressCase {
            id: "plan-proper".into(),
            name: "Properly numbered plan".into(),
            output: "1. Validate model on holdout set\n\
                    2. Deploy to staging environment\n\
                    3. Monitor for 24 hours\n\
                    4. Proceed to production if stable"
                .into(),
            contract: OutputContract::Planning {
                requires_ordered_steps: true,
                max_steps: 10,
                requires_capability_refs: false,
                allowed_capabilities: vec![],
            },
            expected_valid: true, // Should pass
            tests: "Does contract accept proper plans?".into(),
            interpretation: "Contract correctly validates proper plan structure.".into(),
        },
        // Should FAIL: Plan exceeds max steps
        OutputStressCase {
            id: "plan-too-many-steps".into(),
            name: "Plan exceeds maximum steps".into(),
            output: "1. A\n2. B\n3. C\n4. D\n5. E\n6. F\n7. G\n8. H\n9. I\n10. J\n11. K".into(),
            contract: OutputContract::Planning {
                requires_ordered_steps: true,
                max_steps: 10,
                requires_capability_refs: false,
                allowed_capabilities: vec![],
            },
            expected_valid: false, // Should fail - 11 > 10 steps
            tests: "Does contract enforce max_steps?".into(),
            interpretation: "Contract must limit plan complexity.".into(),
        },
        // Should FAIL: Plan references non-existent capability
        OutputStressCase {
            id: "plan-hallucinated-capability".into(),
            name: "Plan references non-existent capability".into(),
            output: "1. Run quantum optimization on the model\n\
                    2. Deploy to Mars datacenter\n\
                    3. Activate neural link interface"
                .into(),
            contract: OutputContract::Planning {
                requires_ordered_steps: true,
                max_steps: 10,
                requires_capability_refs: false, // Not enforced yet
                allowed_capabilities: vec![],
            },
            expected_valid: true, // Will pass - capability refs not enforced
            tests: "Can contract detect hallucinated capabilities?".into(),
            interpretation: "Without requires_capability_refs=true and a capability registry, \
                            planning can hallucinate actions. This is a system-level issue."
                .into(),
        },
        // Should FAIL: Empty plan
        OutputStressCase {
            id: "plan-empty".into(),
            name: "Empty plan".into(),
            output: "No action required.".into(),
            contract: OutputContract::Planning {
                requires_ordered_steps: true,
                max_steps: 10,
                requires_capability_refs: false,
                allowed_capabilities: vec![],
            },
            expected_valid: false, // Should fail - no steps
            tests: "Does contract reject empty plans?".into(),
            interpretation: "Contract must require at least one step.".into(),
        },
        // Edge case: Plan that should abort
        OutputStressCase {
            id: "plan-abort".into(),
            name: "Plan that recommends no action".into(),
            output: "1. Do not deploy - metrics indicate model is not ready\n\
                    2. Return to training with additional data"
                .into(),
            contract: OutputContract::Planning {
                requires_ordered_steps: true,
                max_steps: 10,
                requires_capability_refs: false,
                allowed_capabilities: vec![],
            },
            expected_valid: true, // Should pass - negative action is still action
            tests: "Does contract accept conservative/abort plans?".into(),
            interpretation: "Contract correctly accepts negative recommendations as valid plans."
                .into(),
        },
        // === NEW: Capability registry tests ===

        // Should PASS: Plan using valid capabilities
        OutputStressCase {
            id: "plan-capability-valid".into(),
            name: "Plan references valid capabilities".into(),
            output: "1. Run [validate_model] on the holdout set\n\
                    2. Execute [deploy_staging] with current config\n\
                    3. Use [monitor_metrics] for 24 hours"
                .into(),
            contract: OutputContract::Planning {
                requires_ordered_steps: true,
                max_steps: 10,
                requires_capability_refs: true,
                allowed_capabilities: vec![
                    "validate_model".into(),
                    "deploy_staging".into(),
                    "deploy_production".into(),
                    "monitor_metrics".into(),
                    "rollback".into(),
                ],
            },
            expected_valid: true,
            tests: "Does contract accept plans using valid capabilities?".into(),
            interpretation: "Contract validates capability references against registry.".into(),
        },
        // Should FAIL: Plan references invalid capability
        OutputStressCase {
            id: "plan-capability-invalid".into(),
            name: "Plan references unknown capability".into(),
            output: "1. Run [validate_model] on holdout\n\
                    2. Execute [quantum_optimize] for better results\n\
                    3. Use [deploy_staging] to test"
                .into(),
            contract: OutputContract::Planning {
                requires_ordered_steps: true,
                max_steps: 10,
                requires_capability_refs: true,
                allowed_capabilities: vec![
                    "validate_model".into(),
                    "deploy_staging".into(),
                    "deploy_production".into(),
                    "monitor_metrics".into(),
                ],
            },
            expected_valid: false,
            tests: "Does contract reject unknown capabilities?".into(),
            interpretation: "Contract prevents hallucinated capability references.".into(),
        },
        // Should FAIL: Plan missing capability refs when required
        OutputStressCase {
            id: "plan-capability-missing".into(),
            name: "Plan missing capability references".into(),
            output: "1. Validate the model\n\
                    2. Deploy to staging\n\
                    3. Monitor for issues"
                .into(),
            contract: OutputContract::Planning {
                requires_ordered_steps: true,
                max_steps: 10,
                requires_capability_refs: true,
                allowed_capabilities: vec![
                    "validate_model".into(),
                    "deploy_staging".into(),
                    "monitor_metrics".into(),
                ],
            },
            expected_valid: false,
            tests: "Does contract require capability references when enabled?".into(),
            interpretation: "Contract enforces explicit capability references.".into(),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_all_cases_have_ids() {
        let cases = all_stress_cases();
        for case in &cases {
            assert!(!case.id.is_empty(), "Case missing ID: {:?}", case.name);
        }
    }

    #[test]
    fn test_reasoning_cases() {
        let cases = reasoning_stress_cases();
        assert!(cases.len() >= 5, "Should have at least 5 reasoning cases");
    }

    #[test]
    fn test_evaluation_cases() {
        let cases = evaluation_stress_cases();
        assert!(cases.len() >= 5, "Should have at least 5 evaluation cases");
    }

    #[test]
    fn test_planning_cases() {
        let cases = planning_stress_cases();
        assert!(cases.len() >= 5, "Should have at least 5 planning cases");
    }

    #[test]
    fn test_run_stress_tests() {
        let report = run_output_stress_tests();

        // Report should be generated
        assert!(report.total > 0);

        // Summary should be valid
        let summary = report.summary();
        assert!(summary.contains("OUTPUT CONTRACT STRESS REPORT"));
    }

    #[test]
    fn test_known_limitations_documented() {
        // These cases document known limitations
        let cases = all_stress_cases();

        let hallucination_case = cases
            .iter()
            .find(|c| c.id == "reasoning-hallucinated-facts");
        assert!(hallucination_case.is_some());
        assert!(hallucination_case.unwrap().expected_valid); // Known to pass

        let overconfident_case = cases.iter().find(|c| c.id == "eval-overconfident");
        assert!(overconfident_case.is_some());
        assert!(overconfident_case.unwrap().expected_valid); // Known to pass

        let capability_case = cases
            .iter()
            .find(|c| c.id == "plan-hallucinated-capability");
        assert!(capability_case.is_some());
        assert!(capability_case.unwrap().expected_valid); // Known to pass
    }
}
