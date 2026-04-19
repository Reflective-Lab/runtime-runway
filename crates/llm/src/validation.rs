// Copyright 2024-2026 Reflective Labs

//! Output validation against contracts.
//!
//! This module validates LLM output against `OutputContract` specifications.
//! It is NOT a parser—it checks structural expectations, not semantic content.
//!
//! # Validation Philosophy
//!
//! - **Structural, not semantic**: Checks format, not meaning
//! - **Fail-fast**: Clear errors on contract violations
//! - **Pre-LoRA**: Lock validation before training
//!
//! # Example
//!
//! ```ignore
//! let contract = OutputContract::evaluation();
//! let output = "Option A: 0.85 (confidence: 0.9) - Good performance";
//!
//! let result = validate_output(&output, &contract)?;
//! assert!(result.is_valid());
//! ```

use crate::prompt::{OutputContract, ScoreCardinality, StepFormat};
use serde::{Deserialize, Serialize};

/// Result of validating output against a contract.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationResult {
    /// Whether the output meets the contract
    pub valid: bool,
    /// Contract type that was checked
    pub contract_type: String,
    /// Specific checks that passed
    pub passed_checks: Vec<String>,
    /// Specific checks that failed
    pub failed_checks: Vec<ValidationFailure>,
    /// Warnings (non-fatal issues)
    pub warnings: Vec<String>,
}

impl ValidationResult {
    /// Check if validation passed.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.valid
    }

    /// Get the first failure reason, if any.
    #[must_use]
    pub fn first_failure(&self) -> Option<&ValidationFailure> {
        self.failed_checks.first()
    }

    /// Check if output improperly cites recall context as evidence.
    ///
    /// Recall candidates should NEVER be cited as evidence in output.
    /// This structural enforcement prevents LLM hallucination from being
    /// accidentally treated as grounded evidence.
    #[must_use]
    pub fn check_recall_citation_violation(
        output: &str,
        recall_ids: &[String],
    ) -> Option<ValidationFailure> {
        let output_lower = output.to_lowercase();

        for id in recall_ids {
            let id_lower = id.to_lowercase();

            // Check for various citation patterns
            let citation_patterns = [
                format!("evidence: {}", id_lower),
                format!("citing: {}", id_lower),
                format!("based on {}", id_lower),
                format!("according to {}", id_lower),
                format!("as shown in {}", id_lower),
                format!("reference: {}", id_lower),
                format!("source: {}", id_lower),
            ];

            for pattern in &citation_patterns {
                if output_lower.contains(pattern) {
                    return Some(
                        ValidationFailure::new(
                            "recall_cited_as_evidence",
                            format!("Recall candidate '{}' improperly cited as evidence", id),
                            "Recall context is informational only and cannot be cited as evidence",
                        )
                        .with_found(format!("Found citation pattern: {}", pattern)),
                    );
                }
            }
        }

        None
    }
}

/// A specific validation failure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationFailure {
    /// What check failed
    pub check: String,
    /// Why it failed
    pub reason: String,
    /// What was expected
    pub expected: String,
    /// What was found (if applicable)
    pub found: Option<String>,
}

impl ValidationFailure {
    fn new(
        check: impl Into<String>,
        reason: impl Into<String>,
        expected: impl Into<String>,
    ) -> Self {
        Self {
            check: check.into(),
            reason: reason.into(),
            expected: expected.into(),
            found: None,
        }
    }

    fn with_found(mut self, found: impl Into<String>) -> Self {
        self.found = Some(found.into());
        self
    }
}

