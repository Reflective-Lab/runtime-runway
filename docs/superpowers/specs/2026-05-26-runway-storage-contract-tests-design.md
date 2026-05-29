# Runway Storage Contract Tests — Design

**Status:** Approved design, ready for implementation plan
**Date:** 2026-05-26
**Author:** Kenneth Pernyer (with Claude)

## Goal

Establish a contract test suite that proves the local and remote `StorageKit`
backends behave equivalently against the traits they implement. The suite is
the executable specification of what each trait promises. Apps built on
Runway (Helm, Axiom, Organism) inherit that guarantee — they only need to test
app logic, not "does the platform work in this environment."

## Motivation

`runway-storage` exposes five traits (`DocumentStore`, `VectorStore`,
`ObjectStore`, `EventLog`, `EmbeddingProvider`) and two implementations:

- **Local:** redb (documents, vectors, events) + local FS (objects) + fastembed (embeddings)
- **Remote:** Firestore (documents, events) + GCS (objects) + Vertex AI (vectors, embeddings)

The doc comment in `src/lib.rs` calls the local stack "SQLite + LanceDB" — that's
stale. Both were removed (`kb/History/CHANGELOG.md` records the dependency
conflicts with the burn ML stack: LanceDB had a version conflict with
`burn-core`; sqlx pulled a `libsqlite3-sys` that linker-conflicted with the
`rusqlite` already brought in by `burn-dataset`). redb is the deliberate
replacement, not a casual downgrade. Doc drift is fixed as part of this work.

Today, the assertion that "two modes, same code" holds is unverified. Apps
discover divergences at runtime, in staging or worse. The contract suite
converts this assertion into a fact, and makes divergences a CI-blocking failure.

## Scope

**In scope:**

- A new `runway-storage-contract` crate exposing one suite function per trait
- Three test entry points in `runway-storage/tests/` (local, emulator, real GCP)
- Equivalence-flavor tests for `DocumentStore`, `ObjectStore`, `EventLog`
- Shape-flavor tests for `VectorStore` and `EmbeddingProvider`
- Docker compose for Firestore / Pub/Sub / GCS emulators
- `just` recipes (`contract`, `contract-local`, `contract-emulator`, `contract-staging`)
- Two GitHub Actions workflows (PR-gated emulator suite, release-gated real-GCP suite)
- Four trait surface refinements settled in design (see "Trait refinements")
- Documentation drift cleanup (5 stale references to SQLite/LanceDB)

**Out of scope (follow-up):**

- Contract suites for other Runway crates (`runway-secrets`, `runway-auth`, etc.)
- Performance benchmarks (this is correctness, not perf)
- Downstream caller updates (Helm/Axiom/Organism) for the `Embedding` typed
  wrapper — tracked as a separate piece of work
- Cross-trait transactional guarantees (intentionally not promised by any trait)
- Property-based / fuzz tests (post-MVP if the deterministic suite proves stable)

## Non-goals

- **Cross-trait atomicity.** Local redb could in principle wrap doc+vector writes
  in one transaction; remote (Firestore + Vertex AI) cannot. The trait surface
  does not promise this and the contract suite must not test for it.
- **Embedding semantic equivalence.** fastembed and Vertex AI use different
  models. We do not test that "two related strings produce similar embeddings."
  Only shape/dimensionality/error-mode parity.
- **Vector search score equivalence.** Different ANN implementations score
  differently. We test that exact self-matches return self first, that top-k
  bounds are respected, and that namespaces isolate — not specific scores.

## Architecture

### Crate layout

```
crates/
  runway-storage/                       # existing
    tests/
      contract_local.rs                 # entry point: redb backend
      contract_emulator.rs              # entry point: emulators + fastembed
      contract_real_gcp.rs              # entry point: real GCP, #[ignore]
      docker-compose.contract.yml       # emulator stack
  runway-storage-contract/              # NEW
    src/
      lib.rs                            # SuiteReport, dispatcher
      document.rs                       # run_document_suite
      object.rs                         # run_object_suite
      event.rs                          # run_event_suite
      vector.rs                         # run_vector_shape_suite
      embedding.rs                      # run_embedding_shape_suite
      harness.rs                        # contract_assert! macro, namespace helpers
    Cargo.toml                          # depends on runway-storage only
```

