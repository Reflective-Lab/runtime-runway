---
source: mixed
---
# Deployment

## Targets

| Target | Command | Status |
|--------|---------|--------|
| Local native (api-server) | `just api-up` | Working |
| Local native (converge-application) | `cargo run -p converge-application` | Working |
| Local container | `just docker-up` | Legacy converge-runtime compatibility shell |
| Cloud Run (api-server) | `just api-deploy` | Live — `wolfgang-kb-prod`, `europe-west1` |
| Cloud Run (converge-runtime) | `ALLOW_LEGACY_CONVERGE_RUNTIME_DEPLOY=true ops/scripts/deploy-cloud-run.sh` | Retired compatibility only |
| Cloud Run GPU | `ops/deploy/gpu/cloudrun/deploy.sh` | Script-based |
| Firebase Hosting (apps.reflective.se) | `just apps-deploy` | Live |

---

## api-server (the reference Cloud Run service)

`crates/api-server` is the canonical deployment spike — a minimal Cloud Run binary that wires all five `runway-*` crates and proves the full GCP path end-to-end.

### Local development

```bash
just api-up
# Runs with LOCAL_DEV=true, redb storage at /tmp/api-server, no Firebase needed.

# Test with dev bypass token:
curl http://localhost:8080/health
curl -H "Authorization: Bearer dev" http://localhost:8080/api/me
curl -H "Authorization: Bearer dev" -X POST http://localhost:8080/api/events \
  -H "Content-Type: application/json" \
  -d '{"app_id":"test","event_type":"ping","payload":{"hello":"world"}}'
```

`Authorization: Bearer dev` is accepted in `LOCAL_DEV=true` mode and injects a canned `AuthContext` (uid: `dev-uid`, org: `dev-org`). All other tokens hit Firebase.

### Deploy to Cloud Run

```bash
just api-deploy
# Builds via Cloud Build → pushes to Artifact Registry → deploys Cloud Run → tags revision
```

GCP context:
- **Project:** `wolfgang-kb-prod`
- **Region:** `europe-west1`
- **Artifact Registry:** `europe-west1-docker.pkg.dev/wolfgang-kb-prod/wolfgang/api-server`
- **Cloud Run service:** `api-server`
- **Service account:** `run-api-server@wolfgang-kb-prod.iam.gserviceaccount.com`
- **GCS bucket:** `wolfgang-kb-prod-runway-api` (runway-storage object store)
- **Firestore:** shared with `wolfgang-kb-prod` project
- **Cloud Build config:** `cloudbuild.api-server.yaml`

Every deploy:
1. Builds image tagged `{sha}` via Cloud Build (E2_HIGHCPU_8)
2. Deploys to Cloud Run, sets `ROUTE_PREFIX=/api-server`
3. Tags the revision with `v{major}-{minor}-{patch}` and `sha-{git-sha}`

---

## URL model — apps.reflective.se

All Reflective apps share a single Firebase Hosting entry point. The path prefix is the app name.

```
apps.reflective.se/{app-name}/**     →  Cloud Run service  `{app-name}`
apps.reflective.se/{app-name}/v3/**  →  Cloud Run service  `{app-name}-v3`  (frozen major)
```

The `apps.reflective.se` domain maps to the Firebase Hosting site `apps-reflective-se` in project `wolfgang-kb-prod`. Adding an app means:
1. Add a `run` rewrite block in `ops/infra/firebase/apps/firebase.json`
2. Run `just apps-deploy`

### Versioned URLs

Every deploy tags the Cloud Run revision, giving stable direct URLs that survive future deploys:

| URL | Meaning |
|-----|---------|
| `apps-reflective-se.web.app/api-server/api/me` | Rolling latest |
| `https://v3-4-1---api-server-{hash}-ew.a.run.app/...` | Pinned to version 3.4.1 |
| `https://sha-{sha}---api-server-{hash}-ew.a.run.app/...` | Pinned to exact commit |

### Freezing a major version

When v4 ships and frontends need more migration time on v3:

```bash
just api-freeze 3
# Deploys api-server-v3 Cloud Run service with ROUTE_PREFIX=/api-server/v3.
# Prints the firebase.json block to add for apps.reflective.se/api-server/v3/**.
# Then: just apps-deploy
```

### Adding the custom domain

`apps.reflective.se` is not yet pointing at Firebase Hosting. To activate:
1. Firebase Console → Hosting → `apps-reflective-se` → Add custom domain → `apps.reflective.se`
2. Add the CNAME `apps.reflective.se → apps-reflective-se.web.app` in DNS
3. Firebase issues a managed TLS certificate automatically

---

## Environment variables (Cloud Run)

| Variable | Value | Purpose |
|----------|-------|---------|
| `LOCAL_DEV` | `false` | Switches StorageKit to Firestore/GCS/Vertex AI |
| `GOOGLE_CLOUD_PROJECT` | `wolfgang-kb-prod` | GCP project for Firestore, GCS |
| `FIREBASE_PROJECT_ID` | `wolfgang-kb-prod` | Firebase Auth project |
| `FIREBASE_API_KEY` | (from Wolfgang .env.production) | Token verification |
| `GCS_BUCKET` | `wolfgang-kb-prod-runway-api` | Object store bucket |
| `ROUTE_PREFIX` | `/api-server` | Mount path matching Firebase Hosting rewrite |
| `OTLP_ENDPOINT` | (optional) | Cloud Trace OTLP endpoint |
| `SENTRY_DSN` | (optional) | Sentry error tracking |

---

## Overriding defaults

```bash
# Deploy to a different project:
PROJECT_ID=my-project just api-deploy

# Deploy a frozen v3 service:
SERVICE_NAME=api-server-v3 ROUTE_PREFIX=/api-server/v3 just api-deploy

# Use a different Artifact Registry repo:
REPOSITORY=my-repo just api-deploy
```

---

## converge-application (local + container)

```bash
cargo run -p converge-application
# or: just docker-up
```

See [[Building/Docker]] for Docker compose details. The `converge-runtime`
container path is legacy compatibility only. Current hosted app services should
deploy through `api-server`, `runway-app-host`, or an app-specific backend.

The historical `ops/scripts/deploy-cloud-run.sh` path refuses to run unless
`ALLOW_LEGACY_CONVERGE_RUNTIME_DEPLOY=true` is set.

---

## GPU workers

| Target | Script | Notes |
|--------|--------|-------|
| Cloud Run GPU | `ops/deploy/gpu/cloudrun/deploy.sh` | NVIDIA L4, gRPC 50051 |
| RunPod | `ops/deploy/gpu/runpod/` | CUDA 12.4.1 base |
| Modal | `ops/deploy/gpu/modal/` | Stub only |

See also: [[Building/Docker]], [[Architecture/Crate Map]]
