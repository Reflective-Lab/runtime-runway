---
source: llm
---
# Changelog

## 2026-05-28 — Stripe billing boundary moved to Commerce Rails

- Removed the local Stripe client from `runway-accounts`.
- Runway now keeps billing HTTP routes, auth context, and the org entitlement mirror, then calls the Commerce Rails-owned `commerce-rails-stripe` adapter for provider config, Stripe API calls, webhook signature mechanics, receipt construction, and event mapping.
- `api-server` delegates commercial provider config to `CommerceRailsConfig` instead of carrying Stripe config fields directly.

## 2026-05-11 — api-server deployment spike

Added `crates/api-server` — a minimal Cloud Run binary that wires all five runway-* crates together and proves the deployment path end-to-end:

- Boots with `StorageKit::local()` (redb) in `LOCAL_DEV=true` mode; switches to `StorageKit::remote()` (Firestore + GCS + Vertex AI) in production
- `runway-telemetry::init()` called at startup for Cloud Trace + Sentry
- `runway-auth::AuthLayer` on all API routes; `AuthContext` injected into handlers
- `runway-middleware::stack()` wraps the router (request-id, OTel span, gzip, CORS, error body, `/health`)
- Routes: `GET /health` (open), `GET /api/me` (auth), `GET /api/events` (auth + query params), `POST /api/events` (auth + StoredEvent append)
- Fixed `runway-middleware::serve` — removed bogus `axum::handler::Handler` bound; now takes `Router<()>` directly
- `docker/Dockerfile.api-server` for Cloud Run packaging
- `ops/scripts/deploy-api-server.sh` for `gcloud builds submit` + `gcloud run deploy`
- `just api-up`, `just api-docker-build`, `just api-docker-run`, `just api-deploy` targets

## 2026-05-11 — Shared infrastructure crates (runway-*)

Added five new crates that form the shared infrastructure layer reused by all Reflective apps
(Folio, Wolfgang, Inkling, Scout, Quorum, Vouch, etc.):

- **`runway-storage`** — `StorageKit` with two swappable backends:
  - Local (Tauri offline): redb (document + event + vector), local FS (objects), fastembed (embeddings)
  - Remote (Cloud Run): Firestore REST v1, GCS JSON API, Vertex AI Matching Engine, Vertex AI text-multilingual-embedding-002
  - Embedding standard: 768-dim (Vertex AI compatible); offline vectors zero-padded, re-embedded on sync
  - `EventLog` has dedup (OR-IGNORE on event_id) and unsynced index for offline→cloud sync
- **`runway-auth`** — Firebase Auth Tower Layer: verifies Bearer tokens via Identity Toolkit, injects `AuthContext` with custom claims (`org_id`, `apps`, `role`)
- **`runway-middleware`** — Axum stack: request-id (UUID), OTel trace span, gzip, CORS, JSON error body, graceful SIGTERM, `/health`
- **`runway-secrets`** — GCP Secret Manager client with `SecretString` (zeroized), `SecretMap::require()` for startup validation
- **`runway-telemetry`** — OTel OTLP/HTTP → Cloud Trace, Sentry, JSON structured logging; `TelemetryGuard` for clean shutdown

Resolved three consecutive dependency conflicts during build:
1. LanceDB removed (half version conflict with burn-core)
2. sqlx removed (libsqlite3-sys links conflict with rusqlite via burn-dataset)
3. Final solution: redb (pure Rust, no system library, ACID embedded DB) for all local storage

Quality: `just lint` (fmt + clippy pedantic) passes clean on all five crates.

## 2026-04-19 — Converge dependency pinning

- Runway now pins Converge library crates to GitHub tag `v3.4.0` by default instead of always reading sibling path dependencies.
- Local SDK work now uses an untracked Cargo patch override (`.cargo/config.toml`) generated from `.cargo/config.toml.example`.
- Runtime helper scripts now read runtime source from `~/dev/reflective/stack/bedrock-platform/converge` or `CONVERGE_ROOT`.
- All tracked cross-repo dependencies now resolve through GitHub pins, while local sibling overrides remain opt-in and untracked.
- Quality stamp: `just lint`, `just check`, and `just test` all passed on 2026-04-19 before tagging `v3.4.0`.

## 2026-04-19 — Initial split from converge

Runway created by extracting distribution and infrastructure from the converge repo:

- `crates/application` — the `converge` CLI/TUI binary (was `converge/crates/application`)
- `crates/llm` — local LLM inference (was `converge/dev/llm`)
- `docker/` — container definitions (was `converge/dev/docker`)
- `ops/` — deployment scripts and GPU infra (was `converge/ops`)

Converge stays as a pure SDK/runtime library. Runway owns everything needed to run, package, and deploy.

Runway initially depended on converge crates via sibling path (`../converge/crates/...`).
