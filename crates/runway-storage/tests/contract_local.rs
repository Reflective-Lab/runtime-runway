//! Contract tests against the local (redb + local FS + fastembed) backend.

use std::sync::Arc;

use runway_storage::StorageKit;
use runway_storage_contract::{ContractContext, document, embedding, event, object, vector};

async fn build_kit() -> (StorageKit, tempfile::TempDir) {
    let tmp = tempfile::tempdir().unwrap();
    let kit = StorageKit::local(tmp.path()).await.unwrap();
    (kit, tmp)
}

fn ctx() -> ContractContext {
    ContractContext::new("redb+local-fs+fastembed", "_contract")
}

#[tokio::test]
async fn document_contract() {
    let (kit, _tmp) = build_kit().await;
    document::run_document_suite(Arc::clone(&kit.documents), ctx())
        .await
        .assert_passed();
}

#[tokio::test]
async fn embedding_contract() {
    let (kit, _tmp) = build_kit().await;
    embedding::run_embedding_shape_suite(Arc::clone(&kit.embeddings), ctx())
        .await
        .assert_passed();
}

#[tokio::test]
async fn vector_contract() {
    let (kit, _tmp) = build_kit().await;
    vector::run_vector_shape_suite(Arc::clone(&kit.vectors), ctx())
        .await
        .assert_passed();
}

#[tokio::test]
async fn object_contract() {
    let (kit, _tmp) = build_kit().await;
    object::run_object_suite(Arc::clone(&kit.objects), ctx())
        .await
        .assert_passed();
}

#[tokio::test]
async fn event_contract() {
    let (kit, _tmp) = build_kit().await;
    event::run_event_suite(Arc::clone(&kit.events), ctx())
        .await
        .assert_passed();
}

#[tokio::test]
async fn syncable_event_contract() {
    let (kit, _tmp) = build_kit().await;
    let syncable = kit
        .syncable_events
        .clone()
        .expect("local backend must provide SyncableEventLog");
    event::run_syncable_event_suite(syncable, ctx())
        .await
        .assert_passed();
}
