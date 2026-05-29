use async_trait::async_trait;

use crate::traits::{
    Error, Result,
    embedding::{EMBEDDING_DIMS, Embedding, EmbeddingProvider},
};

/// Local embedder for offline Tauri apps.
///
/// Uses the `fastembed` crate (all-MiniLM-L6-v2, 384 dims) and zero-pads to 768
/// so offline vectors are index-compatible with remote Vertex AI vectors.
/// This is a reasonable approximation for offline use; when online, the Tauri app
/// can re-embed via VertexEmbedder and replace local entries.
pub struct LocalEmbedder;

impl LocalEmbedder {
    pub fn new() -> Self {
        Self
    }
}

impl Default for LocalEmbedder {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl EmbeddingProvider for LocalEmbedder {
    async fn embed(&self, text: &str) -> Result<Embedding> {
        if text.trim().is_empty() {
            return Err(Error::Other("embedding input is empty".into()));
        }
        let results = self.embed_batch(&[text]).await?;
        results
            .into_iter()
            .next()
            .ok_or_else(|| Error::Other("empty local embedding".into()))
    }

    async fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Embedding>> {
        for text in texts {
            if text.trim().is_empty() {
                return Err(Error::Other("embedding input is empty".into()));
            }
        }
        // fastembed is synchronous; run in a blocking thread to avoid blocking the async runtime
        let owned: Vec<String> = texts.iter().map(|s| s.to_string()).collect();
        let raw: Vec<Vec<f32>> = tokio::task::spawn_blocking(move || embed_sync(&owned))
            .await
            .map_err(|e| Error::Other(e.to_string()))?
            .map_err(|e| Error::Other(e.to_string()))?;
        raw.into_iter().map(Embedding::new).collect()
    }
}

fn embed_sync(texts: &[String]) -> anyhow::Result<Vec<Vec<f32>>> {
    use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};
    let model = TextEmbedding::try_new(
        InitOptions::new(EmbeddingModel::AllMiniLML6V2).with_show_download_progress(false),
    )?;
    let embeddings = model.embed(texts.iter().map(|s| s.as_str()).collect(), None)?;
    // Pad 384-dim → 768-dim with zeros for index compatibility
    Ok(embeddings
        .into_iter()
        .map(|mut e| {
            e.resize(EMBEDDING_DIMS, 0.0);
            e
        })
        .collect())
}
