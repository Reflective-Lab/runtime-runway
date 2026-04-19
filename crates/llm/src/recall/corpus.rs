// Copyright 2024-2026 Reflective Labs

//! Corpus fingerprinting for recall reproducibility.

use super::embedder::Embedder;
use serde::{Deserialize, Serialize};

/// Compound fingerprint for corpus versioning.
///
/// All components must match for recall to be reproducible.
/// This enables deterministic replay of recall queries.
///
/// # Components
///
/// - Schema version: Table structure version
/// - Embedder ID: Which embedder was used
/// - Embedder settings hash: Exact settings (model, normalization, etc.)
/// - Dataset snapshot: Content version (git hash, timestamp, etc.)
/// - Content hash: blake3 of sorted record IDs + count (content-derived)
/// - Tenant scope: Optional multi-tenant isolation
///
/// # Invariant
///
/// Same query + same corpus fingerprint + same embedder => same candidates.
/// The `content_hash` field ensures this is testable by including actual
/// corpus content in the fingerprint.
///
/// # Axiom: Records are immutable once indexed
///
/// The `content_hash` is computed from `sorted_ids ∥ count`, not record bodies.
/// This assumes records are **immutable once indexed**:
/// - Re-indexing a changed record must create a new ID
/// - Updating a record's content without changing its ID is undefined behavior
///
/// If record content mutability is ever required, extend the hash to include
/// `id ∥ record_hash` per record. For now, immutability is enforced by design.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct CorpusFingerprint {
    /// Schema version (table structure)
    pub schema_version: String,

    /// Embedder ID
    pub embedder_id: String,

    /// Hash of embedder settings
    pub embedder_settings_hash: String,

    /// Dataset snapshot identifier (commit hash, timestamp, or content hash)
    pub dataset_snapshot: String,

    /// Content-derived hash: blake3(sorted_record_ids || record_count)
    ///
    /// This ensures "same fingerprint => same content" is actually testable.
    /// Updated whenever records are added/removed from the corpus.
    #[serde(default)]
    pub content_hash: Option<String>,

    /// Optional tenant scope
    pub tenant_scope: Option<String>,
}

impl CorpusFingerprint {
    /// Create a new corpus fingerprint.
    ///
    /// The `content_hash` starts as `None` and should be updated via
    /// `with_content_hash` once records are known.
    #[must_use]
    pub fn new(
        schema_version: impl Into<String>,
        embedder: &dyn Embedder,
        dataset_snapshot: impl Into<String>,
        tenant_scope: Option<String>,
    ) -> Self {
        let settings = embedder.settings_snapshot();
        let settings_json = serde_json::to_string(&settings).unwrap_or_default();
        let settings_hash = blake3::hash(settings_json.as_bytes()).to_hex().to_string();

        Self {
            schema_version: schema_version.into(),
            embedder_id: embedder.embedder_id().to_string(),
            embedder_settings_hash: settings_hash,
            dataset_snapshot: dataset_snapshot.into(),
            content_hash: None,
            tenant_scope,
        }
    }

    /// Create a fingerprint with explicit values (for deserialization/testing).
    #[must_use]
    pub fn with_values(
        schema_version: impl Into<String>,
        embedder_id: impl Into<String>,
        embedder_settings_hash: impl Into<String>,
        dataset_snapshot: impl Into<String>,
        tenant_scope: Option<String>,
    ) -> Self {
        Self {
            schema_version: schema_version.into(),
            embedder_id: embedder_id.into(),
            embedder_settings_hash: embedder_settings_hash.into(),
            dataset_snapshot: dataset_snapshot.into(),
            content_hash: None,
            tenant_scope,
        }
    }

    /// Compute content hash from sorted record IDs and count.
    ///
    /// This creates a content-derived fingerprint component that ensures
    /// "same fingerprint => same corpus content" is testable.
    #[must_use]
    pub fn compute_content_hash(record_ids: &[String]) -> String {
        let mut sorted_ids = record_ids.to_vec();
        sorted_ids.sort();

        let mut hasher = blake3::Hasher::new();
        hasher.update(sorted_ids.len().to_le_bytes().as_slice());
        for id in &sorted_ids {
            hasher.update(id.as_bytes());
            hasher.update(b"\0"); // separator
        }
        hasher.finalize().to_hex().to_string()
    }

    /// Return a new fingerprint with content hash set.
    #[must_use]
    pub fn with_content_hash(mut self, record_ids: &[String]) -> Self {
        self.content_hash = Some(Self::compute_content_hash(record_ids));
        self
    }

