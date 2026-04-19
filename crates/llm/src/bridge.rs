// Copyright 2024-2026 Reflective Labs

//! Bridge between Polars DataFrames and LLM StateInjection.
//!
//! This module converts Polars-derived metrics and features into
//! structured state for LLM prompts. It enables the key pattern:
//!
//! ```text
//! Polars DataFrame → compute metrics → StateInjection → PromptStack → LLM
//! ```
//!
//! # Why This Matters
//!
//! Instead of narrative prompting:
//! ```text
//! "The data shows that sales increased by 15% with high confidence..."
//! ```
//!
//! You inject structured state:
//! ```text
//! STATE:
//!   metric_value: 0.15
//!   confidence: 0.92
//!   trend: "increasing"
//! ```
//!
//! This dramatically reduces hallucinations and variance.

use crate::prompt::{StateInjection, StateRecord, StateValue};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Metrics extracted from a Polars computation.
///
/// This is the bridge type between converge-analytics and converge-llm.
/// It captures common metric patterns that can be injected into prompts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolarsMetrics {
    /// Scalar metrics (single values)
    pub scalars: HashMap<String, f64>,

    /// Categorical values
    pub categories: HashMap<String, String>,

    /// Boolean flags
    pub flags: HashMap<String, bool>,

    /// Ranked lists (e.g., top features, sorted candidates)
    pub rankings: HashMap<String, Vec<RankedItem>>,

    /// Statistical summaries
    pub summaries: Vec<StatSummary>,

    /// Source information for traceability
    pub source: Option<MetricsSource>,
}

impl Default for PolarsMetrics {
    fn default() -> Self {
        Self::new()
    }
}

impl PolarsMetrics {
    /// Create empty metrics.
    #[must_use]
    pub fn new() -> Self {
        Self {
            scalars: HashMap::new(),
            categories: HashMap::new(),
            flags: HashMap::new(),
            rankings: HashMap::new(),
            summaries: vec![],
            source: None,
        }
    }

    /// Add a scalar metric.
    #[must_use]
    pub fn with_scalar(mut self, name: impl Into<String>, value: f64) -> Self {
        self.scalars.insert(name.into(), value);
        self
    }

    /// Add a categorical value.
    #[must_use]
    pub fn with_category(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.categories.insert(name.into(), value.into());
        self
    }

    /// Add a boolean flag.
    #[must_use]
    pub fn with_flag(mut self, name: impl Into<String>, value: bool) -> Self {
        self.flags.insert(name.into(), value);
        self
    }

    /// Add a ranked list.
    #[must_use]
    pub fn with_ranking(mut self, name: impl Into<String>, items: Vec<RankedItem>) -> Self {
        self.rankings.insert(name.into(), items);
        self
    }

    /// Add a statistical summary.
    #[must_use]
    pub fn with_summary(mut self, summary: StatSummary) -> Self {
        self.summaries.push(summary);
        self
    }

    /// Set the source information.
    #[must_use]
    pub fn with_source(mut self, source: MetricsSource) -> Self {
        self.source = Some(source);
        self
    }

    /// Convert to StateInjection for prompt building.
    #[must_use]
    pub fn to_state_injection(&self) -> StateInjection {
        let mut state = StateInjection::new();

        // Add scalars
        for (name, value) in &self.scalars {
            state = state.with_scalar(name.clone(), *value);
        }

        // Add categories as strings
        for (name, value) in &self.categories {
            state = state.with_scalar(name.clone(), StateValue::String(value.clone()));
        }

        // Add flags as booleans
        for (name, value) in &self.flags {
            state = state.with_scalar(name.clone(), *value);
        }

        // Add rankings as lists
        for (name, items) in &self.rankings {
            let values: Vec<StateValue> = items
                .iter()
                .map(|item| StateValue::String(format!("{}:{:.2}", item.label, item.score)))
                .collect();
            state = state.with_list(name.clone(), values);
        }

        // Add summaries as records
        for summary in &self.summaries {
            let record = StateRecord::new(&summary.column)
                .with_field("mean", summary.mean)
                .with_field("std", summary.std)
                .with_field("min", summary.min)
                .with_field("max", summary.max)
                .with_field("count", summary.count as i64);
            state = state.with_record(record);
        }

        state
    }
}

