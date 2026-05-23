//! GCP API endpoint URLs used by the remote storage backends.
//!
//! Centralizes the `*.googleapis.com` base URLs that are otherwise
//! repeated verbatim across `remote/{document,event,object,vector}.rs`
//! and `embedding/vertex.rs`. Callers append the operation-specific
//! suffix at the use site, so the operation stays readable while the
//! base URL has a single source of truth.

/// Base URL for Firestore document operations (project-scoped, default
/// database). Callers append:
///   - `/{collection}/{doc_id}` for document reads/writes
///   - `:runQuery` for collection-group queries
///   - `/{path...}:runQuery` for scoped queries
pub fn firestore_documents(project: &str) -> String {
    format!("https://firestore.googleapis.com/v1/projects/{project}/databases/(default)/documents")
}

/// Root URL for Google Cloud Storage. Callers append the operation
/// path (`/storage/v1/b/{bucket}/o/{name}`, `/upload/...`, etc.).
pub const GCS_BASE: &str = "https://storage.googleapis.com";

/// Base URL for Vertex AI in a given region + project. Callers append
/// `/indexes/{id}/upsertDatapoints`, `/indexEndpoints/{id}:findNeighbors`,
/// `/publishers/google/models/{model}:predict`, etc.
pub fn vertex_aiplatform(region: &str, project: &str) -> String {
    format!("https://{region}-aiplatform.googleapis.com/v1/projects/{project}/locations/{region}")
}
