use std::collections::HashMap;

use async_trait::async_trait;
use serde_json::Value;

use crate::{
    remote::GcpToken,
    traits::{
        Error, Result,
        vector::{Match, VectorStore},
    },
};

/// Vertex AI Vector Search (Matching Engine) upsert + query.
/// Requires an index endpoint to be deployed via Terraform (vertex-vector module).
pub struct VertexVectorStore {
    project_id: String,
    region: String,
    token: GcpToken,
    client: reqwest::Client,
    /// Index endpoint ID — read from env VERTEX_INDEX_ENDPOINT_ID
    endpoint_id: String,
    /// Deployed index ID — read from env VERTEX_DEPLOYED_INDEX_ID
    deployed_index_id: String,
}

impl VertexVectorStore {
    pub fn new(project_id: String, region: String, token: GcpToken) -> Self {
        Self {
            project_id,
            region: region.clone(),
            token,
            client: reqwest::Client::new(),
            endpoint_id: std::env::var("VERTEX_INDEX_ENDPOINT_ID").unwrap_or_default(),
            deployed_index_id: std::env::var("VERTEX_DEPLOYED_INDEX_ID").unwrap_or_default(),
        }
    }

    async fn bearer(&self) -> Result<String> {
        self.token
            .get()
            .await
            .map_err(|e| Error::Network(e.to_string()))
    }
}

#[async_trait]
impl VectorStore for VertexVectorStore {
    async fn upsert(
        &self,
        _namespace: &str,
        id: &str,
        embedding: &[f32],
        _text: Option<&str>,
        _metadata: HashMap<String, Value>,
    ) -> Result<()> {
        let url = format!(
            "https://{}-aiplatform.googleapis.com/v1/projects/{}/locations/{}/indexes/{}/upsertDatapoints",
            self.region, self.project_id, self.region, self.endpoint_id
        );
        let body = serde_json::json!({
            "datapoints": [{
                "datapointId": id,
                "featureVector": embedding
            }]
        });
        self.client
            .post(&url)
            .bearer_auth(self.bearer().await?)
            .json(&body)
            .send()
            .await
            .map_err(|e| Error::Network(e.to_string()))?
            .error_for_status()
            .map_err(|e| Error::Network(e.to_string()))?;
        Ok(())
    }

    async fn search(&self, _namespace: &str, query: &[f32], top_k: usize) -> Result<Vec<Match>> {
        let url = format!(
            "https://{}-aiplatform.googleapis.com/v1/projects/{}/locations/{}/indexEndpoints/{}:findNeighbors",
            self.region, self.project_id, self.region, self.endpoint_id
        );
        let body = serde_json::json!({
            "deployedIndexId": self.deployed_index_id,
            "queries": [{
                "datapoint": { "featureVector": query },
                "neighborCount": top_k
            }]
        });
        let resp: Value = self
            .client
            .post(&url)
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

        let mut matches = Vec::new();
        if let Some(results) = resp["nearestNeighbors"][0]["neighbors"].as_array() {
            for n in results {
                let id = n["datapoint"]["datapointId"]
                    .as_str()
                    .unwrap_or("")
                    .to_string();
                let score = n["distance"].as_f64().unwrap_or(0.0) as f32;
                matches.push(Match {
                    id,
                    score,
                    metadata: HashMap::new(),
                    text: None,
                });
            }
        }
        Ok(matches)
    }

    async fn delete(&self, _namespace: &str, id: &str) -> Result<()> {
        let url = format!(
            "https://{}-aiplatform.googleapis.com/v1/projects/{}/locations/{}/indexes/{}/removeDatapoints",
            self.region, self.project_id, self.region, self.endpoint_id
        );
        let body = serde_json::json!({ "datapointIds": [id] });
        self.client
            .post(&url)
            .bearer_auth(self.bearer().await?)
            .json(&body)
            .send()
            .await
            .map_err(|e| Error::Network(e.to_string()))?
            .error_for_status()
            .map_err(|e| Error::Network(e.to_string()))?;
        Ok(())
    }
}