/// Validate LLM output against an output contract.
///
/// This performs structural validation, not semantic analysis.
#[must_use]
pub fn validate_output(output: &str, contract: &OutputContract) -> ValidationResult {
    match contract {
        OutputContract::Reasoning {
            requires_conclusion,
            allows_uncertainty,
            max_steps,
            step_format,
        } => validate_reasoning(
            output,
            *requires_conclusion,
            *allows_uncertainty,
            *max_steps,
            *step_format,
        ),

        OutputContract::Planning {
            requires_ordered_steps,
            max_steps,
            requires_capability_refs,
            allowed_capabilities,
        } => validate_planning(
            output,
            *requires_ordered_steps,
            *max_steps,
            *requires_capability_refs,
            allowed_capabilities,
        ),

        OutputContract::Evaluation {
            score_range,
            confidence_required,
            justification_required,
            cardinality,
            grounding_refs,
        } => validate_evaluation(
            output,
            *score_range,
            *confidence_required,
            *justification_required,
            *cardinality,
            grounding_refs,
        ),

        OutputContract::Classification {
            valid_categories,
            confidence_required,
            multi_label,
        } => validate_classification(output, valid_categories, *confidence_required, *multi_label),

        OutputContract::Extraction {
            required_fields,
            optional_fields: _,
        } => validate_extraction(output, required_fields),

        OutputContract::Freeform { max_tokens } => validate_freeform(output, *max_tokens),
    }
}

fn validate_reasoning(
    output: &str,
    requires_conclusion: bool,
    allows_uncertainty: bool,
    max_steps: Option<usize>,
    step_format: StepFormat,
) -> ValidationResult {
    let mut passed = vec![];
    let mut failed = vec![];
    let mut warnings = vec![];

    let output_upper = output.to_uppercase();

    // Check for conclusion or uncertainty markers first
    let has_conclusion = output_upper.contains("CONCLUSION:")
        || output_upper.contains("THEREFORE:")
        || output_upper.contains("RESULT:");

    let has_uncertainty = output_upper.contains("UNCERTAIN:");

    // Count steps using format-specific counting
    let step_count = count_steps_by_format(output, step_format);

    // Check UNCERTAIN format if present (must have a reason on the same line)
    if has_uncertainty && allows_uncertainty {
        let uncertain_has_reason = check_uncertain_has_reason(output);
        if uncertain_has_reason {
            passed.push("UNCERTAIN has reason code".to_string());
        } else {
            failed.push(
                ValidationFailure::new(
                    "uncertain_reason",
                    "UNCERTAIN statement missing reason",
                    "UNCERTAIN: <one-line reason>",
                )
                .with_found("UNCERTAIN without reason"),
            );
        }
    }

    // Check for reasoning steps (required when a conclusion is expected)
    // Exception: UNCERTAIN without steps is valid (it's saying "cannot reason")
    let uncertainty_is_valid_without_steps = allows_uncertainty && has_uncertainty;

    if requires_conclusion && has_conclusion && step_count == 0 {
        // A conclusion without reasoning is just an assertion, not reasoning
        let expected = match step_format {
            StepFormat::Loose => "At least one reasoning step (Step 1, First, etc.)",
            StepFormat::StepNColon => "At least one step in format: Step 1: <text>",
            StepFormat::NumberedList => "At least one step in format: 1. <text>",
        };
        failed.push(
            ValidationFailure::new(
                "requires_reasoning_steps",
                "No reasoning steps found",
                expected,
            )
            .with_found("No reasoning steps detected"),
        );
    } else if step_count > 0 {
        passed.push(format!(
            "Has {step_count} reasoning step(s) ({step_format:?} format)"
        ));
    } else if uncertainty_is_valid_without_steps {
        passed.push("UNCERTAIN allows omitting reasoning steps".to_string());
    }

    // Check for conclusion
    if requires_conclusion {
        if has_conclusion {
            passed.push("Has explicit conclusion".to_string());
        } else if allows_uncertainty && has_uncertainty {
            passed.push("Has explicit uncertainty statement".to_string());
        } else {
            failed.push(
                ValidationFailure::new(
                    "requires_conclusion",
                    "No explicit conclusion found",
                    "CONCLUSION: <conclusion> or UNCERTAIN: <reason>",
                )
                .with_found("No conclusion marker"),
            );
        }
    }

    // Check step count limit
    if let Some(max) = max_steps {
        if step_count <= max {
            if step_count > 0 {
                passed.push(format!("Step count ({step_count}) within limit ({max})"));
            }
        } else {
            failed.push(
                ValidationFailure::new(
                    "max_steps",
                    "Too many reasoning steps",
                    format!("At most {max} steps"),
                )
                .with_found(format!("{step_count} steps")),
            );
        }
    }

    // Check output length
    if output.len() < 10 {
        warnings.push("Output seems very short for reasoning".to_string());
    }

    ValidationResult {
        valid: failed.is_empty(),
        contract_type: "Reasoning".to_string(),
        passed_checks: passed,
        failed_checks: failed,
        warnings,
    }
}

