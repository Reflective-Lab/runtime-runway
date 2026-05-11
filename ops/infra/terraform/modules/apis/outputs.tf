output "enabled_services" {
  value       = [for svc in google_project_service.required : svc.service]
  description = "List of GCP service APIs enabled by this module"
}
