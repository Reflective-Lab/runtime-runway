output "index_id" {
  value = google_vertex_ai_index.kb_embeddings.id
}

output "endpoint_id" {
  value = google_vertex_ai_index_endpoint.kb.id
}

output "deployed_index_id" {
  value = google_vertex_ai_index_endpoint_deployed_index.kb.deployed_index_id
}

output "public_endpoint_domain" {
  value = google_vertex_ai_index_endpoint.kb.public_endpoint_domain_name
}
