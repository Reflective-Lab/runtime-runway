// Copyright 2024-2026 Reflective Labs

//! Prompt architecture for Burn-based LLM systems.
//!
//! In a Burn system, prompts are NOT conversational instructions.
//! They are a control surface for an embedded reasoning engine.
//!
//! # Prompt Layers
//!
//! ```text
//! [Model Priming]      ← rarely changes, encodes identity
//! [Role / Policy]      ← stable, versioned, per-deployment
//! [Task Frame]         ← per capability/agent
//! [State Injection]    ← Polars-derived, structured
//! [User Intent]        ← thin, minimal
//! ```
//!
//! Only the last layer resembles a traditional "prompt".
//!
//! # Design Principles
//!
//! - **Short over long**: If your prompt is long, you're doing it wrong
//! - **Structured over narrative**: Inject state, don't describe it
//! - **Invariants over instructions**: Encode identity, not tasks
//! - **Model-specific**: Optimized for exact tokenizer/context/quantization

use crate::config::LlmConfig;
use crate::error::{LlmError, LlmResult};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Version identifier for a prompt stack.
///
/// Prompts and models are co-versioned. A PromptStack version without
/// a corresponding model configuration is incomplete.
///
/// Future usage:
/// - `PromptStack::v1_for_llama3_8b()`
/// - `PromptStack::v2_for_llama3_3b()`
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PromptVersion {
    /// Version name (e.g., "reasoning_v1", "scoring_v2")
    pub name: String,
    /// Revision number (incremented on prompt changes)
    pub revision: u32,
    /// Target model family (e.g., "llama3")
    pub model_family: String,
}

impl PromptVersion {
    /// Create a new prompt version.
    #[must_use]
    pub fn new(name: impl Into<String>, revision: u32, model_family: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            revision,
            model_family: model_family.into(),
        }
    }

    /// Default reasoning prompt for Llama 3.
    #[must_use]
    pub fn reasoning_v1_llama3() -> Self {
        Self::new("reasoning", 1, "llama3")
    }

    /// Default scoring prompt for Llama 3.
    #[must_use]
    pub fn scoring_v1_llama3() -> Self {
        Self::new("scoring", 1, "llama3")
    }

    /// Default planning prompt for Llama 3.
    #[must_use]
    pub fn planning_v1_llama3() -> Self {
        Self::new("planning", 1, "llama3")
    }
}

impl std::fmt::Display for PromptVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:v{}:{}", self.name, self.revision, self.model_family)
    }
}

/// Complete prompt stack for a Burn-based reasoning engine.
///
/// This is NOT a chat prompt. It is a control surface.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptStack {
    /// Version identifier for this prompt stack
    pub version: PromptVersion,

    /// Model priming: identity and invariants (rarely changes)
    pub priming: ModelPriming,

    /// Role/policy: stable constraints (versioned per deployment)
    pub policy: RolePolicy,

    /// Task frame: capability-specific framing
    pub task_frame: TaskFrame,

    /// State injection: structured data from Polars/context
    pub state: StateInjection,

    /// User intent: minimal, often just intent + criteria
    pub intent: UserIntent,
}

impl PromptStack {
    /// Render the complete prompt for the model.
    ///
    /// The output is optimized for machine consumption, not human reading.
    #[must_use]
    pub fn render(&self) -> String {
        let mut parts = Vec::new();

        // Priming (small, stable)
        if !self.priming.identity.is_empty() {
            parts.push(self.priming.render());
        }

        // Policy (constraints, versioned)
        if !self.policy.constraints.is_empty() {
            parts.push(self.policy.render());
        }

        // Task frame (what capability is active)
        parts.push(self.task_frame.render());

        // State (the bulk of "context" - structured, not narrative)
        if !self.state.is_empty() {
            parts.push(self.state.render());
        }

        // Intent (minimal)
        parts.push(self.intent.render());

        parts.join("\n\n")
    }

    /// Estimate token count (rough, for budget checking).
    #[must_use]
    pub fn estimated_tokens(&self) -> usize {
        // Rough estimate: 4 chars per token
        self.render().len() / 4
    }

    /// Validate this prompt stack against a model configuration.
    ///
    /// This ensures prompt/model compatibility:
    /// - Context budget fits within model limits
    /// - Model family matches prompt version target
    /// - Optimization hints are appropriate
    ///
    /// # Errors
    ///
    /// Returns an error if the prompt is incompatible with the config.
    pub fn validate_for(&self, config: &LlmConfig) -> LlmResult<()> {
        // Check model family compatibility
        let model_family = if config.model_id.contains("llama3") {
            "llama3"
        } else if config.model_id.contains("llama2") {
            "llama2"
        } else if config.model_id.contains("tiny") {
            "tiny"
        } else {
            "unknown"
        };

        if self.version.model_family != model_family && self.version.model_family != "any" {
            return Err(LlmError::ConfigError(format!(
                "Prompt version {} targets '{}' but config uses '{}'",
                self.version, self.version.model_family, model_family
            )));
        }

        // Check context budget (leave room for generation)
        let estimated = self.estimated_tokens();
        let max_prompt = config.max_context_length / 2; // Reserve half for generation

        if estimated > max_prompt {
            return Err(LlmError::ContextLengthExceeded {
                got: estimated,
                max: max_prompt,
            });
        }

        Ok(())
    }

