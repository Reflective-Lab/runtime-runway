# Runway/Helm App-Host Boundary — Design

**Date:** 2026-05-28
**Status:** Draft for review
**Scope:** Lock the canonical app execution container across Runway, Helm, and a thin app. Prove it end-to-end with Catalyst on Cloud Run.

---

## 1. Goal

Stop the drift toward app-owned backend servers. Runway provides the standard execution container. Helm becomes a pure library of reusable "App + Operator" patterns. Each app is a thin binary that composes Runway + Helm modules + its own domain content.

This spec covers three interlocking workstreams that ship together:

1. **Mount contract** — define the `HelmModule` trait and supporting `HostContext` in `runway-app-host` so modules slot into Runway cleanly.
2. **Helm reshape** — strip host scaffolding out of `helms/crates/application-server`, extract reusable modules as their own crates, decommission `application-server`.
3. **Showcase + Catalyst proof** — relocate CRM showcase code to a new `stack/atelier-showcase` workspace, ship `catalyst-backend` as the first thin app on the new contract, deploy to Cloud Run.

---

## 2. Why now

- Three Immediate-Priority deliverables in `MILESTONES.md` block on each other; staggering them across separate cycles risks refactoring blind toward a contract that hasn't been pinned down.
- Helm's `application-server` currently mixes host infrastructure, reusable patterns, and CRM-specific showcase content. Without untangling, every new app risks copying the same monolithic shape.
- Catalyst's runway proof (`marquee-apps/kb/catalyst-runway-proof.md`) is at P3 — the unfinished step is exactly "name the deployable backend binary and wire it through Runway." This spec answers it.

---

## 3. The four-layer model (canonical)

```
┌─────────────────────────────────────────────────────────┐
│  App (thin)            JTBD + UX + domain content       │
│                        e.g. Catalyst, Wolfgang, Inkling │
├─────────────────────────────────────────────────────────┤
│  Helm                  Reusable "App + Operator" patterns
│                        helm-operator-control,            │
│                        helm-governed-jobs,               │
│                        helm-truth-execution              │
├──────────────────────┬──────────────────────────────────┤
│  Runway              │  Movement                         │
│  Ops authority       │  Commercial authority             │
│  telemetry, GCP,     │  subscriptions, billing,          │
│  storage, auth,      │  entitlements, Stripe Connect     │
│  middleware,         │  → commerce-rails                 │
│  runway-app-host     │                                   │
└──────────────────────┴──────────────────────────────────┘
```

### Authority and dependency direction

- **App** imports Runway + Helm modules. Eventually also imports Movement.
- **Helm** imports Runway. Never imports apps, Movement, or other Helm modules unless explicitly reusing one.
- **Runway** imports nothing above it. Never knows what app or what Helm module is running.
- **Movement** is a peer authority to Runway. Out of scope for this spec — referenced only.

### Repository map

| Repo | Purpose | Touched by this spec |
|---|---|---|
| `runway/` | Runway crates + `runway-app-host` | Yes — extended |
| `stack/bedrock-platform/helms/` | Helm module crates | Yes — restructured |
| `stack/atelier-showcase/` | Non-reusable showcase apps | Yes — bootstrapped |
| `marquee-apps/catalyst-biz/` | Catalyst app | Yes — backend added |
| `movement/commerce-rails/` | Commercial authority | No — referenced only |

---

## 4. `runway-app-host` additions

The existing `RunwayAppHost`, `AppExecutionPacket`, and packet metadata types stay. This spec adds the runtime mount machinery they were always anticipating.

### 4.1 The `HelmModule` trait

```rust
// crates/runway-app-host/src/module.rs
#[async_trait]
pub trait HelmModule: Send + Sync + 'static {
    fn module_id(&self) -> &'static str;
    async fn init(&self, ctx: &HostContext) -> anyhow::Result<()>;
    fn router(self: Arc<Self>) -> axum::Router { axum::Router::new() }
    fn grpc_services(self: Arc<Self>) -> Vec<TonicService> { vec![] }
}
```

