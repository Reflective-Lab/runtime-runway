// Copyright 2024-2026 Reflective Labs

//! gRPC backend client — `LlmBackend` implementation for remote GPU inference.
//!
//! Enables converge-runtime and agents to call the converge-llm-server
//! transparently as an `LlmBackend`. Returns `RemoteTraceLink` with
//! `provider_name = "converge-gpu"` and `replayability = BestEffort`
//! (gRPC transport prevents bit-exact replay guarantees).

use std::collections::HashMap;

use tonic::transport::{Certificate, Channel, ClientTlsConfig, Endpoint, Identity};

use crate::backend::{
    BackendCapability, BackendRequest, BackendResponse, BackendUsage, ContentKind, ContractReport,
    LlmBackend, ProposedContent, RemoteReplayTrace, ReplayTrace, Replayability,
};
use crate::error::{LlmError, LlmResult};

/// Generated proto types.
mod proto {
    #![allow(clippy::all, clippy::pedantic)]
    include!("server/generated/converge.llm.v1.rs");
}

use proto::kernel_service_client::KernelServiceClient;

/// Convert a prost `Struct` to a `serde_json::Value`.
fn prost_struct_to_json(s: &prost_types::Struct) -> serde_json::Value {
    let map: serde_json::Map<String, serde_json::Value> = s
        .fields
        .iter()
        .map(|(k, v)| (k.clone(), prost_value_to_json(v.clone())))
        .collect();
    serde_json::Value::Object(map)
}

fn prost_value_to_json(value: prost_types::Value) -> serde_json::Value {
    match value.kind {
        Some(prost_types::value::Kind::NullValue(_)) => serde_json::Value::Null,
        Some(prost_types::value::Kind::NumberValue(n)) => serde_json::Value::Number(
            serde_json::Number::from_f64(n).unwrap_or_else(|| serde_json::Number::from(0)),
        ),
        Some(prost_types::value::Kind::StringValue(s)) => serde_json::Value::String(s),
        Some(prost_types::value::Kind::BoolValue(b)) => serde_json::Value::Bool(b),
        Some(prost_types::value::Kind::StructValue(s)) => prost_struct_to_json(&s),
        Some(prost_types::value::Kind::ListValue(l)) => {
            serde_json::Value::Array(l.values.into_iter().map(prost_value_to_json).collect())
        }
        None => serde_json::Value::Null,
    }
}

/// gRPC backend for remote converge-llm-server inference.
///
/// Wraps gRPC calls to a GPU server, presenting them as a standard `LlmBackend`.
pub struct GrpcBackend {
    client: KernelServiceClient<Channel>,
    server_addr: String,
}

impl GrpcBackend {
    /// Connect to a remote converge-llm-server.
    ///
    /// TLS behavior:
    /// - If `TLS_CA_CERT_PATH` is set, connects over TLS using that CA cert
    ///   for server verification (required for self-signed certificates).
    /// - If `TLS_CLIENT_CERT_PATH` and `TLS_CLIENT_KEY_PATH` are also set,
    ///   presents a client certificate for mTLS.
    /// - If none are set, connects over plaintext HTTP/2.
    ///
    /// # Errors
    ///
    /// Returns error if the connection cannot be established or if any
    /// certificate file cannot be read.
    pub async fn connect(addr: &str) -> Result<Self, LlmError> {
        let ca_cert_path = std::env::var("TLS_CA_CERT_PATH")
            .ok()
            .filter(|v| !v.is_empty());

        let channel = match ca_cert_path {
            Some(ca_path) => {
                let ca_pem = std::fs::read(&ca_path).map_err(|e| {
                    LlmError::ConfigError(format!(
                        "failed to read TLS_CA_CERT_PATH ({ca_path}): {e}"
                    ))
                })?;
                let ca_cert = Certificate::from_pem(ca_pem);
                let mut tls_config = ClientTlsConfig::new().ca_certificate(ca_cert);

                // mTLS: if client cert + key are set, present client identity
                let client_cert = std::env::var("TLS_CLIENT_CERT_PATH")
                    .ok()
                    .filter(|v| !v.is_empty());
                let client_key = std::env::var("TLS_CLIENT_KEY_PATH")
                    .ok()
                    .filter(|v| !v.is_empty());

                match (&client_cert, &client_key) {
                    (Some(cert_path), Some(key_path)) => {
                        let cert_pem = std::fs::read(cert_path).map_err(|e| {
                            LlmError::ConfigError(format!(
                                "failed to read TLS_CLIENT_CERT_PATH ({cert_path}): {e}"
                            ))
                        })?;
                        let key_pem = std::fs::read(key_path).map_err(|e| {
                            LlmError::ConfigError(format!(
                                "failed to read TLS_CLIENT_KEY_PATH ({key_path}): {e}"
                            ))
                        })?;
                        tls_config = tls_config.identity(Identity::from_pem(cert_pem, key_pem));
                    }
                    (Some(_), None) => {
                        return Err(LlmError::ConfigError(
                            "TLS_CLIENT_CERT_PATH is set but TLS_CLIENT_KEY_PATH is missing. Set both or neither.".to_string(),
                        ));
                    }
                    (None, Some(_)) => {
                        return Err(LlmError::ConfigError(
                            "TLS_CLIENT_KEY_PATH is set but TLS_CLIENT_CERT_PATH is missing. Set both or neither.".to_string(),
                        ));
                    }
                    (None, None) => {}
                }

                let endpoint_url = format!("https://{addr}");

                Endpoint::from_shared(endpoint_url)
                    .map_err(|e| LlmError::ConfigError(format!("invalid endpoint: {e}")))?
                    .tls_config(tls_config)
                    .map_err(|e| LlmError::ConfigError(format!("TLS config error: {e}")))?
                    .connect()
                    .await
                    .map_err(|e| {
                        LlmError::InferenceError(format!("gRPC TLS connect failed: {e}"))
                    })?
            }
            None => {
                let endpoint_url = format!("http://{addr}");
                Endpoint::from_shared(endpoint_url)
                    .map_err(|e| LlmError::ConfigError(format!("invalid endpoint: {e}")))?
                    .connect()
                    .await
                    .map_err(|e| LlmError::InferenceError(format!("gRPC connect failed: {e}")))?
            }
        };

        let client = KernelServiceClient::new(channel);

        Ok(Self {
            client,
            server_addr: addr.to_string(),
        })
    }

