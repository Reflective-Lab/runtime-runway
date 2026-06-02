# Runtime Config Consolidation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace scattered `std::env::var(...)` calls in the runway crates with typed config structs loaded once at startup and passed into services via constructors.

**Architecture:** Extend the existing inject-via-constructor pattern (`AppExecutionPacket`, `RemoteConfig`) to runtime env-var inputs. Each consuming crate exposes a typed parameter (e.g. `local_dev: bool`, or a small `AccountsConfig`/`HostConfig`). The binary (`api-server::main` and `runway-app-host::from_env`) reads env once into a composed `RunwayConfig`/`HostConfig`, then wires services with field values. No new shared crate. No global statics. Crate-local compile-time `const`s stay where they are.

**Tech Stack:** Rust, Axum, anyhow, tower, tracing.

**Scope (in):** `LOCAL_DEV`, `STORAGE_PATH`, `FIREBASE_PROJECT_ID` (+ `GOOGLE_CLOUD_PROJECT`/`GCP_PROJECT_ID` fallbacks), `ROUTE_PREFIX`, `APP_URL`, production assertions for `STRIPE_WEBHOOK_SECRET` and `ALLOWED_ORIGINS`.

**Scope (out, follow-up):** Stripe key vars (`STRIPE_SECRET_KEY`, `STRIPE_PRICE_*`), GCP metadata URL deduplication across `runway-accounts/claims.rs`, `runway-secrets/lib.rs`, `runway-storage/remote/mod.rs`. Googleapis base URLs inside `runway-storage`. These deserve their own pass.

**Verification gate:** `just lint` (= `cargo fmt --check && cargo clippy -- -D warnings`) and `cargo test --all-targets` must pass before each commit. The existing test suite is the regression net for behavior-preserving steps; new tests are added only for the new typed surfaces.

---

## File Structure

| File | Change |
|---|---|
| `crates/runway-auth/src/middleware.rs` | Add `local_dev: bool` to `AuthLayer` + `AuthMiddleware`; remove per-request `env::var` read |
| `crates/runway-accounts/src/claims.rs` | `ClaimsService::new(client, local_dev)`; remove env read |
| `crates/runway-accounts/src/lib.rs` | `AccountsState::new(storage, accounts_config)` taking a small `AccountsConfig` |
| `crates/runway-accounts/src/config.rs` *(new)* | `AccountsConfig { local_dev, app_url }`; `from_env` constructor |
| `crates/runway-accounts/src/http/billing.rs` | Read `app_url` from `AccountsState` instead of env |
| `crates/api-server/src/main.rs` | Replace inline `env::var` calls with `RunwayConfig::from_env()?` |
| `crates/api-server/src/config.rs` *(new)* | `RunwayConfig`; `from_env` with production assertions |
| `crates/runway-app-host/src/lib.rs` | Extract private `firebase_project_id()` / `route_prefix()` into `HostConfig`; `from_env` becomes wrapper over `with_config` |
| `crates/runway-app-host/src/config.rs` *(new)* | `HostConfig`; `from_env(packet)` constructor |

---

## Task 0: Commit pre-existing WIP

**Files:** the in-flight diff in `runtime-runway/` (rustfmt cosmetics + dev-claims tweak in auth middleware).

- [ ] **Step 1: Inspect WIP**

```bash
cd /Users/kpernyer/dev/reflective/runtime-runway
git status
git diff --stat
```

Expected: cosmetic diffs in `claims.rs`, `lib.rs` (accounts), `main.rs` (api-server) plus a small `apps` accumulator addition in `middleware.rs`. Untracked `crates/runway-app-host/` directory.

- [ ] **Step 2: If WIP is intentional, commit it first so the refactor diff stays clean**

The decision belongs to the user — the executor should pause and ask. If the user confirms, commit as a separate prep commit so the env-var refactor lands on a clean base. If the user wants to discard, `git restore` the files.

---

## Task 1: runway-auth — typed `local_dev`

**Files:**
- Modify: `crates/runway-auth/src/middleware.rs`
- Test: `crates/runway-auth/src/middleware.rs` (`#[cfg(test)] mod tests`)

- [ ] **Step 1: Write the failing test**

