//! VectorStore shape suite — equivalence is impossible (different ANN
//! implementations score differently). One equivalence-flavored test:
//! self_match_returns_self_first.

use std::collections::HashMap;
use std::sync::Arc;

use runway_storage::{Embedding, VectorStore};
use serde_json::json;

use crate::harness::{ContractContext, SuiteReport};
use crate::{contract_assert, contract_assert_eq, contract_test};

fn unit_vector(seed: f32) -> Embedding {
    let mut v: Vec<f32> = (0..768).map(|i| (i as f32 + seed).sin()).collect();
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        for x in v.iter_mut() {
            *x /= norm;
        }
    }
    Embedding::new(v).unwrap()
}

pub async fn run_vector_shape_suite(
    store: Arc<dyn VectorStore>,
    ctx: ContractContext,
) -> SuiteReport {
    let report = SuiteReport::new(&ctx.backend, "VectorStore");
    let ns = ctx.scope("vecs");

    contract_test!(&report, "upsert_accepts_valid_embedding", async {
        let e = unit_vector(0.0);
        store
            .upsert(&ns, "v1", &e, Some("hello"), HashMap::new())
            .await
            .map_err(|err| err.to_string())?;
        Ok(())
    });

    contract_test!(&report, "search_returns_at_most_top_k", async {
        let ns = format!("{ns}-topk");
        for i in 0..10 {
            let e = unit_vector(i as f32);
            store
                .upsert(&ns, &format!("id{i}"), &e, None, HashMap::new())
                .await
                .map_err(|err| err.to_string())?;
        }
        let q = unit_vector(0.5);
        let results = store
            .search(&ns, &q, 3)
            .await
            .map_err(|err| err.to_string())?;
        contract_assert!(
            results.len() <= 3,
            "expected at most 3 results, got {}",
            results.len()
        );
        Ok(())
    });

    contract_test!(&report, "search_returns_empty_for_empty_namespace", async {
        let q = unit_vector(0.0);
        let results = store
            .search(&format!("{ns}-empty"), &q, 5)
            .await
            .map_err(|err| err.to_string())?;
        contract_assert!(
            results.is_empty(),
            "expected empty result set, got {} matches",
            results.len()
        );
        Ok(())
    });

    contract_test!(&report, "self_match_returns_self_first", async {
        let ns = format!("{ns}-self");
        let v = unit_vector(42.0);
        store
            .upsert(&ns, "X", &v, Some("self"), HashMap::new())
            .await
            .map_err(|err| err.to_string())?;
        // Insert a few decoys
        for i in 0..3 {
            let d = unit_vector(100.0 + i as f32);
            store
                .upsert(&ns, &format!("decoy{i}"), &d, None, HashMap::new())
                .await
                .map_err(|err| err.to_string())?;
        }
        let results = store
            .search(&ns, &v, 1)
            .await
            .map_err(|err| err.to_string())?;
        contract_assert!(!results.is_empty(), "expected at least 1 result");
        contract_assert_eq!(results[0].id, "X".to_string(), "expected self to be top-1");
        Ok(())
    });

    contract_test!(&report, "delete_then_search_excludes", async {
        let ns = format!("{ns}-del");
        let v = unit_vector(7.0);
        store
            .upsert(&ns, "removeme", &v, None, HashMap::new())
            .await
            .map_err(|err| err.to_string())?;
        store
            .delete(&ns, "removeme")
            .await
            .map_err(|err| err.to_string())?;
        let results = store
            .search(&ns, &v, 5)
            .await
            .map_err(|err| err.to_string())?;
        contract_assert!(
            !results.iter().any(|m| m.id == "removeme"),
            "deleted id still in search results"
        );
        Ok(())
    });

    contract_test!(&report, "namespaces_isolated", async {
        let ns_a = format!("{ns}-isoA");
        let ns_b = format!("{ns}-isoB");
        let v = unit_vector(13.0);
        store
            .upsert(&ns_a, "k", &v, None, HashMap::new())
            .await
            .map_err(|err| err.to_string())?;
        let results = store
            .search(&ns_b, &v, 5)
            .await
            .map_err(|err| err.to_string())?;
        contract_assert!(results.is_empty(), "vector visible across namespaces");
        Ok(())
    });

    contract_test!(&report, "match_metadata_preserved", async {
        let ns = format!("{ns}-meta");
        let v = unit_vector(99.0);
        let mut meta = HashMap::new();
        meta.insert("kind".to_string(), json!("test"));
        meta.insert("score".to_string(), json!(42));
        store
            .upsert(&ns, "withmeta", &v, Some("text!"), meta.clone())
            .await
            .map_err(|err| err.to_string())?;
        let results = store
            .search(&ns, &v, 1)
            .await
            .map_err(|err| err.to_string())?;
        let m = results.first().ok_or("no results returned")?;
        contract_assert_eq!(
            m.metadata.get("kind").cloned(),
            Some(json!("test")),
            "kind preserved"
        );
        contract_assert_eq!(m.text.as_deref(), Some("text!"), "text preserved");
        Ok(())
    });

    report
}
