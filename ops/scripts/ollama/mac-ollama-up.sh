#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
DOCKER_DIR="$ROOT_DIR/docker"

MODEL="${1:-${OLLAMA_MODEL:-qwen2.5:14b-instruct}}"
STOP_DOCKER_OLLAMA="${STOP_DOCKER_OLLAMA:-1}"

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "This script is intended for macOS (Darwin)." >&2
  exit 1
fi

if ! sysctl -n machdep.cpu.brand_string >/dev/null 2>&1; then
  echo "Unable to read CPU information." >&2
  exit 1
fi

brew_bin() {
  if [[ -x "/opt/homebrew/bin/brew" ]]; then
    echo "/opt/homebrew/bin/brew"
    return 0
  fi
  if [[ -x "/usr/local/bin/brew" ]]; then
    echo "/usr/local/bin/brew"
    return 0
  fi
  if command -v brew >/dev/null 2>&1; then
    command -v brew
    return 0
  fi
  return 1
}

compose_cmd() {
  if command -v docker >/dev/null 2>&1 && docker compose version >/dev/null 2>&1; then
    echo "docker compose"
    return 0
  fi
  return 1
}

maybe_stop_docker_ollama() {
  [[ "$STOP_DOCKER_OLLAMA" == "1" ]] || return 0
  if ! command -v docker >/dev/null 2>&1; then
    return 0
  fi
  if [[ ! -d "$DOCKER_DIR" ]]; then
    return 0
  fi

  local cc
  cc="$(compose_cmd || true)"
  if [[ -z "${cc:-}" ]]; then
    return 0
  fi

  # If compose isn't running, this is a no-op.
  if (cd "$DOCKER_DIR" && $cc ps --status running --services 2>/dev/null | grep -qx "ollama"); then
    echo "Stopping docker compose service 'ollama' to free port 11434 for native Ollama (Metal)."
    (cd "$DOCKER_DIR" && $cc --profile llm stop ollama) >/dev/null
  fi
}

ollama_http_base() {
  local host="${OLLAMA_HOST:-127.0.0.1:11434}"
  if [[ "$host" != http://* && "$host" != https://* ]]; then
    host="http://${host}"
  fi
  echo "$host"
}

wait_for_ollama() {
  local base
  base="$(ollama_http_base)"
  for _ in $(seq 1 120); do
    if curl -fsS "${base}/api/tags" >/dev/null 2>&1; then
      echo "Ollama is responding at ${base}"
      return 0
    fi
    sleep 0.25
  done
  echo "Timed out waiting for Ollama at ${base}/api/tags" >&2
  return 1
}

BREW="$(brew_bin || true)"
if [[ -z "${BREW:-}" ]]; then
  echo "Homebrew is required. Install from https://brew.sh and re-run." >&2
  exit 1
fi

maybe_stop_docker_ollama

if ! command -v ollama >/dev/null 2>&1; then
  echo "Installing ollama via Homebrew…"
  "$BREW" install ollama
fi

echo "Starting ollama as a Homebrew service…"
"$BREW" services start ollama >/dev/null

wait_for_ollama

echo "Pulling model: ${MODEL}"
OLLAMA_HOST="${OLLAMA_HOST:-127.0.0.1:11434}" ollama pull "${MODEL}"

echo "Ready."
echo "Tip: run a quick check with: just mac-ollama-smoke \"${MODEL}\""
