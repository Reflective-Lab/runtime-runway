output "bucket_name" {
  value = google_storage_bucket.releases.name
}

output "cdn_ip" {
  value       = google_compute_global_forwarding_rule.releases_https.ip_address
  description = "Point your DNS A record here after apply"
}

output "cdn_url" {
  value = "https://${var.domain}"
}

output "uploader_sa_email" {
  value = google_service_account.release_uploader.email
}
