---
source: llm
---
# Crate Map

Runway hosts two categories of crates: Converge distribution crates (application and LLM) and the shared infrastructure crates (`runway-*`). The runway-* crates have no Converge dependency — they are standalone infra primitives reused by all Reflective apps.

## Crates

```
converge-application     → converge-core, converge-experience,    CLI/TUI distribution
                           converge-provider + optional subsystems
converge-llm             → converge-core, converge-domain          Local LLM inference (Burn)

runway-storage           → redb, reqwest, fastembed, serde_json    StorageKit: DocumentStore +
                                                                    VectorStore + ObjectStore +
                                                                    EventLog + EmbeddingProvider
runway-auth              → reqwest, axum, tower                    Firebase Auth Tower middleware
runway-middleware        → axum, tower-http                        Request-id, trace, CORS, compression
runway-secrets           → reqwest, secrecy, zeroize               GCP Secret Manager client
runway-telemetry         → opentelemetry, sentry, tracing          OTel → Cloud Trace + Sentry
```

## Dependency direction

```
reflective/runway/crates/application  ──→  converge/crates/{core, experience, provider, ...}
reflective/runway/crates/llm          ──→  converge/crates/{core, domain, provider, storage}
reflective/runway/crates/runway-*     ──→  (no converge dependency)
```

Runway pins Converge dependencies to Git tag `v3.4.0` by default.
For local SDK work, copy `.cargo/config.toml.example` to `.cargo/config.toml` or run `just use-local-converge` to patch to `../reflective/stack/bedrock-platform/converge`.
Runtime packaging expects Converge at `~/dev/reflective/stack/bedrock-platform/converge` unless `CONVERGE_ROOT` overrides it.

## converge-llm engines

| Engine | Framework | GPU Support | Models |
|--------|-----------|-------------|--------|
| `LlamaEngine` | llama-burn | CUDA, Metal, CPU | Llama 3.2, LoRA adapters |
| `GemmaEngine` | llama-cpp-2 | Metal, CPU | Google Gemma (GGUF) |
| `TinyLlamaEngine` | Burn | CPU | Resource-constrained |
| `GrpcBackend` | Tonic | Remote GPU | Offload to GPU server |

## Feature matrix (converge-llm)

| Feature | What |
|---------|------|
| `ndarray` (default) | CPU backend |
| `wgpu` | Metal/Vulkan GPU |
| `gemma` | Gemma GGUF inference |
| `lora` | LoRA adapter fine-tuning |
| `server` | gRPC inference server |
| `grpc-client` | gRPC client |
| `recall` | Experience recall |
| `semantic-embedding` | ONNX embedding models |
| `storage` | Remote adapter registry |
| `anthropic` | Anthropic provider bridge |

See also: [[Stack/Burn and Local LLM]], converge `kb/Architecture/Crate Map`
