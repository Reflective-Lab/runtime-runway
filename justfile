# Reflective Runway — Development Commands
# Install: brew install just  |  cargo install just
# Usage:   just --list

set dotenv-load := true

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
    @echo "Both depend on converge crates via path (../converge/crates/...)"