    /// Create a versioned prompt stack for Llama 3 reasoning tasks.
    #[must_use]
    pub fn reasoning_v1_llama3() -> Self {
        PromptStackBuilder::new()
            .version(PromptVersion::reasoning_v1_llama3())
            .priming(ModelPriming::reasoning_component())
            .task_frame(TaskFrame::default())
            .build()
    }

    /// Create a versioned prompt stack for Llama 3 scoring tasks.
    #[must_use]
    pub fn scoring_v1_llama3() -> Self {
        PromptStackBuilder::new()
            .version(PromptVersion::scoring_v1_llama3())
            .priming(ModelPriming::scoring_component())
            .task_frame(TaskFrame::evaluate())
            .build()
    }

    /// Create a versioned prompt stack for Llama 3 planning tasks.
    #[must_use]
    pub fn planning_v1_llama3() -> Self {
        PromptStackBuilder::new()
            .version(PromptVersion::planning_v1_llama3())
            .priming(ModelPriming::planning_component())
            .task_frame(TaskFrame::plan())
            .build()
    }
}

/// Model priming: identity and invariants.
///
/// This is small. It encodes what the model IS, not what it should DO.
/// It rarely mentions the user.
///
/// # Example
///
/// ```text
/// You are a deterministic reasoning component.
/// You operate on structured state provided below.
/// You do not speculate beyond the provided state.
/// When information is insufficient, you say so explicitly.
/// Your output is consumed by software, not a human.
/// ```
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ModelPriming {
    /// Core identity statement (1-2 sentences)
    pub identity: String,

    /// Invariants that always hold
    pub invariants: Vec<String>,
}

impl ModelPriming {
    /// Create priming for a deterministic reasoning component.
    #[must_use]
    pub fn reasoning_component() -> Self {
        Self {
            identity: "You are a deterministic reasoning component.".to_string(),
            invariants: vec![
                "You operate on structured state provided below.".to_string(),
                "You do not speculate beyond the provided state.".to_string(),
                "When information is insufficient, you say so explicitly.".to_string(),
                "Your output is consumed by software, not a human.".to_string(),
            ],
        }
    }

    /// Create priming for a scoring/evaluation component.
    #[must_use]
    pub fn scoring_component() -> Self {
        Self {
            identity: "You are a scoring component that evaluates options.".to_string(),
            invariants: vec![
                "You output numeric scores with brief justifications.".to_string(),
                "You do not add options or modify inputs.".to_string(),
                "Ties are broken by the first criterion listed.".to_string(),
            ],
        }
    }

    /// Create priming for a planning component.
    #[must_use]
    pub fn planning_component() -> Self {
        Self {
            identity: "You are a planning component that sequences actions.".to_string(),
            invariants: vec![
                "You output ordered steps, not explanations.".to_string(),
                "Each step must be achievable given the state.".to_string(),
                "You do not invent capabilities not listed.".to_string(),
            ],
        }
    }

    fn render(&self) -> String {
        let mut lines = vec![self.identity.clone()];
        lines.extend(self.invariants.iter().cloned());
        lines.join("\n")
    }
}

/// Role/policy constraints (stable, versioned).
///
/// These are deployment-specific constraints that change rarely.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RolePolicy {
    /// Version identifier for this policy
    pub version: String,

    /// Hard constraints that must be respected
    pub constraints: Vec<String>,

    /// Output format requirements
    pub output_format: Option<OutputFormat>,
}

impl RolePolicy {
    fn render(&self) -> String {
        let mut lines = Vec::new();

        if !self.version.is_empty() {
            lines.push(format!("POLICY_VERSION: {}", self.version));
        }

        if !self.constraints.is_empty() {
            lines.push("CONSTRAINTS:".to_string());
            for c in &self.constraints {
                lines.push(format!("- {c}"));
            }
        }

        if let Some(fmt) = &self.output_format {
            lines.push(format!("OUTPUT_FORMAT: {}", fmt.name));
            if let Some(schema) = &fmt.schema {
                lines.push(format!("SCHEMA: {schema}"));
            }
        }

        lines.join("\n")
    }
}

/// Output format specification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputFormat {
    /// Format name (json, yaml, structured, freeform)
    pub name: String,

    /// Optional schema definition
    pub schema: Option<String>,
}

/// Step format strictness for reasoning contracts.
///
/// Controls how reasoning steps are detected and validated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum StepFormat {
    /// Loose matching: "step ", "first,", "then,", etc.
    /// Most permissive, good for natural language output.
    #[default]
    Loose,
    /// Strict numbered: "Step 1:", "Step 2:", etc.
    /// Line-anchored: `^Step\s+\d+:`
    StepNColon,
    /// Numbered list: "1.", "2.", etc.
    /// Line-anchored: `^\d+\.`
    NumberedList,
}

