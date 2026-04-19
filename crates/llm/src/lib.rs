// Copyright 2024-2026 Reflective Labs

//! # Converge LLM — Reasoning Kernel
//!
//! A bounded reasoning kernel for the Converge agent framework.
//!
//! ## Architecture
//!
//! converge-llm is a **reasoning kernel** with a single public API:
//!
//! ```ignore
//! run_kernel(intent, context, policy) -> Vec<KernelProposal>
//! ```
//!
//! Everything inside (ChainExecutor, PromptStack, InferenceEnvelope, etc.)
//! is an implementation detail. The kernel boundary enforces:
//!
//! - **ProposedFact boundary**: Outputs are proposals, never facts
//! - **Explicit authority**: Adapter selection comes from policy
//! - **Transparent determinism**: Full trace metadata for reproducibility
//! - **Human authority**: Proposals can require human approval
//!
//! ## Axiom Compliance
//!
//! The kernel model maintains converge-core axiom compliance:
//!
//! | Axiom | How Enforced |
//! |-------|--------------|
//! | Agents Suggest, Engines Decide | Kernel outputs `KernelProposal`, not `Fact` |
//! | Append-Only Truth | Kernel cannot mutate shared truth |
//! | Explicit Authority | Adapter from `KernelPolicy`, not emergent |
//! | Transparent Determinism | `TraceLink` in every proposal |
//! | Human Authority First-Class | `requires_human` flag on proposals |
//!
//! ## Internal Capabilities
//!
//! Inside the kernel (not public API):
//! - **ChainExecutor**: 3-step reasoning pipeline
//! - **LlamaEngine/TinyLlamaEngine/GemmaEngine**: Actual inference
//! - **Recall**: Context retrieval and injection
//! - **Adapters/LoRA**: Model customization
//! - **Contracts**: Output validation

pub mod backend;
pub mod execution_plan;
pub mod kernel;

// Provider abstraction (migrated from converge-core for purity)
pub mod prompt_dsl;
pub mod provider;

// Internal modules (kernel implementation details)
pub mod adapter;
pub mod adapter_registry;
pub mod adversarial;
pub mod agent;
pub mod bridge;
pub mod chain;
pub mod config;
pub mod contract_stress;
#[cfg(any(feature = "llama3", feature = "tiny"))]
pub mod engine;
pub mod error;
#[cfg(feature = "gemma")]
pub mod gemma;
pub mod inference;
pub mod lora;
pub mod lora_merge;
pub mod model;
pub mod observability;
pub mod prompt;
pub mod recall;
pub mod tokenizer;
pub mod trace;
pub mod validation;

// Object store adapter registry (feature-gated)
#[cfg(feature = "storage")]
pub mod storage_registry;

// Remote backend providers (feature-gated)
#[cfg(feature = "anthropic")]
pub mod anthropic;

// gRPC server (standalone GPU binary)
#[cfg(feature = "server")]
pub mod server;

// gRPC client backend (remote GPU inference)
#[cfg(all(feature = "grpc-client", not(feature = "server")))]
pub mod grpc_backend;

// ============================================================================
// Public API: Backend Interface (unified local/remote)
// ============================================================================

pub use backend::{
    AdapterTrace,
    BackendAdapterPolicy,
    BackendBudgets,
    BackendCapability,
    BackendPrompt,
    // Policies
    BackendRecallPolicy,
    // Request/Response
    BackendRequest,
    BackendResponse,
    BackendUsage,
    ContentKind,
    ContractReport,
    ContractSpec,
    DataClassification,
    ExecutionEnv,
    // Core trait
    LlmBackend,
    LocalReplayTrace,
    Message,
    MessageRole,
    // Proposals
    ProposedContent,
    RecallTrace,
    RemoteReplayTrace,
    // ReplayTrace (two shapes)
    ReplayTrace,
    Replayability,
    RiskTier,
    // Routing
    RoutingPolicy,
    SamplerParams,
};

// ============================================================================
// Public API: Kernel (local inference)
// ============================================================================

