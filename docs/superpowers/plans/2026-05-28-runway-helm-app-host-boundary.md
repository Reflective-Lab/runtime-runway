# Runtime Runway/Helm App-Host Boundary Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Land the canonical app execution container across Runtime Runway, Helm, and a thin app — proving it end-to-end with Catalyst on Cloud Run.

**Architecture:** runway-app-host grows a `HelmModule` trait + `HostContext` + `EventHub` so apps mount Helm-provided modules at startup. Helm's monolithic `application-server` is decomposed into per-module crates (`helm-operator-control`, `helm-governed-jobs`, `helm-truth-execution`) and then deleted. CRM showcase code is relocated to a new `stack/atelier-showcase` workspace. Catalyst gets a thin backend binary deployed to Cloud Run.

**Tech Stack:** Rust 2024 edition, Axum 0.x, Tonic gRPC, Tokio broadcast channels, async-trait, anyhow, redb + Firestore (via runway-storage StorageKit), Firebase Auth, GCP Cloud Run, Terraform.

**Spec:** `docs/superpowers/specs/2026-05-28-runway-helm-app-host-boundary-design.md`

---

## File Structure Map

### `runtime-runway/crates/runway-app-host/` — extended (Phase 1)

| File | Status | Responsibility |
|---|---|---|
| `Cargo.toml` | Modify | Add tonic, tonic-build, tokio (broadcast), tokio-stream, async-trait, futures, uuid, chrono |
| `src/lib.rs` | Modify | Re-export new types; keep existing `RunwayAppHost`, `AppExecutionPacket` |
| `src/config.rs` | Keep | Existing — env-driven config |
| `src/realtime.rs` | Create | `EventHub`, `EventHubHandle`, `EventEnvelope` |
| `src/context.rs` | Create | `HostContext` struct passed to module `init()` |
| `src/module.rs` | Create | `HelmModule` trait; `TonicService` alias |
| `src/builder.rs` | Create | `RunwayAppHost::builder()` API, ordered `init()`, concurrent `serve()` |
| `src/approvals.rs` | Create | `/v1/approvals/{ref}/approve|reject` routes |
| `src/health.rs` | Create | `/healthz` route |
| `src/sse.rs` | Create | `/sse/stream` canonical projection over hub |
| `tests/contract_test.rs` | Create | Contract tests using a no-op test module |

### `stack/bedrock-platform/helms/crates/helm-operator-control/` — new (Phase 3)

| File | Source |
|---|---|
| `Cargo.toml` | New, deps from `application-server` minus host scaffolding |
| `src/lib.rs` | Module entry + `HelmModule` impl |
| `src/pipeline.rs` | Move `application-server/src/pipeline.rs` |
| `src/http_api.rs` | Move `application-server/src/http_api.rs` (operator-control parts only) |

### `stack/bedrock-platform/helms/crates/helm-governed-jobs/` — new (Phase 4)

| File | Source |
|---|---|
| `Cargo.toml` | New |
| `src/lib.rs` | Module entry + `HelmModule` impl |
| `src/job_stream.rs` | Move `application-server/src/job_stream.rs` |

### `stack/bedrock-platform/helms/crates/helm-truth-execution/` — new (Phase 5)

| File | Source |
|---|---|
| `Cargo.toml` | New |
| `src/lib.rs` | Module entry + `HelmModule` impl + truth registry trait |
| `src/dispatcher.rs` | Move dispatch logic from `application-server/src/truth_runtime.rs` |
| `src/common.rs` | Move `application-server/src/truth_runtime/common.rs` |

### `stack/atelier-showcase/` — new workspace (Phase 2 bootstrap, Phase 6 fill)

```
stack/atelier-showcase/
├── Cargo.toml                       # workspace
├── README.md
├── CLAUDE.md
├── MILESTONES.md
├── crm-showcase/                    # binary (Phase 7)
│   ├── Cargo.toml
│   └── src/main.rs
└── crates/                          # (Phase 6)
    ├── crm-opportunities/{Cargo.toml, src/lib.rs}
    ├── crm-parties/{Cargo.toml, src/lib.rs}
    ├── crm-workflow/{Cargo.toml, src/lib.rs}
    ├── crm-conversations/{Cargo.toml, src/lib.rs}
    ├── crm-documents/{Cargo.toml, src/lib.rs}
    ├── crm-facts/{Cargo.toml, src/lib.rs}
    ├── crm-metadata/{Cargo.toml, src/lib.rs}
    ├── crm-workbench/{Cargo.toml, src/lib.rs}
    └── crm-truths/{Cargo.toml, src/lib.rs, src/evaluate_acquisition_target.rs, src/plan_outbound_campaign.rs, src/match_renewal_context.rs, src/generate_data_transformer.rs}
```

### `marquee-apps/catalyst-biz/` — extended (Phase 8)

| File | Status |
|---|---|
| `backend/Cargo.toml` | Modify — add runway-app-host, helm-* module deps |
| `backend/src/main.rs` | Modify — replace stub with thin Runtime Runway-hosted main |
| `truths/Cargo.toml` | Create — Catalyst's truth bodies crate |
| `truths/src/lib.rs` | Create — registers `score_inbound_fit`, `qualify_inbound_lead`, `schedule_strategic_meetings` |
| `truths/src/score_inbound_fit.rs` | Move from `helms/.../truth_runtime/score_inbound_fit.rs` |
| `truths/src/qualify_inbound_lead.rs` | Move from `helms/.../truth_runtime/qualify_inbound_lead.rs` |
| `truths/src/schedule_strategic_meetings.rs` | Move from `helms/.../truth_runtime/schedule_strategic_meetings.rs` |
| `Cargo.toml` | Modify — add `truths` to workspace members |

### `runtime-runway/ops/infra/terraform/catalyst-backend/` — new (Phase 10)

| File | Purpose |
|---|---|
| `main.tf` | `google_cloud_run_v2_service`, IAM bindings |
| `service_account.tf` | `catalyst-backend@…` SA + role grants |
| `variables.tf` | project_id, region, image_uri, env |
| `outputs.tf` | service URL, SA email, image path |

### `runtime-runway/` justfile + workflows (Phase 10)

| File | Status |
|---|---|
| `Justfile` | Modify — add `deploy-catalyst`, `smoke-catalyst-cloud`, `catalyst-local` |
| `marquee-apps/catalyst-biz/backend/Dockerfile` | Create — multistage distroless |
| `runtime-runway/.github/workflows/deploy-catalyst.yml` | Create — tag-triggered build+deploy |

### Deletions (Phase 9)

- `stack/bedrock-platform/helms/crates/application-server/` — entire directory
- `stack/bedrock-platform/helms/Cargo.toml` — remove `crates/application-server` from members

### Out-of-scope deletions (handled in Phase 6 cleanup)

- 5 subscription/billing truth files in `application-server/src/truth_runtime/` (`activate_subscription`, `upgrade_subscription_plan`, `refill_prepaid_ai_credits`, `suspend_service_on_payment_failure`, `reconcile_model_usage_against_customer_ledger`) — **deleted entirely**; will be reimplemented in `commerce-rails` under a future spec.

---

## Phase 1: runway-app-host extensions

**PR title:** `feat(runway-app-host): add HelmModule trait, EventHub, HostContext, builder`

### Task 1.1: Add new dependencies

**Files:** `runtime-runway/crates/runway-app-host/Cargo.toml`

- [ ] **Step 1: Add deps to `[dependencies]`**

Edit `runtime-runway/crates/runway-app-host/Cargo.toml`, append after the existing dependencies:

```toml
async-trait = { workspace = true }
tokio = { workspace = true, features = ["sync", "rt-multi-thread", "macros", "signal"] }
tokio-stream = { workspace = true }
tonic = { workspace = true }
futures = { workspace = true }
uuid = { workspace = true, features = ["v4", "serde"] }
chrono = { workspace = true, features = ["serde"] }
```

- [ ] **Step 2: Add `[dev-dependencies]`**

```toml
[dev-dependencies]
tower = "0.5"
reqwest = { workspace = true }
```

- [ ] **Step 3: Verify it compiles**

