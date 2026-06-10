//! Personal-embedding residual adapter (Phase 4d).
//!
//! Loads `adapter.safetensors` produced by `tools/distill/embed_finetune.py`
//! and applies it on top of every base embedding:
//!
//! ```text
//!     out = normalize(base + alpha * tanh(W @ base + b))
//! ```
//!
//! The Python trainer initialises `alpha = 0` (identity at start) and learns
//! `W`, `b`, `alpha` jointly. When no adapter file exists, runtime degrades
//! cleanly to the base provider.

use crate::embed::EmbeddingProvider;
use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use safetensors::SafeTensors;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Debug)]
pub struct EmbeddingAdapter {
    pub dim: usize,
    pub w: Vec<f32>, // dim × dim, row-major
    pub b: Vec<f32>, // dim
    pub alpha: f32,
    pub source_path: PathBuf,
}

impl EmbeddingAdapter {
    /// Try to load from `path`. Returns `Ok(None)` if the file doesn't exist
    /// (the common case for users without signal data yet). Returns `Err`
    /// only for genuine corruption / dim mismatch.
    pub fn load_optional(path: &Path, expected_dim: usize) -> Result<Option<Self>> {
        if !path.exists() {
            return Ok(None);
        }
        let bytes = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
        let st = SafeTensors::deserialize(&bytes).context("parse safetensors")?;

        let w = read_f32_tensor(&st, "linear.weight")?;
        let b = read_f32_tensor(&st, "linear.bias")?;
        let alpha_vec = read_f32_tensor(&st, "alpha")?;

        if alpha_vec.len() != 1 {
            return Err(anyhow!(
                "adapter alpha must be a scalar tensor, got {} elements",
                alpha_vec.len()
            ));
        }
        if b.len() != expected_dim {
            return Err(anyhow!(
                "adapter bias dim {} != base provider dim {}",
                b.len(),
                expected_dim
            ));
        }
        if w.len() != expected_dim * expected_dim {
            return Err(anyhow!(
                "adapter weight has {} elements, expected dim²={}",
                w.len(),
                expected_dim * expected_dim
            ));
        }

        Ok(Some(Self {
            dim: expected_dim,
            w,
            b,
            alpha: alpha_vec[0],
            source_path: path.to_path_buf(),
        }))
    }

    /// Apply `out = normalize(base + alpha * tanh(W @ base + b))`.
    pub fn apply(&self, base: &[f32]) -> Vec<f32> {
        debug_assert_eq!(base.len(), self.dim);
        let mut hidden = vec![0.0f32; self.dim];
        for (i, hidden_i) in hidden.iter_mut().enumerate() {
            let mut acc = self.b[i];
            let row_start = i * self.dim;
            let row = &self.w[row_start..row_start + self.dim];
            for (j, &b_j) in base.iter().enumerate() {
                acc += row[j] * b_j;
            }
            *hidden_i = acc.tanh();
        }
        let mut out = base.to_vec();
        for i in 0..self.dim {
            out[i] += self.alpha * hidden[i];
        }
        l2_normalize(&mut out);
        out
    }
}

fn read_f32_tensor(st: &SafeTensors, name: &str) -> Result<Vec<f32>> {
    let view = st
        .tensor(name)
        .map_err(|e| anyhow!("missing tensor `{name}`: {e}"))?;
    if view.dtype() != safetensors::Dtype::F32 {
        return Err(anyhow!(
            "tensor `{name}` dtype={:?}, expected F32",
            view.dtype()
        ));
    }
    let raw = view.data();
    if raw.len() % 4 != 0 {
        return Err(anyhow!("tensor `{name}` data not aligned to 4 bytes"));
    }
    let mut out = Vec::with_capacity(raw.len() / 4);
    for chunk in raw.chunks_exact(4) {
        out.push(f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
    }
    Ok(out)
}

fn l2_normalize(v: &mut [f32]) {
    let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        for x in v.iter_mut() {
            *x /= norm;
        }
    }
}

