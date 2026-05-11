locals {
  # One ingestion topic per app + platform ExperienceEvents ledger
  topic_names = concat(
    [for app in var.apps : "${var.env}.${app}.ingestion"],
    ["${var.env}.converge.experience-events"]
  )
}

resource "google_pubsub_topic" "main" {
  for_each = toset(local.topic_names)

  project = var.project_id
  name    = each.key

  # 7-day retention — enough for replay after a processor outage
  message_retention_duration = "604800s"

  labels = { env = var.env }
}

resource "google_pubsub_topic" "dead_letter" {
  for_each = toset(local.topic_names)

  project = var.project_id
  name    = "${each.key}.dead-letter"

  labels = { env = var.env }
}

resource "google_pubsub_subscription" "processor" {
  for_each = toset(local.topic_names)

  project = var.project_id
  name    = "${each.key}.processor"
  topic   = google_pubsub_topic.main[each.key].name

  ack_deadline_seconds       = 60
  message_retention_duration = "604800s"

  dead_letter_policy {
    dead_letter_topic     = google_pubsub_topic.dead_letter[each.key].id
    max_delivery_attempts = 5
  }

  retry_policy {
    minimum_backoff = "10s"
    maximum_backoff = "600s"
  }

  labels = { env = var.env }
}