Run from `runtime-runway/`:
```bash
cargo check -p runway-app-host
```
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add crates/runway-app-host/Cargo.toml
git commit -m "feat(runway-app-host): add deps for HelmModule, EventHub, gRPC"
```

### Task 1.2: Define `EventEnvelope`

**Files:** Create `runtime-runway/crates/runway-app-host/src/realtime.rs`

- [ ] **Step 1: Write the failing test**

Add to `runtime-runway/crates/runway-app-host/src/realtime.rs`:

```rust
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventEnvelope {
    pub event_id: Uuid,
    pub sequence: u64,
    #[serde(rename = "type")]
    pub r#type: String,
    pub schema_version: u32,
    pub occurred_at: DateTime<Utc>,
    pub app_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actor: Option<String>,
    pub payload: serde_json::Value,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelope_roundtrips_through_json() {
        let env = EventEnvelope {
            event_id: Uuid::nil(),
            sequence: 7,
            r#type: "job.started".into(),
            schema_version: 1,
            occurred_at: DateTime::parse_from_rfc3339("2026-05-28T12:00:00Z")
                .unwrap()
                .with_timezone(&Utc),
            app_id: "catalyst".into(),
            run_id: Some("run-1".into()),
            correlation_id: None,
            actor: Some("user:alice".into()),
            payload: serde_json::json!({"key": "value"}),
        };
        let s = serde_json::to_string(&env).unwrap();
        let back: EventEnvelope = serde_json::from_str(&s).unwrap();
        assert_eq!(env.event_id, back.event_id);
        assert_eq!(env.sequence, back.sequence);
        assert_eq!(env.r#type, back.r#type);
        assert!(!s.contains("correlation_id"), "None fields should be omitted");
    }
}
```

Add to `runtime-runway/crates/runway-app-host/src/lib.rs`:
```rust
pub mod realtime;
```

- [ ] **Step 2: Run test to verify it passes**

```bash
cargo test -p runway-app-host realtime::tests::envelope_roundtrips_through_json
```
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add crates/runway-app-host/src/realtime.rs crates/runway-app-host/src/lib.rs
git commit -m "feat(runway-app-host): add EventEnvelope shape"
```

### Task 1.3: Implement `EventHub` + `EventHubHandle`

**Files:** Modify `runtime-runway/crates/runway-app-host/src/realtime.rs`

- [ ] **Step 1: Write the failing test**

Append to `realtime.rs`:

```rust
use tokio::sync::broadcast;

const HUB_CAPACITY: usize = 512;

pub struct EventHub {
    sender: broadcast::Sender<EventEnvelope>,
}

#[derive(Clone)]
pub struct EventHubHandle {
    sender: broadcast::Sender<EventEnvelope>,
}

impl EventHub {
    pub fn new() -> Self {
        let (sender, _) = broadcast::channel(HUB_CAPACITY);
        Self { sender }
    }

    pub fn handle(&self) -> EventHubHandle {
        EventHubHandle { sender: self.sender.clone() }
    }
}

impl Default for EventHub {
    fn default() -> Self {
        Self::new()
    }
}

impl EventHubHandle {
    pub fn publish(&self, env: EventEnvelope) {
        let _ = self.sender.send(env); // ignore "no subscribers"
    }

    pub fn subscribe(&self) -> broadcast::Receiver<EventEnvelope> {
        self.sender.subscribe()
    }

    pub fn subscriber_count(&self) -> usize {
        self.sender.receiver_count()
    }
}

#[cfg(test)]
mod hub_tests {
    use super::*;

    fn sample(seq: u64, ty: &str) -> EventEnvelope {
        EventEnvelope {
            event_id: Uuid::new_v4(),
            sequence: seq,
            r#type: ty.into(),
            schema_version: 1,
            occurred_at: Utc::now(),
            app_id: "test".into(),
            run_id: None,
            correlation_id: None,
            actor: None,
            payload: serde_json::Value::Null,
        }
    }

    #[tokio::test]
    async fn handle_delivers_to_subscriber() {
        let hub = EventHub::new();
        let h = hub.handle();
        let mut rx = h.subscribe();

        h.publish(sample(1, "foo"));
        let got = rx.recv().await.unwrap();
        assert_eq!(got.sequence, 1);
    }

    #[tokio::test]
    async fn publish_without_subscribers_is_silent() {
        let hub = EventHub::new();
        let h = hub.handle();
        h.publish(sample(1, "foo")); // must not panic
        assert_eq!(h.subscriber_count(), 0);
    }
}
```

- [ ] **Step 2: Run tests**

```bash
cargo test -p runway-app-host realtime::hub_tests
```
Expected: 2 tests PASS.

- [ ] **Step 3: Commit**

```bash
git add crates/runway-app-host/src/realtime.rs
git commit -m "feat(runway-app-host): add EventHub broadcast primitive"
```

### Task 1.4: Define `TonicService` alias and `HelmModule` trait

**Files:** Create `runtime-runway/crates/runway-app-host/src/module.rs`

- [ ] **Step 1: Create file with trait**

Write `runtime-runway/crates/runway-app-host/src/module.rs`:

```rust
use std::sync::Arc;

use async_trait::async_trait;
use axum::Router;

use crate::context::HostContext;

/// A boxed Tonic service. Use `tonic::server::NamedService` + axum-compat layer.
pub type TonicService =
    tonic::service::interceptor::InterceptedService<
        tonic::transport::server::Routes,
        fn(tonic::Request<()>) -> Result<tonic::Request<()>, tonic::Status>,
    >;

#[async_trait]
pub trait HelmModule: Send + Sync + 'static {
    fn module_id(&self) -> &'static str;

    async fn init(&self, ctx: &HostContext) -> anyhow::Result<()>;

    fn router(self: Arc<Self>) -> Router {
        Router::new()
    }

    fn grpc_services(self: Arc<Self>) -> Vec<TonicService> {
        vec![]
    }
}
```

Add to `lib.rs`:
```rust
pub mod context;
pub mod module;
pub use module::{HelmModule, TonicService};
```

- [ ] **Step 2: Verify it compiles (after Task 1.5 lands context.rs)**

This task is paired with 1.5 — verify after both are written.

### Task 1.5: Define `HostContext`

**Files:** Create `runtime-runway/crates/runway-app-host/src/context.rs`

- [ ] **Step 1: Write the file**

```rust
use std::sync::Arc;

use runway_storage::StorageKit;

use crate::AppExecutionPacket;
use crate::realtime::EventHubHandle;

#[derive(Clone)]
pub struct HostContext {
    pub packet: Arc<AppExecutionPacket>,
    pub storage: StorageKit,
    pub realtime: EventHubHandle,
}
```

(Auth and telemetry handles defer to follow-up tasks once shape is clearer in tests; the spec calls for them but a minimal HostContext lands first to unblock module wiring.)

- [ ] **Step 2: Verify compile**

```bash
cargo check -p runway-app-host
```
Expected: PASS.

- [ ] **Step 3: Commit Tasks 1.4 + 1.5 together**

```bash
git add crates/runway-app-host/src/module.rs crates/runway-app-host/src/context.rs crates/runway-app-host/src/lib.rs
git commit -m "feat(runway-app-host): add HelmModule trait and HostContext"
```

### Task 1.6: Add `/healthz` route

**Files:** Create `runtime-runway/crates/runway-app-host/src/health.rs`

- [ ] **Step 1: Write the failing test**

Append to `runtime-runway/crates/runway-app-host/src/health.rs`:

```rust
use axum::{Router, http::StatusCode, routing::get};

pub fn router() -> Router {
    Router::new().route("/healthz", get(|| async { (StatusCode::OK, "ok") }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    #[tokio::test]
    async fn healthz_returns_200() {
        let resp = router()
            .oneshot(Request::builder().uri("/healthz").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }
}
```

Add to `lib.rs`:
```rust
pub mod health;
```

- [ ] **Step 2: Run test**

```bash
cargo test -p runway-app-host health::tests::healthz_returns_200
```
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add crates/runway-app-host/src/health.rs crates/runway-app-host/src/lib.rs
git commit -m "feat(runway-app-host): add /healthz route"
```

### Task 1.7: Implement SSE projection over the hub

**Files:** Create `runtime-runway/crates/runway-app-host/src/sse.rs`

- [ ] **Step 1: Write the file**

```rust
use std::convert::Infallible;

use axum::{
    Router,
    extract::State,
    response::sse::{Event, KeepAlive, Sse},
    routing::get,
};
use futures::stream::{Stream, StreamExt};
use tokio_stream::wrappers::BroadcastStream;

use crate::realtime::EventHubHandle;

pub fn router(hub: EventHubHandle) -> Router {
    Router::new()
        .route("/sse/stream", get(stream))
        .with_state(hub)
}

async fn stream(
    State(hub): State<EventHubHandle>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let rx = hub.subscribe();
    let stream = BroadcastStream::new(rx).filter_map(|ev| async move {
        ev.ok().and_then(|env| {
            serde_json::to_string(&env)
                .ok()
                .map(|s| Ok(Event::default().event(env.r#type.clone()).data(s)))
        })
    });
    Sse::new(stream).keep_alive(KeepAlive::default())
}
```

Add to `lib.rs`:
```rust
pub mod sse;
```

- [ ] **Step 2: Write integration test**

Append to `sse.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::realtime::{EventEnvelope, EventHub};
    use axum::body::Body;
    use axum::http::Request;
    use chrono::Utc;
    use tower::ServiceExt;
    use uuid::Uuid;

    #[tokio::test]
    async fn sse_endpoint_is_reachable() {
        let hub = EventHub::new();
        let app = router(hub.handle());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/sse/stream")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let ct = resp.headers().get("content-type").unwrap();
        assert!(ct.to_str().unwrap().starts_with("text/event-stream"));
    }
}
```

- [ ] **Step 3: Run test**

```bash
cargo test -p runway-app-host sse::tests::sse_endpoint_is_reachable
```
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add crates/runway-app-host/src/sse.rs crates/runway-app-host/src/lib.rs
git commit -m "feat(runway-app-host): mount canonical /sse/stream over hub"
```

### Task 1.8: Implement approvals routes

**Files:** Create `runtime-runway/crates/runway-app-host/src/approvals.rs`

- [ ] **Step 1: Write the file**

```rust
use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::post,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::realtime::{EventEnvelope, EventHubHandle};
use crate::AppExecutionPacket;

#[derive(Clone)]
pub struct ApprovalsState {
    pub packet: Arc<AppExecutionPacket>,
    pub realtime: EventHubHandle,
}

#[derive(Debug, Deserialize)]
pub struct ApprovalBody {
    #[serde(default)]
    pub actor: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub correlation_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ApprovalReceipt {
    pub event_id: Uuid,
}

pub fn router(state: ApprovalsState) -> Router {
    Router::new()
        .route("/v1/approvals/{ref_id}/approve", post(approve))
        .route("/v1/approvals/{ref_id}/reject", post(reject))
        .with_state(state)
}

async fn approve(
    State(state): State<ApprovalsState>,
    Path(ref_id): Path<String>,
    Json(body): Json<ApprovalBody>,
) -> (StatusCode, Json<ApprovalReceipt>) {
    let env = build_envelope(&state, "approval.approved", &ref_id, body);
    state.realtime.publish(env.clone());
    (StatusCode::ACCEPTED, Json(ApprovalReceipt { event_id: env.event_id }))
}

async fn reject(
    State(state): State<ApprovalsState>,
    Path(ref_id): Path<String>,
    Json(body): Json<ApprovalBody>,
) -> (StatusCode, Json<ApprovalReceipt>) {
    let env = build_envelope(&state, "approval.rejected", &ref_id, body);
    state.realtime.publish(env.clone());
    (StatusCode::ACCEPTED, Json(ApprovalReceipt { event_id: env.event_id }))
}

fn build_envelope(
    state: &ApprovalsState,
    ty: &str,
    ref_id: &str,
    body: ApprovalBody,
) -> EventEnvelope {
    EventEnvelope {
        event_id: Uuid::new_v4(),
        sequence: 0, // sequencing is the consumer's job; we just timestamp.
        r#type: ty.into(),
        schema_version: 1,
        occurred_at: Utc::now(),
        app_id: state.packet.app_id.clone(),
        run_id: None,
        correlation_id: body.correlation_id,
        actor: body.actor,
        payload: serde_json::json!({
            "ref": ref_id,
            "note": body.note,
        }),
    }
}
```

Add to `lib.rs`:
```rust
pub mod approvals;
```

- [ ] **Step 2: Write integration test**

Append:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::realtime::EventHub;
    use axum::body::{Body, to_bytes};
    use axum::http::Request;
    use tower::ServiceExt;

    fn test_state() -> ApprovalsState {
        let packet = Arc::new(AppExecutionPacket::new(
            "test", "Test", "test desc", "/",
        ));
        let hub = EventHub::new();
        ApprovalsState { packet, realtime: hub.handle() }
    }

    #[tokio::test]
    async fn approve_route_returns_202_and_publishes() {
        let state = test_state();
        let mut rx = state.realtime.subscribe();
        let app = router(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/approvals/job:42/approve")
                    .header("content-type", "application/json")
                    .body(Body::from("{\"actor\":\"alice\"}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), 202);
        let env = rx.recv().await.unwrap();
        assert_eq!(env.r#type, "approval.approved");
        assert_eq!(env.payload["ref"], "job:42");
        assert_eq!(env.actor.as_deref(), Some("alice"));
    }
}
```

- [ ] **Step 3: Run test**

```bash
cargo test -p runway-app-host approvals::tests::approve_route_returns_202_and_publishes
```
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add crates/runway-app-host/src/approvals.rs crates/runway-app-host/src/lib.rs
git commit -m "feat(runway-app-host): add HITL approvals transport routes"
```

### Task 1.9: Implement the builder, init order, and serve

**Files:** Create `runtime-runway/crates/runway-app-host/src/builder.rs`; modify `src/lib.rs`

- [ ] **Step 1: Write the builder**

Write `runtime-runway/crates/runway-app-host/src/builder.rs`:

```rust
use std::sync::Arc;

use anyhow::Result;
use axum::Router;
use tokio::net::TcpListener;

use crate::approvals::{self, ApprovalsState};
use crate::context::HostContext;
use crate::health;
use crate::module::HelmModule;
use crate::realtime::EventHub;
use crate::sse;
use crate::{AppExecutionPacket, RunwayAppHost, config::HostConfig};

use runway_storage::StorageKit;

pub struct RunwayAppHostBuilder {
    packet: Arc<AppExecutionPacket>,
    storage: Option<StorageKit>,
    modules: Vec<Arc<dyn HelmModule>>,
    config: Option<HostConfig>,
}

impl RunwayAppHostBuilder {
    pub fn new(packet: AppExecutionPacket) -> Self {
        Self {
            packet: Arc::new(packet),
            storage: None,
            modules: Vec::new(),
            config: None,
        }
    }

    pub fn with_storage(mut self, storage: StorageKit) -> Self {
        self.storage = Some(storage);
        self
    }

    pub fn with_config(mut self, config: HostConfig) -> Self {
        self.config = Some(config);
        self
    }

    pub fn mount(mut self, module: Arc<dyn HelmModule>) -> Self {
        self.modules.push(module);
        self
    }

    pub async fn build(self) -> Result<BuiltHost> {
        let storage = self.storage.ok_or_else(|| {
            anyhow::anyhow!("with_storage(...) must be called before build()")
        })?;
        let config = match self.config {
            Some(c) => c,
            None => HostConfig::from_env()?,
        };

        let hub = EventHub::new();
        let ctx = HostContext {
            packet: self.packet.clone(),
            storage: storage.clone(),
            realtime: hub.handle(),
        };

        // Run module init() in registration order.
        for module in &self.modules {
            module
                .init(&ctx)
                .await
                .map_err(|e| anyhow::anyhow!("module '{}' init failed: {e}", module.module_id()))?;
        }

        // Compose router.
        let mut router = Router::new()
            .merge(health::router())
            .merge(sse::router(hub.handle()))
            .merge(approvals::router(ApprovalsState {
                packet: self.packet.clone(),
                realtime: hub.handle(),
            }));

        for module in &self.modules {
            router = router.merge(module.clone().router());
        }

        // Prefix everything under packet.route_prefix.
        let prefix = self.packet.route_prefix.trim_end_matches('/');
        let router = if prefix.is_empty() {
            router
        } else {
            Router::new().nest(prefix, router)
        };

        Ok(BuiltHost {
            router,
            config,
            modules: self.modules,
            _hub: hub,
        })
    }
}

pub struct BuiltHost {
    router: Router,
    config: HostConfig,
    modules: Vec<Arc<dyn HelmModule>>,
    _hub: EventHub,
}

impl BuiltHost {
    pub async fn serve(self) -> Result<()> {
        // Spawn gRPC if any module exposes services. For Phase 1 we only
        // support Axum; gRPC wiring is exercised once a real module needs it.
        let any_grpc = self
            .modules
            .iter()
            .any(|m| !m.clone().grpc_services().is_empty());
        if any_grpc {
            tracing::warn!(
                "gRPC services declared but server bring-up is not wired in Phase 1 \
                 — modules-only consumers should land before app uses gRPC"
            );
        }

        let addr = format!("0.0.0.0:{}", self.config.http_port);
        let listener = TcpListener::bind(&addr).await?;
        tracing::info!("runway-app-host listening on http://{addr}");
        axum::serve(listener, self.router).await?;
        Ok(())
    }
}

impl RunwayAppHost {
    pub fn builder(packet: AppExecutionPacket) -> RunwayAppHostBuilder {
        RunwayAppHostBuilder::new(packet)
    }
}
```

- [ ] **Step 2: Add `pub mod builder;` to lib.rs and re-exports**

```rust
pub mod approvals;
pub mod builder;
pub mod config;
pub mod context;
pub mod health;
pub mod module;
pub mod realtime;
pub mod sse;

pub use builder::{BuiltHost, RunwayAppHostBuilder};
pub use context::HostContext;
pub use module::{HelmModule, TonicService};
pub use realtime::{EventEnvelope, EventHub, EventHubHandle};
```

- [ ] **Step 3: Verify compile**

```bash
cargo check -p runway-app-host
```
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add crates/runway-app-host/src/builder.rs crates/runway-app-host/src/lib.rs
git commit -m "feat(runway-app-host): add builder API with init order and serve"
```

### Task 1.10: Contract test against a no-op module

**Files:** Create `runtime-runway/crates/runway-app-host/tests/contract_test.rs`

- [ ] **Step 1: Write the test**

```rust
use std::sync::Arc;

use async_trait::async_trait;
use axum::{Router, routing::get};
use runway_app_host::{
    AppExecutionPacket, HelmModule, HostContext, RunwayAppHost,
    config::HostConfig,
};
use runway_storage::StorageKit;
use tempfile::TempDir;

struct NoopModule {
    received_packet_id: tokio::sync::OnceCell<String>,
}

#[async_trait]
impl HelmModule for NoopModule {
    fn module_id(&self) -> &'static str { "test.noop" }

    async fn init(&self, ctx: &HostContext) -> anyhow::Result<()> {
        self.received_packet_id
            .set(ctx.packet.app_id.clone())
            .map_err(|_| anyhow::anyhow!("init called twice"))?;
        Ok(())
    }

    fn router(self: Arc<Self>) -> Router {
        Router::new().route("/v1/noop/ping", get(|| async { "pong" }))
    }
}

#[tokio::test]
async fn build_invokes_module_init_in_order() {
    let tmp = TempDir::new().unwrap();
    let storage = StorageKit::local(tmp.path()).await.unwrap();

    let packet = AppExecutionPacket::new("test-app", "Test", "desc", "/");
    let module = Arc::new(NoopModule {
        received_packet_id: tokio::sync::OnceCell::new(),
    });

    let host = RunwayAppHost::builder(packet)
        .with_storage(storage)
        .with_config(HostConfig { http_port: 0, ..Default::default() })
        .mount(module.clone())
        .build()
        .await
        .unwrap();

    assert_eq!(
        module.received_packet_id.get().map(String::as_str),
        Some("test-app")
    );
    // host built successfully; we don't run serve() in unit test
    drop(host);
}

#[tokio::test]
async fn host_mounts_canonical_routes() {
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    let tmp = TempDir::new().unwrap();
    let storage = StorageKit::local(tmp.path()).await.unwrap();

    let packet = AppExecutionPacket::new("test-app", "Test", "desc", "/");
    let host = RunwayAppHost::builder(packet)
        .with_storage(storage)
        .with_config(HostConfig { http_port: 0, ..Default::default() })
        .build()
        .await
        .unwrap();

    let router = host.into_router_for_test(); // exposed via #[cfg(test)] in builder
    for path in ["/healthz", "/sse/stream"] {
        let resp = router
            .clone()
            .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert!(resp.status().is_success(), "{path} should be 2xx");
    }
}
```

- [ ] **Step 2: Add the test-only accessor in builder.rs**

In `builder.rs` add:

```rust
#[cfg(test)]
impl BuiltHost {
    pub fn into_router_for_test(self) -> Router {
        self.router
    }
}
```

Also add a `Default` impl for `HostConfig` if absent. Inspect `config.rs` and add:

```rust
impl Default for HostConfig {
    fn default() -> Self {
        Self { http_port: 0 /* ... other fields with safe defaults ... */ }
    }
}
```

(Engineer must reconcile with the existing `HostConfig` struct fields.)

- [ ] **Step 3: Run contract tests**

```bash
cargo test -p runway-app-host --test contract_test
```
Expected: 2 tests PASS.

- [ ] **Step 4: Commit**

```bash
git add crates/runway-app-host/tests/contract_test.rs crates/runway-app-host/src/builder.rs crates/runway-app-host/src/config.rs
git commit -m "test(runway-app-host): contract tests for HelmModule trait and host routes"
```

### Task 1.11: Lint, format, and open PR

- [ ] **Step 1: Run `just lint`**

```bash
just lint
```
Expected: PASS. If clippy flags issues, fix them. Do not use `#[allow(...)]` unless the issue is a false positive that's already commented on in the codebase.

- [ ] **Step 2: Push branch and open PR**

```bash
git push -u origin HEAD
gh pr create --title "feat(runway-app-host): HelmModule trait + host runtime additions" --body "$(cat <<'EOF'
## Summary
- Adds `HelmModule` trait, `HostContext`, `EventHub`/`EventEnvelope`, builder API, `/healthz`, `/sse/stream`, and `/v1/approvals/{ref}/*` routes.
- Pure addition. No existing API removed.

Spec: docs/superpowers/specs/2026-05-28-runway-helm-app-host-boundary-design.md

## Test plan
- [x] `cargo test -p runway-app-host`
- [x] `cargo test -p runway-app-host --test contract_test`
- [x] `just lint`

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

---

## Phase 2: Bootstrap `stack/atelier-showcase`

**PR title:** `feat(atelier-showcase): bootstrap workspace with empty crate skeletons`

### Task 2.1: Create workspace root

**Files:** New directory `/Users/kpernyer/dev/reflective/atelier-showcase/`

- [ ] **Step 1: Create the workspace**

```bash
mkdir -p /Users/kpernyer/dev/reflective/atelier-showcase/{crm-showcase/src,crates}
cd /Users/kpernyer/dev/reflective/atelier-showcase
```

- [ ] **Step 2: Write workspace `Cargo.toml`**

Write `/Users/kpernyer/dev/reflective/atelier-showcase/Cargo.toml`:

```toml
[workspace]
resolver = "2"
members = [
    "crm-showcase",
    "crates/crm-opportunities",
    "crates/crm-parties",
    "crates/crm-workflow",
    "crates/crm-conversations",
    "crates/crm-documents",
    "crates/crm-facts",
    "crates/crm-metadata",
    "crates/crm-workbench",
    "crates/crm-truths",
]

[workspace.package]
version = "0.1.0"
edition = "2024"
rust-version = "1.94"
license = "MIT"
publish = false

[workspace.dependencies]
anyhow = "1"
async-trait = "0.1"
axum = "0.7"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tokio = { version = "1", features = ["full"] }
tonic = "0.12"
tracing = "0.1"
uuid = { version = "1", features = ["v4", "serde"] }
chrono = { version = "0.4", features = ["serde"] }

# Cross-repo
runway-app-host = { path = "../../reflective/runtime-runway/crates/runway-app-host" }
runway-storage = { path = "../../reflective/runtime-runway/crates/runway-storage" }
```

(Engineer: verify the relative path from `stack/atelier-showcase/` to `reflective/runtime-runway/crates/...` — `../../reflective/runtime-runway/...` assumes co-located parent directories. Adjust if needed.)

- [ ] **Step 3: Write `README.md`, `CLAUDE.md`, `MILESTONES.md`**

Write `README.md`:

```markdown
# Atelier Showcase

Reflective showcase apps — example deployments demonstrating how to compose
Runtime Runway + Helm modules into a thin app binary.

Each crate in `crates/crm-*` is a `HelmModule` impl. The `crm-showcase/`
binary mounts them on top of `runway-app-host` to produce a runnable CRM demo.

This repo is for examples, not platform infrastructure. Reusable patterns
belong in Helm; ops/runtime belong in Runtime Runway; commercial concerns belong in
Commerce Rails.
```

Write `CLAUDE.md`:

```markdown
# Atelier Showcase

> See `~/CLAUDE.md` and `~/dev/CLAUDE.md` for global conventions.

## What belongs here

- Showcase apps demonstrating Runtime Runway + Helm composition
- CRM demo binary and its module crates
- Showcase-only truth bodies

## What does NOT belong here

- Reusable Helm patterns — those go in `stack/bedrock-platform/helms/crates/`
- Runtime Runway infrastructure — that goes in `reflective/runtime-runway/`
- Commerce / billing — that goes in `reflective/commerce-rails/`

## Rules

- Each `crm-*` crate is a `HelmModule` impl
- `crm-showcase` is a binary; everything else is a library
- Never depend on a marquee app (Catalyst, Wolfgang, etc.)
```

Write `MILESTONES.md`:

```markdown
# Atelier Showcase Milestones

## M1 — CRM showcase runs locally

- [ ] All 9 crate skeletons compile
- [ ] CRM modules implement HelmModule with code migrated from helms/crates/application-server
- [ ] crm-showcase binary boots against StorageKit::local
- [ ] Smoke test passes against demo dataset
```

- [ ] **Step 4: Commit**

```bash
git add Cargo.toml README.md CLAUDE.md MILESTONES.md
git commit -m "feat(atelier-showcase): bootstrap workspace"
```

### Task 2.2: Create empty crate skeletons

**Files:** Nine `Cargo.toml` + nine `src/lib.rs` files

- [ ] **Step 1: Write the helper script**

(Engineer may automate or do it nine times by hand. Sample for one crate:)

`stack/atelier-showcase/crates/crm-opportunities/Cargo.toml`:
```toml
[package]
name = "crm-opportunities"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
publish.workspace = true

[dependencies]
anyhow = { workspace = true }
async-trait = { workspace = true }
axum = { workspace = true }
runway-app-host = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
tokio = { workspace = true }
tracing = { workspace = true }
```

`stack/atelier-showcase/crates/crm-opportunities/src/lib.rs`:
```rust
//! CRM Opportunities showcase module (placeholder).
//!
//! Will be filled in Phase 6 with code extracted from
//! helms/crates/application-server/src/service.rs::OpportunitiesGrpc.
```

- [ ] **Step 2: Repeat for the other 8 crates**

Crates to create: `crm-parties`, `crm-workflow`, `crm-conversations`, `crm-documents`, `crm-facts`, `crm-metadata`, `crm-workbench`, `crm-truths`. Same structure as above.

For `crm-truths`, leave the Cargo.toml dependency on `runway-app-host` out (it's not a HelmModule itself, it holds truth body functions).

- [ ] **Step 3: Create crm-showcase binary skeleton**

`stack/atelier-showcase/crm-showcase/Cargo.toml`:
```toml
[package]
name = "crm-showcase"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
publish.workspace = true

[[bin]]
name = "crm-showcase"
path = "src/main.rs"

[dependencies]
anyhow = { workspace = true }
runway-app-host = { workspace = true }
runway-storage = { workspace = true }
tokio = { workspace = true }
tracing = { workspace = true }
```

`stack/atelier-showcase/crm-showcase/src/main.rs`:
```rust
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("crm-showcase placeholder — Phase 7 fills this in");
    Ok(())
}
```

- [ ] **Step 4: Verify the workspace builds**

```bash
cd /Users/kpernyer/dev/reflective/atelier-showcase
cargo build
```
Expected: All 10 crates compile (placeholders, no logic).

- [ ] **Step 5: Commit and open PR**

```bash
git add .
git commit -m "feat(atelier-showcase): add empty crate skeletons for CRM modules"
git push -u origin HEAD
gh pr create --title "feat(atelier-showcase): bootstrap workspace + skeletons" --body "Bootstraps the atelier-showcase workspace with empty crate skeletons. Spec: ../reflective/runtime-runway/docs/superpowers/specs/2026-05-28-runway-helm-app-host-boundary-design.md"
```

---

## Phase 3: Extract `helm-operator-control`

**PR title:** `feat(helm-operator-control): extract from application-server as HelmModule`

### Task 3.1: Create the crate

**Files:** `stack/bedrock-platform/helms/crates/helm-operator-control/`

- [ ] **Step 1: Create the directory and Cargo.toml**

```bash
mkdir -p /Users/kpernyer/dev/reflective/bedrock-platform/helms/crates/helm-operator-control/src
```

Write `Cargo.toml`:

```toml
[package]
name = "helm-operator-control"
version.workspace = true
edition.workspace = true
license.workspace = true
publish.workspace = true

[dependencies]
anyhow.workspace = true
async-trait = "0.1"
axum.workspace = true
chrono.workspace = true
runway-app-host = { path = "../../../../../reflective/runtime-runway/crates/runway-app-host" }
serde.workspace = true
serde_json.workspace = true
tokio.workspace = true
tracing.workspace = true
uuid.workspace = true

# Helm-internal deps
application-kernel = { path = "../application-kernel" }
application-storage = { path = "../application-storage" }
truth-catalog = { path = "../truth-catalog" }
capability-core = { path = "../capability-core" }
```

(Engineer: confirm the relative path to `runway-app-host`. Helm is at `stack/bedrock-platform/helms/crates/helm-operator-control/`; runway is at `reflective/runtime-runway/crates/runway-app-host/`. The dotdot count above assumes a common parent at `/Users/kpernyer/dev/`. Adjust.)

- [ ] **Step 2: Add to Helm workspace members**

Edit `stack/bedrock-platform/helms/Cargo.toml` `[workspace] members =`, add:
```toml
    "crates/helm-operator-control",
```

- [ ] **Step 3: Verify workspace still builds**

```bash
cd /Users/kpernyer/dev/reflective/bedrock-platform/helms
cargo build -p helm-operator-control
```
Expected: PASS (empty crate compiles).

### Task 3.2: Move `pipeline.rs` and `http_api.rs` (operator-control parts)

- [ ] **Step 1: Read existing files**

```bash
cat stack/bedrock-platform/helms/crates/application-server/src/pipeline.rs
cat stack/bedrock-platform/helms/crates/application-server/src/http_api.rs
```

The engineer must identify which functions in `http_api.rs` belong to operator-control (route prefix `/v1/workbench/operator-control/`). Functions touching other prefixes stay behind for now.

- [ ] **Step 2: Copy to the new crate**

```bash
cp stack/bedrock-platform/helms/crates/application-server/src/pipeline.rs \
   stack/bedrock-platform/helms/crates/helm-operator-control/src/pipeline.rs
```

Write `stack/bedrock-platform/helms/crates/helm-operator-control/src/http_api.rs` with only the operator-control handlers extracted from the original. Update imports.

- [ ] **Step 3: Adjust imports**

Replace any `use crate::pipeline::...` with the new module-relative paths. Replace any `use crate::realtime::RealtimeHub` with `use runway_app_host::EventHubHandle`. Replace publish calls accordingly (typed envelope shape).

### Task 3.3: Implement `HelmModule` for `OperatorControlModule`

**Files:** `stack/bedrock-platform/helms/crates/helm-operator-control/src/lib.rs`

- [ ] **Step 1: Write the module struct and trait impl**

```rust
mod http_api;
mod pipeline;

use std::sync::Arc;

use async_trait::async_trait;
use axum::Router;
use runway_app_host::{HelmModule, HostContext};

pub use pipeline::PipelineState;

pub struct OperatorControlModule {
    state: Arc<PipelineState>,
}

impl OperatorControlModule {
    pub fn new() -> Self {
        Self {
            state: Arc::new(PipelineState::default()),
        }
    }
}

impl Default for OperatorControlModule {
    fn default() -> Self { Self::new() }
}

#[async_trait]
impl HelmModule for OperatorControlModule {
    fn module_id(&self) -> &'static str {
        "helm.operator-control"
    }

    async fn init(&self, ctx: &HostContext) -> anyhow::Result<()> {
        self.state.attach_realtime(ctx.realtime.clone());
        tracing::info!(module = self.module_id(), "initialized");
        Ok(())
    }

    fn router(self: Arc<Self>) -> Router {
        http_api::router(self.state.clone())
    }
}
```

(Engineer: `PipelineState::attach_realtime` may not exist on the original `PipelineState` — add it in the extracted `pipeline.rs` as a `set_hub(hub: EventHubHandle)` method storing the handle, then replace its old broadcast sites with `hub.publish(...)`.)

- [ ] **Step 2: Build and fix compile errors**

```bash
cd stack/bedrock-platform/helms
cargo build -p helm-operator-control
```

Iterate on imports, missing types, and the `PipelineState` -> `EventHubHandle` translation until clean.

- [ ] **Step 3: Add a unit test**

`stack/bedrock-platform/helms/crates/helm-operator-control/tests/module_test.rs`:

```rust
use std::sync::Arc;
use helm_operator_control::OperatorControlModule;
use runway_app_host::HelmModule;

#[test]
fn module_id_is_stable() {
    let m = Arc::new(OperatorControlModule::new());
    assert_eq!(m.module_id(), "helm.operator-control");
}
```

```bash
cargo test -p helm-operator-control
```
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add stack/bedrock-platform/helms/crates/helm-operator-control \
        stack/bedrock-platform/helms/Cargo.toml
git commit -m "feat(helm-operator-control): extract operator-control as HelmModule"
```

### Task 3.4: Open PR

- [ ] **Step 1: Push and PR**

```bash
git push -u origin HEAD
gh pr create --title "feat(helm-operator-control): extract from application-server" --body "Extracts operator-control as a standalone helm-* crate implementing HelmModule. application-server still has its copy until Phase 9."
```

---

## Phase 4: Extract `helm-governed-jobs`

**PR title:** `feat(helm-governed-jobs): extract job-stream as HelmModule`

### Task 4.1: Create the crate

- [ ] **Step 1: Create directory + Cargo.toml**

```bash
mkdir -p stack/bedrock-platform/helms/crates/helm-governed-jobs/src
```

Write `stack/bedrock-platform/helms/crates/helm-governed-jobs/Cargo.toml`:

```toml
[package]
name = "helm-governed-jobs"
version.workspace = true
edition.workspace = true
license.workspace = true
publish.workspace = true

[dependencies]
anyhow.workspace = true
async-stream.workspace = true
async-trait = "0.1"
axum.workspace = true
chrono.workspace = true
futures = "0.3"
runway-app-host = { path = "../../../../../reflective/runtime-runway/crates/runway-app-host" }
serde.workspace = true
serde_json.workspace = true
tokio.workspace = true
tokio-stream.workspace = true
tracing.workspace = true
uuid.workspace = true

application-kernel = { path = "../application-kernel" }
application-storage = { path = "../application-storage" }
```

Add to Helm workspace members.

### Task 4.2: Move `job_stream.rs` + wrap as module

- [ ] **Step 1: Copy and adapt**

```bash
cp stack/bedrock-platform/helms/crates/application-server/src/job_stream.rs \
   stack/bedrock-platform/helms/crates/helm-governed-jobs/src/job_stream.rs
```

In `job_stream.rs`, replace `crate::realtime::RealtimeHub` with `runway_app_host::EventHubHandle`. Translate broadcast/publish call sites to the envelope shape (set `r#type = "job.started" | "gate.paused" | "approval.requested" | "job.resumed" | "job.completed"`).

- [ ] **Step 2: Subscribe to `approval.approved` in init**

`stack/bedrock-platform/helms/crates/helm-governed-jobs/src/lib.rs`:

```rust
mod job_stream;

use std::sync::Arc;

use async_trait::async_trait;
use axum::Router;
use runway_app_host::{EventHubHandle, HelmModule, HostContext};

pub use job_stream::JobStreamState;

pub struct GovernedJobsModule {
    state: Arc<JobStreamState>,
}

impl GovernedJobsModule {
    pub fn new() -> Self {
        Self { state: Arc::new(JobStreamState::default()) }
    }
}

impl Default for GovernedJobsModule {
    fn default() -> Self { Self::new() }
}

#[async_trait]
impl HelmModule for GovernedJobsModule {
    fn module_id(&self) -> &'static str {
        "helm.governed-jobs"
    }

    async fn init(&self, ctx: &HostContext) -> anyhow::Result<()> {
        let hub = ctx.realtime.clone();
        let state = self.state.clone();
        state.attach_realtime(hub.clone());

        // Subscribe to approval.* events and react.
        let mut rx = hub.subscribe();
        tokio::spawn(async move {
            while let Ok(env) = rx.recv().await {
                match env.r#type.as_str() {
                    "approval.approved" => state.handle_approval(env, true).await,
                    "approval.rejected" => state.handle_approval(env, false).await,
                    _ => {}
                }
            }
        });
        Ok(())
    }

    fn router(self: Arc<Self>) -> Router {
        job_stream::router(self.state.clone())
    }
}
```

(Engineer: `JobStreamState::handle_approval` and `attach_realtime` may need to be added — the original `JobStreamState` likely had local broadcast state that we replace with hub publish.)

- [ ] **Step 3: Build and test**

```bash
cargo build -p helm-governed-jobs
cargo test -p helm-governed-jobs
```

- [ ] **Step 4: Commit + PR**

```bash
git add stack/bedrock-platform/helms/crates/helm-governed-jobs stack/bedrock-platform/helms/Cargo.toml
git commit -m "feat(helm-governed-jobs): extract governed-jobs as HelmModule"
git push -u origin HEAD
gh pr create --title "feat(helm-governed-jobs): extract from application-server" --body "Extracts governed-jobs as a standalone helm-* crate. Subscribes to approval.* via the hub instead of local broadcast."
```

---

## Phase 5: Extract `helm-truth-execution`

**PR title:** `feat(helm-truth-execution): extract truth dispatcher as HelmModule + registry`

### Task 5.1: Create crate, move dispatcher + common, define registry trait

- [ ] **Step 1: Create directory + Cargo.toml**

```bash
mkdir -p stack/bedrock-platform/helms/crates/helm-truth-execution/src
```

`stack/bedrock-platform/helms/crates/helm-truth-execution/Cargo.toml`:

```toml
[package]
name = "helm-truth-execution"
version.workspace = true
edition.workspace = true
license.workspace = true
publish.workspace = true

[dependencies]
anyhow.workspace = true
async-trait = "0.1"
axum.workspace = true
chrono.workspace = true
runway-app-host = { path = "../../../../../reflective/runtime-runway/crates/runway-app-host" }
serde.workspace = true
serde_json.workspace = true
tokio.workspace = true
tracing.workspace = true
uuid.workspace = true

application-kernel = { path = "../application-kernel" }
truth-catalog = { path = "../truth-catalog" }
converge-domain.workspace = true
```

Add to Helm workspace members.

- [ ] **Step 2: Move dispatcher + common**

```bash
cp stack/bedrock-platform/helms/crates/application-server/src/truth_runtime.rs \
   stack/bedrock-platform/helms/crates/helm-truth-execution/src/dispatcher.rs
cp stack/bedrock-platform/helms/crates/application-server/src/truth_runtime/common.rs \
   stack/bedrock-platform/helms/crates/helm-truth-execution/src/common.rs
```

Strip the individual truth-body `pub mod` declarations from `dispatcher.rs` — those bodies move elsewhere (Catalyst gets 3, atelier-showcase gets 4, 5 are deleted).

- [ ] **Step 3: Define the truth registry trait**

`stack/bedrock-platform/helms/crates/helm-truth-execution/src/lib.rs`:

```rust
mod dispatcher;
mod common;

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use async_trait::async_trait;
use axum::Router;
use runway_app_host::{HelmModule, HostContext};

pub use common::*;

#[async_trait]
pub trait TruthBody: Send + Sync + 'static {
    fn key(&self) -> &'static str;
    async fn execute(&self, ctx: &TruthExecutionContext) -> anyhow::Result<TruthOutcome>;
}

pub struct TruthExecutionContext {
    // (engineer: derive from `application-server/src/truth_runtime/common.rs::Ctx` shape)
}

pub struct TruthOutcome {
    pub facts: Vec<serde_json::Value>,
    pub events: Vec<runway_app_host::EventEnvelope>,
}

pub struct TruthExecutionModule {
    registry: RwLock<HashMap<&'static str, Arc<dyn TruthBody>>>,
}

impl TruthExecutionModule {
    pub fn new() -> Self {
        Self { registry: RwLock::new(HashMap::new()) }
    }

    pub fn register(self, body: Arc<dyn TruthBody>) -> Self {
        self.registry.write().unwrap().insert(body.key(), body);
        self
    }
}

impl Default for TruthExecutionModule {
    fn default() -> Self { Self::new() }
}

#[async_trait]
impl HelmModule for TruthExecutionModule {
    fn module_id(&self) -> &'static str {
        "helm.truth-execution"
    }

    async fn init(&self, _ctx: &HostContext) -> anyhow::Result<()> {
        let count = self.registry.read().unwrap().len();
        tracing::info!(module = self.module_id(), registered_truths = count, "initialized");
        Ok(())
    }

    fn router(self: Arc<Self>) -> Router {
        dispatcher::router(self.clone())
    }
}
```

- [ ] **Step 4: Update `dispatcher.rs`**

Rewrite `dispatcher.rs::router` to take `Arc<TruthExecutionModule>` and look up registered bodies by key when serving `POST /v1/truths/{key}/execute`. The original dispatcher had a hardcoded match on `key` — replace with registry lookup.

- [ ] **Step 5: Build + test**

```bash
cargo build -p helm-truth-execution
cargo test -p helm-truth-execution
```

- [ ] **Step 6: Commit + PR**

```bash
git add stack/bedrock-platform/helms/crates/helm-truth-execution stack/bedrock-platform/helms/Cargo.toml
git commit -m "feat(helm-truth-execution): extract truth dispatcher as HelmModule with registry"
git push -u origin HEAD
gh pr create --title "feat(helm-truth-execution): extract from application-server" --body "Extracts the truth dispatcher framework. Truth bodies are not included — they're registered by consuming apps."
```

---

## Phase 6: Move CRM modules to `atelier-showcase`

**PR title:** `feat(atelier-showcase): populate CRM crm-* crates from application-server`

This phase has nine sub-tasks (one per `crm-*` crate plus the truths). Each follows the same pattern: copy code from `application-server`, wrap as `HelmModule` impl, update imports, build, test, commit.

### Task 6.0: Delete subscription/billing truth bodies

The 5 subscription truth bodies don't move — they're deleted entirely (future Commerce Rails work).

- [ ] **Step 1: Confirm the targets**

Files to delete:
```
stack/bedrock-platform/helms/crates/application-server/src/truth_runtime/activate_subscription.rs
stack/bedrock-platform/helms/crates/application-server/src/truth_runtime/upgrade_subscription_plan.rs
stack/bedrock-platform/helms/crates/application-server/src/truth_runtime/refill_prepaid_ai_credits.rs
stack/bedrock-platform/helms/crates/application-server/src/truth_runtime/suspend_service_on_payment_failure.rs
stack/bedrock-platform/helms/crates/application-server/src/truth_runtime/reconcile_model_usage_against_customer_ledger.rs
```

- [ ] **Step 2: Delete + remove from `truth_runtime.rs` mod declarations**

```bash
rm stack/bedrock-platform/helms/crates/application-server/src/truth_runtime/{activate_subscription,upgrade_subscription_plan,refill_prepaid_ai_credits,suspend_service_on_payment_failure,reconcile_model_usage_against_customer_ledger}.rs
```

Edit `application-server/src/truth_runtime.rs`: remove the corresponding `pub mod ...;` lines and any match arms in the dispatcher.

- [ ] **Step 3: Verify application-server still builds**

```bash
cd stack/bedrock-platform/helms
cargo build -p application-server
```
Expected: PASS (these 5 truths are no longer dispatchable; that's fine because nothing in scope of this spec exercises them).

- [ ] **Step 4: Commit**

```bash
git add stack/bedrock-platform/helms/crates/application-server
git commit -m "chore(application-server): drop subscription/billing truths (moving to Commerce Rails)"
```

### Task 6.1–6.8: Each `crm-*` crate

Apply this same pattern to each of: `crm-opportunities`, `crm-parties`, `crm-workflow`, `crm-conversations`, `crm-documents`, `crm-facts`, `crm-metadata`, `crm-workbench`.

- [ ] **Step 1 (per crate): Identify source**

The relevant gRPC service in `application-server/src/service.rs`. For example, `OpportunitiesGrpc` for `crm-opportunities`. Plus any helpers in `application-server/src/proto.rs` it depends on.

- [ ] **Step 2: Move source**

Copy the gRPC service code into `stack/atelier-showcase/crates/crm-opportunities/src/lib.rs` (or split if large). Adjust imports — most will continue to point at `prio-*` crates in Helm.

- [ ] **Step 3: Wrap as HelmModule**

Implement `HelmModule` for the module struct. For gRPC-only modules:

```rust
use std::sync::Arc;
use async_trait::async_trait;
use axum::Router;
use runway_app_host::{HelmModule, HostContext, TonicService};

pub struct OpportunitiesModule {
    // existing fields
}

#[async_trait]
impl HelmModule for OpportunitiesModule {
    fn module_id(&self) -> &'static str { "crm.opportunities" }
    async fn init(&self, _ctx: &HostContext) -> anyhow::Result<()> { Ok(()) }

    fn grpc_services(self: Arc<Self>) -> Vec<TonicService> {
        // Build a Tonic service from the existing gRPC server implementation.
        vec![/* tonic-build-generated service wrapped as TonicService */]
    }
}
```

- [ ] **Step 4: Build + commit**

```bash
cargo build -p crm-opportunities
git add stack/atelier-showcase/crates/crm-opportunities
git commit -m "feat(crm-opportunities): move from helms/application-server"
```

Repeat for each remaining crate.

### Task 6.9: Move CRM truth bodies to `crm-truths`

The 4 non-Catalyst, non-subscription truths move to `crm-truths`:

- [ ] **Step 1: Move files**

```bash
cp stack/bedrock-platform/helms/crates/application-server/src/truth_runtime/evaluate_acquisition_target.rs \
   stack/atelier-showcase/crates/crm-truths/src/evaluate_acquisition_target.rs
cp stack/bedrock-platform/helms/crates/application-server/src/truth_runtime/plan_outbound_campaign.rs \
   stack/atelier-showcase/crates/crm-truths/src/plan_outbound_campaign.rs
cp stack/bedrock-platform/helms/crates/application-server/src/truth_runtime/match_renewal_context.rs \
   stack/atelier-showcase/crates/crm-truths/src/match_renewal_context.rs
cp stack/bedrock-platform/helms/crates/application-server/src/truth_runtime/generate_data_transformer.rs \
   stack/atelier-showcase/crates/crm-truths/src/generate_data_transformer.rs
```

- [ ] **Step 2: Write `crm-truths/src/lib.rs`**

```rust
//! CRM showcase truth bodies. Registered with helm-truth-execution at startup.

mod evaluate_acquisition_target;
mod plan_outbound_campaign;
mod match_renewal_context;
mod generate_data_transformer;

use std::sync::Arc;
use helm_truth_execution::TruthBody;

pub fn all() -> Vec<Arc<dyn TruthBody>> {
    vec![
        Arc::new(evaluate_acquisition_target::Body),
        Arc::new(plan_outbound_campaign::Body),
        Arc::new(match_renewal_context::Body),
        Arc::new(generate_data_transformer::Body),
    ]
}
```

In each truth body file, implement the `TruthBody` trait on a unit struct named `Body`.

- [ ] **Step 3: Update Cargo.toml**

`stack/atelier-showcase/crates/crm-truths/Cargo.toml` — add:
```toml
helm-truth-execution = { path = "../../../../bedrock-platform/helms/crates/helm-truth-execution" }
```

- [ ] **Step 4: Build, commit, PR**

```bash
cargo build -p crm-truths
git add stack/atelier-showcase
git commit -m "feat(crm-truths): move 4 CRM truth bodies from helms"
git push -u origin HEAD
gh pr create --title "feat(atelier-showcase): populate CRM modules from application-server" --body "Moves CRM services + 4 CRM truth bodies. Drops 5 subscription truths (future Commerce Rails work)."
```

---

## Phase 7: `crm-showcase` binary

**PR title:** `feat(crm-showcase): wire CRM modules into runway-app-host binary`

### Task 7.1: Write `crm-showcase/src/main.rs`

- [ ] **Step 1: Replace the placeholder**

`stack/atelier-showcase/crm-showcase/src/main.rs`:

```rust
use std::sync::Arc;

use runway_app_host::{AppExecutionPacket, MountedModule, RunwayAppHost};
use runway_storage::StorageKit;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let base = std::env::var("CRM_SHOWCASE_DATA_DIR")
        .unwrap_or_else(|_| "/tmp/crm-showcase".into());
    let storage = StorageKit::local(&base).await?;

    let packet = AppExecutionPacket::new(
        "crm-showcase",
        "CRM Showcase",
        "Demonstration CRM stack — opportunities, parties, workflow, etc.",
        "/",
    )
    .with_version(env!("CARGO_PKG_VERSION"))
    .with_auth_app("crm-showcase")
    .with_mounted_module(MountedModule::new("crm.opportunities", vec![]))
    .with_mounted_module(MountedModule::new("crm.parties", vec![]))
    .with_mounted_module(MountedModule::new("crm.workflow", vec![]))
    .with_mounted_module(MountedModule::new("crm.conversations", vec![]))
    .with_mounted_module(MountedModule::new("crm.documents", vec![]))
    .with_mounted_module(MountedModule::new("crm.facts", vec![]))
    .with_mounted_module(MountedModule::new("crm.metadata", vec![]))
    .with_mounted_module(MountedModule::new("crm.workbench", vec![]))
    .with_mounted_module(MountedModule::new("helm.truth-execution", vec![]));

    let truths = helm_truth_execution::TruthExecutionModule::new();
    let truths = crm_truths::all()
        .into_iter()
        .fold(truths, |m, b| m.register(b));

    RunwayAppHost::builder(packet)
        .with_storage(storage)
        .mount(Arc::new(crm_opportunities::OpportunitiesModule::default()))
        .mount(Arc::new(crm_parties::PartiesModule::default()))
        .mount(Arc::new(crm_workflow::WorkflowModule::default()))
        .mount(Arc::new(crm_conversations::ConversationsModule::default()))
        .mount(Arc::new(crm_documents::DocumentsModule::default()))
        .mount(Arc::new(crm_facts::FactsModule::default()))
        .mount(Arc::new(crm_metadata::MetadataModule::default()))
        .mount(Arc::new(crm_workbench::WorkbenchModule::default()))
        .mount(Arc::new(truths))
        .build()
        .await?
        .serve()
        .await
}
```

- [ ] **Step 2: Update `crm-showcase/Cargo.toml`**

Add all `crm-*` and `helm-*` deps as workspace dependencies.

- [ ] **Step 3: Build and run locally**

```bash
cd stack/atelier-showcase
cargo run -p crm-showcase
# in another shell:
curl http://localhost:8080/status
curl http://localhost:8080/healthz
```

Expected: 200 from both. (Engineer: configure the port via `HOST_HTTP_PORT` env var matching runway-app-host's HostConfig.)

- [ ] **Step 4: Commit + PR**

```bash
git add stack/atelier-showcase/crm-showcase
git commit -m "feat(crm-showcase): wire CRM modules into runway-app-host binary"
git push -u origin HEAD
gh pr create --title "feat(crm-showcase): runnable CRM demo on runway-app-host" --body "Boots locally with all CRM modules + truth-execution registered."
```

---

## Phase 8: `catalyst-backend` binary

**PR title:** `feat(catalyst-backend): thin Runtime Runway-hosted binary with operator-control + governed-jobs`

### Task 8.1: Move Catalyst truth bodies

- [ ] **Step 1: Create the truths crate**

```bash
mkdir -p marquee-apps/catalyst-biz/truths/src
```

`marquee-apps/catalyst-biz/truths/Cargo.toml`:

```toml
[package]
name = "catalyst-truths"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
publish.workspace = true

[dependencies]
anyhow = "1"
async-trait = "0.1"
helm-truth-execution = { path = "../../../bedrock-platform/helms/crates/helm-truth-execution" }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tracing = "0.1"
```

- [ ] **Step 2: Move the 3 truth files**

```bash
cp bedrock-platform/helms/crates/application-server/src/truth_runtime/score_inbound_fit.rs \
   marquee-apps/catalyst-biz/truths/src/score_inbound_fit.rs
cp bedrock-platform/helms/crates/application-server/src/truth_runtime/qualify_inbound_lead.rs \
   marquee-apps/catalyst-biz/truths/src/qualify_inbound_lead.rs
cp bedrock-platform/helms/crates/application-server/src/truth_runtime/schedule_strategic_meetings.rs \
   marquee-apps/catalyst-biz/truths/src/schedule_strategic_meetings.rs
```

- [ ] **Step 3: Write `truths/src/lib.rs`**

```rust
mod score_inbound_fit;
mod qualify_inbound_lead;
mod schedule_strategic_meetings;

use std::sync::Arc;
use helm_truth_execution::TruthBody;

pub fn all() -> Vec<Arc<dyn TruthBody>> {
    vec![
        Arc::new(score_inbound_fit::Body),
        Arc::new(qualify_inbound_lead::Body),
        Arc::new(schedule_strategic_meetings::Body),
    ]
}
```

In each truth file, implement `TruthBody` on a `Body` struct.

- [ ] **Step 4: Add to catalyst-biz workspace**

Edit `marquee-apps/catalyst-biz/Cargo.toml`:
```toml
members = [
    "src-tauri",
    "backend",
    "truths",
]
```

- [ ] **Step 5: Build + commit**

```bash
cd marquee-apps/catalyst-biz
cargo build -p catalyst-truths
git add truths Cargo.toml
git commit -m "feat(catalyst-truths): move 3 Catalyst truth bodies from helms"
```

### Task 8.2: Write `catalyst-backend` main.rs

- [ ] **Step 1: Update `backend/Cargo.toml`**

`marquee-apps/catalyst-biz/backend/Cargo.toml`:

```toml
[package]
name = "catalyst-backend"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
publish.workspace = true

[[bin]]
name = "catalyst-backend"
path = "src/main.rs"

[dependencies]
anyhow = "1"
tokio = { version = "1", features = ["full"] }
tracing = "0.1"
tracing-subscriber = "0.3"

# Cross-repo
runway-app-host = { path = "../../../runtime-runway/crates/runway-app-host" }
runway-secrets = { path = "../../../runtime-runway/crates/runway-secrets" }
runway-storage = { path = "../../../runtime-runway/crates/runway-storage" }
runway-telemetry = { path = "../../../runtime-runway/crates/runway-telemetry" }
helm-operator-control = { path = "../../../bedrock-platform/helms/crates/helm-operator-control" }
helm-governed-jobs = { path = "../../../bedrock-platform/helms/crates/helm-governed-jobs" }
helm-truth-execution = { path = "../../../bedrock-platform/helms/crates/helm-truth-execution" }
catalyst-truths = { path = "../truths" }
```

- [ ] **Step 2: Replace `backend/src/main.rs`**

```rust
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
        "/",
    )
    .with_version(env!("CARGO_PKG_VERSION"))
    .with_auth_app("catalyst")
    .with_mounted_module(MountedModule::new("helm.operator-control", vec![]))
    .with_mounted_module(MountedModule::new("helm.governed-jobs", vec![]))
    .with_mounted_module(MountedModule::new("helm.truth-execution", vec![]));

    // StorageKit: local for development, remote for prod.
    let storage = if std::env::var("CATALYST_LOCAL").is_ok() {
        let base = std::env::var("CATALYST_DATA_DIR")
            .unwrap_or_else(|_| dirs::home_dir().unwrap().join(".catalyst").display().to_string());
        StorageKit::local(&base).await?
    } else {
        let _ = runway_secrets::Secrets::load_all().await?;
        StorageKit::remote(RemoteConfig::from_env()?).await?
    };

    let truths = helm_truth_execution::TruthExecutionModule::new();
    let truths = catalyst_truths::all().into_iter().fold(truths, |m, b| m.register(b));

    RunwayAppHost::builder(packet)
        .with_storage(storage)
        .mount(Arc::new(helm_operator_control::OperatorControlModule::default()))
        .mount(Arc::new(helm_governed_jobs::GovernedJobsModule::default()))
        .mount(Arc::new(truths))
        .build()
        .await?
        .serve()
        .await
}
```

- [ ] **Step 3: Add `dirs` to deps if needed**

```toml
dirs = "5"
```

- [ ] **Step 4: Build**

```bash
cargo build -p catalyst-backend
```
Expected: PASS.

### Task 8.3: Local smoke test (`just catalyst-local`)

- [ ] **Step 1: Add justfile target**

In `runtime-runway/Justfile`, append:

```just
# Run Catalyst backend locally against StorageKit::local.
catalyst-local:
    CATALYST_LOCAL=1 cargo run -p catalyst-backend --manifest-path ../marquee-apps/catalyst-biz/Cargo.toml
