---
source: mixed
---
# Burn and Local LLM

Local LLM inference for Converge agents using Burn and llama.cpp. Lives in `crates/llm/`.

## Burn

Pure Rust neural network framework. No Python runtime, no FFI to PyTorch.

### Backends

| Backend | Hardware | Use Case |
|---------|----------|----------|
| `NdArray` | CPU | Fallback, testing, CI |
| `Wgpu` | Metal (macOS), Vulkan | GPU inference on Apple Silicon |
| `CudaJit` | NVIDIA CUDA | Production GPU (currently broken upstream) |

### Why Burn over Ollama

| | Burn | Ollama |
|---|---|---|
| Runtime | In-process | External process |
| Setup | Compile with weights | `ollama pull model` |
| GPU | Direct CUDA/Metal/CPU | Managed by Ollama |
| Determinism | Reproducible | Not guaranteed |
| Best for | Production, deterministic replay | Development, model variety |

## Engines

| Engine | Model | Framework |
|--------|-------|-----------|
| `LlamaEngine` | Llama 3.2 | llama-burn (LoRA adapters, deterministic replay) |
| `GemmaEngine` | Google Gemma | llama-cpp-2 (GGUF format) |
| `TinyLlamaEngine` | TinyLlama | Burn (resource-constrained) |

## GPU server

`converge-llm-server` — gRPC server hosting Burn engines. Clients connect via `GrpcBackend`. Keeps GPU hardware separate from the convergence engine.

Build: `cargo build -p converge-llm --bin converge-llm-server --features server`

Deploy targets: Cloud Run GPU, RunPod, Modal (stub). See [[Building/Deployment]].

## Known issues

- `llama-burn` upstream dropped `cuda`, `vulkan`, `tch-cpu`, `tch-gpu` features
- Feature flags in `crates/llm/Cargo.toml` reference these dead features
- Workspace resolves but full feature builds will fail until cleaned up

See also: [[Architecture/Crate Map]], [[Building/Deployment]]
