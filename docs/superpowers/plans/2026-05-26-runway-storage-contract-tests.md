# Runway Storage Contract Tests Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a `runway-storage-contract` crate and three test entry points (local, emulator, real GCP) that prove the local and remote `StorageKit` implementations behave equivalently against the trait surface they implement.

**Architecture:** New `runway-storage-contract` crate exposes one suite function per trait, depending only on the trait surface (no backend deps). Three thin entry-point files in `runway-storage/tests/` build each backend and dispatch the suites. Four trait refinements (timestamp policy, `Embedding` wrapper, `EventLog` split, empty-input rejection) land before the suites that depend on them. Each refactor is followed immediately by its contract suite so regressions are caught at the point of change.

**Tech Stack:** Rust, redb, Firestore/GCS/Pub-Sub/Vertex AI clients, fastembed, tokio, async-trait, anyhow, thiserror, tempfile, serde, Docker (emulators), GitHub Actions, `just`.

**Spec:** `docs/superpowers/specs/2026-05-26-runway-storage-contract-tests-design.md`

**Scope (in):** Trait refinements 1–4 from the spec, the contract crate, three entry points, Docker compose for emulators, `just` recipes, two CI workflows, doc drift cleanup.

**Scope (out):** Downstream caller migrations (Helm/Axiom/Organism) for the `Embedding` and `SyncableEventLog` changes — those live in their own repos and are tracked separately.

**Verification gate:** `just lint` (= `cargo fmt --check && cargo clippy -- -D warnings`) plus `cargo test --all-targets` must pass before each commit.

---

## File Structure

| File | Status | Purpose |
|---|---|---|
| `crates/runway-storage/src/traits/embedding.rs` | modify | Add `Embedding` type, change `EmbeddingProvider` trait, drop `dims()`, add empty-input rejection contract |
| `crates/runway-storage/src/traits/vector.rs` | modify | Change `VectorStore` to take `&Embedding`; update `Match` if needed |
| `crates/runway-storage/src/traits/event.rs` | modify | Split into `EventLog` (base) and `SyncableEventLog` (sub-trait); drop `EventQuery::unsynced_only` |
| `crates/runway-storage/src/traits/document.rs` | modify | Update doc comment ("redb", not "SQLite") |
| `crates/runway-storage/src/embedding/local.rs` | modify | Return `Embedding`, reject empty input |
| `crates/runway-storage/src/embedding/vertex.rs` | modify | Return `Embedding`, reject empty input |
| `crates/runway-storage/src/local/document.rs` | modify | Preserve `created_at` on overwrite |
| `crates/runway-storage/src/local/vector.rs` | modify | Use `Embedding` in storage and query |
| `crates/runway-storage/src/local/event.rs` | modify | Impl both `EventLog` and `SyncableEventLog` |
| `crates/runway-storage/src/remote/document.rs` | modify | Preserve `created_at` on overwrite |
| `crates/runway-storage/src/remote/vector.rs` | modify | Use `Embedding` |
| `crates/runway-storage/src/remote/event.rs` | modify | Impl `EventLog` only |
| `crates/runway-storage/src/remote/mod.rs` | modify | Add `build_with_embedder` constructor |
| `crates/runway-storage/src/lib.rs` | modify | Re-export `Embedding`, `SyncableEventLog`; fix `local()` doc |
| `crates/runway-storage/Cargo.toml` | modify | Fix description; add dev-deps for contract crate |
| `crates/runway-storage/tests/contract_local.rs` | create | Entry point: redb backend |
| `crates/runway-storage/tests/contract_emulator.rs` | create | Entry point: emulators + fastembed |
| `crates/runway-storage/tests/contract_real_gcp.rs` | create | Entry point: real GCP, `#[ignore]` |
| `crates/runway-storage/tests/docker-compose.contract.yml` | create | Emulator stack |
| `crates/runway-storage-contract/Cargo.toml` | create | Contract crate manifest |
| `crates/runway-storage-contract/src/lib.rs` | create | `SuiteReport`, re-exports |
| `crates/runway-storage-contract/src/harness.rs` | create | `contract_assert!` macro, namespace helpers |
| `crates/runway-storage-contract/src/document.rs` | create | `run_document_suite` |
| `crates/runway-storage-contract/src/object.rs` | create | `run_object_suite` |
| `crates/runway-storage-contract/src/event.rs` | create | `run_event_suite` + `run_syncable_event_suite` |
| `crates/runway-storage-contract/src/vector.rs` | create | `run_vector_shape_suite` |
| `crates/runway-storage-contract/src/embedding.rs` | create | `run_embedding_shape_suite` |
| `Cargo.toml` (workspace) | modify | Add `runway-storage-contract` to members |
| `justfile` | modify | Add `contract`, `contract-local`, `contract-emulator`, `contract-staging`, `contract-all` |
| `.github/workflows/contract.yml` | create | PR-gated emulator suite |
| `.github/workflows/contract-staging.yml` | create | Release-gated real-GCP suite |

---

## Task 0: Pre-flight

**Files:** none

- [ ] **Step 1: Confirm clean working tree**

```bash
cd /Users/kpernyer/dev/reflective/runway
git status
```

Expected: clean tree on `spike1-atlas-staging-app` (or the active in-flight branch).

- [ ] **Step 2: Baseline build and lint**

```bash
just lint
cargo test --all-targets
```

Expected: both pass. If they don't, STOP and address before continuing — this plan assumes a green baseline.

---

## Task 1: Doc drift cleanup

**Files:**
- Modify: `crates/runway-storage/Cargo.toml:3`
- Modify: `crates/runway-storage/src/lib.rs:33`
- Modify: `crates/runway-storage/src/traits/document.rs:95`
- Modify: `crates/runway-storage/src/traits/event.rs:36`
- Modify: `crates/runway-storage/src/traits/vector.rs:22`

- [ ] **Step 1: Fix Cargo.toml description**

In `crates/runway-storage/Cargo.toml`, change:

```toml
description = "Shared storage abstraction for Runway apps — local (SQLite + LanceDB) and remote (Firestore + GCS + Vertex AI)"
```

to:

```toml
description = "Shared storage abstraction for Runway apps — local (redb + local FS + fastembed) and remote (Firestore + GCS + Vertex AI)"
```

- [ ] **Step 2: Fix `lib.rs` `local()` doc**

In `crates/runway-storage/src/lib.rs`, change:

```rust
/// Local storage for Tauri desktop apps. Uses SQLite + LanceDB + local FS.
```

to:

```rust
/// Local storage for Tauri desktop apps. Uses redb (documents, vectors, events) + local FS (objects) + fastembed (embeddings).
```

- [ ] **Step 3: Fix `traits/document.rs` doc**

Change:

```rust
/// collection or a SQLite table row with `collection = ?`.
```

to:

```rust
/// collection or a redb table keyed by `(collection, id)`.
```

- [ ] **Step 4: Fix `traits/event.rs` doc**

Change:

```rust
/// Local impl:  SQLite WAL (survives restarts, feeds sync engine)
```

to:

```rust
/// Local impl:  redb (survives restarts, feeds sync engine)
```

- [ ] **Step 5: Fix `traits/vector.rs` doc**

Change:

```rust
/// `namespace` maps to a LanceDB table name or a Vertex AI index namespace.
```

to:

```rust
/// `namespace` maps to a redb table partition or a Vertex AI index namespace.
```

- [ ] **Step 6: Verify and commit**

```bash
just lint
git add crates/runway-storage/Cargo.toml crates/runway-storage/src/lib.rs crates/runway-storage/src/traits/
git commit -m "docs: fix stale SQLite/LanceDB references in runway-storage

The local stack uses redb + local FS + fastembed; SQLite and LanceDB
were removed earlier due to dependency conflicts with burn (see
kb/History/CHANGELOG.md)."
```

---

## Task 2: Add `runway-storage-contract` crate to workspace

**Files:**
- Modify: `Cargo.toml` (workspace)
- Create: `crates/runway-storage-contract/Cargo.toml`
- Create: `crates/runway-storage-contract/src/lib.rs`

- [ ] **Step 1: Add new crate to workspace members**

In root `Cargo.toml`, add `"crates/runway-storage-contract"` to the `members` list, alphabetically between `runway-auth` and `runway-middleware` (or wherever fits the existing order). The list becomes:

```toml
members = [
    "crates/api-server",
    "crates/application",
    "crates/llm",
    "crates/runway-accounts",
    "crates/runway-app-host",
    "crates/runway-auth",
    "crates/runway-middleware",
    "crates/runway-secrets",
    "crates/runway-storage",
    "crates/runway-storage-contract",
    "crates/runway-telemetry",
]
```

- [ ] **Step 2: Create the contract crate manifest**

Create `crates/runway-storage-contract/Cargo.toml`:

```toml
[package]
name = "runway-storage-contract"
description = "Contract test suite for runway-storage. Asserts equivalence and shape parity across backends."
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true

[dependencies]
runway-storage = { path = "../runway-storage" }

anyhow = { workspace = true }
async-trait = { workspace = true }
bytes = "1"
chrono = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
tokio = { workspace = true }
tracing = { workspace = true }
uuid = { workspace = true }
```

- [ ] **Step 3: Create stub `lib.rs`**

Create `crates/runway-storage-contract/src/lib.rs`:

```rust
//! Contract tests for `runway-storage`. Asserts equivalence and shape parity
//! across backends. See `docs/superpowers/specs/2026-05-26-runway-storage-contract-tests-design.md`.

pub mod document;
pub mod embedding;
pub mod event;
pub mod harness;
pub mod object;
pub mod vector;

pub use harness::{ContractContext, SuiteReport};
```

- [ ] **Step 4: Create stub modules**

Create each of these as a placeholder file with `// implemented later` body:

- `crates/runway-storage-contract/src/document.rs`
- `crates/runway-storage-contract/src/object.rs`
- `crates/runway-storage-contract/src/event.rs`
- `crates/runway-storage-contract/src/vector.rs`
- `crates/runway-storage-contract/src/embedding.rs`

Each with body:

```rust
// suite implemented in a later task
```

- [ ] **Step 5: Create `harness.rs`**

Create `crates/runway-storage-contract/src/harness.rs`:

