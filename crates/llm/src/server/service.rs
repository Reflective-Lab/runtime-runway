// Copyright 2024-2026 Reflective Labs

//! `KernelService` gRPC implementation.
//!
//! Wraps the converge-llm reasoning kernel in a tonic service. The engine
//! is sync (Burn tensors are `!Send`), so we use `spawn_blocking` for
//! inference calls.

use std::pin::Pin;
use std::sync::Arc;

use tokio::sync::{Mutex, mpsc};
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};

use crate::chain::ChainEngine;
use crate::error::LlmError;
use crate::inference::GenerationParams;

use super::health::HealthState;
use super::proto;
use super::streaming;

// ============================================================================
// KernelEngine — combined trait for service requirements
// ============================================================================

/// Extension trait combining `ChainEngine` with adapter operations.
///
/// `LlamaEngine` provides real implementations; mock engines get default no-ops.
pub trait KernelEngine: ChainEngine + Send + 'static {
    fn kernel_load_adapter(
        &mut self,
        _registry: &dyn crate::adapter::AdapterRegistry,
        _adapter_id: &crate::adapter::AdapterId,
    ) -> crate::error::LlmResult<()> {
        Err(LlmError::AdapterError(
            "adapter operations not supported by this engine".to_string(),
        ))
    }

    fn kernel_detach_adapter(&mut self) -> crate::error::LlmResult<()> {
        Err(LlmError::AdapterError(
            "adapter operations not supported by this engine".to_string(),
        ))
    }

    fn kernel_adapter_state(&self) -> Option<&crate::engine::AdapterState> {
        None
    }

    fn kernel_adapter_lifecycle(&self) -> crate::engine::AdapterLifecycleState {
        crate::engine::AdapterLifecycleState::Detached
    }
}

// Implement KernelEngine for LlamaEngine (all backends)
#[cfg(any(feature = "llama3", feature = "tiny"))]
impl<B: burn::tensor::backend::Backend> KernelEngine for crate::engine::LlamaEngine<B>
where
    crate::engine::LlamaEngine<B>: ChainEngine + Send + 'static,
{
    fn kernel_load_adapter(
        &mut self,
        registry: &dyn crate::adapter::AdapterRegistry,
        adapter_id: &crate::adapter::AdapterId,
    ) -> crate::error::LlmResult<()> {
        self.load_adapter(registry, adapter_id)
    }

    fn kernel_detach_adapter(&mut self) -> crate::error::LlmResult<()> {
        self.detach_adapter()
    }

    fn kernel_adapter_state(&self) -> Option<&crate::engine::AdapterState> {
        self.adapter_state()
    }

    fn kernel_adapter_lifecycle(&self) -> crate::engine::AdapterLifecycleState {
        self.adapter_lifecycle()
    }
}

// ============================================================================
// Service implementation
// ============================================================================

/// The shared engine state, wrapped for safe concurrent access.
pub type SharedEngine<E> = Arc<Mutex<E>>;

/// Shared health state.
pub type SharedHealth = Arc<Mutex<HealthState>>;

/// Shared adapter registry.
pub type SharedRegistry = Arc<dyn crate::adapter::AdapterRegistry>;

/// The `KernelService` gRPC implementation.
///
/// Generic over `E: KernelEngine` so it works with both `LlamaEngine<CudaBackend>`
/// in production and mock engines in tests.
pub struct KernelServiceImpl<E: KernelEngine> {
    engine: SharedEngine<E>,
    health: SharedHealth,
    registry: Option<SharedRegistry>,
}

impl<E: KernelEngine> KernelServiceImpl<E> {
    pub fn new(engine: SharedEngine<E>, health: SharedHealth) -> Self {
        Self {
            engine,
            health,
            registry: None,
        }
    }

    pub fn with_registry(mut self, registry: SharedRegistry) -> Self {
        self.registry = Some(registry);
        self
    }
}

type GrpcResult<T> = Result<Response<T>, Status>;
type StreamResult =
    Pin<Box<dyn tokio_stream::Stream<Item = Result<proto::GenerateChunk, Status>> + Send>>;

#[tonic::async_trait]
impl<E: KernelEngine> proto::kernel_service_server::KernelService for KernelServiceImpl<E> {
    // ========================================================================
    // RunKernel — unary RPC
    // ========================================================================

    async fn run_kernel(
        &self,
        request: Request<proto::RunKernelRequest>,
    ) -> GrpcResult<proto::RunKernelResponse> {
        let req = request.into_inner();

        let intent_proto = req
            .intent
            .ok_or_else(|| Status::invalid_argument("missing intent"))?;
        let context_proto = req
            .context
            .ok_or_else(|| Status::invalid_argument("missing context"))?;
        let policy_proto = req
            .policy
            .ok_or_else(|| Status::invalid_argument("missing policy"))?;

        let intent: crate::kernel::KernelIntent = intent_proto.into();
        let context: crate::kernel::KernelContext = context_proto.into();
        let policy: crate::kernel::KernelPolicy = policy_proto.into();

        let engine = self.engine.clone();
        let start = std::time::Instant::now();

        // Engine is sync — run in blocking thread pool
        let result = tokio::task::spawn_blocking(move || {
            let mut engine_guard = engine.blocking_lock();
            crate::kernel::run_kernel(&mut *engine_guard, &intent, &context, &policy)
        })
        .await
        .map_err(|e| Status::internal(format!("task join error: {e}")))?;

        let generation_time_ms = start.elapsed().as_millis() as u64;

        match result {
            Ok(proposals) => {
                let proto_proposals: Vec<proto::KernelProposal> =
                    proposals.into_iter().map(Into::into).collect();

                Ok(Response::new(proto::RunKernelResponse {
                    proposals: proto_proposals,
                    generation_time_ms,
                }))
            }
            Err(e) => Err(llm_error_to_status(e)),
        }
    }