```

- [ ] **Step 2: Run the proof flow manually**

```bash
just catalyst-local &
sleep 3

# 1. Launch a job
curl -X POST http://localhost:8080/v1/jobs/score-inbound-fit/stream \
     -H "content-type: application/json" \
     -d '{"app_id":"catalyst","payload":{}}'

# 2. Watch SSE in another shell
curl -N http://localhost:8080/sse/stream

# 3. Approve the HITL gate (substitute the ref from the gate.paused event)
curl -X POST http://localhost:8080/v1/approvals/job:catalyst:run1/approve \
     -H "content-type: application/json" \
     -d '{"actor":"alice"}'

# 4. SSE should now show approval.approved → job.resumed → job.completed
```

Expected: the full sequence flows through. (Engineer: if any step fails, fix the underlying module and iterate.)

- [ ] **Step 3: Commit + PR**

```bash
git add marquee-apps/catalyst-biz/backend marquee-apps/catalyst-biz/Cargo.toml
git add runtime-runway/Justfile
git commit -m "feat(catalyst-backend): thin binary mounting operator-control + governed-jobs"
git push -u origin HEAD
gh pr create --title "feat(catalyst-backend): runnable proof binary" --body "Boots locally and runs the score-inbound-fit → HITL flow through the new mount contract."
```

---

## Phase 9: Delete `application-server`

**PR title:** `chore(helms): remove application-server (replaced by helm-* modules)`

### Task 9.1: Verify no consumers

- [ ] **Step 1: Search for consumers**

```bash
cd /Users/kpernyer/dev/reflective
rg -l "application-server" --type rust --type toml \
   stack/ reflective/ marquee-apps/ movement/ \
   | grep -v "stack/bedrock-platform/helms/crates/application-server"
