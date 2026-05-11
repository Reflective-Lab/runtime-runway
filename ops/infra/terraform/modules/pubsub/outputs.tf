output "topic_ids" {
  value = { for k, v in google_pubsub_topic.main : k => v.id }
}

output "subscription_ids" {
  value = { for k, v in google_pubsub_subscription.processor : k => v.id }
}

output "dead_letter_topic_ids" {
  value = { for k, v in google_pubsub_topic.dead_letter : k => v.id }
}
