variable "project_id" { type = string }
variable "env"        { type = string }

variable "domain" {
  type        = string
  description = "Public domain for the CDN endpoint (e.g. releases.example.com)"
}

variable "multi_region_location" {
  type        = string
  default     = "EU"
  description = "GCS multi-region location for release artifacts. EU, US, or ASIA."
}
