// Copyright 2024-2026 Reflective Labs

//! Persistent recall provider with file-based storage.
//!
//! This provider persists recall data using a simple file-based format:
//! - Corpus metadata stored as JSON
//! - Records stored as JSONL (one JSON object per line)
//! - Embeddings stored alongside records
//!
//! Features:
//! - Corpus versioning with `CorpusFingerprint`
//! - Hybrid filters (time window, outcome, contract types, adapter_id)
//! - Deterministic sorting with stable tie-breakers
//! - Full replay capability via provenance tracking
//!
//! # Usage
//!
//! ```ignore
//! use converge_llm::recall::{PersistentRecallProvider, HashEmbedder, RecallFilter};
//!
//! let embedder = HashEmbedder::new(384);
//! let provider = PersistentRecallProvider::create("./data/recall", embedder, "v1")?;
//!
//! // Add records
//! provider.add_record(DecisionRecord { ... })?;
//!
//! // Query with filters
//! let query = RecallQuery::new("deployment failure", 5);
//! let filter = RecallFilter::default()
//!     .with_outcome(DecisionOutcome::Failure)
//!     .with_time_window("2026-01-01", "2026-01-18");
//! let response = provider.recall_with_filter(&query, &filter)?;
//! ```

use super::corpus::{CorpusFingerprint, TenantPolicy};
use super::embedder::{DeterminismLevel, Embedder, EmbedderSettings};
use super::normalizer::{RawRecallResult, RecallNormalizer};
use super::pii::embedding_input_hash;
use super::provider::{RecallProvider, RecallResponse};
use super::types::{
    CandidateProvenance, CandidateScore, CandidateSourceType, DecisionOutcome, DecisionRecord,
    RecallConsumer, RecallProvenanceEnvelope, RecallQuery, RecallTraceLink, RecallUse, StopReason,
};
use crate::error::{LlmError, LlmResult};

use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::RwLock;
use std::time::Instant;

// ============================================================================
// Filter Types
// ============================================================================

/// Filter for recall queries.
///
/// Supports hybrid filtering by multiple dimensions. All filters are AND-ed.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RecallFilter {
    /// Filter by decision outcome
    pub outcome: Option<DecisionOutcome>,
    /// Filter by contract type
    pub contract_type: Option<String>,
    /// Filter by adapter ID
    pub adapter_id: Option<String>,
    /// Filter by source type
    pub source_type: Option<CandidateSourceType>,
    /// Filter by time window start (ISO 8601)
    pub time_start: Option<String>,
    /// Filter by time window end (ISO 8601)
    pub time_end: Option<String>,
    /// Filter by tenant scope
    pub tenant_scope: Option<String>,
}

impl RecallFilter {
    /// Create an empty filter (no filtering).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Filter by decision outcome.
    #[must_use]
    pub fn with_outcome(mut self, outcome: DecisionOutcome) -> Self {
        self.outcome = Some(outcome);
        self
    }

    /// Filter by contract type.
    #[must_use]
    pub fn with_contract_type(mut self, contract_type: impl Into<String>) -> Self {
        self.contract_type = Some(contract_type.into());
        self
    }

    /// Filter by adapter ID.
    #[must_use]
    pub fn with_adapter_id(mut self, adapter_id: impl Into<String>) -> Self {
        self.adapter_id = Some(adapter_id.into());
        self
    }

    /// Filter by source type.
    #[must_use]
    pub fn with_source_type(mut self, source_type: CandidateSourceType) -> Self {
        self.source_type = Some(source_type);
        self
    }

    /// Filter by time window.
    #[must_use]
    pub fn with_time_window(mut self, start: impl Into<String>, end: impl Into<String>) -> Self {
        self.time_start = Some(start.into());
        self.time_end = Some(end.into());
        self
    }

    /// Filter by tenant scope.
    #[must_use]
    pub fn with_tenant_scope(mut self, tenant: impl Into<String>) -> Self {
        self.tenant_scope = Some(tenant.into());
        self
    }

    /// Check if any filters are set.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.outcome.is_none()
            && self.contract_type.is_none()
            && self.adapter_id.is_none()
            && self.source_type.is_none()
            && self.time_start.is_none()
            && self.time_end.is_none()
            && self.tenant_scope.is_none()
    }

    /// Check if a record matches the filter.
    #[must_use]
    pub fn matches(&self, record: &StoredRecord) -> bool {
        // Check outcome
        if let Some(ref outcome) = self.outcome {
            if record.outcome != Some(*outcome) {
                return false;
            }
        }

        // Check contract type
        if let Some(ref contract_type) = self.contract_type {
            if record.contract_type.as_ref() != Some(contract_type) {
                return false;
            }
        }

        // Check adapter ID
        if let Some(ref adapter_id) = self.adapter_id {
            if record.adapter_id.as_ref() != Some(adapter_id) {
                return false;
            }
        }

        // Check source type
        if let Some(ref source_type) = self.source_type {
            if record.source_type != *source_type {
                return false;
            }
        }

        // Check time window
        if let Some(ref start) = self.time_start {
            if record.created_at < *start {
                return false;
            }
        }
        if let Some(ref end) = self.time_end {
            if record.created_at > *end {
                return false;
            }
        }

        // Check tenant scope
        if let Some(ref tenant) = self.tenant_scope {
            if record.tenant_scope.as_ref() != Some(tenant) {
                return false;
            }
        }

        true
    }

    /// Compute a deterministic hash of the filter for provenance.
    #[must_use]
    pub fn filter_hash(&self) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        if let Some(ref o) = self.outcome {
            format!("{o:?}").hash(&mut hasher);
        }
        self.contract_type.hash(&mut hasher);
        self.adapter_id.hash(&mut hasher);
        if let Some(ref s) = self.source_type {
            format!("{s:?}").hash(&mut hasher);
        }
        self.time_start.hash(&mut hasher);
        self.time_end.hash(&mut hasher);
        self.tenant_scope.hash(&mut hasher);
        format!("{:016x}", hasher.finish())
    }
}