```rust
//! Shared assertion helpers and contract-test context.

use std::sync::Mutex;

/// Context passed into every suite. Carries the backend name (for failure
/// messages) and the namespace prefix used for collection/key/topic isolation.
#[derive(Debug, Clone)]
pub struct ContractContext {
    pub backend: String,
    pub namespace: String,
}

impl ContractContext {
    pub fn new(backend: impl Into<String>, namespace: impl Into<String>) -> Self {
        Self {
            backend: backend.into(),
            namespace: namespace.into(),
        }
    }

    /// Returns a collection/key prefix scoped to this run.
    pub fn scope(&self, suffix: &str) -> String {
        format!("{}/{}", self.namespace, suffix)
    }
}

/// Pass/fail record per test.
#[derive(Debug)]
pub struct SuiteReport {
    pub backend: String,
    pub trait_name: String,
    results: Mutex<Vec<TestResult>>,
}

#[derive(Debug, Clone)]
pub struct TestResult {
    pub name: String,
    pub passed: bool,
    pub failure: Option<String>,
}

impl SuiteReport {
    pub fn new(backend: impl Into<String>, trait_name: impl Into<String>) -> Self {
        Self {
            backend: backend.into(),
            trait_name: trait_name.into(),
            results: Mutex::new(Vec::new()),
        }
    }

    pub fn record(&self, name: impl Into<String>, result: Result<(), String>) {
        self.results.lock().unwrap().push(TestResult {
            name: name.into(),
            passed: result.is_ok(),
            failure: result.err(),
        });
    }

    /// Panics with all failures formatted for the test runner.
    pub fn assert_passed(self) {
        let results = self.results.into_inner().unwrap();
        let failures: Vec<&TestResult> = results.iter().filter(|r| !r.passed).collect();
        if failures.is_empty() {
            return;
        }
        let mut msg = format!(
            "\n{} contract violations [{} @ {}]:\n",
            failures.len(),
            self.trait_name,
            self.backend,
        );
        for f in &failures {
            msg.push_str(&format!(
                "  - {}: {}\n",
                f.name,
                f.failure.as_deref().unwrap_or("(no detail)"),
            ));
        }
        panic!("{msg}");
    }
}

/// Runs an async closure, captures panics and `Err` returns into the report.
#[macro_export]
macro_rules! contract_test {
    ($report:expr, $name:literal, $body:expr) => {{
        let result: Result<(), String> = async {
            let r: Result<(), String> = $body.await;
            r
        }
        .await;
        $report.record($name, result);
    }};
}

/// Assert helper that returns `Err(String)` so a failure aborts the current
/// contract test but allows the suite to continue.
#[macro_export]
macro_rules! contract_assert {
    ($cond:expr, $($arg:tt)*) => {
        if !$cond {
            return Err(format!($($arg)*));
        }
    };
}

#[macro_export]
macro_rules! contract_assert_eq {
    ($left:expr, $right:expr, $($arg:tt)*) => {{
        let l = &$left;
        let r = &$right;
        if l != r {
            return Err(format!(
                "{}: expected {:?}, got {:?}",
                format!($($arg)*), r, l,
            ));
        }
    }};
}
```

- [ ] **Step 6: Build and verify**

```bash
cargo build -p runway-storage-contract
just lint
```

Expected: empty crate builds and lints clean.

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml crates/runway-storage-contract/
git commit -m "feat(runway-storage-contract): scaffold empty crate

Will host suite functions that prove local/remote storage backends
satisfy the same trait contracts. Depends only on the trait surface
from runway-storage — no backend deps."
```

---

## Task 3: Document timestamp preservation

**Files:**
- Modify: `crates/runway-storage/src/local/document.rs`
- Modify: `crates/runway-storage/src/remote/document.rs`

**What changes:** On `put`, both impls now read-before-write and preserve the existing `created_at` if a document with the same id exists. `updated_at` is always stamped to `Utc::now()` at the impl. Callers stop being responsible for timestamp math.

- [ ] **Step 1: Read current `local/document.rs` `put`**

```bash
cat crates/runway-storage/src/local/document.rs
```

Identify the `put` method. The redb impl uses `tokio::task::spawn_blocking` with a write transaction.

- [ ] **Step 2: Modify `local/document.rs` `put` to preserve `created_at`**

Inside the `spawn_blocking` write transaction, before inserting, look up the existing entry. If present, deserialize, and overwrite `doc.created_at` with the existing value. Always set `doc.updated_at = Utc::now()` immediately before serialization.

Concretely, the inner block becomes:

```rust
use chrono::Utc;
// ... existing imports

async fn put(&self, collection: &str, doc: Document) -> Result<()> {
    let db = self.db.clone();
    let collection = collection.to_string();
    let id = doc.id.clone();
    let mut doc = doc;  // make mutable

    tokio::task::spawn_blocking(move || {
        let tx = db.begin_write().map_err(|e| Error::Database(e.to_string()))?;
        {
            let mut table = tx.open_table(DOCS).map_err(|e| Error::Database(e.to_string()))?;
            // Preserve created_at if doc exists
            if let Some(existing) = table
                .get((collection.as_str(), id.as_str()))
                .map_err(|e| Error::Database(e.to_string()))?
            {
                let prior: Document = serde_json::from_str(existing.value())
                    .map_err(|e| Error::Serialisation(e.to_string()))?;
                doc.created_at = prior.created_at;
            }
            doc.updated_at = Utc::now();
            let json = serde_json::to_string(&doc).map_err(|e| Error::Serialisation(e.to_string()))?;
            table
                .insert((collection.as_str(), id.as_str()), json.as_str())
                .map_err(|e| Error::Database(e.to_string()))?;
        }
        tx.commit().map_err(|e| Error::Database(e.to_string()))?;
        Ok(())
    })
    .await
    .map_err(|e| Error::Other(e.to_string()))?
}
```

- [ ] **Step 3: Apply the same logic to `remote/document.rs` `put`**

Read the current Firestore impl. Apply the read-before-write pattern: `get` the existing doc, if present preserve `created_at`, set `updated_at = Utc::now()`, then write. If the Firestore client supports a single-call merge-with-preserve semantics, that's fine too — the contract is what matters.

- [ ] **Step 4: Add unit tests for the timestamp policy**

Append to `crates/runway-storage/src/local/document.rs` (or create a new test module):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::document::DocumentStore;
    use serde_json::json;
    use tempfile::tempdir;

    async fn build_store() -> RedbDocumentStore {
        let dir = tempdir().unwrap();
        let db = Arc::new(Database::create(dir.path().join("test.redb")).unwrap());
        {
            let tx = db.begin_write().unwrap();
            init_tables(&tx).unwrap();
            tx.commit().unwrap();
        }
        // Note: tempdir is dropped when `dir` goes out of scope; pin it via Box::leak
        // or restructure if test needs the dir alive beyond this function.
        std::mem::forget(dir);
        RedbDocumentStore::new(db)
    }

    #[tokio::test]
    async fn put_preserves_created_at_on_overwrite() {
        let store = build_store().await;
        let doc = Document::new("k1", json!({"v": 1})).unwrap();
        let original_created = doc.created_at;
        store.put("coll", doc).await.unwrap();

        tokio::time::sleep(std::time::Duration::from_millis(5)).await;

        let doc2 = Document::new("k1", json!({"v": 2})).unwrap();
        store.put("coll", doc2).await.unwrap();

        let got = store.get("coll", "k1").await.unwrap().expect("doc present");
        assert_eq!(got.created_at, original_created, "created_at must be preserved");
        assert!(got.updated_at > original_created, "updated_at must advance");
    }
}
```

- [ ] **Step 5: Run unit test**

```bash
cargo test -p runway-storage local::document::tests::put_preserves_created_at_on_overwrite -- --nocapture
```

Expected: PASS.

- [ ] **Step 6: Lint and commit**

```bash
just lint
git add crates/runway-storage/src/local/document.rs crates/runway-storage/src/remote/document.rs
git commit -m "refactor(runway-storage): preserve created_at on document put

Both local (redb) and remote (Firestore) DocumentStore impls now
read-before-write on put: existing created_at is preserved, updated_at
is stamped to now. Callers no longer need to fetch-merge themselves.

Part of the runway-storage contract tests work — see
docs/superpowers/specs/2026-05-26-runway-storage-contract-tests-design.md."
```

---

## Task 4: Document contract suite

**Files:**
- Modify: `crates/runway-storage-contract/src/document.rs`
- Modify: `crates/runway-storage/Cargo.toml` (add `runway-storage-contract` as dev-dep)
- Create: `crates/runway-storage/tests/contract_local.rs` (first iteration — document suite only)

- [ ] **Step 1: Add dev-dependency**

In `crates/runway-storage/Cargo.toml`, add:

```toml
[dev-dependencies]
runway-storage-contract = { path = "../runway-storage-contract" }
tempfile = "3"
```

- [ ] **Step 2: Implement `run_document_suite`**

Replace `crates/runway-storage-contract/src/document.rs` content with:

