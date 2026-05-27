use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventEnvelope {
    pub event_id: Uuid,
    pub sequence: u64,
    #[serde(rename = "type")]
    pub r#type: String,
    pub schema_version: u32,
    pub occurred_at: DateTime<Utc>,
    pub app_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actor: Option<String>,
    pub payload: serde_json::Value,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelope_roundtrips_through_json() {
        let env = EventEnvelope {
            event_id: Uuid::nil(),
            sequence: 7,
            r#type: "job.started".into(),
            schema_version: 1,
            occurred_at: DateTime::parse_from_rfc3339("2026-05-28T12:00:00Z")
                .unwrap()
                .with_timezone(&Utc),
            app_id: "catalyst".into(),
            run_id: Some("run-1".into()),
            correlation_id: None,
            actor: Some("user:alice".into()),
            payload: serde_json::json!({"key": "value"}),
        };
        let s = serde_json::to_string(&env).unwrap();
        let back: EventEnvelope = serde_json::from_str(&s).unwrap();
        assert_eq!(env.event_id, back.event_id);
        assert_eq!(env.sequence, back.sequence);
        assert_eq!(env.r#type, back.r#type);
        assert!(
            !s.contains("correlation_id"),
            "None fields should be omitted"
        );
    }
}

const HUB_CAPACITY: usize = 512;

pub struct EventHub {
    sender: broadcast::Sender<EventEnvelope>,
}

#[derive(Clone)]
pub struct EventHubHandle {
    sender: broadcast::Sender<EventEnvelope>,
}

impl EventHub {
    pub fn new() -> Self {
        let (sender, _) = broadcast::channel(HUB_CAPACITY);
        Self { sender }
    }

    pub fn handle(&self) -> EventHubHandle {
        EventHubHandle {
            sender: self.sender.clone(),
        }
    }
}

impl Default for EventHub {
    fn default() -> Self {
        Self::new()
    }
}

impl EventHubHandle {
    pub fn publish(&self, env: EventEnvelope) {
        let _ = self.sender.send(env);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<EventEnvelope> {
        self.sender.subscribe()
    }

    pub fn subscriber_count(&self) -> usize {
        self.sender.receiver_count()
    }
}

#[cfg(test)]
mod hub_tests {
    use super::*;

    fn sample(seq: u64, ty: &str) -> EventEnvelope {
        EventEnvelope {
            event_id: Uuid::new_v4(),
            sequence: seq,
            r#type: ty.into(),
            schema_version: 1,
            occurred_at: Utc::now(),
            app_id: "test".into(),
            run_id: None,
            correlation_id: None,
            actor: None,
            payload: serde_json::Value::Null,
        }
    }

    #[tokio::test]
    async fn handle_delivers_to_subscriber() {
        let hub = EventHub::new();
        let h = hub.handle();
        let mut rx = h.subscribe();

        h.publish(sample(1, "foo"));
        let got = rx.recv().await.unwrap();
        assert_eq!(got.sequence, 1);
    }

    #[tokio::test]
    async fn publish_without_subscribers_is_silent() {
        let hub = EventHub::new();
        let h = hub.handle();
        h.publish(sample(1, "foo"));
        assert_eq!(h.subscriber_count(), 0);
    }
}