### Key property: the contract crate is backend-agnostic

`runway-storage-contract` depends only on the trait surface from
`runway-storage`. It never imports `redb`, `firestore`, `gcs`, `vertex_ai`, or
any backend-specific type. Backend instantiation lives entirely in the three
test entry points. Future backends are added by writing one new entry point.

### Suite function signature

```rust
// runway-storage-contract/src/document.rs
pub async fn run_document_suite(
    store: Arc<dyn DocumentStore>,
    backend_name: &str,   // for failure messages: "redb" | "firestore-emulator" | "firestore"
    namespace: &str,      // collection prefix, e.g. "_contract/<uuid>"
) -> SuiteReport;
```

`SuiteReport` aggregates pass/fail per test so the caller can decide whether
to `panic!` (in `#[test]`) or return structured data (future audit tools).

### Failure message format

```
contract violation [DocumentStore @ firestore-emulator]:
  expected updated_at to advance after put-overwrite,
  got: created=2026-05-26T14:22:01Z updated=2026-05-26T14:22:01Z (same)
  collection=_contract/<uuid>/docs id=overwrite-1
```

Format produced by a `contract_assert!` macro in `harness.rs`. The prefix
`[Trait @ backend]` makes the failure self-locating without reading test source.

## Trait refinements (settled in design)

These are small refactors to the existing trait surface, done as part of the
contract test work because the contracts can't be written cleanly without them.

### 1. Document timestamp policy

**Decision:** Preserve `created_at` on overwrite, advance `updated_at`.

Impls do a read-before-write (or `MERGE`-equivalent) on `put`: if the document
exists, the new doc keeps the existing `created_at` and gets a fresh
`updated_at`. If it doesn't exist, both fields are stamped at the impl. Callers
no longer need to pre-fetch to compute timestamps.

**Affected files:** `local/document.rs`, `remote/document.rs`.

### 2. EventLog sync semantics — split into sub-trait

**Decision:** Move `mark_synced` and `EventQuery::unsynced_only` off `EventLog`
into a new `SyncableEventLog: EventLog` trait that only the redb local impl
implements. The base trait has only `append` and a `query` whose
`EventQuery` no longer contains `unsynced_only`. The sync engine accepts
`Arc<dyn SyncableEventLog>`; everyone else accepts `Arc<dyn EventLog>`.

The type system now enforces the local-only boundary that was previously a
runtime caveat.

**Affected files:** `traits/event.rs` (split), `local/event.rs` (impls both
traits), `remote/event.rs` (impls base only), plus any callers of the sync
engine. Callers that used `unsynced_only=true` on a base-trait reference will
fail to compile until they upgrade to `Arc<dyn SyncableEventLog>` — surfacing
exactly the bugs this refactor exists to prevent.

### 3. Typed `Embedding` wrapper

**Decision:** Introduce a typed wrapper that makes wrong-dim embeddings
unrepresentable.

```rust
pub const EMBEDDING_DIMS: usize = 768;

#[derive(Debug, Clone, PartialEq)]
pub struct Embedding([f32; EMBEDDING_DIMS]);

impl Embedding {
    pub fn new(values: Vec<f32>) -> Result<Self, Error> {
        let arr: [f32; EMBEDDING_DIMS] = values
            .try_into()
            .map_err(|v: Vec<f32>| Error::Other(format!(
                "expected {EMBEDDING_DIMS} dims, got {}", v.len()
            )))?;
        Ok(Self(arr))
    }

    pub fn as_slice(&self) -> &[f32] {
        &self.0
    }
}
```