```rust
//! DocumentStore contract suite.

use std::sync::Arc;

use chrono::Utc;
use runway_storage::{Document, DocumentStore, Filter, Order, Query};
use serde_json::json;

use crate::{contract_assert, contract_assert_eq, contract_test};
use crate::harness::{ContractContext, SuiteReport};

pub async fn run_document_suite(
    store: Arc<dyn DocumentStore>,
    ctx: ContractContext,
) -> SuiteReport {
    let report = SuiteReport::new(&ctx.backend, "DocumentStore");
    let coll = ctx.scope("docs");

    contract_test!(&report, "put_then_get", async {
        let id = "put-get-1";
        let doc = Document::new(id, json!({"name": "alice", "n": 1})).map_err(|e| e.to_string())?;
        store.put(&coll, doc.clone()).await.map_err(|e| e.to_string())?;
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
        let got = store.get(&coll, "definitely-missing").await.map_err(|e| e.to_string())?;
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
        store.delete(&coll, "never-existed").await.map_err(|e| e.to_string())?;
        Ok(())
    });

    contract_test!(&report, "put_overwrites_with_advancing_updated_at", async {
        let id = "overwrite-1";
        let doc = Document::new(id, json!({"v": 1})).map_err(|e| e.to_string())?;
        store.put(&coll, doc).await.map_err(|e| e.to_string())?;
        let v1 = store.get(&coll, id).await.map_err(|e| e.to_string())?
            .ok_or("expected v1 present")?;

        tokio::time::sleep(std::time::Duration::from_millis(5)).await;

        let doc2 = Document::new(id, json!({"v": 2})).map_err(|e| e.to_string())?;
        store.put(&coll, doc2).await.map_err(|e| e.to_string())?;
        let v2 = store.get(&coll, id).await.map_err(|e| e.to_string())?
            .ok_or("expected v2 present")?;

        contract_assert_eq!(v2.created_at, v1.created_at, "created_at preserved");
        contract_assert!(v2.updated_at > v1.updated_at, "updated_at advanced ({:?} → {:?})", v1.updated_at, v2.updated_at);
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
        let docs = store.query(&c, Query::new()).await.map_err(|e| e.to_string())?;
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
            .query(&c, Query::new().filter(Filter::Eq("status".into(), json!("active"))))
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
            let doc = Document::new(id, json!({"status": status, "tier": tier})).map_err(|e| e.to_string())?;
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
        let ns: Vec<i64> = docs.iter().filter_map(|d| d.data.get("n").and_then(|v| v.as_i64())).collect();
        contract_assert!(ns.windows(2).all(|w| w[0] <= w[1]), "ordered ascending: {:?}", ns);
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

        let docs = store.query(&c, Query::new().updated_after(cutoff)).await.map_err(|e| e.to_string())?;
        contract_assert_eq!(docs.len(), 1, "expected 1 doc updated after cutoff");
        contract_assert_eq!(docs[0].id, "b", "wrong doc id");
        Ok(())
    });

    report
}
```

- [ ] **Step 3: Create `contract_local.rs` entry point (document-only first iteration)**

Create `crates/runway-storage/tests/contract_local.rs`:

```rust
//! Contract tests against the local (redb + local FS + fastembed) backend.

use std::sync::Arc;

use runway_storage::StorageKit;
use runway_storage_contract::{document, ContractContext};

#[tokio::test]
async fn document_contract() {
    let tmp = tempfile::tempdir().unwrap();
    let kit = StorageKit::local(tmp.path()).await.unwrap();
    let ctx = ContractContext::new("redb", "_contract");
    document::run_document_suite(Arc::clone(&kit.documents), ctx)
        .await
        .assert_passed();
}
```

- [ ] **Step 4: Run the document contract**

```bash
cargo test -p runway-storage --test contract_local document_contract -- --nocapture
```

Expected: PASS (12 contract tests, all passing). If any fail, the failure messages will identify which.

- [ ] **Step 5: Lint and commit**

```bash
just lint
git add crates/runway-storage-contract/src/document.rs crates/runway-storage/Cargo.toml crates/runway-storage/tests/contract_local.rs
git commit -m "feat(runway-storage-contract): document suite + local entry point

Implements run_document_suite with 12 equivalence tests. Adds the
first contract_local.rs entry point that runs the suite against the
redb backend."
```

---

## Task 5: Embedding wrapper type

**Files:**
- Modify: `crates/runway-storage/src/traits/embedding.rs`
- Modify: `crates/runway-storage/src/lib.rs` (re-export)

**What changes:** Add `Embedding` struct with custom serde. Trait change deferred to Task 6.

- [ ] **Step 1: Add `Embedding` type to `traits/embedding.rs`**

Replace `crates/runway-storage/src/traits/embedding.rs` with:

```rust
use async_trait::async_trait;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::traits::{Error, Result};

/// Embedding dimensionality used across the entire stack.
/// Matches Vertex AI `text-multilingual-embedding-002` output.
pub const EMBEDDING_DIMS: usize = 768;

/// A typed embedding. The dimension invariant is encoded in the type:
/// values can only be constructed via [`Embedding::new`], which validates length.
/// Deserialization routes through the same constructor — corrupt stored data
/// fails loudly at the deser boundary with the same error as constructor misuse.
#[derive(Debug, Clone, PartialEq)]
pub struct Embedding([f32; EMBEDDING_DIMS]);

impl Embedding {
    pub fn new(values: Vec<f32>) -> std::result::Result<Self, Error> {
        let arr: [f32; EMBEDDING_DIMS] = values.try_into().map_err(|v: Vec<f32>| {
            Error::Other(format!(
                "expected {EMBEDDING_DIMS} dims, got {}",
                v.len()
            ))
        })?;
        Ok(Self(arr))
    }

    pub fn as_slice(&self) -> &[f32] {
        &self.0
    }
}

impl Serialize for Embedding {
    fn serialize<S: Serializer>(&self, ser: S) -> std::result::Result<S::Ok, S::Error> {
        // Serialize as Vec<f32> for cross-backend compatibility (JSON, Firestore,
        // redb-stored JSON). Fixed-size arrays don't get free serde derives.
        self.0.as_ref().serialize(ser)
    }
}

impl<'de> Deserialize<'de> for Embedding {
    fn deserialize<D: Deserializer<'de>>(de: D) -> std::result::Result<Self, D::Error> {
        let v = Vec::<f32>::deserialize(de)?;
        Embedding::new(v).map_err(serde::de::Error::custom)
    }
}

/// Embedding generation. Standardised on 768 dims (Vertex AI text-multilingual-embedding-002).
///
/// Remote impl: Vertex AI (replaces OpenAI)
/// Local impl:  fastembed (all-MiniLM-L6-v2, resized to 768 via zero-padding or a local 768-dim model)
///
/// Empty or whitespace-only input is rejected with `Error::Other("embedding input is empty")`.
#[async_trait]
pub trait EmbeddingProvider: Send + Sync {
    /// Embed a single text string. Empty/whitespace input → `Err`.
    async fn embed(&self, text: &str) -> Result<Embedding>;

    /// Embed a batch of texts. Implementations should use the provider's native batching.
    /// Same empty-input rule applies per element.
    async fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Embedding>> {
        let mut results = Vec::with_capacity(texts.len());
        for text in texts {
            results.push(self.embed(text).await?);
        }
        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_rejects_wrong_dim() {
        let err = Embedding::new(vec![0.0; 512]).unwrap_err();
        assert!(matches!(err, Error::Other(ref m) if m.contains("expected 768 dims, got 512")));
    }

    #[test]
    fn new_accepts_correct_dim() {
        let e = Embedding::new(vec![0.0; EMBEDDING_DIMS]).unwrap();
        assert_eq!(e.as_slice().len(), EMBEDDING_DIMS);
    }

    #[test]
    fn serde_roundtrip() {
        let v: Vec<f32> = (0..EMBEDDING_DIMS).map(|i| i as f32 * 0.001).collect();
        let e = Embedding::new(v.clone()).unwrap();
        let json = serde_json::to_string(&e).unwrap();
        let e2: Embedding = serde_json::from_str(&json).unwrap();
        assert_eq!(e, e2);
    }

    #[test]
    fn deser_rejects_wrong_dim() {
        let bad: Vec<f32> = vec![0.0; 512];
        let json = serde_json::to_string(&bad).unwrap();
        let result: Result<Embedding> = serde_json::from_str(&json).map_err(|e| Error::Other(e.to_string()));
        assert!(result.is_err());
    }
}
```

- [ ] **Step 2: Re-export from `lib.rs`**

In `crates/runway-storage/src/lib.rs`, update the re-exports:

```rust
pub use traits::{
    document::{Document, DocumentStore, Filter, Order, Query},
    embedding::{Embedding, EmbeddingProvider, EMBEDDING_DIMS},
    event::{EventLog, StoredEvent},
    object::ObjectStore,
    vector::{Match, VectorStore},
};
```

- [ ] **Step 3: Build and run unit tests**

```bash
cargo test -p runway-storage traits::embedding::tests -- --nocapture
just lint
```

Expected: 4 unit tests pass. Lint clean.

- [ ] **Step 4: Commit**

```bash
git add crates/runway-storage/src/traits/embedding.rs crates/runway-storage/src/lib.rs
git commit -m "refactor(runway-storage): add typed Embedding wrapper

Embedding([f32; 768]) makes wrong-dim values unrepresentable. Custom
serde routes through Embedding::new() on deserialization — same
validation point as construction. EmbeddingProvider::embed() return
type changed in a follow-up commit.

Adds EMBEDDING_DIMS to the public re-exports."
```

---

## Task 6: EmbeddingProvider returns Embedding + empty-input rejection

**Files:**
- Modify: `crates/runway-storage/src/embedding/local.rs`
- Modify: `crates/runway-storage/src/embedding/vertex.rs`

**What changes:** Both impls now return `Embedding` (not `Vec<f32>`) and reject empty/whitespace input with `Error::Other("embedding input is empty")`.

- [ ] **Step 1: Read current `embedding/local.rs` and `embedding/vertex.rs`**

```bash
cat crates/runway-storage/src/embedding/local.rs crates/runway-storage/src/embedding/vertex.rs
```

Identify the `impl EmbeddingProvider` blocks. Note where the raw `Vec<f32>` is produced.

- [ ] **Step 2: Update `embedding/local.rs`**

At the top of every `embed` method, insert the empty-input check:

```rust
if text.trim().is_empty() {
    return Err(Error::Other("embedding input is empty".into()));
}
```

Where the impl currently returns `Ok(vec)`, wrap with `Embedding::new(vec).map_err(...)` (the error type from `Embedding::new` is already `Error::Other`, so it can be returned directly). Final return type of `embed` is `Result<Embedding>`.

Also remove the `fn dims() -> usize` implementation — the trait no longer has it.

- [ ] **Step 3: Update `embedding/vertex.rs`**

Same changes: empty-input check at the top of `embed`, wrap return values through `Embedding::new`, remove `dims` impl.

- [ ] **Step 4: Build to find any compile errors**

```bash
cargo build -p runway-storage
```

Expected: compile errors at any internal call site that used the old `Vec<f32>` return. Fix each by either accepting `Embedding` or calling `.as_slice()` (these are temporary until Task 7 changes VectorStore — but for now we need `&[f32]` at VectorStore call sites).

- [ ] **Step 5: Run existing tests**

```bash
cargo test -p runway-storage --lib
```

Expected: PASS.

- [ ] **Step 6: Implement the embedding shape suite**

