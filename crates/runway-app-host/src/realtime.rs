use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
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
        assert!(!s.contains("correlation_id"), "None fields should be omitted");
    }
}
