use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use redb::{Database, TableDefinition, WriteTransaction};
use serde_json::Value;

use crate::traits::{
    Error, Result,
    vector::{EMBEDDING_DIMS, Match, VectorStore},
};

const VECTORS: TableDefinition<(&str, &str), &str> = TableDefinition::new("vectors");

pub fn init_tables(tx: &WriteTransaction) -> anyhow::Result<()> {
    tx.open_table(VECTORS)?;
    Ok(())
}

pub struct FileVectorStore {
    db: Arc<Database>,
}

impl FileVectorStore {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct VectorEntry {
    id: String,
    embedding: Vec<f32>,
    text: Option<String>,
    metadata: HashMap<String, Value>,
}

#[async_trait]
impl VectorStore for FileVectorStore {
    async fn upsert(
        &self,
        namespace: &str,
        id: &str,
        embedding: &[f32],
        text: Option<&str>,
        metadata: HashMap<String, Value>,
    ) -> Result<()> {
        if embedding.len() != EMBEDDING_DIMS {
            return Err(Error::Other(format!(
                "embedding has {} dims, expected {EMBEDDING_DIMS}",
                embedding.len()
            )));
        }

        let db = self.db.clone();
        let namespace = namespace.to_string();
        let id = id.to_string();
        let entry = VectorEntry {
            id: id.clone(),
            embedding: embedding.to_vec(),
            text: text.map(|s| s.to_string()),
            metadata,
        };
        let json =
            serde_json::to_string(&entry).map_err(|e| Error::Serialisation(e.to_string()))?;

        tokio::task::spawn_blocking(move || {
            let tx = db
                .begin_write()
                .map_err(|e| Error::Database(e.to_string()))?;
            {
                let mut table = tx
                    .open_table(VECTORS)
                    .map_err(|e| Error::Database(e.to_string()))?;
                table
                    .insert((namespace.as_str(), id.as_str()), json.as_str())
                    .map_err(|e| Error::Database(e.to_string()))?;
            }
            tx.commit().map_err(|e| Error::Database(e.to_string()))
        })
        .await
        .map_err(|e| Error::Other(e.to_string()))?
    }

    async fn search(&self, namespace: &str, query: &[f32], top_k: usize) -> Result<Vec<Match>> {
        let db = self.db.clone();
        let namespace = namespace.to_string();
        let query = query.to_vec();

        let matches: Vec<Match> = tokio::task::spawn_blocking(move || {
            let tx = db
                .begin_read()
                .map_err(|e| Error::Database(e.to_string()))?;
            let table = tx
                .open_table(VECTORS)
                .map_err(|e| Error::Database(e.to_string()))?;

            let end_ns = format!("{}\x7f", namespace);
            let start: (&str, &str) = (namespace.as_str(), "");
            let end: (&str, &str) = (end_ns.as_str(), "\x7f");

            let mut scored: Vec<(f32, VectorEntry)> = Vec::new();
            for entry in table
                .range(start..=end)
                .map_err(|e| Error::Database(e.to_string()))?
            {
                let (_, val) = entry.map_err(|e| Error::Database(e.to_string()))?;
                let ve: VectorEntry = serde_json::from_str(val.value())
                    .map_err(|e| Error::Serialisation(e.to_string()))?;
                let score = cosine_similarity(&query, &ve.embedding);
                scored.push((score, ve));
            }

            scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
            scored.truncate(top_k);

            Ok(scored
                .into_iter()
                .map(|(score, ve)| Match {
                    id: ve.id,
                    score,
                    metadata: ve.metadata,
                    text: ve.text,
                })
                .collect())
        })
        .await
        .map_err(|e| Error::Other(e.to_string()))??;

        Ok(matches)
    }

    async fn delete(&self, namespace: &str, id: &str) -> Result<()> {
        let db = self.db.clone();
        let namespace = namespace.to_string();
        let id = id.to_string();

        tokio::task::spawn_blocking(move || {
            let tx = db
                .begin_write()
                .map_err(|e| Error::Database(e.to_string()))?;
            {
                let mut table = tx
                    .open_table(VECTORS)
                    .map_err(|e| Error::Database(e.to_string()))?;
                table
                    .remove((namespace.as_str(), id.as_str()))
                    .map_err(|e| Error::Database(e.to_string()))?;
            }
            tx.commit().map_err(|e| Error::Database(e.to_string()))
        })
        .await
        .map_err(|e| Error::Other(e.to_string()))?
    }
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    let len = a.len().min(b.len());
    let dot: f32 = a[..len].iter().zip(&b[..len]).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a[..len].iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b[..len].iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        0.0
    } else {
        dot / (norm_a * norm_b)
    }
}