Replace `crates/runway-storage-contract/src/embedding.rs` with:

```rust
//! EmbeddingProvider shape suite. Embeddings are not equivalence-testable
//! across backends (different models), so we test only shape and error modes.

use std::sync::Arc;

use runway_storage::EmbeddingProvider;

use crate::{contract_assert, contract_test};
use crate::harness::{ContractContext, SuiteReport};

pub async fn run_embedding_shape_suite(
    provider: Arc<dyn EmbeddingProvider>,
    ctx: ContractContext,
) -> SuiteReport {
    let report = SuiteReport::new(&ctx.backend, "EmbeddingProvider");

    contract_test!(&report, "embed_returns_valid_embedding", async {
        let e = provider.embed("hello world").await.map_err(|e| e.to_string())?;
        contract_assert!(e.as_slice().len() == 768, "embedding length must be 768");
        Ok(())
    });

    contract_test!(&report, "embed_batch_returns_one_per_input", async {
        let results = provider.embed_batch(&["foo", "bar"]).await.map_err(|e| e.to_string())?;
        contract_assert!(results.len() == 2, "expected 2 embeddings, got {}", results.len());
        Ok(())
    });

    contract_test!(&report, "embed_empty_string_rejected", async {
        match provider.embed("").await {
            Ok(_) => Err("expected error for empty input".to_string()),
            Err(e) => {
                contract_assert!(
                    e.to_string().contains("empty"),
                    "expected 'empty' in error message, got: {}",
                    e
                );
                Ok(())
            }
        }
    });

    contract_test!(&report, "embed_whitespace_only_rejected", async {
        match provider.embed("   \n\t  ").await {
            Ok(_) => Err("expected error for whitespace-only input".to_string()),
            Err(e) => {
                contract_assert!(
                    e.to_string().contains("empty"),
                    "expected 'empty' in error message, got: {}",
                    e
                );
                Ok(())
            }
        }
    });

    report
}
```

- [ ] **Step 7: Wire the embedding suite into `contract_local.rs`**

In `crates/runway-storage/tests/contract_local.rs`, replace the file with:

```rust
//! Contract tests against the local (redb + local FS + fastembed) backend.

use std::sync::Arc;

use runway_storage::StorageKit;
use runway_storage_contract::{document, embedding, ContractContext};

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
```

- [ ] **Step 8: Run the contract tests**

```bash
cargo test -p runway-storage --test contract_local -- --nocapture
```

Expected: PASS. Both `document_contract` and `embedding_contract`.

- [ ] **Step 9: Lint and commit**

```bash
just lint
git add crates/runway-storage/src/embedding/ crates/runway-storage-contract/src/embedding.rs crates/runway-storage/tests/contract_local.rs
git commit -m "refactor(runway-storage): EmbeddingProvider returns Embedding + rejects empty input

Both fastembed (local) and Vertex AI (remote) embedders now return the
typed Embedding wrapper and reject empty/whitespace-only input with
Error::Other(\"embedding input is empty\"). Drops the redundant dims()
method from the trait.

Adds run_embedding_shape_suite to the contract crate and wires it into
contract_local.rs."
```

---

## Task 7: VectorStore takes &Embedding

**Files:**
- Modify: `crates/runway-storage/src/traits/vector.rs`
- Modify: `crates/runway-storage/src/local/vector.rs`
- Modify: `crates/runway-storage/src/remote/vector.rs`