Trait surface changes:

- `VectorStore::upsert(.., embedding: &Embedding, ..)`
- `VectorStore::search(.., query: &Embedding, ..)`
- `EmbeddingProvider::embed(text) -> Result<Embedding>`
- `EmbeddingProvider::embed_batch(texts) -> Result<Vec<Embedding>>`
- `EmbeddingProvider::dims()` **removed** (redundant — the type encodes it)

Serde for `Embedding` implements `Serialize`/`Deserialize` manually, routing
through `Embedding::new()` on deserialization. This gives free validation at
the deser boundary — corrupt or malformed stored data fails loudly with the
same error message as constructor misuse.

**Affected files:** `traits/vector.rs`, `traits/embedding.rs`,
`embedding/local.rs`, `embedding/vertex.rs`, `local/vector.rs`,
`remote/vector.rs`. Downstream Helm/Axiom/Organism updates are out of scope
for this PR and tracked separately.

### 4. Empty-input embedding policy

**Decision:** Reject empty or whitespace-only input with an explicit error.

```rust
async fn embed(&self, text: &str) -> Result<Embedding> {
    if text.trim().is_empty() {
        return Err(Error::Other("embedding input is empty".into()));
    }
    // ... backend-specific code
}
```

Same check in both impls, same error message. Callers must decide whether to
drop, summarize, or replace empty content before embedding — the provider does
not invent semantic content for missing content.

## Contract surface per trait

### DocumentStore — equivalence

| Test | Assertion |
|---|---|
| `put_then_get` | Put doc, get back identical `Document` (id, data, timestamps) |
| `get_missing_returns_none` | `get` on unknown id → `Ok(None)`, not an error |
| `delete_then_get` | Delete, then get → `Ok(None)` |
| `delete_idempotent` | Delete unknown id → `Ok(())` |
| `put_overwrites_with_advancing_updated_at` | Second put: `created_at` preserved, `updated_at` strictly increases |
| `collections_isolated` | Put in `coll_a`, get from `coll_b` → `None` |
| `query_no_filter_returns_all` | Insert N, default query → returns N |
| `query_eq_filter` | `Filter::Eq` matches subset |
| `query_range_filters` | `Gt`/`Lt`/`Gte`/`Lte` work on numerics and timestamps |
| `query_and_composition` | `And([a, b])` returns intersection |
| `query_or_composition` | `Or([Eq(a), Eq(b)])` returns union — single field only |
| `query_order_by_then_limit` | Order respected, limit truncates |
| `query_updated_after` | Returns only docs with `updated_at > ts` |

**Parity hazard managed:** real Firestore requires composite indexes for some
multi-field `Or` queries; the emulator does not. The `or_composition` test
uses single-field disjunctions only. Multi-field `Or` is therefore not in the
contract — apps that need it must provision the index and test independently.

**Not tested:** ordering when `order_by` is `None`; concurrent-write behavior;
cross-document transactions.

### ObjectStore — equivalence

| Test | Assertion |
|---|---|
| `put_then_get_byte_equal` | Bytes round-trip exactly |
| `put_get_text_roundtrip` | UTF-8 helper preserves string |
| `put_get_json_roundtrip` | JSON helper preserves structure |
| `get_missing_returns_not_found` | `get` on unknown key → `Err(NotFound)` (trait returns `Result<Bytes>`, not `Option`) |
| `exists_reflects_state` | `exists` false → true → false across put/delete |
| `delete_idempotent` | Delete unknown key → `Ok(())` |
| `list_prefix_returns_matching_keys` | `list("a/")` on `{a/1, a/2, b/1}` returns `[a/1, a/2]` |
| `list_prefix_empty_when_no_match` | `list("nope/")` → `Ok(vec![])` |
| `large_payload_roundtrip` | 1 MB payload round-trips byte-equal |

**Not tested:** content-type on read (trait has no `get_metadata`); range or
streaming reads (not in trait); listing order.