// ============================================================================
// Stored Record (persisted format)
// ============================================================================

/// A record stored in the persistent corpus.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredRecord {
    /// Unique identifier
    pub id: String,
    /// Summary text
    pub summary: String,
    /// Embedding vector
    pub embedding: Vec<f32>,
    /// Source type
    pub source_type: CandidateSourceType,
    /// Decision outcome (if applicable)
    pub outcome: Option<DecisionOutcome>,
    /// Contract type (if applicable)
    pub contract_type: Option<String>,
    /// Adapter ID (if applicable)
    pub adapter_id: Option<String>,
    /// Chain ID (if applicable)
    pub chain_id: Option<String>,
    /// Creation timestamp (ISO 8601)
    pub created_at: String,
    /// Tenant scope (if applicable)
    pub tenant_scope: Option<String>,
    /// Corpus version when indexed
    pub corpus_version: String,
    /// Decision step (if applicable)
    pub step: Option<String>,
}

impl StoredRecord {
    /// Create from a decision record and embedding.
    pub fn from_decision_record(
        record: &DecisionRecord,
        embedding: Vec<f32>,
        corpus_version: &str,
    ) -> Self {
        let source_type = match record.outcome {
            DecisionOutcome::Success => CandidateSourceType::SimilarSuccess,
            DecisionOutcome::Failure | DecisionOutcome::Partial => {
                CandidateSourceType::SimilarFailure
            }
        };

        Self {
            id: record.id.clone(),
            summary: record.output_summary.clone(),
            embedding,
            source_type,
            outcome: Some(record.outcome),
            contract_type: Some(record.contract_type.clone()),
            adapter_id: None,
            chain_id: Some(record.chain_id.clone()),
            created_at: record.created_at.clone(),
            tenant_scope: record.tenant_scope.clone(),
            corpus_version: corpus_version.to_string(),
            step: Some(record.step.as_str().to_string()),
        }
    }
}

// ============================================================================
// Corpus Metadata (persisted)
// ============================================================================

/// Persisted corpus metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorpusMetadata {
    /// Unique identifier for this corpus
    pub id: String,
    /// Schema version
    pub schema_version: String,
    /// Embedder ID
    pub embedder_id: String,
    /// Hash of embedder settings
    pub embedder_settings_hash: String,
    /// Dataset snapshot identifier
    pub dataset_snapshot: String,
    /// Content-derived hash (blake3 of sorted record IDs + count)
    #[serde(default)]
    pub content_hash: Option<String>,
    /// Optional tenant scope
    pub tenant_scope: Option<String>,
    /// Tenant policy for this corpus
    #[serde(default)]
    pub tenant_policy: TenantPolicy,
    /// When this corpus was created
    pub created_at: String,
    /// Embedding dimensions
    pub embedding_dim: usize,
    /// Number of records
    pub record_count: usize,
}

impl CorpusMetadata {
    /// Create from a corpus fingerprint, embedder, and its settings.
    pub fn from_fingerprint<E: Embedder>(
        fp: &CorpusFingerprint,
        embedder: &E,
        embedder_settings: &EmbedderSettings,
    ) -> Self {
        Self {
            id: fp.to_version_string(),
            schema_version: fp.schema_version.clone(),
            embedder_id: embedder.embedder_id().to_string(),
            embedder_settings_hash: embedder_settings.settings_hash(),
            dataset_snapshot: fp.dataset_snapshot.clone(),
            content_hash: fp.content_hash.clone(),
            tenant_scope: fp.tenant_scope.clone(),
            tenant_policy: TenantPolicy::Optional,
            created_at: timestamp_now(),
            embedding_dim: embedder.dimensions(),
            record_count: 0,
        }
    }

    /// Create with explicit tenant policy.
    pub fn from_fingerprint_with_policy<E: Embedder>(
        fp: &CorpusFingerprint,
        embedder: &E,
        embedder_settings: &EmbedderSettings,
        tenant_policy: TenantPolicy,
    ) -> Self {
        let mut meta = Self::from_fingerprint(fp, embedder, embedder_settings);
        meta.tenant_policy = tenant_policy;
        meta
    }
}

// ============================================================================
// Persistent Recall Provider
// ============================================================================