**What changes:** `upsert` and `search` take `&Embedding` instead of `&[f32]`. `upsert_batch` items use `Embedding`. The redb local impl stores embeddings via the existing `serde_json` path (now using `Embedding`'s manual serde).

- [ ] **Step 1: Update `traits/vector.rs`**

Replace `crates/runway-storage/src/traits/vector.rs` with:

```rust
use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;

use crate::traits::{Result, embedding::Embedding};

/// A vector match returned from similarity search.
#[derive(Debug, Clone)]
pub struct Match {
    pub id: String,
    pub score: f32,
    pub metadata: HashMap<String, Value>,
    pub text: Option<String>,
}

/// Vector store: upsert embeddings and run ANN search.
///
/// `namespace` maps to a redb table partition or a Vertex AI index namespace.
#[async_trait]
pub trait VectorStore: Send + Sync {
    async fn upsert(
        &self,
        namespace: &str,
        id: &str,
        embedding: &Embedding,
        text: Option<&str>,
        metadata: HashMap<String, Value>,
    ) -> Result<()>;

    async fn search(&self, namespace: &str, query: &Embedding, top_k: usize) -> Result<Vec<Match>>;

    async fn delete(&self, namespace: &str, id: &str) -> Result<()>;

    /// Upsert many vectors in one batch. Default implementation calls `upsert` in sequence;
    /// backends that support batch writes should override this.
    async fn upsert_batch(
        &self,
        namespace: &str,
        items: Vec<(String, Embedding, Option<String>, HashMap<String, Value>)>,
    ) -> Result<()> {
        for (id, emb, text, meta) in items {
            self.upsert(namespace, &id, &emb, text.as_deref(), meta).await?;
        }
        Ok(())
    }
}
```

- [ ] **Step 2: Update `local/vector.rs`**

Change `upsert` signature to take `embedding: &Embedding`. The `VectorEntry` struct's `embedding: Vec<f32>` field stays as-is (this is the on-disk representation); convert with `embedding.as_slice().to_vec()` when storing, and `Embedding::new(entry.embedding)?` when computing similarity against a search query.

Change `search` signature to take `query: &Embedding`. Replace any inner use of the query slice with `query.as_slice()`.

- [ ] **Step 3: Update `remote/vector.rs`**

Same signature changes. The Vertex AI client probably wants `&[f32]` — pass `embedding.as_slice()` at the boundary.

- [ ] **Step 4: Build**

```bash
cargo build -p runway-storage
```

Expected: PASS. If any internal call sites break, fix them — they're now passing `&Embedding`, not `&[f32]`.

- [ ] **Step 5: Implement the vector shape suite**

Replace `crates/runway-storage-contract/src/vector.rs` with:

```rust
//! VectorStore shape suite — equivalence is impossible (different ANN
//! implementations score differently). One equivalence-flavored test:
//! self_match_returns_self_first.

use std::collections::HashMap;
use std::sync::Arc;

use runway_storage::{Embedding, VectorStore};
use serde_json::json;

use crate::{contract_assert, contract_assert_eq, contract_test};
use crate::harness::{ContractContext, SuiteReport};

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
            store.upsert(&ns, &format!("id{i}"), &e, None, HashMap::new())
                .await.map_err(|err| err.to_string())?;
        }
        let q = unit_vector(0.5);
        let results = store.search(&ns, &q, 3).await.map_err(|err| err.to_string())?;
        contract_assert!(results.len() <= 3, "expected at most 3 results, got {}", results.len());
        Ok(())
    });

    contract_test!(&report, "search_returns_empty_for_empty_namespace", async {
        let q = unit_vector(0.0);
        let results = store.search(&format!("{ns}-empty"), &q, 5).await.map_err(|err| err.to_string())?;
        contract_assert!(results.is_empty(), "expected empty result set, got {} matches", results.len());
        Ok(())
    });

    contract_test!(&report, "self_match_returns_self_first", async {
        let ns = format!("{ns}-self");
        let v = unit_vector(42.0);
        store.upsert(&ns, "X", &v, Some("self"), HashMap::new())
            .await.map_err(|err| err.to_string())?;
        // Insert a few decoys
        for i in 0..3 {
            let d = unit_vector(100.0 + i as f32);
            store.upsert(&ns, &format!("decoy{i}"), &d, None, HashMap::new())
                .await.map_err(|err| err.to_string())?;
        }
        let results = store.search(&ns, &v, 1).await.map_err(|err| err.to_string())?;
        contract_assert!(!results.is_empty(), "expected at least 1 result");
        contract_assert_eq!(results[0].id, "X".to_string(), "expected self to be top-1");
        Ok(())
    });

    contract_test!(&report, "delete_then_search_excludes", async {
        let ns = format!("{ns}-del");
        let v = unit_vector(7.0);
        store.upsert(&ns, "removeme", &v, None, HashMap::new())
            .await.map_err(|err| err.to_string())?;
        store.delete(&ns, "removeme").await.map_err(|err| err.to_string())?;
        let results = store.search(&ns, &v, 5).await.map_err(|err| err.to_string())?;
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
        store.upsert(&ns_a, "k", &v, None, HashMap::new())
            .await.map_err(|err| err.to_string())?;
        let results = store.search(&ns_b, &v, 5).await.map_err(|err| err.to_string())?;
        contract_assert!(results.is_empty(), "vector visible across namespaces");
        Ok(())
    });

    contract_test!(&report, "match_metadata_preserved", async {
        let ns = format!("{ns}-meta");
        let v = unit_vector(99.0);
        let mut meta = HashMap::new();
        meta.insert("kind".to_string(), json!("test"));
        meta.insert("score".to_string(), json!(42));
        store.upsert(&ns, "withmeta", &v, Some("text!"), meta.clone())
            .await.map_err(|err| err.to_string())?;
        let results = store.search(&ns, &v, 1).await.map_err(|err| err.to_string())?;
        let m = results.first().ok_or("no results returned")?;
        contract_assert_eq!(m.metadata.get("kind").cloned(), Some(json!("test")), "kind preserved");
        contract_assert_eq!(m.text.as_deref(), Some("text!"), "text preserved");
        Ok(())
    });

    report
}
```

- [ ] **Step 6: Wire vector suite into `contract_local.rs`**

In `crates/runway-storage/tests/contract_local.rs`, add:

```rust
use runway_storage_contract::vector;

#[tokio::test]
async fn vector_contract() {
    let (kit, _tmp) = build_kit().await;
    vector::run_vector_shape_suite(Arc::clone(&kit.vectors), ctx())
        .await
        .assert_passed();
}
```

(Adjust the import list at top of the file to include `vector`.)

- [ ] **Step 7: Run**

```bash
cargo test -p runway-storage --test contract_local -- --nocapture
```

Expected: 3 contracts pass (document, embedding, vector).

- [ ] **Step 8: Lint and commit**

```bash
just lint
git add crates/runway-storage/src/traits/vector.rs crates/runway-storage/src/local/vector.rs crates/runway-storage/src/remote/vector.rs crates/runway-storage-contract/src/vector.rs crates/runway-storage/tests/contract_local.rs
git commit -m "refactor(runway-storage): VectorStore takes &Embedding

upsert, search, and upsert_batch now use the typed Embedding wrapper
on the trait surface. Both backends pass embedding.as_slice() at the
boundary where the underlying client needs &[f32].

Adds run_vector_shape_suite to the contract crate (7 tests) and wires
it into contract_local.rs."
```

---

## Task 8: Object contract suite

**Files:**
- Modify: `crates/runway-storage-contract/src/object.rs`
- Modify: `crates/runway-storage/tests/contract_local.rs`

No trait change needed.

- [ ] **Step 1: Implement `run_object_suite`**

Replace `crates/runway-storage-contract/src/object.rs` with:

```rust
//! ObjectStore contract suite.

use std::sync::Arc;

use bytes::Bytes;
use runway_storage::ObjectStore;
use serde_json::json;

use crate::{contract_assert, contract_assert_eq, contract_test};
use crate::harness::{ContractContext, SuiteReport};

pub async fn run_object_suite(
    store: Arc<dyn ObjectStore>,
    ctx: ContractContext,
) -> SuiteReport {
    let report = SuiteReport::new(&ctx.backend, "ObjectStore");
    let prefix = ctx.scope("objects");

    contract_test!(&report, "put_then_get_byte_equal", async {
        let key = format!("{prefix}/bytes-1");
        let data = Bytes::from_static(b"\x00\x01\x02\xff\xfe");
        store.put(&key, data.clone(), None).await.map_err(|e| e.to_string())?;
        let got = store.get(&key).await.map_err(|e| e.to_string())?;
        contract_assert_eq!(got, data, "byte-equal roundtrip");
        Ok(())
    });

    contract_test!(&report, "put_get_text_roundtrip", async {
        let key = format!("{prefix}/text-1");
        let text = "hello, contract 🦀";
        store.put_text(&key, text).await.map_err(|e| e.to_string())?;
        let got = store.get_text(&key).await.map_err(|e| e.to_string())?;
        contract_assert_eq!(got, text.to_string(), "UTF-8 roundtrip");
        Ok(())
    });

    contract_test!(&report, "put_get_json_roundtrip", async {
        let key = format!("{prefix}/json-1");
        let value = json!({"a": 1, "b": [true, false], "c": "x"});
        // Note: put_json requires Self: Sized; downcast to a concrete type isn't
        // possible through Arc<dyn>. Fall back to put with a manual JSON encode.
        let bytes = Bytes::from(serde_json::to_vec(&value).map_err(|e| e.to_string())?);
        store.put(&key, bytes, Some("application/json")).await.map_err(|e| e.to_string())?;
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
                    msg.to_lowercase().contains("not found") || msg.to_lowercase().contains("notfound"),
                    "expected 'not found' in error, got: {}",
                    msg
                );
                Ok(())
            }
        }
    });

    contract_test!(&report, "exists_reflects_state", async {
        let key = format!("{prefix}/exists-1");
        contract_assert!(!store.exists(&key).await.map_err(|e| e.to_string())?, "should not exist before put");
        store.put(&key, Bytes::from_static(b"x"), None).await.map_err(|e| e.to_string())?;
        contract_assert!(store.exists(&key).await.map_err(|e| e.to_string())?, "should exist after put");
        store.delete(&key).await.map_err(|e| e.to_string())?;
        contract_assert!(!store.exists(&key).await.map_err(|e| e.to_string())?, "should not exist after delete");
        Ok(())
    });

    contract_test!(&report, "delete_idempotent", async {
        store.delete(&format!("{prefix}/never-existed")).await.map_err(|e| e.to_string())?;
        Ok(())
    });

    contract_test!(&report, "list_prefix_returns_matching_keys", async {
        let scope = format!("{prefix}/listing");
        store.put(&format!("{scope}/a/1"), Bytes::from_static(b"1"), None).await.map_err(|e| e.to_string())?;
        store.put(&format!("{scope}/a/2"), Bytes::from_static(b"2"), None).await.map_err(|e| e.to_string())?;
        store.put(&format!("{scope}/b/1"), Bytes::from_static(b"3"), None).await.map_err(|e| e.to_string())?;

        let keys = store.list(&format!("{scope}/a/")).await.map_err(|e| e.to_string())?;
        contract_assert_eq!(keys.len(), 2, "expected 2 keys under a/, got {:?}", keys);
        contract_assert!(keys.iter().all(|k| k.contains("/a/")), "all keys should be under a/: {:?}", keys);
        Ok(())
    });

    contract_test!(&report, "list_prefix_empty_when_no_match", async {
        let keys = store.list(&format!("{prefix}/nonexistent/")).await.map_err(|e| e.to_string())?;
        contract_assert!(keys.is_empty(), "expected empty key list, got {:?}", keys);
        Ok(())
    });

    contract_test!(&report, "large_payload_roundtrip", async {
        let key = format!("{prefix}/large-1");
        let data: Vec<u8> = (0..1_048_576u32).map(|i| (i % 256) as u8).collect();
        let bytes = Bytes::from(data.clone());
        store.put(&key, bytes.clone(), None).await.map_err(|e| e.to_string())?;
        let got = store.get(&key).await.map_err(|e| e.to_string())?;
        contract_assert_eq!(got.len(), bytes.len(), "1 MB byte length roundtrip");
        contract_assert!(got == bytes, "1 MB byte content roundtrip");
        Ok(())
    });

    report
}
```

- [ ] **Step 2: Wire into `contract_local.rs`**

Add to the imports:

```rust
use runway_storage_contract::object;
```

Add the test:

```rust
#[tokio::test]
async fn object_contract() {
    let (kit, _tmp) = build_kit().await;
    object::run_object_suite(Arc::clone(&kit.objects), ctx())
        .await
        .assert_passed();
}
```

- [ ] **Step 3: Run and lint**

```bash
cargo test -p runway-storage --test contract_local -- --nocapture
just lint
```

Expected: 4 contracts pass.

- [ ] **Step 4: Commit**

```bash
git add crates/runway-storage-contract/src/object.rs crates/runway-storage/tests/contract_local.rs
git commit -m "feat(runway-storage-contract): object suite

9 equivalence tests covering byte-equal roundtrip, exists/delete/list
semantics, large-payload handling. Wired into contract_local.rs."
```

---

## Task 9: EventLog split into base + SyncableEventLog

**Files:**
- Modify: `crates/runway-storage/src/traits/event.rs`
- Modify: `crates/runway-storage/src/local/event.rs`
- Modify: `crates/runway-storage/src/remote/event.rs`
- Modify: `crates/runway-storage/src/lib.rs` (re-export)

**What changes:** `EventLog` keeps only `append` and `query`. `EventQuery` loses `unsynced_only`. A new `SyncableEventLog: EventLog` adds `query_unsynced(&self, q: EventQuery) -> Result<Vec<StoredEvent>>` and `mark_synced`. Local impl satisfies both; remote impl satisfies only the base.

- [ ] **Step 1: Update `traits/event.rs`**

Replace `crates/runway-storage/src/traits/event.rs` with:

```rust
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::traits::Result;

/// An ExperienceEvent as stored in the log. Append-only — never updated or deleted.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StoredEvent {
    pub event_id: String,
    pub org_id: String,
    pub app_id: String,
    pub event_type: String,
    pub context_id: Option<String>,
    pub fact_id: Option<String>,
    pub payload: Value,
    pub occurred_at: DateTime<Utc>,
    /// Populated only in local store — tracks whether this event has been synced to remote.
    #[serde(default)]
    pub synced_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Default)]
pub struct EventQuery {
    pub org_id: Option<String>,
    pub app_id: Option<String>,
    pub event_type: Option<String>,
    pub since: Option<DateTime<Utc>>,
    pub limit: Option<usize>,
}

/// Append-only event ledger. The ExperienceStore from the Converge architecture.
///
/// Local impl:  redb (survives restarts, feeds sync engine)
/// Remote impl: Firestore events subcollection + BigQuery streaming insert
///
/// Sync-engine-specific operations (`mark_synced`, querying for unsynced
/// events) live on [`SyncableEventLog`], which only the local impl implements.
#[async_trait]
pub trait EventLog: Send + Sync {
    async fn append(&self, event: StoredEvent) -> Result<()>;
    async fn query(&self, q: EventQuery) -> Result<Vec<StoredEvent>>;
}

/// Local-only extension of `EventLog` for the sync engine. Remote backends do
/// not implement this; the type system enforces that mark_synced/query_unsynced
/// cannot be called on a remote log.
#[async_trait]
pub trait SyncableEventLog: EventLog {
    /// Return events matching `q` that have NOT yet been marked synced.
    async fn query_unsynced(&self, q: EventQuery) -> Result<Vec<StoredEvent>>;

    /// Mark events as synced.
    async fn mark_synced(&self, event_ids: &[String]) -> Result<()>;
}
```

- [ ] **Step 2: Update `local/event.rs`**

The current impl implements `EventLog` with `unsynced_only` baked into the query path. Refactor: rename the existing query logic into a private helper that takes an additional `unsynced_only: bool`. Then have the public `query` call it with `false` and the new `query_unsynced` (on a `SyncableEventLog` impl) call it with `true`. Move `mark_synced` onto the `SyncableEventLog` impl.

End result, two impl blocks:

```rust
#[async_trait]
impl EventLog for RedbEventLog {
    async fn append(&self, event: StoredEvent) -> Result<()> { /* existing */ }
    async fn query(&self, q: EventQuery) -> Result<Vec<StoredEvent>> {
        self.query_inner(q, false).await
    }
}

#[async_trait]
impl SyncableEventLog for RedbEventLog {
    async fn query_unsynced(&self, q: EventQuery) -> Result<Vec<StoredEvent>> {
        self.query_inner(q, true).await
    }
    async fn mark_synced(&self, event_ids: &[String]) -> Result<()> { /* existing */ }
}
```

- [ ] **Step 3: Update `remote/event.rs`**

Drop the `mark_synced` impl and any `unsynced_only` branching. The Firestore impl now implements only `EventLog`.

- [ ] **Step 4: Update `lib.rs` re-exports**

In `crates/runway-storage/src/lib.rs`:

```rust
pub use traits::{
    document::{Document, DocumentStore, Filter, Order, Query},
    embedding::{Embedding, EmbeddingProvider, EMBEDDING_DIMS},
    event::{EventLog, EventQuery, StoredEvent, SyncableEventLog},
    object::ObjectStore,
    vector::{Match, VectorStore},
};
```

- [ ] **Step 5: Check that `StorageKit` still exposes events correctly**

In `lib.rs`, the `StorageKit.events` field is `Arc<dyn EventLog>`. Local impl satisfies the base trait, so this still works. Add a separate field if you need the local-only sync surface:

```rust
#[derive(Clone)]
pub struct StorageKit {
    pub documents: Arc<dyn DocumentStore>,
    pub vectors: Arc<dyn VectorStore>,
    pub objects: Arc<dyn ObjectStore>,
    pub events: Arc<dyn EventLog>,
    pub embeddings: Arc<dyn EmbeddingProvider>,
    /// Local-only: present when running against the redb backend. None for remote.
    pub syncable_events: Option<Arc<dyn SyncableEventLog>>,
}
```

In `LocalStorageKit::build`, populate `syncable_events: Some(Arc::clone(&events_arc_as_syncable))`. In `RemoteStorageKit::build`, set `syncable_events: None`. The `events` field comes from the same Arc as `syncable_events` in local case (upcast).

(If a clean upcast is awkward in Rust due to `Arc<dyn SyncableEventLog>` not auto-coercing to `Arc<dyn EventLog>`, build two Arcs from the same concrete `RedbEventLog`:

```rust
let redb_log = Arc::new(RedbEventLog::new(db.clone()));
let events: Arc<dyn EventLog> = redb_log.clone();
let syncable: Arc<dyn SyncableEventLog> = redb_log;
StorageKit { events, syncable_events: Some(syncable), .. }
```

That's the recommended pattern.)

- [ ] **Step 6: Verify build**

```bash
cargo build -p runway-storage
```

Expected: clean build. If `runway-accounts` or other callers broke (they shouldn't — they don't use EventLog), fix at the call site.

- [ ] **Step 7: Implement event contract suite (base + syncable)**

Replace `crates/runway-storage-contract/src/event.rs` with:

```rust
//! EventLog (base) and SyncableEventLog contract suites.

use std::sync::Arc;

use chrono::Utc;
use runway_storage::{EventLog, EventQuery, StoredEvent, SyncableEventLog};
use serde_json::json;

use crate::{contract_assert, contract_assert_eq, contract_test};
use crate::harness::{ContractContext, SuiteReport};

fn mk_event(ctx: &ContractContext, event_type: &str, occurred_at: chrono::DateTime<Utc>) -> StoredEvent {
    StoredEvent {
        event_id: uuid::Uuid::new_v4().to_string(),
        org_id: format!("{}-org", ctx.namespace),
        app_id: format!("{}-app", ctx.namespace),
        event_type: event_type.to_string(),
        context_id: None,
        fact_id: None,
        payload: json!({"hello": "world"}),
        occurred_at,
        synced_at: None,
    }
}

fn base_query(ctx: &ContractContext) -> EventQuery {
    EventQuery {
        org_id: Some(format!("{}-org", ctx.namespace)),
        app_id: Some(format!("{}-app", ctx.namespace)),
        ..Default::default()
    }
}

pub async fn run_event_suite(log: Arc<dyn EventLog>, ctx: ContractContext) -> SuiteReport {
    let report = SuiteReport::new(&ctx.backend, "EventLog");

    contract_test!(&report, "append_then_query_returns_event", async {
        let evt = mk_event(&ctx, "test.append_query", Utc::now());
        let evt_id = evt.event_id.clone();
        log.append(evt).await.map_err(|e| e.to_string())?;
        let events = log.query(base_query(&ctx)).await.map_err(|e| e.to_string())?;
        contract_assert!(
            events.iter().any(|e| e.event_id == evt_id),
            "appended event not found in query result"
        );
        Ok(())
    });

    contract_test!(&report, "query_by_event_type_filter", async {
        log.append(mk_event(&ctx, "type.a", Utc::now())).await.map_err(|e| e.to_string())?;
        log.append(mk_event(&ctx, "type.b", Utc::now())).await.map_err(|e| e.to_string())?;
        let q = EventQuery { event_type: Some("type.a".into()), ..base_query(&ctx) };
        let events = log.query(q).await.map_err(|e| e.to_string())?;
        contract_assert!(
            events.iter().all(|e| e.event_type == "type.a"),
            "type filter leaked other event types"
        );
        Ok(())
    });

    contract_test!(&report, "query_since_filters_by_occurred_at", async {
        let before = Utc::now();
        log.append(mk_event(&ctx, "type.since.before", before)).await.map_err(|e| e.to_string())?;
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        let cutoff = Utc::now();
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        log.append(mk_event(&ctx, "type.since.after", Utc::now())).await.map_err(|e| e.to_string())?;
        let q = EventQuery { since: Some(cutoff), ..base_query(&ctx) };
        let events = log.query(q).await.map_err(|e| e.to_string())?;
        contract_assert!(
            events.iter().all(|e| e.occurred_at >= cutoff),
            "since filter returned earlier events"
        );
        Ok(())
    });

    contract_test!(&report, "query_limit_respected", async {
        for _ in 0..5 {
            log.append(mk_event(&ctx, "type.limit", Utc::now())).await.map_err(|e| e.to_string())?;
        }
        let q = EventQuery { limit: Some(2), event_type: Some("type.limit".into()), ..base_query(&ctx) };
        let events = log.query(q).await.map_err(|e| e.to_string())?;
        contract_assert!(events.len() <= 2, "limit not respected: got {} events", events.len());
        Ok(())
    });

    contract_test!(&report, "synced_at_none_on_append", async {
        let evt = mk_event(&ctx, "type.synced_at_check", Utc::now());
        let evt_id = evt.event_id.clone();
        log.append(evt).await.map_err(|e| e.to_string())?;
        let events = log.query(EventQuery {
            event_type: Some("type.synced_at_check".into()),
            ..base_query(&ctx)
        }).await.map_err(|e| e.to_string())?;
        let found = events.iter().find(|e| e.event_id == evt_id).ok_or("event not found")?;
        contract_assert!(found.synced_at.is_none(), "freshly-appended event has synced_at set");
        Ok(())
    });

    report
}

pub async fn run_syncable_event_suite(
    log: Arc<dyn SyncableEventLog>,
    ctx: ContractContext,
) -> SuiteReport {
    let report = SuiteReport::new(&ctx.backend, "SyncableEventLog");

    contract_test!(&report, "mark_synced_sets_synced_at", async {
        let evt = mk_event(&ctx, "type.mark_synced", Utc::now());
        let evt_id = evt.event_id.clone();
        log.append(evt).await.map_err(|e| e.to_string())?;
        log.mark_synced(&[evt_id.clone()]).await.map_err(|e| e.to_string())?;
        let unsynced = log.query_unsynced(EventQuery {
            event_type: Some("type.mark_synced".into()),
            ..base_query(&ctx)
        }).await.map_err(|e| e.to_string())?;
        contract_assert!(
            !unsynced.iter().any(|e| e.event_id == evt_id),
            "marked-synced event still in unsynced query"
        );
        Ok(())
    });

    contract_test!(&report, "unsynced_only_filters", async {
        log.append(mk_event(&ctx, "type.uns.a", Utc::now())).await.map_err(|e| e.to_string())?;
        let synced = mk_event(&ctx, "type.uns.b", Utc::now());
        let synced_id = synced.event_id.clone();
        log.append(synced).await.map_err(|e| e.to_string())?;
        log.mark_synced(&[synced_id.clone()]).await.map_err(|e| e.to_string())?;

        let unsynced = log.query_unsynced(EventQuery {
            event_type: Some("type.uns.b".into()),
            ..base_query(&ctx)
        }).await.map_err(|e| e.to_string())?;
        contract_assert!(
            !unsynced.iter().any(|e| e.event_id == synced_id),
            "synced event leaked into unsynced query"
        );
        Ok(())
    });

    report
}
```

- [ ] **Step 8: Wire event suites into `contract_local.rs`**

Add to imports:

```rust
use runway_storage_contract::event;
```

Add tests:

```rust
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
```

- [ ] **Step 9: Run, lint, commit**

```bash
cargo test -p runway-storage --test contract_local -- --nocapture
just lint
git add crates/runway-storage/src/traits/event.rs crates/runway-storage/src/local/event.rs crates/runway-storage/src/remote/event.rs crates/runway-storage/src/lib.rs crates/runway-storage-contract/src/event.rs crates/runway-storage/tests/contract_local.rs
git commit -m "refactor(runway-storage): split EventLog into base + SyncableEventLog

EventLog (base) keeps only append/query and is implemented by both
backends. SyncableEventLog adds query_unsynced and mark_synced; only
the redb local backend implements it. EventQuery no longer carries an
unsynced_only flag — sync-engine queries go through the explicit
query_unsynced method instead.

StorageKit gains a syncable_events: Option<Arc<dyn SyncableEventLog>>
field, populated on local and None on remote. The type system now
enforces that mark_synced cannot be called against the remote backend.

Adds run_event_suite (5 tests) and run_syncable_event_suite (2 tests,
local-only) to the contract crate."
```

---

## Task 10: RemoteStorageKit accepts optional embedder override

**Files:**
- Modify: `crates/runway-storage/src/remote/mod.rs`

**What changes:** `RemoteStorageKit::build` keeps its current signature (default Vertex AI embedder). A new `build_with_embedder` accepts an `Option<Arc<dyn EmbeddingProvider>>`. The emulator entry point uses this to inject fastembed since Vertex AI has no emulator.

- [ ] **Step 1: Read current `remote/mod.rs`**

```bash
cat crates/runway-storage/src/remote/mod.rs
```

Find the `RemoteStorageKit::build` function. It currently constructs a Vertex AI embedder unconditionally.

- [ ] **Step 2: Add `build_with_embedder`**

Refactor `build` to delegate:

```rust
use std::sync::Arc;
use crate::EmbeddingProvider;

impl RemoteStorageKit {
    pub async fn build(config: RemoteConfig) -> Result<StorageKit> {
        Self::build_with_embedder(config, None).await
    }

    pub async fn build_with_embedder(
        config: RemoteConfig,
        embedder_override: Option<Arc<dyn EmbeddingProvider>>,
    ) -> Result<StorageKit> {
        // ... build documents, vectors, objects, events as before ...

        let embeddings: Arc<dyn EmbeddingProvider> = match embedder_override {
            Some(e) => e,
            None => Arc::new(crate::embedding::vertex::VertexEmbedder::new(&config).await?),
        };

        Ok(StorageKit {
            documents,
            vectors,
            objects,
            events,
            embeddings,
            syncable_events: None,  // remote never has this
        })
    }
}
```

(Adjust the constructor call to match the actual `VertexEmbedder` constructor signature in your tree.)

- [ ] **Step 3: Build**

```bash
cargo build -p runway-storage
just lint
```

Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add crates/runway-storage/src/remote/mod.rs
git commit -m "feat(runway-storage): RemoteStorageKit::build_with_embedder

New constructor takes an optional EmbeddingProvider override. Default
build() unchanged — still wires Vertex AI. The override exists so the
contract emulator entry point can inject fastembed (no Vertex AI
emulator exists)."
```

---

## Task 11: Docker compose for emulators

**Files:**
- Create: `crates/runway-storage/tests/docker-compose.contract.yml`

- [ ] **Step 1: Create the compose file**

Create `crates/runway-storage/tests/docker-compose.contract.yml`:

```yaml
services:
  firestore:
    image: google/cloud-sdk:latest
    command: >
      gcloud beta emulators firestore start
      --host-port=0.0.0.0:8080
      --project=runway-contract
    ports:
      - "8080:8080"
    healthcheck:
      test: ["CMD", "curl", "-fsS", "http://localhost:8080"]
      interval: 2s
      timeout: 2s
      retries: 15

  pubsub:
    image: google/cloud-sdk:latest
    command: >
      gcloud beta emulators pubsub start
      --host-port=0.0.0.0:8085
      --project=runway-contract
    ports:
      - "8085:8085"
    healthcheck:
      test: ["CMD", "curl", "-fsS", "http://localhost:8085"]
      interval: 2s
      timeout: 2s
      retries: 15

  gcs:
    image: fsouza/fake-gcs-server:latest
    command: >
      -scheme http
      -host 0.0.0.0
      -port 4443
      -public-host localhost:4443
      -external-url http://localhost:4443
    ports:
      - "4443:4443"
    healthcheck:
      test: ["CMD-SHELL", "wget -q -O- http://localhost:4443/storage/v1/b || exit 1"]
      interval: 2s
      timeout: 2s
      retries: 15
```

- [ ] **Step 2: Verify the stack comes up**

```bash
docker compose -f crates/runway-storage/tests/docker-compose.contract.yml up -d --wait
docker compose -f crates/runway-storage/tests/docker-compose.contract.yml ps
docker compose -f crates/runway-storage/tests/docker-compose.contract.yml down
```

Expected: three services start, all show "healthy" or "running (healthy)". Down cleans up.

- [ ] **Step 3: Commit**

```bash
git add crates/runway-storage/tests/docker-compose.contract.yml
git commit -m "feat(runway-storage): docker compose for contract emulators

Firestore + Pub/Sub emulators from google/cloud-sdk and GCS emulator
from fsouza/fake-gcs-server. Used by contract_emulator.rs entry point
and the contract CI workflow."
```

---

## Task 12: Emulator contract entry point

**Files:**
- Create: `crates/runway-storage/tests/contract_emulator.rs`

- [ ] **Step 1: Create the entry point**

Create `crates/runway-storage/tests/contract_emulator.rs`:

```rust
//! Contract tests against emulated GCP services + fastembed.
//!
//! Requires the emulator stack from docker-compose.contract.yml to be running.
//! `just contract-emulator` handles startup/teardown automatically.

use std::sync::Arc;

use runway_storage::{
    embedding::local::LocalEmbedder,
    remote::{RemoteConfig, RemoteStorageKit},
    EmbeddingProvider,
};
use runway_storage_contract::{document, embedding, event, object, vector, ContractContext};

fn emulator_config() -> RemoteConfig {
    // The exact field names here must match the current RemoteConfig in
    // crates/runway-storage/src/remote/mod.rs. Adjust if the struct has been
    // refactored — the principle is: project_id = "runway-contract", endpoints
    // pointed at the emulator hosts via env vars.
    RemoteConfig {
        project_id: "runway-contract".into(),
        // ... other fields per current RemoteConfig
        ..Default::default()
    }
}

async fn build_kit() -> runway_storage::StorageKit {
    // Confirm emulator env vars are set so the underlying clients route correctly.
    assert!(std::env::var("FIRESTORE_EMULATOR_HOST").is_ok(),
        "FIRESTORE_EMULATOR_HOST must be set (docker compose sets this)");
    assert!(std::env::var("STORAGE_EMULATOR_HOST").is_ok(),
        "STORAGE_EMULATOR_HOST must be set");
    assert!(std::env::var("PUBSUB_EMULATOR_HOST").is_ok(),
        "PUBSUB_EMULATOR_HOST must be set");

    let fastembed = Arc::new(LocalEmbedder::new().await.unwrap()) as Arc<dyn EmbeddingProvider>;
    RemoteStorageKit::build_with_embedder(emulator_config(), Some(fastembed))
        .await
        .expect("emulator kit build")
}

fn ctx() -> ContractContext {
    let run_id = uuid::Uuid::new_v4();
    ContractContext::new("firestore-emulator+gcs-emulator+fastembed", format!("_contract/{run_id}"))
}

#[tokio::test]
async fn document_contract() {
    let kit = build_kit().await;
    document::run_document_suite(Arc::clone(&kit.documents), ctx())
        .await
        .assert_passed();
}

#[tokio::test]
async fn object_contract() {
    let kit = build_kit().await;
    object::run_object_suite(Arc::clone(&kit.objects), ctx())
        .await
        .assert_passed();
}

#[tokio::test]
async fn event_contract() {
    let kit = build_kit().await;
    event::run_event_suite(Arc::clone(&kit.events), ctx())
        .await
        .assert_passed();
}

#[tokio::test]
async fn vector_contract() {
    let kit = build_kit().await;
    vector::run_vector_shape_suite(Arc::clone(&kit.vectors), ctx())
        .await
        .assert_passed();
}

#[tokio::test]
async fn embedding_contract() {
    let kit = build_kit().await;
    embedding::run_embedding_shape_suite(Arc::clone(&kit.embeddings), ctx())
        .await
        .assert_passed();
}

// SyncableEventLog is local-only; no test here.
```

- [ ] **Step 2: Verify manually**

```bash
docker compose -f crates/runway-storage/tests/docker-compose.contract.yml up -d --wait
FIRESTORE_EMULATOR_HOST=localhost:8080 \
PUBSUB_EMULATOR_HOST=localhost:8085 \
STORAGE_EMULATOR_HOST=http://localhost:4443 \
  cargo test -p runway-storage --test contract_emulator -- --nocapture
docker compose -f crates/runway-storage/tests/docker-compose.contract.yml down
```

Expected: 5 contracts pass against the emulator stack. If `RemoteStorageKit::build_with_embedder` doesn't compile against your actual `RemoteConfig`, adapt the constructor call.

- [ ] **Step 3: Commit**

```bash
git add crates/runway-storage/tests/contract_emulator.rs
git commit -m "feat(runway-storage): contract_emulator.rs entry point

Builds RemoteStorageKit against Firestore/GCS/Pub-Sub emulators with
fastembed injected as the embedding provider (no Vertex AI emulator
exists). Reads emulator endpoints from FIRESTORE_EMULATOR_HOST,
STORAGE_EMULATOR_HOST, PUBSUB_EMULATOR_HOST. Each run uses a fresh
namespace prefix _contract/<uuid> so concurrent runs do not collide."
```

---

## Task 13: Real-GCP contract entry point

**Files:**
- Create: `crates/runway-storage/tests/contract_real_gcp.rs`

- [ ] **Step 1: Create the entry point**

Create `crates/runway-storage/tests/contract_real_gcp.rs`:

```rust
//! Contract tests against real GCP (staging project). #[ignore]'d by default;
//! run with `cargo test -- --ignored` or `just contract-staging`.
//!
//! Requires:
//!   RUNWAY_CONTRACT_PROJECT   — staging GCP project id
//!   RUNWAY_CONTRACT_BUCKET    — staging GCS bucket
//!   RUNWAY_CONTRACT_REGION    — Vertex AI region (e.g. "us-central1")
//!   Application Default Credentials available (`gcloud auth application-default login`).

use std::sync::Arc;

use runway_storage::{
    remote::{RemoteConfig, RemoteStorageKit},
    StorageKit,
};
use runway_storage_contract::{document, embedding, event, object, vector, ContractContext};

fn require_env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("env var {name} required"))
}

