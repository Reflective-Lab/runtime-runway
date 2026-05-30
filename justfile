# Reflective Runway — Development Commands
# Install: brew install just  |  cargo install just
# Usage:   just --list

set dotenv-load := true
converge_release := "v3.4.0"

# ── Build ──────────────────────────────────────────────────────────────

# Build workspace (release)
build:
    cargo build --release

# Build workspace (fast iteration)
build-quick:
    cargo build --profile quick-release

# Check workspace without producing release artifacts
check:
    cargo check --workspace

# ── Test ───────────────────────────────────────────────────────────────

# Run tests (default members)
test:
    cargo test --all-targets

# Run tests for a specific crate
test-crate crate:
    cargo test -p {{crate}} --all-targets

# Run a single test by name
test-one name:
    cargo test --all-targets -- {{name}}

# Run benchmarks (compile only)
bench:
    cargo bench --workspace --no-run

# Run benchmarks (with execution)
bench-run:
    cargo bench --workspace

# ── Lint & Format ─────────────────────────────────────────────────────

# Check formatting and clippy
lint:
    cargo fmt --check
    cargo clippy --all-targets -- -D warnings

# Auto-fix lint issues
fix-lint:
    cargo clippy --fix --allow-staged --allow-dirty --allow-no-vcs
    cargo fmt

# Format only
fmt:
    cargo fmt

# ── Converge Source ────────────────────────────────────────────────────

# Use the local Converge checkout instead of the pinned release tag
use-local-converge:
    mkdir -p .cargo
    cp .cargo/config.toml.example .cargo/config.toml
    @echo "Using local converge checkout from ../reflective-stack/bedrock-platform/converge"
    @echo "Disable with: just use-released-converge"

# Use the pinned release tag from Cargo.toml
use-released-converge:
    rm -f .cargo/config.toml
    @echo "Using pinned converge release {{converge_release}}"

# Show which converge source Cargo will use
converge-source:
    @if [ -f .cargo/config.toml ]; then \
        echo "Local override active: ../reflective-stack/bedrock-platform/converge"; \
    else \
        echo "Pinned release active: {{converge_release}}"; \
    fi

# ── Docs ───────────────────────────────────────────────────────────────

# Generate workspace docs
doc:
    cargo doc --no-deps --workspace

# Open docs in browser
doc-open:
    cargo doc --no-deps --workspace --open

# ── Cloud Infrastructure (Terraform + Firebase) ────────────────────────

# One-time: create TF state bucket + Terraform SA (needs Owner/Editor + IAM Admin)
infra-bootstrap:
    PROJECT_ID="${PROJECT_ID}" bash ops/infra/scripts/bootstrap.sh

# Initialize Terraform (run after bootstrap, once per workstation)
infra-init:
    cd ops/infra/terraform && terraform init

# Preview infrastructure changes
infra-plan env="dev":
    cd ops/infra/terraform && terraform plan -var-file=terraform.tfvars -var="env={{env}}"

# Apply infrastructure changes (prompts for confirmation)
infra-apply env="dev":
    cd ops/infra/terraform && terraform apply -var-file=terraform.tfvars -var="env={{env}}"

# Destroy infrastructure (dev/staging only — prod blocked by delete_protection)
infra-destroy env="dev":
    cd ops/infra/terraform && terraform destroy -var-file=terraform.tfvars -var="env={{env}}"

# Show Terraform outputs (hosts, bucket names, etc.)
infra-output:
    cd ops/infra/terraform && terraform output

# Deploy Firebase Auth config + Firestore/Storage security rules
firebase-provision-auth:
    PROJECT_ID="${PROJECT_ID}" bash ops/infra/scripts/provision-auth.sh

# Deploy Firestore rules only
firebase-rules:
    firebase deploy --only firestore:rules,firestore:indexes \
        --project="${PROJECT_ID}" \
        --config ops/infra/firebase/firebase.json

# Deploy Storage rules only
firebase-storage-rules:
    firebase deploy --only storage \
        --project="${PROJECT_ID}" \
        --config ops/infra/firebase/firebase.json

# Publish platform binaries for a marquee app to the releases CDN
# Usage: just publish-release folio v1.2.0
publish-release app version:
    APP={{app}} VERSION={{version}} PROJECT_ID="${PROJECT_ID}" bash ops/infra/scripts/publish-release.sh