Modules implementing this trait are constructed by the app (with their own deps), passed to the host as `Arc<dyn HelmModule>`, and registered via the builder.

- `init()` runs once after the host is fully built but before `serve()`. Modules use it to subscribe to the hub, register typed event schemas, validate startup invariants.
- `router()` returns an Axum router; the host nests it under the packet's `route_prefix`.
- `grpc_services()` returns Tonic services; the host registers them with its shared Tonic server.
- A module that only speaks HTTP overrides `router()`. A pure-gRPC module overrides `grpc_services()`. Mixed modules implement both.

### 4.2 `HostContext`

```rust
// crates/runway-app-host/src/context.rs
pub struct HostContext {
    pub packet:    Arc<AppExecutionPacket>,
    pub storage:   StorageKit,
    pub auth:      AuthPolicy,
    pub realtime:  EventHubHandle,
    pub telemetry: TelemetryHandle,
}
```

Modules receive `&HostContext` in `init()` and clone the handles they need into their own state. No global statics.

`AuthPolicy` exposes helpers like `requires_app(app_id)` and `requires_claim(name, value)` returning Tower layers modules apply to their routers.

### 4.3 `EventHub` — canonical realtime infrastructure

```rust
// crates/runway-app-host/src/realtime.rs
pub struct EventHub { /* tokio::sync::broadcast, 512 capacity */ }
pub struct EventEnvelope {
    pub event_id: Uuid,
    pub sequence: u64,
    pub r#type: String,
    pub schema_version: u32,
    pub occurred_at: DateTime<Utc>,
    pub app_id: String,
    pub run_id: Option<String>,
    pub correlation_id: Option<String>,
    pub actor: Option<String>,
    pub payload: serde_json::Value,
}
```

Envelope shape matches the contract documented in `marquee-apps/kb/catalyst-runway-proof.md` § Realtime Contract.

`EventHubHandle` is the cheaply-cloneable handle stored on `HostContext` — modules clone it freely into their own state and background tasks. The underlying `EventHub` lives in the host and is dropped on shutdown.

**One hub per app runtime, owned by Runway.** Connection lifecycle, auth context, CORS, heartbeats, backpressure, tracing, shutdown are host concerns. Modules publish typed events through `ctx.realtime.publish(...)` and subscribe via `ctx.realtime.subscribe()`.

The host mounts one canonical SSE projection at:

```
GET  {route_prefix}/sse/stream
```

Modules MAY expose filtered projection routes (e.g. `GET /v1/jobs/{key}/stream`) — these are filters over the same hub, not separate hubs. A module never constructs its own broadcaster.

### 4.4 HITL approvals — transport vs semantics

Runway owns the transport. Modules own the semantics.

The host mounts:

```
POST {route_prefix}/v1/approvals/{ref}/approve
POST {route_prefix}/v1/approvals/{ref}/reject
```

These routes validate auth, persist a receipt event through `StorageKit::events`, and publish `approval.approved` / `approval.rejected` envelopes through the hub. They do **not** know what the approval is for.

Modules that gate work behind approvals subscribe to the hub for `approval.*` events in their `init()` and react. No handler registration; no inversion of control through the host.

### 4.5 Host-mounted endpoints (full list)

```
GET  {route_prefix}/status      — packet metadata (already exists)
GET  {route_prefix}/healthz     — process liveness
GET  {route_prefix}/sse/stream  — canonical event projection
POST {route_prefix}/v1/approvals/{ref}/approve
POST {route_prefix}/v1/approvals/{ref}/reject
```

### 4.6 Builder + serve

```rust
let host = RunwayAppHost::builder(packet)
    .with_storage(storage)
    .with_secrets(secrets)
    .mount(Arc::new(OperatorControlModule::new(deps)))
    .mount(Arc::new(GovernedJobsModule::new(deps)))
    .build()           // calls init() on each module in registration order
    .await?
    .serve()           // tokio::try_join! over Axum HTTP + Tonic gRPC
    .await
```