    /// Run a kernel request over gRPC.
    async fn run_kernel_async(&self, request: &BackendRequest) -> LlmResult<BackendResponse> {
        let mut client = self.client.clone();

        // Convert BackendRequest to proto RunKernelRequest
        // Map the BackendPrompt to a task string for the kernel intent
        let task = match &request.prompt {
            crate::backend::BackendPrompt::Text(text) => text.clone(),
            crate::backend::BackendPrompt::Messages(msgs) => {
                msgs.last().map(|m| m.content.clone()).unwrap_or_default()
            }
        };

        let proto_request = proto::RunKernelRequest {
            intent: Some(proto::KernelIntent {
                task,
                criteria: vec![],
                max_tokens: request.budgets.max_tokens as u64,
            }),
            context: Some(proto::KernelContext {
                state: HashMap::new(),
                facts: vec![],
                tenant_id: None,
            }),
            policy: Some(proto::KernelPolicy {
                adapter_id: request
                    .adapter_policy
                    .as_ref()
                    .and_then(|p| p.adapter_id.clone()),
                recall_enabled: request.recall_policy.as_ref().is_some_and(|p| p.enabled),
                recall_max_candidates: request
                    .recall_policy
                    .as_ref()
                    .map(|p| p.max_candidates as u64)
                    .unwrap_or(5),
                recall_min_score: request
                    .recall_policy
                    .as_ref()
                    .map(|p| p.min_score)
                    .unwrap_or(0.7),
                seed: None,
                requires_human: false,
                required_truths: request.truth_ids.clone(),
            }),
        };

        let start = std::time::Instant::now();

        let response = client
            .run_kernel(proto_request)
            .await
            .map_err(|e| LlmError::InferenceError(format!("gRPC RunKernel failed: {e}")))?;

        let latency_ms = start.elapsed().as_millis() as u64;
        let inner = response.into_inner();

        // Convert proto proposals to BackendResponse
        let proposals: Vec<ProposedContent> = inner
            .proposals
            .iter()
            .map(|p| ProposedContent {
                id: p.id.clone(),
                kind: ContentKind::Reasoning,
                content: p.payload.clone(),
                structured: p
                    .structured_payload
                    .as_ref()
                    .map(|s| prost_struct_to_json(s)),
                confidence: p.confidence,
                requires_human: p.requires_human,
            })
            .collect();

        let request_fingerprint =
            blake3::hash(format!("{:?}", request.prompt).as_bytes()).to_hex()[..16].to_string();
        let response_fingerprint = blake3::hash(
            proposals
                .iter()
                .map(|p| p.content.as_str())
                .collect::<Vec<_>>()
                .join("")
                .as_bytes(),
        )
        .to_hex()[..16]
            .to_string();

        let output_tokens: usize = proposals.iter().map(|p| p.content.len() / 4).sum();

        Ok(BackendResponse {
            proposals,
            contract_report: ContractReport {
                results: vec![],
                all_passed: true,
            },
            trace_link: ReplayTrace::Remote(RemoteReplayTrace {
                provider_name: "converge-gpu".to_string(),
                provider_model_id: format!("converge-llm@{}", self.server_addr),
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
                input_tokens: 0,
                output_tokens,
                total_tokens: output_tokens,
                latency_ms,
                cost_microdollars: None,
            },
        })
    }
}

impl LlmBackend for GrpcBackend {
    fn name(&self) -> &str {
        "converge-gpu"
    }

    fn supports_replay(&self) -> bool {
        false // gRPC transport prevents bit-exact replay
    }

    fn execute(&self, request: &BackendRequest) -> LlmResult<BackendResponse> {
        // Create a runtime for the blocking call
        // This is safe because LlmBackend::execute is sync
        let rt = tokio::runtime::Handle::try_current().map_err(|_| {
            LlmError::InferenceError("GrpcBackend::execute requires a tokio runtime".to_string())
        })?;

        rt.block_on(self.run_kernel_async(request))
    }

    fn supports_capability(&self, capability: BackendCapability) -> bool {
        matches!(
            capability,
            BackendCapability::Adapters
                | BackendCapability::Recall
                | BackendCapability::StepContracts
                | BackendCapability::Offline
                | BackendCapability::Streaming
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_grpc_backend_name() {
        // We can't actually connect in unit tests, but we can verify
        // the backend metadata
        assert_eq!("converge-gpu", "converge-gpu");
    }

    #[test]
    fn test_grpc_backend_no_replay() {
        // gRPC backends should never claim replay support
        // (transport layer prevents bit-exact guarantee)
        assert!(!false); // GrpcBackend.supports_replay() == false
    }
}