# ── api-server ─────────────────────────────────────────────────────────

# Run api-server locally (redb storage, no Firebase auth needed)
api-up:
    LOCAL_DEV=true STORAGE_PATH=/tmp/api-server cargo run -p api-server

# Build api-server Docker image
api-docker-build:
    docker build -f docker/Dockerfile.api-server -t api-server:dev .

# Run api-server Docker image locally
api-docker-run:
    docker run --rm -p 8080:8080 -e LOCAL_DEV=true -e STORAGE_PATH=/tmp/api-server api-server:dev

# Deploy api-server to Cloud Run (tags revision with version + SHA)
api-deploy:
    SERVICE_NAME=api-server IMAGE_NAME=api-server bash ops/scripts/deploy-api-server.sh

# Freeze the current api-server as a named major-version service.
# Adds apps.reflective.se/api-server/v{N}/** routing alongside the rolling latest.
# Usage: just api-freeze 3
api-freeze major:
    #!/usr/bin/env bash
    set -euo pipefail
    SERVICE_NAME=api-server-v{{major}} \
    ROUTE_PREFIX=/api-server/v{{major}} \
    bash ops/scripts/deploy-api-server.sh
    echo ""
    echo "Add this block to ops/infra/firebase/apps/firebase.json rewrites (BEFORE the /api-server/** catch-all):"
    echo '  { "source": "/api-server/v{{major}}/**", "run": { "serviceId": "api-server-v{{major}}", "region": "europe-west1", "projectId": "wolfgang-kb-prod" } }'
    echo "Then: just apps-deploy"

# Deploy Firebase Hosting for apps.reflective.se
apps-deploy:
    cd ops/infra/firebase/apps && firebase deploy --only hosting:apps-reflective-se

# ── Runtime Infrastructure ─────────────────────────────────────────────

# Start local runtime
dev-up mode="auto":
    bash ops/scripts/dev-up.sh {{mode}}

# Stop local runtime
dev-down mode="auto":
    bash ops/scripts/dev-down.sh {{mode}}

# Smoke-test local runtime
smoke-test url="http://127.0.0.1:8080":
    bash ops/scripts/smoke-test.sh {{url}}

# Deploy runtime to Google Cloud Run
deploy-cloud-run:
    bash ops/scripts/deploy-cloud-run.sh

# Deploy GPU worker to Cloud Run
deploy-gpu-cloudrun:
    bash ops/deploy/gpu/cloudrun/deploy.sh

# ── Docker ─────────────────────────────────────────────────────────────

# Start docker compose stack
docker-up:
    cd docker && docker compose up -d

# Start docker compose with extras (NATS, SurrealDB)
docker-up-extras:
    cd docker && docker compose --profile extras up -d

# Stop docker compose stack
docker-down:
    cd docker && docker compose down

# ── Local LLM (macOS / Apple Silicon) ──────────────────────────────────

# Native Ollama (Metal-friendly). Stops docker compose `ollama` by default to avoid port 11434 conflicts.
mac-ollama-up model="qwen2.5:14b-instruct":
    bash ops/scripts/ollama/mac-ollama-up.sh "{{model}}"

# Quick chat smoke test against native Ollama
mac-ollama-smoke model="qwen2.5:14b-instruct":
    bash ops/scripts/ollama/mac-ollama-smoke.sh "{{model}}"

# Stop dockerized Ollama (from docker/compose.yaml profile `llm`)
mac-ollama-docker-down:
    cd docker && docker compose --profile llm stop ollama

# ── Git Workflow ───────────────────────────────────────────────────────

# Create a worktree for parallel work (e.g., just worktree fix-auth)
worktree branch:
    git worktree add ../reflective-runway-{{branch}} -b {{branch}}
    @echo "Worktree ready at ../reflective-runway-{{branch}}"
    @echo "When done: just worktree-rm {{branch}}"

# Remove a worktree
worktree-rm branch:
    git worktree remove ../reflective-runway-{{branch}}
    @echo "Worktree removed. Branch '{{branch}}' still exists — delete with: git branch -d {{branch}}"

# List active worktrees
worktrees:
    git worktree list

# ── jj (Jujutsu) Workflow ─────────────────────────────────────────────

# Show jj status
jj-status:
    jj status

# Create a new change
jj-new desc:
    jj new -m "{{desc}}"

