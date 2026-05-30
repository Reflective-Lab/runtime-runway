use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub job_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actor: Option<String>,
    pub payload: serde_json::Value,
}

/// Cursor identifying a point in the event stream. Used for SSE catch-up.
#[derive(Debug, Clone, Default)]
pub struct EventCursor {
    /// Last sequence number the caller has already consumed. `subscribe_with_cursor`
    /// returns events with `sequence > last_sequence` as replay, then live events.
    pub last_sequence: Option<u64>,
    /// Optional filter: only events matching this `run_id`.
    pub run_id: Option<String>,
    /// Optional filter: only events matching this `job_id`.
    pub job_id: Option<String>,
}

/// Returned by [`EventHubHandle::subscribe_with_cursor`].
pub struct EventSubscription {
    /// Buffered events to replay before the live stream starts.
    pub replay: Vec<EventEnvelope>,
    /// Live stream from this point forward.
    pub receiver: broadcast::Receiver<EventEnvelope>,
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
            job_id: None,
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
        assert!(
            !s.contains("job_id"),
            "None job_id should be omitted from JSON"
        );
    }

    #[test]
    fn envelope_job_id_roundtrips_through_json() {
        let env = EventEnvelope {
            event_id: Uuid::nil(),
            sequence: 8,
            r#type: "job.completed".into(),
            schema_version: 1,
            occurred_at: DateTime::parse_from_rfc3339("2026-05-28T12:00:00Z")
                .unwrap()
                .with_timezone(&Utc),
            app_id: "catalyst".into(),
            run_id: Some("run-1".into()),
            job_id: Some("my-job".into()),
            correlation_id: None,
            actor: None,
            payload: serde_json::Value::Null,
        };
        let s = serde_json::to_string(&env).unwrap();
        let back: EventEnvelope = serde_json::from_str(&s).unwrap();
        assert_eq!(back.job_id.as_deref(), Some("my-job"));
        assert!(s.contains("\"job_id\":\"my-job\""));
    }

    #[test]
    fn envelope_without_job_id_field_deserializes_to_none() {
        // Simulate a legacy producer that doesn't emit job_id.
        let json = r#"{"event_id":"00000000-0000-0000-0000-000000000000","sequence":1,"type":"tick","schema_version":1,"occurred_at":"2026-05-28T12:00:00Z","app_id":"old-producer","payload":null}"#;
        let env: EventEnvelope = serde_json::from_str(json).unwrap();
        assert_eq!(env.job_id, None);
    }
}

const HUB_CAPACITY: usize = 512;

pub struct EventHub {
    sender: broadcast::Sender<EventEnvelope>,
    replay: Arc<Mutex<VecDeque<EventEnvelope>>>,
    capacity: usize,
}

#[derive(Clone)]
pub struct EventHubHandle {
    sender: broadcast::Sender<EventEnvelope>,
    replay: Arc<Mutex<VecDeque<EventEnvelope>>>,
    capacity: usize,
}

impl EventHub {
    pub fn new() -> Self {
        Self::with_capacity(HUB_CAPACITY)
    }

    pub fn with_capacity(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity.max(1));
        Self {
            sender,
            replay: Arc::new(Mutex::new(VecDeque::with_capacity(capacity))),
            capacity,
        }
    }

    pub fn handle(&self) -> EventHubHandle {
        EventHubHandle {
            sender: self.sender.clone(),
            replay: Arc::clone(&self.replay),
            capacity: self.capacity,
        }
    }
}

impl Default for EventHub {
    fn default() -> Self {
        Self::new()
    }
}

impl EventHubHandle {
    /// Publish an event to all current subscribers and append it to the replay buffer.
    ///
    /// If the buffer is at capacity the oldest event is evicted first.
    pub fn publish(&self, env: EventEnvelope) {
        {
            let mut replay = self.replay.lock().expect("replay buffer lock poisoned");
            if replay.len() >= self.capacity {
                replay.pop_front();
            }
            replay.push_back(env.clone());
        }
        let _ = self.sender.send(env);
    }

    /// Subscribe to the live broadcast channel without any replay catch-up.
    ///
    /// Backwards-compatible zero-arg form; callers that need catch-up should
    /// use [`subscribe_with_cursor`] instead.
    pub fn subscribe(&self) -> broadcast::Receiver<EventEnvelope> {
        self.sender.subscribe()
    }

    /// Subscribe with cursor-based catch-up.
    ///
    /// Returns a snapshot of replay events that satisfy the cursor filters,
    /// then a live broadcast receiver starting from the moment of the call.
    /// Replay events with `sequence <= cursor.last_sequence` are excluded.
    pub fn subscribe_with_cursor(&self, cursor: EventCursor) -> EventSubscription {
        // Subscribe to the live channel first so we don't miss events published
        // between snapshotting the replay buffer and the caller draining replay.
        let receiver = self.sender.subscribe();
        let replay = {
            let buf = self.replay.lock().expect("replay buffer lock poisoned");
            buf.iter()
                .filter(|env| matches_cursor(env, &cursor))
                .cloned()
                .collect()
        };
        EventSubscription { replay, receiver }
    }