/// A ranked item with label and score.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RankedItem {
    /// Item label/identifier
    pub label: String,
    /// Score (higher = better, typically)
    pub score: f64,
    /// Optional rank position
    pub rank: Option<usize>,
}

impl RankedItem {
    /// Create a new ranked item.
    #[must_use]
    pub fn new(label: impl Into<String>, score: f64) -> Self {
        Self {
            label: label.into(),
            score,
            rank: None,
        }
    }

    /// Create with explicit rank.
    #[must_use]
    pub fn with_rank(mut self, rank: usize) -> Self {
        self.rank = Some(rank);
        self
    }
}

/// Statistical summary of a column.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatSummary {
    /// Column name
    pub column: String,
    /// Mean value
    pub mean: f64,
    /// Standard deviation
    pub std: f64,
    /// Minimum value
    pub min: f64,
    /// Maximum value
    pub max: f64,
    /// Count of values
    pub count: usize,
}

impl StatSummary {
    /// Create a new statistical summary.
    #[must_use]
    pub fn new(column: impl Into<String>) -> Self {
        Self {
            column: column.into(),
            mean: 0.0,
            std: 0.0,
            min: 0.0,
            max: 0.0,
            count: 0,
        }
    }

    /// Set all statistics.
    #[must_use]
    pub fn with_stats(mut self, mean: f64, std: f64, min: f64, max: f64, count: usize) -> Self {
        self.mean = mean;
        self.std = std;
        self.min = min;
        self.max = max;
        self.count = count;
        self
    }
}

/// Source information for metrics traceability.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsSource {
    /// Dataset or table name
    pub dataset: String,
    /// Number of rows processed
    pub row_count: usize,
    /// Timestamp of computation
    pub computed_at: String,
    /// Version/iteration
    pub version: Option<String>,
}

impl MetricsSource {
    /// Create a new source reference.
    #[must_use]
    pub fn new(dataset: impl Into<String>, row_count: usize) -> Self {
        Self {
            dataset: dataset.into(),
            row_count,
            computed_at: "2026-01-16T00:00:00Z".to_string(),
            version: None,
        }
    }
}

/// Common metric patterns from ML evaluation.
///
/// These map directly to converge-analytics EvaluationReport patterns.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvaluationMetrics {
    /// Mean Absolute Error
    pub mae: f64,
    /// Success ratio (1 - normalized error)
    pub success_ratio: f64,
    /// Number of validation samples
    pub val_rows: usize,
    /// Iteration number
    pub iteration: usize,
    /// Model path/identifier
    pub model_id: String,
}

impl EvaluationMetrics {
    /// Convert to PolarsMetrics.
    #[must_use]
    pub fn to_polars_metrics(&self) -> PolarsMetrics {
        PolarsMetrics::new()
            .with_scalar("mae", self.mae)
            .with_scalar("success_ratio", self.success_ratio)
            .with_scalar("val_rows", self.val_rows as f64)
            .with_scalar("iteration", self.iteration as f64)
            .with_category("model_id", &self.model_id)
            .with_flag("meets_threshold", self.success_ratio >= 0.75)
    }

    /// Convert directly to StateInjection.
    #[must_use]
    pub fn to_state_injection(&self) -> StateInjection {
        self.to_polars_metrics().to_state_injection()
    }
}

/// Builder for creating metrics from converge-analytics patterns.
#[derive(Debug, Default)]
pub struct MetricsBuilder {
    metrics: PolarsMetrics,
}

impl MetricsBuilder {
    /// Create a new builder.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add evaluation metrics (MAE, success_ratio pattern).
    #[must_use]
    pub fn evaluation(mut self, mae: f64, success_ratio: f64) -> Self {
        self.metrics.scalars.insert("mae".to_string(), mae);
        self.metrics
            .scalars
            .insert("success_ratio".to_string(), success_ratio);
        self.metrics
            .flags
            .insert("meets_threshold".to_string(), success_ratio >= 0.75);
        self
    }

