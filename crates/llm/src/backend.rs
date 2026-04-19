// Copyright 2024-2026 Reflective Labs

//! LLM Backend Interface — The unification boundary for local and remote LLMs.
//!
//! # Migration Note
//!
//! The core backend types have been moved to `converge-core::backend`.
//! This module re-exports them for backward compatibility and adds
//! LLM-specific extensions.
//!
//! # The Unification Rule
//!
//! All model invocations—local or remote—must produce the same top-level artifact:
//! - `BackendResponse` containing `ProposedContent`(s)
//! - Plus a `ReplayTrace` that makes the invocation auditable, budgeted, and comparable
//!
//! "Interchangeable" means:
//! - Same request type
//! - Same output type
//! - Same contract evaluation surface
//! - Different execution backend
//!
//! # Determinism Guarantees
//!
//! | Backend | Determinism | ReplayTrace |
//! |---------|-------------|-----------|
//! | Local (converge-llm) | Strong (replay-eligible) | `LocalReplayTrace` |
//! | Remote (providers) | Bounded stochasticity (audit-eligible) | `RemoteReplayTrace` |
//!
//! Remote runs are:
//! - **Auditable**: Full request/response + metadata
//! - **Repeatable-ish**: Best effort (temp=0 helps)
//! - **Non-replayable**: Strictly (model versions, safety layers can shift)

use crate::error::LlmResult;

// Re-export kernel boundary types from converge-core
// These are the constitutional types shared across all kernels
pub use converge_core::kernel_boundary::{
    AdapterTrace,
    ContentKind,
    DataClassification,
    ExecutionEnv,
    LocalReplayTrace,
    // Proposal types
    ProposedContent,
    RecallTrace,
    RemoteReplayTrace,
    // ReplayTrace types
    ReplayTrace,
    Replayability,
    // Routing types (already in core)
    RiskTier,
    RoutingPolicy,
    SamplerParams,
};

// Re-export backend types from converge-core
// These are the unified interface types shared across all backends
// NOTE: We do NOT re-export the deprecated CoreLlmBackend trait.
// Use the LlmBackend trait defined in this module instead.
pub use converge_core::backend::{
    BackendAdapterPolicy,
    BackendBudgets,
    // Capabilities
    BackendCapability,
    BackendContractResult,
    // Error types
    BackendError,
    BackendPrompt,
    BackendRecallPolicy,
    // Request types
    BackendRequest,
    // Response types
    BackendResponse,
    BackendResult,
    BackendUsage,
    ContractReport,
    ContractSpec,
    Message,
    MessageRole,
};

// ============================================================================
// LlmBackend Trait (LLM-specific extension)
// ============================================================================

/// The unified backend interface for converge-llm.
///
/// This trait extends the core `LlmBackend` with LLM-specific error handling.
/// Implementations in converge-llm use `LlmResult` for error propagation.
///
/// Both local kernel and remote providers implement this trait.
pub trait LlmBackend: Send + Sync {
    /// Backend name for identification.
    fn name(&self) -> &str;

    /// Whether this backend supports deterministic replay.
    fn supports_replay(&self) -> bool;

    /// Execute an LLM request.
    ///
    /// Uses `LlmResult` for LLM-specific error handling.
    fn execute(&self, request: &BackendRequest) -> LlmResult<BackendResponse>;

    /// Check if this backend supports a specific capability.
    fn supports_capability(&self, capability: BackendCapability) -> bool;

