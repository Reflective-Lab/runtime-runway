# Milestones

Strategic milestones for getting Reflective apps online. Each milestone is a shippable state.

---

## Immediate priority — Canonical app execution container

Stop the drift toward app-owned backend servers. Runway should provide the
standard execution container, Helm should mount operator-control/job modules
into it, and marquee apps should instantiate it with typed app packets.

- [x] Document the Runway/Helm/app execution-container boundary
- [x] Define the first `AppExecutionPacket` shape using Catalyst
- [ ] Split Helm `application-server` responsibilities into Runway host concerns
      and Helm module concerns
- [x] Extract reusable Runway host construction into `crates/runway-app-host`
- [ ] Mount Helm operator-control and governed-job routes into the Runway host
- [ ] Prove Catalyst through the Runway container before adding new app-local
      backend patterns

---

## M1 — Shared infrastructure compiles ✅ DONE 2026-05-11

All five `runway-*` crates build and pass `just lint`:

- [x] `runway-storage` — StorageKit with local (redb) and remote (Firestore/GCS/Vertex AI) backends
- [x] `runway-auth` — Firebase Auth Tower middleware
- [x] `runway-middleware` — Axum middleware stack (request-id, trace, CORS, gzip, graceful shutdown)
- [x] `runway-secrets` — GCP Secret Manager client (SecretString, zeroized)
- [x] `runway-telemetry` — OTel OTLP/HTTP → Cloud Trace, Sentry, JSON logs

---

## M2 — GCP project ready for production traffic

Infrastructure provisioned via Terraform, security rules live, billing connected.

**Terraform (ops/infra/terraform/)**
- [ ] Audit all 9 modules: apis, firestore, spanner, storage, pubsub, bigquery, vertex-vector, memorystore, releases
- [ ] Add IAM service accounts + least-privilege bindings to each module
- [ ] `just tf-init` / `just tf-plan` / `just tf-apply` targets in justfile
- [ ] `terraform.tfvars` for prod + staging environments

**Firebase (ops/infra/firebase/)**
- [ ] Deploy `firestore.rules` and `storage.rules` via `firebase deploy --only firestore:rules,storage`
- [ ] Deploy `firestore.indexes.json` via `firebase deploy --only firestore:indexes`
- [ ] Firebase Auth: enable custom claims flow (set by backend on org creation)

**Releases CDN**
- [ ] `reflective.se/downloads` static page (SvelteKit or plain HTML) — detects OS/arch, fetches `latest.json`
- [ ] `latest.json` per app: `{ version, files: { "darwin-aarch64": { url, sha256 }, ... } }`

**Secrets + billing**
- [ ] Populate Secret Manager: `prod-platform-firebase-api-key`, `prod-platform-stripe-webhook-secret`
- [ ] Stripe billing webhook handler (shared Cloud Run) deployed

---

## M3 — Reference app wired (Wolfgang or Inkling)

One app uses all five runway crates end-to-end in its Cloud Run backend.

- [ ] `runway-telemetry::init()` called at startup; traces flowing to Cloud Trace
- [ ] `runway-secrets::Secrets::load_all()` at startup; fails fast on missing secrets
- [ ] `runway-storage::StorageKit::remote()` initialized with `RemoteConfig::from_env()`
- [ ] `runway-auth::AuthLayer` on all protected Axum routes; `AuthContext` available in handlers
- [ ] `runway-middleware::stack()` wrapping the router
- [ ] Firestore `EventLog::query()` working (remote): events queryable by org+app+type
- [ ] Left column component wired in SvelteKit frontend: user avatar, subscription badge, app switcher

---

## M4 — Tauri offline-first working

Tauri app runs fully with `StorageKit::local()` and syncs when online.

- [ ] `StorageKit::local("~/.{app}")` initialized in Tauri Rust backend
- [ ] `local/sync.rs` sync engine complete:
  - Push: `EventLog::query(unsynced_only: true)` → remote `append()` → `mark_synced()`
  - Pull: remote `DocumentStore::query(updated_after: checkpoint)` → local `put()`
  - Checkpoint stored in local object store at `sync/checkpoint.json`
  - Conflict rule: remote wins on `status` fields, local wins on `body`/`content`
