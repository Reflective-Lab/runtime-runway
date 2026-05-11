resource "google_project_service" "required" {
  for_each = toset([
    "firestore.googleapis.com",
    "spanner.googleapis.com",
    "storage.googleapis.com",
    "pubsub.googleapis.com",
    "bigquery.googleapis.com",
    "aiplatform.googleapis.com",
    "redis.googleapis.com",
    "secretmanager.googleapis.com",
    "run.googleapis.com",
    "cloudbuild.googleapis.com",
    "artifactregistry.googleapis.com",
    "firebase.googleapis.com",
    "identitytoolkit.googleapis.com",
    "cloudresourcemanager.googleapis.com",
    "servicenetworking.googleapis.com",
    "vpcaccess.googleapis.com",
    "iam.googleapis.com",
    "compute.googleapis.com",
  ])

  project            = var.project_id
  service            = each.key
  disable_on_destroy = false
}
