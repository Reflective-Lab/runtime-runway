#!/usr/bin/env bash
set -euo pipefail

PROJECT_ID="${PROJECT_ID:-${GOOGLE_CLOUD_PROJECT:-}}"
REGION="${REGION:-europe-west1}"
SERVICE_NAME="${SERVICE_NAME:-converge-runtime}"
REPOSITORY="${REPOSITORY:-converge}"
IMAGE_NAME="${IMAGE_NAME:-converge-runtime}"
FIREBASE_PROJECT_ID="${FIREBASE_PROJECT_ID:-$PROJECT_ID}"

if [[ -z "$PROJECT_ID" ]]; then
  echo "Set PROJECT_ID or GOOGLE_CLOUD_PROJECT before deploying." >&2
  exit 1
fi

command -v gcloud >/dev/null 2>&1 || {
  echo "gcloud CLI is required" >&2
  exit 1
}

IMAGE_URI="${REGION}-docker.pkg.dev/${PROJECT_ID}/${REPOSITORY}/${IMAGE_NAME}:$(git rev-parse --short HEAD)"

echo "Using project: $PROJECT_ID"
echo "Using region:  $REGION"
echo "Using service: $SERVICE_NAME"
echo "Using Firebase project: $FIREBASE_PROJECT_ID"
echo "Building image: $IMAGE_URI"

gcloud auth configure-docker "${REGION}-docker.pkg.dev"
gcloud artifacts repositories describe "$REPOSITORY" \
  --location="$REGION" \
  --project="$PROJECT_ID" >/dev/null 2>&1 || \
gcloud artifacts repositories create "$REPOSITORY" \
  --repository-format=docker \
  --location="$REGION" \
  --project="$PROJECT_ID"

gcloud builds submit --tag "$IMAGE_URI" --project="$PROJECT_ID"

gcloud run deploy "$SERVICE_NAME" \
  --image "$IMAGE_URI" \
  --region "$REGION" \
  --project "$PROJECT_ID" \
  --platform managed \
  --allow-unauthenticated \
  --port 8080 \
  --set-env-vars "PORT=8080,RUST_LOG=${RUST_LOG:-info},LOCAL_DEV=false,GCP_PROJECT_ID=${PROJECT_ID},GOOGLE_CLOUD_PROJECT=${PROJECT_ID},FIREBASE_PROJECT_ID=${FIREBASE_PROJECT_ID}"

echo "Cloud Run deployment finished."