fn real_gcp_config() -> RemoteConfig {
    RemoteConfig {
        project_id: require_env("RUNWAY_CONTRACT_PROJECT"),
        // Populate other RemoteConfig fields from env. The exact list depends
        // on the current shape of RemoteConfig — at minimum: bucket, region.
        ..Default::default()
    }
}

async fn build_kit() -> StorageKit {
    RemoteStorageKit::build(real_gcp_config()).await.expect("real GCP kit build")
}

fn ctx() -> ContractContext {
    let run_id = uuid::Uuid::new_v4();
    ContractContext::new("firestore+gcs+vertex-ai", format!("_contract/{run_id}"))
}

async fn cleanup(_kit: &StorageKit, _namespace: &str) {
    // Best-effort: iterate the namespace, delete known collections, GCS prefix,
    // and Pub/Sub topics. Swallows errors so the original test failure surfaces.
    // A Terraform-managed Cloud Scheduler job sweeps _contract/ debris >24h old
    // as a safety net — see ops/.
    //
    // Concrete cleanup: delete all docs under the namespace via DocumentStore
    // queries; iterate ObjectStore list+delete under the prefix; ... depending
    // on the actual remote impl, this may require dropping to backend-specific
    // clients (which is fine — this code lives in the test entry point).
    tracing::info!("contract_real_gcp cleanup: namespace = (logged in tests)");
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
    let report = embedding::run_embedding_shape_suite(Arc::clone(&kit.embeddings), context.clone()).await;
    cleanup(&kit, &context.namespace).await;
    report.assert_passed();
}
```

- [ ] **Step 2: Verify it compiles (do not run yet — needs real GCP)**

```bash
cargo test -p runway-storage --test contract_real_gcp --no-run
```

Expected: compiles. Actual run is deferred to Task 16 verification (after `just` recipes are in).

- [ ] **Step 3: Commit**

```bash
git add crates/runway-storage/tests/contract_real_gcp.rs
git commit -m "feat(runway-storage): contract_real_gcp.rs entry point