/// Persistent recall provider with file-based storage.
///
/// Stores records in JSONL format with corpus metadata in a separate JSON file.
///
/// # Determinism Policy
///
/// When `RecallUse::TrainingCandidateSelection` or kernel requires replayability:
/// - Embedder must be `DeterminismLevel::BitExact` deterministic
/// - Otherwise, results are returned with `StopReason::EmbedderNotDeterministic`
///   and marked as audit-only
///
/// # Tenant Policy
///
/// When `TenantPolicy::Required`:
/// - Queries without tenant scope return empty results
/// - Stop reason set to `StopReason::TenantScopeMissing`
pub struct PersistentRecallProvider<E: Embedder> {
    embedder: E,
    corpus_fingerprint: CorpusFingerprint,
    corpus_metadata: CorpusMetadata,
    data_dir: PathBuf,
    records: RwLock<Vec<StoredRecord>>,
    normalizer: RecallNormalizer,
    tenant_policy: TenantPolicy,
}

impl<E: Embedder> PersistentRecallProvider<E> {
    /// Create a new persistent provider, creating the data directory if needed.
    ///
    /// # Errors
    ///
    /// Returns an error if directory creation or file I/O fails.
    pub fn create(
        data_dir: impl AsRef<Path>,
        embedder: E,
        schema_version: impl Into<String>,
    ) -> LlmResult<Self> {
        Self::create_with_policy(data_dir, embedder, schema_version, TenantPolicy::Optional)
    }

    /// Create a new persistent provider with explicit tenant policy.
    ///
    /// # Errors
    ///
    /// Returns an error if directory creation or file I/O fails.
    pub fn create_with_policy(
        data_dir: impl AsRef<Path>,
        embedder: E,
        schema_version: impl Into<String>,
        tenant_policy: TenantPolicy,
    ) -> LlmResult<Self> {
        let data_dir = data_dir.as_ref().to_path_buf();
        let schema_version = schema_version.into();
        let embedder_settings = embedder.settings_snapshot();

        // Create data directory
        fs::create_dir_all(&data_dir)?;

        // Create corpus fingerprint
        let corpus_fingerprint =
            CorpusFingerprint::new(&schema_version, &embedder, "initial", None);

        // Create corpus metadata with tenant policy
        let mut corpus_metadata = CorpusMetadata::from_fingerprint_with_policy(
            &corpus_fingerprint,
            &embedder,
            &embedder_settings,
            tenant_policy,
        );
        corpus_metadata.record_count = 0;

        // Save metadata
        let metadata_path = data_dir.join("corpus_metadata.json");
        let metadata_json = serde_json::to_string_pretty(&corpus_metadata)?;
        fs::write(&metadata_path, metadata_json)?;

        // Create empty records file
        let records_path = data_dir.join("records.jsonl");
        File::create(&records_path)?;

        Ok(Self {
            embedder,
            corpus_fingerprint,
            corpus_metadata,
            data_dir,
            records: RwLock::new(Vec::new()),
            normalizer: RecallNormalizer::new().with_min_threshold(0.0),
            tenant_policy,
        })
    }

    /// Open an existing persistent provider.
    ///
    /// # Errors
    ///
    /// Returns an error if the data directory doesn't exist or files are corrupted.
    pub fn open(data_dir: impl AsRef<Path>, embedder: E) -> LlmResult<Self> {
        let data_dir = data_dir.as_ref().to_path_buf();

        // Load metadata
        let metadata_path = data_dir.join("corpus_metadata.json");
        let metadata_json = fs::read_to_string(&metadata_path)?;
        let corpus_metadata: CorpusMetadata = serde_json::from_str(&metadata_json)?;

        // Load records
        let records_path = data_dir.join("records.jsonl");
        let records = Self::load_records(&records_path)?;

        // Extract tenant policy
        let tenant_policy = corpus_metadata.tenant_policy;

        // Recreate corpus fingerprint with content hash
        let record_ids: Vec<String> = records.iter().map(|r| r.id.clone()).collect();
        let mut corpus_fingerprint = CorpusFingerprint::new(
            &corpus_metadata.schema_version,
            &embedder,
            &corpus_metadata.dataset_snapshot,
            corpus_metadata.tenant_scope.clone(),
        );
        if !record_ids.is_empty() {
            corpus_fingerprint.update_content_hash(&record_ids);
        }

        Ok(Self {
            embedder,
            corpus_fingerprint,
            corpus_metadata,
            data_dir,
            records: RwLock::new(records),
            normalizer: RecallNormalizer::new().with_min_threshold(0.0),
            tenant_policy,
        })
    }

    /// Add a decision record to the corpus.
    ///
    /// # Errors
    ///
    /// Returns an error if embedding or file I/O fails.
    pub fn add_record(&self, record: &DecisionRecord) -> LlmResult<()> {
        // Create embedding
        let embedding_text = record.to_embedding_text();
        let embedding_result = self.embedder.embed(&embedding_text)?;

        // Create stored record
        let stored_record = StoredRecord::from_decision_record(
            record,
            embedding_result.vector,
            &self.corpus_fingerprint.to_version_string(),
        );

        // Append to records file
        let records_path = self.data_dir.join("records.jsonl");
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&records_path)?;
        let json_line = serde_json::to_string(&stored_record)?;
        writeln!(file, "{json_line}")?;

