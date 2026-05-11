output "instance_name" {
  value = google_spanner_instance.main.name
}

output "database_name" {
  value = google_spanner_database.governance.name
}

output "connection_string" {
  value = "projects/${var.project_id}/instances/${google_spanner_instance.main.name}/databases/${google_spanner_database.governance.name}"
}
