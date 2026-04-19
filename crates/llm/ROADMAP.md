# converge-llm Roadmap

## Gemma 4 Support via vLLM/Ollama Backend

Gemma 4 (dense, 31B) is a strong candidate for local inference. Dense architecture means no MoE routing complexity — simpler serving, simpler debugging, predictable latency.

### Why Gemma 4

- Dense 31B outperforms MoE models at comparable active parameter counts
- No expert-parallel communication, no routing kernels, no token-dropping
- Trivial to serve compared to MoE architectures
- Strong reasoning and instruction-following benchmarks

### Integration Path

The `LlmBackend` trait already supports local and remote backends. Add a new feature-gated backend that talks to a local vLLM or Ollama instance over HTTP.

**Proposed feature flags:**
- `ollama` — Ollama backend (`localhost:11434`), simplest path, runs any GGUF model
- `vllm` — vLLM backend, better for production GPU serving (batching, PagedAttention, tensor parallelism)

**Architecture:**
```
converge-llm
  ├── src/ollama.rs      # OllamaBackend: impl LlmBackend
  ├── src/vllm.rs        # VllmBackend: impl LlmBackend
  └── Cargo.toml         # ollama = ["dep:reqwest"], vllm = ["dep:reqwest"]
```

Both backends reuse `reqwest` (already an optional dep) and implement `LlmBackend`. No new dependencies required beyond what's already in the workspace.

**Ollama API surface:**
- `POST /api/generate` — completion
- `POST /api/chat` — chat completion (maps directly to `BackendRequest`)
- `GET /api/tags` — list available models

**vLLM API surface:**
- OpenAI-compatible `/v1/chat/completions` endpoint
- Supports streaming, tool use, structured output

### Models to Target

| Model | Params | Type | Backend |
|-------|--------|------|---------|
| Gemma 4 31B | 31B | Dense | Ollama, vLLM |
| Gemma 4 12B | 12B | Dense | Ollama, vLLM |
| Llama 3 8B | 8B | Dense | llama-burn (existing), Ollama |
| Qwen 3 | Various | MoE/Dense | Ollama, vLLM |

### Not in Scope

- Implementing Gemma architecture natively in Burn (use upstream `gemma-burn` if/when available)
- MoE-specific routing or expert-parallel inference
- Training or fine-tuning through these backends (inference only)