fn validate_planning(
    output: &str,
    requires_ordered_steps: bool,
    max_steps: usize,
    requires_capability_refs: bool,
    allowed_capabilities: &[String],
) -> ValidationResult {
    let mut passed = vec![];
    let mut failed = vec![];
    let mut warnings = vec![];

    // Check for ordered steps
    if requires_ordered_steps {
        let step_count = count_numbered_steps(output);
        if step_count > 0 {
            passed.push(format!("Has {step_count} numbered steps"));

            if step_count <= max_steps {
                passed.push(format!("Step count within limit ({max_steps})"));
            } else {
                failed.push(
                    ValidationFailure::new(
                        "max_steps",
                        "Too many steps",
                        format!("At most {max_steps} steps"),
                    )
                    .with_found(format!("{step_count} steps")),
                );
            }
        } else {
            failed.push(
                ValidationFailure::new(
                    "requires_ordered_steps",
                    "No numbered steps found",
                    "Steps like: 1. ... 2. ... 3. ...",
                )
                .with_found("No step markers"),
            );
        }
    }

    // Check capability references if required
    if requires_capability_refs && !allowed_capabilities.is_empty() {
        let output_lower = output.to_lowercase();
        let mut found_capabilities = vec![];
        let mut unknown_capabilities = vec![];

        // Look for capability references (case-insensitive)
        for cap in allowed_capabilities {
            if output_lower.contains(&cap.to_lowercase()) {
                found_capabilities.push(cap.clone());
            }
        }

        // Check for potential unknown capability references
        // Look for patterns like "CAPABILITY:", "[capability_name]", or action verbs
        for line in output.lines() {
            let line_lower = line.to_lowercase();

            // Check for "CAPABILITY:" format
            if line_lower.contains("capability:") {
                if let Some(idx) = line_lower.find("capability:") {
                    let cap_name = line[idx + 11..]
                        .trim()
                        .split_whitespace()
                        .next()
                        .unwrap_or("");
                    if !allowed_capabilities
                        .iter()
                        .any(|c| c.eq_ignore_ascii_case(cap_name))
                    {
                        unknown_capabilities.push(cap_name.to_string());
                    }
                }
            }

            // Check for "[capability_name]" format (common for explicit capability references)
            let mut remaining = line;
            while let Some(start) = remaining.find('[') {
                if let Some(end) = remaining[start..].find(']') {
                    let cap_name = &remaining[start + 1..start + end];
                    // Only consider valid capability-like names (alphanumeric + underscore)
                    if !cap_name.is_empty()
                        && cap_name.chars().all(|c| c.is_alphanumeric() || c == '_')
                    {
                        if !allowed_capabilities
                            .iter()
                            .any(|c| c.eq_ignore_ascii_case(cap_name))
                        {
                            unknown_capabilities.push(cap_name.to_string());
                        }
                    }
                    remaining = &remaining[start + end + 1..];
                } else {
                    break;
                }
            }
        }

        if found_capabilities.is_empty() {
            failed.push(
                ValidationFailure::new(
                    "requires_capability_refs",
                    "No valid capabilities referenced",
                    format!(
                        "Must reference at least one of: {}",
                        allowed_capabilities.join(", ")
                    ),
                )
                .with_found("No capability references found"),
            );
        } else {
            passed.push(format!(
                "References capabilities: {}",
                found_capabilities.join(", ")
            ));
        }

        if !unknown_capabilities.is_empty() {
            failed.push(
                ValidationFailure::new(
                    "unknown_capability",
                    "References unknown capabilities",
                    format!("Allowed: {}", allowed_capabilities.join(", ")),
                )
                .with_found(format!("Unknown: {}", unknown_capabilities.join(", "))),
            );
        }
    } else if requires_capability_refs && allowed_capabilities.is_empty() {
        warnings.push("Capability refs required but no allowed capabilities specified".to_string());
    }

    ValidationResult {
        valid: failed.is_empty(),
        contract_type: "Planning".to_string(),
        passed_checks: passed,
        failed_checks: failed,
        warnings,
    }
}

