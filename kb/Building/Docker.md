---
source: mixed
---
# Docker

Container definitions for `api-server`, the retired `converge-runtime`
compatibility shell, and supporting services.

## Files

| File | Purpose |
|------|---------|
| `docker/Dockerfile` | Multi-stage build for legacy `converge-runtime` compatibility shell |
| `docker/Dockerfile.api-server` | Multi-stage build for `api-server` (runway-* reference service) |
| `docker/compose.yaml` | Local dev stack |
| `cloudbuild.api-server.yaml` | Cloud Build config for api-server |

---

## Dockerfile.api-server

Self-contained build — no external source required (unlike `converge-runtime`).

```
Builder:  rust:1.94-bookworm
          cargo build -p api-server --release
          (only api-server and its 5 runway-* deps are compiled)

Runtime:  debian:bookworm-slim + ca-certificates + curl
          /usr/local/bin/api-server
          PORT=8080, LOCAL_DEV=true (override in Cloud Run)
```

Build via Cloud Build (recommended):
```bash
just api-deploy          # build + push + deploy Cloud Run
just api-docker-build    # local Docker build only
just api-docker-run      # run the Docker image locally
```

### .gcloudignore

`Cargo.lock` is in `.gitignore` (library convention) but Cloud Build needs it for reproducible builds. `.gcloudignore` negates that exclusion:

```
#!include:.gitignore
!Cargo.lock
```

---

## Dockerfile (legacy converge-runtime)

This path is retained for compatibility checks only. It is not the current
Reflective stack runtime. Current app services should use `api-server`,
`runway-app-host`, or an app-specific backend.

Two-stage build:
1. **Builder** — `rust:1.94-bookworm`, compiles `converge-runtime --features gcp,auth,firebase`
2. **Runtime** — `debian:bookworm-slim`, exposes port 8080

Build context must contain the Converge repo root. `docker/compose.yaml` pulls
it from `../../bedrock-platform/converge`; `ops/scripts/dev-up.sh` resolves it
to an absolute path.

---

## compose.yaml services

| Service | Image | Port | Profile |
|---------|-------|------|---------|
| `converge-runtime` | Built from `docker/Dockerfile` | 8080 | default |
| `nats` | `nats:2.10-alpine` | 4222 | extras |
| `surrealdb` | `surrealdb/surrealdb:latest` | 8000 | extras |
| `ollama` | `ollama/ollama:latest` | 11434 | llm |

```bash
just docker-up                # converge-runtime only
just docker-up-extras         # + nats + surrealdb
# LLM: just mac-ollama-up     # native Ollama on Apple Silicon (avoids Docker for Metal)
```

SurrealDB runs in-memory (`root:root`). NATS has no auth in dev.

---

## Cloud Build (api-server)

`cloudbuild.api-server.yaml` builds the api-server image on an E2_HIGHCPU_8 machine with a 30-minute timeout. Substitution `_IMAGE_URI` receives the full Artifact Registry tag from the deploy script.

First cold build takes ~5 minutes (all deps from scratch). Subsequent builds are faster because Cloud Build caches layers.

See also: [[Building/Deployment]]