    /// Add feature importance ranking.
    #[must_use]
    pub fn feature_importance(mut self, features: Vec<(String, f64)>) -> Self {
        let ranked: Vec<RankedItem> = features
            .into_iter()
            .enumerate()
            .map(|(i, (label, score))| RankedItem::new(label, score).with_rank(i + 1))
            .collect();
        self.metrics
            .rankings
            .insert("feature_importance".to_string(), ranked);
        self
    }

    /// Add data quality flags.
    #[must_use]
    pub fn data_quality(
        mut self,
        has_nulls: bool,
        has_outliers: bool,
        drift_detected: bool,
    ) -> Self {
        self.metrics
            .flags
            .insert("has_nulls".to_string(), has_nulls);
        self.metrics
            .flags
            .insert("has_outliers".to_string(), has_outliers);
        self.metrics
            .flags
            .insert("drift_detected".to_string(), drift_detected);
        self
    }

    /// Add deployment decision context.
    #[must_use]
    pub fn deployment_context(mut self, action: &str, should_retrain: bool) -> Self {
        self.metrics
            .categories
            .insert("deployment_action".to_string(), action.to_string());
        self.metrics
            .flags
            .insert("should_retrain".to_string(), should_retrain);
        self
    }

    /// Build the metrics.
    #[must_use]
    pub fn build(self) -> PolarsMetrics {
        self.metrics
    }

    /// Build directly to StateInjection.
    #[must_use]
    pub fn build_state(self) -> StateInjection {
        self.metrics.to_state_injection()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_polars_metrics_to_state_injection() {
        let metrics = PolarsMetrics::new()
            .with_scalar("mae", 0.15)
            .with_scalar("success_ratio", 0.85)
            .with_flag("meets_threshold", true)
            .with_category("status", "healthy");

        let state = metrics.to_state_injection();

        // State should contain all metrics
        assert!(!state.scalars.is_empty());
    }

    #[test]
    fn test_ranked_items() {
        let metrics = PolarsMetrics::new().with_ranking(
            "top_features",
            vec![
                RankedItem::new("feature_a", 0.95).with_rank(1),
                RankedItem::new("feature_b", 0.82).with_rank(2),
            ],
        );

        let state = metrics.to_state_injection();
        assert!(state.lists.contains_key("top_features"));
    }

    #[test]
    fn test_stat_summary() {
        let summary = StatSummary::new("price").with_stats(100.0, 25.0, 10.0, 500.0, 1000);

        let metrics = PolarsMetrics::new().with_summary(summary);
        let state = metrics.to_state_injection();

        assert_eq!(state.records.len(), 1);
        assert_eq!(state.records[0].name, "price");
    }

    #[test]
    fn test_evaluation_metrics() {
        let eval = EvaluationMetrics {
            mae: 0.12,
            success_ratio: 0.88,
            val_rows: 500,
            iteration: 3,
            model_id: "model_v3".to_string(),
        };

        let state = eval.to_state_injection();
        assert!(!state.scalars.is_empty());
    }

    #[test]
    fn test_metrics_builder() {
        let state = MetricsBuilder::new()
            .evaluation(0.15, 0.85)
            .feature_importance(vec![
                ("feature_a".to_string(), 0.9),
                ("feature_b".to_string(), 0.7),
            ])
            .data_quality(false, true, false)
            .deployment_context("deploy", false)
            .build_state();

        assert!(state.scalars.contains_key(&"mae".to_string()));
        assert!(state.scalars.contains_key(&"success_ratio".to_string()));
        assert!(state.lists.contains_key(&"feature_importance".to_string()));
    }

    #[test]
    fn test_integration_with_prompt_stack() {
        use crate::prompt::{PromptStackBuilder, UserIntent};

        let metrics = MetricsBuilder::new().evaluation(0.12, 0.88).build_state();

        let stack = PromptStackBuilder::new()
            .state(metrics)
            .intent(UserIntent::new("interpret_metrics").with_criteria("deployment_readiness"))
            .build();

        let rendered = stack.render();
        assert!(rendered.contains("mae"));
        assert!(rendered.contains("success_ratio"));
        assert!(rendered.contains("interpret_metrics"));
    }
}
