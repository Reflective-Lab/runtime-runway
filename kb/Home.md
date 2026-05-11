---
source: llm
---
# Reflective Runway

Distribution, deployment, and infrastructure for the Converge stack. Separated from converge on 2026-04-19 to keep the SDK pure.

## What lives here

| Area | Purpose | Directory |
|------|---------|-----------|
| [[Architecture/Application]] | Converge CLI/TUI binary | `crates/application/` |
| [[Stack/Burn and Local LLM]] | Local inference (Burn, llama.cpp) | `crates/llm/` |
| [[Building/Docker]] | Container definitions | `docker/` |
| [[Building/Deployment]] | Deploy scripts, GPU infra | `ops/` |

## Principles

- Runway **consumes** converge crates, never contributes to the SDK
- Infrastructure is imperative scripts today, IaC later
- GPU workers are separated from the main runtime
- Everything proprietary (`LicenseRef-Proprietary`, `publish = false`)

## Known gaps

- No Terraform / IaC — cloud infra is bash + `gcloud`
- No Kubernetes manifests
- No Firebase config files (just env vars)
- No CI/CD (GitHub Actions live in converge)
- No monitoring/alerting config

## See also

- [[Building/Deployment]] — full deployment guide
- [[Architecture/Crate Map]] — what crates live here and their deps
- Converge SDK: `~/dev/reflective/stack/bedrock-platform/converge/kb/`
