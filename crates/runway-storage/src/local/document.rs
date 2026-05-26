use std::sync::Arc;

use async_trait::async_trait;
use redb::{Database, ReadableTable, TableDefinition, WriteTransaction};

use crate::traits::{
    Error, Result,
    document::{Document, DocumentStore, Query},
};

// Table: (collection, id) → JSON string
const DOCS: TableDefinition<(&str, &str), &str> = TableDefinition::new("documents");

pub fn init_tables(tx: &WriteTransaction) -> anyhow::Result<()> {
    tx.open_table(DOCS)?;
    Ok(())
}

pub struct RedbDocumentStore {
    db: Arc<Database>,
}

impl RedbDocumentStore {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl DocumentStore for RedbDocumentStore {
    async fn put(&self, collection: &str, doc: Document) -> Result<()> {
        let db = self.db.clone();
        let collection = collection.to_string();
        let id = doc.id.clone();
        let mut doc = doc;

        tokio::task::spawn_blocking(move || {
            let tx = db
                .begin_write()
                .map_err(|e| Error::Database(e.to_string()))?;
            {
                let mut table = tx
                    .open_table(DOCS)
                    .map_err(|e| Error::Database(e.to_string()))?;
                // Preserve created_at if a document with this id already exists.
                if let Some(existing) = table
                    .get((collection.as_str(), id.as_str()))
                    .map_err(|e| Error::Database(e.to_string()))?
                {
                    let prior: Document = serde_json::from_str(existing.value())
                        .map_err(|e| Error::Serialisation(e.to_string()))?;
                    doc.created_at = prior.created_at;
                }
                doc.updated_at = chrono::Utc::now();
                let json =
                    serde_json::to_string(&doc).map_err(|e| Error::Serialisation(e.to_string()))?;
                table
                    .insert((collection.as_str(), id.as_str()), json.as_str())
                    .map_err(|e| Error::Database(e.to_string()))?;
            }
            tx.commit().map_err(|e| Error::Database(e.to_string()))
        })
        .await
        .map_err(|e| Error::Other(e.to_string()))?
    }

    async fn get(&self, collection: &str, id: &str) -> Result<Option<Document>> {
        let db = self.db.clone();
        let collection = collection.to_string();
        let id = id.to_string();

        tokio::task::spawn_blocking(move || {
            let tx = db
                .begin_read()
                .map_err(|e| Error::Database(e.to_string()))?;
            let table = tx
                .open_table(DOCS)
                .map_err(|e| Error::Database(e.to_string()))?;
            match table
                .get((collection.as_str(), id.as_str()))
                .map_err(|e| Error::Database(e.to_string()))?
            {
                None => Ok(None),
                Some(guard) => {
                    let doc: Document = serde_json::from_str(guard.value())
                        .map_err(|e| Error::Serialisation(e.to_string()))?;
                    Ok(Some(doc))
                }
            }
        })
        .await
        .map_err(|e| Error::Other(e.to_string()))?
    }

    async fn delete(&self, collection: &str, id: &str) -> Result<()> {
        let db = self.db.clone();
        let collection = collection.to_string();
        let id = id.to_string();

        tokio::task::spawn_blocking(move || {
            let tx = db
                .begin_write()
                .map_err(|e| Error::Database(e.to_string()))?;
            {
                let mut table = tx
                    .open_table(DOCS)
                    .map_err(|e| Error::Database(e.to_string()))?;
                table
                    .remove((collection.as_str(), id.as_str()))
                    .map_err(|e| Error::Database(e.to_string()))?;
            }
            tx.commit().map_err(|e| Error::Database(e.to_string()))
        })
        .await
        .map_err(|e| Error::Other(e.to_string()))?
    }

    async fn query(&self, collection: &str, q: Query) -> Result<Vec<Document>> {
        let db = self.db.clone();
        let collection = collection.to_string();

        let all: Vec<Document> = tokio::task::spawn_blocking(move || {
            let tx = db
                .begin_read()
                .map_err(|e| Error::Database(e.to_string()))?;
            let table = tx
                .open_table(DOCS)
                .map_err(|e| Error::Database(e.to_string()))?;

            // Scan the range for this collection prefix
            let end_col = format!("{}\x7f", collection);
            let start: (&str, &str) = (collection.as_str(), "");
            let end: (&str, &str) = (end_col.as_str(), "\x7f");

            let mut docs = Vec::new();
            for entry in table
                .range(start..=end)
                .map_err(|e| Error::Database(e.to_string()))?
            {
                let (_, val) = entry.map_err(|e| Error::Database(e.to_string()))?;
                let doc: Document = serde_json::from_str(val.value())
                    .map_err(|e| Error::Serialisation(e.to_string()))?;
                docs.push(doc);
            }
            Ok(docs)
        })
        .await
        .map_err(|e| Error::Other(e.to_string()))??;

        // Apply filters in Rust (offline scale is small enough for this)
        let mut result: Vec<Document> = all
            .into_iter()
            .filter(|doc| {
                if let Some(ts) = q.updated_after
                    && doc.updated_at <= ts
                {
                    return false;
                }
                if let Some(crate::traits::document::Filter::Eq(ref field, ref val)) = q.filter
                    && doc.data.get(field) != Some(val)
                {
                    return false;
                }
                true
            })
            .collect();

        // Sort by updated_at desc by default
        result.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));

        if let Some(n) = q.limit {
            result.truncate(n);
        }

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use redb::Database;
    use serde_json::json;
    use tempfile::tempdir;

    use super::{RedbDocumentStore, init_tables};
    use crate::traits::document::{Document, DocumentStore};

    async fn build_store() -> RedbDocumentStore {
        let dir = tempdir().unwrap();
        let db = Arc::new(Database::create(dir.path().join("test.redb")).unwrap());
        {
            let tx = db.begin_write().unwrap();
            init_tables(&tx).unwrap();
            tx.commit().unwrap();
        }
        // Keep the temp directory alive for the duration of the test.
        std::mem::forget(dir);
        RedbDocumentStore::new(db)
    }

    #[tokio::test]
    async fn put_preserves_created_at_on_overwrite() {
        let store = build_store().await;
        let doc = Document::new("k1", json!({"v": 1})).unwrap();
        let original_created = doc.created_at;
        store.put("coll", doc).await.unwrap();

        tokio::time::sleep(std::time::Duration::from_millis(5)).await;

        let doc2 = Document::new("k1", json!({"v": 2})).unwrap();
        store.put("coll", doc2).await.unwrap();

        let got = store.get("coll", "k1").await.unwrap().expect("doc present");
        assert_eq!(
            got.created_at, original_created,
            "created_at must be preserved"
        );
        assert!(got.updated_at > original_created, "updated_at must advance");
    }
}
