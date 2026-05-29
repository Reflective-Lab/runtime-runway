//! Contract tests against real GCP (staging project). #[ignore]'d by default;
//! run with `cargo test -- --ignored` or `just contract-staging` (Task 14).
//!
//! Requires:
//!   RUNWAY_CONTRACT_PROJECT — staging GCP project id
//!   RUNWAY_CONTRACT_BUCKET  — staging GCS bucket (must exist; pre-provisioned)
//!   RUNWAY_CONTRACT_REGION  — Vertex AI region (e.g. "us-central1")
//!   RUNWAY_CONTRACT_TOKEN   — OAuth2 bearer token (locally: `gcloud auth print-access-token`;
//!                             in CI: injected by workload identity federation / service account).
//!
//! Each run uses a fresh _contract/<uuid> namespace prefix; per-test cleanup
//! is best-effort. A Terraform-managed retention policy / scheduled sweep on
//! the staging bucket catches debris from failed runs.

use std::sync::Arc;

use runway_storage::{
    StorageKit,
    remote::{RemoteConfig, RemoteStorageKit, TokenSource},
};
use runway_storage_contract::{ContractContext, document, embedding, event, object, vector};

fn require_env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("env var {name} required"))
}

fn real_gcp_config() -> RemoteConfig {
    RemoteConfig {
        project_id: require_env("RUNWAY_CONTRACT_PROJECT"),
        region: require_env("RUNWAY_CONTRACT_REGION"),
        bucket: require_env("RUNWAY_CONTRACT_BUCKET"),
        // Static token: locally populate via `gcloud auth print-access-token`.
        // In CI, inject from workload identity federation or a service account key.
        // TokenSource::Metadata is only suitable on GCE/Cloud Run where the
        // metadata server is reachable; Static is the correct variant for both
        // local dev and CI environments that are not running on GCE.
        token_source: TokenSource::Static(require_env("RUNWAY_CONTRACT_TOKEN")),
    }
}

async fn build_kit() -> StorageKit {
    RemoteStorageKit::build(real_gcp_config())
        .await
        .expect("real GCP kit build")
}

fn ctx() -> ContractContext {
    let run_id = uuid::Uuid::new_v4();
    ContractContext::new("firestore+gcs+vertex-ai", format!("_contract/{run_id}"))
}

/// Best-effort namespace cleanup. Swallows errors so the original test failure
/// (if any) is what the developer sees. The namespace prefix (uuid-based) plus
/// a Terraform-managed retention policy / scheduled sweep on the staging bucket
/// and Firestore catches anything missed.
async fn cleanup(kit: &StorageKit, namespace: &str) {
    // Log the namespace so debugging is easier.
    tracing::info!(
        namespace = %namespace,
        "contract_real_gcp cleanup namespace (see Terraform retention sweep for actual deletion)"
    );

    // Object suite: GCS list + delete via ObjectStore trait.
    if let Ok(keys) = kit.objects.list(&format!("{namespace}/objects/")).await {
        for key in keys {
            let _ = kit.objects.delete(&key).await;
        }
    }

    // Document, vector, embedding, event suites: per-item cleanup requires
    // tracking inserted ids, which the suites don't expose. The retention sweep
    // handles Firestore subcollections and Vertex AI index entries.
}

#[tokio::test]
#[ignore]
async fn document_contract() {
    let kit = build_kit().await;
    let context = ctx();
    let report = document::run_document_suite(Arc::clone(&kit.documents), context.clone()).await;
    cleanup(&kit, &context.namespace).await;
    report.assert_passed();
}

#[tokio::test]
#[ignore]
async fn object_contract() {
    let kit = build_kit().await;
    let context = ctx();
    let report = object::run_object_suite(Arc::clone(&kit.objects), context.clone()).await;
    cleanup(&kit, &context.namespace).await;
    report.assert_passed();
}

#[tokio::test]
#[ignore]
async fn event_contract() {
    let kit = build_kit().await;
    let context = ctx();
    let report = event::run_event_suite(Arc::clone(&kit.events), context.clone()).await;
    cleanup(&kit, &context.namespace).await;
    report.assert_passed();
}

#[tokio::test]
#[ignore]
async fn vector_contract() {
    let kit = build_kit().await;
    let context = ctx();
    let report = vector::run_vector_shape_suite(Arc::clone(&kit.vectors), context.clone()).await;
    cleanup(&kit, &context.namespace).await;
    report.assert_passed();
}

#[tokio::test]
#[ignore]
async fn embedding_contract() {
    let kit = build_kit().await;
    let context = ctx();
    let report =
        embedding::run_embedding_shape_suite(Arc::clone(&kit.embeddings), context.clone()).await;
    cleanup(&kit, &context.namespace).await;
    report.assert_passed();
}
