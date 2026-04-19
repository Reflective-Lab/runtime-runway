# Reflective Runway

Distribution, deployment, and infrastructure for the Converge stack.

## Structure

```
crates/
  application/   Converge CLI/TUI binary
  llm/           Local LLM inference (Burn, llama.cpp)
docker/          Container definitions
ops/
  deploy/        GPU deployment (RunPod, Cloud Run, Modal)
  scripts/       Dev lifecycle scripts
```

## Building

```bash
cargo build -p converge-application
cargo build -p converge-llm
```

Requires the `converge` repo as a sibling directory.
