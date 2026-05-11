# Reflective Runway

Distribution, deployment, and infrastructure for the [Converge](https://github.com/Reflective-Lab/converge) stack.

Runway owns everything needed to **run, package, and deploy** Converge. The SDK stays pure; Runway handles the messy reality of binaries, containers, GPUs, and cloud services.

## A New World

The old world shipped instructions; the new world ships intent-driven, governed runtimes. Models and orchestration turn declared intent into decisions at runtime — but only if the runtime, the providers, the GPUs, and the deployment surface actually exist in the messy real world. Runway owns that messy world.

**Why it matters.** A doctrine of safe runtime intent resolution requires a runtime that can actually be deployed, run, and reasoned about on real hardware. Runway is the boundary between the pure SDK upstairs and the binaries, containers, and GPUs that make the rest of the stack real.

## Architecture

```
reflective/runway/
  crates/
    application/        The `converge` CLI/TUI binary
    llm/                Local LLM inference (Burn, llama.cpp)
    runway-auth/        Firebase Auth middleware (Tower Layer)
    runway-middleware/  Axum request-id, trace, CORS, compression stack
    runway-secrets/     GCP Secret Manager client (SecretString, zeroized)
    runway-storage/     StorageKit — DocumentStore, VectorStore, ObjectStore, EventLog, EmbeddingProvider
    runway-telemetry/   OTel → Cloud Trace, Sentry, JSON logging
  docker/               Container definitions (Dockerfile, compose)
  ops/
    infra/terraform/    GCP Terraform modules (Firestore, GCS, Vertex AI, Pub/Sub, etc.)
    infra/firebase/     Firestore security rules and indexes
    deploy/             GPU deployment (Cloud Run, RunPod, Modal)
    scripts/            Dev lifecycle scripts
```

### Two Runtime Modes — Same Code

The `runway-storage` crate's `StorageKit` is the central hinge:

```
StorageKit::local(base_path)   →  redb + file vectors + local FS   (Tauri offline)
StorageKit::remote(config)     →  Firestore + GCS + Vertex AI       (Cloud Run)
```

Apps call `StorageKit::local()` or `StorageKit::remote()` at startup. All Converge loops,
Suggestors, and domain logic receive a `StorageKit` and never care which backend is live.

### GCP Stack

| Service | Purpose |
|---------|---------|
| Firestore | Document store — multi-tenant `orgs/{orgId}/apps/{appId}/...` hierarchy |
| Cloud Storage | Object store — binaries, assets, model weights |
| Vertex AI Matching Engine | ANN vector search at cloud scale (768-dim embeddings) |
| Vertex AI `text-multilingual-embedding-002` | Embeddings (multilingual, 768-dim) |
| Cloud Pub/Sub | Event ingestion pipeline, feed processing |
| BigQuery | Analytics — `learning_episodes` + `experience_events` |
| Cloud Spanner | Multi-region ACID for billing and governance |
| Memorystore (Redis) | Distributed locks, rate limiting, sessions |
| Secret Manager | All secrets: API keys, signing certs, Stripe webhook keys |

### Dependency Direction

Runway **consumes** Converge crates via path — never the reverse.

```
reflective/runway/crates/application  ──>  converge/crates/{core, experience, provider, ...}
reflective/runway/crates/llm          ──>  converge/crates/{core, domain, provider, storage}
reflective/runway/crates/runway-*     ──>  (no converge dependency — standalone infra crates)
```

Local SDK work expects Converge at `~/dev/reflective/stack/bedrock-platform/converge`.

## Crates

### converge-application

The `converge` binary — packages domain packs, providers, and runtime into a deployable CLI/TUI.

| Command | Purpose |
|---------|---------|
| `tui` | Interactive terminal UI (ratatui) |
| `packs` | Domain pack management |
| `run` | Execute jobs from templates |
| `eval` | Reproducible test fixtures |

Optional features: `tui` (default), `knowledge`, `llm`, `analytics`, `optimization`, `full`.

### converge-llm

Local LLM inference for Converge agents using pure Rust frameworks.

| Engine | Model | Framework | GPU |
|--------|-------|-----------|-----|
| `LlamaEngine` | Llama 3.2 | llama-burn | CUDA, Metal, CPU |
| `GemmaEngine` | Google Gemma | llama-cpp-2 | Metal, CPU |
| `TinyLlamaEngine` | TinyLlama | Burn | CPU |
| `GrpcBackend` | Any | Tonic | Remote GPU |

Features: `ndarray` (default), `wgpu`, `gemma`, `lora`, `server`, `grpc-client`, `recall`, `semantic-embedding`, `anthropic`.

### runway-storage

Shared storage abstraction for all Reflective apps. One trait set, two backends.

```rust
// Tauri desktop (offline-first)
let kit = StorageKit::local("~/.inkling").await?;

// Cloud Run backend
let kit = StorageKit::remote(RemoteConfig::from_env()?).await?;
```

| Trait | Local | Remote |
|-------|-------|--------|
| `DocumentStore` | redb (ACID embedded) | Firestore REST v1 |
| `VectorStore` | redb + brute-force cosine | Vertex AI Matching Engine |
| `ObjectStore` | Local filesystem (atomic write) | GCS JSON API |
| `EventLog` | redb (dedup + unsynced index) | Firestore subcollection |
| `EmbeddingProvider` | fastembed AllMiniLML6V2 (384→768-padded) | Vertex AI text-multilingual-embedding-002 |

Offline vectors are zero-padded to 768 dims for index compatibility with remote Vertex AI vectors.
When the Tauri app goes online, it re-embeds via VertexEmbedder to replace approximations.

### runway-auth

Firebase Auth Tower middleware. Validates Bearer tokens via Identity Toolkit, injects `AuthContext`
with `{ uid, email, org_id, apps: Vec<String>, role }` custom claims into Axum handlers.

### runway-middleware

Axum middleware stack: request-id (UUID), OTel trace, gzip compression, CORS, JSON error formatter,
graceful SIGTERM shutdown, `/health` endpoint.

### runway-secrets

GCP Secret Manager client. Secrets are named `{env}-{app}-{key}`. All values held as `SecretString`
(zeroized on drop). `SecretMap::require()` fails fast at startup for missing secrets.

### runway-telemetry

OpenTelemetry → Cloud Trace (OTLP/HTTP), Sentry error tracking, JSON structured logging → Cloud Logging.
Returns a `TelemetryGuard` that shuts down the tracer provider on drop.

## Building

Requires: Rust 1.94+, `just`, and the `converge` repo as a sibling.

```bash
just build              # cargo build --release
just build-quick        # fast iteration (quick-release profile)
just check              # cargo check --workspace
just test               # cargo test --all-targets
just lint               # fmt + clippy
just fix-lint           # auto-fix
```

## Deployment

### Local

```bash
cargo run -p converge-application                      # native
cargo run -p converge-application --features full      # all features
cd docker && docker compose up                          # containerized
```

### Cloud

| Target | Method | Status |
|--------|--------|--------|
| Google Cloud Run (runtime) | `just deploy-cloud-run` | Script-based |
| Google Cloud Run (GPU) | `ops/deploy/gpu/cloudrun/deploy.sh` | Script-based |
| RunPod (GPU) | `ops/deploy/gpu/runpod/` | Dockerfile ready |
| Modal (GPU) | `ops/deploy/gpu/modal/` | Stub |

### GPU Worker

The `converge-llm-server` binary hosts Burn engines behind gRPC. Clients connect via `GrpcBackend`, keeping GPU hardware separate from the convergence engine.

```bash
# Build the server
cargo build -p converge-llm --bin converge-llm-server --features server

# Deploy to Cloud Run with GPU
PROJECT_ID=my-project ./ops/deploy/gpu/cloudrun/deploy.sh
```

## Development Workflow

```bash
just focus          # session opener — repo health
just sync           # PRs, issues, build status
just dev-up         # start local runtime
just smoke-test     # verify health
just dev-down       # stop runtime
```

See the [knowledge base](kb/Home.md) for full documentation.

## Design Principles

- Runway **consumes** Converge crates, never contributes to the SDK
- `unsafe` code is forbidden (`unsafe_code = "forbid"`)
- Infrastructure is imperative scripts today, IaC later
- GPU workers are separated from the main runtime
- Everything proprietary (`LicenseRef-Proprietary`, `publish = false`)
- Edition 2024, Rust 1.94, Clippy pedantic

## Security

See [SECURITY.md](SECURITY.md) for vulnerability reporting and security practices.

## License

Proprietary. Copyright 2024-2026 Reflective Group AB. All rights reserved.

See [LICENSE](LICENSE) for details.