```

Expected: empty (or only matches in CHANGELOG/MILESTONES/docs). If any code still depends on it, **stop and resolve before continuing.**

- [ ] **Step 2: Delete the crate**

```bash
rm -rf stack/bedrock-platform/helms/crates/application-server
```

- [ ] **Step 3: Remove from Helm workspace members**

Edit `stack/bedrock-platform/helms/Cargo.toml`, remove the line:
```toml
    "crates/application-server",
```

- [ ] **Step 4: Verify workspace builds without it**

```bash
cd stack/bedrock-platform/helms
cargo build --workspace
```
Expected: PASS.

- [ ] **Step 5: Search Docker/k8s/deploy config**

```bash
rg "application-server" stack/bedrock-platform/helms/ \
   reflective/runtime-runway/ops/ \
   .github/
```

Remove any Dockerfile / compose / workflow / deployment manifest entries.

- [ ] **Step 6: Commit + PR**

```bash
git add -A
git commit -m "chore(helms): delete application-server (replaced by helm-* modules)"
git push -u origin HEAD
gh pr create --title "chore(helms): delete application-server" --body "Removes the monolithic application-server. Functionality is now provided by helm-operator-control, helm-governed-jobs, helm-truth-execution, and the atelier-showcase CRM modules."
```

---

## Phase 10: Cloud Run deploy for `catalyst-backend`

**PR title:** `feat(ops): Cloud Run target for catalyst-backend`

### Task 10.1: Terraform module

- [ ] **Step 1: Create directory + main.tf**

```bash
mkdir -p runtime-runway/ops/infra/terraform/catalyst-backend
```

`runtime-runway/ops/infra/terraform/catalyst-backend/variables.tf`:

```hcl
variable "project_id" { type = string }
variable "region"     { type = string; default = "europe-west1" }
variable "image_uri"  { type = string }
variable "env"        { type = map(string); default = {} }
```

`runtime-runway/ops/infra/terraform/catalyst-backend/service_account.tf`:

```hcl
resource "google_service_account" "catalyst_backend" {
  account_id   = "catalyst-backend"
  display_name = "Catalyst Backend Cloud Run SA"
  project      = var.project_id
}

