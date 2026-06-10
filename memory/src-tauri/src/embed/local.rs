//! Local (on-device) embedding provider. OpenAI-compatible `/v1/embeddings`.

use crate::embed::EmbeddingProvider;
use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use serde_json::{json, Value};

pub const DEFAULT_ENDPOINT: &str = "http://localhost:8081/v1";
pub const DEFAULT_MODEL: &str = "nomic-embed-text-v1.5";

pub struct LocalEmbeddingProvider {
    client: reqwest::Client,
    endpoint: String,
    api_key: String,
    model: String,
    dim: usize,
}

impl LocalEmbeddingProvider {
    pub fn new(model: String, endpoint: String, dim: usize) -> Result<Self> {
        let api_key = std::env::var("CAIRN_LOCAL_API_KEY").unwrap_or_default();
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .build()?;
        Ok(Self {
            client,
            endpoint,
            api_key,
            model,
            dim,
        })
    }
}

#[async_trait]
impl EmbeddingProvider for LocalEmbeddingProvider {
    fn name(&self) -> &'static str {
        "local"
    }
    fn dim(&self) -> usize {
        self.dim
    }

    async fn embed_batch(&self, inputs: &[String]) -> Result<Vec<Vec<f32>>> {
        if inputs.is_empty() {
            return Ok(vec![]);
        }
        let url = format!("{}/embeddings", self.endpoint.trim_end_matches('/'));
        let body = json!({
            "model": self.model,
            "input": inputs,
            "encoding_format": "float"
        });

        let mut req = self
            .client
            .post(&url)
            .header("content-type", "application/json")
            .json(&body);
        if !self.api_key.is_empty() {
            req = req.header("authorization", format!("Bearer {}", self.api_key));
        }

        let resp = req.send().await.context("local embeddings request")?;
        let status = resp.status();
        let payload: Value = resp.json().await.context("decode embeddings json")?;
        if !status.is_success() {
            return Err(anyhow!("local embeddings error {}: {}", status, payload));
        }

        let data = payload
            .get("data")
            .and_then(|v| v.as_array())
            .ok_or_else(|| anyhow!("no data array in embeddings response: {}", payload))?;

        let mut out = Vec::with_capacity(data.len());
        for item in data {
            let arr = item
                .get("embedding")
                .and_then(|v| v.as_array())
                .ok_or_else(|| anyhow!("no embedding array in item: {}", item))?;
            let mut v = Vec::with_capacity(arr.len());
            for n in arr {
                v.push(n.as_f64().unwrap_or(0.0) as f32);
            }
            // Local servers often don't honour `dimensions`; truncate/pad on our side.
            v = align_dim(v, self.dim);
            out.push(v);
        }
        Ok(out)
    }
}

/// Truncate or zero-pad to the configured dim. Truncation followed by L2-renorm
/// is the canonical approach for Matryoshka-trained models; zero-pad is benign
/// for fixed-dim models since cosine similarity is unaffected by trailing zeros.
fn align_dim(mut v: Vec<f32>, target: usize) -> Vec<f32> {
    match v.len().cmp(&target) {
        std::cmp::Ordering::Equal => v,
        std::cmp::Ordering::Greater => {
            v.truncate(target);
            l2_normalize(&mut v);
            v
        }
        std::cmp::Ordering::Less => {
            v.resize(target, 0.0);
            v
        }
    }
}

fn l2_normalize(v: &mut [f32]) {
    let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        for x in v.iter_mut() {
            *x /= norm;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn align_truncates_long_and_renorms() {
        let mut v = vec![1.0, 0.0, 0.0, 0.0, 0.0];
        v = align_dim(v, 3);
        assert_eq!(v.len(), 3);
        let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-6);
    }

    #[test]
    fn align_pads_short() {
        let v = align_dim(vec![1.0, 2.0], 5);
        assert_eq!(v, vec![1.0, 2.0, 0.0, 0.0, 0.0]);
    }
}