    /// Update the content hash in place.
    pub fn update_content_hash(&mut self, record_ids: &[String]) {
        self.content_hash = Some(Self::compute_content_hash(record_ids));
    }

    /// Get the canonical version string for comparison.
    ///
    /// Format: `schema:{version}/embedder:{id}@{hash8}/dataset:{snapshot}/content:{hash8}/tenant:{scope}`
    ///
    /// The content hash ensures this string changes when corpus content changes.
    #[must_use]
    pub fn to_version_string(&self) -> String {
        let settings_prefix = if self.embedder_settings_hash.len() >= 8 {
            &self.embedder_settings_hash[..8]
        } else {
            &self.embedder_settings_hash
        };

        let content_prefix = self
            .content_hash
            .as_ref()
            .map(|h| if h.len() >= 8 { &h[..8] } else { h.as_str() })
            .unwrap_or("none");

        format!(
            "schema:{}/embedder:{}@{}/dataset:{}/content:{}/tenant:{}",
            self.schema_version,
            self.embedder_id,
            settings_prefix,
            self.dataset_snapshot,
            content_prefix,
            self.tenant_scope.as_deref().unwrap_or("global")
        )
    }

    /// Check if two fingerprints are compatible for comparison.
    ///
    /// Fingerprints are compatible if they have the same schema version
    /// and embedder configuration. Dataset snapshots may differ.
    #[must_use]
    pub fn is_compatible(&self, other: &Self) -> bool {
        self.schema_version == other.schema_version
            && self.embedder_id == other.embedder_id
            && self.embedder_settings_hash == other.embedder_settings_hash
    }

    /// Check if this fingerprint matches another exactly.
    ///
    /// Exact match means all components are identical.
    #[must_use]
    pub fn matches_exactly(&self, other: &Self) -> bool {
        self == other
    }

    /// Get a short identifier for logging.
    #[must_use]
    pub fn short_id(&self) -> String {
        let hash_prefix = if self.embedder_settings_hash.len() >= 4 {
            &self.embedder_settings_hash[..4]
        } else {
            &self.embedder_settings_hash
        };

        format!(
            "{}@{}:{}",
            self.embedder_id,
            hash_prefix,
            &self.dataset_snapshot[..self.dataset_snapshot.len().min(8)]
        )
    }
}

// ============================================================================
// Tenant Policy
// ============================================================================

/// Policy for tenant scope enforcement.
///
/// Controls whether tenant scope is required for corpus operations.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TenantPolicy {
    /// Tenant scope is optional (single-tenant or global corpus).
    #[default]
    Optional,
    /// Tenant scope is required (multi-tenant corpus).
    /// Queries without tenant scope will be rejected.
    Required,
}

impl TenantPolicy {
    /// Check if a tenant scope value is valid for this policy.
    ///
    /// Returns `true` if:
    /// - Policy is `Optional`, OR
    /// - Policy is `Required` AND tenant is `Some(_)`
    #[must_use]
    pub fn is_valid(&self, tenant: Option<&str>) -> bool {
        match self {
            Self::Optional => true,
            Self::Required => tenant.is_some(),
        }
    }
}

// ============================================================================
// Builder
// ============================================================================

/// Builder for corpus fingerprints.
#[derive(Debug, Default)]
pub struct CorpusFingerprintBuilder {
    schema_version: Option<String>,
    embedder_id: Option<String>,
    embedder_settings_hash: Option<String>,
    dataset_snapshot: Option<String>,
    content_hash: Option<String>,
    tenant_scope: Option<String>,
}

impl CorpusFingerprintBuilder {
    /// Create a new builder.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the schema version.
    #[must_use]
    pub fn schema_version(mut self, version: impl Into<String>) -> Self {
        self.schema_version = Some(version.into());
        self
    }

    /// Set embedder information from an embedder instance.
    #[must_use]
    pub fn embedder(mut self, embedder: &dyn Embedder) -> Self {
        let settings = embedder.settings_snapshot();
        let settings_json = serde_json::to_string(&settings).unwrap_or_default();
        let settings_hash = blake3::hash(settings_json.as_bytes()).to_hex().to_string();

        self.embedder_id = Some(embedder.embedder_id().to_string());
        self.embedder_settings_hash = Some(settings_hash);
        self
    }

    /// Set the dataset snapshot identifier.
    #[must_use]
    pub fn dataset_snapshot(mut self, snapshot: impl Into<String>) -> Self {
        self.dataset_snapshot = Some(snapshot.into());
        self
    }

    /// Set the content hash from record IDs.
    #[must_use]
    pub fn content_hash_from_records(mut self, record_ids: &[String]) -> Self {
        self.content_hash = Some(CorpusFingerprint::compute_content_hash(record_ids));
        self
    }

