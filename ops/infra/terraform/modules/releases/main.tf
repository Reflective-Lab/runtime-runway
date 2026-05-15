# Release artifact storage with Cloud CDN.
# Hosts downloadable builds for all marquee apps (Folio, Scout, Quorum, Wolfgang, Vouch, ...).
#
# Bucket layout: {app}/{version}/{platform}-{arch}/{filename}
#   e.g. folio/v1.2.0/macos-aarch64/folio.dmg
#        folio/v1.2.0/windows-x86_64/folio-setup.exe
#        folio/v1.2.0/linux-x86_64/folio.AppImage
#        scout/v2.0.0/macos-aarch64/scout.dmg
#        wolfgang/v1.0.0/linux-x86_64/wolfgang.deb
#
# latest.json per app: {app}/latest.json
# CDN URL pattern:     https://{domain}/{app}/{version}/{platform}-{arch}/{filename}
#
# Access: public read via CDN for released versions.
# Upload: dedicated SA with Storage Object Creator on this bucket only.

resource "google_storage_bucket" "releases" {
  project  = var.project_id
  name     = "reflective-${var.env}-releases"

  # Multi-region for geo-redundant CDN origin (EU, US, or ASIA)
  location     = var.multi_region_location
  storage_class = "STANDARD"

  uniform_bucket_level_access = true

  versioning {
    enabled = true
  }

  # Keep the 10 most recent versions of each object; archive older ones
  lifecycle_rule {
    condition {
      num_newer_versions = 10
      with_state        = "ARCHIVED"
    }
    action { type = "Delete" }
  }

  # Move artifacts older than 2 years to Coldline
  lifecycle_rule {
    condition { age = 730 }
    action {
      type          = "SetStorageClass"
      storage_class = "COLDLINE"
    }
  }

  # CORS for browser-based download pages
  cors {
    origin          = ["*"]
    method          = ["GET", "HEAD"]
    response_header = ["Content-Type", "Content-Disposition"]
    max_age_seconds = 3600
  }

  labels = {
    env  = var.env
    role = "releases"
  }
}

# Public read — allUsers can download released artifacts via CDN
resource "google_storage_bucket_iam_member" "public_read" {
  bucket = google_storage_bucket.releases.name
  role   = "roles/storage.objectViewer"
  member = "allUsers"
}

# Dedicated uploader SA — CI/CD uses this to publish new releases
resource "google_service_account" "release_uploader" {
  project      = var.project_id
  account_id   = "release-uploader-${var.env}"
  display_name = "Release Uploader ${title(var.env)}"
}

resource "google_storage_bucket_iam_member" "uploader" {
  bucket = google_storage_bucket.releases.name
  role   = "roles/storage.objectCreator"
  member = "serviceAccount:${google_service_account.release_uploader.email}"
}

# ── Cloud CDN via Backend Bucket ────────────────────────────────────────

resource "google_compute_backend_bucket" "releases" {
  project     = var.project_id
  name        = "reflective-${var.env}-releases-cdn"
  description = "CDN-backed GCS bucket for app release artifacts"
  bucket_name = google_storage_bucket.releases.name
  enable_cdn  = true

  cdn_policy {
    cache_mode        = "CACHE_ALL_STATIC"
    default_ttl       = 3600
    max_ttl           = 86400
    negative_caching  = true
    serve_while_stale = 86400
  }
}

resource "google_compute_url_map" "releases" {
  project         = var.project_id
  name            = "reflective-${var.env}-releases"
  default_service = google_compute_backend_bucket.releases.id
}

# Managed SSL cert — Google provisions and renews automatically
resource "google_compute_managed_ssl_certificate" "releases" {
  project = var.project_id
  name    = "reflective-${var.env}-releases-cert"

  managed {
    domains = [var.domain]
  }
}

resource "google_compute_target_https_proxy" "releases" {
  project          = var.project_id
  name             = "reflective-${var.env}-releases-https"
  url_map          = google_compute_url_map.releases.id
  ssl_certificates = [google_compute_managed_ssl_certificate.releases.id]
}

resource "google_compute_global_forwarding_rule" "releases_https" {
  project    = var.project_id
  name       = "reflective-${var.env}-releases-https"
  target     = google_compute_target_https_proxy.releases.id
  port_range = "443"
  ip_protocol = "TCP"
}

# HTTP → HTTPS redirect
resource "google_compute_url_map" "releases_redirect" {
  project = var.project_id
  name    = "reflective-${var.env}-releases-redirect"

  default_url_redirect {
    https_redirect         = true
    redirect_response_code = "MOVED_PERMANENTLY_DEFAULT"
    strip_query            = false
  }
}

resource "google_compute_target_http_proxy" "releases_redirect" {
  project = var.project_id
  name    = "reflective-${var.env}-releases-redirect"
  url_map = google_compute_url_map.releases_redirect.id
}

resource "google_compute_global_forwarding_rule" "releases_http" {
  project     = var.project_id
  name        = "reflective-${var.env}-releases-http"
  target      = google_compute_target_http_proxy.releases_redirect.id
  port_range  = "80"
  ip_protocol = "TCP"
}
