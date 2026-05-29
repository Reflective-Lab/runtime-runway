use std::path::PathBuf;

use async_trait::async_trait;
use bytes::Bytes;

use crate::traits::{Error, Result, object::ObjectStore};

/// Local filesystem object store. Paths are relative to `base_dir`.
/// Mirrors the GCS key convention: `{org_id}/{app_id}/{name}`.
pub struct LocalObjectStore {
    base: PathBuf,
}

impl LocalObjectStore {
    pub fn new(base: PathBuf) -> Self {
        Self { base }
    }

    fn resolve(&self, key: &str) -> PathBuf {
        // Strip leading slashes to keep paths relative to base
        self.base.join(key.trim_start_matches('/'))
    }
}

#[async_trait]
impl ObjectStore for LocalObjectStore {
    async fn put(&self, key: &str, data: Bytes, _content_type: Option<&str>) -> Result<()> {
        let path = self.resolve(key);
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| Error::Other(e.to_string()))?;
        }
        // Write to a temp file then rename for atomicity (no partial reads)
        let tmp = path.with_extension("tmp");
        tokio::fs::write(&tmp, &data)
            .await
            .map_err(|e| Error::Other(e.to_string()))?;
        tokio::fs::rename(&tmp, &path)
            .await
            .map_err(|e| Error::Other(e.to_string()))?;
        Ok(())
    }

    async fn get(&self, key: &str) -> Result<Bytes> {
        let path = self.resolve(key);
        tokio::fs::read(&path).await.map(Bytes::from).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                Error::NotFound(key.to_string())
            } else {
                Error::Other(e.to_string())
            }
        })
    }

    async fn delete(&self, key: &str) -> Result<()> {
        let path = self.resolve(key);
        match tokio::fs::remove_file(&path).await {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(Error::Other(e.to_string())),
        }
    }

    async fn exists(&self, key: &str) -> Result<bool> {
        Ok(self.resolve(key).exists())
    }

    async fn list(&self, prefix: &str) -> Result<Vec<String>> {
        let dir = self.resolve(prefix);
        if !dir.exists() {
            return Ok(vec![]);
        }
        let mut result = Vec::new();
        let mut stack = vec![dir.clone()];
        while let Some(path) = stack.pop() {
            let mut entries = tokio::fs::read_dir(&path)
                .await
                .map_err(|e| Error::Other(e.to_string()))?;
            while let Some(entry) = entries
                .next_entry()
                .await
                .map_err(|e| Error::Other(e.to_string()))?
            {
                let ft = entry
                    .file_type()
                    .await
                    .map_err(|e| Error::Other(e.to_string()))?;
                if ft.is_dir() {
                    stack.push(entry.path());
                } else {
                    // Return path relative to base
                    let rel = entry
                        .path()
                        .strip_prefix(&self.base)
                        .map(|p| p.to_string_lossy().replace('\\', "/"))
                        .unwrap_or_default();
                    result.push(rel);
                }
            }
        }
        result.sort();
        Ok(result)
    }
}
