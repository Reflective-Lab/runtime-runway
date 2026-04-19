// Copyright 2024-2026 Reflective Labs

//! Observability module for metrics and structured tracing.
//!
//! This module provides:
//! - **Metrics**: Token counts, latency, cache hits, error rates
//! - **Tracing integration**: Structured spans for key operations
//! - **Pluggable recording**: In-memory, prometheus, or custom backends
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │  Operation (inference, recall, adapter load)                │
//! │      │                                                      │
//! │      ▼                                                      │
//! │  record_*() functions                                       │
//! │      │                                                      │
//! │      ▼                                                      │
//! │  MetricsRecorder trait                                      │
//! │      │                                                      │
//! │      ├─► InMemoryMetrics (default, for testing)             │
//! │      ├─► PrometheusMetrics (future)                         │
//! │      └─► NoOpMetrics (zero overhead when disabled)          │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Usage
//!
//! ```
//! use converge_llm::observability::{InMemoryMetrics, MetricsRecorder, FinishReason};
//!
//! let metrics = InMemoryMetrics::new();
//!
//! // Record inference metrics
//! metrics.record_inference(100, 50, 150, FinishReason::Eos);
//!
//! // Get snapshot
//! let snapshot = metrics.snapshot();
//! assert_eq!(snapshot.inference.total_calls, 1);
//! assert_eq!(snapshot.inference.total_input_tokens, 100);
//! ```

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

// ============================================================================
// Metrics Types
// ============================================================================

/// Snapshot of inference metrics at a point in time.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InferenceMetricsSnapshot {
    /// Total inference calls made
    pub total_calls: u64,
    /// Total input tokens processed
    pub total_input_tokens: u64,
    /// Total output tokens generated
    pub total_output_tokens: u64,
    /// Total latency in milliseconds
    pub total_latency_ms: u64,
    /// Number of calls that hit max tokens limit
    pub max_tokens_hits: u64,
    /// Number of calls that hit EOS naturally
    pub eos_hits: u64,
    /// Number of calls stopped by stop sequence
    pub stop_sequence_hits: u64,
}

impl InferenceMetricsSnapshot {
    /// Calculate average input tokens per call.
    #[must_use]
    pub fn avg_input_tokens(&self) -> f64 {
        if self.total_calls == 0 {
            0.0
        } else {
            self.total_input_tokens as f64 / self.total_calls as f64
        }
    }

    /// Calculate average output tokens per call.
    #[must_use]
    pub fn avg_output_tokens(&self) -> f64 {
        if self.total_calls == 0 {
            0.0
        } else {
            self.total_output_tokens as f64 / self.total_calls as f64
        }
    }

    /// Calculate average latency in milliseconds.
    #[must_use]
    pub fn avg_latency_ms(&self) -> f64 {
        if self.total_calls == 0 {
            0.0
        } else {
            self.total_latency_ms as f64 / self.total_calls as f64
        }
    }

    /// Calculate tokens per second throughput.
    #[must_use]
    pub fn tokens_per_second(&self) -> f64 {
        if self.total_latency_ms == 0 {
            0.0
        } else {
            (self.total_output_tokens as f64 * 1000.0) / self.total_latency_ms as f64
        }
    }
}

/// Snapshot of recall/retrieval metrics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RecallMetricsSnapshot {
    /// Total recall queries made
    pub total_queries: u64,
    /// Total candidates returned
    pub total_candidates: u64,
    /// Total candidates that passed threshold
    pub candidates_above_threshold: u64,
    /// Total latency for recall operations in milliseconds
    pub total_latency_ms: u64,
    /// Number of queries that returned zero results
    pub empty_results: u64,
    /// Cache hits (if caching enabled)
    pub cache_hits: u64,
    /// Cache misses
    pub cache_misses: u64,
}

impl RecallMetricsSnapshot {
    /// Calculate average candidates per query.
    #[must_use]
    pub fn avg_candidates(&self) -> f64 {
        if self.total_queries == 0 {
            0.0
        } else {
            self.total_candidates as f64 / self.total_queries as f64
        }
    }