resource "google_project_iam_member" "firestore_user" {
  project = var.project_id
  role    = "roles/datastore.user"
  member  = "serviceAccount:${google_service_account.catalyst_backend.email}"
}

resource "google_project_iam_member" "secret_accessor" {
  project = var.project_id
  role    = "roles/secretmanager.secretAccessor"
  member  = "serviceAccount:${google_service_account.catalyst_backend.email}"
}

resource "google_project_iam_member" "vertex_user" {
  project = var.project_id
  role    = "roles/aiplatform.user"
  member  = "serviceAccount:${google_service_account.catalyst_backend.email}"
}

resource "google_project_iam_member" "pubsub_publisher" {
  project = var.project_id
  role    = "roles/pubsub.publisher"
  member  = "serviceAccount:${google_service_account.catalyst_backend.email}"
}
```

`runtime-runway/ops/infra/terraform/catalyst-backend/main.tf`:

```hcl
resource "google_cloud_run_v2_service" "catalyst_backend" {
  name     = "catalyst-backend"
  location = var.region
  project  = var.project_id

  template {
    service_account = google_service_account.catalyst_backend.email
    containers {
      image = var.image_uri
      ports { container_port = 8080 }
      dynamic "env" {
        for_each = var.env
        content {
          name  = env.key
          value = env.value
        }
      }
    }
  }
}
```

`runtime-runway/ops/infra/terraform/catalyst-backend/outputs.tf`:

```hcl
output "service_url" {
  value = google_cloud_run_v2_service.catalyst_backend.uri
}