Append to `crates/runway-auth/src/middleware.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auth_layer_stores_local_dev_flag() {
        let auth = FirebaseAuth::new("dev-project".to_string());
        let layer = AuthLayer::new(auth, true);
        assert!(layer.local_dev, "expected local_dev=true to be stored on AuthLayer");
    }

    #[test]
    fn auth_layer_defaults_to_strict_in_prod() {
        let auth = FirebaseAuth::new("dev-project".to_string());
        let layer = AuthLayer::new(auth, false);
        assert!(!layer.local_dev);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cd /Users/kpernyer/dev/reflective/runtime-runway
cargo test -p runway-auth auth_layer_stores_local_dev_flag
```

Expected: FAIL — `local_dev` field does not exist on `AuthLayer`, and `AuthLayer::new` takes one argument.

- [ ] **Step 3: Modify `AuthLayer` to carry `local_dev`**

In `crates/runway-auth/src/middleware.rs`, replace lines 62–82 with:

```rust
/// Tower layer that validates a Firebase Bearer token and injects `AuthContext`.
#[derive(Clone)]
pub struct AuthLayer {
    auth: Arc<FirebaseAuth>,
    /// If set, only allow requests where the org has access to this app.
    required_app: Option<String>,
    /// When true, accept the bypass token `"dev"` and inject a canned context.
    /// Read once at construction; never re-read from env per request.
    pub(crate) local_dev: bool,
}

impl AuthLayer {
    pub fn new(auth: FirebaseAuth, local_dev: bool) -> Self {
        Self {
            auth: Arc::new(auth),
            required_app: None,
            local_dev,
        }
    }

    pub fn requiring_app(mut self, app: impl Into<String>) -> Self {
        self.required_app = Some(app.into());
        self
    }
}
```

Then replace the `Layer` impl (lines 84–94) to forward the flag:

```rust
impl<S> Layer<S> for AuthLayer {
    type Service = AuthMiddleware<S>;

    fn layer(&self, inner: S) -> Self::Service {
        AuthMiddleware {
            inner,
            auth: self.auth.clone(),
            required_app: self.required_app.clone(),
            local_dev: self.local_dev,
        }
    }
}
```

Add `local_dev: bool` to `AuthMiddleware` (line 97 struct):

```rust
#[derive(Clone)]
pub struct AuthMiddleware<S> {
    inner: S,
    auth: Arc<FirebaseAuth>,
    required_app: Option<String>,
    local_dev: bool,
}
```

In `fn call` (around line 118), capture the flag and replace the env-var read at line 131:

```rust
fn call(&mut self, mut req: Request) -> Self::Future {
    let auth = self.auth.clone();
    let required_app = self.required_app.clone();
    let local_dev = self.local_dev;
    let mut inner = self.inner.clone();

    Box::pin(async move {
        let token = extract_bearer(req.headers());
        let token = match token {
            Ok(t) => t,
            Err(e) => return Ok(e.into_response()),
        };

        // In LOCAL_DEV mode, accept "dev" as a bypass token and inject a canned context.
        let claims = if local_dev && token == "dev" {
            // ... (rest of the dev-claims block stays as-is)
```

Leave the `apps` accumulator logic from the pre-existing WIP intact.

- [ ] **Step 4: Run test to verify it passes**

```bash
cargo test -p runway-auth
```

Expected: PASS, all auth tests including the two new ones.

- [ ] **Step 5: Update call sites (compile fix)**

`crates/api-server/src/main.rs:56-57`:

```rust
let auth = FirebaseAuth::new(project_id);
let auth_layer = AuthLayer::new(auth, local_dev);
```

`crates/runway-app-host/src/lib.rs:351`:

```rust
let auth_layer = AuthLayer::new(FirebaseAuth::new(firebase_project_id()), local_dev_flag())
    .requiring_app(self.packet.required_auth_app());
```

(where `local_dev_flag()` is a temporary private helper at the bottom of `lib.rs`:
```rust
fn local_dev_flag() -> bool {
    std::env::var("LOCAL_DEV").as_deref() == Ok("true")
}
```
This helper is removed in Task 4 when `HostConfig` lands. It exists for one task only to keep the workspace compiling.)

- [ ] **Step 6: Verify the workspace still compiles**

```bash
cargo check --workspace
```

