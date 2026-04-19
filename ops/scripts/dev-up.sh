#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
STATE_DIR="$ROOT_DIR/.converge"
PID_FILE="$STATE_DIR/runtime.pid"
LOG_FILE="$STATE_DIR/runtime.log"
MODE="${1:-auto}"
PORT="${PORT:-8080}"
FEATURES="${CONVERGE_RUNTIME_FEATURES:-gcp,auth,firebase}"

mkdir -p "$STATE_DIR"

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

wait_for_health() {
  local url="http://127.0.0.1:${PORT}/health"
  for _ in $(seq 1 60); do
    if curl -fsS "$url" >/dev/null 2>&1; then
      echo "Converge Runtime is ready at $url"
      return 0
    fi
    sleep 1
  done
  echo "Timed out waiting for $url" >&2
  return 1
}

start_native() {
  command -v cargo >/dev/null 2>&1 || {
    echo "cargo is required for native mode" >&2
    exit 1
  }

  if [[ -f "$PID_FILE" ]] && kill -0 "$(cat "$PID_FILE")" >/dev/null 2>&1; then
    echo "Native runtime already running with PID $(cat "$PID_FILE")"
    wait_for_health
    return 0
  fi

  echo "Starting converge-runtime in native mode on port ${PORT}"
  (
    cd "$ROOT_DIR"
    nohup env PORT="$PORT" \
      LOCAL_DEV="${LOCAL_DEV:-true}" \
      RUST_LOG="${RUST_LOG:-info}" \
      GCP_PROJECT_ID="${GCP_PROJECT_ID:-local-project}" \
      GOOGLE_CLOUD_PROJECT="${GOOGLE_CLOUD_PROJECT:-${GCP_PROJECT_ID:-local-project}}" \
      FIREBASE_PROJECT_ID="${FIREBASE_PROJECT_ID:-${GCP_PROJECT_ID:-local-project}}" \
      FIREBASE_AUTH_EMULATOR_HOST="${FIREBASE_AUTH_EMULATOR_HOST:-}" \
      cargo run -p converge-runtime --features "$FEATURES" >"$LOG_FILE" 2>&1 </dev/null &
    echo $! >"$PID_FILE"
  ) &
  sleep 1

  if ! wait_for_health; then
    echo "Runtime failed to become healthy. Recent logs:" >&2
    tail -n 40 "$LOG_FILE" >&2 || true
    exit 1
  fi

  echo "Logs: $LOG_FILE"
  echo "Features: $FEATURES"
}

start_container() {
  local compose
  compose="$(compose_cmd)" || {
    echo "No supported compose backend found. Install Docker Desktop, OrbStack, Colima+docker, or Podman." >&2
    exit 1
  }

  echo "Starting converge-runtime in container mode with: $compose"
  (
    cd "$ROOT_DIR"
    eval "$compose up --build -d converge-runtime"
  )

  wait_for_health
}

case "$MODE" in
  auto)
    if command -v cargo >/dev/null 2>&1; then
      start_native
    else
      start_container
    fi
    ;;
  native)
    start_native
    ;;
  container|compose)
    start_container
    ;;
  *)
    echo "Usage: $0 [auto|native|container]" >&2
    exit 1
    ;;
esac