    /// List all capabilities this backend supports.
    fn capabilities(&self) -> Vec<BackendCapability> {
        let all_caps = [
            BackendCapability::Replay,
            BackendCapability::Adapters,
            BackendCapability::Recall,
            BackendCapability::StepContracts,
            BackendCapability::FrontierReasoning,
            BackendCapability::FastIteration,
            BackendCapability::Offline,
            BackendCapability::Streaming,
            BackendCapability::Vision,
            BackendCapability::ToolUse,
        ];
        all_caps
            .iter()
            .filter(|cap| self.supports_capability(**cap))
            .copied()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_trace_link_replayability() {
        let local = ReplayTrace::Local(LocalReplayTrace {
            base_model_hash: "abc123".to_string(),
            adapter: None,
            tokenizer_hash: "tok123".to_string(),
            seed: 42,
            sampler: SamplerParams {
                temperature: 0.0,
                top_p: 1.0,
                top_k: None,
            },
            prompt_version: "v1".to_string(),
            recall: None,
            weights_mutated: false,
            execution_env: ExecutionEnv {
                device: "metal".to_string(),
                backend: "wgpu".to_string(),
                precision: "f32".to_string(),
            },
        });

        let remote = ReplayTrace::Remote(RemoteReplayTrace {
            provider_name: "anthropic".to_string(),
            provider_model_id: "claude-3-opus".to_string(),
            request_fingerprint: "req123".to_string(),
            response_fingerprint: "resp456".to_string(),
            temperature: 0.0,
            top_p: 1.0,
            max_tokens: 1024,
            provider_metadata: HashMap::new(),
            retried: false,
            retry_reasons: vec![],
            replayability: Replayability::BestEffort,
        });

        assert!(local.is_replay_eligible());
        assert!(!remote.is_replay_eligible());

        assert_eq!(local.replayability(), Replayability::Deterministic);
        assert_eq!(remote.replayability(), Replayability::BestEffort);
    }

    #[test]
    fn test_routing_policy() {
        let mut policy = RoutingPolicy {
            truth_preferences: HashMap::new(),
            risk_tier_backends: HashMap::new(),
            data_classification_backends: HashMap::new(),
            default_backend: "local".to_string(),
        };

        policy
            .truth_preferences
            .insert("grounded-answering".to_string(), "local".to_string());
        policy
            .risk_tier_backends
            .insert(RiskTier::Critical, vec!["local".to_string()]);
        policy
            .data_classification_backends
            .insert(DataClassification::Restricted, vec!["local".to_string()]);

        // Truth preference wins
        assert_eq!(
            policy.select_backend(
                &["grounded-answering".to_string()],
                RiskTier::Low,
                DataClassification::Public,
            ),
            "local"
        );

        // Risk tier for critical
        assert_eq!(
            policy.select_backend(
                &["unknown".to_string()],
                RiskTier::Critical,
                DataClassification::Public,
            ),
            "local"
        );

        // Default fallback
        assert_eq!(
            policy.select_backend(
                &["unknown".to_string()],
                RiskTier::Low,
                DataClassification::Public,
            ),
            "local"
        );
    }

    #[test]
    fn test_backend_budgets_default() {
        let budgets = BackendBudgets::default();
        assert_eq!(budgets.max_tokens, 1024);
        assert_eq!(budgets.max_iterations, 1);
        assert_eq!(budgets.latency_ceiling_ms, 0);
    }

    // =========================================================================
    // MockRemoteBackend for Testing
    // =========================================================================

    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Mock remote backend for testing without real API calls.
    ///
    /// This backend simulates a remote provider (like Anthropic) and:
    /// - Returns `RemoteReplayTrace` (audit-only, NOT replayable)
    /// - Can be configured to return different responses
    /// - Tracks calls for verification
    struct MockRemoteBackend {
        name: String,
        response_text: String,
        call_count: AtomicUsize,
    }

    impl MockRemoteBackend {
        fn new(name: &str, response_text: &str) -> Self {
            Self {
                name: name.to_string(),
                response_text: response_text.to_string(),
                call_count: AtomicUsize::new(0),
            }
        }

        #[allow(dead_code)]
        fn call_count(&self) -> usize {
            self.call_count.load(Ordering::SeqCst)
        }
    }

    impl LlmBackend for MockRemoteBackend {
        fn name(&self) -> &str {
            &self.name
        }

        fn supports_replay(&self) -> bool {
            false // Remote backends do NOT support deterministic replay
        }

        fn execute(&self, request: &BackendRequest) -> crate::error::LlmResult<BackendResponse> {
            self.call_count.fetch_add(1, Ordering::SeqCst);

            // Generate request/response fingerprints for audit
            let request_fingerprint = format!("req_{:08x}", request.intent_id.len());
            let response_fingerprint = format!("resp_{:08x}", self.response_text.len());

            Ok(BackendResponse {
                proposals: vec![ProposedContent {
                    id: "mock-proposal-001".to_string(),
                    kind: ContentKind::Reasoning,
                    content: self.response_text.clone(),
                    structured: None,
                    confidence: Some(0.9),
                    requires_human: false,
                }],
                contract_report: ContractReport {
                    results: vec![],
                    all_passed: true,
                },
                trace_link: ReplayTrace::Remote(RemoteReplayTrace {
                    provider_name: self.name.clone(),
                    provider_model_id: "mock-model-v1".to_string(),
                    request_fingerprint,
                    response_fingerprint,
                    temperature: 0.0,
                    top_p: 1.0,
                    max_tokens: request.budgets.max_tokens,
                    provider_metadata: HashMap::new(),
                    retried: false,
                    retry_reasons: vec![],
                    replayability: Replayability::BestEffort,
                }),
                usage: BackendUsage {
                    input_tokens: 100,
                    output_tokens: 50,
                    total_tokens: 150,
                    latency_ms: 200,
                    cost_microdollars: Some(300), // $0.0003
                },
            })
        }

        fn supports_capability(&self, capability: BackendCapability) -> bool {
            matches!(capability, BackendCapability::FrontierReasoning)
        }
    }

    // =========================================================================
    // E2E Tests: Remote Backend Audit-Only Behavior
    // =========================================================================

    /// E2E test: Remote backend responses are audit-only (NOT replayable).
    ///
    /// This proves the invariant:
    /// > Remote runs are auditable but NOT deterministically replayable.
    #[test]
    fn test_remote_backend_is_audit_only_not_replayable() {
        let backend = MockRemoteBackend::new("mock-anthropic", "This is a mock response");

        let request = BackendRequest {
            intent_id: "test-intent-001".to_string(),
            truth_ids: vec!["grounded-answering".to_string()],
            prompt_version: "v1".to_string(),
            state_injection_hash: "state123".to_string(),
            prompt: BackendPrompt::Text("What is 2+2?".to_string()),
            contracts: vec![],
            budgets: BackendBudgets::default(),
            recall_policy: None,
            adapter_policy: None,
            retry_policy: None,
        };

        let response = backend.execute(&request).expect("should succeed");

        // CRITICAL ASSERTION: Remote trace is NOT replay-eligible
        assert!(
            !response.trace_link.is_replay_eligible(),
            "Remote backend response must NOT be replay-eligible"
        );

        // Remote trace should have BestEffort replayability
        assert_eq!(
            response.trace_link.replayability(),
            Replayability::BestEffort,
            "Remote backend should have BestEffort replayability"
        );

        // Verify it's a RemoteReplayTrace (not LocalReplayTrace)
        match &response.trace_link {
            ReplayTrace::Remote(remote) => {
                assert_eq!(remote.provider_name, "mock-anthropic");
                // Audit trail fields should be populated
                assert!(!remote.request_fingerprint.is_empty());
                assert!(!remote.response_fingerprint.is_empty());
            }
            ReplayTrace::Local(_) => {
                panic!("Remote backend should return RemoteReplayTrace, not LocalReplayTrace");
            }
        }
    }

    /// E2E test: Verify backend correctly reports it doesn't support replay.
    #[test]
    fn test_remote_backend_does_not_support_replay_capability() {
        let backend = MockRemoteBackend::new("mock-anthropic", "test");

        assert!(
            !backend.supports_replay(),
            "Remote backend must report supports_replay = false"
        );

        assert!(
            !backend.supports_capability(BackendCapability::Replay),
            "Remote backend must not support Replay capability"
        );
    }

    // =========================================================================
    // E2E Tests: Routing Policy Enforcement
    // =========================================================================

    /// E2E test: default_deny_remote policy blocks remote for critical/restricted.
    #[test]
    fn test_routing_policy_default_deny_remote() {
        let policy = RoutingPolicy::default_deny_remote();

        // Critical risk tier -> only local allowed
        assert!(
            policy.is_backend_allowed("local", RiskTier::Critical, DataClassification::Public),
            "Local should be allowed for critical risk"
        );
        assert!(
            !policy.is_backend_allowed("anthropic", RiskTier::Critical, DataClassification::Public),
            "Remote should be DENIED for critical risk"
        );

        // Restricted data -> only local allowed
        assert!(
            policy.is_backend_allowed("local", RiskTier::Low, DataClassification::Restricted),
            "Local should be allowed for restricted data"
        );
        assert!(
            !policy.is_backend_allowed("anthropic", RiskTier::Low, DataClassification::Restricted),
            "Remote should be DENIED for restricted data"
        );

        // Confidential data -> only local allowed
        assert!(
            !policy.is_backend_allowed("openai", RiskTier::Low, DataClassification::Confidential),
            "Remote should be DENIED for confidential data"
        );
    }

    /// E2E test: Routing policy respects truth preferences.
    #[test]
    fn test_routing_policy_truth_preferences() {
        let mut policy = RoutingPolicy::default();
        policy
            .truth_preferences
            .insert("grounded-answering".to_string(), "local".to_string());
        policy
            .truth_preferences
            .insert("frontier-reasoning".to_string(), "anthropic".to_string());

        // Grounded answering -> local
        assert_eq!(
            policy.select_backend(
                &["grounded-answering".to_string()],
                RiskTier::Low,
                DataClassification::Public,
            ),
            "local"
        );

        // Frontier reasoning -> anthropic
        assert_eq!(
            policy.select_backend(
                &["frontier-reasoning".to_string()],
                RiskTier::Low,
                DataClassification::Public,
            ),
            "anthropic"
        );
    }

    /// E2E test: Local and remote backends have different replayability.
    #[test]
    fn test_local_vs_remote_replayability_difference() {
        // Simulate a local trace (deterministic)
        let local_trace = ReplayTrace::Local(LocalReplayTrace {
            base_model_hash: "abc123".to_string(),
            adapter: None,
            tokenizer_hash: "tok123".to_string(),
            seed: 42,
            sampler: SamplerParams {
                temperature: 0.0,
                top_p: 1.0,
                top_k: None,
            },
            prompt_version: "v1".to_string(),
            recall: None,
            weights_mutated: false,
            execution_env: ExecutionEnv {
                device: "metal".to_string(),
                backend: "wgpu".to_string(),
                precision: "f32".to_string(),
            },
        });

        // Simulate a remote trace (audit-only)
        let remote_trace = ReplayTrace::Remote(RemoteReplayTrace {
            provider_name: "anthropic".to_string(),
            provider_model_id: "claude-3-opus".to_string(),
            request_fingerprint: "req123".to_string(),
            response_fingerprint: "resp456".to_string(),
            temperature: 0.0,
            top_p: 1.0,
            max_tokens: 1024,
            provider_metadata: HashMap::new(),
            retried: false,
            retry_reasons: vec![],
            replayability: Replayability::BestEffort,
        });

        // Key differences
        assert!(
            local_trace.is_replay_eligible(),
            "Local trace IS replay-eligible"
        );
        assert!(
            !remote_trace.is_replay_eligible(),
            "Remote trace is NOT replay-eligible"
        );

        assert_eq!(local_trace.replayability(), Replayability::Deterministic);
        assert_eq!(remote_trace.replayability(), Replayability::BestEffort);
    }

    /// E2E test: Routing policy default backend selection.
    #[test]
    fn test_routing_policy_default_backend() {
        let mut policy = RoutingPolicy::default();
        policy.default_backend = "local".to_string();

        // No matching preferences -> default backend
        assert_eq!(
            policy.select_backend(
                &["unknown-truth".to_string()],
                RiskTier::Low,
                DataClassification::Public,
            ),
            "local"
        );

        // Change default
        policy.default_backend = "anthropic".to_string();
        assert_eq!(
            policy.select_backend(
                &["unknown-truth".to_string()],
                RiskTier::Low,
                DataClassification::Public,
            ),
            "anthropic"
        );
    }
}
