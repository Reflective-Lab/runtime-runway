output "firestore_database" {
  value = module.firestore.database_name
}

output "spanner_instance" {
  value = module.spanner.instance_name
}

output "spanner_database" {
  value = module.spanner.database_name
}

output "storage_buckets" {
  value = module.storage.bucket_names
}

output "pubsub_topics" {
  value = module.pubsub.topic_ids
}

output "bigquery_datasets" {
  value = module.bigquery.dataset_ids
}

output "experience_events_table" {
  value = module.bigquery.experience_events_table
}

output "learning_episodes_table" {
  value = module.bigquery.learning_episodes_table
}

output "vertex_vector_index" {
  value = module.vertex_vector.index_id
}

output "vertex_vector_endpoint" {
  value = module.vertex_vector.endpoint_id
}

output "redis_host" {
  value     = module.memorystore.host
  sensitive = true
}

output "redis_port" {
  value = module.memorystore.port
}

output "releases_bucket" {
  value = module.releases.bucket_name
}

output "releases_cdn_ip" {
  value       = module.releases.cdn_ip
  description = "Point your DNS A record for releases_domain here"
}

output "releases_cdn_url" {
  value = module.releases.cdn_url
}

output "releases_uploader_sa" {
  value = module.releases.uploader_sa_email
}

output "cloudrun_service_accounts" {
  value       = { for app, sa in google_service_account.cloudrun_app : app => sa.email }
  description = "Cloud Run service account emails, keyed by app name"
}

output "releases_uploader_sa_root" {
  value       = google_service_account.releases_uploader.email
  description = "releases-uploader SA email for use in GitHub Actions OIDC config"
}