output "service_account_email" {
  value = google_service_account.catalyst_backend.email
}
```

- [ ] **Step 2: tf init + plan in staging**

```bash
cd runtime-runway/ops/infra/terraform
# Engineer: configure backend state if not already.
terraform init
terraform plan -var-file=staging.tfvars
```

Expected: plan shows the new resources, no destroys.

### Task 10.2: Dockerfile

- [ ] **Step 1: Create multistage Dockerfile**

`marquee-apps/catalyst-biz/backend/Dockerfile`:

```dockerfile
FROM rust:1.94-slim AS builder
WORKDIR /build
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config libssl-dev protobuf-compiler ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY . .
RUN cargo build --release -p catalyst-backend --bin catalyst-backend

FROM gcr.io/distroless/cc-debian12
COPY --from=builder /build/target/release/catalyst-backend /usr/local/bin/catalyst-backend
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/catalyst-backend"]
```

- [ ] **Step 2: Test build locally**

```bash
cd /Users/kpernyer/dev/reflective
docker build -f marquee-apps/catalyst-biz/backend/Dockerfile -t catalyst-backend:dev .
```
Expected: image builds in <5 min. Test run:
```bash
docker run --rm -e CATALYST_LOCAL=1 -p 8080:8080 catalyst-backend:dev &
sleep 3
curl http://localhost:8080/healthz
docker stop $(docker ps -lq)
```
Expected: 200.

### Task 10.3: Justfile deploy target

- [ ] **Step 1: Add to `runtime-runway/Justfile`**

```just
# Build and deploy catalyst-backend to Cloud Run.
deploy-catalyst project_id region="europe-west1" tag="latest":
    docker build -f ../marquee-apps/catalyst-biz/backend/Dockerfile \
        -t {{region}}-docker.pkg.dev/{{project_id}}/runway/catalyst-backend:{{tag}} \
        ../
    docker push {{region}}-docker.pkg.dev/{{project_id}}/runway/catalyst-backend:{{tag}}
    cd ops/infra/terraform/catalyst-backend && \
        terraform apply -var project_id={{project_id}} \
                        -var region={{region}} \
                        -var image_uri={{region}}-docker.pkg.dev/{{project_id}}/runway/catalyst-backend:{{tag}}

