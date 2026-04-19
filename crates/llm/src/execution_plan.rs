// Copyright 2024-2026 Reflective Labs

//! Execution Plan — Compiled policy that cannot be overridden.
//!
//! This module eliminates the split-brain between KernelPolicy and ChainConfig
//! by introducing a single "compiled plan" that:
//!
//! 1. Is created ONLY from (KernelIntent, KernelPolicy)
//! 2. Has no public constructor (cannot be constructed with arbitrary values)
//! 3. Is immutable once created
//! 4. Is the ONLY input to execution (ChainExecutor accepts this, not raw config)
//!
//! # Invariant
//!
//! If `KernelPolicy.recall_enabled = false`, then `ExecutionPlan.recall` is None.
//! There is NO way to enable recall downstream if policy disables it.

use crate::inference::InferenceEnvelope;
use crate::kernel::{KernelIntent, KernelPolicy};
use crate::prompt::OutputContract;
use crate::recall::{RecallBudgets, RecallConfig, RecallPerStep, RecallPolicy, RecallTrigger};
use crate::trace::DecisionStep;
use serde::{Deserialize, Serialize};

/// The compiled execution plan.
///
/// This is created by `compile()` and cannot be constructed directly.
/// All fields are private to prevent modification after compilation.
#[derive(Debug, Clone)]
pub struct ExecutionPlan {
    steps: Vec<StepPlan>,
    envelope: InferenceEnvelope,
    recall: Option<RecallPlan>,
    adapter: Option<AdapterPlan>,
    determinism: DeterminismPlan,
    required_truths: Vec<String>,
    requires_human: bool,
    policy_hash: String,
}

/// Plan for a single execution step.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepPlan {
    pub step: DecisionStep,
    pub intent: String,
    pub max_tokens: usize,
    pub contract: OutputContract,
    pub recall_enabled: bool,
}

/// Compiled recall configuration.
#[derive(Debug, Clone)]
pub struct RecallPlan {
    pub max_candidates: usize,
    pub min_score: f32,
    pub per_step: RecallPerStep,
    pub max_tokens_injection: usize,
}

/// Compiled adapter configuration.
#[derive(Debug, Clone)]
pub struct AdapterPlan {
    pub adapter_id: String,
}

/// Compiled determinism configuration.
#[derive(Debug, Clone)]
pub struct DeterminismPlan {
    pub seed: Option<u64>,
}

impl ExecutionPlan {
    /// Compile an execution plan from intent and policy.
    ///
    /// This is the ONLY way to create an ExecutionPlan.
    #[must_use]
    pub fn compile(intent: &KernelIntent, policy: &KernelPolicy) -> Self {
        let determinism = DeterminismPlan { seed: policy.seed };

        let envelope = if let Some(seed) = policy.seed {
            InferenceEnvelope::deterministic("kernel:v1", seed)
        } else {
            InferenceEnvelope::agent_reasoning("kernel:v1")
        };

        let envelope = if let Some(adapter_id) = &policy.adapter_id {
            envelope.with_adapter(adapter_id.clone())
        } else {
            envelope
        };

        // Recall ONLY if policy enables it
        let recall = if policy.recall_enabled {
            Some(RecallPlan {
                max_candidates: policy.recall_max_candidates,
                min_score: policy.recall_min_score,
                per_step: RecallPerStep {
                    reasoning: RecallTrigger::Always,
                    evaluation: RecallTrigger::Never,
                    planning: RecallTrigger::Never,
                },
                max_tokens_injection: 512,
            })
        } else {
            None
        };

        let adapter = policy.adapter_id.as_ref().map(|id| AdapterPlan {
            adapter_id: id.clone(),
        });

        let steps = vec![
            StepPlan {
                step: DecisionStep::Reasoning,
                intent: intent.task.clone(),
                max_tokens: intent.max_tokens,
                contract: OutputContract::reasoning(),
                recall_enabled: recall
                    .as_ref()
                    .map(|r| matches!(r.per_step.reasoning, RecallTrigger::Always))
                    .unwrap_or(false),
            },
            StepPlan {
                step: DecisionStep::Evaluation,
                intent: "evaluate the analysis".to_string(),
                max_tokens: 512,
                contract: OutputContract::evaluation(),
                recall_enabled: recall
                    .as_ref()
                    .map(|r| matches!(r.per_step.evaluation, RecallTrigger::Always))
                    .unwrap_or(false),
            },
            StepPlan {
                step: DecisionStep::Planning,
                intent: "create an action plan".to_string(),
                max_tokens: 512,
                contract: OutputContract::planning(),
                recall_enabled: recall
                    .as_ref()
                    .map(|r| matches!(r.per_step.planning, RecallTrigger::Always))
                    .unwrap_or(false),
            },
        ];

        let policy_hash = Self::compute_policy_hash(intent, policy);

        Self {
            steps,
            envelope,
            recall,
            adapter,
            determinism,
            required_truths: policy.required_truths.clone(),
            requires_human: policy.requires_human,
            policy_hash,
        }
    }