        // Update in-memory records
        let mut records = self.records.write().map_err(|_| {
            LlmError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                "Lock poisoned",
            ))
        })?;
        records.push(stored_record);

        Ok(())
    }

    /// Perform a recall query with filters.
    ///
    /// # Policy Enforcement
    ///
    /// - **Tenant Policy**: If `TenantPolicy::Required` and query has no tenant scope,
    ///   returns empty results with `StopReason::TenantScopeMissing`.
    /// - **Determinism Policy**: If embedder is not `Fully` deterministic,
    ///   results include `StopReason::EmbedderNotDeterministic` to indicate
    ///   audit-only status.
    ///
    /// # Errors
    ///
    /// Returns an error if embedding fails.
    pub fn recall_with_filter(
        &self,
        query: &RecallQuery,
        filter: &RecallFilter,
    ) -> LlmResult<RecallResponse> {
        let start = Instant::now();

        // Check tenant policy enforcement
        if !self.tenant_policy.is_valid(query.tenant_scope.as_deref()) {
            let elapsed = start.elapsed().as_millis() as u64;
            let trace_link = RecallTraceLink {
                embedding_hash: String::new(),
                corpus_version: self.corpus_fingerprint.to_version_string(),
                embedder_id: self.embedder.embedder_id().to_string(),
                candidates_searched: 0,
                candidates_returned: 0,
                latency_ms: elapsed,
            };
            let embedder_settings = self.embedder.settings_snapshot();
            let provenance = RecallProvenanceEnvelope {
                query_hash: query.query_hash(),
                embedding_input_hash: embedding_input_hash(&query.query_text),
                embedding_hash: String::new(),
                embedder_id: self.embedder.embedder_id().to_string(),
                embedder_settings_hash: embedder_settings.settings_hash(),
                corpus_fingerprint: self.corpus_fingerprint.to_version_string(),
                policy_snapshot_hash: String::new(),
                purpose: RecallUse::RuntimeAugmentation,
                consumers: vec![RecallConsumer::Kernel],
                candidate_scores: vec![],
                candidates_searched: 0,
                candidates_returned: 0,
                stop_reason: Some(StopReason::TenantScopeMissing),
                latency_ms: elapsed,
                timestamp: timestamp_now(),
                signature: "unsigned".to_string(),
            };
            return Ok(RecallResponse {
                candidates: vec![],
                stop_reason: Some(StopReason::TenantScopeMissing),
                trace_link,
                provenance: Some(provenance),
            });
        }

        // Embed the query
        let query_result = self.embedder.embed(&query.query_text)?;
        let query_embedding = &query_result.vector;

        // Check embedder determinism
        let determinism_contract = self.embedder.determinism_contract();
        let is_deterministic = determinism_contract.level == DeterminismLevel::BitExact;

        // Get records (read lock)
        let records = self.records.read().map_err(|_| {
            LlmError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                "Lock poisoned",
            ))
        })?;

        // Score all matching records
        let mut scored: Vec<(f64, &StoredRecord)> = records
            .iter()
            .filter(|r| filter.matches(r))
            .map(|r| (Self::cosine_similarity(query_embedding, &r.embedding), r))
            .collect();

        let candidates_searched = scored.len();

        // Sort by score descending, then by ID ascending (deterministic tie-breaker)
        scored.sort_by(|a, b| match b.0.partial_cmp(&a.0) {
            Some(std::cmp::Ordering::Equal) | None => a.1.id.cmp(&b.1.id),
            Some(ord) => ord,
        });

        // Take top_k
        scored.truncate(query.top_k);

        // Convert to raw results
        let raw_results: Vec<RawRecallResult> = scored
            .iter()
            .map(|(score, record)| {
                RawRecallResult::new(
                    record.id.clone(),
                    record.summary.clone(),
                    *score,
                    record.source_type,
                )
                .with_provenance(CandidateProvenance {
                    created_at: record.created_at.clone(),
                    source_chain_id: record.chain_id.clone(),
                    source_step: record.step.as_ref().and_then(|s| parse_step(s)),
                    corpus_version: record.corpus_version.clone(),
                })
            })
            .collect();

        let elapsed = start.elapsed().as_millis() as u64;

        // Create trace link
        let trace_link = RecallTraceLink {
            embedding_hash: query_result.embedding_hash(),
            corpus_version: self.corpus_fingerprint.to_version_string(),
            embedder_id: self.embedder.embedder_id().to_string(),
            candidates_searched,
            candidates_returned: raw_results.len(),
            latency_ms: elapsed,
        };

        // Normalize results
        let normalized =
            self.normalizer
                .normalize_batch(raw_results, trace_link.clone(), query.top_k);

        // Determine stop reason - prioritize determinism check over normal stop reason
        let stop_reason = if !is_deterministic {
            Some(StopReason::EmbedderNotDeterministic)
        } else {
            normalized.stop_reason
        };

        // Build candidate scores for provenance
        let candidate_scores: Vec<CandidateScore> = normalized
            .candidates
            .iter()
            .map(|c| CandidateScore {
                id: c.id.clone(),
                score: c.final_score,
            })
            .collect();

        // Create full provenance envelope
        let embedder_settings = self.embedder.settings_snapshot();
        let provenance = RecallProvenanceEnvelope {
            query_hash: query.query_hash(),
            embedding_input_hash: embedding_input_hash(&query.query_text),
            embedding_hash: query_result.embedding_hash(),
            embedder_id: self.embedder.embedder_id().to_string(),
            embedder_settings_hash: embedder_settings.settings_hash(),
            corpus_fingerprint: self.corpus_fingerprint.to_version_string(),
            policy_snapshot_hash: String::new(),
            purpose: RecallUse::RuntimeAugmentation,
            consumers: vec![RecallConsumer::Kernel],
            candidate_scores,
            candidates_searched,
            candidates_returned: normalized.candidates.len(),
            stop_reason,
            latency_ms: elapsed,
            timestamp: timestamp_now(),
            signature: "unsigned".to_string(),
        };

        Ok(RecallResponse {
            candidates: normalized.candidates,
            stop_reason,
            trace_link,
            provenance: Some(provenance),
        })
    }

    /// Get the corpus metadata.
    #[must_use]
    pub fn corpus_metadata(&self) -> &CorpusMetadata {
        &self.corpus_metadata
    }

    /// Get the number of records in the corpus.
    pub fn record_count(&self) -> LlmResult<usize> {
        let records = self.records.read().map_err(|_| {
            LlmError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                "Lock poisoned",
            ))
        })?;
        Ok(records.len())
    }

    /// Compute cosine similarity between two vectors.
    fn cosine_similarity(a: &[f32], b: &[f32]) -> f64 {
        if a.len() != b.len() {
            return 0.0;
        }

        let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
        let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
        let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();

        if norm_a == 0.0 || norm_b == 0.0 {
            0.0
        } else {
            f64::from(dot / (norm_a * norm_b))
        }
    }

    /// Load records from JSONL file.
    fn load_records(path: &Path) -> LlmResult<Vec<StoredRecord>> {
        if !path.exists() {
            return Ok(Vec::new());
        }

        let file = File::open(path)?;
        let reader = BufReader::new(file);
        let mut records = Vec::new();

        for line in reader.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            let record: StoredRecord = serde_json::from_str(&line)?;
            records.push(record);
        }

        Ok(records)
    }
}

