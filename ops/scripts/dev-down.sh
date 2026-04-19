#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
STATE_DIR="$ROOT_DIR/.converge"
PID_FILE="$STATE_DIR/runtime.pid"
MODE="${1:-auto}"

compose_cmd() {
  if command -v docker >/dev/null 2>&1 && docker compose version >/dev/null 2>&1; then
    echo "docker compose"
    return 0
  fi
  if command -v podman >/dev/null 2>&1 && podman compose version >/dev/null 2>&1; then
    echo "podman compose"
    return 0
  fi
  if command -v podman-compose >/dev/null 2>&1; then
    echo "podman-compose"
    return 0
  fi
  return 1
}

stop_native() {
  if [[ -f "$PID_FILE" ]]; then
    local pid
    pid="$(cat "$PID_FILE")"
    if kill -0 "$pid" >/dev/null 2>&1; then
      kill "$pid"
      echo "Stopped native runtime PID $pid"
    fi
    rm -f "$PID_FILE"
  fi
}

stop_container() {
  local compose
  compose="$(compose_cmd)" || return 0
  (
    cd "$ROOT_DIR"
    eval "$compose down"
  )
}

case "$MODE" in
  auto)
    stop_native
    stop_container
    ;;
  native)
    stop_native
    ;;
  container|compose)
    stop_container
    ;;
  *)
    echo "Usage: $0 [auto|native|container]" >&2
    exit 1
    ;;
esac
