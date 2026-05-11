use async_trait::async_trait;

use crate::traits::Result;

/// Embedding generation. Standardised on 768 dims (Vertex AI text-multilingual-embedding-002).
///
/// Remote impl: Vertex AI (replaces OpenAI)
/// Local impl:  fastembed (all-MiniLM-L6-v2, resized to 768 via zero-padding or a local 768-dim model)
#[async_trait]
pub trait EmbeddingProvider: Send + Sync {
    /// Embed a single text string.
    async fn embed(&self, text: &str) -> Result<Vec<f32>>;

    /// Embed a batch of texts. Implementations should use the provider's native batching.
    async fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        let mut results = Vec::with_capacity(texts.len());
        for text in texts {
            results.push(self.embed(text).await?);
        }
        Ok(results)
    }

    /// Returns the output dimensionality. All implementations in this crate return 768.
    fn dims(&self) -> usize;
}