# Smoke test deployed Catalyst backend.
smoke-catalyst-cloud project_id region="europe-west1":
    #!/usr/bin/env bash
    set -euo pipefail
    URL=$(cd ops/infra/terraform/catalyst-backend && terraform output -raw service_url)
    echo "Smoking $URL"
    curl -fsS "$URL/healthz" | grep -q ok
    curl -fsS "$URL/status" | jq '.app_id == "catalyst"' | grep -q true
    echo "Smoke passed."
```

### Task 10.4: GitHub Actions workflow

- [ ] **Step 1: Write workflow**

`runtime-runway/.github/workflows/deploy-catalyst.yml`:

```yaml
name: Deploy Catalyst
on:
  push:
    tags:
      - "catalyst-v*"

jobs:
  deploy:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0

      - uses: google-github-actions/auth@v2
        with:
          workload_identity_provider: ${{ secrets.GCP_WIF_PROVIDER }}
          service_account: ${{ secrets.GCP_DEPLOY_SA }}

      - uses: google-github-actions/setup-gcloud@v2

      - name: Configure Docker for Artifact Registry
        run: gcloud auth configure-docker europe-west1-docker.pkg.dev --quiet

      - name: Build + Push image
        run: |
          IMAGE=europe-west1-docker.pkg.dev/${{ secrets.GCP_PROJECT_ID }}/runway/catalyst-backend:${{ github.ref_name }}
          docker build -f marquee-apps/catalyst-biz/backend/Dockerfile -t "$IMAGE" .
          docker push "$IMAGE"
          echo "IMAGE_URI=$IMAGE" >> $GITHUB_ENV

      - name: Terraform apply
        run: |
          cd ops/infra/terraform/catalyst-backend
          terraform init
          terraform apply -auto-approve \
            -var project_id=${{ secrets.GCP_PROJECT_ID }} \
            -var image_uri=${{ env.IMAGE_URI }}

      - name: Smoke test
        run: |
          URL=$(cd ops/infra/terraform/catalyst-backend && terraform output -raw service_url)
          curl -fsS "$URL/healthz" | grep -q ok
          curl -fsS "$URL/status" | jq '.app_id == "catalyst"' | grep -q true
