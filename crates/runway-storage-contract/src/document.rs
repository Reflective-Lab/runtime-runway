//! DocumentStore contract suite.

use std::sync::Arc;

use chrono::Utc;
use runway_storage::{Document, DocumentStore, Filter, Order, Query};
use serde_json::json;

use crate::harness::{ContractContext, SuiteReport};
use crate::{contract_assert, contract_assert_eq, contract_test};

pub async fn run_document_suite(
    store: Arc<dyn DocumentStore>,
    ctx: ContractContext,
) -> SuiteReport {
    let report = SuiteReport::new(&ctx.backend, "DocumentStore");
    let coll = ctx.scope("docs");

    contract_test!(&report, "put_then_get", async {
        let id = "put-get-1";
        let doc = Document::new(id, json!({"name": "alice", "n": 1})).map_err(|e| e.to_string())?;
        store
            .put(&coll, doc.clone())
            .await
            .map_err(|e| e.to_string())?;
        let got = store.get(&coll, id).await.map_err(|e| e.to_string())?;
        let got = got.ok_or("expected Some(doc), got None")?;
        contract_assert_eq!(got.id, doc.id, "id roundtrip");
        contract_assert!(
            got.data.get("name").and_then(|v| v.as_str()) == Some("alice"),
            "name field roundtrip"
        );
        Ok(())
    });

    contract_test!(&report, "get_missing_returns_none", async {
        let got = store
            .get(&coll, "definitely-missing")
            .await
            .map_err(|e| e.to_string())?;
        contract_assert!(got.is_none(), "expected None for missing id, got {:?}", got);
        Ok(())
    });

    contract_test!(&report, "delete_then_get", async {
        let id = "del-1";
        let doc = Document::new(id, json!({"x": 1})).map_err(|e| e.to_string())?;
        store.put(&coll, doc).await.map_err(|e| e.to_string())?;
        store.delete(&coll, id).await.map_err(|e| e.to_string())?;
        let got = store.get(&coll, id).await.map_err(|e| e.to_string())?;
        contract_assert!(got.is_none(), "expected None after delete");
        Ok(())
    });

    contract_test!(&report, "delete_idempotent", async {
        store
            .delete(&coll, "never-existed")
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    });

    contract_test!(&report, "put_overwrites_with_advancing_updated_at", async {
        let id = "overwrite-1";
        let doc = Document::new(id, json!({"v": 1})).map_err(|e| e.to_string())?;
        store.put(&coll, doc).await.map_err(|e| e.to_string())?;
        let v1 = store
            .get(&coll, id)
            .await
            .map_err(|e| e.to_string())?
            .ok_or("expected v1 present")?;

        tokio::time::sleep(std::time::Duration::from_millis(5)).await;

        let doc2 = Document::new(id, json!({"v": 2})).map_err(|e| e.to_string())?;
        store.put(&coll, doc2).await.map_err(|e| e.to_string())?;
        let v2 = store
            .get(&coll, id)
            .await
            .map_err(|e| e.to_string())?
            .ok_or("expected v2 present")?;

        contract_assert_eq!(v2.created_at, v1.created_at, "created_at preserved");
        contract_assert!(
            v2.updated_at > v1.updated_at,
            "updated_at advanced ({:?} -> {:?})",
            v1.updated_at,
            v2.updated_at
        );
        Ok(())
    });

    contract_test!(&report, "collections_isolated", async {
        let coll_a = format!("{coll}-a");
        let coll_b = format!("{coll}-b");
        let doc = Document::new("k", json!({"a": true})).map_err(|e| e.to_string())?;
        store.put(&coll_a, doc).await.map_err(|e| e.to_string())?;
        let got = store.get(&coll_b, "k").await.map_err(|e| e.to_string())?;
        contract_assert!(got.is_none(), "key visible across collections");
        Ok(())
    });

    contract_test!(&report, "query_no_filter_returns_all", async {
        let c = format!("{coll}-allq");
        for i in 0..3 {
            let doc = Document::new(format!("k{i}"), json!({"n": i})).map_err(|e| e.to_string())?;
            store.put(&c, doc).await.map_err(|e| e.to_string())?;
        }
        let docs = store
            .query(&c, Query::new())
            .await
            .map_err(|e| e.to_string())?;
        contract_assert_eq!(docs.len(), 3, "expected 3 docs from no-filter query");
        Ok(())
    });

    contract_test!(&report, "query_eq_filter", async {
        let c = format!("{coll}-eqq");
        for (i, status) in [("1", "active"), ("2", "inactive"), ("3", "active")] {
            let doc = Document::new(i, json!({"status": status})).map_err(|e| e.to_string())?;
            store.put(&c, doc).await.map_err(|e| e.to_string())?;
        }
        let docs = store
            .query(
                &c,
                Query::new().filter(Filter::Eq("status".into(), json!("active"))),
            )
            .await
            .map_err(|e| e.to_string())?;
        contract_assert_eq!(docs.len(), 2, "expected 2 active docs");
        Ok(())
    });

    contract_test!(&report, "query_range_filters", async {
        let c = format!("{coll}-rangeq");
        for n in 0..5i64 {
            let doc = Document::new(n.to_string(), json!({"n": n})).map_err(|e| e.to_string())?;
            store.put(&c, doc).await.map_err(|e| e.to_string())?;
        }
        let docs = store
            .query(&c, Query::new().filter(Filter::Gte("n".into(), json!(2))))
            .await
            .map_err(|e| e.to_string())?;
        contract_assert_eq!(docs.len(), 3, "expected 3 docs with n >= 2");
        Ok(())
    });

    contract_test!(&report, "query_and_composition", async {
        let c = format!("{coll}-andq");
        for (id, status, tier) in [("a", "active", 1), ("b", "active", 2), ("c", "inactive", 1)] {
            let doc = Document::new(id, json!({"status": status, "tier": tier}))
                .map_err(|e| e.to_string())?;
            store.put(&c, doc).await.map_err(|e| e.to_string())?;
        }
        let docs = store
            .query(
                &c,
                Query::new().filter(Filter::And(vec![
                    Filter::Eq("status".into(), json!("active")),
                    Filter::Eq("tier".into(), json!(1)),
                ])),
            )
            .await
            .map_err(|e| e.to_string())?;
        contract_assert_eq!(docs.len(), 1, "expected exactly 1 doc (active AND tier=1)");
        Ok(())
    });

    contract_test!(&report, "query_or_composition_single_field", async {
        let c = format!("{coll}-orq");
        for (id, status) in [("a", "active"), ("b", "paused"), ("c", "inactive")] {
            let doc = Document::new(id, json!({"status": status})).map_err(|e| e.to_string())?;
            store.put(&c, doc).await.map_err(|e| e.to_string())?;
        }
        // Single-field OR only — multi-field OR requires Firestore composite indexes
        // and is outside the contract surface.
        let docs = store
            .query(
                &c,
                Query::new().filter(Filter::Or(vec![
                    Filter::Eq("status".into(), json!("active")),
                    Filter::Eq("status".into(), json!("paused")),
                ])),
            )
            .await
            .map_err(|e| e.to_string())?;
        contract_assert_eq!(docs.len(), 2, "expected 2 docs (active OR paused)");
        Ok(())
    });

    contract_test!(&report, "query_order_by_then_limit", async {
        let c = format!("{coll}-orderq");
        for n in [3i64, 1, 4, 1, 5] {
            let doc = Document::new(format!("k{n}-{}", uuid::Uuid::new_v4()), json!({"n": n}))
                .map_err(|e| e.to_string())?;
            store.put(&c, doc).await.map_err(|e| e.to_string())?;
        }
        let docs = store
            .query(&c, Query::new().order("n", Order::Asc).limit(2))
            .await
            .map_err(|e| e.to_string())?;
        contract_assert_eq!(docs.len(), 2, "limit truncated to 2");
        let ns: Vec<i64> = docs
            .iter()
            .filter_map(|d| d.data.get("n").and_then(|v| v.as_i64()))
            .collect();
        contract_assert!(
            ns.windows(2).all(|w| w[0] <= w[1]),
            "ordered ascending: {:?}",
            ns
        );
        Ok(())
    });

    contract_test!(&report, "query_updated_after", async {
        let c = format!("{coll}-tsq");
        let doc1 = Document::new("a", json!({"k": 1})).map_err(|e| e.to_string())?;
        store.put(&c, doc1).await.map_err(|e| e.to_string())?;
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        let cutoff = Utc::now();
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        let doc2 = Document::new("b", json!({"k": 2})).map_err(|e| e.to_string())?;
        store.put(&c, doc2).await.map_err(|e| e.to_string())?;

        let docs = store
            .query(&c, Query::new().updated_after(cutoff))
            .await
            .map_err(|e| e.to_string())?;
        contract_assert_eq!(docs.len(), 1, "expected 1 doc updated after cutoff");
        contract_assert_eq!(docs[0].id, "b".to_string(), "wrong doc id");
        Ok(())
    });

    report
}
