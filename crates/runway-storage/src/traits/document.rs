use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::collections::HashMap;

use crate::traits::Result;

/// A stored document. The `data` field is the full JSON body; implementations
/// own the `id`, `created_at`, and `updated_at` fields.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Document {
    pub id: String,
    pub data: HashMap<String, Value>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Document {
    pub fn new(id: impl Into<String>, data: impl serde::Serialize) -> anyhow::Result<Self> {
        let value = serde_json::to_value(data)?;
        let map = match value {
            Value::Object(m) => m.into_iter().collect(),
            _ => anyhow::bail!("document data must be a JSON object"),
        };
        let now = Utc::now();
        Ok(Self {
            id: id.into(),
            data: map,
            created_at: now,
            updated_at: now,
        })
    }

    pub fn get<T: serde::de::DeserializeOwned>(&self, key: &str) -> Option<T> {
        self.data
            .get(key)
            .and_then(|v| serde_json::from_value(v.clone()).ok())
    }
}

/// Composable query filter. Implementations translate this to SQL WHERE clauses
/// or Firestore structured queries.
#[derive(Debug, Clone)]
pub enum Filter {
    Eq(String, Value),
    Gt(String, Value),
    Lt(String, Value),
    Gte(String, Value),
    Lte(String, Value),
    And(Vec<Filter>),
    Or(Vec<Filter>),
}

#[derive(Debug, Clone, Copy)]
pub enum Order {
    Asc,
    Desc,
}

#[derive(Debug, Clone, Default)]
pub struct Query {
    pub filter: Option<Filter>,
    pub order_by: Option<(String, Order)>,
    pub limit: Option<usize>,
    pub updated_after: Option<DateTime<Utc>>,
}

impl Query {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn filter(mut self, f: Filter) -> Self {
        self.filter = Some(f);
        self
    }

    pub fn order(mut self, field: impl Into<String>, order: Order) -> Self {
        self.order_by = Some((field.into(), order));
        self
    }

    pub fn limit(mut self, n: usize) -> Self {
        self.limit = Some(n);
        self
    }

    pub fn updated_after(mut self, ts: DateTime<Utc>) -> Self {
        self.updated_after = Some(ts);
        self
    }
}

/// Structured document store. The `collection` string maps to a Firestore
/// collection or a SQLite table row with `collection = ?`.
#[async_trait]
pub trait DocumentStore: Send + Sync {
    async fn put(&self, collection: &str, doc: Document) -> Result<()>;
    async fn get(&self, collection: &str, id: &str) -> Result<Option<Document>>;
    async fn delete(&self, collection: &str, id: &str) -> Result<()>;
    async fn query(&self, collection: &str, q: Query) -> Result<Vec<Document>>;

    /// Convenience: put a serializable value directly.
    async fn put_value(
        &self,
        collection: &str,
        id: impl Into<String> + Send,
        value: impl serde::Serialize + Send,
    ) -> Result<()>
    where
        Self: Sized,
    {
        let doc =
            Document::new(id, value).map_err(|e| crate::traits::Error::Other(e.to_string()))?;
        self.put(collection, doc).await
    }

    /// Convenience: get and deserialize.
    async fn get_as<T: serde::de::DeserializeOwned>(
        &self,
        collection: &str,
        id: &str,
    ) -> Result<Option<T>>
    where
        Self: Sized,
    {
        match self.get(collection, id).await? {
            None => Ok(None),
            Some(doc) => {
                let v = serde_json::to_value(doc.data)
                    .map_err(|e| crate::traits::Error::Other(e.to_string()))?;
                let t = serde_json::from_value(v)
                    .map_err(|e| crate::traits::Error::Other(e.to_string()))?;
                Ok(Some(t))
            }
        }
    }
}
