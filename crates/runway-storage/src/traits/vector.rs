use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;

use crate::traits::Result;

/// Embedding dimensionality used across the entire stack.
/// Matches Vertex AI `text-multilingual-embedding-002` output.
pub const EMBEDDING_DIMS: usize = 768;

/// A vector match returned from similarity search.
#[derive(Debug, Clone)]
pub struct Match {
    pub id: String,
    pub score: f32,
    pub metadata: HashMap<String, Value>,
    pub text: Option<String>,
}

/// Vector store: upsert embeddings and run ANN search.
///
/// `namespace` maps to a LanceDB table name or a Vertex AI index namespace.
#[async_trait]
pub trait VectorStore: Send + Sync {
    async fn upsert(
        &self,
        namespace: &str,
        id: &str,
        embedding: &[f32],
        text: Option<&str>,
        metadata: HashMap<String, Value>,
    ) -> Result<()>;

    async fn search(&self, namespace: &str, query: &[f32], top_k: usize) -> Result<Vec<Match>>;

    async fn delete(&self, namespace: &str, id: &str) -> Result<()>;

    /// Upsert many vectors in one batch. Default implementation calls `upsert` in sequence;
    /// backends that support batch writes should override this.
    async fn upsert_batch(
        &self,
        namespace: &str,
        items: Vec<(String, Vec<f32>, Option<String>, HashMap<String, Value>)>,
    ) -> Result<()> {
        for (id, emb, text, meta) in items {
            self.upsert(namespace, &id, &emb, text.as_deref(), meta)
                .await?;
        }
        Ok(())
    }
}