`serve()` runs Axum HTTP and Tonic gRPC concurrently on configured ports. If a module exposes no gRPC services, the Tonic server is not started.

---

## 5. Helm reshape — pure library

By the end of this spec, `stack/bedrock-platform/helms/crates/application-server` is **deleted**. Helm becomes a Cargo workspace of reusable module crates with no binaries.

### 5.1 New module crates

| Crate | Extracted from `application-server` | Surface |
|---|---|---|
| `helm-operator-control` | `src/.../operator-control` + `http_api.rs` + `PipelineState` | Axum routes for truth preview/execution UI |
| `helm-governed-jobs` | `src/.../job_stream` + `JobStreamState` | Axum routes incl. `/v1/jobs/{key}/stream` (filtered projection over host hub) |
| `helm-truth-execution` | `src/.../truth_runtime` (dispatcher framework only — *not* the truth bodies) | Axum route `/v1/truths/{key}/execute`, registry trait for truth implementations |

Each implements `HelmModule`. Each holds its own state. None reaches outside its crate for shared mutable state — anything cross-module flows through the hub.

### 5.2 What moves out of Helm entirely

| Concern | Destination | Why |
|---|---|---|
| Health, CORS, tracing, request-id scaffolding | Deleted; covered by `runway-app-host` + `runway-middleware` | Already host concerns |
| `RealtimeHub` | Replaced by `runway-app-host::EventHub` | Hub is platform infrastructure |
| Approvals routes `/v1/approvals/{ref}/...` | Moved to `runway-app-host` | Transport is host concern |
| CRM business services (Opportunities, Parties, Workflow, Conversations, Documents, Facts, Metadata, Workbench dashboard) | Moved to `stack/atelier-showcase` (see § 6) | Not reusable patterns |
| CRM truth bodies (`score-inbound-fit`, `qualify-inbound-lead`, 17+ others) | Moved to `stack/atelier-showcase/crates/crm-truths` | Showcase content, not platform |
| Subscriptions / billing routes | Deleted | Belongs in Movement (`commerce-rails`) — future spec |

### 5.3 What stays Helm-internal

- Helm's existing dependencies (`application-kernel`, `application-storage`, `capability-*`, `truth-catalog`, `converge-*`, `organism-pack`) keep their current shape. This spec touches their consumers, not them.
- Helm's TypeScript packages (`packages/helm-flow/*`) are wire-compatible with the new envelope; no JS/TS changes required.

---

## 6. `stack/atelier-showcase` bootstrap

New workspace at `/Users/kpernyer/dev/reflective/stack/atelier-showcase/`. Mirrors Helm's conventions (Cargo workspace, `crates/` directory, top-level `README.md`, `CLAUDE.md`, `MILESTONES.md`).

### 6.1 Layout

```
stack/atelier-showcase/
├── Cargo.toml                # workspace
├── README.md
├── CLAUDE.md
├── MILESTONES.md
├── crm-showcase/             # the binary — a Helm-style demo app
│   ├── Cargo.toml
│   └── src/main.rs
└── crates/
    ├── crm-opportunities/    # was OpportunitiesGrpc
    ├── crm-parties/          # was PartiesGrpc + Organizations HTTP
    ├── crm-workflow/         # was WorkflowGrpc (lead/case state machines)
    ├── crm-conversations/    # was ConversationsGrpc
    ├── crm-documents/        # was DocumentsGrpc
    ├── crm-facts/            # was FactsGrpc (immutable audit log)
    ├── crm-metadata/         # was MetadataGrpc (schema defs)
    ├── crm-workbench/        # was /v1/workbench/dashboard
    └── crm-truths/           # 17+ CRM truth bodies, registered with helm-truth-execution
```

Each `crm-*` crate is a `HelmModule` impl with both HTTP and gRPC surfaces as needed. `crm-showcase/src/main.rs` builds an `AppExecutionPacket` for the CRM demo and mounts all `crm-*` modules + the relevant `helm-*` modules.

