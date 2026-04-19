---
source: llm
---
# Changelog

## 2026-04-19 — Converge dependency pinning

- Runway now pins Converge library crates to GitHub tag `v3.4.0` by default instead of always reading sibling path dependencies.
- Local SDK work now uses an untracked Cargo patch override (`.cargo/config.toml`) generated from `.cargo/config.toml.example`.
- Runtime helper scripts now read runtime source from sibling `../converge` or `CONVERGE_ROOT`.
- All tracked cross-repo dependencies now resolve through GitHub pins, while local sibling overrides remain opt-in and untracked.
- Quality stamp: `just lint`, `just check`, and `just test` all passed on 2026-04-19 before tagging `v3.4.0`.

## 2026-04-19 — Initial split from converge

Runway created by extracting distribution and infrastructure from the converge repo:

- `crates/application` — the `converge` CLI/TUI binary (was `converge/crates/application`)
- `crates/llm` — local LLM inference (was `converge/dev/llm`)
- `docker/` — container definitions (was `converge/dev/docker`)
- `ops/` — deployment scripts and GPU infra (was `converge/ops`)

Converge stays as a pure SDK/runtime library. Runway owns everything needed to run, package, and deploy.

Runway initially depended on converge crates via sibling path (`../converge/crates/...`).