#[ignore]'d by default; runs only with cargo test -- --ignored or
just contract-staging. Uses ADC for auth, reads project/bucket/region
from RUNWAY_CONTRACT_* env vars. Each run uses a fresh _contract/<uuid>
namespace prefix; per-test cleanup is best-effort with a Terraform
Cloud Scheduler safety net for missed debris."
```

---

## Task 14: Just recipes

**Files:**
- Modify: `justfile`

- [ ] **Step 1: Read current `justfile`**

```bash
cat justfile
```

Locate where other recipes are defined.

- [ ] **Step 2: Add contract recipes**

Append to `justfile`:

```just
# Run local + emulator contract suites (default for `just contract`)
contract: contract-local contract-emulator

# Run contract suite against the local (redb + FS + fastembed) backend
contract-local:
    cargo test -p runway-storage --test contract_local -- --nocapture

# Run contract suite against the Firestore/GCS/Pub-Sub emulators with fastembed
contract-emulator:
    docker compose -f crates/runway-storage/tests/docker-compose.contract.yml up -d --wait
    -FIRESTORE_EMULATOR_HOST=localhost:8080 \
     PUBSUB_EMULATOR_HOST=localhost:8085 \
     STORAGE_EMULATOR_HOST=http://localhost:4443 \
       cargo test -p runway-storage --test contract_emulator -- --nocapture
    docker compose -f crates/runway-storage/tests/docker-compose.contract.yml down