### 6.2 Symmetry with Catalyst

Catalyst and CRM-showcase are both thin app binaries composing the same primitives. They differ only in:
- which Helm modules they mount,
- which truth bodies they register,
- their `AppExecutionPacket` (id, route_prefix, version),
- their frontends (separate repos).

This symmetry is the proof that the contract works for two very different domains. If a third app (Wolfgang, Inkling, Folio) reaches the same shape later, the contract holds.

### 6.3 Frontend impact

CRM showcase's existing gRPC-web frontend code (currently bundled with Helm or marquee-apps) keeps working — the gRPC services move crates but keep their proto contracts. Wire format unchanged.

---

## 7. Catalyst backend shape

### 7.1 Location

New crate at `marquee-apps/catalyst-biz/backend/` added to Catalyst's workspace.

### 7.2 `main.rs`

```rust
// marquee-apps/catalyst-biz/backend/src/main.rs
use std::sync::Arc;
use runway_app_host::{AppExecutionPacket, MountedModule, RunwayAppHost};
use runway_storage::{StorageKit, remote::RemoteConfig};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    runway_telemetry::init("catalyst-backend")?;

    let packet = AppExecutionPacket::new(
            "catalyst",
            "Catalyst",
            "Sales pipeline proof — score-inbound-fit through HITL",
            "/")
        .with_version(env!("CARGO_PKG_VERSION"))
        .with_auth_app("catalyst")
        .with_mounted_module(MountedModule::new("helm.operator-control", /* routes */))
        .with_mounted_module(MountedModule::new("helm.governed-jobs",    /* routes */))
        .with_mounted_module(MountedModule::new("helm.truth-execution",  /* routes */));

    let secrets = runway_secrets::Secrets::load_all().await?;
    let storage = StorageKit::remote(RemoteConfig::from_env()?).await?;

    let truths = helm_truth_execution::Module::new()
        .register(catalyst_truths::score_inbound_fit())
        .register(catalyst_truths::qualify_inbound_lead())
        .register(catalyst_truths::schedule_strategic_meetings());

    RunwayAppHost::builder(packet)
        .with_storage(storage)
        .with_secrets(secrets)
        .mount(Arc::new(helm_operator_control::Module::new(&storage)))
        .mount(Arc::new(helm_governed_jobs::Module::new(&storage)))
        .mount(Arc::new(truths))
        .build()
        .await?
        .serve()
        .await
}
```

Catalyst's truth bodies live in a sibling crate `marquee-apps/catalyst-biz/truths/`. The backend binary is JTBD/UX-agnostic wiring.

### 7.3 Local proof flow

Driven by `just catalyst-local`:

1. `cargo run -p catalyst-backend` against `StorageKit::local("~/.catalyst")`.
2. Frontend posts `POST /v1/jobs/score-inbound-fit/stream`.
3. SSE on `/sse/stream` shows `job.started → gate.paused` envelopes.
4. Operator hits `POST /v1/approvals/{ref}/approve`.
5. SSE shows `approval.approved → job.resumed → job.completed`.
6. `EventLog::query(app_id="catalyst", run_id=…)` returns the full receipt trail.

This is the on-machine equivalent of the proof's acceptance criteria.

---

## 8. Cloud Run deploy

### 8.1 Terraform

New module `ops/infra/terraform/catalyst-backend/`:
- `google_cloud_run_v2_service` for the backend.
- `google_service_account` `catalyst-backend@…`.
- IAM bindings: Firestore user, GCS object admin (scoped bucket), Pub/Sub publisher (scoped topic), Vertex AI user, Secret Manager accessor.
- Outputs: service URL, service account email, image artifact path.

### 8.2 Secrets

Populated in Secret Manager before first deploy:
- `prod-catalyst-firebase-api-key`
- `prod-catalyst-pubsub-topic`
- (shared `prod-platform-*` already exist)