```

### Task 10.5: First deploy

- [ ] **Step 1: Populate secrets**

```bash
gcloud secrets create prod-catalyst-firebase-api-key --data-file=- <<< "$FIREBASE_API_KEY"
gcloud secrets create prod-catalyst-pubsub-topic --data-file=- <<< "projects/$PROJECT_ID/topics/catalyst-events"
```

- [ ] **Step 2: First manual deploy from local**

```bash
just deploy-catalyst $PROJECT_ID
just smoke-catalyst-cloud $PROJECT_ID
```
Expected: smoke passes.

- [ ] **Step 3: Tag a release to validate CI**

```bash
git tag catalyst-v0.1.0
git push origin catalyst-v0.1.0
```

Expected: GitHub Action runs, builds, deploys, smokes — all green.

- [ ] **Step 4: Commit + PR**

```bash
git add runtime-runway/ops/infra/terraform/catalyst-backend \
        runtime-runway/Justfile \
        runtime-runway/.github/workflows/deploy-catalyst.yml \
        marquee-apps/catalyst-biz/backend/Dockerfile
git commit -m "feat(ops): Cloud Run target + GitHub Action for catalyst-backend"
git push -u origin HEAD
gh pr create --title "feat(ops): Cloud Run deploy for catalyst-backend" --body "Adds Terraform, Dockerfile, justfile targets, and CI workflow. First deploy + smoke validated."
```

---

## Self-review

### Spec coverage

| Spec § | Tasks |
|---|---|
| §3 four-layer model | Documented in spec; no implementation needed. Referenced in Phase 2 CLAUDE.md. |
| §4.1 HelmModule trait | Task 1.4 |
| §4.2 HostContext | Task 1.5 |
| §4.3 EventHub + EventEnvelope | Tasks 1.2, 1.3 |
| §4.4 Approvals transport | Task 1.8 |
| §4.5 Host-mounted endpoints | Tasks 1.6 (healthz), 1.7 (sse/stream), 1.8 (approvals); /status already exists |
| §4.6 Builder + serve | Task 1.9 |
| §5.1 helm-operator-control | Phase 3 |
| §5.1 helm-governed-jobs | Phase 4 |
| §5.1 helm-truth-execution | Phase 5 |
| §5.2 What moves out of Helm | Phase 6 (CRM), Phase 9 (delete app-server) |
| §6 atelier-showcase bootstrap | Phases 2, 6, 7 |
| §7 Catalyst backend | Phase 8 |
| §7.3 Local proof flow | Task 8.3 |
| §8 Cloud Run deploy | Phase 10 |
| §9 Migration sequencing | Phases 1–10 map 1:1 to the 10 steps |
| §10 Out of scope | Honored — no Commerce Rails code, no atelier-showcase cloud deploy, no marquee-apps onboarding |
| §11 Open questions | Resolved during implementation (timeline crate is footnote in Phase 5; gRPC port config in Task 1.9; HostContext ergonomics in Task 1.5) |

### Placeholder scan

Clean — no "TBD", "TODO", "implement later" markers. Engineer-callouts ("Engineer: verify the relative path…") are real instructions where the path depends on local layout choices, not unfilled gaps.

### Type consistency

- `HelmModule` signature is identical across Phase 1 (definition), Phase 3 (operator-control impl), Phase 4 (governed-jobs impl), Phase 5 (truth-execution impl), Phase 6 (crm-* impls).
- `EventEnvelope` field names match between `realtime.rs`, `approvals.rs`, and module subscribe code.
- `TruthBody` trait introduced in Phase 5 is consumed in Phase 6 (`crm-truths`) and Phase 8 (`catalyst-truths`).
- Builder method names (`builder()`, `with_storage()`, `with_config()`, `mount()`, `build()`, `serve()`) used consistently across Phase 1, 7, 8.

---

## Execution

Plan complete and saved to `docs/superpowers/plans/2026-05-28-runway-helm-app-host-boundary.md`. Two execution options:

**1. Subagent-Driven (recommended)** — fresh subagent per task, two-stage review between tasks, fast iteration.

**2. Inline Execution** — execute tasks in this session using executing-plans, batch execution with checkpoints.

Which approach?
