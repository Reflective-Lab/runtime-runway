---
source: mixed
---
# Deployment

Adapted from converge `kb/Building/Deployment.md` after the 2026-04-19 split.

## Targets

| Target | How | Status |
|--------|-----|--------|
| Local native | `cargo run -p converge-application` | Working |
| Local container | `docker compose up` (from `docker/`) | Working with local Converge checkout |
| Google Cloud Run (runtime) | `ops/scripts/deploy-cloud-run.sh` | Script-based; stages local Converge source |
| Google Cloud Run (GPU) | `ops/deploy/gpu/cloudrun/deploy.sh` | Script-based |
| RunPod (GPU) | `ops/deploy/gpu/runpod/` | Dockerfile ready |
| Modal (GPU) | `ops/deploy/gpu/modal/` | Stub only |

## Local development

### Native

```bash
cargo run -p converge-application
# or with features:
cargo run -p converge-application --features full

# local Converge SDK work:
just use-local-converge
# back to the pinned release tag:
just use-released-converge
```

### Docker

```bash
cd docker/
docker compose up                    # runtime only
docker compose --profile extras up   # + NATS + SurrealDB
```

By default, the Docker build reads runtime source from `~/dev/reflective/stack/bedrock-platform/converge`.
Override with `CONVERGE_ROOT=/abs/path/to/converge` if needed.

Services:
- `converge-runtime` on port 8080 (features: `gcp,auth,firebase`)
- `nats` on port 4222 (extras profile)
- `surrealdb` on port 8000 (extras profile, in-memory)

### Health check

```bash
curl http://localhost:8080/health
curl http://localhost:8080/ready
```

## Cloud Run deployment (runtime)

Script: `ops/scripts/deploy-cloud-run.sh`

Requires: `gcloud` CLI, `PROJECT_ID` or `GOOGLE_CLOUD_PROJECT` env var, and a local `~/dev/reflective/stack/bedrock-platform/converge` checkout (or `CONVERGE_ROOT`).

Flow:
1. Validates GCP project
2. Creates Artifact Registry if needed
3. Stages a temporary Docker build context from the Converge checkout plus Runway's Dockerfile
4. Builds + pushes image tagged by git commit hash
5. Deploys to Cloud Run with GCP/Firebase env vars

## GPU worker deployment

### Cloud Run GPU

Script: `ops/deploy/gpu/cloudrun/deploy.sh`

```bash
PROJECT_ID=my-project ./ops/deploy/gpu/cloudrun/deploy.sh
```

Deploys `converge-llm-server` on Cloud Run with:
- 1x NVIDIA L4 GPU, 8 CPU, 32GB RAM
- gRPC on port 50051
- Env: `MODEL_PATH`, `MODEL_VARIANT` (default: llama3-8b), `MAX_SEQ_LEN` (default: 4096)
- Region: `europe-west1`
- No unauthenticated access

### RunPod

Dockerfile + `start-worker.sh` at `ops/deploy/gpu/runpod/`. Same CUDA 12.4.1 base, same `converge-llm-server` binary.

### Modal

Stub at `ops/deploy/gpu/modal/`. Returns healthcheck only. WIP.

## Known gaps

1. No Terraform — all infra is imperative bash scripts
2. No Kubernetes manifests
3. No Firebase Hosting config (`firebase.json`, rules)
4. No CI/CD pipeline for runway builds
5. GPU worker scaffolding is prepared, not production-complete
6. No service-to-service auth between runtime and GPU workers
7. No model artifact strategy (where weights live, how they're fetched)

## Verified facts

- Runway pins Converge library crates to Git tag `v3.4.0` by default
- Local SDK development can patch those crates back to `../reflective/stack/bedrock-platform/converge`
- Runtime helper scripts resolve runtime source from `~/dev/reflective/stack/bedrock-platform/converge` or `CONVERGE_ROOT`
- Cloud Run GPU deploy script is syntactically correct

## Resume commands

```bash
cargo run -p converge-application
cd docker && docker compose up
ops/scripts/smoke-test.sh
```

See also: [[Building/Docker]], [[Architecture/Application]]
