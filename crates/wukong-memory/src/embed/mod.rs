//! Embedding layer: trait, mock backend, and pure vector math.

use crate::error::Result;

/// Cosine similarity in [-1, 1]. Returns 0.0 for empty, length-mismatched,
/// or zero-magnitude inputs.
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f64 {
    if a.is_empty() || a.len() != b.len() {
        return 0.0;
    }
    let mut dot = 0.0f64;
    let mut na = 0.0f64;
    let mut nb = 0.0f64;
    for i in 0..a.len() {
        let (x, y) = (a[i] as f64, b[i] as f64);
        dot += x * y;
        na += x * x;
        nb += y * y;
    }
    if na == 0.0 || nb == 0.0 {
        return 0.0;
    }
    dot / (na.sqrt() * nb.sqrt())
}

/// Serialize an embedding to little-endian f32 bytes.
pub fn embedding_to_blob(v: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(v.len() * 4);
    for x in v {
        out.extend_from_slice(&x.to_le_bytes());
    }
    out
}

/// Deserialize little-endian f32 bytes back to an embedding.
pub fn blob_to_embedding(b: &[u8]) -> Vec<f32> {
    b.chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

/// Turns text into a fixed-dimension embedding. Implementors must be cheap to
/// share across threads (used from a background task).
pub trait Embedder: Send + Sync {
    fn embed(&self, text: &str) -> Result<Vec<f32>>;
    fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        texts.iter().map(|t| self.embed(t)).collect()
    }
    fn dim(&self) -> usize;
    fn model_id(&self) -> &str;
}

/// Deterministic, dependency-free embedder for tests. Same text always maps to
/// the same unit vector; different text usually maps elsewhere. NOT semantic —
/// it only exercises the plumbing.
pub struct MockEmbedder {
    dim: usize,
}

impl MockEmbedder {
    pub fn new(dim: usize) -> Self {
        Self { dim }
    }
}

impl Embedder for MockEmbedder {
    fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let mut v = vec![0.0f32; self.dim];
        for (i, b) in text.bytes().enumerate() {
            v[(b as usize + i) % self.dim] += 1.0;
        }
        let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 0.0 {
            for x in &mut v {
                *x /= norm;
            }
        }
        Ok(v)
    }

    fn dim(&self) -> usize {
        self.dim
    }

    fn model_id(&self) -> &str {
        "mock"
    }
}

/// Real local embedder backed by fastembed (ONNX). Behind the `embed` feature
/// so the default build pulls no heavy dependencies. Default model:
/// all-MiniLM-L6-v2 (384 dims). First use downloads the model to cache.
#[cfg(feature = "embed")]
pub struct FastembedBackend {
    model: fastembed::TextEmbedding,
    dim: usize,
    model_id: String,
}

#[cfg(feature = "embed")]
impl FastembedBackend {
    /// Construct with the default all-MiniLM-L6-v2 model.
    pub fn new() -> Result<Self> {
        use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};
        let model = TextEmbedding::try_new(
            InitOptions::new(EmbeddingModel::AllMiniLML6V2).with_show_download_progress(false),
        )
        .map_err(|e| crate::error::MemoryError::Embed(e.to_string()))?;
        Ok(Self {
            model,
            dim: 384,
            model_id: "all-MiniLM-L6-v2".to_string(),
        })
    }
}

#[cfg(feature = "embed")]
impl Embedder for FastembedBackend {
    fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let mut out = self
            .model
            .embed(vec![text], None)
            .map_err(|e| crate::error::MemoryError::Embed(e.to_string()))?;
        out.pop()
            .ok_or_else(|| crate::error::MemoryError::Embed("empty embedding".to_string()))
    }

    fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        self.model
            .embed(texts.to_vec(), None)
            .map_err(|e| crate::error::MemoryError::Embed(e.to_string()))
    }

    fn dim(&self) -> usize {
        self.dim
    }

    fn model_id(&self) -> &str {
        &self.model_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cosine_identical_is_one() {
        let a = vec![1.0, 2.0, 3.0];
        assert!((cosine_similarity(&a, &a) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn cosine_orthogonal_is_zero() {
        let a = vec![1.0, 0.0];
        let b = vec![0.0, 1.0];
        assert!(cosine_similarity(&a, &b).abs() < 1e-9);
    }

    #[test]
    fn cosine_opposite_is_minus_one() {
        let a = vec![1.0, 0.0];
        let b = vec![-1.0, 0.0];
        assert!((cosine_similarity(&a, &b) + 1.0).abs() < 1e-9);
    }

    #[test]
    fn cosine_zero_or_mismatch_is_zero() {
        assert_eq!(cosine_similarity(&[0.0, 0.0], &[1.0, 1.0]), 0.0);
        assert_eq!(cosine_similarity(&[1.0], &[1.0, 2.0]), 0.0);
        assert_eq!(cosine_similarity(&[], &[]), 0.0);
    }

    #[test]
    fn blob_roundtrip_preserves_values() {
        let v = vec![1.5f32, -2.25, 0.0, 3.125];
        let restored = blob_to_embedding(&embedding_to_blob(&v));
        assert_eq!(v, restored);
    }

    #[test]
    fn mock_is_deterministic() {
        let e = MockEmbedder::new(8);
        assert_eq!(e.embed("hello").unwrap(), e.embed("hello").unwrap());
    }

    #[test]
    fn mock_differs_for_different_text() {
        let e = MockEmbedder::new(8);
        assert_ne!(e.embed("hello").unwrap(), e.embed("world!!").unwrap());
    }

    #[test]
    fn mock_dim_and_batch() {
        let e = MockEmbedder::new(8);
        assert_eq!(e.embed("x").unwrap().len(), 8);
        assert_eq!(e.dim(), 8);
        let batch = e.embed_batch(&["a".into(), "b".into()]).unwrap();
        assert_eq!(batch.len(), 2);
        assert_eq!(batch[0], e.embed("a").unwrap());
    }
}