    /// Calculate cache hit rate (0.0 - 1.0).
    #[must_use]
    pub fn cache_hit_rate(&self) -> f64 {
        let total = self.cache_hits + self.cache_misses;
        if total == 0 {
            0.0
        } else {
            self.cache_hits as f64 / total as f64
        }
    }

    /// Calculate average latency in milliseconds.
    #[must_use]
    pub fn avg_latency_ms(&self) -> f64 {
        if self.total_queries == 0 {
            0.0
        } else {
            self.total_latency_ms as f64 / self.total_queries as f64
        }
    }
}

/// Snapshot of backend usage metrics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BackendMetricsSnapshot {
    /// Total backend calls
    pub total_calls: u64,
    /// Successful calls
    pub successful_calls: u64,
    /// Failed calls
    pub failed_calls: u64,
    /// Retried calls
    pub retried_calls: u64,
    /// Total retries (may be > retried_calls if multiple retries per call)
    pub total_retries: u64,
    /// Timeouts
    pub timeouts: u64,
    /// Circuit breaker trips
    pub circuit_breaker_trips: u64,
    /// Total cost in microdollars
    pub total_cost_microdollars: u64,
}

impl BackendMetricsSnapshot {
    /// Calculate success rate (0.0 - 1.0).
    #[must_use]
    pub fn success_rate(&self) -> f64 {
        if self.total_calls == 0 {
            1.0
        } else {
            self.successful_calls as f64 / self.total_calls as f64
        }
    }

    /// Calculate error rate (0.0 - 1.0).
    #[must_use]
    pub fn error_rate(&self) -> f64 {
        if self.total_calls == 0 {
            0.0
        } else {
            self.failed_calls as f64 / self.total_calls as f64
        }
    }

    /// Calculate retry rate (retries per call).
    #[must_use]
    pub fn retry_rate(&self) -> f64 {
        if self.total_calls == 0 {
            0.0
        } else {
            self.total_retries as f64 / self.total_calls as f64
        }
    }
}

/// Snapshot of adapter lifecycle metrics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AdapterMetricsSnapshot {
    /// Total adapter load operations
    pub total_loads: u64,
    /// Successful loads
    pub successful_loads: u64,
    /// Failed loads
    pub failed_loads: u64,
    /// Total unload/detach operations
    pub total_unloads: u64,
    /// Rollbacks performed
    pub rollbacks: u64,
    /// Total time spent loading adapters in milliseconds
    pub total_load_time_ms: u64,
}

/// Complete metrics snapshot.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MetricsSnapshot {
    /// Inference metrics
    pub inference: InferenceMetricsSnapshot,
    /// Recall metrics
    pub recall: RecallMetricsSnapshot,
    /// Backend metrics
    pub backend: BackendMetricsSnapshot,
    /// Adapter metrics
    pub adapter: AdapterMetricsSnapshot,
    /// Timestamp when snapshot was taken (Unix millis)
    pub timestamp_ms: u64,
}

// ============================================================================
// Metrics Recorder Trait
// ============================================================================

/// Trait for recording metrics.
///
/// Implementations should be thread-safe and low-overhead.
pub trait MetricsRecorder: Send + Sync {
    /// Record an inference operation.
    fn record_inference(
        &self,
        input_tokens: usize,
        output_tokens: usize,
        latency_ms: u64,
        finish_reason: FinishReason,
    );

    /// Record a recall operation.
    fn record_recall(
        &self,
        candidates: usize,
        above_threshold: usize,
        latency_ms: u64,
        cache_hit: bool,
    );

    /// Record a backend call.
    fn record_backend_call(&self, success: bool, retries: usize, cost_microdollars: Option<u64>);

    /// Record a timeout.
    fn record_timeout(&self);

    /// Record a circuit breaker trip.
    fn record_circuit_breaker_trip(&self);