Expected: clean.

- [ ] **Step 7: Commit**

```bash
git add crates/runway-auth/src/middleware.rs \
        crates/api-server/src/main.rs \
        crates/runway-app-host/src/lib.rs
git commit -m "refactor(runway-auth): take local_dev via constructor instead of env

Removes per-request std::env::var('LOCAL_DEV') lookup from the auth
middleware hot path. AuthLayer::new now takes local_dev as a typed
bool parameter, set once by the binary at startup."
```

---

## Task 2: runway-accounts — `AccountsConfig`

**Files:**
- Create: `crates/runway-accounts/src/config.rs`
- Modify: `crates/runway-accounts/src/claims.rs`
- Modify: `crates/runway-accounts/src/lib.rs`
- Modify: `crates/runway-accounts/src/http/billing.rs`

- [ ] **Step 1: Write the new module**

Create `crates/runway-accounts/src/config.rs`:

```rust
//! Runtime configuration for the accounts crate.
//!
//! Populated by the binary at startup from env vars and passed into
//! `AccountsState::new`. Library code reads config from the struct,
//! never directly from the process environment.

#[derive(Debug, Clone)]
pub struct AccountsConfig {
    /// LOCAL_DEV mode: skip side-effects (e.g. Firebase custom claims).
    pub local_dev: bool,
    /// Base URL used when generating Stripe checkout/portal return URLs.
    /// Production deployments must set this explicitly.
    pub app_url: String,
}

impl AccountsConfig {
    /// Convenience for tests and local development.
    pub fn local() -> Self {
        Self {
            local_dev: true,
            app_url: "http://localhost:3000".to_string(),
        }
    }
}
```

- [ ] **Step 2: Write a failing test for `ClaimsService::new` taking `local_dev`**

Append to `crates/runway-accounts/src/claims.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claims_service_stores_local_dev_flag() {
        let client = reqwest::Client::new();
        let svc = ClaimsService::new(client, true);
        assert!(svc.local_dev);
    }
}
```

- [ ] **Step 3: Run to verify failure**

```bash
cargo test -p runway-accounts claims_service_stores_local_dev_flag
```

Expected: FAIL — `ClaimsService::new` takes 1 argument.

- [ ] **Step 4: Update `ClaimsService::new` to take the flag**

In `crates/runway-accounts/src/claims.rs`, replace lines 9–13:

```rust
impl ClaimsService {
    pub fn new(client: reqwest::Client, local_dev: bool) -> Self {
        Self { client, local_dev }
    }
```

The `local_dev` field on the struct (line 6) already exists and stays.

- [ ] **Step 5: Update `AccountsState`**

In `crates/runway-accounts/src/lib.rs`, add the module and update the constructor. Replace lines 1–35 with:

```rust
pub mod claims;
pub mod config;
pub mod domain;
pub mod error;
mod http;
pub mod store;
pub mod stripe;

pub use claims::ClaimsService;
pub use config::AccountsConfig;
pub use domain::{Account, Org, OrgInvite, OrgMember, Plan, Role};
pub use error::AccountError;
pub use store::AccountStore;
pub use stripe::StripeClient;

use std::sync::Arc;

use axum::{Router, routing};
use runway_storage::StorageKit;

#[derive(Clone)]
pub struct AccountsState {
    pub store: AccountStore,
    pub stripe: StripeClient,
    pub claims: ClaimsService,
    pub config: AccountsConfig,
}

impl AccountsState {
    pub fn new(storage: Arc<StorageKit>, config: AccountsConfig) -> Self {
        let client = reqwest::Client::new();
        Self {
            store: AccountStore::new(storage),
            stripe: StripeClient::new(client.clone()),
            claims: ClaimsService::new(client, config.local_dev),
            config,
        }
    }
}
```

- [ ] **Step 6: Replace `APP_URL` env reads in billing.rs**

Find the two existing sites (around lines 103 and 154) and replace each `std::env::var("APP_URL").unwrap_or_else(|_| "https://apps.reflective.se".into())` with `state.config.app_url.clone()` (or `&state.config.app_url`, whichever matches the surrounding type).

Use this command to locate them precisely:

```bash
grep -n 'APP_URL' crates/runway-accounts/src/http/billing.rs
```

