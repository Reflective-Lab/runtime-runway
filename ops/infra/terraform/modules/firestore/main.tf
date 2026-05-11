resource "google_firestore_database" "main" {
  project     = var.project_id
  name        = "(default)"
  location_id = var.region
  type        = "FIRESTORE_NATIVE"

  concurrency_mode            = "OPTIMISTIC"
  app_engine_integration_mode = "DISABLED"

  # Protect prod from accidental deletion
  delete_protection_state = var.env == "prod" ? "DELETE_PROTECTION_ENABLED" : "DELETE_PROTECTION_DISABLED"
  deletion_policy         = "DELETE"
}