/// Score cardinality requirement for evaluation contracts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScoreCardinality {
    /// At least N scores required.
    AtLeast(usize),
    /// Exactly N scores required.
    Exactly(usize),
}

impl Default for ScoreCardinality {
    fn default() -> Self {
        Self::AtLeast(1)
    }
}

/// Output contract: expected shape of model output.
///
/// This is NOT a parser. It is a contract that declares expectations.
/// Validation happens post-inference; this defines what "valid" means.
///
/// # Why Before LoRA
///
/// Learning amplifies output ambiguity. Lock output expectations before
/// training begins, or LoRA will learn to produce ambiguous outputs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OutputContract {
    /// Reasoning task: derives conclusions from state.
    Reasoning {
        /// Must end with an explicit conclusion
        requires_conclusion: bool,
        /// Allows "insufficient information" as valid output
        allows_uncertainty: bool,
        /// Maximum reasoning steps before conclusion
        max_steps: Option<usize>,
        /// Step format strictness (default: Loose)
        #[serde(default)]
        step_format: StepFormat,
    },

    /// Planning task: produces ordered action steps.
    Planning {
        /// Steps must be numbered/ordered
        requires_ordered_steps: bool,
        /// Maximum number of steps
        max_steps: usize,
        /// Each step must reference available capabilities
        requires_capability_refs: bool,
        /// Allowed capabilities (if non-empty, validates against this set)
        #[serde(default)]
        allowed_capabilities: Vec<String>,
    },

    /// Evaluation/scoring task: assigns scores to options.
    Evaluation {
        /// Valid score range (min, max)
        score_range: (f32, f32),
        /// Must include confidence with each score
        confidence_required: bool,
        /// Must include justification with each score
        justification_required: bool,
        /// Score cardinality requirement (default: AtLeast(1))
        #[serde(default)]
        cardinality: ScoreCardinality,
        /// Grounding references: justification must mention at least one of these
        /// Prevents "Lorem ipsum" from passing. Empty = no grounding check.
        #[serde(default)]
        grounding_refs: Vec<String>,
    },

    /// Classification task: assigns category labels.
    Classification {
        /// Valid categories (if empty, any category allowed)
        valid_categories: Vec<String>,
        /// Must include confidence score
        confidence_required: bool,
        /// Allows multiple categories
        multi_label: bool,
    },

    /// Extraction task: pulls structured data from text.
    Extraction {
        /// Required fields in output
        required_fields: Vec<String>,
        /// Optional fields in output
        optional_fields: Vec<String>,
    },

    /// Free-form output (use sparingly).
    Freeform {
        /// Maximum output length in tokens
        max_tokens: usize,
    },
}

impl OutputContract {
    /// Default contract for reasoning tasks.
    #[must_use]
    pub fn reasoning() -> Self {
        Self::Reasoning {
            requires_conclusion: true,
            allows_uncertainty: true,
            max_steps: Some(5),
            step_format: StepFormat::Loose,
        }
    }

    /// Reasoning contract with strict step format.
    #[must_use]
    pub fn reasoning_strict() -> Self {
        Self::Reasoning {
            requires_conclusion: true,
            allows_uncertainty: true,
            max_steps: Some(5),
            step_format: StepFormat::StepNColon,
        }
    }

    /// Default contract for planning tasks.
    #[must_use]
    pub fn planning() -> Self {
        Self::Planning {
            requires_ordered_steps: true,
            max_steps: 10,
            requires_capability_refs: false,
            allowed_capabilities: vec![],
        }
    }

    /// Planning contract with capability registry.
    #[must_use]
    pub fn planning_with_capabilities(capabilities: Vec<String>) -> Self {
        Self::Planning {
            requires_ordered_steps: true,
            max_steps: 10,
            requires_capability_refs: true,
            allowed_capabilities: capabilities,
        }
    }

    /// Default contract for evaluation tasks.
    #[must_use]
    pub fn evaluation() -> Self {
        Self::Evaluation {
            score_range: (0.0, 1.0),
            confidence_required: true,
            justification_required: true,
            cardinality: ScoreCardinality::AtLeast(1),
            grounding_refs: vec![],
        }
    }

    /// Evaluation contract with grounding references.
    #[must_use]
    pub fn evaluation_grounded(grounding_refs: Vec<String>) -> Self {
        Self::Evaluation {
            score_range: (0.0, 1.0),
            confidence_required: true,
            justification_required: true,
            cardinality: ScoreCardinality::AtLeast(1),
            grounding_refs,
        }
    }

    /// Default contract for classification tasks.
    #[must_use]
    pub fn classification() -> Self {
        Self::Classification {
            valid_categories: vec![],
            confidence_required: true,
            multi_label: false,
        }
    }

    /// Contract for extraction with specific fields.
    #[must_use]
    pub fn extraction(required: Vec<String>) -> Self {
        Self::Extraction {
            required_fields: required,
            optional_fields: vec![],
        }
    }