### EventLog — equivalence (base trait) + SyncableEventLog (local-only)

| Test | Trait | Assertion |
|---|---|---|
| `append_then_query_returns_event` | base | Append, query matching org/app → event present |
| `query_by_event_type_filter` | base | Type filter excludes other types |
| `query_since_filters_by_occurred_at` | base | `since` excludes earlier events |
| `query_limit_respected` | base | Limit truncates |
| `synced_at_none_on_append` | base | Newly appended events have `synced_at = None` |
| `mark_synced_sets_synced_at` | `SyncableEventLog` | After `mark_synced(ids)`, query with `unsynced_only=true` excludes them |
| `unsynced_only_filters` | `SyncableEventLog` | `unsynced_only=true` excludes already-synced events |

The two `SyncableEventLog` tests run **only on `contract_local.rs`** —
emulator and real-GCP entry points don't construct a `SyncableEventLog`
because remote impls don't satisfy the trait.

**Not tested:** ordering of events with identical `occurred_at`; deletion (the
log is append-only).

### VectorStore — shape + one equivalence carve-out

| Test | Flavor | Assertion |
|---|---|---|
| `upsert_accepts_valid_embedding` | shape | Insert with valid `Embedding` succeeds |
| `search_returns_at_most_top_k` | shape | Insert 10, search top_k=3 → ≤3 results |
| `search_returns_empty_for_empty_namespace` | shape | Search before any upsert → `Ok(vec![])` |
| `self_match_returns_self_first` | equivalence | Upsert V@id="X", search with V top_k=1 → first result.id == "X" |
| `delete_then_search_excludes` | shape | After delete, result set excludes the deleted id |
| `namespaces_isolated` | shape | Vector in ns_a not visible from search in ns_b |
| `match_metadata_preserved` | shape | Returned `Match` carries the metadata that was upserted |

The `Embedding` typed wrapper makes a "wrong-dim rejection" test unnecessary
at runtime — wrong dims fail to compile. The wrapper's constructor is unit-
tested separately, in `runway-storage` itself.

**Not tested:** specific top-k ordering of non-self matches; exact scores;
batched upsert performance.

### EmbeddingProvider — shape only

| Test | Assertion |
|---|---|
| `embed_returns_valid_embedding` | `embed("hello")` returns a typed `Embedding` (length is type-guaranteed) |
| `embed_batch_returns_one_per_input` | `embed_batch(&["a","b"])` returns 2 embeddings |
| `embed_empty_string_rejected` | `embed("")` → `Err(Error::Other("embedding input is empty"))` |
| `embed_whitespace_only_rejected` | `embed("   \n\t")` → same error |

The emulator backend uses fastembed for embeddings (no Vertex AI emulator
exists), so on the emulator entry point this suite tests the same code as the
local entry point. Real fastembed-vs-Vertex parity is only verified by the
real-GCP entry point.

**Not tested:** semantic similarity (model-dependent); determinism across
backends (different models); rate limit / batching efficiency.

## Backend wiring

### `contract_local.rs`

```rust
let tmp = tempfile::tempdir()?;
let kit = StorageKit::local(tmp.path()).await?;
run_all_suites(&kit, "redb", "_contract").await.assert_passed();
```

No external dependencies. Runs on every push.

### `contract_emulator.rs`

Brings up three emulators via `docker-compose.contract.yml`:

| Service | Image / command | Port | Env var |
|---|---|---|---|
| Firestore | `google/cloud-sdk` + `gcloud beta emulators firestore start` | 8080 | `FIRESTORE_EMULATOR_HOST=localhost:8080` |
| Pub/Sub | `google/cloud-sdk` + `gcloud beta emulators pubsub start` | 8085 | `PUBSUB_EMULATOR_HOST=localhost:8085` |
| GCS | `fsouza/fake-gcs-server` | 4443 | `STORAGE_EMULATOR_HOST=http://localhost:4443` |