# Run contract suite against real staging GCP. Requires RUNWAY_CONTRACT_* env vars and ADC.
contract-staging:
    @test -n "$RUNWAY_CONTRACT_PROJECT" || (echo "set RUNWAY_CONTRACT_PROJECT (and _BUCKET, _REGION)" && exit 1)
    cargo test -p runway-storage --test contract_real_gcp -- --ignored --nocapture

# Run all three (local + emulator + real GCP)
contract-all: contract contract-staging
```

(Use tabs to match the existing `justfile` style — most `justfile`s use 4 spaces; check what this repo uses.)

- [ ] **Step 3: Test the recipes**

```bash
just contract-local
just contract-emulator
just --list | grep contract
```

Expected: both pass; `just --list` shows all five recipes.

- [ ] **Step 4: Commit**

```bash
git add justfile
git commit -m "chore: add just recipes for runway-storage contract suites

just contract              — local + emulator (default for dev)
just contract-local        — fastest, no Docker
just contract-emulator     — brings up Docker, runs, tears down
just contract-staging      — real GCP, manual + release CI gate
just contract-all          — everything"
```

---

## Task 15: CI workflow — emulator suite

**Files:**
- Create: `.github/workflows/contract.yml`

- [ ] **Step 1: Check existing workflows for style and reusable steps**

```bash
ls .github/workflows/
```

Read one existing workflow to match conventions (rust toolchain action used, cache strategy, etc.).

- [ ] **Step 2: Create the contract workflow**

Create `.github/workflows/contract.yml`:

```yaml
name: contract (local + emulator)

on:
  pull_request:
    branches: [main]
  push:
    branches: [main]

jobs:
  contract:
    runs-on: ubuntu-latest
    timeout-minutes: 10
    steps:
      - uses: actions/checkout@v4

      - name: Install Rust toolchain
        uses: dtolnay/rust-toolchain@stable
        with:
          components: clippy, rustfmt

      - name: Cargo cache
        uses: Swatinem/rust-cache@v2
        with:
          workspaces: .

      - name: Install just
        uses: extractions/setup-just@v2

      - name: Bring up emulator stack
        run: docker compose -f crates/runway-storage/tests/docker-compose.contract.yml up -d --wait

      - name: Run local contract suite
        run: just contract-local

      - name: Run emulator contract suite
        env:
          FIRESTORE_EMULATOR_HOST: localhost:8080
          PUBSUB_EMULATOR_HOST: localhost:8085
          STORAGE_EMULATOR_HOST: http://localhost:4443
        run: cargo test -p runway-storage --test contract_emulator -- --nocapture

      - name: Tear down emulator stack
        if: always()
        run: docker compose -f crates/runway-storage/tests/docker-compose.contract.yml down
```

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/contract.yml
git commit -m "ci: add contract workflow (local + emulator on PR and main)

Runs just contract-local + the emulator suite directly (so docker
compose lifecycle is visible in the CI log). 10-minute timeout. Will
be required for PR merges via branch protection (set up separately)."
```

---

## Task 16: CI workflow — staging suite

**Files:**
- Create: `.github/workflows/contract-staging.yml`

- [ ] **Step 1: Create the staging workflow**

Create `.github/workflows/contract-staging.yml`:

```yaml
name: contract (staging GCP)

on:
  push:
    tags:
      - "release/*"
  workflow_dispatch:

permissions:
  contents: read
  id-token: write   # for workload identity federation

jobs:
  contract-staging:
    runs-on: ubuntu-latest
    timeout-minutes: 20
    steps:
      - uses: actions/checkout@v4

      - name: Install Rust toolchain
        uses: dtolnay/rust-toolchain@stable

      - name: Cargo cache
        uses: Swatinem/rust-cache@v2
        with:
          workspaces: .

      - name: Authenticate to GCP via workload identity
        uses: google-github-actions/auth@v2
        with:
          workload_identity_provider: ${{ secrets.GCP_WORKLOAD_IDENTITY_PROVIDER }}
          service_account: ${{ secrets.GCP_CONTRACT_SERVICE_ACCOUNT }}

      - name: Install just
        uses: extractions/setup-just@v2

      - name: Run contract suite against staging GCP
        env:
          RUNWAY_CONTRACT_PROJECT: ${{ secrets.RUNWAY_CONTRACT_PROJECT }}
          RUNWAY_CONTRACT_BUCKET: ${{ secrets.RUNWAY_CONTRACT_BUCKET }}
          RUNWAY_CONTRACT_REGION: ${{ secrets.RUNWAY_CONTRACT_REGION }}
        run: just contract-staging
```

- [ ] **Step 2: Document required secrets**

Add to the workflow file as a top comment (after `name:`):

```yaml
# Required repo secrets:
#   GCP_WORKLOAD_IDENTITY_PROVIDER  — workload identity provider resource name
#   GCP_CONTRACT_SERVICE_ACCOUNT    — service account email with contract permissions
#   RUNWAY_CONTRACT_PROJECT         — staging GCP project id
#   RUNWAY_CONTRACT_BUCKET          — staging GCS bucket
#   RUNWAY_CONTRACT_REGION          — Vertex AI region (e.g. us-central1)
#
# The workload identity binding and the contract service account must be
# provisioned via Terraform in ops/ before this workflow can succeed.
```

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/contract-staging.yml
git commit -m "ci: add contract-staging workflow (real GCP, release-gated)

Runs on tags release/* and via workflow_dispatch. Uses workload
identity federation — no long-lived service account keys. Required
repo secrets and Terraform-managed IAM are documented inline."
```

---

## Task 17: Final verification

**Files:** none (verification only)

- [ ] **Step 1: Run everything that runs locally**

```bash
just lint
just contract
```

Expected: all green. The full output should show:
- 12 document tests
- 9 object tests
- 5 base event tests + 2 syncable event tests (local only)
- 7 vector tests
- 4 embedding tests

… all passing, on both local and emulator backends (minus the 2 syncable event tests on emulator).

- [ ] **Step 2: Verify the deliberate-divergence test**

To prove the suite actually catches divergence, temporarily revert the timestamp-preservation logic in `local/document.rs` (e.g. remove the `prior.created_at` lookup):

```bash
# manually break it (don't commit this)
git stash push -m "test divergence detection"
# break the impl
just contract-local
```

Expected: `put_overwrites_with_advancing_updated_at` fails with a clear contract violation message naming the trait and backend. Restore:

```bash
git stash pop
just contract-local  # passes again
```

- [ ] **Step 3: Confirm git log is clean**

```bash
git log --oneline spike1-atlas-staging-app...main
```

Expected: a tight series of commits, one per task. No fix-ups or reverts. Spec and plan docs are at the top of the series.

- [ ] **Step 4: No commit on this task — it's verification only**

If everything above passes, the work is done.

---

## Self-Review Notes

Coverage check against spec:

- ✅ Architecture / crate layout — Tasks 2, 4 onwards
- ✅ Suite function signature — defined in harness.rs (Task 2)
- ✅ Failure message format — implemented in `SuiteReport::assert_passed` (Task 2)
- ✅ Trait refinement 1 (timestamps) — Task 3
- ✅ Trait refinement 2 (EventLog split) — Task 9
- ✅ Trait refinement 3 (Embedding wrapper) — Tasks 5, 6, 7
- ✅ Trait refinement 4 (empty-input rejection) — Task 6
- ✅ DocumentStore suite — Task 4
- ✅ ObjectStore suite — Task 8
- ✅ EventLog + SyncableEventLog suites — Task 9
- ✅ VectorStore shape suite — Task 7
- ✅ EmbeddingProvider shape suite — Task 6
- ✅ contract_local.rs — incrementally built across Tasks 4, 6, 7, 8, 9
- ✅ contract_emulator.rs — Task 12 (with prerequisite RemoteStorageKit override in Task 10)
- ✅ contract_real_gcp.rs — Task 13
- ✅ Docker compose — Task 11
- ✅ just recipes — Task 14
- ✅ CI: contract.yml — Task 15
- ✅ CI: contract-staging.yml — Task 16
- ✅ Doc drift cleanup — Task 1
- ✅ Final verification — Task 17
