locals {
  # Full set of apps that get Cloud Run service accounts.
  # Must match var.apps — kept in sync here for IAM iteration.
  cloudrun_apps = toset(var.apps)
}

module "apis" {
  source     = "./modules/apis"
  project_id = var.project_id
}

module "firestore" {
  source     = "./modules/firestore"
  project_id = var.project_id
  region     = var.region
  env        = var.env
  depends_on = [module.apis]
}

module "spanner" {
  source         = "./modules/spanner"
  project_id     = var.project_id
  env            = var.env
  spanner_config = var.spanner_config
  depends_on     = [module.apis]
}

module "storage" {
  source     = "./modules/storage"
  project_id = var.project_id
  region     = var.region
  env        = var.env
  apps       = var.apps
  depends_on = [module.apis]
}

module "pubsub" {
  source     = "./modules/pubsub"
  project_id = var.project_id
  env        = var.env
  apps       = var.apps
  depends_on = [module.apis]
}

module "bigquery" {
  source     = "./modules/bigquery"
  project_id = var.project_id
  region     = var.region
  env        = var.env
  apps       = var.apps
  depends_on = [module.apis]
}

module "vertex_vector" {
  source     = "./modules/vertex-vector"
  project_id = var.project_id
  region     = var.region
  env        = var.env
  depends_on = [module.storage]
}

module "memorystore" {
  source     = "./modules/memorystore"
  project_id = var.project_id
  region     = var.region
  env        = var.env
  tier       = var.redis_tier
  memory_gb  = var.redis_memory_gb
  depends_on = [module.apis]
}

module "releases" {
  source                = "./modules/releases"
  project_id            = var.project_id
  env                   = var.env
  domain                = var.releases_domain
  multi_region_location = var.releases_location
  depends_on            = [module.apis]
}

# ── IAM: Cloud Run service accounts ───────────────────────────────────────────
# One SA per app: {app}-cloudrun@{project}.iam.gserviceaccount.com

resource "google_service_account" "cloudrun_app" {
  for_each = local.cloudrun_apps

  project      = var.project_id
  account_id   = "${each.key}-cloudrun"
  display_name = "${title(each.key)} Cloud Run Service Account"

  depends_on = [module.apis]
}

# Firestore read/write for each app's Cloud Run service
resource "google_project_iam_member" "cloudrun_firestore" {
  for_each = local.cloudrun_apps

  project = var.project_id
  role    = "roles/datastore.user"
  member  = "serviceAccount:${google_service_account.cloudrun_app[each.key].email}"
}

# Secret Manager access — tokens, API keys, etc.
resource "google_project_iam_member" "cloudrun_secretmanager" {
  for_each = local.cloudrun_apps

  project = var.project_id
  role    = "roles/secretmanager.secretAccessor"
  member  = "serviceAccount:${google_service_account.cloudrun_app[each.key].email}"
}

# Storage objectAdmin scoped to each app's own bucket only
resource "google_storage_bucket_iam_member" "cloudrun_app_bucket" {
  for_each = local.cloudrun_apps

  bucket = "reflective-${var.env}-${each.key}"
  role   = "roles/storage.objectAdmin"
  member = "serviceAccount:${google_service_account.cloudrun_app[each.key].email}"

  depends_on = [module.storage]
}

# ── IAM: releases-uploader service account ─────────────────────────────────────
# Used by GitHub Actions to publish new release artifacts.

resource "google_service_account" "releases_uploader" {
  project      = var.project_id
  account_id   = "releases-uploader"
  display_name = "Releases Uploader (GitHub Actions)"

  depends_on = [module.apis]
}

resource "google_storage_bucket_iam_member" "releases_uploader" {
  bucket = module.releases.bucket_name
  role   = "roles/storage.objectAdmin"
  member = "serviceAccount:${google_service_account.releases_uploader.email}"

  depends_on = [module.releases]
}