Inspect each match and swap. There should be no remaining `APP_URL` references in this file after the edit.

- [ ] **Step 7: Update the call site in `api-server`**

`crates/api-server/src/main.rs:59`. Replace:

```rust
let accounts = AccountsState::new(Arc::clone(&storage));
```

with:

```rust
let accounts_config = AccountsConfig {
    local_dev,
    app_url: std::env::var("APP_URL").unwrap_or_else(|_| "https://apps.reflective.se".into()),
};
let accounts = AccountsState::new(Arc::clone(&storage), accounts_config);
```

Add the import at the top of `main.rs`:

```rust
use runway_accounts::{AccountsConfig, AccountsState};
```

(`AccountsConfig` will be folded into `RunwayConfig` in Task 3 — this is the bridging form.)

- [ ] **Step 8: Run tests and lint**

```bash
cargo test --workspace
just lint
```

Expected: PASS, clean.

- [ ] **Step 9: Commit**

```bash
git add crates/runway-accounts/src/config.rs \
        crates/runway-accounts/src/claims.rs \
        crates/runway-accounts/src/lib.rs \
        crates/runway-accounts/src/http/billing.rs \
        crates/api-server/src/main.rs
git commit -m "refactor(runway-accounts): inject AccountsConfig instead of reading env

Replaces scattered std::env::var calls for LOCAL_DEV and APP_URL with
a typed AccountsConfig struct populated once by the binary."
```

---

## Task 3: api-server — `RunwayConfig`

**Files:**
- Create: `crates/api-server/src/config.rs`
- Modify: `crates/api-server/src/main.rs`

- [ ] **Step 1: Write the new config module**

Create `crates/api-server/src/config.rs`:

```rust
//! Top-level runtime configuration for the api-server binary.
//!
//! Loaded once from environment variables in `main`, then passed by
//! field-value into the services that need them. Library crates never
//! read env directly.

use anyhow::{Context, Result};

#[derive(Debug, Clone)]
pub struct RunwayConfig {
    pub local_dev: bool,
    pub storage_path: String,
    pub firebase_project_id: String,
    pub route_prefix: Option<String>,
    pub app_url: String,
}

impl RunwayConfig {
    pub fn from_env() -> Result<Self> {
        let local_dev = std::env::var("LOCAL_DEV").as_deref() == Ok("true");

        if !local_dev {
            let stripe_secret = std::env::var("STRIPE_WEBHOOK_SECRET").unwrap_or_default();
            anyhow::ensure!(
                !stripe_secret.is_empty(),
                "STRIPE_WEBHOOK_SECRET must be set in production (empty value disables HMAC verification)"
            );
            let allowed_origins = std::env::var("ALLOWED_ORIGINS").unwrap_or_default();
            anyhow::ensure!(
                !allowed_origins.is_empty(),
                "ALLOWED_ORIGINS must be set in production (e.g. https://apps.reflective.se)"
            );
        }

        let storage_path =
            std::env::var("STORAGE_PATH").unwrap_or_else(|_| "/tmp/api-server".to_string());

        let firebase_project_id = std::env::var("FIREBASE_PROJECT_ID")
            .or_else(|_| std::env::var("GOOGLE_CLOUD_PROJECT"))
            .or_else(|_| std::env::var("GCP_PROJECT_ID"))
            .unwrap_or_else(|_| "dev-project".to_string());

        let route_prefix = match std::env::var("ROUTE_PREFIX") {
            Ok(prefix) => {
                let trimmed = prefix.trim();
                if trimmed.is_empty() || trimmed == "/" {
                    None
                } else if trimmed.starts_with('/') {
                    Some(trimmed.to_string())
                } else {
                    Some(format!("/{trimmed}"))
                }
            }
            Err(_) => None,
        };

        let app_url =
            std::env::var("APP_URL").unwrap_or_else(|_| "https://apps.reflective.se".to_string());

        Ok(Self {
            local_dev,
            storage_path,
            firebase_project_id,
            route_prefix,
            app_url,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_prefix_normalizes() {
        // SAFETY: tests in this module run single-threaded under cargo test
        // by default for the same binary target; we mutate process env.
        unsafe {
            std::env::set_var("LOCAL_DEV", "true");
            std::env::set_var("ROUTE_PREFIX", "api-server");
        }
        let cfg = RunwayConfig::from_env().expect("from_env in local_dev");
        assert_eq!(cfg.route_prefix.as_deref(), Some("/api-server"));

        unsafe {
            std::env::set_var("ROUTE_PREFIX", "/");
        }
        let cfg = RunwayConfig::from_env().unwrap();
        assert_eq!(cfg.route_prefix, None);

        unsafe {
            std::env::remove_var("ROUTE_PREFIX");
            std::env::remove_var("LOCAL_DEV");
        }
    }

    #[test]
    fn production_requires_stripe_webhook_secret() {
        unsafe {
            std::env::remove_var("LOCAL_DEV");
            std::env::remove_var("STRIPE_WEBHOOK_SECRET");
            std::env::set_var("ALLOWED_ORIGINS", "https://example.com");
        }
        let err = RunwayConfig::from_env().expect_err("should require STRIPE_WEBHOOK_SECRET");
        assert!(err.to_string().contains("STRIPE_WEBHOOK_SECRET"));
        unsafe {
            std::env::remove_var("ALLOWED_ORIGINS");
        }
    }
}
```

