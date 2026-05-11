# Per-app buckets: KB parquets, model weights, story JSON, WASM artifacts
resource "google_storage_bucket" "app" {
  for_each = toset(var.apps)

  project  = var.project_id
  name     = "reflective-${var.env}-${each.key}"
  location = var.region

  uniform_bucket_level_access = true

  versioning {
    enabled = true
  }

  lifecycle_rule {
    condition  { age = 90 }
    action { type = "SetStorageClass"; storage_class = "NEARLINE" }
  }
  lifecycle_rule {
    condition  { age = 365 }
    action { type = "SetStorageClass"; storage_class = "COLDLINE" }
  }

  labels = {
    env = var.env
    app = each.key
  }
}

# Platform bucket: model weights, Axiom WASM, Vertex Vector staging
resource "google_storage_bucket" "platform" {
  project  = var.project_id
  name     = "reflective-${var.env}-platform"
  location = var.region

  uniform_bucket_level_access = true

  versioning {
    enabled = true
  }

  labels = {
    env = var.env
    app = "platform"
  }
}
