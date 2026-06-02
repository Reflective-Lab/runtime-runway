#!/usr/bin/env bash
# Deploy api-server to Cloud Run.
#
# Versioning model:
#   apps.reflective.se/api-server/**      — this service (rolling latest)
#   apps.reflective.se/api-server/v3/**   — SERVICE_NAME=api-server-v3 ROUTE_PREFIX=/api-server/v3
#
# After each deploy the new revision is tagged with the semantic version
# (v3-4-1) and sha (sha-abc1234) so it has a stable direct URL for rollback
# or frontend pinning: https://v3-4-1---api-server-{hash}-ew.a.run.app
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
ROUTE_PREFIX="${ROUTE_PREFIX:-/api-server}"
SERVICE_ACCOUNT="${SERVICE_ACCOUNT:-run-api-server@${PROJECT_ID}.iam.gserviceaccount.com}"

command -v gcloud >/dev/null 2>&1 || { echo "gcloud CLI is required" >&2; exit 1; }

GIT_SHA="$(git -C "$ROOT_DIR" rev-parse --short HEAD)"
IMAGE_URI="${REGION}-docker.pkg.dev/${PROJECT_ID}/${REPOSITORY}/${IMAGE_NAME}:${GIT_SHA}"

# Read version from workspace Cargo.toml (e.g. "3.4.1")
CARGO_VERSION="$(grep '^version' "$ROOT_DIR/Cargo.toml" | head -1 | sed 's/.*= "\(.*\)"/\1/')"
# Cloud Run tag format: lowercase letters, digits, hyphens only — dots not allowed
VERSION_TAG="v$(echo "$CARGO_VERSION" | tr '.' '-')"
SHA_TAG="sha-${GIT_SHA}"

echo "Project:        $PROJECT_ID"
echo "Region:         $REGION"
echo "Service:        $SERVICE_NAME"
echo "Image:          $IMAGE_URI"
echo "Version:        $CARGO_VERSION  →  tags: $VERSION_TAG, $SHA_TAG"
echo "Route prefix:   $ROUTE_PREFIX"
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
    --set-env-vars "LOCAL_DEV=false,GOOGLE_CLOUD_PROJECT=${PROJECT_ID},FIREBASE_PROJECT_ID=${FIREBASE_PROJECT_ID},GCS_BUCKET=${GCS_BUCKET},FIREBASE_API_KEY=${FIREBASE_API_KEY},ROUTE_PREFIX=${ROUTE_PREFIX}" \
    --memory 512Mi \
    --cpu 1 \
    --min-instances 0 \
    --max-instances 5

echo "Tagging revision..."
REVISION=$(gcloud run services describe "$SERVICE_NAME" \
    --region "$REGION" \
    --project "$PROJECT_ID" \
    --format='value(status.latestReadyRevisionName)')

gcloud run services update-traffic "$SERVICE_NAME" \
    --region "$REGION" \
    --project "$PROJECT_ID" \
    --set-tags "${VERSION_TAG}=${REVISION},${SHA_TAG}=${REVISION}"

echo ""
SERVICE_URL=$(gcloud run services describe "$SERVICE_NAME" --region "$REGION" --project "$PROJECT_ID" --format='value(status.url)')
# Derive the tagged URL base from the service URL
# Service URL: https://api-server-{hash}-ew.a.run.app
# Tagged URL:  https://{tag}---api-server-{hash}-ew.a.run.app
SERVICE_HOST="${SERVICE_URL#https://}"
TAGGED_BASE="https://${VERSION_TAG}---${SERVICE_HOST}"

echo "Deployed:        $SERVICE_URL"
echo "Health:          $SERVICE_URL/health"
echo "Version pinned:  $TAGGED_BASE  (tag: $VERSION_TAG)"
echo "SHA pinned:      https://${SHA_TAG}---${SERVICE_HOST}  (tag: $SHA_TAG)"
echo ""
echo "To freeze this version as a named route:"
echo "  SERVICE_NAME=${SERVICE_NAME}-v$(echo "$CARGO_VERSION" | cut -d. -f1) ROUTE_PREFIX=${ROUTE_PREFIX}/v$(echo "$CARGO_VERSION" | cut -d. -f1) just api-deploy"

echo ""
echo "Registering in apps portal..."
bash "$ROOT_DIR/ops/scripts/register-app.sh" \
    --key         "api-server" \
    --name        "API Server" \
    --description "Runtime Runway reference service — auth, storage, telemetry" \
    --path        "$ROUTE_PREFIX" \
    --status-path "${ROUTE_PREFIX}/status" \
    --version     "$CARGO_VERSION" \
    --sha         "$GIT_SHA"

echo "Deploying apps portal..."
(cd "$ROOT_DIR/ops/infra/firebase/apps" && firebase deploy --only hosting:apps-reflective-se --project "$PROJECT_ID" --non-interactive)