    /// Describe the contract as text for prompt inclusion.
    #[must_use]
    pub fn as_prompt_hint(&self) -> String {
        match self {
            Self::Reasoning {
                requires_conclusion,
                allows_uncertainty,
                max_steps,
                step_format,
            } => {
                let mut hints = vec![];

                // Step format instructions
                match step_format {
                    StepFormat::Loose => {
                        hints.push("Show your reasoning steps".to_string());
                    }
                    StepFormat::StepNColon => {
                        hints.push(
                            "Use format: Step 1: <reasoning>, Step 2: <reasoning>, ...".to_string(),
                        );
                    }
                    StepFormat::NumberedList => {
                        hints.push(
                            "Use numbered list: 1. <reasoning>, 2. <reasoning>, ...".to_string(),
                        );
                    }
                }

                if *requires_conclusion {
                    hints.push("End with CONCLUSION: <your conclusion>".to_string());
                }
                if *allows_uncertainty {
                    hints.push(
                        "If information is insufficient, state UNCERTAIN: <one-line reason>"
                            .to_string(),
                    );
                }
                if let Some(max) = max_steps {
                    hints.push(format!("Use at most {max} reasoning steps"));
                }
                hints.join("\n")
            }
            Self::Planning {
                requires_ordered_steps,
                max_steps,
                requires_capability_refs,
                allowed_capabilities,
            } => {
                let mut hints = vec![];
                if *requires_ordered_steps {
                    hints.push("Output numbered steps: 1. 2. 3. ...".to_string());
                }
                hints.push(format!("Maximum {max_steps} steps"));
                if *requires_capability_refs {
                    if allowed_capabilities.is_empty() {
                        hints.push("Each step must reference a capability from STATE".to_string());
                    } else {
                        hints.push(format!(
                            "Each step must use one of: {}",
                            allowed_capabilities.join(", ")
                        ));
                    }
                }
                hints.join("\n")
            }
            Self::Evaluation {
                score_range,
                confidence_required,
                justification_required,
                cardinality,
                grounding_refs,
            } => {
                let mut hints = vec![format!(
                    "Score each option from {:.1} to {:.1}",
                    score_range.0, score_range.1
                )];

                // Cardinality hint
                match cardinality {
                    ScoreCardinality::AtLeast(n) if *n > 1 => {
                        hints.push(format!("Provide at least {n} scores"));
                    }
                    ScoreCardinality::Exactly(n) => {
                        hints.push(format!("Provide exactly {n} score(s)"));
                    }
                    _ => {}
                }
                if *confidence_required {
                    hints.push("Include confidence (0.0-1.0) for each score".to_string());
                }
                if *justification_required {
                    hints.push("Include brief justification for each score".to_string());
                }

                // Grounding requirement
                if !grounding_refs.is_empty() {
                    hints.push(format!(
                        "Justification must reference: {}",
                        grounding_refs.join(", ")
                    ));
                }

                hints.join("\n")
            }
            Self::Classification {
                valid_categories,
                confidence_required,
                multi_label,
            } => {
                let mut hints = vec![];
                if !valid_categories.is_empty() {
                    hints.push(format!("Valid categories: {}", valid_categories.join(", ")));
                }
                if *confidence_required {
                    hints.push("Include confidence (0.0-1.0)".to_string());
                }
                if *multi_label {
                    hints.push("Multiple categories allowed".to_string());
                } else {
                    hints.push("Select exactly one category".to_string());
                }
                hints.join("\n")
            }
            Self::Extraction {
                required_fields,
                optional_fields,
            } => {
                let mut hints = vec![];
                if !required_fields.is_empty() {
                    hints.push(format!("Required fields: {}", required_fields.join(", ")));
                }
                if !optional_fields.is_empty() {
                    hints.push(format!("Optional fields: {}", optional_fields.join(", ")));
                }
                hints.join("\n")
            }
            Self::Freeform { max_tokens } => {
                format!("Maximum {max_tokens} tokens")
            }
        }
    }
}

impl Default for OutputContract {
    fn default() -> Self {
        Self::reasoning()
    }
}

/// Task frame: what capability is currently active.
///
/// This is per-agent or per-capability framing.
/// Now includes an `OutputContract` that declares expected output shape.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskFrame {
    /// Task identifier
    pub task: String,

    /// Expected output type (brief description)
    pub output_type: String,

    /// Output contract: formal expectations for output shape
    pub output_contract: OutputContract,

    /// Any task-specific hints (keep minimal)
    pub hints: Vec<String>,
}

impl Default for TaskFrame {
    fn default() -> Self {
        Self {
            task: "analyze".to_string(),
            output_type: "structured_response".to_string(),
            output_contract: OutputContract::reasoning(),
            hints: vec![],
        }
    }
}

impl TaskFrame {
    /// Create a task frame for reasoning/analysis.
    #[must_use]
    pub fn reason() -> Self {
        Self {
            task: "reason".to_string(),
            output_type: "reasoned_conclusion".to_string(),
            output_contract: OutputContract::reasoning(),
            hints: vec![],
        }
    }

