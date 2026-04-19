---
tags: [moc]
source: llm
---
# Index

Entity catalog for the Runway knowledge base. Update when projects, crates, services, or domain concepts change.

## Crates

| Entity | Type | Location |
|--------|------|----------|
| converge-application | binary | `crates/application/` |
| converge-llm | library + binary | `crates/llm/` |

## Infrastructure

| Entity | Type | Location |
|--------|------|----------|
| Docker compose | container stack | `docker/` |
| Cloud Run deploy | script | `ops/scripts/deploy-cloud-run.sh` |
| Cloud Run GPU | script + Dockerfile | `ops/deploy/gpu/cloudrun/` |
| RunPod GPU | Dockerfile | `ops/deploy/gpu/runpod/` |
| Modal GPU | Python stub | `ops/deploy/gpu/modal/` |
| Dev scripts | lifecycle | `ops/scripts/` |
| Workflow scripts | session helpers | `ops/scripts/workflow/` |

## LLM Engines

| Entity | Framework | Models |
|--------|-----------|--------|
| LlamaEngine | llama-burn | Llama 3.2, LoRA |
| GemmaEngine | llama-cpp-2 | Google Gemma (GGUF) |
| TinyLlamaEngine | Burn | TinyLlama |
| GrpcBackend | Tonic | Remote GPU offload |

## KB Sections

| Section | Purpose |
|---------|---------|
| [[Architecture/Application]] | CLI/TUI binary |
| [[Architecture/Crate Map]] | Crate layout and deps |
| [[Building/Deployment]] | Deploy guide |
| [[Building/Docker]] | Container definitions |
| [[Stack/Burn and Local LLM]] | Inference engines |
| [[Workflow/Daily Journey]] | Workflow cheat sheet |
| [[History/CHANGELOG]] | Release notes |