fn validate_evaluation(
    output: &str,
    score_range: (f32, f32),
    confidence_required: bool,
    justification_required: bool,
    cardinality: ScoreCardinality,
    grounding_refs: &[String],
) -> ValidationResult {
    let mut passed = vec![];
    let mut failed = vec![];
    let mut warnings = vec![];

    // Try to find scores in the output
    let scores = extract_scores(output);

    if scores.is_empty() {
        failed.push(
            ValidationFailure::new(
                "scores",
                "No scores found in output",
                format!("Scores in range {:.1}-{:.1}", score_range.0, score_range.1),
            )
            .with_found("No numeric scores"),
        );
    } else {
        // Check score range
        let out_of_range: Vec<_> = scores
            .iter()
            .filter(|s| **s < score_range.0 || **s > score_range.1)
            .collect();

        if out_of_range.is_empty() {
            passed.push(format!("All {} scores within range", scores.len()));
        } else {
            failed.push(
                ValidationFailure::new(
                    "score_range",
                    "Scores outside valid range",
                    format!("{:.1}-{:.1}", score_range.0, score_range.1),
                )
                .with_found(format!("{:?}", out_of_range)),
            );
        }

        // Check cardinality
        match cardinality {
            ScoreCardinality::AtLeast(n) => {
                if scores.len() >= n {
                    passed.push(format!(
                        "Score count ({}) meets minimum ({})",
                        scores.len(),
                        n
                    ));
                } else {
                    failed.push(
                        ValidationFailure::new(
                            "score_cardinality",
                            "Not enough scores",
                            format!("At least {} score(s) required", n),
                        )
                        .with_found(format!("{} score(s)", scores.len())),
                    );
                }
            }
            ScoreCardinality::Exactly(n) => {
                if scores.len() == n {
                    passed.push(format!(
                        "Score count ({}) matches required ({})",
                        scores.len(),
                        n
                    ));
                } else {
                    failed.push(
                        ValidationFailure::new(
                            "score_cardinality",
                            "Wrong number of scores",
                            format!("Exactly {} score(s) required", n),
                        )
                        .with_found(format!("{} score(s)", scores.len())),
                    );
                }
            }
        }
    }

    // Check for confidence
    if confidence_required {
        let output_lower = output.to_lowercase();
        let has_confidence = output_lower.contains("confidence")
            || output_lower.contains("certainty")
            || output_lower.contains("conf:");

        if has_confidence {
            passed.push("Has confidence indicators".to_string());
        } else {
            failed.push(ValidationFailure::new(
                "confidence_required",
                "No confidence values found",
                "Include confidence (0.0-1.0) for each score",
            ));
        }
    }

    // Check for justification
    if justification_required {
        // Simple heuristic: justification usually means longer text per score
        // Require at least 10 words per score to count as justification
        let word_count = output.split_whitespace().count();
        let score_count = scores.len().max(1);
        let words_per_score = word_count as f32 / score_count as f32;
        if words_per_score >= 10.0 {
            passed.push("Has justification text".to_string());
        } else if words_per_score > 5.0 {
            warnings.push("Justification may be too brief".to_string());
            passed.push("Has minimal justification".to_string());
        } else {
            failed.push(
                ValidationFailure::new(
                    "justification_required",
                    "No meaningful justification found",
                    "Justification text explaining the score (at least 10 words per score)",
                )
                .with_found(format!("Only {:.1} words per score", words_per_score)),
            );
        }
    }

    // Check grounding references (prevents "Lorem ipsum" from passing)
    if !grounding_refs.is_empty() {
        let output_lower = output.to_lowercase();
        let found_refs: Vec<_> = grounding_refs
            .iter()
            .filter(|r| output_lower.contains(&r.to_lowercase()))
            .collect();

        if found_refs.is_empty() {
            failed.push(
                ValidationFailure::new(
                    "grounding_refs",
                    "Justification lacks grounding references",
                    format!(
                        "Must reference at least one of: {}",
                        grounding_refs.join(", ")
                    ),
                )
                .with_found("No grounding references found"),
            );
        } else {
            passed.push(format!(
                "Grounded in: {}",
                found_refs
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
    }

    ValidationResult {
        valid: failed.is_empty(),
        contract_type: "Evaluation".to_string(),
        passed_checks: passed,
        failed_checks: failed,
        warnings,
    }
}

fn validate_classification(
    output: &str,
    valid_categories: &[String],
    confidence_required: bool,
    multi_label: bool,
) -> ValidationResult {
    let mut passed = vec![];
    let mut failed = vec![];
    let warnings = vec![];

    let output_upper = output.to_uppercase();

    // Check for category markers
    let has_category_marker = output_upper.contains("CATEGORY:")
        || output_upper.contains("CLASS:")
        || output_upper.contains("LABEL:");

    if has_category_marker {
        passed.push("Has category marker".to_string());
    }

    // If valid_categories specified, check if output contains one
    if !valid_categories.is_empty() {
        let found_categories: Vec<_> = valid_categories
            .iter()
            .filter(|c| output_upper.contains(&c.to_uppercase()))
            .collect();

        if found_categories.is_empty() {
            failed.push(
                ValidationFailure::new(
                    "valid_categories",
                    "No valid category found in output",
                    format!("One of: {}", valid_categories.join(", ")),
                )
                .with_found("None matched"),
            );
        } else if !multi_label && found_categories.len() > 1 {
            failed.push(
                ValidationFailure::new(
                    "multi_label",
                    "Multiple categories found but multi_label is false",
                    "Exactly one category",
                )
                .with_found(format!("{} categories", found_categories.len())),
            );
        } else {
            passed.push(format!("Found valid category: {:?}", found_categories));
        }
    }

    // Check for confidence
    if confidence_required {
        // Look for confidence patterns directly (since extract_scores skips confidence values)
        let output_lower = output.to_lowercase();
        let has_confidence = output_lower.contains("confidence:")
            || output_lower.contains("conf:")
            || output_lower.contains("certainty:");

        if has_confidence {
            passed.push("Has confidence score".to_string());
        } else {
            failed.push(ValidationFailure::new(
                "confidence_required",
                "No confidence score found",
                "Include confidence (0.0-1.0)",
            ));
        }
    }

    ValidationResult {
        valid: failed.is_empty(),
        contract_type: "Classification".to_string(),
        passed_checks: passed,
        failed_checks: failed,
        warnings,
    }
}

fn validate_extraction(output: &str, required_fields: &[String]) -> ValidationResult {
    let mut passed = vec![];
    let mut failed = vec![];
    let warnings = vec![];

    let output_lower = output.to_lowercase();

    for field in required_fields {
        let field_lower = field.to_lowercase();

        // Check if field appears (as key: value or similar)
        let has_field = output_lower.contains(&format!("{field_lower}:"))
            || output_lower.contains(&format!("{field_lower} ="))
            || output_lower.contains(&format!("\"{field_lower}\""));

        if has_field {
            passed.push(format!("Found field: {field}"));
        } else {
            failed.push(ValidationFailure::new(
                "required_field",
                format!("Missing required field: {field}"),
                format!("{field}: <value>"),
            ));
        }
    }

    ValidationResult {
        valid: failed.is_empty(),
        contract_type: "Extraction".to_string(),
        passed_checks: passed,
        failed_checks: failed,
        warnings,
    }
}

fn validate_freeform(output: &str, max_tokens: usize) -> ValidationResult {
    let mut passed = vec![];
    let mut failed = vec![];
    let mut warnings = vec![];

    // Estimate token count (rough: ~4 chars per token)
    let estimated_tokens = (output.len() + 3) / 4;

    if estimated_tokens <= max_tokens {
        passed.push(format!(
            "Output length (~{estimated_tokens} tokens) within limit ({max_tokens})"
        ));
    } else {
        failed.push(
            ValidationFailure::new(
                "max_tokens",
                "Output too long",
                format!("At most {max_tokens} tokens"),
            )
            .with_found(format!("~{estimated_tokens} tokens")),
        );
    }

    if output.is_empty() {
        warnings.push("Output is empty".to_string());
    }

    ValidationResult {
        valid: failed.is_empty(),
        contract_type: "Freeform".to_string(),
        passed_checks: passed,
        failed_checks: failed,
        warnings,
    }
}

// Helper functions

/// Count steps according to the specified format.
fn count_steps_by_format(output: &str, format: StepFormat) -> usize {
    match format {
        StepFormat::Loose => count_reasoning_steps_loose(output),
        StepFormat::StepNColon => count_step_n_colon(output),
        StepFormat::NumberedList => count_numbered_list(output),
    }
}

/// Loose step counting: keywords anywhere in text.
fn count_reasoning_steps_loose(output: &str) -> usize {
    let step_patterns = [
        "step ",
        "first,",
        "second,",
        "then,",
        "finally,",
        "therefore,",
    ];
    let output_lower = output.to_lowercase();

    step_patterns
        .iter()
        .map(|p| output_lower.matches(p).count())
        .sum()
}

/// Strict step counting: "Step N:" at line start.
fn count_step_n_colon(output: &str) -> usize {
    let mut count = 0;
    for line in output.lines() {
        let trimmed = line.trim().to_lowercase();
        // Check for "step N:" at start of line
        if trimmed.starts_with("step ") {
            // Extract the number after "step "
            let rest = &trimmed[5..];
            if let Some(colon_pos) = rest.find(':') {
                let num_part = rest[..colon_pos].trim();
                if num_part.chars().all(|c| c.is_ascii_digit()) {
                    count += 1;
                }
            }
        }
    }
    count
}

/// Numbered list counting: "N." at line start.
fn count_numbered_list(output: &str) -> usize {
    let mut count = 0;
    let mut expected = 1;
    for line in output.lines() {
        let trimmed = line.trim();
        // Check for "N." at start of line
        let pattern = format!("{}.", expected);
        if trimmed.starts_with(&pattern) {
            count += 1;
            expected += 1;
        }
    }
    count
}

/// Check if UNCERTAIN statement has a reason on the same line.
fn check_uncertain_has_reason(output: &str) -> bool {
    for line in output.lines() {
        let line_upper = line.to_uppercase();
        if let Some(pos) = line_upper.find("UNCERTAIN:") {
            // Check if there's meaningful content after "UNCERTAIN:"
            let after = &line[pos + 10..].trim();
            // Must have at least 3 characters of reason
            if after.len() >= 3 {
                return true;
            }
        }
    }
    false
}

fn count_numbered_steps(output: &str) -> usize {
    let mut count = 0;
    for i in 1..=20 {
        if output.contains(&format!("{i}.")) || output.contains(&format!("{i})")) {
            count = i;
        } else {
            break;
        }
    }
    count
}

fn extract_scores(output: &str) -> Vec<f32> {
    let mut scores = vec![];

    // Context-aware score extraction
    // Skip values that appear after "confidence", "conf:", "certainty" etc.
    let words: Vec<&str> = output.split_whitespace().collect();

    for (i, word) in words.iter().enumerate() {
        let cleaned = word.trim_matches(|c: char| !c.is_numeric() && c != '.' && c != '-');
        if let Ok(score) = cleaned.parse::<f32>() {
            // Check if previous word indicates this is a confidence value (not a primary score)
            let is_confidence = i > 0 && {
                let prev = words[i - 1].to_lowercase();
                prev.contains("confidence")
                    || prev.contains("conf:")
                    || prev.contains("certainty")
                    || prev.ends_with("confidence:")
            };

            // Skip confidence values
            if is_confidence {
                continue;
            }

            // Normalize percentage to 0-1 range (only if clearly a percentage: 10-100)
            let normalized = if score >= 10.0 && score <= 100.0 {
                score / 100.0
            } else {
                score
            };
            scores.push(normalized);
        }
    }

    scores
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_reasoning_with_conclusion() {
        let output = "Based on the data, step 1 we see X. Step 2, Y follows. CONCLUSION: The trend is positive.";
        let contract = OutputContract::Reasoning {
            requires_conclusion: true,
            allows_uncertainty: true,
            max_steps: Some(5),
            step_format: StepFormat::Loose,
        };

        let result = validate_output(output, &contract);
        assert!(result.is_valid());
    }

    #[test]
    fn test_validate_reasoning_with_uncertainty() {
        let output = "UNCERTAIN: Insufficient data to determine the trend.";
        let contract = OutputContract::Reasoning {
            requires_conclusion: true,
            allows_uncertainty: true,
            max_steps: Some(5),
            step_format: StepFormat::Loose,
        };

        let result = validate_output(output, &contract);
        assert!(result.is_valid());
    }

    #[test]
    fn test_validate_reasoning_missing_conclusion() {
        let output = "The data shows some patterns but no clear answer.";
        let contract = OutputContract::Reasoning {
            requires_conclusion: true,
            allows_uncertainty: false,
            max_steps: Some(5),
            step_format: StepFormat::Loose,
        };

        let result = validate_output(output, &contract);
        assert!(!result.is_valid());
    }

    #[test]
    fn test_validate_planning_with_steps() {
        let output =
            "1. First, gather requirements\n2. Then, design the solution\n3. Finally, implement";
        let contract = OutputContract::Planning {
            requires_ordered_steps: true,
            max_steps: 10,
            requires_capability_refs: false,
            allowed_capabilities: vec![],
        };

        let result = validate_output(output, &contract);
        assert!(result.is_valid());
    }

    #[test]
    fn test_validate_planning_too_many_steps() {
        let output = "1. A\n2. B\n3. C\n4. D\n5. E\n6. F";
        let contract = OutputContract::Planning {
            requires_ordered_steps: true,
            max_steps: 3,
            requires_capability_refs: false,
            allowed_capabilities: vec![],
        };

        let result = validate_output(output, &contract);
        assert!(!result.is_valid());
    }

    #[test]
    fn test_validate_evaluation_with_scores() {
        let output = "Option A: 0.85 (confidence: 0.9) - Good performance on validation metrics with \
            low error rates and high success ratio across all test cases. \
            Option B: 0.72 (confidence: 0.8) - Acceptable performance though with some variance \
            in edge cases that could benefit from additional training data.";
        let contract = OutputContract::Evaluation {
            score_range: (0.0, 1.0),
            confidence_required: true,
            justification_required: true,
            cardinality: ScoreCardinality::AtLeast(1),
            grounding_refs: vec![],
        };

        let result = validate_output(output, &contract);
        assert!(result.is_valid());
    }

    #[test]
    fn test_validate_evaluation_out_of_range() {
        let output = "Score: 1.5 - This is too high";
        let contract = OutputContract::Evaluation {
            score_range: (0.0, 1.0),
            confidence_required: false,
            justification_required: false,
            cardinality: ScoreCardinality::AtLeast(1),
            grounding_refs: vec![],
        };

        let result = validate_output(output, &contract);
        assert!(!result.is_valid());
    }

    #[test]
    fn test_validate_classification() {
        let output = "CATEGORY: positive (confidence: 0.92)";
        let contract = OutputContract::Classification {
            valid_categories: vec![
                "positive".to_string(),
                "negative".to_string(),
                "neutral".to_string(),
            ],
            confidence_required: true,
            multi_label: false,
        };

        let result = validate_output(output, &contract);
        assert!(result.is_valid());
    }

    #[test]
    fn test_validate_extraction() {
        let output = "name: Widget X\nvalue: 42.5\nunit: kg";
        let contract = OutputContract::Extraction {
            required_fields: vec!["name".to_string(), "value".to_string()],
            optional_fields: vec!["unit".to_string()],
        };

        let result = validate_output(output, &contract);
        assert!(result.is_valid());
    }

    #[test]
    fn test_validate_extraction_missing_field() {
        let output = "name: Widget X";
        let contract = OutputContract::Extraction {
            required_fields: vec!["name".to_string(), "value".to_string()],
            optional_fields: vec![],
        };

        let result = validate_output(output, &contract);
        assert!(!result.is_valid());
        assert!(result.first_failure().unwrap().reason.contains("value"));
    }

    #[test]
    fn test_validate_freeform() {
        let output = "This is a short response.";
        let contract = OutputContract::Freeform { max_tokens: 100 };

        let result = validate_output(output, &contract);
        assert!(result.is_valid());
    }
}