    /// Create a task frame for evaluation/scoring.
    #[must_use]
    pub fn evaluate() -> Self {
        Self {
            task: "evaluate".to_string(),
            output_type: "scores_with_justification".to_string(),
            output_contract: OutputContract::evaluation(),
            hints: vec![],
        }
    }

    /// Create a task frame for planning.
    #[must_use]
    pub fn plan() -> Self {
        Self {
            task: "plan".to_string(),
            output_type: "ordered_steps".to_string(),
            output_contract: OutputContract::planning(),
            hints: vec![],
        }
    }

    /// Create a task frame for classification.
    #[must_use]
    pub fn classify() -> Self {
        Self {
            task: "classify".to_string(),
            output_type: "category_with_confidence".to_string(),
            output_contract: OutputContract::classification(),
            hints: vec![],
        }
    }

    /// Create a task frame for extraction.
    #[must_use]
    pub fn extract(required_fields: Vec<String>) -> Self {
        Self {
            task: "extract".to_string(),
            output_type: "structured_fields".to_string(),
            output_contract: OutputContract::extraction(required_fields),
            hints: vec![],
        }
    }

    /// Create a custom task frame with explicit contract.
    #[must_use]
    pub fn custom(task: impl Into<String>, contract: OutputContract) -> Self {
        Self {
            task: task.into(),
            output_type: "custom".to_string(),
            output_contract: contract,
            hints: vec![],
        }
    }

    /// Get the output contract for this task frame.
    #[must_use]
    pub fn contract(&self) -> &OutputContract {
        &self.output_contract
    }

    fn render(&self) -> String {
        let mut lines = vec![
            format!("TASK: {}", self.task),
            format!("OUTPUT_TYPE: {}", self.output_type),
        ];

        // Add output contract hints
        let contract_hint = self.output_contract.as_prompt_hint();
        if !contract_hint.is_empty() {
            lines.push(format!("OUTPUT_CONTRACT:\n{contract_hint}"));
        }

        if !self.hints.is_empty() {
            for hint in &self.hints {
                lines.push(format!("HINT: {hint}"));
            }
        }

        lines.join("\n")
    }
}

/// State injection: structured data replacing narrative prompting.
///
/// This is where Polars-derived features, ranked candidates, and
/// computed summaries go. NOT as text descriptions, but as structured data.
///
/// # Example
///
/// Instead of: "Here is a long description of recent events…"
///
/// Inject:
/// ```text
/// STATE:
/// - top_events: [E17, E04, E22]
/// - confidence_scores: [0.91, 0.76, 0.63]
/// - trend_slope: -0.12
/// - constraint_violations: 1
/// ```
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StateInjection {
    /// Scalar values (numbers, booleans)
    pub scalars: HashMap<String, StateValue>,

    /// List values (rankings, candidates)
    pub lists: HashMap<String, Vec<StateValue>>,

    /// Structured records
    pub records: Vec<StateRecord>,

    /// RECALL CONTEXT - explicitly separated from evidence.
    /// Validators MUST reject citations referencing this namespace.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recall_context: Option<crate::recall::RecallContext>,
}

impl StateInjection {
    /// Create empty state injection.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a scalar value.
    #[must_use]
    pub fn with_scalar(mut self, key: impl Into<String>, value: impl Into<StateValue>) -> Self {
        self.scalars.insert(key.into(), value.into());
        self
    }

    /// Add a list value.
    #[must_use]
    pub fn with_list(mut self, key: impl Into<String>, values: Vec<StateValue>) -> Self {
        self.lists.insert(key.into(), values);
        self
    }

    /// Add a structured record.
    #[must_use]
    pub fn with_record(mut self, record: StateRecord) -> Self {
        self.records.push(record);
        self
    }

    /// Add a boolean flag (convenience method).
    #[must_use]
    pub fn with_flag(self, key: impl Into<String>, value: bool) -> Self {
        self.with_scalar(key, value)
    }