    /// Record an adapter operation.
    fn record_adapter_op(&self, op: AdapterOp, success: bool, latency_ms: u64);

    /// Get a snapshot of current metrics.
    fn snapshot(&self) -> MetricsSnapshot;

    /// Reset all metrics to zero.
    fn reset(&self);
}

/// Re-export inference finish reason for metrics use.
pub use crate::inference::FinishReason;

/// Adapter operation type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdapterOp {
    /// Load adapter
    Load,
    /// Unload/detach adapter
    Unload,
    /// Rollback adapter
    Rollback,
}

// ============================================================================
// In-Memory Metrics Implementation
// ============================================================================

/// Thread-safe in-memory metrics recorder.
///
/// Uses atomic operations for low-overhead recording.
/// Suitable for testing and development.
#[derive(Debug, Default)]
pub struct InMemoryMetrics {
    // Inference metrics
    inference_calls: AtomicU64,
    inference_input_tokens: AtomicU64,
    inference_output_tokens: AtomicU64,
    inference_latency_ms: AtomicU64,
    inference_max_tokens_hits: AtomicU64,
    inference_eos_hits: AtomicU64,
    inference_stop_sequence_hits: AtomicU64,

    // Recall metrics
    recall_queries: AtomicU64,
    recall_candidates: AtomicU64,
    recall_above_threshold: AtomicU64,
    recall_latency_ms: AtomicU64,
    recall_empty_results: AtomicU64,
    recall_cache_hits: AtomicU64,
    recall_cache_misses: AtomicU64,

    // Backend metrics
    backend_calls: AtomicU64,
    backend_successful: AtomicU64,
    backend_failed: AtomicU64,
    backend_retried: AtomicU64,
    backend_retries: AtomicU64,
    backend_timeouts: AtomicU64,
    backend_circuit_trips: AtomicU64,
    backend_cost: AtomicU64,

    // Adapter metrics
    adapter_loads: AtomicU64,
    adapter_successful_loads: AtomicU64,
    adapter_failed_loads: AtomicU64,
    adapter_unloads: AtomicU64,
    adapter_rollbacks: AtomicU64,
    adapter_load_time_ms: AtomicU64,
}

impl InMemoryMetrics {
    /// Create a new in-memory metrics recorder.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

impl MetricsRecorder for InMemoryMetrics {
    fn record_inference(
        &self,
        input_tokens: usize,
        output_tokens: usize,
        latency_ms: u64,
        finish_reason: FinishReason,
    ) {
        self.inference_calls.fetch_add(1, Ordering::Relaxed);
        self.inference_input_tokens
            .fetch_add(input_tokens as u64, Ordering::Relaxed);
        self.inference_output_tokens
            .fetch_add(output_tokens as u64, Ordering::Relaxed);
        self.inference_latency_ms
            .fetch_add(latency_ms, Ordering::Relaxed);

        match finish_reason {
            FinishReason::Length => {
                self.inference_max_tokens_hits
                    .fetch_add(1, Ordering::Relaxed);
            }
            FinishReason::Eos => {
                self.inference_eos_hits.fetch_add(1, Ordering::Relaxed);
            }
            FinishReason::StopSequence => {
                self.inference_stop_sequence_hits
                    .fetch_add(1, Ordering::Relaxed);
            }
            FinishReason::Other => {}
        }
    }