    pub fn subscriber_count(&self) -> usize {
        self.sender.receiver_count()
    }
}

fn matches_cursor(env: &EventEnvelope, cursor: &EventCursor) -> bool {
    if let Some(last) = cursor.last_sequence
        && env.sequence <= last
    {
        return false;
    }
    if let Some(ref rid) = cursor.run_id
        && env.run_id.as_deref() != Some(rid.as_str())
    {
        return false;
    }
    if let Some(ref jid) = cursor.job_id
        && env.job_id.as_deref() != Some(jid.as_str())
    {
        return false;
    }
    true
}

#[cfg(test)]
mod hub_tests {
    use super::*;

    fn sample(seq: u64, ty: &str) -> EventEnvelope {
        sample_env(seq, ty, None, None)
    }

    fn sample_env(
        seq: u64,
        ty: &str,
        run_id: Option<&str>,
        job_id: Option<&str>,
    ) -> EventEnvelope {
        EventEnvelope {
            event_id: Uuid::new_v4(),
            sequence: seq,
            r#type: ty.into(),
            schema_version: 1,
            occurred_at: Utc::now(),
            app_id: "test".into(),
            run_id: run_id.map(String::from),
            job_id: job_id.map(String::from),
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

    // ── Replay buffer tests ──────────────────────────────────────────────

    #[tokio::test]
    async fn replay_buffer_catches_up_late_subscriber() {
        let hub = EventHub::with_capacity(8);
        let h = hub.handle();
        // Publish 3 events before subscribing.
        for seq in 1..=3 {
            h.publish(sample_env(seq, "job.started", Some("run-1"), None));
        }
        let sub = h.subscribe_with_cursor(EventCursor::default());
        assert_eq!(sub.replay.len(), 3);
        assert_eq!(sub.replay[0].sequence, 1);
    }

    #[tokio::test]
    async fn replay_buffer_filters_by_run_id() {
        let hub = EventHub::with_capacity(8);
        let h = hub.handle();
        h.publish(sample_env(1, "job.started", Some("run-1"), Some("job-A")));
        h.publish(sample_env(2, "job.started", Some("run-2"), Some("job-B")));
        h.publish(sample_env(3, "job.completed", Some("run-1"), Some("job-A")));
        let cursor = EventCursor {
            last_sequence: None,
            run_id: Some("run-1".into()),
            job_id: None,
        };
        let sub = h.subscribe_with_cursor(cursor);
        assert_eq!(sub.replay.len(), 2); // only run-1 events
        assert!(sub
            .replay
            .iter()
            .all(|e| e.run_id.as_deref() == Some("run-1")));
    }

    #[tokio::test]
    async fn replay_buffer_trims_when_full() {
        let hub = EventHub::with_capacity(2);
        let h = hub.handle();
        for seq in 1..=5 {
            h.publish(sample_env(seq, "job.started", None, None));
        }
        let sub = h.subscribe_with_cursor(EventCursor::default());
        assert_eq!(sub.replay.len(), 2);
        assert_eq!(sub.replay[0].sequence, 4); // earliest retained
        assert_eq!(sub.replay[1].sequence, 5);
    }

    #[tokio::test]
    async fn subscribe_with_cursor_after_sequence_skips_replay() {
        let hub = EventHub::with_capacity(8);
        let h = hub.handle();
        h.publish(sample_env(1, "job.started", None, None));
        h.publish(sample_env(2, "job.completed", None, None));
        let cursor = EventCursor {
            last_sequence: Some(2),
            run_id: None,
            job_id: None,
        };
        let sub = h.subscribe_with_cursor(cursor);
        assert_eq!(sub.replay.len(), 0); // no events after sequence 2
    }

    #[tokio::test]
    async fn replay_buffer_filters_by_job_id() {
        let hub = EventHub::with_capacity(8);
        let h = hub.handle();
        h.publish(sample_env(1, "job.started", Some("run-1"), Some("job-A")));
        h.publish(sample_env(2, "job.started", Some("run-2"), Some("job-B")));
        h.publish(sample_env(3, "job.completed", Some("run-1"), Some("job-A")));
        let cursor = EventCursor {
            last_sequence: None,
            run_id: None,
            job_id: Some("job-A".into()),
        };
        let sub = h.subscribe_with_cursor(cursor);
        assert_eq!(sub.replay.len(), 2); // only job-A events
        assert!(sub
            .replay
            .iter()
            .all(|e| e.job_id.as_deref() == Some("job-A")));
    }

    #[tokio::test]
    async fn subscribe_with_cursor_default_returns_all_replay() {
        let hub = EventHub::with_capacity(8);
        let h = hub.handle();
        for seq in 1..=4 {
            h.publish(sample_env(seq, "tick", None, None));
        }
        let sub = h.subscribe_with_cursor(EventCursor::default());
        assert_eq!(sub.replay.len(), 4);
    }
}
