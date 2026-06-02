# Reflective Runtime Runway

Distribution, deployment, and infrastructure for the Converge stack.

> See `~/CLAUDE.md` and `~/dev/CLAUDE.md` for global conventions.
> Read and follow `AGENTS.md` for the full agent contract.

## What belongs here

- `crates/application` — the `converge` binary (CLI/TUI distribution)
- `crates/llm` — local LLM inference (Burn, llama.cpp)
- `crates/runway-auth` — Firebase Auth middleware
- `crates/runway-middleware` — Axum service middleware
- `crates/runway-secrets` — Secret Manager integration
- `crates/runway-storage` — local/remote storage kit
- `crates/runway-telemetry` — tracing, logging, and error reporting
- `crates/api-server` — reference Cloud Run API server
- `docker/` — Docker compose, Dockerfiles
- `ops/` — deployment scripts, GPU deploy (RunPod, Cloud Run, Modal)

## What does NOT belong here

- Converge SDK crates — those live in `~/dev/reflective/bedrock-platform/converge/`
- Product code (Helms, Wolfgang) — separate repos
- Organism runtime — separate repo

## Dependencies

Runtime Runway pins Converge crates to a release tag by default.
For local SDK work, use `just use-local-converge` to patch to `../reflective/bedrock-platform/converge`.

## Session scope

- **Milestones:** `MILESTONES.md` (when it exists)
- **Changelog:** `kb/History/CHANGELOG.md`
- **Strategic context:** `~/dev/reflective/bedrock-platform/EPIC.md`

## Rules

- Run `just lint` before considering work done
- Never push to main without confirmation
- Never commit secrets, .env files, or credentials
- Skills are available in `.claude/skills/` — use `/help` to see them
- KB lives in `kb/` — update it when architecture or process changes
- Root directory stays clean: docs at root, knowledge in `kb/`, source in `crates/`