- [ ] Re-embedding on sync: replace zero-padded local fastembed vectors with Vertex AI 768-dim vectors
- [ ] Tauri `onMounted` hook triggers sync; spinner UI while syncing

---

## M5 — All marquee apps online

Folio, Wolfgang, Inkling, Scout, Quorum, Vouch — each fully deployed.

**Per app:**
- [ ] Firebase Hosting web frontend (SvelteKit), deployed via GitHub Actions on push to main
- [ ] Cloud Run Rust backend using all five runway crates, deployed via `just deploy-{app}`
- [ ] Downloadable Tauri binary: macOS aarch64 + x86_64, Windows x64, Linux x64

**Shared release pipeline (`.github/workflows/release.yml`):**
- [ ] Triggered on `v*` tag push
- [ ] Matrix build: `macos-14` (aarch64), `macos-13` (x86_64), `windows-2022`, `ubuntu-22.04`
- [ ] Code signing: Apple notarytool, Windows EV cert (signtool), Linux GPG detached sig
- [ ] ClamAV scan on built binary
- [ ] Upload to `gs://reflective-prod-releases/{app}/{version}/{platform}-{arch}/`
- [ ] Update `gs://reflective-prod-releases/{app}/latest.json`
- [ ] CDN cache invalidation

**Subscription enforcement:**
- [ ] Stripe webhook sets `apps` custom claim on Firebase user (via Admin SDK in shared Cloud Run)
- [ ] `runway-auth::AuthLayer::requiring_app("folio")` returns 403 if not in claim

---

## Current sprint — parallel workstreams (2026-05-11)

Four agents running in parallel, each adding one piece to the stack:

| Workstream | Target | Status |
|------------|--------|--------|
| A — Sync engine | `runway-storage/src/local/sync.rs` | In progress |
| B — Release CI/CD | `.github/workflows/release.yml` | In progress |
| C — Terraform audit | `ops/infra/terraform/` modules + justfile targets | In progress |
| D — Remote EventLog query | `runway-storage/src/remote/event.rs::query()` | In progress |

---

## Boundary debt — relocate after canonical execution container lands

Surfaced 2026-05-28 during the Runway/Helm app-host boundary work. The layer
model says Runway owns ops substrate and Commerce Rails owns commercial
authority — but several crates currently sit on the
wrong side of that line.

- [x] **`runway-accounts/` → `commerce-rails/`** — fixed 2026-05-28.
      Stripe provider config, API calls, webhook signature mechanics, receipt
      construction, and webhook event mapping now live in
      `commerce-rails/crates/commerce-rails-stripe/`. Runway keeps the
      intended HTTPS route and identity/org mirror plumbing, and calls the
      Commerce Rails-owned adapter instead of carrying Stripe business logic inside
      `runway-accounts`.

---

## Architecture decisions (locked)

| Decision | Choice | Rationale |
|----------|--------|-----------|
| GCP all-in | Google Cloud + Firebase | Managed services, no DB ops |
| Local storage | redb (pure Rust, ACID) | No system lib conflicts with burn/rusqlite |
| Remote storage | Firestore + GCS + Vertex AI | Fully managed, no ops |
| Embeddings | Vertex AI text-multilingual-embedding-002 | 768-dim, multilingual (Swedish Folio pilot) |
| Auth | Firebase Auth + custom claims | One identity, many app entitlements |
| Vector dims | 768 everywhere | Index compatibility local↔remote |
| Offline vectors | fastembed 384-dim zero-padded → 768 | Re-embedded to exact 768-dim on sync |
| Multi-tenancy | `orgs/{orgId}/apps/{appId}/...` Firestore path | Enforced by security rules + auth claims |
| Messaging | Pub/Sub only (no NATS) | Same capability, fully managed |
| Consensus/Raft | `lattice` crate, not Runway | Runway wraps services; Lattice holds algorithms |
| Stripe billing | `org_id` = Stripe customer | One org = one subscription, multiple app entitlements |
