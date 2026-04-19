# Reflective Runway

Distribution, deployment, and infrastructure for the Converge stack.

> See `~/CLAUDE.md` and `~/dev/CLAUDE.md` for global conventions.

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

## Rules

- Run `just lint` before considering work done
- Never push to main without confirmation
- Never commit secrets, .env files, or credentials
