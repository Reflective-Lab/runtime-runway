// Sync engine: push local ExperienceEvents to remote, pull remote document changes.
//
// Protocol:
//   1. Push: query local event_log WHERE synced_at IS NULL → append to remote EventLog → mark_synced
//   2. Pull: query remote documents WHERE updated_at > last_checkpoint → put into local DocumentStore
//   3. Conflict: remote wins — when we pull remote docs we overwrite local.
//      Local events are append-only, so there is no conflict on push.
//   4. Update checkpoint in local objects store at "sync/checkpoint.json"

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{
    StorageKit,
    traits::{Error, document::Query, event::EventQuery},
};

const CHECKPOINT_KEY: &str = "sync/checkpoint.json";

#[derive(Debug, Serialize, Deserialize)]
struct Checkpoint {
    last_sync: DateTime<Utc>,
}

/// Offline-to-cloud sync engine for Tauri desktop apps.
///
/// One `SyncEngine` per app session. Call [`SyncEngine::sync`] on connect and
/// periodically (e.g. every 30 s) to keep local and remote storage in agreement.
pub struct SyncEngine {
    local: StorageKit,
    remote: StorageKit,
    /// Firestore collections to pull on every sync cycle.
    collections: Vec<String>,
}

impl SyncEngine {
    pub fn new(local: StorageKit, remote: StorageKit, collections: Vec<String>) -> Self {
        Self {
            local,
            remote,
            collections,
        }
    }

    /// Run one full sync cycle.
    ///
    /// Returns `(events_pushed, docs_pulled)`.
    ///
    /// # Errors
    ///
    /// Push errors per-event are logged and skipped — only successfully pushed
    /// events are marked synced. Pull errors terminate the cycle and the
    /// checkpoint is **not** advanced, so the next cycle will retry.
    pub async fn sync(&self) -> Result<(usize, usize), Error> {
        let events_pushed = self.push_events().await?;
        let docs_pulled = self.pull_documents().await?;
        tracing::info!(events_pushed, docs_pulled, "sync cycle complete");
        Ok((events_pushed, docs_pulled))
    }

    // ── Push phase ────────────────────────────────────────────────────────────

    async fn push_events(&self) -> Result<usize, Error> {
        let unsynced = self
            .local
            .events
            .query(EventQuery {
                unsynced_only: true,
                ..Default::default()
            })
            .await?;

        if unsynced.is_empty() {
            return Ok(0);
        }

        let mut synced_ids: Vec<String> = Vec::with_capacity(unsynced.len());

        for event in unsynced {
            let id = event.event_id.clone();
            match self.remote.events.append(event).await {
                Ok(()) => {
                    synced_ids.push(id);
                }
                Err(err) => {
                    tracing::warn!(event_id = %id, error = %err, "skipping event — remote append failed");
                }
            }
        }

        let pushed = synced_ids.len();
        if !synced_ids.is_empty() {
            self.local.events.mark_synced(&synced_ids).await?;
        }

        Ok(pushed)
    }

    // ── Pull phase ────────────────────────────────────────────────────────────

    async fn pull_documents(&self) -> Result<usize, Error> {
        let checkpoint = self.load_checkpoint().await?;

        let mut docs_pulled: usize = 0;

        for collection in &self.collections {
            let mut q = Query::new();
            if let Some(ts) = checkpoint {
                q = q.updated_after(ts);
            }

            let docs = self.remote.documents.query(collection, q).await?;

            for doc in docs {
                self.local.documents.put(collection, doc).await?;
                docs_pulled += 1;
            }
        }

        // Only advance the checkpoint after a fully successful pull.
        self.save_checkpoint(Utc::now()).await?;

        Ok(docs_pulled)
    }

    // ── Checkpoint helpers ────────────────────────────────────────────────────

    async fn load_checkpoint(&self) -> Result<Option<DateTime<Utc>>, Error> {
        match self.local.objects.get_text(CHECKPOINT_KEY).await {
            Ok(text) => {
                let cp: Checkpoint =
                    serde_json::from_str(&text).map_err(|e| Error::Serialisation(e.to_string()))?;
                Ok(Some(cp.last_sync))
            }
            // No checkpoint yet — first sync, pull everything.
            Err(Error::NotFound(_)) => Ok(None),
            Err(err) => Err(err),
        }
    }

    async fn save_checkpoint(&self, ts: DateTime<Utc>) -> Result<(), Error> {
        let cp = Checkpoint { last_sync: ts };
        let text = serde_json::to_string(&cp).map_err(|e| Error::Serialisation(e.to_string()))?;
        self.local.objects.put_text(CHECKPOINT_KEY, &text).await
    }
}
