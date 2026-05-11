output "host" {
  value = google_redis_instance.main.host
}

output "port" {
  value = google_redis_instance.main.port
}

output "auth_string" {
  value     = google_redis_instance.main.auth_string
  sensitive = true
}