    // ========================================================================
    // StreamGenerate — server-streaming RPC
    // ========================================================================

    type StreamGenerateStream = StreamResult;

    async fn stream_generate(
        &self,
        request: Request<proto::StreamGenerateRequest>,
    ) -> GrpcResult<Self::StreamGenerateStream> {
        let req = request.into_inner();

        // Build the prompt from chat history
        let system_prompt = req.system_prompt.as_deref();
        let tool_instructions = streaming::build_tool_instructions(&req.tools);

        let effective_system = match (system_prompt, &tool_instructions) {
            (Some(sys), Some(tools)) => Some(format!("{sys}\n\n{tools}")),
            (Some(sys), None) => Some(sys.to_string()),
            (None, Some(tools)) => Some(tools.clone()),
            (None, None) => None,
        };

        let prompt = streaming::format_chat_template(&req.messages, effective_system.as_deref());

        let _params: GenerationParams = req
            .params
            .map(Into::into)
            .unwrap_or_else(GenerationParams::agent);

        let tools = req.tools;
        let engine = self.engine.clone();

        let (tx, rx) = mpsc::channel(64);

        // Spawn the generation task
        tokio::task::spawn_blocking(move || {
            let mut engine_guard = engine.blocking_lock();

            let envelope = crate::inference::InferenceEnvelope::agent_reasoning("stream-v1");
            let stack = crate::prompt::PromptStackBuilder::new()
                .version(crate::prompt::PromptVersion::new("stream", 1, "llama3"))
                .intent(crate::prompt::UserIntent {
                    intent: prompt.clone(),
                    criteria: None,
                    params: std::collections::HashMap::new(),
                })
                .build();

            match engine_guard.generate(&stack, &envelope) {
                Ok(output) => {
                    let mut token_index = 0u64;

                    // Detect tool calls in the output
                    let tool_calls = if !tools.is_empty() {
                        streaming::detect_tool_calls(&output)
                    } else {
                        Vec::new()
                    };
                    let has_tool_calls = !tool_calls.is_empty();

                    // Send output as token chunks
                    // In production with real streaming, each token would arrive individually
                    for chunk_text in output.chars().collect::<Vec<_>>().chunks(10) {
                        let text: String = chunk_text.iter().collect();
                        let chunk = proto::GenerateChunk {
                            chunk: Some(proto::generate_chunk::Chunk::Token(proto::TokenChunk {
                                text,
                                token_index,
                            })),
                        };
                        token_index += 1;
                        if tx.blocking_send(Ok(chunk)).is_err() {
                            return;
                        }
                    }

                    // Send tool calls if detected
                    for tc in tool_calls {
                        let chunk = proto::GenerateChunk {
                            chunk: Some(proto::generate_chunk::Chunk::ToolCall(tc.into())),
                        };
                        if tx.blocking_send(Ok(chunk)).is_err() {
                            return;
                        }
                    }

                    // Finish chunk
                    let finish_reason = if has_tool_calls {
                        proto::FinishReason::ToolCall
                    } else {
                        proto::FinishReason::Eos
                    };

                    let _ = tx.blocking_send(Ok(proto::GenerateChunk {
                        chunk: Some(proto::generate_chunk::Chunk::Finish(proto::FinishChunk {
                            reason: finish_reason.into(),
                            input_tokens: 0,
                            output_tokens: output.len() as u64,
                            full_text: output,
                        })),
                    }));
                }
                Err(e) => {
                    let error_kind = match &e {
                        LlmError::ModelNotLoaded(_) => proto::ErrorKind::ModelNotLoaded,
                        LlmError::ContextLengthExceeded { .. } => {
                            proto::ErrorKind::ContextLengthExceeded
                        }
                        LlmError::InferenceError(_) => proto::ErrorKind::InferenceFailed,
                        LlmError::InvalidParams(_) => proto::ErrorKind::InvalidParams,
                        _ => proto::ErrorKind::Unspecified,
                    };

                    let _ = tx.blocking_send(Ok(proto::GenerateChunk {
                        chunk: Some(proto::generate_chunk::Chunk::Error(proto::ErrorChunk {
                            message: e.to_string(),
                            kind: error_kind.into(),
                        })),
                    }));
                }
            }
        });

        let stream = ReceiverStream::new(rx);
        Ok(Response::new(Box::pin(stream)))
    }

    // ========================================================================
    // Adapter Management
    // ========================================================================

