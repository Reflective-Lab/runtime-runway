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

# Use sibling ../converge instead of the pinned release tag
use-local-converge:
    mkdir -p .cargo
    cp .cargo/config.toml.example .cargo/config.toml
    @echo "Using local converge checkout from ../converge"
    @echo "Disable with: just use-released-converge"

# Use the pinned release tag from Cargo.toml
use-released-converge:
    rm -f .cargo/config.toml
    @echo "Using pinned converge release {{converge_release}}"

# Show which converge source Cargo will use
converge-source:
    @if [ -f .cargo/config.toml ]; then \
        echo "Local override active: ../converge"; \
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

# ── Infrastructure ─────────────────────────────────────────────────────

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

# ── Git Workflow ───────────────────────────────────────────────────────

# Create a worktree for parallel work (e.g., just worktree fix-auth)
worktree branch:
    git worktree add ../runway-{{branch}} -b {{branch}}
    @echo "Worktree ready at ../runway-{{branch}}"
    @echo "When done: just worktree-rm {{branch}}"

# Remove a worktree
worktree-rm branch:
    git worktree remove ../runway-{{branch}}
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
