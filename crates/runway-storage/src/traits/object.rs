use async_trait::async_trait;
use bytes::Bytes;

use crate::traits::Result;

/// Object / blob store. Key is a slash-delimited path.
///
/// Local impl:  base_dir / key  (local FS)
/// Remote impl: gs://bucket/key (GCS)
#[async_trait]
pub trait ObjectStore: Send + Sync {
    async fn put(&self, key: &str, data: Bytes, content_type: Option<&str>) -> Result<()>;
    async fn get(&self, key: &str) -> Result<Bytes>;
    async fn delete(&self, key: &str) -> Result<()>;
    async fn exists(&self, key: &str) -> Result<bool>;

    /// List all keys with the given prefix.
    async fn list(&self, prefix: &str) -> Result<Vec<String>>;

    /// Put a UTF-8 string.
    async fn put_text(&self, key: &str, text: &str) -> Result<()> {
        self.put(
            key,
            Bytes::copy_from_slice(text.as_bytes()),
            Some("text/plain"),
        )
        .await
    }

    /// Get as UTF-8 string.
    async fn get_text(&self, key: &str) -> Result<String> {
        let bytes = self.get(key).await?;
        String::from_utf8(bytes.to_vec()).map_err(|e| crate::traits::Error::Other(e.to_string()))
    }

    /// Put JSON-serializable value.
    async fn put_json(&self, key: &str, value: &(impl serde::Serialize + Sync)) -> Result<()>
    where
        Self: Sized,
    {
        let json =
            serde_json::to_vec(value).map_err(|e| crate::traits::Error::Other(e.to_string()))?;
        self.put(key, Bytes::from(json), Some("application/json"))
            .await
    }

    /// Get and deserialize JSON.
    async fn get_json<T: serde::de::DeserializeOwned>(&self, key: &str) -> Result<T>
    where
        Self: Sized,
    {
        let bytes = self.get(key).await?;
        serde_json::from_slice(&bytes).map_err(|e| crate::traits::Error::Other(e.to_string()))
    }
}