    async fn load_adapter(
        &self,
        request: Request<proto::LoadAdapterRequest>,
    ) -> GrpcResult<proto::LoadAdapterResponse> {
        let req = request.into_inner();
        let adapter_id = crate::adapter::AdapterId::parse(&req.adapter_id)
            .map_err(|e| Status::invalid_argument(format!("invalid adapter_id: {e}")))?;

        let registry = self
            .registry
            .as_ref()
            .ok_or_else(|| Status::unimplemented("no adapter registry configured"))?;

        let engine = self.engine.clone();
        let registry = registry.clone();

        let result = tokio::task::spawn_blocking(move || {
            let mut engine_guard = engine.blocking_lock();
            engine_guard.kernel_load_adapter(registry.as_ref(), &adapter_id)
        })
        .await
        .map_err(|e| Status::internal(format!("task join error: {e}")))?;

        match result {
            Ok(()) => Ok(Response::new(proto::LoadAdapterResponse {
                success: true,
                error: None,
                state: proto::AdapterLifecycleState::Attached.into(),
            })),
            Err(e) => Ok(Response::new(proto::LoadAdapterResponse {
                success: false,
                error: Some(e.to_string()),
                state: proto::AdapterLifecycleState::Detached.into(),
            })),
        }
    }

    async fn detach_adapter(
        &self,
        _request: Request<proto::DetachAdapterRequest>,
    ) -> GrpcResult<proto::DetachAdapterResponse> {
        let engine = self.engine.clone();

        let result = tokio::task::spawn_blocking(move || {
            let mut engine_guard = engine.blocking_lock();
            engine_guard.kernel_detach_adapter()
        })
        .await
        .map_err(|e| Status::internal(format!("task join error: {e}")))?;

        match result {
            Ok(()) => Ok(Response::new(proto::DetachAdapterResponse {
                success: true,
                error: None,
            })),
            Err(e) => Ok(Response::new(proto::DetachAdapterResponse {
                success: false,
                error: Some(e.to_string()),
            })),
        }
    }

    async fn list_adapters(
        &self,
        _request: Request<proto::ListAdaptersRequest>,
    ) -> GrpcResult<proto::ListAdaptersResponse> {
        let engine = self.engine.clone();

        let (current, lifecycle_state) = tokio::task::spawn_blocking(move || {
            let engine_guard = engine.blocking_lock();
            let current = engine_guard
                .kernel_adapter_state()
                .map(|a| proto::AdapterInfo {
                    adapter_id: a.adapter_id.to_string(),
                    merged: a.merged,
                });
            let state = engine_guard.kernel_adapter_lifecycle();
            (current, state)
        })
        .await
        .map_err(|e| Status::internal(format!("task join error: {e}")))?;

        let proto_state = match lifecycle_state {
            crate::engine::AdapterLifecycleState::Detached => {
                proto::AdapterLifecycleState::Detached
            }
            crate::engine::AdapterLifecycleState::Loading => proto::AdapterLifecycleState::Loading,
            crate::engine::AdapterLifecycleState::Attached => {
                proto::AdapterLifecycleState::Attached
            }
            crate::engine::AdapterLifecycleState::Detaching => {
                proto::AdapterLifecycleState::Detaching
            }
        };

        Ok(Response::new(proto::ListAdaptersResponse {
            current,
            lifecycle_state: proto_state.into(),
        }))
    }

    // ========================================================================
    // Health Check
    // ========================================================================

    async fn get_health(
        &self,
        _request: Request<proto::GetHealthRequest>,
    ) -> GrpcResult<proto::GetHealthResponse> {
        let health = self.health.lock().await;
        Ok(Response::new(health.to_health_response()))
    }
}

/// Convert `LlmError` to gRPC `Status`.
fn llm_error_to_status(e: LlmError) -> Status {
    match e {
        LlmError::ModelNotLoaded(msg) => Status::unavailable(msg),
        LlmError::ContextLengthExceeded { got, max } => {
            Status::invalid_argument(format!("context length exceeded: got {got}, max {max}"))
        }
        LlmError::InvalidParams(msg) => Status::invalid_argument(msg),
        LlmError::InferenceError(msg) => Status::internal(msg),
        LlmError::TokenizationError(msg) => Status::internal(msg),
        LlmError::ConfigError(msg) => Status::invalid_argument(msg),
        LlmError::AdapterError(msg) => Status::internal(msg),
        LlmError::AdapterNotFound(msg) => Status::not_found(msg),
        LlmError::AdapterIncompatible { reason } => Status::failed_precondition(reason),
        LlmError::AdapterPolicyViolation(msg) => Status::permission_denied(msg),
        LlmError::AdapterLoadError(msg) => Status::internal(msg),
        LlmError::PolicyViolation { reason } => Status::permission_denied(reason),
        LlmError::PolicyOverrideAttempted { field } => {
            Status::permission_denied(format!("policy override: {field}"))
        }
        LlmError::WeightLoadError(msg) => Status::internal(msg),
        LlmError::Io(e) => Status::internal(e.to_string()),
        LlmError::Serialization(e) => Status::internal(e.to_string()),
    }
}
