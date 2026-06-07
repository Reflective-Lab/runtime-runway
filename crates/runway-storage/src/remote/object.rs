use async_trait::async_trait;
use bytes::Bytes;

use crate::{
    remote::{BearerAuthExt, GcpToken},
    traits::{Error, Result, object::ObjectStore},
};

pub struct GcsObjectStore {
    bucket: String,
    token: GcpToken,
    client: reqwest::Client,
}

impl GcsObjectStore {
    pub fn new(bucket: String, token: GcpToken) -> Self {
        Self {
            bucket,
            token,
            // RP-HERMETIC-UNIT (Reflective QUALITY_BACKLOG.md →
            // QF-2026-06-02-05): production constructor for GCS object
            // store; tests use GCS emulators at the test harness
            // level, not DI through this struct.
            #[allow(clippy::disallowed_methods)]
            client: reqwest::Client::new(),
        }
    }

    fn upload_url(&self, key: &str) -> String {
        format!(
            "{}/upload/storage/v1/b/{}/o?uploadType=media&name={}",
            crate::endpoints::gcs_base(),
            self.bucket,
            urlencoding::encode(key)
        )
    }

    fn download_url(&self, key: &str) -> String {
        format!(
            "{}/storage/v1/b/{}/o/{}?alt=media",
            crate::endpoints::gcs_base(),
            self.bucket,
            urlencoding::encode(key)
        )
    }

    fn meta_url(&self, key: &str) -> String {
        format!(
            "{}/storage/v1/b/{}/o/{}",
            crate::endpoints::gcs_base(),
            self.bucket,
            urlencoding::encode(key)
        )
    }

    async fn bearer(&self) -> Result<String> {
        self.token
            .get()
            .await
            .map_err(|e| Error::Network(e.to_string()))
    }
}

#[async_trait]
impl ObjectStore for GcsObjectStore {
    async fn put(&self, key: &str, data: Bytes, content_type: Option<&str>) -> Result<()> {
        let ct = content_type.unwrap_or("application/octet-stream");
        self.client
            .post(self.upload_url(key))
            .bearer_auth_if_set(&self.bearer().await?)
            .header("Content-Type", ct)
            .body(data)
            .send()
            .await
            .map_err(|e| Error::Network(e.to_string()))?
            .error_for_status()
            .map_err(|e| Error::Network(e.to_string()))?;
        Ok(())
    }

    async fn get(&self, key: &str) -> Result<Bytes> {
        let resp = self
            .client
            .get(self.download_url(key))
            .bearer_auth_if_set(&self.bearer().await?)
            .send()
            .await
            .map_err(|e| Error::Network(e.to_string()))?;

        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(Error::NotFound(key.to_string()));
        }
        let bytes = resp
            .error_for_status()
            .map_err(|e| Error::Network(e.to_string()))?
            .bytes()
            .await
            .map_err(|e| Error::Network(e.to_string()))?;
        Ok(bytes)
    }

    async fn delete(&self, key: &str) -> Result<()> {
        let resp = self
            .client
            .delete(self.meta_url(key))
            .bearer_auth_if_set(&self.bearer().await?)
            .send()
            .await
            .map_err(|e| Error::Network(e.to_string()))?;

        // Idempotent: deleting a missing object is success.
        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(());
        }
        resp.error_for_status()
            .map_err(|e| Error::Network(e.to_string()))?;
        Ok(())
    }

    async fn exists(&self, key: &str) -> Result<bool> {
        let resp = self
            .client
            .head(self.meta_url(key))
            .bearer_auth_if_set(&self.bearer().await?)
            .send()
            .await
            .map_err(|e| Error::Network(e.to_string()))?;
        Ok(resp.status().is_success())
    }

    async fn list(&self, prefix: &str) -> Result<Vec<String>> {
        let url = format!(
            "{}/storage/v1/b/{}/o?prefix={}",
            crate::endpoints::gcs_base(),
            self.bucket,
            urlencoding::encode(prefix)
        );
        let body: serde_json::Value = self
            .client
            .get(&url)
            .bearer_auth_if_set(&self.bearer().await?)
            .send()
            .await
            .map_err(|e| Error::Network(e.to_string()))?
            .json()
            .await
            .map_err(|e| Error::Network(e.to_string()))?;

        let keys = body["items"]
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item["name"].as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();
        Ok(keys)
    }
}
