terraform {
  required_providers {
    google-beta = {
      source  = "hashicorp/google-beta"
      version = "~> 5.0"
    }
  }
}

# Expert KB embeddings — text-embedding-004 output is 768 dims.
# Source parquets live in GCS; this index is regeneratable from them.
# Note: index creation and deployment each take 30–90 minutes on first apply.
resource "google_vertex_ai_index" "kb_embeddings" {
  provider     = google-beta
  project      = var.project_id
  region       = var.region
  display_name = "reflective-${var.env}-kb-embeddings"

  metadata {
    # Staging bucket must exist before apply (created by storage module)
    contents_delta_uri = "gs://reflective-${var.env}-platform/vector-index-staging/"
    config {
      dimensions                  = 768
      approximate_neighbors_count = 150
      distance_measure_type       = "DOT_PRODUCT_DISTANCE"
      algorithm_config {
        tree_ah_config {
          leaf_node_embedding_count    = 500
          leaf_nodes_to_search_percent = 7
        }
      }
    }
  }

  # STREAM_UPDATE enables incremental upserts as new KB parquets are processed
  index_update_method = "STREAM_UPDATE"

  labels = { env = var.env }
}

resource "google_vertex_ai_index_endpoint" "kb" {
  provider     = google-beta
  project      = var.project_id
  region       = var.region
  display_name = "reflective-${var.env}-kb-endpoint"

  # Public endpoint — no VPC peering required at current scale
  public_endpoint_enabled = true

  labels = { env = var.env }
}

resource "google_vertex_ai_index_endpoint_deployed_index" "kb" {
  provider          = google-beta
  index_endpoint    = google_vertex_ai_index_endpoint.kb.id
  index             = google_vertex_ai_index.kb_embeddings.id
  deployed_index_id = "kb_${var.env}"

  automatic_resources {
    min_replica_count = var.env == "prod" ? 2 : 1
    max_replica_count = var.env == "prod" ? 10 : 2
  }
}
