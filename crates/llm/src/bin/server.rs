// Copyright 2024-2026 Reflective Labs

//! converge-llm-server — standalone gRPC server for GPU inference.
//!
//! Loads a Llama model on startup and exposes it as a `KernelService`.
//! Designed for deployment on GCE GPU VMs with CUDA.
//!
//! # Environment Variables
//!
//! - `MODEL_PATH`: Path to model checkpoint directory (required unless using `pretrained` feature)
//! - `TOKENIZER_PATH`: Path to tokenizer file (defaults to `MODEL_PATH/tokenizer.model`)
//! - `BIND_ADDR`: Address to bind gRPC server (default: `0.0.0.0:50051`)
//! - `MAX_SEQ_LEN`: Maximum sequence length (default: `4096`)
//! - `MODEL_VARIANT`: Model variant: `llama3-8b`, `llama3-3b` (default: `llama3-8b`)
//! - `RUST_LOG`: Logging level (default: `info`)
//! - `TLS_CERT_PATH`: Path to PEM server certificate (enables TLS when set with `TLS_KEY_PATH`)
//! - `TLS_KEY_PATH`: Path to PEM server private key (enables TLS when set with `TLS_CERT_PATH`)
//! - `TLS_CLIENT_CA_PATH`: Path to PEM CA for client cert verification (enables mTLS, requires TLS)

use std::sync::Arc;

use tokio::sync::Mutex;
use tonic::transport::{Certificate, Identity, Server, ServerTlsConfig};
use tracing::{info, warn};

use converge_llm::server::health::HealthState;
use converge_llm::server::proto::kernel_service_server::KernelServiceServer;
use converge_llm::server::service::KernelServiceImpl;

#[cfg(feature = "cuda")]
type ServerBackend = burn::backend::CudaJit<burn::tensor::f16, i32>;

#[cfg(all(not(feature = "cuda"), feature = "wgpu"))]
type ServerBackend = burn::backend::Wgpu;

#[cfg(all(not(feature = "cuda"), not(feature = "wgpu")))]
type ServerBackend = burn::backend::ndarray::NdArray;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let bind_addr = std::env::var("BIND_ADDR").unwrap_or_else(|_| "0.0.0.0:50051".to_string());
    let max_seq_len: usize = std::env::var("MAX_SEQ_LEN")
        .unwrap_or_else(|_| "4096".to_string())
        .parse()
        .expect("MAX_SEQ_LEN must be a valid integer");
    let model_variant = std::env::var("MODEL_VARIANT").unwrap_or_else(|_| "llama3-8b".to_string());

    info!("converge-llm-server starting");
    info!("  bind_addr: {bind_addr}");
    info!("  max_seq_len: {max_seq_len}");
    info!("  model_variant: {model_variant}");

    // Health state — available immediately for load balancer probes
    let health = Arc::new(Mutex::new(HealthState::new(
        backend_name(),
        &model_variant,
        max_seq_len,
    )));

    // Load the model
    info!("Loading model (this may take 30-90 seconds for 8B)...");

    let engine = load_engine(max_seq_len)?;
    let shared_engine = Arc::new(Mutex::new(engine));

    // Mark health as ready
    {
        let mut h = health.lock().await;
        h.mark_ready();
    }
    info!("Model loaded and ready");

    // Build the service
    let service = KernelServiceImpl::new(shared_engine, health);

    let addr = bind_addr.parse()?;

    // TLS configuration: if both TLS_CERT_PATH and TLS_KEY_PATH are set,
    // serve over TLS. If neither is set, fall back to plaintext HTTP/2.
    // If only one is set, fail loudly — this is always a misconfiguration.
    let tls_cert = std::env::var("TLS_CERT_PATH")
        .ok()
        .filter(|v| !v.is_empty());
    let tls_key = std::env::var("TLS_KEY_PATH").ok().filter(|v| !v.is_empty());

    match (&tls_cert, &tls_key) {
        (Some(cert_path), Some(key_path)) => {
            let cert_pem = std::fs::read(cert_path)
                .map_err(|e| format!("failed to read TLS_CERT_PATH ({cert_path}): {e}"))?;
            let key_pem = std::fs::read(key_path)
                .map_err(|e| format!("failed to read TLS_KEY_PATH ({key_path}): {e}"))?;

            let identity = Identity::from_pem(cert_pem, key_pem);
            let mut tls_config = ServerTlsConfig::new().identity(identity);

            // mTLS: if TLS_CLIENT_CA_PATH is set, require client certificates
            let client_ca = std::env::var("TLS_CLIENT_CA_PATH")
                .ok()
                .filter(|v| !v.is_empty());
            if let Some(ca_path) = &client_ca {
                let ca_pem = std::fs::read(ca_path)
                    .map_err(|e| format!("failed to read TLS_CLIENT_CA_PATH ({ca_path}): {e}"))?;
                tls_config = tls_config.client_ca_root(Certificate::from_pem(ca_pem));
                info!("Serving on {addr} with mTLS");
                info!("  cert:      {cert_path}");
                info!("  key:       {key_path}");
                info!("  client_ca: {ca_path}");
            } else {
                info!("Serving on {addr} with TLS");
                info!("  cert: {cert_path}");
                info!("  key:  {key_path}");
            }

            Server::builder()
                .tls_config(tls_config)?
                .add_service(KernelServiceServer::new(service))
                .serve(addr)
                .await?;
        }
        (None, None) => {
            warn!("TLS_CERT_PATH and TLS_KEY_PATH not set — serving PLAINTEXT on {addr}");
            warn!("  Set both to enable TLS. See scripts/generate-dev-certs.sh");

            Server::builder()
                .add_service(KernelServiceServer::new(service))
                .serve(addr)
                .await?;
        }
        (Some(_), None) => {
            return Err(
                "TLS_CERT_PATH is set but TLS_KEY_PATH is missing. Set both or neither.".into(),
            );
        }
        (None, Some(_)) => {
            return Err(
                "TLS_KEY_PATH is set but TLS_CERT_PATH is missing. Set both or neither.".into(),
            );
        }
    }

    Ok(())
}

/// Detect the backend name based on compiled features.
fn backend_name() -> &'static str {
    #[cfg(feature = "cuda")]
    {
        return "cuda";
    }
    #[cfg(feature = "wgpu")]
    {
        return "wgpu";
    }
    #[allow(unreachable_code)]
    "ndarray"
}

// ============================================================================
// Engine loading — always uses load_from_checkpoint (MODEL_PATH required)
//
// For dev/testing with pretrained models, enable the `pretrained` feature
// and omit MODEL_PATH. Production deployments should always use checkpoints.
// ============================================================================

fn load_engine(
    max_seq_len: usize,
) -> Result<converge_llm::LlamaEngine<ServerBackend>, Box<dyn std::error::Error>> {
    let device = burn::tensor::Device::<ServerBackend>::default();

    let model_path = std::env::var("MODEL_PATH")
        .expect("MODEL_PATH environment variable is required (path to model checkpoint)");
    let tokenizer_path =
        std::env::var("TOKENIZER_PATH").unwrap_or_else(|_| format!("{model_path}/tokenizer.model"));

    info!("  model_path: {model_path}");
    info!("  tokenizer_path: {tokenizer_path}");

    let engine = converge_llm::LlamaEngine::load_from_checkpoint(
        &model_path,
        &tokenizer_path,
        max_seq_len,
        &device,
    )
    .map_err(|e| format!("failed to load model: {e}"))?;

    Ok(engine)
}
