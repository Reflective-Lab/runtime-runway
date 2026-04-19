# Reflective Runway

Distribution, deployment, and infrastructure for the [Converge](https://github.com/Reflective-Lab/converge) stack.

Runway owns everything needed to **run, package, and deploy** Converge. The SDK stays pure; Runway handles the messy reality of binaries, containers, GPUs, and cloud services.

## Architecture

```
runway/
  crates/
    application/    The `converge` CLI/TUI binary
    llm/            Local LLM inference (Burn, llama.cpp)
  docker/           Container definitions (Dockerfile, compose)
  ops/
    deploy/         GPU deployment (Cloud Run, RunPod, Modal)
    scripts/        Dev lifecycle scripts
```

### Dependency Direction

Runway **consumes** Converge crates via path — never the reverse.

```
runway/crates/application  ──>  converge/crates/{core, experience, provider, ...}
runway/crates/llm          ──>  converge/crates/{core, domain, provider, storage}
```

Both repos must be checked out as siblings under `~/dev/work/`.

## Crates

### converge-application

The `converge` binary — packages domain packs, providers, and runtime into a deployable CLI/TUI.

| Command | Purpose |
|---------|---------|
| `tui` | Interactive terminal UI (ratatui) |
| `packs` | Domain pack management |
| `run` | Execute jobs from templates |
| `eval` | Reproducible test fixtures |

Optional features: `tui` (default), `knowledge`, `llm`, `analytics`, `optimization`, `full`.

### converge-llm

Local LLM inference for Converge agents using pure Rust frameworks.

| Engine | Model | Framework | GPU |
|--------|-------|-----------|-----|
| `LlamaEngine` | Llama 3.2 | llama-burn | CUDA, Metal, CPU |
| `GemmaEngine` | Google Gemma | llama-cpp-2 | Metal, CPU |
| `TinyLlamaEngine` | TinyLlama | Burn | CPU |
| `GrpcBackend` | Any | Tonic | Remote GPU |

Features: `ndarray` (default), `wgpu`, `gemma`, `lora`, `server`, `grpc-client`, `recall`, `semantic-embedding`, `anthropic`.

## Building

Requires: Rust 1.94+, `just`, and the `converge` repo as a sibling.

```bash
just build              # cargo build --release
just build-quick        # fast iteration (quick-release profile)
just check              # cargo check --workspace
just test               # cargo test --all-targets
just lint               # fmt + clippy
just fix-lint           # auto-fix
```

## Deployment

### Local

```bash
cargo run -p converge-application                      # native
cargo run -p converge-application --features full      # all features
cd docker && docker compose up                          # containerized
```

### Cloud

| Target | Method | Status |
|--------|--------|--------|
| Google Cloud Run (runtime) | `just deploy-cloud-run` | Script-based |
| Google Cloud Run (GPU) | `ops/deploy/gpu/cloudrun/deploy.sh` | Script-based |
| RunPod (GPU) | `ops/deploy/gpu/runpod/` | Dockerfile ready |
| Modal (GPU) | `ops/deploy/gpu/modal/` | Stub |

### GPU Worker

The `converge-llm-server` binary hosts Burn engines behind gRPC. Clients connect via `GrpcBackend`, keeping GPU hardware separate from the convergence engine.

```bash
# Build the server
cargo build -p converge-llm --bin converge-llm-server --features server

# Deploy to Cloud Run with GPU
PROJECT_ID=my-project ./ops/deploy/gpu/cloudrun/deploy.sh
```

## Development Workflow

```bash
just focus          # session opener — repo health
just sync           # PRs, issues, build status
just dev-up         # start local runtime
just smoke-test     # verify health
just dev-down       # stop runtime
```

See the [knowledge base](kb/Home.md) for full documentation.

## Design Principles

- Runway **consumes** Converge crates, never contributes to the SDK
- `unsafe` code is forbidden (`unsafe_code = "forbid"`)
- Infrastructure is imperative scripts today, IaC later
- GPU workers are separated from the main runtime
- Everything proprietary (`LicenseRef-Proprietary`, `publish = false`)
- Edition 2024, Rust 1.94, Clippy pedantic

## Security

See [SECURITY.md](SECURITY.md) for vulnerability reporting and security practices.

## License

Proprietary. Copyright 2024-2026 Reflective Group AB. All rights reserved.

See [LICENSE](LICENSE) for details.
