use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use redb::{Database, ReadableTable, TableDefinition, WriteTransaction};

use crate::traits::{
    Error, Result,
    event::{EventLog, EventQuery, StoredEvent, SyncableEventLog},
};

// Table: event_id → JSON string (primary store, deduplicates by event_id)
const EVENTS: TableDefinition<&str, &str> = TableDefinition::new("events");
// Table: event_id → "" (index of unsynced events)
const UNSYNCED: TableDefinition<&str, &str> = TableDefinition::new("events_unsynced");

pub fn init_tables(tx: &WriteTransaction) -> anyhow::Result<()> {
    tx.open_table(EVENTS)?;
    tx.open_table(UNSYNCED)?;
    Ok(())
}

pub struct RedbEventLog {
    db: Arc<Database>,
}

impl RedbEventLog {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    async fn query_inner(&self, q: EventQuery, unsynced_only: bool) -> Result<Vec<StoredEvent>> {
        let db = self.db.clone();

        let raw: Vec<StoredEvent> = tokio::task::spawn_blocking(move || {
            let tx = db
                .begin_read()
                .map_err(|e| Error::Database(e.to_string()))?;
            let events = tx
                .open_table(EVENTS)
                .map_err(|e| Error::Database(e.to_string()))?;

            let mut result = Vec::new();
            if unsynced_only {
                let unsynced = tx
                    .open_table(UNSYNCED)
                    .map_err(|e| Error::Database(e.to_string()))?;
                for entry in unsynced
                    .iter()
                    .map_err(|e| Error::Database(e.to_string()))?
                {
                    let (id_guard, _) = entry.map_err(|e| Error::Database(e.to_string()))?;
                    let id = id_guard.value();
                    if let Some(val) = events.get(id).map_err(|e| Error::Database(e.to_string()))? {
                        let ev: StoredEvent = serde_json::from_str(val.value())
                            .map_err(|e| Error::Serialisation(e.to_string()))?;
                        result.push(ev);
                    }
                }
            } else {
                for entry in events.iter().map_err(|e| Error::Database(e.to_string()))? {
                    let (_, val) = entry.map_err(|e| Error::Database(e.to_string()))?;
                    let ev: StoredEvent = serde_json::from_str(val.value())
                        .map_err(|e| Error::Serialisation(e.to_string()))?;
                    result.push(ev);
                }
            }
            Ok(result)
        })
        .await
        .map_err(|e| Error::Other(e.to_string()))??;

        // Filter in Rust
        let mut result: Vec<StoredEvent> = raw
            .into_iter()
            .filter(|e| {
                if let Some(ref org) = q.org_id
                    && &e.org_id != org
                {
                    return false;
                }
                if let Some(ref app) = q.app_id
                    && &e.app_id != app
                {
                    return false;
                }
                if let Some(ref et) = q.event_type
                    && &e.event_type != et
                {
                    return false;
                }
                if let Some(since) = q.since
                    && e.occurred_at <= since
                {
                    return false;
                }
                true
            })
            .collect();

        result.sort_by_key(|e| e.occurred_at);
        if let Some(n) = q.limit {
            result.truncate(n);
        }
        Ok(result)
    }
}

#[async_trait]
impl EventLog for RedbEventLog {
    async fn append(&self, event: StoredEvent) -> Result<()> {
        let db = self.db.clone();
        let json =
            serde_json::to_string(&event).map_err(|e| Error::Serialisation(e.to_string()))?;
        let id = event.event_id.clone();

        tokio::task::spawn_blocking(move || {
            let tx = db
                .begin_write()
                .map_err(|e| Error::Database(e.to_string()))?;
            {
                let mut events = tx
                    .open_table(EVENTS)
                    .map_err(|e| Error::Database(e.to_string()))?;
                // OR IGNORE equivalent: only insert if not present
                if events
                    .get(id.as_str())
                    .map_err(|e| Error::Database(e.to_string()))?
                    .is_none()
                {
                    events
                        .insert(id.as_str(), json.as_str())
                        .map_err(|e| Error::Database(e.to_string()))?;

                    let mut unsynced = tx
                        .open_table(UNSYNCED)
                        .map_err(|e| Error::Database(e.to_string()))?;
                    unsynced
                        .insert(id.as_str(), "")
                        .map_err(|e| Error::Database(e.to_string()))?;
                }
            }
            tx.commit().map_err(|e| Error::Database(e.to_string()))
        })
        .await
        .map_err(|e| Error::Other(e.to_string()))?
    }

    async fn query(&self, q: EventQuery) -> Result<Vec<StoredEvent>> {
        self.query_inner(q, false).await
    }
}

#[async_trait]
impl SyncableEventLog for RedbEventLog {
    async fn query_unsynced(&self, q: EventQuery) -> Result<Vec<StoredEvent>> {
        self.query_inner(q, true).await
    }

    async fn mark_synced(&self, event_ids: &[String]) -> Result<()> {
        let db = self.db.clone();
        let ids: Vec<String> = event_ids.to_vec();

        tokio::task::spawn_blocking(move || {
            let now = Utc::now();
            let tx = db
                .begin_write()
                .map_err(|e| Error::Database(e.to_string()))?;
            {
                let mut events = tx
                    .open_table(EVENTS)
                    .map_err(|e| Error::Database(e.to_string()))?;
                let mut unsynced = tx
                    .open_table(UNSYNCED)
                    .map_err(|e| Error::Database(e.to_string()))?;
                for id in &ids {
                    // Rewrite the event record with synced_at set.
                    // Read-then-drop the guard before the mutable insert to
                    // satisfy the borrow checker.
                    let updated_json: Option<String> = {
                        let guard = events
                            .get(id.as_str())
                            .map_err(|e| Error::Database(e.to_string()))?;
                        if let Some(g) = guard {
                            let mut ev: StoredEvent = serde_json::from_str(g.value())
                                .map_err(|e| Error::Serialisation(e.to_string()))?;
                            ev.synced_at = Some(now);
                            Some(
                                serde_json::to_string(&ev)
                                    .map_err(|e| Error::Serialisation(e.to_string()))?,
                            )
                        } else {
                            None
                        }
                        // guard dropped here — immutable borrow ends
                    };
                    if let Some(json) = updated_json {
                        events
                            .insert(id.as_str(), json.as_str())
                            .map_err(|e| Error::Database(e.to_string()))?;
                    }
                    unsynced
                        .remove(id.as_str())
                        .map_err(|e| Error::Database(e.to_string()))?;
                }
            }
            tx.commit().map_err(|e| Error::Database(e.to_string()))
        })
        .await
        .map_err(|e| Error::Other(e.to_string()))?
    }
}