    /// Check if state is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.scalars.is_empty()
            && self.lists.is_empty()
            && self.records.is_empty()
            && self
                .recall_context
                .as_ref()
                .map_or(true, |r: &crate::recall::RecallContext| r.is_empty())
    }

    /// Add recall context to the state.
    #[must_use]
    pub fn with_recall_context(mut self, context: crate::recall::RecallContext) -> Self {
        self.recall_context = Some(context);
        self
    }

    /// Render the state injection as a string.
    #[must_use]
    pub fn render(&self) -> String {
        let mut lines = vec!["STATE:".to_string()];

        // Scalars
        for (key, value) in &self.scalars {
            lines.push(format!("  {key}: {value}"));
        }

        // Lists
        for (key, values) in &self.lists {
            let formatted: Vec<String> = values.iter().map(ToString::to_string).collect();
            lines.push(format!("  {key}: [{}]", formatted.join(", ")));
        }

        // Records
        for record in &self.records {
            lines.push(format!("  {}:", record.name));
            for (k, v) in &record.fields {
                lines.push(format!("    {k}: {v}"));
            }
        }

        // Recall context (NOT EVIDENCE)
        if let Some(recall) = &self.recall_context {
            lines.push(String::new());
            lines.push(Self::render_recall_context(recall));
        }

        lines.join("\n")
    }

    /// Render recall context with explicit "NOT EVIDENCE" marker.
    fn render_recall_context(recall: &crate::recall::RecallContext) -> String {
        let mut lines =
            vec!["RECALL_CONTEXT (informational only - NOT citable as evidence):".to_string()];

        if !recall.similar_failures.is_empty() {
            lines.push("  similar_failures:".to_string());
            for hint in &recall.similar_failures {
                lines.push(format!(
                    "    - [{}] {} (score:{:.2})",
                    hint.relevance, hint.summary, hint.score
                ));
            }
        }

        if !recall.similar_successes.is_empty() {
            lines.push("  similar_successes:".to_string());
            for hint in &recall.similar_successes {
                lines.push(format!(
                    "    - [{}] {} (score:{:.2})",
                    hint.relevance, hint.summary, hint.score
                ));
            }
        }

        if !recall.suggested_runbooks.is_empty() {
            lines.push("  suggested_runbooks:".to_string());
            for hint in &recall.suggested_runbooks {
                lines.push(format!(
                    "    - [{}] {} (score:{:.2})",
                    hint.relevance, hint.summary, hint.score
                ));
            }
        }

        if !recall.recommended_adapter_ids.is_empty() {
            lines.push(format!(
                "  recommended_adapters: [{}]",
                recall.recommended_adapter_ids.join(", ")
            ));
        }

        if !recall.anti_patterns.is_empty() {
            lines.push("  anti_patterns:".to_string());
            for hint in &recall.anti_patterns {
                lines.push(format!(
                    "    - [{}] {} (score:{:.2})",
                    hint.relevance, hint.summary, hint.score
                ));
            }
        }

        lines.join("\n")
    }
}

/// A value in state injection.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum StateValue {
    Int(i64),
    Float(f64),
    Bool(bool),
    String(String),
}

impl std::fmt::Display for StateValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Int(v) => write!(f, "{v}"),
            Self::Float(v) => write!(f, "{v:.2}"),
            Self::Bool(v) => write!(f, "{v}"),
            Self::String(v) => write!(f, "{v}"),
        }
    }
}

impl From<i64> for StateValue {
    fn from(v: i64) -> Self {
        Self::Int(v)
    }
}

impl From<f64> for StateValue {
    fn from(v: f64) -> Self {
        Self::Float(v)
    }
}

impl From<bool> for StateValue {
    fn from(v: bool) -> Self {
        Self::Bool(v)
    }
}

impl From<&str> for StateValue {
    fn from(v: &str) -> Self {
        Self::String(v.to_string())
    }
}

impl From<String> for StateValue {
    fn from(v: String) -> Self {
        Self::String(v)
    }
}

/// A structured record in state injection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateRecord {
    /// Record type/name
    pub name: String,
    /// Fields
    pub fields: HashMap<String, StateValue>,
}

impl StateRecord {
    /// Create a new state record.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            fields: HashMap::new(),
        }
    }

    /// Add a field.
    #[must_use]
    pub fn with_field(mut self, key: impl Into<String>, value: impl Into<StateValue>) -> Self {
        self.fields.insert(key.into(), value.into());
        self
    }
}

/// User intent: minimal, often just intent + criteria.
///
/// In many Burn systems, this is barely a prompt at all.
///
/// # Example
///
/// ```text
/// INTENT: evaluate_options
/// CRITERIA: risk_adjusted_outcome
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserIntent {
    /// The intent identifier
    pub intent: String,

    /// Criteria for evaluation (if applicable)
    pub criteria: Option<String>,

    /// Any additional parameters (keep minimal)
    pub params: HashMap<String, String>,
}

impl Default for UserIntent {
    fn default() -> Self {
        Self {
            intent: "analyze".to_string(),
            criteria: None,
            params: HashMap::new(),
        }
    }
}

impl UserIntent {
    /// Create a new user intent.
    #[must_use]
    pub fn new(intent: impl Into<String>) -> Self {
        Self {
            intent: intent.into(),
            criteria: None,
            params: HashMap::new(),
        }
    }

    /// Add criteria.
    #[must_use]
    pub fn with_criteria(mut self, criteria: impl Into<String>) -> Self {
        self.criteria = Some(criteria.into());
        self
    }

    /// Add a parameter.
    #[must_use]
    pub fn with_param(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.params.insert(key.into(), value.into());
        self
    }

    fn render(&self) -> String {
        let mut lines = vec![format!("INTENT: {}", self.intent)];

        if let Some(c) = &self.criteria {
            lines.push(format!("CRITERIA: {c}"));
        }

        for (k, v) in &self.params {
            lines.push(format!("{}: {v}", k.to_uppercase()));
        }

        lines.join("\n")
    }
}

