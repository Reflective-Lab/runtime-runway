#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
CONVERGE_ROOT="${CONVERGE_ROOT:-$ROOT_DIR/../converge}"
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
[[ -f "$CONVERGE_ROOT/Cargo.toml" ]] || {
  echo "Converge source not found at $CONVERGE_ROOT" >&2
  echo "Set CONVERGE_ROOT or check out ../converge next to runway." >&2
  exit 1
}

IMAGE_URI="${REGION}-docker.pkg.dev/${PROJECT_ID}/${REPOSITORY}/${IMAGE_NAME}:$(git -C "$ROOT_DIR" rev-parse --short HEAD)"
STAGING_DIR="$(mktemp -d "${TMPDIR:-/tmp}/runway-cloud-build.XXXXXX")"
trap 'rm -rf "$STAGING_DIR"' EXIT

echo "Using project: $PROJECT_ID"
echo "Using region:  $REGION"
echo "Using service: $SERVICE_NAME"
echo "Using Firebase project: $FIREBASE_PROJECT_ID"
echo "Building image: $IMAGE_URI"
echo "Using Converge source: $CONVERGE_ROOT"

cp "$ROOT_DIR/docker/Dockerfile" "$STAGING_DIR/Dockerfile"
cp "$CONVERGE_ROOT/Cargo.toml" "$STAGING_DIR/Cargo.toml"
cp "$CONVERGE_ROOT/Cargo.lock" "$STAGING_DIR/Cargo.lock"
cp -R "$CONVERGE_ROOT/crates" "$STAGING_DIR/crates"
cp -R "$CONVERGE_ROOT/examples" "$STAGING_DIR/examples"
cp -R "$CONVERGE_ROOT/schema" "$STAGING_DIR/schema"

gcloud auth configure-docker "${REGION}-docker.pkg.dev"
gcloud artifacts repositories describe "$REPOSITORY" \
  --location="$REGION" \
  --project="$PROJECT_ID" >/dev/null 2>&1 || \
gcloud artifacts repositories create "$REPOSITORY" \
  --repository-format=docker \
  --location="$REGION" \
  --project="$PROJECT_ID"

gcloud builds submit "$STAGING_DIR" --tag "$IMAGE_URI" --project="$PROJECT_ID"

gcloud run deploy "$SERVICE_NAME" \
  --image "$IMAGE_URI" \
  --region "$REGION" \
  --project "$PROJECT_ID" \
  --platform managed \
  --allow-unauthenticated \
  --port 8080 \
  --set-env-vars "PORT=8080,RUST_LOG=${RUST_LOG:-info},LOCAL_DEV=false,GCP_PROJECT_ID=${PROJECT_ID},GOOGLE_CLOUD_PROJECT=${PROJECT_ID},FIREBASE_PROJECT_ID=${FIREBASE_PROJECT_ID}"

echo "Cloud Run deployment finished."