Note on the `unsafe { std::env::set_var(...) }` blocks: Rust 2024 marks env-var mutation unsafe because it is not thread-safe. The two tests above mutate process-wide state. If the workspace already has other env-mutating tests, follow the local convention (e.g. a `serial_test` attribute). If not, accept the small risk for two assertions; the production assertion path is not otherwise exercised by tests.

- [ ] **Step 2: Run the new tests**

```bash
cargo test -p api-server --bin api-server config::
```

Expected: PASS.

- [ ] **Step 3: Wire `RunwayConfig` into `main.rs`**

Replace lines 25–94 of `crates/api-server/src/main.rs` with:

```rust
mod config;

use config::RunwayConfig;

#[tokio::main]
async fn main() -> Result<()> {
    let _telemetry = init_telemetry(TelemetryConfig::from_env("api-server"))?;

    let cfg = RunwayConfig::from_env()?;

    let storage = if cfg.local_dev {
        StorageKit::local(&cfg.storage_path).await?
    } else {
        RemoteStorageKit::build(RemoteConfig::from_env()?).await?
    };

    let storage = Arc::new(storage);

    let auth = FirebaseAuth::new(cfg.firebase_project_id.clone());
    let auth_layer = AuthLayer::new(auth, cfg.local_dev);

    let accounts_config = AccountsConfig {
        local_dev: cfg.local_dev,
        app_url: cfg.app_url.clone(),
    };
    let accounts = AccountsState::new(Arc::clone(&storage), accounts_config);

    // Public routes: no auth required.
    let public = Router::new()
        .route("/status", get(status))
        .merge(runway_accounts::public_routes(accounts.clone()));

    // Protected API routes — served with AppState.
    let api_protected: Router<()> = Router::new()
        .route("/api/me", get(me))
        .route("/api/events", get(list_events).post(append_event))
        .with_state(AppState {
            storage: Arc::clone(&storage),
        });

    // Protected accounts routes — served with AccountsState (already called with_state).
    let accounts_protected: Router<()> = runway_accounts::protected_routes(accounts);

    // Merge all protected routes then apply the auth layer once.
    let protected = api_protected.merge(accounts_protected).layer(auth_layer);

    // ROUTE_PREFIX=/api-server mounts all routes under that path.
    // Firebase Hosting rewrites pass the full path through, so this lets
    // apps.reflective.se/api-server/** route to this service.
    // /health always stays at root for Cloud Run health checks.
    let routed = match cfg.route_prefix.as_deref() {
        Some(prefix) => Router::new().nest(prefix, public.merge(protected)),
        None => public.merge(protected),
    };

    let app = stack(routed);

    info!("api-server starting");
    serve(app).await;
    Ok(())
}
```

- [ ] **Step 4: Verify build + tests + lint**

```bash
cargo test --workspace
just lint
```

Expected: clean.

- [ ] **Step 5: Smoke-test the binary locally**

```bash
LOCAL_DEV=true cargo run -p api-server &
sleep 2
curl -s http://127.0.0.1:8080/status
kill %1
```

