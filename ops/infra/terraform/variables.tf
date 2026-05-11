variable "project_id" {
  type        = string
  description = "GCP project ID"
}

variable "region" {
  type        = string
  default     = "europe-west1"
  description = "Primary GCP region"
}

variable "env" {
  type        = string
  description = "Deployment environment (dev, staging, prod)"
  validation {
    condition     = contains(["dev", "staging", "prod"], var.env)
    error_message = "env must be dev, staging, or prod"
  }
}

variable "apps" {
  type        = list(string)
  default     = ["folio", "inkling", "wolfgang", "scout", "quorum", "vouch"]
  description = "App names used to namespace GCS buckets, BigQuery datasets, and Pub/Sub topics"
}

variable "spanner_config" {
  type        = string
  default     = "regional-europe-west1"
  description = "Spanner instance config. Use nam-eur-asia1 for multi-region prod."
}

variable "redis_tier" {
  type        = string
  default     = "BASIC"
  description = "Memorystore tier. STANDARD_HA for prod."
}

variable "redis_memory_gb" {
  type        = number
  default     = 1
  description = "Memorystore memory size in GB"
}

variable "releases_domain" {
  type        = string
  description = "Public domain for the app releases CDN endpoint (e.g. releases.yourapp.com)"
}

variable "releases_location" {
  type        = string
  default     = "EU"
  description = "GCS multi-region location for release artifacts (EU, US, ASIA)"
}
