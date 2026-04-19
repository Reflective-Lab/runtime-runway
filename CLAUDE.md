# Reflective Runway

Distribution, deployment, and infrastructure for the Converge stack.

> See `~/CLAUDE.md` and `~/dev/CLAUDE.md` for global conventions.
> Read and follow `AGENTS.md` for the full agent contract.

## What belongs here

- `crates/application` — the `converge` binary (CLI/TUI distribution)
- `crates/llm` — local LLM inference (Burn, llama.cpp)
- `docker/` — Docker compose, Dockerfiles
- `ops/` — deployment scripts, GPU deploy (RunPod, Cloud Run, Modal)

## What does NOT belong here

- Converge SDK crates — those live in `../converge/`
- Product code (Helms, Wolfgang) — separate repos
- Organism runtime — separate repo

## Dependencies

Runway depends on Converge crates via path (`../converge/crates/...`).
Both repos must be checked out as siblings under `~/dev/work/`.

## Session scope

- **Milestones:** `MILESTONES.md` (when it exists)
- **Changelog:** `kb/History/CHANGELOG.md`
- **Strategic context:** `~/dev/work/EPIC.md`

## Rules

- Run `just lint` before considering work done
- Never push to main without confirmation
- Never commit secrets, .env files, or credentials
- Skills are available in `.claude/skills/` — use `/help` to see them
- KB lives in `kb/` — update it when architecture or process changes
- Root directory stays clean: docs at root, knowledge in `kb/`, source in `crates/`
