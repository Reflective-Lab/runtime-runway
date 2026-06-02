---
source: llm
---
# Reflective Runtime Runway

Distribution, deployment, and infrastructure for apps that embed the Converge
stack. Separated from converge on 2026-04-19 to keep the SDK pure.

## What lives here

| Area | Purpose | Directory |
|------|---------|-----------|
| [[Architecture/Application]] | Converge CLI/TUI binary | `crates/application/` |
| [[Architecture/App Execution Container]] | Standard app backend host | `crates/runway-app-host/` |
| [[Stack/Burn and Local LLM]] | Local inference (Burn, llama.cpp) | `crates/llm/` |
| [[Building/Docker]] | Container definitions | `docker/` |
| [[Building/Deployment]] | Deploy scripts, GPU infra | `ops/` |

## Principles

- Runtime Runway **consumes** converge crates, never contributes to the SDK
- The standalone `converge-runtime` service is retired as the canonical runtime;
  legacy scripts are compatibility checks only
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
- [[Architecture/App Execution Container]] — standard Runtime Runway host for Helm and marquee apps
- Converge SDK: `~/dev/reflective/bedrock-platform/converge/kb/`
