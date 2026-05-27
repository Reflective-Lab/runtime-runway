use async_trait::async_trait;

use crate::{
    remote::GcpToken,
    traits::{
        Error, Result,
        embedding::{Embedding, EmbeddingProvider},
    },
};

/// Vertex AI `text-multilingual-embedding-002` (768 dims).
///
/// Replaces OpenAI `text-embedding-ada-002` / `text-embedding-3-small`.
/// Multilingual — correct choice for Folio (Swedish content) and any non-English data.
/// Endpoint: POST https://{region}-aiplatform.googleapis.com/v1/projects/{project}/locations/{region}/
///           publishers/google/models/text-multilingual-embedding-002:predict
pub struct VertexEmbedder {
    project_id: String,
    region: String,
    token: GcpToken,
    client: reqwest::Client,
}

impl VertexEmbedder {
    pub fn new(project_id: String, region: String, token: GcpToken) -> Self {
        Self {
            project_id,
            region,
            token,
            client: reqwest::Client::new(),
        }
    }

    fn endpoint(&self) -> String {
        format!(
            "{}/publishers/google/models/text-multilingual-embedding-002:predict",
            crate::endpoints::vertex_aiplatform(&self.region, &self.project_id)
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
impl EmbeddingProvider for VertexEmbedder {
    async fn embed(&self, text: &str) -> Result<Embedding> {
        if text.trim().is_empty() {
            return Err(Error::Other("embedding input is empty".into()));
        }
        let results = self.embed_batch(&[text]).await?;
        results
            .into_iter()
            .next()
            .ok_or_else(|| Error::Other("empty embedding response".into()))
    }

    async fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Embedding>> {
        for text in texts {
            if text.trim().is_empty() {
                return Err(Error::Other("embedding input is empty".into()));
            }
        }
        let instances: Vec<_> = texts
            .iter()
            .map(|t| serde_json::json!({ "content": t }))
            .collect();
        let body = serde_json::json!({ "instances": instances });

        let resp: serde_json::Value = self
            .client
            .post(self.endpoint())
            .bearer_auth(self.bearer().await?)
            .json(&body)
            .send()
            .await
            .map_err(|e| Error::Network(e.to_string()))?
            .error_for_status()
            .map_err(|e| Error::Network(e.to_string()))?
            .json()
            .await
            .map_err(|e| Error::Network(e.to_string()))?;

        let predictions = resp["predictions"]
            .as_array()
            .ok_or_else(|| Error::Other("missing predictions field".into()))?;

        predictions
            .iter()
            .map(|p| {
                let values = p["embeddings"]["values"]
                    .as_array()
                    .ok_or_else(|| Error::Other("missing embeddings.values".into()))?;
                let floats: Vec<f32> = values
                    .iter()
                    .map(|v| {
                        v.as_f64()
                            .map(|f| f as f32)
                            .ok_or_else(|| Error::Other("non-float in embedding".into()))
                    })
                    .collect::<Result<Vec<f32>>>()?;
                Embedding::new(floats)
            })
            .collect()
    }
}