/// Builder for creating prompt stacks.
#[derive(Debug)]
pub struct PromptStackBuilder {
    version: Option<PromptVersion>,
    priming: Option<ModelPriming>,
    policy: Option<RolePolicy>,
    task_frame: Option<TaskFrame>,
    state: Option<StateInjection>,
    intent: Option<UserIntent>,
}

impl Default for PromptStackBuilder {
    fn default() -> Self {
        Self {
            version: None,
            priming: None,
            policy: None,
            task_frame: None,
            state: None,
            intent: None,
        }
    }
}

impl PromptStackBuilder {
    /// Create a new builder.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the prompt version.
    #[must_use]
    pub fn version(mut self, version: PromptVersion) -> Self {
        self.version = Some(version);
        self
    }

    /// Set model priming.
    #[must_use]
    pub fn priming(mut self, priming: ModelPriming) -> Self {
        self.priming = Some(priming);
        self
    }

    /// Set role policy.
    #[must_use]
    pub fn policy(mut self, policy: RolePolicy) -> Self {
        self.policy = Some(policy);
        self
    }

    /// Set task frame.
    #[must_use]
    pub fn task_frame(mut self, task_frame: TaskFrame) -> Self {
        self.task_frame = Some(task_frame);
        self
    }

    /// Set state injection.
    #[must_use]
    pub fn state(mut self, state: StateInjection) -> Self {
        self.state = Some(state);
        self
    }

    /// Set user intent.
    #[must_use]
    pub fn intent(mut self, intent: UserIntent) -> Self {
        self.intent = Some(intent);
        self
    }

    /// Build the prompt stack.
    #[must_use]
    pub fn build(self) -> PromptStack {
        PromptStack {
            version: self.version.unwrap_or(PromptVersion::reasoning_v1_llama3()),
            priming: self
                .priming
                .unwrap_or_else(ModelPriming::reasoning_component),
            policy: self.policy.unwrap_or_default(),
            task_frame: self.task_frame.unwrap_or_default(),
            state: self.state.unwrap_or_default(),
            intent: self.intent.unwrap_or_default(),
        }
    }
}

/// Model-specific prompt optimization hints.
///
/// Because you control tokenizer/context/quantization in Burn,
/// prompts must be optimized for the exact configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelOptimization {
    /// Model class (affects prompt style)
    pub model_class: ModelClass,

    /// Maximum tokens for prompt (leave room for generation)
    pub max_prompt_tokens: usize,

    /// Whether to use explicit output schemas
    pub explicit_schemas: bool,

    /// Whether to reduce narrative language
    pub minimal_narrative: bool,
}

/// Model class for optimization hints.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ModelClass {
    /// 3B class (Qwen, TinyLlama): shorter spans, explicit schemas
    Small,
    /// 7-8B class (Llama 3): richer framing, implicit reasoning
    Medium,
    /// 13B+ class: more tolerance for complexity
    Large,
}

impl ModelOptimization {
    /// Optimization for small models (3B class).
    #[must_use]
    pub fn small() -> Self {
        Self {
            model_class: ModelClass::Small,
            max_prompt_tokens: 1024,
            explicit_schemas: true,
            minimal_narrative: true,
        }
    }

    /// Optimization for medium models (7-8B class).
    #[must_use]
    pub fn medium() -> Self {
        Self {
            model_class: ModelClass::Medium,
            max_prompt_tokens: 2048,
            explicit_schemas: false,
            minimal_narrative: false,
        }
    }

