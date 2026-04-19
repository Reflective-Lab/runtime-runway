// Copyright 2024-2026 Reflective Labs

//! Health check implementation with GPU status reporting.

use std::time::Instant;

use super::proto;

/// Tracks server health state.
pub struct HealthState {
    /// When the server started
    pub started_at: Instant,
    /// Whether the model is loaded and ready
    pub model_ready: bool,
    /// Backend name (e.g., "cuda", "wgpu", "ndarray")
    pub backend_name: String,
    /// Model family (e.g., "llama3")
    pub model_family: String,
    /// Maximum sequence length
    pub max_seq_len: usize,
}

impl HealthState {
    pub fn new(backend_name: &str, model_family: &str, max_seq_len: usize) -> Self {
        Self {
            started_at: Instant::now(),
            model_ready: false,
            backend_name: backend_name.to_string(),
            model_family: model_family.to_string(),
            max_seq_len,
        }
    }

    pub fn mark_ready(&mut self) {
        self.model_ready = true;
    }

    pub fn to_health_response(&self) -> proto::GetHealthResponse {
        let uptime = self.started_at.elapsed().as_secs();

        let model_status = if self.model_ready {
            proto::ModelStatus::Ready
        } else {
            proto::ModelStatus::Loading
        };

        proto::GetHealthResponse {
            ready: self.model_ready,
            model_status: model_status.into(),
            gpu_info: query_gpu_info(),
            uptime_seconds: uptime,
            backend_name: self.backend_name.clone(),
            model_family: self.model_family.clone(),
            max_seq_len: self.max_seq_len as u64,
        }
    }
}

/// Query GPU info. Returns None if GPU info is unavailable.
///
/// On CUDA builds, this could use NVML to get real GPU stats.
/// For now, returns a placeholder that indicates the backend type.
fn query_gpu_info() -> Option<proto::GpuInfo> {
    // In a real deployment, this would use nvidia-ml-sys or nvml-wrapper
    // to query actual GPU memory and utilization.
    #[cfg(feature = "cuda")]
    {
        Some(proto::GpuInfo {
            device_name: "CUDA device".to_string(),
            total_memory_mb: 0,
            used_memory_mb: 0,
            utilization_percent: 0.0,
        })
    }

    #[cfg(not(feature = "cuda"))]
    {
        None
    }
}
