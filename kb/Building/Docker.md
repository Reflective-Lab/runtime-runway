---
source: llm
---
# Docker

Container definitions for the Converge runtime and supporting services.

## Files

| File | Purpose |
|------|---------|
| `docker/Dockerfile` | Multi-stage build for `converge-runtime` |
| `docker/compose.yaml` | Local dev stack |

## Dockerfile

Two-stage build:
1. **Builder**: `rust:1.94-bookworm`, compiles `converge-runtime` with configurable features (default: `gcp,auth,firebase`)
2. **Runtime**: `debian:bookworm-slim` + `ca-certificates` + `curl`, exposes port 8080

Build arg: `CONVERGE_RUNTIME_FEATURES` controls which features are compiled in.

The Dockerfile expects the Converge repo root as its build context.
`docker/compose.yaml` points there by default with `CONVERGE_ROOT=../../converge`, and `ops/scripts/dev-up.sh` sets absolute paths automatically.

## Compose services

| Service | Image | Port | Profile |
|---------|-------|------|---------|
| `converge-runtime` | Built from Dockerfile | 8080 | default |
| `nats` | `nats:2.10-alpine` | 4222 | extras |
| `surrealdb` | `surrealdb/surrealdb:latest` | 8000 | extras |

SurrealDB runs in-memory mode with `root:root` credentials (dev only).

## Known issues

- Runtime image builds still require a sibling `../converge` checkout or an explicit `CONVERGE_ROOT`
- No GPU-enabled Dockerfile for local dev (GPU Dockerfiles live in `ops/deploy/gpu/`)

See also: [[Building/Deployment]]