    fn record_recall(
        &self,
        candidates: usize,
        above_threshold: usize,
        latency_ms: u64,
        cache_hit: bool,
    ) {
        self.recall_queries.fetch_add(1, Ordering::Relaxed);
        self.recall_candidates
            .fetch_add(candidates as u64, Ordering::Relaxed);
        self.recall_above_threshold
            .fetch_add(above_threshold as u64, Ordering::Relaxed);
        self.recall_latency_ms
            .fetch_add(latency_ms, Ordering::Relaxed);

        if candidates == 0 {
            self.recall_empty_results.fetch_add(1, Ordering::Relaxed);
        }

        if cache_hit {
            self.recall_cache_hits.fetch_add(1, Ordering::Relaxed);
        } else {
            self.recall_cache_misses.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn record_backend_call(&self, success: bool, retries: usize, cost_microdollars: Option<u64>) {
        self.backend_calls.fetch_add(1, Ordering::Relaxed);

        if success {
            self.backend_successful.fetch_add(1, Ordering::Relaxed);
        } else {
            self.backend_failed.fetch_add(1, Ordering::Relaxed);
        }

        if retries > 0 {
            self.backend_retried.fetch_add(1, Ordering::Relaxed);
            self.backend_retries
                .fetch_add(retries as u64, Ordering::Relaxed);
        }

        if let Some(cost) = cost_microdollars {
            self.backend_cost.fetch_add(cost, Ordering::Relaxed);
        }
    }

    fn record_timeout(&self) {
        self.backend_timeouts.fetch_add(1, Ordering::Relaxed);
    }

    fn record_circuit_breaker_trip(&self) {
        self.backend_circuit_trips.fetch_add(1, Ordering::Relaxed);
    }

    fn record_adapter_op(&self, op: AdapterOp, success: bool, latency_ms: u64) {
        match op {
            AdapterOp::Load => {
                self.adapter_loads.fetch_add(1, Ordering::Relaxed);
                self.adapter_load_time_ms
                    .fetch_add(latency_ms, Ordering::Relaxed);
                if success {
                    self.adapter_successful_loads
                        .fetch_add(1, Ordering::Relaxed);
                } else {
                    self.adapter_failed_loads.fetch_add(1, Ordering::Relaxed);
                }
            }
            AdapterOp::Unload => {
                self.adapter_unloads.fetch_add(1, Ordering::Relaxed);
            }
            AdapterOp::Rollback => {
                self.adapter_rollbacks.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    fn snapshot(&self) -> MetricsSnapshot {
        MetricsSnapshot {
            inference: InferenceMetricsSnapshot {
                total_calls: self.inference_calls.load(Ordering::Relaxed),
                total_input_tokens: self.inference_input_tokens.load(Ordering::Relaxed),
                total_output_tokens: self.inference_output_tokens.load(Ordering::Relaxed),
                total_latency_ms: self.inference_latency_ms.load(Ordering::Relaxed),
                max_tokens_hits: self.inference_max_tokens_hits.load(Ordering::Relaxed),
                eos_hits: self.inference_eos_hits.load(Ordering::Relaxed),
                stop_sequence_hits: self.inference_stop_sequence_hits.load(Ordering::Relaxed),
            },
            recall: RecallMetricsSnapshot {
                total_queries: self.recall_queries.load(Ordering::Relaxed),
                total_candidates: self.recall_candidates.load(Ordering::Relaxed),
                candidates_above_threshold: self.recall_above_threshold.load(Ordering::Relaxed),
                total_latency_ms: self.recall_latency_ms.load(Ordering::Relaxed),
                empty_results: self.recall_empty_results.load(Ordering::Relaxed),
                cache_hits: self.recall_cache_hits.load(Ordering::Relaxed),
                cache_misses: self.recall_cache_misses.load(Ordering::Relaxed),
            },
            backend: BackendMetricsSnapshot {
                total_calls: self.backend_calls.load(Ordering::Relaxed),
                successful_calls: self.backend_successful.load(Ordering::Relaxed),
                failed_calls: self.backend_failed.load(Ordering::Relaxed),
                retried_calls: self.backend_retried.load(Ordering::Relaxed),
                total_retries: self.backend_retries.load(Ordering::Relaxed),
                timeouts: self.backend_timeouts.load(Ordering::Relaxed),
                circuit_breaker_trips: self.backend_circuit_trips.load(Ordering::Relaxed),
                total_cost_microdollars: self.backend_cost.load(Ordering::Relaxed),
            },
            adapter: AdapterMetricsSnapshot {
                total_loads: self.adapter_loads.load(Ordering::Relaxed),
                successful_loads: self.adapter_successful_loads.load(Ordering::Relaxed),
                failed_loads: self.adapter_failed_loads.load(Ordering::Relaxed),
                total_unloads: self.adapter_unloads.load(Ordering::Relaxed),
                rollbacks: self.adapter_rollbacks.load(Ordering::Relaxed),
                total_load_time_ms: self.adapter_load_time_ms.load(Ordering::Relaxed),
            },
            timestamp_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0),
        }
    }

    fn reset(&self) {
        self.inference_calls.store(0, Ordering::Relaxed);
        self.inference_input_tokens.store(0, Ordering::Relaxed);
        self.inference_output_tokens.store(0, Ordering::Relaxed);
        self.inference_latency_ms.store(0, Ordering::Relaxed);
        self.inference_max_tokens_hits.store(0, Ordering::Relaxed);
        self.inference_eos_hits.store(0, Ordering::Relaxed);
        self.inference_stop_sequence_hits
            .store(0, Ordering::Relaxed);

        self.recall_queries.store(0, Ordering::Relaxed);
        self.recall_candidates.store(0, Ordering::Relaxed);
        self.recall_above_threshold.store(0, Ordering::Relaxed);
        self.recall_latency_ms.store(0, Ordering::Relaxed);
        self.recall_empty_results.store(0, Ordering::Relaxed);
        self.recall_cache_hits.store(0, Ordering::Relaxed);
        self.recall_cache_misses.store(0, Ordering::Relaxed);

        self.backend_calls.store(0, Ordering::Relaxed);
        self.backend_successful.store(0, Ordering::Relaxed);
        self.backend_failed.store(0, Ordering::Relaxed);
        self.backend_retried.store(0, Ordering::Relaxed);
        self.backend_retries.store(0, Ordering::Relaxed);
        self.backend_timeouts.store(0, Ordering::Relaxed);
        self.backend_circuit_trips.store(0, Ordering::Relaxed);
        self.backend_cost.store(0, Ordering::Relaxed);

        self.adapter_loads.store(0, Ordering::Relaxed);
        self.adapter_successful_loads.store(0, Ordering::Relaxed);
        self.adapter_failed_loads.store(0, Ordering::Relaxed);
        self.adapter_unloads.store(0, Ordering::Relaxed);
        self.adapter_rollbacks.store(0, Ordering::Relaxed);
        self.adapter_load_time_ms.store(0, Ordering::Relaxed);
    }
}

// ============================================================================
// No-Op Metrics (Zero Overhead)
// ============================================================================

/// No-op metrics recorder for when observability is disabled.
///
/// All methods are no-ops, providing zero runtime overhead.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoOpMetrics;

impl NoOpMetrics {
    /// Create a new no-op metrics recorder.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl MetricsRecorder for NoOpMetrics {
    fn record_inference(&self, _: usize, _: usize, _: u64, _: FinishReason) {}
    fn record_recall(&self, _: usize, _: usize, _: u64, _: bool) {}
    fn record_backend_call(&self, _: bool, _: usize, _: Option<u64>) {}
    fn record_timeout(&self) {}
    fn record_circuit_breaker_trip(&self) {}
    fn record_adapter_op(&self, _: AdapterOp, _: bool, _: u64) {}

    fn snapshot(&self) -> MetricsSnapshot {
        MetricsSnapshot::default()
    }

    fn reset(&self) {}
}

// ============================================================================
// Timing Helper
// ============================================================================

/// Timer for measuring operation latency.
///
/// # Example
///
/// ```
/// use converge_llm::observability::Timer;
///
/// let timer = Timer::start();
/// // ... do work ...
/// let elapsed_ms = timer.elapsed_ms();
/// ```
#[derive(Debug)]
pub struct Timer {
    start: Instant,
}

impl Timer {
    /// Start a new timer.
    #[must_use]
    pub fn start() -> Self {
        Self {
            start: Instant::now(),
        }
    }

    /// Get elapsed time in milliseconds.
    #[must_use]
    pub fn elapsed_ms(&self) -> u64 {
        self.start.elapsed().as_millis() as u64
    }

    /// Get elapsed time in microseconds.
    #[must_use]
    pub fn elapsed_us(&self) -> u64 {
        self.start.elapsed().as_micros() as u64
    }
}

impl Default for Timer {
    fn default() -> Self {
        Self::start()
    }
}

// ============================================================================
// Global Metrics (Optional)
// ============================================================================

use std::sync::OnceLock;

static GLOBAL_METRICS: OnceLock<Arc<dyn MetricsRecorder>> = OnceLock::new();

/// Initialize global metrics with a specific recorder.
///
/// Can only be called once. Subsequent calls are no-ops.
pub fn init_global_metrics(recorder: Arc<dyn MetricsRecorder>) {
    let _ = GLOBAL_METRICS.set(recorder);
}

/// Get the global metrics recorder.
///
/// Returns a reference to `NoOpMetrics` if not initialized.
pub fn global_metrics() -> &'static dyn MetricsRecorder {
    static NOOP: NoOpMetrics = NoOpMetrics;
    GLOBAL_METRICS
        .get()
        .map(|arc| arc.as_ref())
        .unwrap_or(&NOOP)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_in_memory_metrics_inference() {
        let metrics = InMemoryMetrics::new();

        metrics.record_inference(100, 50, 1000, FinishReason::Eos);
        metrics.record_inference(200, 100, 2000, FinishReason::Length);

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.inference.total_calls, 2);
        assert_eq!(snapshot.inference.total_input_tokens, 300);
        assert_eq!(snapshot.inference.total_output_tokens, 150);
        assert_eq!(snapshot.inference.total_latency_ms, 3000);
        assert_eq!(snapshot.inference.eos_hits, 1);
        assert_eq!(snapshot.inference.max_tokens_hits, 1);
    }

    #[test]
    fn test_in_memory_metrics_recall() {
        let metrics = InMemoryMetrics::new();

        metrics.record_recall(10, 5, 100, true);
        metrics.record_recall(0, 0, 50, false);

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.recall.total_queries, 2);
        assert_eq!(snapshot.recall.total_candidates, 10);
        assert_eq!(snapshot.recall.candidates_above_threshold, 5);
        assert_eq!(snapshot.recall.cache_hits, 1);
        assert_eq!(snapshot.recall.cache_misses, 1);
        assert_eq!(snapshot.recall.empty_results, 1);
    }

    #[test]
    fn test_in_memory_metrics_backend() {
        let metrics = InMemoryMetrics::new();

        metrics.record_backend_call(true, 0, Some(100));
        metrics.record_backend_call(true, 2, Some(200));
        metrics.record_backend_call(false, 3, None);
        metrics.record_timeout();
        metrics.record_circuit_breaker_trip();

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.backend.total_calls, 3);
        assert_eq!(snapshot.backend.successful_calls, 2);
        assert_eq!(snapshot.backend.failed_calls, 1);
        assert_eq!(snapshot.backend.retried_calls, 2);
        assert_eq!(snapshot.backend.total_retries, 5);
        assert_eq!(snapshot.backend.timeouts, 1);
        assert_eq!(snapshot.backend.circuit_breaker_trips, 1);
        assert_eq!(snapshot.backend.total_cost_microdollars, 300);
    }

