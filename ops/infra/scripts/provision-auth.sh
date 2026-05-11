#!/usr/bin/env bash
set -euo pipefail
# Configure Firebase Auth providers and deploy security rules.
# Run after terraform apply (Firestore database must exist).
#
# Usage:
#   PROJECT_ID=my-project bash ops/infra/scripts/provision-auth.sh
#   GOOGLE_OAUTH_CLIENT_ID=... GOOGLE_OAUTH_CLIENT_SECRET=... PROJECT_ID=... bash ...

PROJECT_ID="${PROJECT_ID:-}"
FIREBASE_DIR="ops/infra/firebase"

[[ -z "$PROJECT_ID" ]] && { echo "Set PROJECT_ID"; exit 1; }

command -v gcloud >/dev/null 2>&1   || { echo "gcloud CLI required"; exit 1; }
command -v firebase >/dev/null 2>&1 || { echo "firebase CLI required: npm install -g firebase-tools"; exit 1; }

[[ -f "${FIREBASE_DIR}/.firebaserc" ]] || {
  echo "Copy ${FIREBASE_DIR}/.firebaserc.example to ${FIREBASE_DIR}/.firebaserc and fill in project IDs"
  exit 1
}

echo "Configuring Firebase Auth for: $PROJECT_ID"

# Enable Email/Password sign-in
gcloud alpha identity-platform config update \
  --project="$PROJECT_ID" \
  --no-enable-email-link-signin 2>/dev/null || true

gcloud alpha identity-platform tenants list \
  --project="$PROJECT_ID" >/dev/null 2>&1 || true

# Enable Google Sign-In provider (set GOOGLE_OAUTH_CLIENT_ID/SECRET env vars)
if [[ -n "${GOOGLE_OAUTH_CLIENT_ID:-}" && -n "${GOOGLE_OAUTH_CLIENT_SECRET:-}" ]]; then
  gcloud alpha identity-platform oauth-idp-configs create google.com \
    --project="$PROJECT_ID" \
    --display-name="Google" \
    --client-id="$GOOGLE_OAUTH_CLIENT_ID" \
    --client-secret="$GOOGLE_OAUTH_CLIENT_SECRET" \
    --enabled 2>/dev/null || \
  gcloud alpha identity-platform oauth-idp-configs update google.com \
    --project="$PROJECT_ID" \
    --client-id="$GOOGLE_OAUTH_CLIENT_ID" \
    --client-secret="$GOOGLE_OAUTH_CLIENT_SECRET" \
    --enabled
  echo "Google Sign-In configured."
else
  echo "Skipping Google Sign-In (GOOGLE_OAUTH_CLIENT_ID not set)"
fi

# Deploy Firestore rules + composite indexes
firebase deploy \
  --only firestore:rules,firestore:indexes \
  --project="$PROJECT_ID" \
  --config "${FIREBASE_DIR}/firebase.json"

# Deploy Storage security rules
firebase deploy \
  --only storage \
  --project="$PROJECT_ID" \
  --config "${FIREBASE_DIR}/firebase.json"

echo ""
echo "Auth and rules deployed for: $PROJECT_ID"
echo ""
echo "Next: set Firebase Auth custom claims (org_id, app_access) from your app server"
echo "using the Firebase Admin SDK or the REST API."
