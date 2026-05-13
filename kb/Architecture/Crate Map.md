---
source: llm
---
# Crate Map

Runway hosts two categories of crates: Converge distribution crates (application and LLM) and the shared infrastructure crates (`runway-*`). The runway-* crates have no Converge dependency — they are standalone infra primitives reused by all Reflective apps.

`api-server` is the reference binary that wires all five runway-* crates together and proves the Cloud Run deployment path.

## Crates

```
api-server               → all runway-* crates            Reference Cloud Run service;
                                                          proves deployment end-to-end

converge-application     → converge-core, converge-experience,    CLI/TUI distribution
                           converge-provider + optional subsystems
converge-llm             → converge-core, converge-domain          Local LLM inference (Burn)

runway-accounts          → runway-{auth,storage}, reqwest, hmac    Account + org + Stripe billing
runway-storage           → redb, reqwest, fastembed, serde_json    StorageKit: DocumentStore +
                                                                    VectorStore + ObjectStore +
                                                                    EventLog + EmbeddingProvider
runway-auth              → reqwest, axum, tower                    Firebase Auth Tower middleware
runway-middleware        → axum, tower-http                        Request-id, trace, CORS,
                                                                    compression, /health, serve()
runway-secrets           → reqwest, secrecy, zeroize               GCP Secret Manager client
runway-telemetry         → opentelemetry, sentry, tracing          OTel → Cloud Trace + Sentry
```

## Dependency direction

```
reflective/runway/crates/api-server   ──→  runway-{storage, auth, middleware, secrets, telemetry}
reflective/runway/crates/application  ──→  converge/crates/{core, experience, provider, ...}
reflective/runway/crates/llm          ──→  converge/crates/{core, domain, provider, storage}
reflective/runway/crates/runway-*     ──→  (no converge dependency)
```

## runway-* crate reference

### runway-accounts

User, organisation, and Stripe billing management. Exposed as two router bundles:

```rust
runway_accounts::public_routes(state)     // POST /v1/billing/webhooks/stripe (HMAC-verified)
runway_accounts::protected_routes(state)  // /v1/accounts/me, /v1/orgs/:id, /v1/billing/*
```

Data is stored in Firestore (production) or redb (LOCAL_DEV) via `runway-storage`. Custom claims are updated via the Firebase Admin REST API after subscription changes. See [[Architecture/Security]] for the full auth model.

### runway-storage

Two-mode `StorageKit` — same API, backend selected at startup:

```rust
StorageKit::local(base_path)   // Tauri: redb + local FS + fastembed
StorageKit::remote(config)     // Cloud Run: Firestore + GCS + Vertex AI
```

| Trait | Local | Remote |
|-------|-------|--------|
| `DocumentStore` | redb (ACID, WAL) | Firestore REST v1 |
| `VectorStore` | redb + brute-force cosine | Vertex AI Matching Engine |
| `ObjectStore` | local FS (atomic write) | GCS JSON API |
| `EventLog` | redb + UNSYNCED index | Firestore subcollection |
| `EmbeddingProvider` | fastembed 384-dim → zero-padded 768 | Vertex AI text-multilingual-embedding-002 768-dim |

Embedding standard: 768-dim everywhere. Offline vectors are approximate; replaced by exact Vertex AI embeddings on sync.

### runway-auth

Firebase Auth Tower layer. Verifies Bearer tokens via Identity Toolkit, injects `AuthContext { uid, org_id, apps, role }` into Axum handlers via `Extension<AuthContext>`.

```rust
AuthLayer::new(FirebaseAuth::new(api_key))
    .requiring_app("inkling")   // optional app-level entitlement check
```

`LOCAL_DEV=true` + `Bearer dev` → injects canned `AuthContext` without hitting Firebase.

### runway-middleware

Attaches the full HTTP stack to any Axum router and serves it on `PORT` (default 8080):

```rust
let app = stack(router);   // adds /health, request-id, OTel span, gzip, CORS, JSON error body
serve(app).await;           // binds PORT, graceful SIGTERM
```

`ROUTE_PREFIX` env var: if set, routes are mounted under that prefix (e.g. `/api-server`).

### runway-secrets

GCP Secret Manager client. Secrets named `{env}-{app}-{key}` or `{env}-platform-{key}`.

```rust
let secrets = Secrets::from_env()?;          // reads PROJECT_ID, ENV, APP from env
let key = secrets.get("firebase-api-key").await?;   // fetches + caches
```

`SecretString` is zeroized on drop. `SecretMap::require(key)` panics fast on missing secrets at startup.

### runway-telemetry

OTel tracing → Cloud Trace (OTLP/HTTP), Sentry error tracking, JSON structured logs. Call once at `main()` top, hold the guard for process lifetime:

```rust
let _guard = runway_telemetry::init(TelemetryConfig::from_env("api-server"))?;
```

`TelemetryGuard` flushes spans and Sentry events on drop (clean shutdown).

---

## converge-llm engines

| Engine | Framework | GPU Support | Models |
|--------|-----------|-------------|--------|
| `LlamaEngine` | llama-burn | CUDA, Metal, CPU | Llama 3.2, LoRA adapters |
| `GemmaEngine` | llama-cpp-2 | Metal, CPU | Google Gemma (GGUF) |
| `TinyLlamaEngine` | Burn | CPU | Resource-constrained |
| `GrpcBackend` | Tonic | Remote GPU | Offload to GPU server |

## Feature matrix (converge-llm)

| Feature | What |
|---------|------|
| `ndarray` (default) | CPU backend |
| `wgpu` | Metal/Vulkan GPU |
| `gemma` | Gemma GGUF inference |
| `lora` | LoRA adapter fine-tuning |
| `server` | gRPC inference server |
| `grpc-client` | gRPC client |
| `recall` | Experience recall |
| `semantic-embedding` | ONNX embedding models |
| `storage` | Remote adapter registry |
| `anthropic` | Anthropic provider bridge |

Runway pins Converge dependencies to Git tag `v3.4.0` by default.
For local SDK work: `just use-local-converge` → patches to `../reflective/stack/bedrock-platform/converge`.

See also: [[Building/Deployment]], [[Stack/Burn and Local LLM]], converge `kb/Architecture/Crate Map`
