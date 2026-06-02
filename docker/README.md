## Docker (Runtime Runway)

### Run a local Qwen model (Ollama)

This repo’s `docker/compose.yaml` includes an optional `ollama` service under the `llm` profile.

### macOS (Apple Silicon): prefer native Ollama for Metal performance

Dockerized Ollama is convenient, but on macOS you typically want **native Ollama** for the best Apple Silicon (Metal) performance.

From the repo root:

```bash
just mac-ollama-up "qwen2.5:14b-instruct"
just mac-ollama-smoke "qwen2.5:14b-instruct"
```

If you previously started the docker compose `ollama` service and hit port conflicts, stop it with:

```bash
just mac-ollama-docker-down
```

Start Ollama and pull the model:

```bash
cd docker
OLLAMA_MODEL="qwen2.5:14b-instruct" docker compose --profile llm up -d ollama
```

Watch logs until the pull completes:

```bash
docker compose logs -f ollama
```

Smoke test (generate):

```bash
curl -fsS http://127.0.0.1:11434/api/generate \
  -d '{"model":"qwen2.5:14b-instruct","prompt":"Write a 1-sentence summary of why local models matter.","stream":false}' \
  | python3 -c 'import json,sys; print(json.load(sys.stdin)["response"])'
```

Chat-style smoke test (usually what you want for “instruct” models):

```bash
curl -fsS http://127.0.0.1:11434/api/chat \
  -d '{"model":"qwen2.5:14b-instruct","messages":[{"role":"user","content":"Reply with exactly: ok"}],"stream":false}' \
  | python3 -c 'import json,sys; print(json.load(sys.stdin)["message"]["content"])'
```

GPU (Linux + NVIDIA Container Toolkit):

```bash
cd docker
OLLAMA_MODEL="qwen2.5:14b-instruct" \
  docker compose --profile llm -f compose.yaml -f compose.ollama-gpu.yaml up -d ollama
```

Notes:
- Model files persist in the `ollama` Docker volume.
- Change the exposed port with `OLLAMA_PORT=11434`.
- **Docker Desktop on macOS does not expose Apple Silicon GPU to Linux containers** the way a native `ollama` install does. This compose stack is still useful for a standardized local server, but expect CPU inference unless you run it on Linux with GPU passthrough (or use Ollama outside Docker on macOS).
