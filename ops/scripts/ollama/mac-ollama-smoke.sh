#!/usr/bin/env bash
set -euo pipefail

MODEL="${1:-${OLLAMA_MODEL:-qwen2.5:14b-instruct}}"

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "This script is intended for macOS (Darwin)." >&2
  exit 1
fi

ollama_http_base() {
  local host="${OLLAMA_HOST:-127.0.0.1:11434}"
  if [[ "$host" != http://* && "$host" != https://* ]]; then
    host="http://${host}"
  fi
  echo "$host"
}

BASE="$(ollama_http_base)"

curl -fsS "${BASE}/api/chat" \
  -d "$(python3 - <<PY
import json
print(json.dumps({
  "model": "${MODEL}",
  "messages": [{"role": "user", "content": "Reply with exactly: ok"}],
  "stream": False,
}))
PY
)" | python3 -c 'import json,sys; print(json.load(sys.stdin)["message"]["content"].strip())'
