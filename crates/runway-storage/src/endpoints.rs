//! GCP API endpoint URLs used by the remote storage backends.
//!
//! Centralizes the `*.googleapis.com` base URLs that are otherwise
//! repeated verbatim across `remote/{document,event,object,vector}.rs`
//! and `embedding/vertex.rs`. Callers append the operation-specific
//! suffix at the use site, so the operation stays readable while the
//! base URL has a single source of truth.
//!
//! Honors the standard GCP emulator env vars at call time:
//!   - `FIRESTORE_EMULATOR_HOST` (e.g. `localhost:8080`) for Firestore
//!   - `STORAGE_EMULATOR_HOST` (e.g. `http://localhost:4443`) for GCS
//!
//! Vertex AI has no official emulator; fastembed substitutes locally.

/// Base URL for Firestore document operations (project-scoped, default
/// database). Callers append:
///   - `/{collection}/{doc_id}` for document reads/writes
///   - `:runQuery` for collection-group queries
///   - `/{path...}:runQuery` for scoped queries
pub fn firestore_documents(project: &str) -> String {
    let base = match std::env::var("FIRESTORE_EMULATOR_HOST") {
        Ok(host) if !host.is_empty() => format!("http://{host}"),
        _ => "https://firestore.googleapis.com".to_string(),
    };
    format!("{base}/v1/projects/{project}/databases/(default)/documents")
}

/// Root URL for Google Cloud Storage. Callers append the operation
/// path (`/storage/v1/b/{bucket}/o/{name}`, `/upload/...`, etc.).
pub fn gcs_base() -> String {
    match std::env::var("STORAGE_EMULATOR_HOST") {
        Ok(host) if !host.is_empty() => {
            if host.starts_with("http://") || host.starts_with("https://") {
                host
            } else {
                format!("http://{host}")
            }
        }
        _ => "https://storage.googleapis.com".to_string(),
    }
}

/// Base URL for Vertex AI in a given region + project. Callers append
/// `/indexes/{id}/upsertDatapoints`, `/indexEndpoints/{id}:findNeighbors`,
/// `/publishers/google/models/{model}:predict`, etc.
pub fn vertex_aiplatform(region: &str, project: &str) -> String {
    format!("https://{region}-aiplatform.googleapis.com/v1/projects/{project}/locations/{region}")
}
