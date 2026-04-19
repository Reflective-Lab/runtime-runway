#!/usr/bin/env bash
set -euo pipefail

export BIND_ADDR="${BIND_ADDR:-0.0.0.0:50051}"
export MODEL_PATH="${MODEL_PATH:-/workspace/models/llama3}"
export MODEL_VARIANT="${MODEL_VARIANT:-llama3-8b}"
export MAX_SEQ_LEN="${MAX_SEQ_LEN:-4096}"
export RUST_LOG="${RUST_LOG:-info}"

exec converge-llm-server
