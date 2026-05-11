#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
PROJECT_ID="${PROJECT_ID:-wolfgang-kb-prod}"
REGION="${REGION:-europe-west1}"
SERVICE_NAME="${SERVICE_NAME:-api-server}"
REPOSITORY="${REPOSITORY:-wolfgang}"
IMAGE_NAME="${IMAGE_NAME:-api-server}"
GCS_BUCKET="${GCS_BUCKET:-wolfgang-kb-prod-runway-api}"
FIREBASE_PROJECT_ID="${FIREBASE_PROJECT_ID:-$PROJECT_ID}"
FIREBASE_API_KEY="${FIREBASE_API_KEY:-AIzaSyC2tfrm79dj9F3yATwpvropscc7B3DQ2oc}"
SERVICE_ACCOUNT="${SERVICE_ACCOUNT:-run-api-server@${PROJECT_ID}.iam.gserviceaccount.com}"

command -v gcloud >/dev/null 2>&1 || { echo "gcloud CLI is required" >&2; exit 1; }

IMAGE_URI="${REGION}-docker.pkg.dev/${PROJECT_ID}/${REPOSITORY}/${IMAGE_NAME}:$(git -C "$ROOT_DIR" rev-parse --short HEAD)"

echo "Project:        $PROJECT_ID"
echo "Region:         $REGION"
echo "Service:        $SERVICE_NAME"
echo "Repository:     $REPOSITORY"
echo "Image:          $IMAGE_URI"
echo "GCS bucket:     $GCS_BUCKET"
echo "SA:             $SERVICE_ACCOUNT"
echo ""

gcloud auth configure-docker "${REGION}-docker.pkg.dev" --quiet

echo "Building image via Cloud Build..."
gcloud builds submit "$ROOT_DIR" \
    --config="$ROOT_DIR/cloudbuild.api-server.yaml" \
    --substitutions="_IMAGE_URI=${IMAGE_URI}" \
    --project="$PROJECT_ID"

echo "Deploying to Cloud Run..."
gcloud run deploy "$SERVICE_NAME" \
    --image "$IMAGE_URI" \
    --region "$REGION" \
    --project "$PROJECT_ID" \
    --platform managed \
    --allow-unauthenticated \
    --service-account "$SERVICE_ACCOUNT" \
    --set-env-vars "LOCAL_DEV=false,GOOGLE_CLOUD_PROJECT=${PROJECT_ID},FIREBASE_PROJECT_ID=${FIREBASE_PROJECT_ID},GCS_BUCKET=${GCS_BUCKET},FIREBASE_API_KEY=${FIREBASE_API_KEY},ROUTE_PREFIX=/api-server" \
    --memory 512Mi \
    --cpu 1 \
    --min-instances 0 \
    --max-instances 5

echo ""
URL=$(gcloud run services describe "$SERVICE_NAME" --region "$REGION" --project "$PROJECT_ID" --format='value(status.url)')
echo "Deployed: $URL"
echo "Health:   $URL/health"
