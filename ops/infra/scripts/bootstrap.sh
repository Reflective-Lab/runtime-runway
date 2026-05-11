#!/usr/bin/env bash
set -euo pipefail
# One-time project bootstrap.
# Creates the Terraform state bucket and a Terraform service account with required roles.
# Run once before terraform init. Requires Owner or Editor + IAM Admin.
#
# Usage:
#   PROJECT_ID=my-project bash ops/infra/scripts/bootstrap.sh
#   PROJECT_ID=my-project REGION=us-central1 bash ops/infra/scripts/bootstrap.sh

PROJECT_ID="${PROJECT_ID:-}"
REGION="${REGION:-europe-west1}"
TF_STATE_BUCKET="${TF_STATE_BUCKET:-${PROJECT_ID}-tf-state}"
SA_NAME="terraform"

[[ -z "$PROJECT_ID" ]] && { echo "Set PROJECT_ID"; exit 1; }

command -v gcloud >/dev/null 2>&1 || { echo "gcloud CLI required"; exit 1; }

echo "Bootstrapping project: $PROJECT_ID"
echo "Region: $REGION"
echo "TF state bucket: gs://${TF_STATE_BUCKET}"

# GCS bucket for Terraform remote state
gcloud storage buckets create "gs://${TF_STATE_BUCKET}" \
  --project="$PROJECT_ID" \
  --location="$REGION" \
  --uniform-bucket-level-access \
  --public-access-prevention 2>/dev/null || echo "  (bucket already exists)"

gcloud storage buckets update "gs://${TF_STATE_BUCKET}" \
  --versioning \
  --project="$PROJECT_ID"

# Terraform service account
SA_EMAIL="${SA_NAME}@${PROJECT_ID}.iam.gserviceaccount.com"

gcloud iam service-accounts create "$SA_NAME" \
  --project="$PROJECT_ID" \
  --display-name="Terraform" 2>/dev/null || echo "  (SA already exists)"

for ROLE in \
  roles/editor \
  roles/iam.securityAdmin \
  roles/firebase.admin \
  roles/spanner.admin \
  roles/bigquery.admin \
  roles/redis.admin \
  roles/aiplatform.admin \
  roles/storage.admin \
  roles/pubsub.admin \
  roles/datastore.owner \
  roles/serviceusage.serviceUsageAdmin; do
  gcloud projects add-iam-policy-binding "$PROJECT_ID" \
    --member="serviceAccount:${SA_EMAIL}" \
    --role="$ROLE" \
    --quiet
done

gcloud storage buckets add-iam-policy-binding "gs://${TF_STATE_BUCKET}" \
  --member="serviceAccount:${SA_EMAIL}" \
  --role="roles/storage.admin"

echo ""
echo "Done. Next steps:"
echo "  1. Download SA key (or configure Workload Identity):"
echo "     gcloud iam service-accounts keys create key.json --iam-account=${SA_EMAIL}"
echo "     export GOOGLE_APPLICATION_CREDENTIALS=key.json"
echo ""
echo "  2. Uncomment the backend block in ops/infra/terraform/versions.tf:"
echo "     bucket = \"${TF_STATE_BUCKET}\""
echo ""
echo "  3. Copy and fill terraform.tfvars.example:"
echo "     cp ops/infra/terraform/terraform.tfvars.example ops/infra/terraform/terraform.tfvars"
echo ""
echo "  4. Provision:"
echo "     just infra-init && just infra-plan && just infra-apply"
