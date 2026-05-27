use async_trait::async_trait;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::traits::{Error, Result};

/// Embedding dimensionality used across the entire stack.
/// Matches Vertex AI `text-multilingual-embedding-002` output.
pub const EMBEDDING_DIMS: usize = 768;

/// A typed embedding. The dimension invariant is encoded in the type:
/// values can only be constructed via [`Embedding::new`], which validates length.
/// Deserialization routes through the same constructor — corrupt stored data
/// fails loudly at the deser boundary with the same error as constructor misuse.
#[derive(Debug, Clone, PartialEq)]
pub struct Embedding([f32; EMBEDDING_DIMS]);

impl Embedding {
    pub fn new(values: Vec<f32>) -> std::result::Result<Self, Error> {
        let arr: [f32; EMBEDDING_DIMS] = values.try_into().map_err(|v: Vec<f32>| {
            Error::Other(format!("expected {EMBEDDING_DIMS} dims, got {}", v.len()))
        })?;
        Ok(Self(arr))
    }

    pub fn as_slice(&self) -> &[f32] {
        &self.0
    }
}

impl Serialize for Embedding {
    fn serialize<S: Serializer>(&self, ser: S) -> std::result::Result<S::Ok, S::Error> {
        // Serialize as Vec<f32> for cross-backend compatibility (JSON, Firestore,
        // redb-stored JSON). Fixed-size arrays don't get free serde derives.
        self.0.as_ref().serialize(ser)
    }
}

impl<'de> Deserialize<'de> for Embedding {
    fn deserialize<D: Deserializer<'de>>(de: D) -> std::result::Result<Self, D::Error> {
        let v = Vec::<f32>::deserialize(de)?;
        Embedding::new(v).map_err(serde::de::Error::custom)
    }
}

/// Embedding generation. Standardised on 768 dims (Vertex AI text-multilingual-embedding-002).
///
/// Remote impl: Vertex AI (replaces OpenAI)
/// Local impl:  fastembed (all-MiniLM-L6-v2, resized to 768 via zero-padding or a local 768-dim model)
#[async_trait]
pub trait EmbeddingProvider: Send + Sync {
    /// Embed a single text string. Empty/whitespace input → `Err`.
    async fn embed(&self, text: &str) -> Result<Embedding>;

    /// Embed a batch of texts. Implementations should use the provider's native batching.
    /// Same empty-input rule applies per element.
    async fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Embedding>> {
        let mut results = Vec::with_capacity(texts.len());
        for text in texts {
            results.push(self.embed(text).await?);
        }
        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_rejects_wrong_dim() {
        let err = Embedding::new(vec![0.0; 512]).unwrap_err();
        assert!(matches!(err, Error::Other(ref m) if m.contains("expected 768 dims, got 512")));
    }

    #[test]
    fn new_accepts_correct_dim() {
        let e = Embedding::new(vec![0.0; EMBEDDING_DIMS]).unwrap();
        assert_eq!(e.as_slice().len(), EMBEDDING_DIMS);
    }

    #[test]
    fn serde_roundtrip() {
        let v: Vec<f32> = (0..EMBEDDING_DIMS).map(|i| i as f32 * 0.001).collect();
        let e = Embedding::new(v.clone()).unwrap();
        let json = serde_json::to_string(&e).unwrap();
        let e2: Embedding = serde_json::from_str(&json).unwrap();
        assert_eq!(e, e2);
    }

    #[test]
    fn deser_rejects_wrong_dim() {
        let bad: Vec<f32> = vec![0.0; 512];
        let json = serde_json::to_string(&bad).unwrap();
        let result: std::result::Result<Embedding, _> = serde_json::from_str(&json);
        assert!(result.is_err());
    }
}
