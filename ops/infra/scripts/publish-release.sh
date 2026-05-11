#!/usr/bin/env bash
set -euo pipefail
# Upload platform binaries for a marquee app to the releases bucket and invalidate CDN cache.
#
# Usage:
#   APP=folio VERSION=v1.2.0 PROJECT_ID=my-project bash ops/infra/scripts/publish-release.sh
#   APP=scout VERSION=v2.0.0 PROJECT_ID=my-project ENV=staging bash ...
#
# Expected artifacts in dist/ (produced by each app's CI pipeline):
#   dist/macos-aarch64/
#   dist/macos-x86_64/
#   dist/windows-x86_64/
#   dist/linux-x86_64/
#   dist/linux-aarch64/
#
# Resulting bucket paths:
#   gs://{bucket}/{app}/{version}/{platform}-{arch}/{filename}

APP="${APP:-}"
VERSION="${VERSION:-}"
PROJECT_ID="${PROJECT_ID:-}"
ENV="${ENV:-prod}"
BUCKET="reflective-${ENV}-releases"
DIST_DIR="${DIST_DIR:-dist}"

[[ -z "$APP" ]]        && { echo "Set APP (e.g. folio, scout, quorum, wolfgang, vouch)"; exit 1; }
[[ -z "$VERSION" ]]    && { echo "Set VERSION (e.g. v1.2.0)"; exit 1; }
[[ -z "$PROJECT_ID" ]] && { echo "Set PROJECT_ID"; exit 1; }

command -v gcloud >/dev/null 2>&1 || { echo "gcloud CLI required"; exit 1; }

DEST="gs://${BUCKET}/${APP}/${VERSION}"
echo "Publishing ${APP} ${VERSION} → ${DEST}/"

PLATFORMS=(
  "macos-aarch64"
  "macos-x86_64"
  "windows-x86_64"
  "linux-x86_64"
  "linux-aarch64"
)

UPLOADED=0
for PLATFORM in "${PLATFORMS[@]}"; do
  SRC="${DIST_DIR}/${PLATFORM}"
  [[ -d "$SRC" ]] || { echo "  Skipping $PLATFORM (no dist/${PLATFORM}/ directory)"; continue; }

  echo "  Uploading $PLATFORM ..."
  gcloud storage cp --recursive "${SRC}/" \
    "${DEST}/${PLATFORM}/" \
    --project="$PROJECT_ID"
  UPLOADED=$((UPLOADED + 1))
done

[[ $UPLOADED -eq 0 ]] && { echo "No platform directories found in ${DIST_DIR}/"; exit 1; }

# latest.json per app — clients poll this to detect available updates
LATEST_JSON=$(cat <<JSON
{
  "app": "${APP}",
  "version": "${VERSION}",
  "published_at": "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
}
JSON
)

echo "$LATEST_JSON" | gcloud storage cp - \
  "gs://${BUCKET}/${APP}/latest.json" \
  --content-type="application/json" \
  --project="$PROJECT_ID"

# Invalidate CDN cache for this app's new version + its latest pointer
LB_NAME="reflective-${ENV}-releases"
echo "Invalidating CDN cache ..."
gcloud compute url-maps invalidate-cdn-cache "$LB_NAME" \
  --path="/${APP}/${VERSION}/*" \
  --project="$PROJECT_ID" \
  --async 2>/dev/null || echo "  (CDN invalidation skipped — Load Balancer not yet provisioned)"

gcloud compute url-maps invalidate-cdn-cache "$LB_NAME" \
  --path="/${APP}/latest.json" \
  --project="$PROJECT_ID" \
  --async 2>/dev/null || true

echo ""
echo "Done. Published ${UPLOADED} platform(s)."
echo "  Bucket:  ${DEST}/"
echo "  Latest:  gs://${BUCKET}/${APP}/latest.json"