Required API change: `RemoteStorageKit::build` accepts an optional embedder
override. The emulator entry point passes a fastembed instance; production
callers continue to omit the override and get the Vertex AI default.

### `contract_real_gcp.rs`

`#[tokio::test] #[ignore]`. Runs only with `cargo test -- --ignored` or
`just contract-staging`.

Configuration is read from env:

- `RUNWAY_CONTRACT_PROJECT` — staging GCP project id
- `RUNWAY_CONTRACT_BUCKET` — staging GCS bucket
- `RUNWAY_CONTRACT_REGION` — Vertex AI region

Credentials come from Application Default Credentials (ADC) — `gcloud auth
application-default login` locally, workload identity federation in CI. No
service account JSON keys.

**Namespacing:** every run uses `_contract/<uuid>/` as a prefix for Firestore
collection names, GCS object keys, and a `_contract-<uuid>-` prefix for
Pub/Sub topics. Per-test teardown deletes the namespace best-effort (swallows
errors so the original failure is what the developer sees).

**Safety net:** a Terraform-managed Cloud Scheduler job sweeps `_contract/`
debris older than 24 hours from the bucket and Firestore. Catches the cases
where the test process crashes before teardown.

## DX & CI integration

### `just` recipes

```just
contract: contract-local contract-emulator

contract-local:
    cargo test -p runway-storage --test contract_local

contract-emulator:
    docker compose -f crates/runway-storage/tests/docker-compose.contract.yml up -d --wait
    -cargo test -p runway-storage --test contract_emulator
    docker compose -f crates/runway-storage/tests/docker-compose.contract.yml down

contract-staging:
    @test -n "$RUNWAY_CONTRACT_PROJECT" || (echo "set RUNWAY_CONTRACT_PROJECT" && exit 1)
    cargo test -p runway-storage --test contract_real_gcp -- --ignored --nocapture

contract-all: contract contract-staging
```

The leading `-` on `cargo test` in `contract-emulator` ensures
`docker compose down` runs even when tests fail.

### GitHub Actions

**`contract.yml`** — on every PR and push to `main`:
- Runs `just contract` (local + emulator)
- Cargo registry and `target/` cached
- 10-minute timeout
- Required check before merge

**`contract-staging.yml`** — on tags `release/*` and `workflow_dispatch`:
- Authenticates via workload identity federation
- Sets `RUNWAY_CONTRACT_*` env vars from repo secrets
- Runs `just contract-staging`
- NOT a required check for PRs (cost + cold-cache latency)
- IS a required check for the release pipeline — a failure blocks production deploy

## Documentation drift cleanup

Stale references to be updated as part of this work:

- `crates/runway-storage/Cargo.toml:3` — description mentions SQLite + LanceDB
- `crates/runway-storage/src/lib.rs:33` — `local()` doc comment
- `crates/runway-storage/src/traits/document.rs:95` — mentions "SQLite table row"
- `crates/runway-storage/src/traits/event.rs:36` — mentions "SQLite WAL"
- `crates/runway-storage/src/traits/vector.rs:22` — mentions "LanceDB table name"

All should reference the current stack: redb for documents/events/vectors,
local FS for objects, fastembed for embeddings.

## Open questions

None. The four trait-refinement TBDs were settled during design. Downstream
callers (Helm/Axiom/Organism) will need migration for the `Embedding` wrapper
and `SyncableEventLog` split — that work lives in those repos and is tracked
separately.

## Success criteria

- All five suite functions compile, run, and pass against the local backend
- The emulator suite runs in CI on every PR, passes, and completes in <5 min
- The real-GCP suite runs on demand via `just contract-staging`, passes, and
  cleans up its namespace
- A deliberate divergence introduced into either backend impl is caught by
  the suite with a clear `contract violation [Trait @ backend]:` message
- The four trait refinements compile across the runway repo with `just lint`
  passing
- All five stale SQLite/LanceDB doc references are updated
