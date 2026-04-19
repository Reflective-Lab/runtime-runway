#!/usr/bin/env bash
set -euo pipefail

PROJECT_ID="${PROJECT_ID:-${GOOGLE_CLOUD_PROJECT:-}}"
REGION="${REGION:-europe-west1}"
SERVICE_NAME="${SERVICE_NAME:-converge-llm-worker}"
REPOSITORY="${REPOSITORY:-converge}"
IMAGE_NAME="${IMAGE_NAME:-converge-llm-worker}"
MODEL_PATH="${MODEL_PATH:-/models/llama3}"

if [[ -z "$PROJECT_ID" ]]; then
  echo "Set PROJECT_ID or GOOGLE_CLOUD_PROJECT first." >&2
  exit 1
fi

IMAGE_URI="${REGION}-docker.pkg.dev/${PROJECT_ID}/${REPOSITORY}/${IMAGE_NAME}:$(git rev-parse --short HEAD)"

gcloud auth configure-docker "${REGION}-docker.pkg.dev"
gcloud artifacts repositories describe "$REPOSITORY" \
  --location="$REGION" \
  --project="$PROJECT_ID" >/dev/null 2>&1 || \
gcloud artifacts repositories create "$REPOSITORY" \
  --repository-format=docker \
  --location="$REGION" \
  --project="$PROJECT_ID"

gcloud builds submit \
  --tag "$IMAGE_URI" \
  --file deploy/gpu/cloudrun/Dockerfile \
  --project "$PROJECT_ID" .

gcloud run deploy "$SERVICE_NAME" \
  --project "$PROJECT_ID" \
  --region "$REGION" \
  --image "$IMAGE_URI" \
  --port 50051 \
  --gpu 1 \
  --gpu-type nvidia-l4 \
  --cpu 8 \
  --memory 32Gi \
  --no-allow-unauthenticated \
  --set-env-vars "BIND_ADDR=0.0.0.0:50051,MODEL_PATH=${MODEL_PATH},MODEL_VARIANT=${MODEL_VARIANT:-llama3-8b},MAX_SEQ_LEN=${MAX_SEQ_LEN:-4096}"