impl<E: Embedder + Send + Sync> RecallProvider for PersistentRecallProvider<E> {
    fn corpus_fingerprint(&self) -> &CorpusFingerprint {
        &self.corpus_fingerprint
    }

    fn recall(&self, query: &RecallQuery) -> LlmResult<RecallResponse> {
        self.recall_with_filter(query, &RecallFilter::default())
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Parse decision step from string.
fn parse_step(s: &str) -> Option<crate::trace::DecisionStep> {
    match s.to_lowercase().as_str() {
        "reasoning" => Some(crate::trace::DecisionStep::Reasoning),
        "evaluation" => Some(crate::trace::DecisionStep::Evaluation),
        "planning" => Some(crate::trace::DecisionStep::Planning),
        _ => None,
    }
}

/// Generate current timestamp in ISO 8601 format.
fn timestamp_now() -> String {
    // In production, use chrono
    "2026-01-18T00:00:00Z".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recall::HashEmbedder;
    use crate::trace::DecisionStep;
    use tempfile::TempDir;

    #[test]
    fn test_recall_filter_empty() {
        let filter = RecallFilter::new();
        assert!(filter.is_empty());
    }

    #[test]
    fn test_recall_filter_with_outcome() {
        let filter = RecallFilter::new().with_outcome(DecisionOutcome::Failure);
        assert!(!filter.is_empty());
    }

    #[test]
    fn test_recall_filter_hash_deterministic() {
        let filter = RecallFilter::new()
            .with_outcome(DecisionOutcome::Failure)
            .with_contract_type("Reasoning");

        let hash1 = filter.filter_hash();
        let hash2 = filter.filter_hash();
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_recall_filter_hash_different() {
        let filter1 = RecallFilter::new().with_outcome(DecisionOutcome::Failure);
        let filter2 = RecallFilter::new().with_outcome(DecisionOutcome::Success);

        assert_ne!(filter1.filter_hash(), filter2.filter_hash());
    }

    #[test]
    fn test_corpus_metadata_creation() {
        let embedder = HashEmbedder::new(128);
        let fp = CorpusFingerprint::new("v1", &embedder, "snapshot-1", None);
        let settings = embedder.settings_snapshot();

        let metadata = CorpusMetadata::from_fingerprint(&fp, &embedder, &settings);
        assert_eq!(metadata.schema_version, "v1");
        assert_eq!(metadata.dataset_snapshot, "snapshot-1");
        assert_eq!(metadata.embedding_dim, 128);
    }

    #[test]
    fn test_persistent_provider_create_and_add() {
        let temp_dir = TempDir::new().unwrap();
        let embedder = HashEmbedder::new(128);

        let provider = PersistentRecallProvider::create(temp_dir.path(), embedder, "v1").unwrap();

        let record = DecisionRecord {
            id: "rec-001".to_string(),
            step: DecisionStep::Reasoning,
            outcome: DecisionOutcome::Failure,
            contract_type: "Reasoning".to_string(),
            input_summary: "Test input".to_string(),
            output_summary: "Deployment failed due to memory limit".to_string(),
            chain_id: "chain-001".to_string(),
            created_at: "2026-01-18T12:00:00Z".to_string(),
            tenant_scope: None,
        };

        provider.add_record(&record).unwrap();
        assert_eq!(provider.record_count().unwrap(), 1);
    }

    #[test]
    fn test_persistent_provider_recall_basic() {
        let temp_dir = TempDir::new().unwrap();
        let embedder = HashEmbedder::new(128);

        let provider = PersistentRecallProvider::create(temp_dir.path(), embedder, "v1").unwrap();

        // Add some records
        let records = vec![
            DecisionRecord {
                id: "fail-001".to_string(),
                step: DecisionStep::Reasoning,
                outcome: DecisionOutcome::Failure,
                contract_type: "Reasoning".to_string(),
                input_summary: "Test".to_string(),
                output_summary: "Deployment failed due to memory limit".to_string(),
                chain_id: "chain-001".to_string(),
                created_at: "2026-01-18T12:00:00Z".to_string(),
                tenant_scope: None,
            },
            DecisionRecord {
                id: "success-001".to_string(),
                step: DecisionStep::Reasoning,
                outcome: DecisionOutcome::Success,
                contract_type: "Reasoning".to_string(),
                input_summary: "Test".to_string(),
                output_summary: "Deployment succeeded with rolling update".to_string(),
                chain_id: "chain-002".to_string(),
                created_at: "2026-01-18T12:00:00Z".to_string(),
                tenant_scope: None,
            },
        ];

        for record in &records {
            provider.add_record(record).unwrap();
        }

        // Query
        let query = RecallQuery::new("deployment failure memory", 5);
        let response = provider.recall(&query).unwrap();

        assert!(!response.is_empty());
        assert!(response.provenance.is_some());
    }

    #[test]
    fn test_persistent_provider_recall_with_filter() {
        let temp_dir = TempDir::new().unwrap();
        let embedder = HashEmbedder::new(128);

        let provider = PersistentRecallProvider::create(temp_dir.path(), embedder, "v1").unwrap();

        // Add records with different outcomes
        provider
            .add_record(&DecisionRecord {
                id: "fail-001".to_string(),
                step: DecisionStep::Reasoning,
                outcome: DecisionOutcome::Failure,
                contract_type: "Reasoning".to_string(),
                input_summary: "Test".to_string(),
                output_summary: "Deployment failed".to_string(),
                chain_id: "chain-001".to_string(),
                created_at: "2026-01-18T12:00:00Z".to_string(),
                tenant_scope: None,
            })
            .unwrap();

        provider
            .add_record(&DecisionRecord {
                id: "success-001".to_string(),
                step: DecisionStep::Reasoning,
                outcome: DecisionOutcome::Success,
                contract_type: "Reasoning".to_string(),
                input_summary: "Test".to_string(),
                output_summary: "Deployment succeeded".to_string(),
                chain_id: "chain-002".to_string(),
                created_at: "2026-01-18T12:00:00Z".to_string(),
                tenant_scope: None,
            })
            .unwrap();

        // Query with filter for failures only
        let query = RecallQuery::new("deployment", 5);
        let filter = RecallFilter::new().with_outcome(DecisionOutcome::Failure);
        let response = provider.recall_with_filter(&query, &filter).unwrap();

        // Should only return the failure record
        assert_eq!(response.len(), 1);
        assert_eq!(response.candidates[0].id, "fail-001");
    }

    #[test]
    fn test_persistent_provider_deterministic_sorting() {
        let temp_dir = TempDir::new().unwrap();
        let embedder = HashEmbedder::new(128);

        let provider = PersistentRecallProvider::create(temp_dir.path(), embedder, "v1").unwrap();

        // Add multiple records with same content (will have same similarity scores)
        for i in 0..5 {
            provider
                .add_record(&DecisionRecord {
                    id: format!("rec-{:03}", i),
                    step: DecisionStep::Reasoning,
                    outcome: DecisionOutcome::Failure,
                    contract_type: "Reasoning".to_string(),
                    input_summary: "Test".to_string(),
                    output_summary: "Identical content for all".to_string(),
                    chain_id: format!("chain-{:03}", i),
                    created_at: "2026-01-18T12:00:00Z".to_string(),
                    tenant_scope: None,
                })
                .unwrap();
        }

        // Query multiple times - should get same order due to deterministic tie-breaker
        let query = RecallQuery::new("Identical content", 5);

        let r1 = provider.recall(&query).unwrap();
        let r2 = provider.recall(&query).unwrap();

        // Results should be in same order
        let ids1: Vec<&str> = r1.candidates.iter().map(|c| c.id.as_str()).collect();
        let ids2: Vec<&str> = r2.candidates.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids1, ids2, "Results should be deterministically sorted");

        // IDs should be sorted alphabetically for tie-breaker
        let mut sorted_ids = ids1.clone();
        sorted_ids.sort();
        assert_eq!(ids1, sorted_ids, "Tie-breaker should sort by ID");
    }

    #[test]
    fn test_persistent_provider_open_existing() {
        let temp_dir = TempDir::new().unwrap();

        // Create and populate
        {
            let embedder = HashEmbedder::new(128);
            let provider =
                PersistentRecallProvider::create(temp_dir.path(), embedder, "v1").unwrap();

            provider
                .add_record(&DecisionRecord {
                    id: "rec-001".to_string(),
                    step: DecisionStep::Reasoning,
                    outcome: DecisionOutcome::Failure,
                    contract_type: "Reasoning".to_string(),
                    input_summary: "Test".to_string(),
                    output_summary: "Test output".to_string(),
                    chain_id: "chain-001".to_string(),
                    created_at: "2026-01-18T12:00:00Z".to_string(),
                    tenant_scope: None,
                })
                .unwrap();
        }

        // Reopen and verify
        {
            let embedder = HashEmbedder::new(128);
            let provider = PersistentRecallProvider::open(temp_dir.path(), embedder).unwrap();

            assert_eq!(provider.record_count().unwrap(), 1);

            let query = RecallQuery::new("Test", 5);
            let response = provider.recall(&query).unwrap();
            assert!(!response.is_empty());
        }
    }

    #[test]
    fn test_persistent_provider_provenance_complete() {
        let temp_dir = TempDir::new().unwrap();
        let embedder = HashEmbedder::new(128);

        let provider = PersistentRecallProvider::create(temp_dir.path(), embedder, "v1").unwrap();

        provider
            .add_record(&DecisionRecord {
                id: "rec-001".to_string(),
                step: DecisionStep::Reasoning,
                outcome: DecisionOutcome::Failure,
                contract_type: "Reasoning".to_string(),
                input_summary: "Test".to_string(),
                output_summary: "Test output".to_string(),
                chain_id: "chain-001".to_string(),
                created_at: "2026-01-18T12:00:00Z".to_string(),
                tenant_scope: None,
            })
            .unwrap();

        let query = RecallQuery::new("Test", 5);
        let response = provider.recall(&query).unwrap();

        // Verify provenance is complete
        let provenance = response.provenance.unwrap();
        assert!(!provenance.query_hash.is_empty());
        assert!(!provenance.embedding_input_hash.is_empty());
        assert!(!provenance.embedding_hash.is_empty());
        assert!(!provenance.embedder_id.is_empty());
        assert!(!provenance.embedder_settings_hash.is_empty());
        assert!(!provenance.corpus_fingerprint.is_empty());
        assert_eq!(provenance.purpose, RecallUse::RuntimeAugmentation);
        assert_eq!(provenance.consumers, vec![RecallConsumer::Kernel]);
    }

    // ========================================================================
    // Tenant Policy Enforcement Tests
    // ========================================================================

    #[test]
    fn test_tenant_policy_required_rejects_missing_scope() {
        let temp_dir = TempDir::new().unwrap();
        let embedder = HashEmbedder::new(128);

        // Create provider with required tenant policy
        let provider = PersistentRecallProvider::create_with_policy(
            temp_dir.path(),
            embedder,
            "v1",
            TenantPolicy::Required,
        )
        .unwrap();

        // Add a record with tenant scope
        provider
            .add_record(&DecisionRecord {
                id: "rec-001".to_string(),
                step: DecisionStep::Reasoning,
                outcome: DecisionOutcome::Failure,
                contract_type: "Reasoning".to_string(),
                input_summary: "Test".to_string(),
                output_summary: "Test output".to_string(),
                chain_id: "chain-001".to_string(),
                created_at: "2026-01-18T12:00:00Z".to_string(),
                tenant_scope: Some("tenant-1".to_string()),
            })
            .unwrap();

        // Query WITHOUT tenant scope should be rejected
        let query = RecallQuery::new("Test", 5);
        let response = provider.recall(&query).unwrap();

        assert!(
            response.candidates.is_empty(),
            "Should return no candidates"
        );
        assert_eq!(
            response.stop_reason,
            Some(StopReason::TenantScopeMissing),
            "Stop reason should be TenantScopeMissing"
        );
    }

    #[test]
    fn test_tenant_policy_required_accepts_with_scope() {
        let temp_dir = TempDir::new().unwrap();
        let embedder = HashEmbedder::new(128);

        let provider = PersistentRecallProvider::create_with_policy(
            temp_dir.path(),
            embedder,
            "v1",
            TenantPolicy::Required,
        )
        .unwrap();

        provider
            .add_record(&DecisionRecord {
                id: "rec-001".to_string(),
                step: DecisionStep::Reasoning,
                outcome: DecisionOutcome::Failure,
                contract_type: "Reasoning".to_string(),
                input_summary: "Test".to_string(),
                output_summary: "Test output".to_string(),
                chain_id: "chain-001".to_string(),
                created_at: "2026-01-18T12:00:00Z".to_string(),
                tenant_scope: Some("tenant-1".to_string()),
            })
            .unwrap();

        // Query WITH tenant scope should work
        let query = RecallQuery::new("Test", 5).with_tenant_scope("tenant-1");
        let filter = RecallFilter::new().with_tenant_scope("tenant-1");
        let response = provider.recall_with_filter(&query, &filter).unwrap();

        assert!(!response.candidates.is_empty(), "Should return candidates");
        assert_ne!(
            response.stop_reason,
            Some(StopReason::TenantScopeMissing),
            "Stop reason should NOT be TenantScopeMissing"
        );
    }

    #[test]
    fn test_tenant_policy_optional_accepts_missing_scope() {
        let temp_dir = TempDir::new().unwrap();
        let embedder = HashEmbedder::new(128);

        // Default policy is Optional
        let provider = PersistentRecallProvider::create(temp_dir.path(), embedder, "v1").unwrap();

        provider
            .add_record(&DecisionRecord {
                id: "rec-001".to_string(),
                step: DecisionStep::Reasoning,
                outcome: DecisionOutcome::Failure,
                contract_type: "Reasoning".to_string(),
                input_summary: "Test".to_string(),
                output_summary: "Test output".to_string(),
                chain_id: "chain-001".to_string(),
                created_at: "2026-01-18T12:00:00Z".to_string(),
                tenant_scope: None,
            })
            .unwrap();

        // Query without tenant scope should work with Optional policy
        let query = RecallQuery::new("Test", 5);
        let response = provider.recall(&query).unwrap();

        assert!(!response.candidates.is_empty(), "Should return candidates");
    }

    // ========================================================================
    // Content Hash Tests
    // ========================================================================

    #[test]
    fn test_corpus_fingerprint_includes_content_hash_after_open() {
        let temp_dir = TempDir::new().unwrap();

        // Create and populate
        {
            let embedder = HashEmbedder::new(128);
            let provider =
                PersistentRecallProvider::create(temp_dir.path(), embedder, "v1").unwrap();

            provider
                .add_record(&DecisionRecord {
                    id: "rec-001".to_string(),
                    step: DecisionStep::Reasoning,
                    outcome: DecisionOutcome::Failure,
                    contract_type: "Reasoning".to_string(),
                    input_summary: "Test".to_string(),
                    output_summary: "Test output".to_string(),
                    chain_id: "chain-001".to_string(),
                    created_at: "2026-01-18T12:00:00Z".to_string(),
                    tenant_scope: None,
                })
                .unwrap();
        }

        // Reopen and verify content hash is populated
        {
            let embedder = HashEmbedder::new(128);
            let provider = PersistentRecallProvider::open(temp_dir.path(), embedder).unwrap();

            let fp = provider.corpus_fingerprint();
            assert!(
                fp.content_hash.is_some(),
                "Content hash should be populated after opening"
            );
        }
    }

    #[test]
    fn test_same_records_produce_same_content_hash() {
        let temp_dir1 = TempDir::new().unwrap();
        let temp_dir2 = TempDir::new().unwrap();

        let record = DecisionRecord {
            id: "rec-001".to_string(),
            step: DecisionStep::Reasoning,
            outcome: DecisionOutcome::Failure,
            contract_type: "Reasoning".to_string(),
            input_summary: "Test".to_string(),
            output_summary: "Test output".to_string(),
            chain_id: "chain-001".to_string(),
            created_at: "2026-01-18T12:00:00Z".to_string(),
            tenant_scope: None,
        };

        // Create two providers with same record
        {
            let embedder = HashEmbedder::new(128);
            let provider =
                PersistentRecallProvider::create(temp_dir1.path(), embedder, "v1").unwrap();
            provider.add_record(&record).unwrap();
        }
        {
            let embedder = HashEmbedder::new(128);
            let provider =
                PersistentRecallProvider::create(temp_dir2.path(), embedder, "v1").unwrap();
            provider.add_record(&record).unwrap();
        }

        // Reopen and compare content hashes
        let embedder1 = HashEmbedder::new(128);
        let provider1 = PersistentRecallProvider::open(temp_dir1.path(), embedder1).unwrap();

        let embedder2 = HashEmbedder::new(128);
        let provider2 = PersistentRecallProvider::open(temp_dir2.path(), embedder2).unwrap();

        assert_eq!(
            provider1.corpus_fingerprint().content_hash,
            provider2.corpus_fingerprint().content_hash,
            "Same records must produce same content hash"
        );
    }

    // ========================================================================
    // Determinism Policy Tests
    // ========================================================================

    #[test]
    fn test_hash_embedder_is_fully_deterministic() {
        let embedder = HashEmbedder::new(128);
        let contract = embedder.determinism_contract();
        assert_eq!(
            contract.level,
            DeterminismLevel::BitExact,
            "HashEmbedder must be Fully deterministic"
        );
    }

    #[test]
    fn test_deterministic_embedder_no_stop_reason() {
        let temp_dir = TempDir::new().unwrap();
        let embedder = HashEmbedder::new(128);

        let provider = PersistentRecallProvider::create(temp_dir.path(), embedder, "v1").unwrap();

        provider
            .add_record(&DecisionRecord {
                id: "rec-001".to_string(),
                step: DecisionStep::Reasoning,
                outcome: DecisionOutcome::Failure,
                contract_type: "Reasoning".to_string(),
                input_summary: "Test".to_string(),
                output_summary: "Test output".to_string(),
                chain_id: "chain-001".to_string(),
                created_at: "2026-01-18T12:00:00Z".to_string(),
                tenant_scope: None,
            })
            .unwrap();

        let query = RecallQuery::new("Test", 5);
        let response = provider.recall(&query).unwrap();

        // With fully deterministic embedder, should NOT have EmbedderNotDeterministic stop reason
        assert_ne!(
            response.stop_reason,
            Some(StopReason::EmbedderNotDeterministic),
            "Deterministic embedder should not trigger EmbedderNotDeterministic"
        );
    }
}