Expected: JSON response with `"status": "ok"`. Confirms `from_env` + wiring produce equivalent behavior to the old inline reads. (Port may differ — confirm against `runway-middleware::serve`.)

- [ ] **Step 6: Commit**

```bash
git add crates/api-server/src/config.rs crates/api-server/src/main.rs
git commit -m "refactor(api-server): introduce RunwayConfig::from_env

Single load of LOCAL_DEV, STORAGE_PATH, FIREBASE_PROJECT_ID,
ROUTE_PREFIX, APP_URL plus production assertions for
STRIPE_WEBHOOK_SECRET and ALLOWED_ORIGINS. Services receive typed
fields instead of reading env at construction."
```

---

## Task 4: runway-app-host — `HostConfig`

**Files:**
- Create: `crates/runway-app-host/src/config.rs`
- Modify: `crates/runway-app-host/src/lib.rs`

- [ ] **Step 1: Write the new module**

Create `crates/runway-app-host/src/config.rs`:

```rust
//! Runtime configuration for the app-host (the binary container that
//! runs a Runtime Runway app inside Cloud Run or locally).

use crate::AppExecutionPacket;

#[derive(Debug, Clone)]
pub struct HostConfig {
    pub local_dev: bool,
    pub storage_path: String,
    pub firebase_project_id: String,
    pub route_prefix: Option<String>,
}

impl HostConfig {
    /// Read from process environment, falling back to packet-supplied
    /// defaults where applicable.
    pub fn from_env(packet: &AppExecutionPacket) -> Self {
        let local_dev = std::env::var("LOCAL_DEV").as_deref() == Ok("true");

        let storage_path =
            std::env::var("STORAGE_PATH").unwrap_or_else(|_| format!("/tmp/{}", packet.app_id));

        let firebase_project_id = std::env::var("FIREBASE_PROJECT_ID")
            .or_else(|_| std::env::var("GOOGLE_CLOUD_PROJECT"))
            .or_else(|_| std::env::var("GCP_PROJECT_ID"))
            .unwrap_or_else(|_| "dev-project".to_string());

        let route_prefix = {
            let raw = std::env::var("ROUTE_PREFIX").unwrap_or_else(|_| packet.route_prefix.clone());
            let trimmed = raw.trim();
            if trimmed.is_empty() || trimmed == "/" {
                None
            } else if trimmed.starts_with('/') {
                Some(trimmed.to_string())
            } else {
                Some(format!("/{trimmed}"))
            }
        };

        Self {
            local_dev,
            storage_path,
            firebase_project_id,
            route_prefix,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_prefix_falls_back_to_packet() {
        let packet = AppExecutionPacket::new("my-app", "My App", "desc", "my-app");
        unsafe {
            std::env::remove_var("ROUTE_PREFIX");
            std::env::set_var("LOCAL_DEV", "true");
        }
        let cfg = HostConfig::from_env(&packet);
        assert_eq!(cfg.route_prefix.as_deref(), Some("/my-app"));
        unsafe { std::env::remove_var("LOCAL_DEV"); }
    }
}
```

- [ ] **Step 2: Add `mod config;` and re-export**

In `crates/runway-app-host/src/lib.rs`, near the top (alongside `use` lines), add:

```rust
mod config;
pub use config::HostConfig;
```

- [ ] **Step 3: Replace `RunwayAppHost::from_env` to use `HostConfig`**

Replace the current `impl RunwayAppHost { pub async fn from_env(...) }` and the two private helpers (`firebase_project_id`, `route_prefix`) at lines 381–398.

New body (replacing lines 308–333 and removing 381–398 entirely):

```rust
impl RunwayAppHost {
    pub async fn from_env(packet: AppExecutionPacket) -> Result<Self> {
        let config = HostConfig::from_env(&packet);
        Self::with_config(packet, config).await
    }

    pub async fn with_config(packet: AppExecutionPacket, config: HostConfig) -> Result<Self> {
        let _telemetry = init_telemetry(TelemetryConfig::from_env(&packet.app_id))?;

        let storage = if config.local_dev {
            StorageKit::local(&config.storage_path).await?
        } else {
            StorageKit::remote(RemoteConfig::from_env()?).await?
        };

        tracing::info!(
            app_id = %packet.app_id,
            route_prefix = ?config.route_prefix,
            local_dev = config.local_dev,
            "runway app host initialized"
        );

        Ok(Self {
            packet: Arc::new(packet),
            storage: Arc::new(storage),
            config,
            _telemetry,
        })
    }
```