/// Wraps any base `EmbeddingProvider` with the residual adapter. Pass-through
/// if no adapter is attached.
pub struct AdaptedEmbeddingProvider {
    base: Arc<dyn EmbeddingProvider>,
    adapter: Option<EmbeddingAdapter>,
}

impl AdaptedEmbeddingProvider {
    pub fn wrap(base: Arc<dyn EmbeddingProvider>, adapter: Option<EmbeddingAdapter>) -> Self {
        if let Some(a) = &adapter {
            tracing::info!(path = %a.source_path.display(), alpha = a.alpha, "personal embedding adapter loaded");
        }
        Self { base, adapter }
    }
}

#[async_trait]
impl EmbeddingProvider for AdaptedEmbeddingProvider {
    fn name(&self) -> &'static str {
        if self.adapter.is_some() {
            "adapted"
        } else {
            "passthrough"
        }
    }
    fn dim(&self) -> usize {
        self.base.dim()
    }
    async fn embed_batch(&self, inputs: &[String]) -> Result<Vec<Vec<f32>>> {
        let raw = self.base.embed_batch(inputs).await?;
        let Some(a) = &self.adapter else {
            return Ok(raw);
        };
        Ok(raw.into_iter().map(|v| a.apply(&v)).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use safetensors::tensor::TensorView;
    use safetensors::Dtype;
    use std::collections::HashMap;
    use tempfile::tempdir;

    fn make_test_adapter_file(path: &Path, dim: usize, alpha_val: f32) {
        // Identity-ish adapter: W = 0, b = 0, alpha = whatever.
        // With W=0, b=0 and tanh(0)=0, out = normalize(base + alpha * 0) = normalize(base).
        let w: Vec<f32> = vec![0.0; dim * dim];
        let b: Vec<f32> = vec![0.0; dim];
        let alpha: Vec<f32> = vec![alpha_val];

        fn bytes(v: &[f32]) -> Vec<u8> {
            let mut out = Vec::with_capacity(v.len() * 4);
            for x in v {
                out.extend_from_slice(&x.to_le_bytes());
            }
            out
        }
        let w_bytes = bytes(&w);
        let b_bytes = bytes(&b);
        let a_bytes = bytes(&alpha);

        let mut map = HashMap::new();
        map.insert(
            "linear.weight",
            TensorView::new(Dtype::F32, vec![dim, dim], &w_bytes).unwrap(),
        );
        map.insert(
            "linear.bias",
            TensorView::new(Dtype::F32, vec![dim], &b_bytes).unwrap(),
        );
        map.insert(
            "alpha",
            TensorView::new(Dtype::F32, vec![1], &a_bytes).unwrap(),
        );
        safetensors::serialize_to_file(&map, &None, path).unwrap();
    }

    #[test]
    fn missing_file_returns_none() {
        let dir = tempdir().unwrap();
        let p = dir.path().join("nope.safetensors");
        assert!(EmbeddingAdapter::load_optional(&p, 1024).unwrap().is_none());
    }

    #[test]
    fn identity_adapter_normalises_input() {
        let dir = tempdir().unwrap();
        let p = dir.path().join("adapter.safetensors");
        make_test_adapter_file(&p, 4, 0.5);

        let a = EmbeddingAdapter::load_optional(&p, 4)
            .unwrap()
            .expect("adapter loads");
        assert_eq!(a.dim, 4);
        assert_eq!(a.alpha, 0.5);

        // Base = [3, 4, 0, 0] → norm = 5 → after apply (W=0, b=0): normalize([3,4,0,0]) = [0.6,0.8,0,0]
        let out = a.apply(&[3.0, 4.0, 0.0, 0.0]);
        assert!((out[0] - 0.6).abs() < 1e-6);
        assert!((out[1] - 0.8).abs() < 1e-6);
    }

    #[test]
    fn dim_mismatch_rejected() {
        let dir = tempdir().unwrap();
        let p = dir.path().join("adapter.safetensors");
        make_test_adapter_file(&p, 4, 0.1);
        let err = EmbeddingAdapter::load_optional(&p, 8).unwrap_err();
        assert!(format!("{err}").contains("dim"));
    }
}
