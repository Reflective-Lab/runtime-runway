//! Contract tests against emulated GCP services + fastembed. #[ignore]'d by
//! default so `cargo test --all-targets` (which has no emulator running)
//! skips them; run with `cargo test -- --ignored` or `just contract-emulator`.
//!
//! Requires the emulator stack from docker-compose.contract.yml to be running.
//! `just contract-emulator` (Task 14) handles startup/teardown automatically.
//! For manual runs, ensure the env vars FIRESTORE_EMULATOR_HOST,
//! PUBSUB_EMULATOR_HOST, STORAGE_EMULATOR_HOST are set before invoking.

use std::sync::Arc;

use runway_storage::{
    EmbeddingProvider, StorageKit,
    embedding::local::LocalEmbedder,
    remote::{RemoteConfig, RemoteStorageKit, TokenSource},
};
use runway_storage_contract::{ContractContext, document, embedding, event, object};

/// Build a RemoteConfig pointing at the emulators. Project id is fixed to
/// "runway-contract" — the same value docker-compose.contract.yml passes to
/// the Firestore and Pub/Sub emulators via --project.
///
/// Token source is set to a static empty string; the emulators do not enforce
/// auth, so any (or no) token is accepted.
///
/// The `region` and `bucket` fields must be populated but emulator routing is
/// controlled entirely by the FIRESTORE_EMULATOR_HOST / STORAGE_EMULATOR_HOST
/// env vars that the underlying GCP client libraries read automatically.
fn emulator_config() -> RemoteConfig {
    RemoteConfig {
        project_id: "runway-contract".into(),
        region: "us-central1".into(),
        bucket: "runway-contract".into(),
        token_source: TokenSource::Static(String::new()),
    }
}

async fn build_kit() -> StorageKit {
    assert!(
        std::env::var("FIRESTORE_EMULATOR_HOST").is_ok(),
        "FIRESTORE_EMULATOR_HOST must be set (docker compose sets this automatically; \
         set it manually for bare `cargo test` runs)"
    );
    assert!(
        std::env::var("STORAGE_EMULATOR_HOST").is_ok(),
        "STORAGE_EMULATOR_HOST must be set (docker compose sets this automatically; \
         set it manually for bare `cargo test` runs)"
    );
    // PUBSUB_EMULATOR_HOST: required only if Pub/Sub is directly exercised by
    // the suites. The contract suites currently go through Firestore for events,
    // so we don't hard-assert it.

    let fastembed = Arc::new(LocalEmbedder::new()) as Arc<dyn EmbeddingProvider>;

    RemoteStorageKit::build_with_embedder(emulator_config(), Some(fastembed))
        .await
        .expect("emulator kit build failed — check that the emulator stack is running")
}

fn ctx() -> ContractContext {
    let run_id = uuid::Uuid::new_v4();
    ContractContext::new(
        "firestore-emulator+gcs-emulator+fastembed",
        format!("_contract/{run_id}"),
    )
}

#[tokio::test]
#[ignore]
async fn document_contract() {
    let kit = build_kit().await;
    document::run_document_suite(Arc::clone(&kit.documents), ctx())
        .await
        .assert_passed();
}

#[tokio::test]
#[ignore]
async fn object_contract() {
    let kit = build_kit().await;
    object::run_object_suite(Arc::clone(&kit.objects), ctx())
        .await
        .assert_passed();
}

#[tokio::test]
#[ignore]
async fn event_contract() {
    let kit = build_kit().await;
    event::run_event_suite(Arc::clone(&kit.events), ctx())
        .await
        .assert_passed();
}

// VectorStore exercises Vertex AI Vector Search, which has no public
// emulator. The contract suite for vectors only runs against real GCP
// (contract_real_gcp.rs). Re-enable here only if/when a usable emulator
// substitute appears.

#[tokio::test]
#[ignore]
async fn embedding_contract() {
    let kit = build_kit().await;
    embedding::run_embedding_shape_suite(Arc::clone(&kit.embeddings), ctx())
        .await
        .assert_passed();
}

// SyncableEventLog is local-only (redb backend); no test here.