pub use kernel::{
    ContextFact,
    ContractResult as KernelContractResult,
    KernelContext,
    KernelIntent,
    KernelPolicy,
    KernelProposal,
    KernelRunner,
    KernelTraceLink,
    ProposalKind,
    ProposalRecallMetadata,
    // Recall provenance validation
    RecallImpact,
    RecallProvenanceError,
    RecallProvenanceMissing,
    // Replayability tracking (downgrade propagation)
    ReplayabilityDowngradeReason,
    check_recall_as_evidence_violation,
    is_opaque_recall_id,
    // Opaque recall ID functions (Recall ≠ Evidence enforcement)
    opaque_recall_id,
    run_kernel,
    validate_chain_recall_provenance,
    validate_recall_provenance,
};

// Execution Plan (compiled policy that cannot be overridden)
pub use execution_plan::{AdapterPlan, DeterminismPlan, ExecutionPlan, RecallPlan, StepPlan};

// RecallMetadata from trace module (used for decision trace observability)
pub use trace::RecallMetadata;

// Suggestor types (legacy PromptTemplate kept for compatibility)
pub use agent::{LlmAgent, PromptTemplate, ReasoningConfig};

// Configuration
pub use config::{ConfigValidationError, LlmConfig, Precision, TokenizerConfig, TokenizerType};

// Errors
pub use error::{LlmError, LlmResult};

// Inference
pub use inference::{
    FinishReason, GenerationParams, GenerationResult, InferenceEngine, InferenceEnvelope,
    SeedPolicy, StoppingCriteria, TokenizerSnapshot,
};

// Model
pub use model::{LlamaModel, ModelMetadata};

// Prompt architecture (the correct way for Burn)
pub use prompt::{
    ModelClass, ModelOptimization, ModelPriming, OutputContract, OutputFormat, PromptStack,
    PromptStackBuilder, PromptVersion, RolePolicy, ScoreCardinality, StateInjection, StateRecord,
    StateValue, StepFormat, TaskFrame, UserIntent,
};

// Tokenizer
pub use tokenizer::{TokenSequence, Tokenizer};

// Bridge (Polars → StateInjection)
pub use bridge::{
    EvaluationMetrics, MetricsBuilder, MetricsSource, PolarsMetrics, RankedItem, StatSummary,
};

// Validation (Output validation against contracts)
pub use validation::{ValidationFailure, ValidationResult, validate_output};

// Engine (Real inference with llama-burn)
#[cfg(any(feature = "llama3", feature = "tiny"))]
pub use engine::ModelFingerprint;
#[cfg(feature = "tiny")]
pub use engine::TinyLlamaEngine;
#[cfg(feature = "llama3")]
pub use engine::{
    AdapterLifecycleState, AdapterState, GoldenTestResult, LlamaEngine, MergeReport, golden_test,
};
#[cfg(feature = "gemma")]
pub use gemma::{GemmaConfig, GemmaEngine};

// Anthropic (Remote Claude backend)
#[cfg(feature = "anthropic")]
pub use anthropic::AnthropicBackend;

// Trace (Decision chain observability)
pub use trace::{
    AdapterMetadata, DecisionChain, DecisionStep, DecisionTrace, DecisionTraceBuilder, StateSummary,
};

// Chain (Multi-step agent execution)
pub use chain::{ChainConfig, ChainEngine, ChainExecutor, StepResult, StepSignals};

// Adversarial (Input state stress testing)
pub use adversarial::{
    AdversarialHarness, AdversarialReport, AdversarialScenario, Anomaly, AnomalyType,
    ExpectedBehavior, ScenarioCategory, ScenarioGenerator, ScenarioResult,
};

// Contract Stress (Output contract stress testing)
pub use contract_stress::{
    OutputStressCase, OutputStressReport, all_stress_cases, run_output_stress_tests,
};

// Adapter (LoRA adapter lifecycle)
pub use adapter::{
    AdapterId, AdapterLifecycleEvent, AdapterManifest, AdapterPolicy, AdapterRecord,
    AdapterRegistry, AdapterRollbackRecord, AdapterWeights,
};

// Re-export governed artifact types from converge-core (portable governance semantics)
// These types apply to any artifact that can change outcomes: adapters, contracts, corpora, etc.
pub use converge_core::governed_artifact::{
    GovernedArtifactState, InvalidStateTransition, LifecycleEvent, ReplayIntegrityViolation,
    RollbackImpact, RollbackRecord, RollbackSeverity, validate_transition,
};

