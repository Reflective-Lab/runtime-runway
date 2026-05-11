use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::traits::Result;

/// An ExperienceEvent as stored in the log. Append-only — never updated or deleted.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StoredEvent {
    pub event_id: String,
    pub org_id: String,
    pub app_id: String,
    pub event_type: String,
    pub context_id: Option<String>,
    pub fact_id: Option<String>,
    pub payload: Value,
    pub occurred_at: DateTime<Utc>,
    /// Populated only in local store — tracks whether this event has been synced to remote.
    #[serde(default)]
    pub synced_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Default)]
pub struct EventQuery {
    pub org_id: Option<String>,
    pub app_id: Option<String>,
    pub event_type: Option<String>,
    pub since: Option<DateTime<Utc>>,
    pub limit: Option<usize>,
    /// Only return events not yet synced (local use for sync engine).
    pub unsynced_only: bool,
}

/// Append-only event ledger. The ExperienceStore from the Converge architecture.
///
/// Local impl:  SQLite WAL (survives restarts, feeds sync engine)
/// Remote impl: Firestore events subcollection + BigQuery streaming insert
#[async_trait]
pub trait EventLog: Send + Sync {
    async fn append(&self, event: StoredEvent) -> Result<()>;
    async fn query(&self, q: EventQuery) -> Result<Vec<StoredEvent>>;

    /// Mark events as synced. Only meaningful in local implementations.
    async fn mark_synced(&self, event_ids: &[String]) -> Result<()>;
}
