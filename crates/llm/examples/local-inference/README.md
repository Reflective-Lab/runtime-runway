# Local Inference Example

Run LLM inference locally on Apple Silicon (M1/M2/M3/M4).

The default path is now embedded Gemma GGUF inference via `llama.cpp`.

The Gemma examples expect a local GGUF file either in `~/models/` or at the
path pointed to by `CONVERGE_GEMMA_MODEL_PATH`.

## Quick Start

```bash
# Embedded Gemma on macOS / Apple Silicon
CONVERGE_GEMMA_MODEL_PATH=~/models/gemma-7b-it-Q4_K_M.gguf \
cargo run -p example-local-inference --features "gemma" --release

# Burn / Llama 3 fallback on Metal GPU
cargo run -p example-local-inference --features "wgpu,llama3,pretrained" --release

# Quick test with tiny model
cargo run -p example-local-inference --features "wgpu,tiny,pretrained" --release
```

## Expected Performance (M4 Mac)

| Model | Tokens/sec |
|-------|-----------|
| Gemma 7B Q4 (GGUF, Metal offload) | ~20-50 |
| Tiny (1.1B) | ~100+ |
| Llama 3.2 3B (quantized) | ~20-40 |
| Llama 3 8B (quantized) | ~10-20 |

The Gemma path uses a local GGUF file and does not start a separate Ollama or vLLM process.
