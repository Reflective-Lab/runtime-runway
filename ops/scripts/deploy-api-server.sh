#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
PROJECT_ID="${PROJECT_ID:-${GOOGLE_CLOUD_PROJECT:-}}"
REGION="${REGION:-europe-west1}"
SERVICE_NAME="${SERVICE_NAME:-api-server}"
REPOSITORY="${REPOSITORY:-runway}"
IMAGE_NAME="${IMAGE_NAME:-api-server}"
FIREBASE_PROJECT_ID="${FIREBASE_PROJECT_ID:-$PROJECT_ID}"

if [[ -z "$PROJECT_ID" ]]; then
    echo "Set PROJECT_ID or GOOGLE_CLOUD_PROJECT before deploying." >&2
    exit 1
fi

command -v gcloud >/dev/null 2>&1 || { echo "gcloud CLI is required" >&2; exit 1; }

IMAGE_URI="${REGION}-docker.pkg.dev/${PROJECT_ID}/${REPOSITORY}/${IMAGE_NAME}:$(git -C "$ROOT_DIR" rev-parse --short HEAD)"

echo "Project:        $PROJECT_ID"
echo "Region:         $REGION"
echo "Service:        $SERVICE_NAME"
echo "Image:          $IMAGE_URI"

gcloud auth configure-docker "${REGION}-docker.pkg.dev"

gcloud artifacts repositories describe "$REPOSITORY" \
    --location="$REGION" \
    --project="$PROJECT_ID" >/dev/null 2>&1 || \
gcloud artifacts repositories create "$REPOSITORY" \
    --repository-format=docker \
    --location="$REGION" \
    --project="$PROJECT_ID"

gcloud builds submit "$ROOT_DIR" \
    --tag "$IMAGE_URI" \
    --project="$PROJECT_ID" \
    --dockerfile="$ROOT_DIR/docker/Dockerfile.api-server"

gcloud run deploy "$SERVICE_NAME" \
    --image "$IMAGE_URI" \
    --region "$REGION" \
    --project "$PROJECT_ID" \
    --platform managed \
    --allow-unauthenticated \
    --set-env-vars "LOCAL_DEV=false,GOOGLE_CLOUD_PROJECT=${PROJECT_ID},FIREBASE_PROJECT_ID=${FIREBASE_PROJECT_ID}" \
    --service-account "api-server@${PROJECT_ID}.iam.gserviceaccount.com" \
    --memory 512Mi \
    --cpu 1 \
    --min-instances 0 \
    --max-instances 10

echo ""
echo "Deployed: $(gcloud run services describe "$SERVICE_NAME" --region "$REGION" --project "$PROJECT_ID" --format='value(status.url)')"
