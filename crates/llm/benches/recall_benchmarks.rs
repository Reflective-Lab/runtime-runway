// Copyright 2024-2026 Reflective Labs

//! Benchmarks for recall subsystem.
//!
//! Measures p50/p95 latencies for:
//! - Embedding generation (HashEmbedder)
//! - Query execution (PersistentRecallProvider)
//! - End-to-end recall augmentation
//!
//! Run with: `cargo bench --features persistent-recall`
//!
//! Budget targets (from RecallBudgets::default()):
//! - max_latency_ms: 100ms
//! - max_embedding_calls: 3 per chain
//! - max_tokens_per_candidate: 100

use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use std::time::Duration;

// ============================================================================
// Benchmark: Embedding Latency
// ============================================================================

fn bench_embedding_latency(c: &mut Criterion) {
    use converge_llm::recall::Embedder;
    use converge_llm::recall::HashEmbedder;

    let mut group = c.benchmark_group("embedding");
    group.measurement_time(Duration::from_secs(5));

    // Test various embedding dimensions
    for dim in [128, 256, 384, 768] {
        let embedder = HashEmbedder::new(dim);

        group.throughput(Throughput::Elements(1));
        group.bench_with_input(BenchmarkId::new("hash_embedder", dim), &dim, |b, _| {
            b.iter(|| {
                let result = embedder
                    .embed(black_box("deployment failure memory limit exceeded"))
                    .unwrap();
                black_box(result)
            });
        });
    }

    // Benchmark with varying input lengths
    let embedder = HashEmbedder::new(384);
    for len in [10, 50, 100, 500, 1000] {
        let input: String = "word ".repeat(len);
        group.bench_with_input(
            BenchmarkId::new("hash_embedder_input_len", len),
            &input,
            |b, input| {
                b.iter(|| {
                    let result = embedder.embed(black_box(input)).unwrap();
                    black_box(result)
                });
            },
        );
    }

    group.finish();
}

// ============================================================================
// Benchmark: Query Latency (PersistentRecallProvider)
// ============================================================================

#[cfg(feature = "persistent-recall")]
fn bench_query_latency(c: &mut Criterion) {
    use converge_llm::recall::{
        DecisionOutcome, DecisionRecord, HashEmbedder, PersistentRecallProvider, RecallFilter,
        RecallProvider, RecallQuery,
    };
    use converge_llm::trace::DecisionStep;
    use tempfile::TempDir;

    let mut group = c.benchmark_group("query");
    group.measurement_time(Duration::from_secs(10));

    // Test with varying corpus sizes
    for corpus_size in [10, 100, 500, 1000] {
        let temp_dir = TempDir::new().unwrap();
        let embedder = HashEmbedder::new(384);
        let provider = PersistentRecallProvider::create(temp_dir.path(), embedder, "v1").unwrap();

        // Populate corpus
        for i in 0..corpus_size {
            let record = DecisionRecord {
                id: format!("rec-{i:05}"),
                step: DecisionStep::Reasoning,
                outcome: if i % 3 == 0 {
                    DecisionOutcome::Failure
                } else {
                    DecisionOutcome::Success
                },
                contract_type: "Reasoning".to_string(),
                input_summary: format!("Input for record {i}"),
                output_summary: format!(
                    "Deployment {} due to resource constraints",
                    if i % 3 == 0 { "failed" } else { "succeeded" }
                ),
                chain_id: format!("chain-{:03}", i % 100),
                created_at: "2026-01-18T12:00:00Z".to_string(),
                tenant_scope: None,
            };
            provider.add_record(&record).unwrap();
        }

        // Benchmark query without filter
        group.throughput(Throughput::Elements(1));
        group.bench_with_input(
            BenchmarkId::new("no_filter", corpus_size),
            &corpus_size,
            |b, _| {
                let query = RecallQuery::new("deployment failure resource", 5);
                b.iter(|| {
                    let response = provider.recall(black_box(&query)).unwrap();
                    black_box(response)
                });
            },
        );

        // Benchmark query with filter
        group.bench_with_input(
            BenchmarkId::new("with_filter", corpus_size),
            &corpus_size,
            |b, _| {
                let query = RecallQuery::new("deployment failure resource", 5);
                let filter = RecallFilter::new().with_outcome(DecisionOutcome::Failure);
                b.iter(|| {
                    let response = provider
                        .recall_with_filter(black_box(&query), black_box(&filter))
                        .unwrap();
                    black_box(response)
                });
            },
        );
    }

    group.finish();
}

#[cfg(not(feature = "persistent-recall"))]
fn bench_query_latency(_c: &mut Criterion) {
    // No-op when persistent-recall feature is disabled
}

// ============================================================================
// Benchmark: End-to-End Recall Augmentation
// ============================================================================

