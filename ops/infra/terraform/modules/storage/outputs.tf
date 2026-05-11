output "bucket_names" {
  value = merge(
    { for k, v in google_storage_bucket.app : k => v.name },
    { platform = google_storage_bucket.platform.name }
  )
}

output "platform_bucket" {
  value = google_storage_bucket.platform.name
}
