mod document;
mod event;
mod object;
pub mod sync;
mod vector;

use std::{path::Path, sync::Arc};

use anyhow::Result;
use redb::Database;

use crate::{StorageKit, embedding::local::LocalEmbedder};

pub struct LocalStorageKit;

impl LocalStorageKit {
    pub async fn build(base: &Path) -> Result<StorageKit> {
        tokio::fs::create_dir_all(base).await?;

        let db_path = base.join("runway.redb");
        // redb::Database::create blocks; run on the blocking thread pool
        let db_path2 = db_path.clone();
        let db = tokio::task::spawn_blocking(move || Database::create(db_path2)).await??;
        let db = Arc::new(db);

        // Initialise tables
        {
            let write = db.begin_write()?;
            document::init_tables(&write)?;
            event::init_tables(&write)?;
            vector::init_tables(&write)?;
            write.commit()?;
        }

        let object_base = base.join("objects");
        tokio::fs::create_dir_all(&object_base).await?;

        let redb_log = Arc::new(event::RedbEventLog::new(db.clone()));
        let events: Arc<dyn crate::traits::event::EventLog> = redb_log.clone();
        let syncable: Arc<dyn crate::traits::event::SyncableEventLog> = redb_log;

        Ok(StorageKit {
            documents: Arc::new(document::RedbDocumentStore::new(db.clone())),
            vectors: Arc::new(vector::FileVectorStore::new(db.clone())),
            objects: Arc::new(object::LocalObjectStore::new(object_base)),
            events,
            embeddings: Arc::new(LocalEmbedder::new()),
            syncable_events: Some(syncable),
        })
    }
}
