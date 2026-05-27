//! ObjectStore contract suite.

use std::sync::Arc;

use bytes::Bytes;
use runway_storage::ObjectStore;
use serde_json::json;

use crate::harness::{ContractContext, SuiteReport};
use crate::{contract_assert, contract_assert_eq, contract_test};

pub async fn run_object_suite(store: Arc<dyn ObjectStore>, ctx: ContractContext) -> SuiteReport {
    let report = SuiteReport::new(&ctx.backend, "ObjectStore");
    let prefix = ctx.scope("objects");

    contract_test!(&report, "put_then_get_byte_equal", async {
        let key = format!("{prefix}/bytes-1");
        let data = Bytes::from_static(b"\x00\x01\x02\xff\xfe");
        store
            .put(&key, data.clone(), None)
            .await
            .map_err(|e| e.to_string())?;
        let got = store.get(&key).await.map_err(|e| e.to_string())?;
        contract_assert_eq!(got, data, "byte-equal roundtrip");
        Ok(())
    });

    contract_test!(&report, "put_get_text_roundtrip", async {
        let key = format!("{prefix}/text-1");
        let text = "hello, contract";
        store
            .put_text(&key, text)
            .await
            .map_err(|e| e.to_string())?;
        let got = store.get_text(&key).await.map_err(|e| e.to_string())?;
        contract_assert_eq!(got, text.to_string(), "UTF-8 roundtrip");
        Ok(())
    });

    contract_test!(&report, "put_get_json_roundtrip", async {
        let key = format!("{prefix}/json-1");
        let value = json!({"a": 1, "b": [true, false], "c": "x"});
        // Note: put_json/get_json require Self: Sized so they can't be called
        // through Arc<dyn>. Use put/get with manual JSON encode/decode.
        let bytes = Bytes::from(serde_json::to_vec(&value).map_err(|e| e.to_string())?);
        store
            .put(&key, bytes, Some("application/json"))
            .await
            .map_err(|e| e.to_string())?;
        let got = store.get(&key).await.map_err(|e| e.to_string())?;
        let decoded: serde_json::Value = serde_json::from_slice(&got).map_err(|e| e.to_string())?;
        contract_assert_eq!(decoded, value, "JSON roundtrip");
        Ok(())
    });

    contract_test!(&report, "get_missing_returns_not_found", async {
        match store.get(&format!("{prefix}/never-existed")).await {
            Ok(_) => Err("expected NotFound error, got Ok".to_string()),
            Err(e) => {
                let msg = e.to_string();
                contract_assert!(
                    msg.to_lowercase().contains("not found")
                        || msg.to_lowercase().contains("notfound"),
                    "expected 'not found' in error, got: {}",
                    msg
                );
                Ok(())
            }
        }
    });

    contract_test!(&report, "exists_reflects_state", async {
        let key = format!("{prefix}/exists-1");
        contract_assert!(
            !store.exists(&key).await.map_err(|e| e.to_string())?,
            "should not exist before put"
        );
        store
            .put(&key, Bytes::from_static(b"x"), None)
            .await
            .map_err(|e| e.to_string())?;
        contract_assert!(
            store.exists(&key).await.map_err(|e| e.to_string())?,
            "should exist after put"
        );
        store.delete(&key).await.map_err(|e| e.to_string())?;
        contract_assert!(
            !store.exists(&key).await.map_err(|e| e.to_string())?,
            "should not exist after delete"
        );
        Ok(())
    });

    contract_test!(&report, "delete_idempotent", async {
        store
            .delete(&format!("{prefix}/never-existed"))
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    });

    contract_test!(&report, "list_prefix_returns_matching_keys", async {
        let scope = format!("{prefix}/listing");
        store
            .put(&format!("{scope}/a/1"), Bytes::from_static(b"1"), None)
            .await
            .map_err(|e| e.to_string())?;
        store
            .put(&format!("{scope}/a/2"), Bytes::from_static(b"2"), None)
            .await
            .map_err(|e| e.to_string())?;
        store
            .put(&format!("{scope}/b/1"), Bytes::from_static(b"3"), None)
            .await
            .map_err(|e| e.to_string())?;

        let keys = store
            .list(&format!("{scope}/a/"))
            .await
            .map_err(|e| e.to_string())?;
        contract_assert_eq!(keys.len(), 2, "expected 2 keys under a/, got {:?}", keys);
        contract_assert!(
            keys.iter().all(|k| k.contains("/a/")),
            "all keys should be under a/: {:?}",
            keys
        );
        Ok(())
    });

    contract_test!(&report, "list_prefix_empty_when_no_match", async {
        let keys = store
            .list(&format!("{prefix}/nonexistent/"))
            .await
            .map_err(|e| e.to_string())?;
        contract_assert!(keys.is_empty(), "expected empty key list, got {:?}", keys);
        Ok(())
    });

    contract_test!(&report, "large_payload_roundtrip", async {
        let key = format!("{prefix}/large-1");
        let data: Vec<u8> = (0..1_048_576u32).map(|i| (i % 256) as u8).collect();
        let bytes = Bytes::from(data.clone());
        store
            .put(&key, bytes.clone(), None)
            .await
            .map_err(|e| e.to_string())?;
        let got = store.get(&key).await.map_err(|e| e.to_string())?;
        contract_assert_eq!(got.len(), bytes.len(), "1 MB byte length roundtrip");
        contract_assert!(got == bytes, "1 MB byte content roundtrip");
        Ok(())
    });

    report
}