### 8.3 Image + deploy

- New `marquee-apps/catalyst-biz/backend/Dockerfile` — multistage Rust build, distroless final.
- `just deploy-catalyst` in `runway/justfile` — builds, pushes to Google Artifact Registry, runs `gcloud run deploy`.
- `.github/workflows/deploy-catalyst.yml` — triggered on `v*` tag push on `marquee-apps`.

### 8.4 Smoke test

`just smoke-catalyst-cloud` (in runway):
1. `curl $SERVICE_URL/healthz` → 200.
2. `curl $SERVICE_URL/status` → packet JSON.
3. Authenticated POST starts `score-inbound-fit`, asserts SSE progression, asserts `EventLog` query returns the trail.

---

## 9. Migration sequencing

Each step is a separate PR. Steps 3–6 don't break the running Helm because `application-server` still has the original code until step 9.

1. **`runway-app-host` extensions** — `HelmModule` trait (with gRPC), `EventHub`, `HostContext`, approvals routes, `/healthz`. Contract tests cover the new surface against a no-op module.
2. **Bootstrap `stack/atelier-showcase`** — empty workspace, README, CLAUDE.md, MILESTONES.md, empty crate skeletons.
3. **Extract `helm-operator-control`** — copy out of `application-server`, wrap as `HelmModule`. Add unit tests.
4. **Extract `helm-governed-jobs`** — same shape.
5. **Extract `helm-truth-execution`** (dispatcher only) — same shape.
6. **Move CRM modules to `atelier-showcase/crates/crm-*`** — copy out of `application-server`, wrap as `HelmModule` impls (gRPC + HTTP). Move CRM truth bodies to `crm-truths/`.
7. **`atelier-showcase/crm-showcase/` binary** — main.rs composes everything; `just crm-showcase-local` smoke-tests against `StorageKit::local`.
8. **`catalyst-backend` binary** — main.rs composes operator-control + governed-jobs + truth-execution + Catalyst's own truth bodies. `just catalyst-local` runs the proof flow.
9. **Delete `helms/crates/application-server`** — no consumers remain. Update Helm `Cargo.toml` workspace, remove related Docker/deploy config.
10. **Cloud Run deploy for `catalyst-backend`** — Terraform, secrets, image, `just deploy-catalyst`, `just smoke-catalyst-cloud`.

---

## 10. Out of scope

The following are acknowledged but deferred to follow-up specs:

- **Movement integration.** `commerce-rails` Stripe webhook + entitlements wiring into `runway-auth` custom claims.
- **Atelier-showcase Cloud Run deploy.** Bootstrapping the workspace and proving it locally is in scope; cloud-deploying the CRM showcase is not.
- **Other marquee apps** (Wolfgang, Inkling, Folio, Scout, Quorum, Vouch) onto runway-app-host.
- **Movement-side billing routes** previously in `application-server`. They're deleted in step 9; reinstatement happens through Movement.
- **gRPC-side approvals or realtime.** gRPC support exists in the trait for module surfaces. Streaming events over gRPC (instead of SSE) is not added in this spec.
- **WebSocket transport** mentioned in the proof doc — SSE is the only realtime transport in scope.

---

## 11. Open questions

- **Timeline module home.** The doc places `helm-timeline` as Helm-side, but it's a thin projection over `EventLog`. If implementation reveals it has no module-specific state, it folds into `runway-app-host` as a host-level endpoint instead. Decided during step 5 (it's a footnote, not a blocker).
- **gRPC port allocation.** `RunwayAppHost::serve()` needs a config knob for the gRPC port (HTTP port already exists). Default behavior: gRPC is disabled unless at least one mounted module returns non-empty `grpc_services()`.
- **`HostContext` ergonomics.** Whether to expose `auth: AuthPolicy` as concrete struct or `auth: Arc<dyn AuthService>` — decided during implementation of step 1 based on what feels cleanest in tests.

These are intentionally left open for the implementation plan to resolve, not because they're undecided.
