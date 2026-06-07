# Reflective Runtime Runway

Distribution, deployment, and infrastructure for the [Converge](https://github.com/Reflective-Lab/converge) stack.

Runtime Runway owns everything needed to **run, package, and deploy** Reflective
apps that embed Converge. The Converge SDK stays pure; Runtime Runway handles
the messy reality of binaries, containers, GPUs, app hosts, auth, storage,
secrets, telemetry, and cloud services.

## Boundary

> Owns: auth, app host, storage, secrets, telemetry, deployment runtime, LLM/GPU paths, **managed-service wrappers** (Pub/Sub, Spanner, Memorystore). Does NOT own: governance (→ Converge); commercial state (→ Commerce Rails); in-process distributed consensus (→ Lattice Mesh).

— Canonical claim: [Runtime Runway](https://github.com/Reflective-Lab/reflective/blob/main/KB/04-architecture/current-system-map.md#runtime-runway) in the boundary registry. Update there first; this README quotes that source.

## Relationship to Commerce Rails

Runtime Runway and [Commerce Rails](../commerce-rails/) are sibling authorities with a clean boundary:

| Question | Owner |
|---|---|
| Who can log in? Where does code run? Where do secrets live? | **Runtime Runway** |
| Who pays? What is owed? What is granted? What must be reconciled? | **Commerce Rails** |

Runtime Runway owns canonical users, orgs, auth, membership, deployments, secrets, and runtime substrate. Commerce Rails owns subscriptions, entitlements, billing, revenue-share, payouts, and reconciliation.

Stripe crosses both: Runtime Runway routes the webhook, holds the signing secret, and provides runtime observability. Commerce Rails verifies the provider semantics, records receipts, and decides commercial state.

See [`kb/Architecture/Commerce Rails Boundary.md`](kb/Architecture/Commerce%20Rails%20Boundary.md) for the full authority table.

---

## A New World

The old world shipped instructions; the new world ships intent-driven, governed runtimes. Models and orchestration turn declared intent into decisions at runtime — but only if the runtime, the providers, the GPUs, and the deployment surface actually exist in the messy real world. Runtime Runway owns that messy world.

**Why it matters.** A doctrine of safe runtime intent resolution requires a runtime that can actually be deployed, run, and reasoned about on real hardware. Runtime Runway is the boundary between the pure SDK upstairs and the binaries, containers, and GPUs that make the rest of the stack real.

## Architecture

```
reflective/runtime-runway/
  crates/
    application/        The `converge` CLI/TUI binary
    llm/                Local LLM inference (Burn, llama.cpp)
    api-server/         Cloud Run reference binary (wires all runway-* crates)
    runway-accounts/    Users, orgs, invites, roles, and billing mirror
    runway-auth/        Firebase Auth middleware (Tower Layer, offline JWKS)
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

Runtime Runway **consumes** Converge crates via path — never the reverse.

```
reflective/runtime-runway/crates/application  ──>  converge/crates/{core, experience, provider, ...}
reflective/runtime-runway/crates/llm          ──>  converge/crates/{core, domain, provider, storage}
reflective/runtime-runway/crates/runway-*     ──>  (no converge dependency — standalone infra crates)
```

Local SDK work expects Converge at `~/dev/reflective/bedrock-platform/converge`.

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

### runway-accounts

Users, organisations, team invites, roles, and the billing entitlement mirror — the canonical identity and membership layer. Stripe provider behavior lives in Commerce Rails.

Routes (all under `runway_accounts::protected_routes()`, behind `AuthLayer`):

| Method | Path | Auth |
|--------|------|------|
| `GET` | `/v1/accounts/me` | any token — provisions on first access |
| `GET` | `/v1/orgs/:org_id` | member or billing owner |
| `GET` | `/v1/orgs/:org_id/members` | admin |
| `DELETE` | `/v1/orgs/:org_id/members/:uid` | admin (billing owner protected) |
| `POST` | `/v1/orgs/:org_id/invites` | admin |
| `GET` | `/v1/orgs/:org_id/invites` | admin |
| `POST` | `/v1/invites/:token/accept` | any authed user |
| `GET` | `/v1/billing/summary` | any member |
| `POST` | `/v1/billing/checkout` | any member |
| `POST` | `/v1/billing/portal` | admin |

The Stripe webhook (`/v1/billing/webhooks/stripe`) is public, HMAC-verified internally, and mounted via `runway_accounts::public_routes()`.

**Runtime Runway owns identity and transport.** The commercial meaning of subscription events — what the org is entitled to, revenue-share, payout — belongs to [Commerce Rails](../commerce-rails/).

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

## Runtime Ownership

The standalone `converge-runtime` service is retired as the canonical deployed
runtime. It remains available only as a legacy compatibility shell for old
smoke tests and scripts. Current services should deploy through Runtime
Runway's app-host/app-backend path, such as `api-server` or a thin application
backend.

The old `deploy-cloud-run` script is guarded by
`ALLOW_LEGACY_CONVERGE_RUNTIME_DEPLOY=true` so accidental deployments of the
retired service fail fast.

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
| Google Cloud Run (api-server/app backend) | `just api-deploy` or app deploy recipe | Current |
| Google Cloud Run (legacy converge-runtime) | `ALLOW_LEGACY_CONVERGE_RUNTIME_DEPLOY=true just deploy-cloud-run` | Retired compatibility only |
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
just dev-up         # start legacy local converge-runtime compatibility shell
just smoke-test     # verify legacy shell health
just dev-down       # stop legacy shell
```

See the [knowledge base](kb/Home.md) for full documentation.

## Design Principles

- Runtime Runway **consumes** Converge crates, never contributes to the SDK
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
