resource "google_redis_instance" "main" {
  project        = var.project_id
  name           = "reflective-${var.env}"
  tier           = var.tier
  memory_size_gb = var.memory_gb
  region         = var.region

  redis_version = "REDIS_7_0"
  display_name  = "Reflective ${title(var.env)} Cache"

  # Auth required — retrieve auth_string from outputs after apply
  auth_enabled = true

  labels = { env = var.env }
}