    /// Set the tenant scope.
    #[must_use]
    pub fn tenant_scope(mut self, scope: impl Into<String>) -> Self {
        self.tenant_scope = Some(scope.into());
        self
    }

    /// Build the fingerprint.
    ///
    /// # Panics
    ///
    /// Panics if required fields are not set.
    #[must_use]
    pub fn build(self) -> CorpusFingerprint {
        CorpusFingerprint {
            schema_version: self.schema_version.expect("schema_version required"),
            embedder_id: self.embedder_id.expect("embedder_id required"),
            embedder_settings_hash: self
                .embedder_settings_hash
                .expect("embedder_settings_hash required"),
            dataset_snapshot: self.dataset_snapshot.expect("dataset_snapshot required"),
            content_hash: self.content_hash,
            tenant_scope: self.tenant_scope,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recall::HashEmbedder;

    #[test]
    fn test_corpus_fingerprint_creation() {
        let embedder = HashEmbedder::new(384);
        let fp = CorpusFingerprint::new("v1", &embedder, "abc123", None);

        assert_eq!(fp.schema_version, "v1");
        assert_eq!(fp.embedder_id, "hash-embedder-test");
        assert_eq!(fp.dataset_snapshot, "abc123");
        assert!(fp.content_hash.is_none());
        assert!(fp.tenant_scope.is_none());
    }

    #[test]
    fn test_corpus_fingerprint_equality() {
        let embedder = HashEmbedder::new(384);
        let fp1 = CorpusFingerprint::new("v1", &embedder, "abc123", None);
        let fp2 = CorpusFingerprint::new("v1", &embedder, "abc123", None);
        assert_eq!(fp1, fp2);
    }

    #[test]
    fn test_corpus_fingerprint_inequality() {
        let embedder = HashEmbedder::new(384);
        let fp1 = CorpusFingerprint::new("v1", &embedder, "abc123", None);
        let fp2 = CorpusFingerprint::new("v1", &embedder, "def456", None);
        assert_ne!(fp1, fp2);
    }

    #[test]
    fn test_version_string() {
        let embedder = HashEmbedder::new(384);
        let fp = CorpusFingerprint::new("v1", &embedder, "abc123", None);
        let version = fp.to_version_string();

        assert!(version.contains("schema:v1"));
        assert!(version.contains("embedder:hash-embedder-test"));
        assert!(version.contains("dataset:abc123"));
        assert!(version.contains("content:none"));
        assert!(version.contains("tenant:global"));
    }

    #[test]
    fn test_version_string_with_tenant() {
        let embedder = HashEmbedder::new(384);
        let fp = CorpusFingerprint::new("v1", &embedder, "abc123", Some("tenant-1".to_string()));
        let version = fp.to_version_string();

        assert!(version.contains("tenant:tenant-1"));
    }

    #[test]
    fn test_is_compatible() {
        let embedder = HashEmbedder::new(384);
        let fp1 = CorpusFingerprint::new("v1", &embedder, "abc123", None);
        let fp2 = CorpusFingerprint::new("v1", &embedder, "def456", None);

        // Same schema and embedder, different dataset
        assert!(fp1.is_compatible(&fp2));
    }

    #[test]
    fn test_not_compatible_different_schema() {
        let embedder = HashEmbedder::new(384);
        let fp1 = CorpusFingerprint::new("v1", &embedder, "abc123", None);
        let fp2 = CorpusFingerprint::new("v2", &embedder, "abc123", None);

        assert!(!fp1.is_compatible(&fp2));
    }

    #[test]
    fn test_builder() {
        let embedder = HashEmbedder::new(384);
        let fp = CorpusFingerprintBuilder::new()
            .schema_version("v1")
            .embedder(&embedder)
            .dataset_snapshot("commit-abc")
            .tenant_scope("acme")
            .build();

        assert_eq!(fp.schema_version, "v1");
        assert_eq!(fp.embedder_id, "hash-embedder-test");
        assert_eq!(fp.dataset_snapshot, "commit-abc");
        assert_eq!(fp.tenant_scope, Some("acme".to_string()));
    }

    #[test]
    fn test_short_id() {
        let embedder = HashEmbedder::new(384);
        let fp = CorpusFingerprint::new("v1", &embedder, "abc123def", None);
        let short = fp.short_id();

        assert!(short.contains("hash-embedder-test"));
        assert!(short.len() < 50);
    }

    // ========================================================================
    // Content Hash Tests
    // ========================================================================

    #[test]
    fn test_content_hash_deterministic() {
        let ids = vec!["id-1".to_string(), "id-2".to_string(), "id-3".to_string()];
        let hash1 = CorpusFingerprint::compute_content_hash(&ids);
        let hash2 = CorpusFingerprint::compute_content_hash(&ids);
        assert_eq!(hash1, hash2, "Same IDs must produce same hash");
    }

    #[test]
    fn test_content_hash_order_independent() {
        let ids1 = vec!["id-3".to_string(), "id-1".to_string(), "id-2".to_string()];
        let ids2 = vec!["id-1".to_string(), "id-2".to_string(), "id-3".to_string()];
        let hash1 = CorpusFingerprint::compute_content_hash(&ids1);
        let hash2 = CorpusFingerprint::compute_content_hash(&ids2);
        assert_eq!(hash1, hash2, "Different order must produce same hash");
    }

    #[test]
    fn test_content_hash_different_for_different_ids() {
        let ids1 = vec!["id-1".to_string(), "id-2".to_string()];
        let ids2 = vec!["id-1".to_string(), "id-3".to_string()];
        let hash1 = CorpusFingerprint::compute_content_hash(&ids1);
        let hash2 = CorpusFingerprint::compute_content_hash(&ids2);
        assert_ne!(hash1, hash2, "Different IDs must produce different hash");
    }

    #[test]
    fn test_content_hash_includes_count() {
        // Same IDs but different count should produce different hash
        let ids1 = vec!["id-1".to_string()];
        let ids2 = vec!["id-1".to_string(), "id-1".to_string()];
        let hash1 = CorpusFingerprint::compute_content_hash(&ids1);
        let hash2 = CorpusFingerprint::compute_content_hash(&ids2);
        assert_ne!(hash1, hash2, "Different count must produce different hash");
    }

    #[test]
    fn test_with_content_hash() {
        let embedder = HashEmbedder::new(384);
        let ids = vec!["rec-1".to_string(), "rec-2".to_string()];
        let fp = CorpusFingerprint::new("v1", &embedder, "snap", None).with_content_hash(&ids);

        assert!(fp.content_hash.is_some());
        let version = fp.to_version_string();
        assert!(!version.contains("content:none"));
    }

    #[test]
    fn test_update_content_hash() {
        let embedder = HashEmbedder::new(384);
        let mut fp = CorpusFingerprint::new("v1", &embedder, "snap", None);
        assert!(fp.content_hash.is_none());

        let ids = vec!["rec-1".to_string()];
        fp.update_content_hash(&ids);
        assert!(fp.content_hash.is_some());
    }

    #[test]
    fn test_fingerprint_equality_with_content_hash() {
        let embedder = HashEmbedder::new(384);
        let ids = vec!["rec-1".to_string(), "rec-2".to_string()];

        let fp1 = CorpusFingerprint::new("v1", &embedder, "snap", None).with_content_hash(&ids);
        let fp2 = CorpusFingerprint::new("v1", &embedder, "snap", None).with_content_hash(&ids);

        assert_eq!(fp1, fp2, "Same content must produce equal fingerprints");
    }

    #[test]
    fn test_fingerprint_inequality_different_content() {
        let embedder = HashEmbedder::new(384);
        let ids1 = vec!["rec-1".to_string()];
        let ids2 = vec!["rec-1".to_string(), "rec-2".to_string()];

        let fp1 = CorpusFingerprint::new("v1", &embedder, "snap", None).with_content_hash(&ids1);
        let fp2 = CorpusFingerprint::new("v1", &embedder, "snap", None).with_content_hash(&ids2);

        assert_ne!(
            fp1, fp2,
            "Different content must produce different fingerprints"
        );
    }

    // ========================================================================
    // Tenant Policy Tests
    // ========================================================================

    #[test]
    fn test_tenant_policy_optional_accepts_none() {
        let policy = TenantPolicy::Optional;
        assert!(policy.is_valid(None));
    }

    #[test]
    fn test_tenant_policy_optional_accepts_some() {
        let policy = TenantPolicy::Optional;
        assert!(policy.is_valid(Some("tenant-1")));
    }

    #[test]
    fn test_tenant_policy_required_rejects_none() {
        let policy = TenantPolicy::Required;
        assert!(
            !policy.is_valid(None),
            "Required policy must reject None tenant"
        );
    }

    #[test]
    fn test_tenant_policy_required_accepts_some() {
        let policy = TenantPolicy::Required;
        assert!(policy.is_valid(Some("tenant-1")));
    }

    #[test]
    fn test_tenant_policy_default_is_optional() {
        let policy = TenantPolicy::default();
        assert_eq!(policy, TenantPolicy::Optional);
    }
}