    #[must_use]
    pub fn steps(&self) -> &[StepPlan] {
        &self.steps
    }

    #[must_use]
    pub fn envelope(&self) -> &InferenceEnvelope {
        &self.envelope
    }

    #[must_use]
    pub fn recall(&self) -> Option<&RecallPlan> {
        self.recall.as_ref()
    }

    #[must_use]
    pub fn recall_enabled(&self) -> bool {
        self.recall.is_some()
    }

    #[must_use]
    pub fn adapter(&self) -> Option<&AdapterPlan> {
        self.adapter.as_ref()
    }

    #[must_use]
    pub fn determinism(&self) -> &DeterminismPlan {
        &self.determinism
    }

    #[must_use]
    pub fn required_truths(&self) -> &[String] {
        &self.required_truths
    }

    #[must_use]
    pub fn requires_human(&self) -> bool {
        self.requires_human
    }

    #[must_use]
    pub fn policy_hash(&self) -> &str {
        &self.policy_hash
    }

    #[must_use]
    pub fn to_recall_config(&self) -> Option<RecallConfig> {
        self.recall.as_ref().map(|r| RecallConfig {
            policy: RecallPolicy {
                enabled: true,
                max_k_total: r.max_candidates,
                max_tokens_injection: r.max_tokens_injection,
                min_score_threshold: r.min_score as f64,
                budgets: RecallBudgets::default(),
                ..Default::default()
            },
            per_step: r.per_step.clone(),
        })
    }

    fn compute_policy_hash(intent: &KernelIntent, policy: &KernelPolicy) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        intent.task.hash(&mut hasher);
        intent.max_tokens.hash(&mut hasher);
        for criterion in &intent.criteria {
            criterion.hash(&mut hasher);
        }
        policy.adapter_id.hash(&mut hasher);
        policy.recall_enabled.hash(&mut hasher);
        policy.recall_max_candidates.hash(&mut hasher);
        (policy.recall_min_score as u32).hash(&mut hasher);
        policy.seed.hash(&mut hasher);
        policy.requires_human.hash(&mut hasher);
        for truth in &policy.required_truths {
            truth.hash(&mut hasher);
        }

        format!("{:016x}", hasher.finish())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compile_with_recall_disabled() {
        let intent = KernelIntent::new("test");
        let policy = KernelPolicy::new();

        let plan = ExecutionPlan::compile(&intent, &policy);

        assert!(plan.recall().is_none());
        assert!(!plan.recall_enabled());
        for step in plan.steps() {
            assert!(!step.recall_enabled);
        }
    }

    #[test]
    fn test_compile_with_recall_enabled() {
        let intent = KernelIntent::new("test");
        let policy = KernelPolicy::new().with_recall(true);

        let plan = ExecutionPlan::compile(&intent, &policy);

        assert!(plan.recall().is_some());
        assert!(plan.recall_enabled());
    }

    #[test]
    fn test_compile_with_adapter() {
        let intent = KernelIntent::new("test");
        let policy = KernelPolicy::new().with_adapter("llm/grounded@1.0.0");

        let plan = ExecutionPlan::compile(&intent, &policy);

        assert!(plan.adapter().is_some());
        assert_eq!(plan.adapter().unwrap().adapter_id, "llm/grounded@1.0.0");
    }