Update the `RunwayAppHost` struct (lines 302–306) to carry `config`:

```rust
pub struct RunwayAppHost {
    packet: Arc<AppExecutionPacket>,
    storage: Arc<StorageKit>,
    config: HostConfig,
    _telemetry: TelemetryGuard,
}
```

Update `router` (lines 350–361) to use `self.config` instead of the deleted private helpers:

```rust
pub fn router(&self, public_routes: Router, protected_routes: Router) -> Router {
    let auth_layer =
        AuthLayer::new(FirebaseAuth::new(self.config.firebase_project_id.clone()), self.config.local_dev)
            .requiring_app(self.packet.required_auth_app());
    let protected = protected_routes.layer(auth_layer);
    let public = self.status_routes().merge(public_routes);
    let routed = match self.config.route_prefix.as_deref() {
        Some(prefix) => Router::new().nest(prefix, public.merge(protected)),
        None => public.merge(protected),
    };

    stack(routed)
}
```

Delete the temporary `local_dev_flag()` helper added in Task 1.

- [ ] **Step 4: Build + test + lint**

```bash
cargo test --workspace
just lint
```

Expected: clean.

- [ ] **Step 5: Commit**

```bash
git add crates/runway-app-host/src/config.rs crates/runway-app-host/src/lib.rs
git commit -m "refactor(runway-app-host): introduce HostConfig

Replaces inline std::env::var reads with a typed HostConfig populated
once via HostConfig::from_env(&packet). RunwayAppHost::with_config
allows tests to construct hosts without setting process env."
```

---

## Task 5: Final sweep

- [ ] **Step 1: Confirm no remaining env reads in the targeted scope**

```bash
cd /Users/kpernyer/dev/reflective/runtime-runway
grep -rn 'env::var("LOCAL_DEV"\|env::var("STORAGE_PATH"\|env::var("FIREBASE_PROJECT_ID"\|env::var("ROUTE_PREFIX"\|env::var("APP_URL"' crates/ \
  | grep -v 'src/config.rs'
```

Expected: empty (apart from the two `config.rs` files which own the reads).

- [ ] **Step 2: Full verification**

```bash
just lint
cargo test --workspace
just build-quick
```

Expected: all clean.

- [ ] **Step 3: Update kb if architecture page exists**

```bash
ls kb/Architecture/
```

If a "Configuration" or "Runtime Config" page exists, update it to describe the new pattern (RunwayConfig / HostConfig / AccountsConfig, loaded once at startup, passed via constructors). If not, do not create one in this pass — kb hygiene is a separate concern.

---

## Self-Review

**Spec coverage:**
- LOCAL_DEV (4 crates) → Task 1 (auth), Task 2 (accounts), Task 3 (api-server), Task 4 (app-host). ✓
- STORAGE_PATH (2 crates) → Task 3 + Task 4. ✓
- FIREBASE_PROJECT_ID + fallbacks (2 crates) → Task 3 + Task 4. ✓
- ROUTE_PREFIX (2 crates) → Task 3 + Task 4. ✓
- APP_URL (1 crate) → Task 2 + Task 3. ✓
- STRIPE_WEBHOOK_SECRET / ALLOWED_ORIGINS production assertions → Task 3. ✓
- Per-request env::var read in auth hot path → Task 1 fixes. ✓

**Out-of-scope (documented above, deferred):** Stripe keys, GCP metadata URL dedup, googleapis base URLs in storage. Each warrants its own pass once this pattern is established.

**Type consistency check:**
- `AuthLayer::new(auth, local_dev: bool)` — used same way in api-server (Task 1 step 5) and app-host (Task 4 step 3). ✓
- `AccountsState::new(storage, AccountsConfig)` — bridge form in Task 2 step 7, final form in Task 3 step 3. Both use `AccountsConfig { local_dev, app_url }`. ✓
- `HostConfig` field names (`local_dev`, `storage_path`, `firebase_project_id`, `route_prefix`) match `RunwayConfig` field names. ✓