// Adapter Registry (Storage backends)
pub use adapter_registry::{FilesystemRegistry, InMemoryRegistry};

// LoRA (Low-Rank Adaptation)
pub use lora::{
    LoraBuilder, LoraCheckpoint, LoraConfig, LoraLayerWeights, LoraLinear, LoraLinearOrBase,
};

// LoRA Merge (Weight merging for runtime adapter application)
pub use lora_merge::{
    DeltaCanonical, LayerMapper, MergeArtifact, MergeArtifactBuilder, MergePlan,
    MergeVerificationError, OriginalWeights,
};

// Recall (Semantic similarity search for decision chains)
pub use recall::{
    CandidateScore,
    CandidateSourceType,
    CorpusFingerprint,
    CorpusFingerprintBuilder,
    DecisionOutcome,
    DecisionRecord,
    Embedder,
    EmbedderSettings,
    EmbeddingResult,
    HashEmbedder,
    MockRecallProvider,
    NormalizedRecall,
    RawRecallResult,
    RecallBudgets,
    RecallCandidate,
    RecallConfig,
    RecallConsumer,
    RecallContext,
    RecallHint,
    RecallNormalizer,
    RecallPerStep,
    RecallPolicy,
    RecallProvenanceEnvelope,
    RecallProvider,
    RecallQuery,
    RecallResponse,
    RecallTraceLink,
    RecallTrigger,
    // Recall Use/Consumer types (Recall ≠ Training boundary)
    RecallUse,
    StopReason,
    canonicalize_for_embedding,
    contains_pii,
    count_pii_patterns,
    embedding_input_hash,
    recall_use_allowed,
    redact_pii,
};

// Observability (Metrics and tracing for inference operations)
pub use observability::{
    // Metrics snapshots
    AdapterMetricsSnapshot,
    // Helper types
    AdapterOp,
    BackendMetricsSnapshot,
    FinishReason as MetricsFinishReason,
    // Metrics recording
    InMemoryMetrics,
    InferenceMetricsSnapshot,
    MetricsRecorder,
    MetricsSnapshot,
    NoOpMetrics,
    RecallMetricsSnapshot,
    Timer,
    // Global metrics
    global_metrics,
    init_global_metrics,
};

// Semantic embedder (requires semantic-embedding feature)
#[cfg(feature = "semantic-embedding")]
pub use recall::SemanticEmbedder;

// Trace - RecallMetadata is now exported from kernel module

// ============================================================================
// Public API: Sync chat adapter utilities
// ============================================================================

// Re-export core LLM types from provider module (which re-exports from converge_core::traits)
// Using Core* prefix to avoid conflicts with kernel-level types (LlmError, FinishReason)
pub use provider::{
    // Core types (re-exported from converge_core::traits)
    ChatMessage as CoreChatMessage,
    ChatRequest as CoreChatRequest,
    ChatResponse as CoreChatResponse,
    ChatRole as CoreChatRole,
    FinishReason as CoreFinishReason,
    LlmAgentConfig,
    LlmError as CoreLlmError,
    LlmRole,
    // Router types
    LlmRouter,
    // Local mock provider (extends core with confidence field)
    MockProvider,
    MockResponse,
    ModelConfig,
    MultiLineParser,
    // Suggestor types
    ProviderAgent,
    // Extended error type
    ProviderError,
    // Provider-specific aliases
    ProviderFinishReason,
    ResponseParser,
    SimpleParser,
    TokenUsage as CoreTokenUsage,
};

// ============================================================================
// Public API: Prompt DSL (migrated from converge-core)
// ============================================================================

pub use prompt_dsl::{
    AgentPrompt as DslAgentPrompt, AgentRole as DslAgentRole, Constraint as DslConstraint,
    DslOutputContract, PromptContext as DslPromptContext, PromptFormat as DslPromptFormat,
};

// gRPC server (feature-gated)
#[cfg(feature = "server")]
pub use server::KernelServiceImpl;

// gRPC client backend (feature-gated)
#[cfg(all(feature = "grpc-client", not(feature = "server")))]
pub use grpc_backend::GrpcBackend;