    #[test]
    fn test_in_memory_metrics_adapter() {
        let metrics = InMemoryMetrics::new();

        metrics.record_adapter_op(AdapterOp::Load, true, 500);
        metrics.record_adapter_op(AdapterOp::Load, false, 100);
        metrics.record_adapter_op(AdapterOp::Unload, true, 50);
        metrics.record_adapter_op(AdapterOp::Rollback, true, 200);

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.adapter.total_loads, 2);
        assert_eq!(snapshot.adapter.successful_loads, 1);
        assert_eq!(snapshot.adapter.failed_loads, 1);
        assert_eq!(snapshot.adapter.total_unloads, 1);
        assert_eq!(snapshot.adapter.rollbacks, 1);
        assert_eq!(snapshot.adapter.total_load_time_ms, 600);
    }

    #[test]
    fn test_metrics_reset() {
        let metrics = InMemoryMetrics::new();

        metrics.record_inference(100, 50, 1000, FinishReason::Eos);
        metrics.record_recall(10, 5, 100, true);
        metrics.record_backend_call(true, 0, Some(100));

        metrics.reset();

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.inference.total_calls, 0);
        assert_eq!(snapshot.recall.total_queries, 0);
        assert_eq!(snapshot.backend.total_calls, 0);
    }

    #[test]
    fn test_inference_metrics_calculations() {
        let snapshot = InferenceMetricsSnapshot {
            total_calls: 10,
            total_input_tokens: 1000,
            total_output_tokens: 500,
            total_latency_ms: 5000,
            ..Default::default()
        };

        assert!((snapshot.avg_input_tokens() - 100.0).abs() < 0.01);
        assert!((snapshot.avg_output_tokens() - 50.0).abs() < 0.01);
        assert!((snapshot.avg_latency_ms() - 500.0).abs() < 0.01);
        assert!((snapshot.tokens_per_second() - 100.0).abs() < 0.01);
    }

    #[test]
    fn test_recall_metrics_calculations() {
        let snapshot = RecallMetricsSnapshot {
            total_queries: 100,
            total_candidates: 500,
            cache_hits: 80,
            cache_misses: 20,
            total_latency_ms: 1000,
            ..Default::default()
        };

        assert!((snapshot.avg_candidates() - 5.0).abs() < 0.01);
        assert!((snapshot.cache_hit_rate() - 0.8).abs() < 0.01);
        assert!((snapshot.avg_latency_ms() - 10.0).abs() < 0.01);
    }

    #[test]
    fn test_backend_metrics_calculations() {
        let snapshot = BackendMetricsSnapshot {
            total_calls: 100,
            successful_calls: 90,
            failed_calls: 10,
            total_retries: 20,
            ..Default::default()
        };

        assert!((snapshot.success_rate() - 0.9).abs() < 0.01);
        assert!((snapshot.error_rate() - 0.1).abs() < 0.01);
        assert!((snapshot.retry_rate() - 0.2).abs() < 0.01);
    }

    #[test]
    fn test_timer() {
        let timer = Timer::start();
        std::thread::sleep(std::time::Duration::from_millis(10));
        let elapsed = timer.elapsed_ms();

        // Allow some variance in timing
        assert!(elapsed >= 10, "Timer should measure at least 10ms");
        assert!(elapsed < 100, "Timer should not measure more than 100ms");
    }

    #[test]
    fn test_noop_metrics() {
        let metrics = NoOpMetrics::new();

        // All operations should be no-ops
        metrics.record_inference(100, 50, 1000, FinishReason::Eos);
        metrics.record_recall(10, 5, 100, true);
        metrics.record_backend_call(true, 0, Some(100));

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.inference.total_calls, 0);
        assert_eq!(snapshot.recall.total_queries, 0);
        assert_eq!(snapshot.backend.total_calls, 0);
    }

    #[test]
    fn test_metrics_thread_safety() {
        use std::sync::Arc;
        use std::thread;

        let metrics = Arc::new(InMemoryMetrics::new());
        let mut handles = vec![];

        for _ in 0..10 {
            let m = Arc::clone(&metrics);
            handles.push(thread::spawn(move || {
                for _ in 0..100 {
                    m.record_inference(10, 5, 10, FinishReason::Eos);
                }
            }));
        }

        for handle in handles {
            handle.join().unwrap();
        }

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.inference.total_calls, 1000);
        assert_eq!(snapshot.inference.total_input_tokens, 10000);
    }
}