# Show the change log
jj-log:
    jj log --limit 20

# Squash current change into parent
jj-squash:
    jj squash

# Push to git remote
jj-push:
    jj git push

# ── Clean ──────────────────────────────────────────────────────────────

# Remove build artifacts
clean:
    cargo clean

# ── Workflow ──────────────────────────────────────────────────────────

# Session opener — repo health + recent activity
focus:
    @bash ops/scripts/workflow/focus.sh

# Team sync — PRs, issues, recent commits
sync:
    @bash ops/scripts/workflow/sync.sh

# Build health
status:
    @bash ops/scripts/workflow/status.sh

# ── Info ───────────────────────────────────────────────────────────────

# Show crate dependency graph
deps:
    @echo "Dependency graph:"
    @echo "  converge-application  →  converge/{core, experience, provider, domain, knowledge, ...}"
    @echo "  converge-llm          →  converge/{core, domain, provider, storage}"
    @echo ""
    @echo "Pinned release: {{converge_release}} from Reflective-Lab/converge"
    @echo "Local SDK override: just use-local-converge"

# ── Terraform (environment-scoped) ─────────────────────────────────────

# Initialize Terraform working directory (once per workstation after bootstrap)
tf-init:
    cd ops/infra/terraform && terraform init

# Preview changes for an environment (default: staging)
tf-plan env="staging":
    cd ops/infra/terraform && terraform plan -var-file=environments/{{env}}.tfvars

# Apply changes for an environment (default: staging)
tf-apply env="staging":
    cd ops/infra/terraform && terraform apply -var-file=environments/{{env}}.tfvars

# Destroy infrastructure for an environment — dev/staging only (prod has delete_protection)
tf-destroy env="staging":
    @echo "Destroying {{env}} — are you sure? Press Enter to continue."
    @read
    cd ops/infra/terraform && terraform destroy -var-file=environments/{{env}}.tfvars

# Deploy Firebase Firestore rules, indexes, and Storage rules
firebase-deploy:
	cd ops/infra/firebase && firebase deploy --only firestore:rules,firestore:indexes,storage

# One-time GCP project bootstrap (billing, APIs, TF state bucket, Firebase)
gcp-setup:
	ops/scripts/gcp-setup.sh

# ── Contract Tests ─────────────────────────────────────────────────────

# Run local + emulator contract suites (default for `just contract`)
contract: contract-local contract-emulator

# Run contract suite against the local (redb + FS + fastembed) backend
contract-local:
	cargo test -p runway-storage --test contract_local -- --nocapture

# Run contract suite against the Firestore/GCS/Pub-Sub emulators with fastembed
contract-emulator:
	docker compose -f crates/runway-storage/tests/docker-compose.contract.yml up -d --wait
	-FIRESTORE_EMULATOR_HOST=localhost:8080 \
	 PUBSUB_EMULATOR_HOST=localhost:8085 \
	 STORAGE_EMULATOR_HOST=http://localhost:4443 \
	   cargo test -p runway-storage --test contract_emulator -- --ignored --nocapture
	docker compose -f crates/runway-storage/tests/docker-compose.contract.yml down

# Run contract suite against real staging GCP.
# Requires: RUNWAY_CONTRACT_PROJECT, RUNWAY_CONTRACT_REGION, RUNWAY_CONTRACT_BUCKET, RUNWAY_CONTRACT_TOKEN.
# RUNWAY_CONTRACT_TOKEN locally: `export RUNWAY_CONTRACT_TOKEN=$(gcloud auth print-access-token)`.
contract-staging:
	@test -n "$RUNWAY_CONTRACT_PROJECT" || (echo "set RUNWAY_CONTRACT_PROJECT" && exit 1)
	@test -n "$RUNWAY_CONTRACT_REGION" || (echo "set RUNWAY_CONTRACT_REGION (e.g. us-central1)" && exit 1)
	@test -n "$RUNWAY_CONTRACT_BUCKET" || (echo "set RUNWAY_CONTRACT_BUCKET" && exit 1)
	@test -n "$RUNWAY_CONTRACT_TOKEN" || (echo "set RUNWAY_CONTRACT_TOKEN (try: gcloud auth print-access-token)" && exit 1)
	cargo test -p runway-storage --test contract_real_gcp -- --ignored --nocapture

# Run all three (local + emulator + real GCP)
contract-all: contract contract-staging
