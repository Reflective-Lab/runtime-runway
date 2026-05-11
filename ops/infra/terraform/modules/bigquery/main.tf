# Platform dataset: ExperienceEvents ledger + LearningEpisodes (PriorCalibration feed)
resource "google_bigquery_dataset" "platform" {
  project    = var.project_id
  dataset_id = "reflective_${var.env}_platform"
  location   = var.region

  delete_contents_on_destroy = var.env != "prod"

  labels = {
    env = var.env
    app = "platform"
  }
}

# Append-only audit ledger for all ExperienceEvents across every app
resource "google_bigquery_table" "experience_events" {
  project    = var.project_id
  dataset_id = google_bigquery_dataset.platform.dataset_id
  table_id   = "experience_events"

  deletion_protection = var.env == "prod"

  time_partitioning {
    type  = "DAY"
    field = "occurred_at"
  }

  clustering = ["org_id", "app_id", "event_type"]

  schema = jsonencode([
    { name = "org_id",      type = "STRING",    mode = "REQUIRED" },
    { name = "app_id",      type = "STRING",    mode = "REQUIRED" },
    { name = "event_id",    type = "STRING",    mode = "REQUIRED" },
    { name = "event_type",  type = "STRING",    mode = "REQUIRED" },
    { name = "context_id",  type = "STRING",    mode = "NULLABLE" },
    { name = "fact_id",     type = "STRING",    mode = "NULLABLE" },
    { name = "payload",     type = "JSON",      mode = "NULLABLE" },
    { name = "occurred_at", type = "TIMESTAMP", mode = "REQUIRED" },
  ])
}

# Formation success rates and hypothesis accuracy — feeds PriorCalibration in Organism
resource "google_bigquery_table" "learning_episodes" {
  project    = var.project_id
  dataset_id = google_bigquery_dataset.platform.dataset_id
  table_id   = "learning_episodes"

  deletion_protection = var.env == "prod"

  time_partitioning {
    type  = "DAY"
    field = "recorded_at"
  }

  clustering = ["org_id", "app_id", "formation_id"]

  schema = jsonencode([
    { name = "org_id",         type = "STRING",    mode = "REQUIRED" },
    { name = "app_id",         type = "STRING",    mode = "REQUIRED" },
    { name = "episode_id",     type = "STRING",    mode = "REQUIRED" },
    { name = "formation_id",   type = "STRING",    mode = "NULLABLE" },
    { name = "intent_type",    type = "STRING",    mode = "NULLABLE" },
    { name = "outcome",        type = "STRING",    mode = "NULLABLE" },
    { name = "confidence",     type = "FLOAT64",   mode = "NULLABLE" },
    { name = "actual_result",  type = "JSON",      mode = "NULLABLE" },
    { name = "recorded_at",    type = "TIMESTAMP", mode = "REQUIRED" },
  ])
}

# Per-app analytics datasets (edition performance, signal aggregates, etc.)
resource "google_bigquery_dataset" "app" {
  for_each = toset(var.apps)

  project    = var.project_id
  dataset_id = "reflective_${var.env}_${each.key}"
  location   = var.region

  delete_contents_on_destroy = var.env != "prod"

  labels = {
    env = var.env
    app = each.key
  }
}
