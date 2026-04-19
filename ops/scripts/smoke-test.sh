#!/usr/bin/env bash
set -euo pipefail

BASE_URL="${1:-http://127.0.0.1:8080}"

echo "Checking $BASE_URL/health"
curl -fsS "$BASE_URL/health"
echo

echo "Checking $BASE_URL/ready"
curl -fsS "$BASE_URL/ready"
echo

echo "Checking $BASE_URL/api/v1/templates"
curl -fsS "$BASE_URL/api/v1/templates"
echo

echo "Smoke test passed."
