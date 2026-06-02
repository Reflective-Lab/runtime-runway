#!/usr/bin/env bash
# Deploy catalyst-backend to Cloud Run.
#
# catalyst-backend has path deps on Runtime Runway crates (../../../runtime-runway/crates/).
# Cloud Build can't access sibling repos, so this script stages both trees
# under a common root that matches the relative path structure before submitting.
#
# Staged layout:
#   staging/
#     runtime-runway/crates/        ← Runtime Runway infra crates
#     marquee-apps/catalyst-biz/   ← catalyst source
#     Dockerfile
#     cloudbuild.yaml
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
CATALYST_ROOT="${CATALYST_ROOT:-$(cd "$ROOT_DIR/../marquee-apps/catalyst-biz" && pwd)}"
PROJECT_ID="${PROJECT_ID:-wolfgang-kb-prod}"
REGION="${REGION:-europe-west1}"
SERVICE_NAME="${SERVICE_NAME:-catalyst-backend}"
REPOSITORY="${REPOSITORY:-wolfgang}"
IMAGE_NAME="${IMAGE_NAME:-catalyst-backend}"
GCS_BUCKET="${GCS_BUCKET:-wolfgang-kb-prod-runway-catalyst}"
FIREBASE_PROJECT_ID="${FIREBASE_PROJECT_ID:-$PROJECT_ID}"
FIREBASE_API_KEY="${FIREBASE_API_KEY:-AIzaSyC2tfrm79dj9F3yATwpvropscc7B3DQ2oc}"
ROUTE_PREFIX="${ROUTE_PREFIX:-/catalyst}"
SERVICE_ACCOUNT="${SERVICE_ACCOUNT:-run-catalyst-backend@${PROJECT_ID}.iam.gserviceaccount.com}"

command -v gcloud >/dev/null 2>&1 || { echo "gcloud CLI is required" >&2; exit 1; }
[[ -d "$CATALYST_ROOT" ]] || { echo "Catalyst not found at $CATALYST_ROOT" >&2; exit 1; }

GIT_SHA="$(git -C "$ROOT_DIR" rev-parse --short HEAD)"
IMAGE_URI="${REGION}-docker.pkg.dev/${PROJECT_ID}/${REPOSITORY}/${IMAGE_NAME}:${GIT_SHA}"

CARGO_VERSION="$(grep '^version' "$CATALYST_ROOT/Cargo.toml" | head -1 | sed 's/.*= "\(.*\)"/\1/')"
VERSION_TAG="v$(echo "$CARGO_VERSION" | tr '.' '-')"
SHA_TAG="sha-${GIT_SHA}"

echo "Project:        $PROJECT_ID"
echo "Region:         $REGION"
echo "Service:        $SERVICE_NAME"
echo "Image:          $IMAGE_URI"
echo "Version:        $CARGO_VERSION  →  tags: $VERSION_TAG, $SHA_TAG"
echo "Route prefix:   $ROUTE_PREFIX"
echo "Catalyst root:  $CATALYST_ROOT"
echo ""

STAGING="$(mktemp -d "${TMPDIR:-/tmp}/runway-catalyst-build.XXXXXX")"
trap 'rm -rf "$STAGING"' EXIT

# Stage Runtime Runway workspace (crates + workspace Cargo.toml + Cargo.lock so
# workspace.package inheritance resolves inside the Cloud Build container)
mkdir -p "$STAGING/runtime-runway"
cp "$ROOT_DIR/Cargo.toml" "$STAGING/runtime-runway/Cargo.toml"
cp "$ROOT_DIR/Cargo.lock" "$STAGING/runtime-runway/Cargo.lock"
cp -R "$ROOT_DIR/crates" "$STAGING/runtime-runway/crates"

# Stage catalyst-biz (exclude build artifacts and node_modules)
mkdir -p "$STAGING/marquee-apps"
rsync -a \
    --exclude='target/' \
    --exclude='node_modules/' \
    --exclude='.git/' \
    --exclude='.svelte-kit/' \
    --exclude='build/' \
    --exclude='dist/' \
    "$CATALYST_ROOT/" "$STAGING/marquee-apps/catalyst-biz/"

# Dockerfile — WORKDIR matches the path dep structure:
#   /build/marquee-apps/catalyst-biz/backend/Cargo.toml
#   path = "../../../runtime-runway/crates/runway-auth"
#   resolves to /build/runtime-runway/crates/runway-auth ✓
cat > "$STAGING/Dockerfile" << 'DOCKERFILE'
FROM rust:1.94-bookworm AS builder

WORKDIR /build
COPY runtime-runway/ runtime-runway/
COPY marquee-apps/ marquee-apps/

WORKDIR /build/marquee-apps/catalyst-biz
RUN cargo build -p catalyst-backend --release

FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=builder /build/marquee-apps/catalyst-biz/target/release/catalyst-backend /usr/local/bin/catalyst-backend

ENV RUST_LOG=info
ENV PORT=8080
ENV LOCAL_DEV=true
ENV STORAGE_PATH=/tmp/catalyst

EXPOSE 8080

HEALTHCHECK --interval=10s --timeout=3s --start-period=15s --retries=12 \
  CMD curl -fsS http://127.0.0.1:8080/health || exit 1

CMD ["catalyst-backend"]
DOCKERFILE

cat > "$STAGING/cloudbuild.yaml" << CLOUDBUILD
steps:
  - name: 'gcr.io/cloud-builders/docker'
    args: ['build', '-t', '\${_IMAGE_URI}', '.']
    timeout: 1200s
images: ['\${_IMAGE_URI}']
options:
  machineType: 'E2_HIGHCPU_8'
  logging: CLOUD_LOGGING_ONLY
timeout: 1800s
CLOUDBUILD

gcloud auth configure-docker "${REGION}-docker.pkg.dev" --quiet

echo "Building image via Cloud Build (staging both Runtime Runway crates + catalyst-biz)..."
gcloud builds submit "$STAGING" \
    --config="$STAGING/cloudbuild.yaml" \
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
    --region "$REGION" --project "$PROJECT_ID" \
    --format='value(status.latestReadyRevisionName)')

gcloud run services update-traffic "$SERVICE_NAME" \
    --region "$REGION" --project "$PROJECT_ID" \
    --set-tags "${VERSION_TAG}=${REVISION},${SHA_TAG}=${REVISION}"

echo ""
SERVICE_URL=$(gcloud run services describe "$SERVICE_NAME" \
    --region "$REGION" --project "$PROJECT_ID" \
    --format='value(status.url)')
SERVICE_HOST="${SERVICE_URL#https://}"

echo "Deployed:        $SERVICE_URL"
echo "Health:          $SERVICE_URL/health"
echo "Version pinned:  https://${VERSION_TAG}---${SERVICE_HOST}"
echo "SHA pinned:      https://${SHA_TAG}---${SERVICE_HOST}"

echo ""
echo "Registering in apps portal..."
bash "$ROOT_DIR/ops/scripts/register-app.sh" \
    --key        "catalyst" \
    --name       "Catalyst" \
    --description "Business ops workflows with human approval gates" \
    --path       "$ROUTE_PREFIX" \
    --status-path "${ROUTE_PREFIX}/status" \
    --version    "$CARGO_VERSION" \
    --sha        "$GIT_SHA"

echo "Deploying apps portal..."
(cd "$ROOT_DIR/ops/infra/firebase/apps" && firebase deploy --only hosting:apps-reflective-se --project "$PROJECT_ID" --non-interactive)