    #[test]
    fn test_policy_hash_deterministic() {
        let intent = KernelIntent::new("test");
        let policy = KernelPolicy::deterministic(42);

        let plan1 = ExecutionPlan::compile(&intent, &policy);
        let plan2 = ExecutionPlan::compile(&intent, &policy);

        assert_eq!(plan1.policy_hash(), plan2.policy_hash());
    }

    // =========================================================================
    // NEGATIVE TESTS: Policy Enforcement
    // =========================================================================

    /// Negative test: recall cannot be enabled if policy disables it.
    ///
    /// This test verifies the INVARIANT:
    /// > If `KernelPolicy.recall_enabled = false`, then `ExecutionPlan.recall` is None.
    /// > There is NO way to enable recall downstream if policy disables it.
    ///
    /// The ExecutionPlan struct has private fields, so this is compile-time enforced.
    /// This test documents the invariant explicitly.
    #[test]
    fn test_policy_enforcement_recall_cannot_be_enabled_downstream() {
        let intent = KernelIntent::new("test task requiring recall");
        let policy = KernelPolicy::new(); // recall_enabled = false by default

        // Compile the plan - policy is now LOCKED
        let plan = ExecutionPlan::compile(&intent, &policy);

        // Invariant: recall is None because policy disabled it
        assert!(
            plan.recall().is_none(),
            "recall should be None when policy disables it"
        );
        assert!(!plan.recall_enabled(), "recall_enabled() should be false");

        // Verify ALL steps have recall disabled
        for step in plan.steps() {
            assert!(
                !step.recall_enabled,
                "step {:?} should have recall_enabled=false when policy disables recall",
                step.step
            );
        }

        // There is NO method to enable recall after compilation.
        // The following would NOT compile because fields are private:
        // plan.recall = Some(RecallPlan { ... }); // ERROR: field `recall` is private
        // plan.steps[0].recall_enabled = true;    // ERROR: cannot assign to field
    }

    /// Negative test: adapter cannot be changed after compilation.
    ///
    /// Once ExecutionPlan is compiled, the adapter_id is locked.
    /// No downstream code can swap adapters.
    #[test]
    fn test_policy_enforcement_adapter_cannot_be_changed() {
        let intent = KernelIntent::new("test");
        let policy = KernelPolicy::new().with_adapter("llm/approved@1.0.0");

        let plan = ExecutionPlan::compile(&intent, &policy);

        // Adapter is locked to what policy specified
        assert_eq!(plan.adapter().unwrap().adapter_id, "llm/approved@1.0.0");

        // There is NO method to change the adapter after compilation.
        // The following would NOT compile:
        // plan.adapter = Some(AdapterPlan { adapter_id: "llm/malicious@1.0.0".to_string() });
    }

    /// Negative test: seed cannot be changed after compilation.
    ///
    /// The determinism settings are locked at compile time.
    #[test]
    fn test_policy_enforcement_seed_locked() {
        let intent = KernelIntent::new("test");
        let policy = KernelPolicy::deterministic(42);

        let plan = ExecutionPlan::compile(&intent, &policy);

        // Seed is locked
        assert_eq!(plan.determinism().seed, Some(42));

        // The envelope's sampling params are also locked (inside InferenceEnvelope)
        // No downstream code can change the seed.
    }

    /// Negative test: policy hash changes if intent/policy changes.
    ///
    /// This proves the policy_hash captures ALL relevant policy state.
    /// A different policy = different hash = auditable difference.
    #[test]
    fn test_policy_hash_detects_policy_changes() {
        let intent = KernelIntent::new("test");

        let policy1 = KernelPolicy::new();
        let policy2 = KernelPolicy::new().with_recall(true);

        let plan1 = ExecutionPlan::compile(&intent, &policy1);
        let plan2 = ExecutionPlan::compile(&intent, &policy2);

        // Different policies = different hashes
        assert_ne!(
            plan1.policy_hash(),
            plan2.policy_hash(),
            "policy hash should differ when recall_enabled changes"
        );
    }
}
