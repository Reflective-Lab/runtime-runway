//! Contract tests against the local (redb + local FS + fastembed) backend.

use std::sync::Arc;

use runway_storage::StorageKit;
use runway_storage_contract::{ContractContext, document};

#[tokio::test]
async fn document_contract() {
    let tmp = tempfile::tempdir().unwrap();
    let kit = StorageKit::local(tmp.path()).await.unwrap();
    let ctx = ContractContext::new("redb", "_contract");
    document::run_document_suite(Arc::clone(&kit.documents), ctx)
        .await
        .assert_passed();
}