    /// Optimization for large models (13B+ class).
    #[must_use]
    pub fn large() -> Self {
        Self {
            model_class: ModelClass::Large,
            max_prompt_tokens: 4096,
            explicit_schemas: false,
            minimal_narrative: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prompt_stack_render() {
        let stack = PromptStackBuilder::new()
            .priming(ModelPriming::reasoning_component())
            .task_frame(TaskFrame::evaluate())
            .state(
                StateInjection::new()
                    .with_scalar("confidence", 0.85)
                    .with_scalar("violations", 0_i64)
                    .with_list("candidates", vec!["A".into(), "B".into(), "C".into()]),
            )
            .intent(UserIntent::new("rank_options").with_criteria("risk_adjusted"))
            .build();

        let rendered = stack.render();

        assert!(rendered.contains("deterministic reasoning component"));
        assert!(rendered.contains("TASK: evaluate"));
        assert!(rendered.contains("confidence: 0.85"));
        assert!(rendered.contains("candidates: [A, B, C]"));
        assert!(rendered.contains("INTENT: rank_options"));
        assert!(rendered.contains("CRITERIA: risk_adjusted"));
    }

    #[test]
    fn test_minimal_prompt() {
        // A minimal prompt should be short
        let stack = PromptStackBuilder::new()
            .task_frame(TaskFrame::classify())
            .intent(UserIntent::new("classify"))
            .build();

        let rendered = stack.render();
        let token_estimate = stack.estimated_tokens();

        // Should be relatively short
        assert!(token_estimate < 200);
        assert!(rendered.contains("TASK: classify"));
    }

    #[test]
    fn test_state_injection() {
        let state = StateInjection::new()
            .with_scalar("trend_slope", -0.12)
            .with_scalar("is_anomaly", false)
            .with_list("top_events", vec!["E17".into(), "E04".into()])
            .with_record(
                StateRecord::new("summary")
                    .with_field("total", 42_i64)
                    .with_field("status", "healthy"),
            );

        let rendered = state.render();

        assert!(rendered.contains("trend_slope: -0.12"));
        assert!(rendered.contains("is_anomaly: false"));
        assert!(rendered.contains("top_events: [E17, E04]"));
        assert!(rendered.contains("summary:"));
        assert!(rendered.contains("total: 42"));
    }

    #[test]
    fn test_model_priming_variants() {
        let reasoning = ModelPriming::reasoning_component();
        let scoring = ModelPriming::scoring_component();
        let planning = ModelPriming::planning_component();

        assert!(reasoning.identity.contains("reasoning"));
        assert!(scoring.identity.contains("scoring"));
        assert!(planning.identity.contains("planning"));
    }

    #[test]
    fn test_prompt_version_display() {
        let version = PromptVersion::reasoning_v1_llama3();
        let display = format!("{version}");
        assert_eq!(display, "reasoning:v1:llama3");
    }

    #[test]
    fn test_versioned_prompt_stacks() {
        let reasoning = PromptStack::reasoning_v1_llama3();
        let scoring = PromptStack::scoring_v1_llama3();
        let planning = PromptStack::planning_v1_llama3();

        assert_eq!(reasoning.version.name, "reasoning");
        assert_eq!(scoring.version.name, "scoring");
        assert_eq!(planning.version.name, "planning");
    }

    #[test]
    fn test_validate_for_compatible_config() {
        use crate::config::LlmConfig;

        let stack = PromptStack::reasoning_v1_llama3();
        let config = LlmConfig::default(); // llama3-8b

        assert!(stack.validate_for(&config).is_ok());
    }

    #[test]
    fn test_validate_for_wrong_model_family() {
        use crate::config::LlmConfig;

        let stack = PromptStack::reasoning_v1_llama3();
        let mut config = LlmConfig::default();
        config.model_id = "llama2-7b".to_string();

        let result = stack.validate_for(&config);
        assert!(result.is_err());
    }

    #[test]
    fn test_output_contract_reasoning() {
        let contract = OutputContract::reasoning();
        match contract {
            OutputContract::Reasoning {
                requires_conclusion,
                allows_uncertainty,
                max_steps,
                ..
            } => {
                assert!(requires_conclusion);
                assert!(allows_uncertainty);
                assert_eq!(max_steps, Some(5));
            }
            _ => panic!("Expected Reasoning contract"),
        }
    }

    #[test]
    fn test_output_contract_evaluation() {
        let contract = OutputContract::evaluation();
        match contract {
            OutputContract::Evaluation {
                score_range,
                confidence_required,
                justification_required,
                ..
            } => {
                assert_eq!(score_range, (0.0, 1.0));
                assert!(confidence_required);
                assert!(justification_required);
            }
            _ => panic!("Expected Evaluation contract"),
        }
    }

    #[test]
    fn test_output_contract_planning() {
        let contract = OutputContract::planning();
        match contract {
            OutputContract::Planning {
                requires_ordered_steps,
                max_steps,
                ..
            } => {
                assert!(requires_ordered_steps);
                assert_eq!(max_steps, 10);
            }
            _ => panic!("Expected Planning contract"),
        }
    }

    #[test]
    fn test_task_frame_includes_contract() {
        let frame = TaskFrame::evaluate();
        assert!(matches!(
            frame.contract(),
            OutputContract::Evaluation { .. }
        ));

        let frame = TaskFrame::plan();
        assert!(matches!(frame.contract(), OutputContract::Planning { .. }));

        let frame = TaskFrame::classify();
        assert!(matches!(
            frame.contract(),
            OutputContract::Classification { .. }
        ));
    }

    #[test]
    fn test_output_contract_prompt_hint() {
        let contract = OutputContract::evaluation();
        let hint = contract.as_prompt_hint();

        assert!(hint.contains("Score each option"));
        assert!(hint.contains("confidence"));
        assert!(hint.contains("justification"));
    }

    #[test]
    fn test_task_frame_render_includes_contract() {
        let frame = TaskFrame::evaluate();
        let rendered = frame.render();

        assert!(rendered.contains("TASK: evaluate"));
        assert!(rendered.contains("OUTPUT_CONTRACT:"));
        assert!(rendered.contains("Score each option"));
    }

    #[test]
    fn test_custom_task_frame() {
        let contract = OutputContract::Extraction {
            required_fields: vec!["name".into(), "value".into()],
            optional_fields: vec!["unit".into()],
        };
        let frame = TaskFrame::custom("extract_metrics", contract);

        assert_eq!(frame.task, "extract_metrics");
        assert!(matches!(
            frame.contract(),
            OutputContract::Extraction { .. }
        ));
    }
}
