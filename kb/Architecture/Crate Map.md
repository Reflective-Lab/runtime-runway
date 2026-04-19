---
source: llm
---
# Crate Map

Runway hosts crates that were split from converge on 2026-04-19. Both are proprietary and unpublished.

## Crates

```
converge-application     → converge-core, converge-experience,    CLI/TUI distribution
                           converge-provider + optional subsystems
converge-llm             → converge-core, converge-domain          Local LLM inference (Burn)
```

## Dependency direction

```
runway/crates/application  ──→  converge/crates/{core, experience, provider, ...}
runway/crates/llm          ──→  converge/crates/{core, domain, provider, storage}
```

Runway pins Converge dependencies to Git tag `v3.4.0` by default.
For local SDK work, copy `.cargo/config.toml.example` to `.cargo/config.toml` or run `just use-local-converge` to patch to sibling `../converge`.
Both repos should still be siblings under `~/dev/work/` for that local mode and for runtime packaging.

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
