variable "project_id" { type = string }
variable "region"     { type = string }
variable "env"        { type = string }

variable "tier" {
  type    = string
  default = "BASIC"
}

variable "memory_gb" {
  type    = number
  default = 1
}
