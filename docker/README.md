## Docker (Runway)

### Run a local Qwen model (Ollama)

This repo’s `docker/compose.yaml` includes an optional `ollama` service under the `llm` profile.

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
curl -sS http://127.0.0.1:11434/api/generate \
  -d '{"model":"qwen2.5:14b-instruct","prompt":"Write a 1-sentence summary of why local models matter.","stream":false}' \
  | jq -r '.response'
```

Notes:
- Model files persist in the `ollama` Docker volume.
- Change the exposed port with `OLLAMA_PORT=11434`.