#[cfg(feature = "persistent-recall")]
fn bench_e2e_recall(c: &mut Criterion) {
    use converge_llm::recall::{
        DecisionOutcome, DecisionRecord, HashEmbedder, PersistentRecallProvider, RecallContext,
        RecallFilter, RecallHint, RecallQuery,
    };
    use converge_llm::trace::DecisionStep;
    use tempfile::TempDir;

    let mut group = c.benchmark_group("e2e_recall");
    group.measurement_time(Duration::from_secs(10));

    // Setup: corpus with 500 records
    let temp_dir = TempDir::new().unwrap();
    let embedder = HashEmbedder::new(384);
    let provider = PersistentRecallProvider::create(temp_dir.path(), embedder, "v1").unwrap();

    for i in 0..500 {
        let record = DecisionRecord {
            id: format!("rec-{i:05}"),
            step: DecisionStep::Reasoning,
            outcome: if i % 3 == 0 {
                DecisionOutcome::Failure
            } else {
                DecisionOutcome::Success
            },
            contract_type: "Reasoning".to_string(),
            input_summary: format!("Input {i}"),
            output_summary: format!("Output {i} with deployment context"),
            chain_id: format!("chain-{:03}", i % 50),
            created_at: "2026-01-18T12:00:00Z".to_string(),
            tenant_scope: None,
        };
        provider.add_record(&record).unwrap();
    }

    // Benchmark: full recall → context building pipeline
    group.throughput(Throughput::Elements(1));
    group.bench_function("full_pipeline", |b| {
        b.iter(|| {
            // Query for failures
            let query = RecallQuery::new("deployment failure", 5);
            let filter = RecallFilter::new().with_outcome(DecisionOutcome::Failure);
            let response = provider.recall_with_filter(&query, &filter).unwrap();

            // Build RecallContext from candidates
            let mut context = RecallContext::empty();
            for candidate in &response.candidates {
                let hint = RecallHint::from_candidate(candidate);
                match candidate.source_type {
                    converge_llm::recall::CandidateSourceType::SimilarFailure => {
                        context.similar_failures.push(hint);
                    }
                    converge_llm::recall::CandidateSourceType::SimilarSuccess => {
                        context.similar_successes.push(hint);
                    }
                    _ => {}
                }
            }
            context.trace_link = Some(response.trace_link.clone());

            black_box(context)
        });
    });

    // Benchmark: query only (for comparison)
    group.bench_function("query_only", |b| {
        b.iter(|| {
            let query = RecallQuery::new("deployment failure", 5);
            let filter = RecallFilter::new().with_outcome(DecisionOutcome::Failure);
            let response = provider
                .recall_with_filter(black_box(&query), black_box(&filter))
                .unwrap();
            black_box(response)
        });
    });

    group.finish();
}

#[cfg(not(feature = "persistent-recall"))]
fn bench_e2e_recall(_c: &mut Criterion) {
    // No-op when persistent-recall feature is disabled
}

// ============================================================================
// Benchmark: Budget Compliance Under Load
// ============================================================================

#[cfg(feature = "persistent-recall")]
fn bench_budget_compliance(c: &mut Criterion) {
    use converge_llm::recall::{
        DecisionOutcome, DecisionRecord, HashEmbedder, PersistentRecallProvider, RecallBudgets,
        RecallFilter, RecallQuery,
    };
    use converge_llm::trace::DecisionStep;
    use tempfile::TempDir;

    let mut group = c.benchmark_group("budget_compliance");
    group.measurement_time(Duration::from_secs(15));

    // Get default budget constraints
    let budgets = RecallBudgets::default();
    let max_latency_ms = budgets.max_latency_ms; // 100ms

    // Setup: larger corpus to stress test
    let temp_dir = TempDir::new().unwrap();
    let embedder = HashEmbedder::new(384);
    let provider = PersistentRecallProvider::create(temp_dir.path(), embedder, "v1").unwrap();

    // Populate with 2000 records
    for i in 0..2000 {
        let record = DecisionRecord {
            id: format!("rec-{i:05}"),
            step: match i % 3 {
                0 => DecisionStep::Reasoning,
                1 => DecisionStep::Evaluation,
                _ => DecisionStep::Planning,
            },
            outcome: if i % 4 == 0 {
                DecisionOutcome::Failure
            } else if i % 4 == 1 {
                DecisionOutcome::Partial
            } else {
                DecisionOutcome::Success
            },
            contract_type: match i % 3 {
                0 => "Reasoning",
                1 => "Evaluation",
                _ => "Planning",
            }
            .to_string(),
            input_summary: format!("Complex input scenario {i} with multiple conditions"),
            output_summary: format!(
                "Detailed output {i} describing deployment state and resource allocation"
            ),
            chain_id: format!("chain-{:03}", i % 200),
            created_at: format!("2026-01-{:02}T12:00:00Z", (i % 28) + 1),
            tenant_scope: Some(format!("tenant-{}", i % 10)),
        };
        provider.add_record(&record).unwrap();
    }

    // Benchmark: verify queries complete within budget
    group.bench_function("within_budget_100ms", |b| {
        b.iter(|| {
            let start = std::time::Instant::now();

            let query = RecallQuery::new("deployment failure resource allocation", 10);
            let filter = RecallFilter::new()
                .with_outcome(DecisionOutcome::Failure)
                .with_tenant_scope("tenant-5");
            let response = provider.recall_with_filter(&query, &filter).unwrap();

            let elapsed = start.elapsed();

            // Assert budget compliance (will show in benchmark results)
            assert!(
                elapsed.as_millis() <= max_latency_ms as u128,
                "Query exceeded budget: {}ms > {}ms",
                elapsed.as_millis(),
                max_latency_ms
            );

            black_box(response)
        });
    });

    // Benchmark: heavy filter combination
    group.bench_function("complex_filter", |b| {
        b.iter(|| {
            let query = RecallQuery::new("deployment", 5)
                .with_step_context(DecisionStep::Reasoning)
                .with_tenant_scope("tenant-3");
            let filter = RecallFilter::new()
                .with_outcome(DecisionOutcome::Failure)
                .with_contract_type("Reasoning")
                .with_time_window("2026-01-01", "2026-01-15");

            let response = provider
                .recall_with_filter(black_box(&query), black_box(&filter))
                .unwrap();
            black_box(response)
        });
    });

    group.finish();
}

#[cfg(not(feature = "persistent-recall"))]
fn bench_budget_compliance(_c: &mut Criterion) {
    // No-op when persistent-recall feature is disabled
}

// ============================================================================
// Criterion Main
// ============================================================================

criterion_group!(
    benches,
    bench_embedding_latency,
    bench_query_latency,
    bench_e2e_recall,
    bench_budget_compliance,
);

criterion_main!(benches);
