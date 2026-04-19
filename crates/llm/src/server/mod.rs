// Copyright 2024-2026 Reflective Labs

//! gRPC server for the converge-llm reasoning kernel.
//!
//! Exposes the kernel as a `KernelService` with:
//! - `RunKernel` — unary RPC for the 3-step reasoning chain
//! - `StreamGenerate` — server-streaming for coding agent mode
//! - Adapter management (LoRA lifecycle)
//! - Health checks with GPU status

pub mod convert;
pub mod health;
pub mod service;
pub mod streaming;

/// Generated protobuf types from `proto/kernel.proto`.
pub mod proto {
    #![allow(clippy::all, clippy::pedantic)]
    include!("generated/converge.llm.v1.rs");
}

pub use service::KernelServiceImpl;
