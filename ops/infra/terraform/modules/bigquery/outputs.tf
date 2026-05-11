output "dataset_ids" {
  value = merge(
    { platform = google_bigquery_dataset.platform.dataset_id },
    { for k, v in google_bigquery_dataset.app : k => v.dataset_id }
  )
}

output "experience_events_table" {
  value = "${google_bigquery_dataset.platform.dataset_id}.${google_bigquery_table.experience_events.table_id}"
}

output "learning_episodes_table" {
  value = "${google_bigquery_dataset.platform.dataset_id}.${google_bigquery_table.learning_episodes.table_id}"
}
