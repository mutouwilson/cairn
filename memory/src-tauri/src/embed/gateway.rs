//! Vercel AI Gateway embedding provider. OpenAI-compatible `/v1/embeddings`.

use crate::embed::EmbeddingProvider;
use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use serde_json::{json, Value};

pub const DEFAULT_BASE_URL: &str = "https://ai-gateway.vercel.sh/v1";
pub const DEFAULT_MODEL: &str = "openai/text-embedding-3-small";

pub struct GatewayEmbeddingProvider {
    client: reqwest::Client,
    base_url: String,
    api_key: String,
    model: String,
    dim: usize,
    /// Tag used in usage telemetry — `"gateway"` for Vercel, `"openai"`,
    /// `"glm"`, etc. when the same OpenAI-compatible client points at a
    /// different provider's `/v1/embeddings` endpoint.
    provider_tag: String,
}

impl GatewayEmbeddingProvider {
    pub fn new(model: String, dim: usize) -> Result<Self> {
        let api_key = std::env::var("AI_GATEWAY_API_KEY")
            .or_else(|_| std::env::var("AI_GATEWAY_TOKEN"))
            .context("AI_GATEWAY_API_KEY (or AI_GATEWAY_TOKEN) not set")?;
        let base_url =
            std::env::var("AI_GATEWAY_BASE_URL").unwrap_or_else(|_| DEFAULT_BASE_URL.to_string());
        Self::with_config(api_key, model, dim, base_url, "gateway".into())
    }

    /// Build a provider against an arbitrary OpenAI-compatible
    /// `/v1/embeddings` endpoint (used by the multi-provider settings).
    pub fn with_config(
        api_key: String,
        model: String,
        dim: usize,
        base_url: String,
        provider_tag: String,
    ) -> Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .build()?;
        Ok(Self {
            client,
            base_url,
            api_key,
            model,
            dim,
            provider_tag,
        })
    }
}

#[async_trait]
impl EmbeddingProvider for GatewayEmbeddingProvider {
    fn name(&self) -> &'static str {
        "gateway"
    }
    fn dim(&self) -> usize {
        self.dim
    }

    async fn embed_batch(&self, inputs: &[String]) -> Result<Vec<Vec<f32>>> {
        if inputs.is_empty() {
            return Ok(vec![]);
        }
        let url = format!("{}/embeddings", self.base_url.trim_end_matches('/'));

        // OpenAI's text-embedding-3-* honours the `dimensions` field. Other
        // backends silently ignore it; we re-check the returned vector size.
        let body = json!({
            "model": self.model,
            "input": inputs,
            "dimensions": self.dim,
            "encoding_format": "float"
        });

        let started = std::time::Instant::now();
        let resp = self
            .client
            .post(&url)
            .header("authorization", format!("Bearer {}", self.api_key))
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                crate::usage::record(crate::usage::UsageEvent {
                    operation: "embed",
                    provider: self.provider_tag.clone(),
                    model: self.model.clone(),
                    prompt_tokens: None,
                    completion_tokens: None,
                    cost_micro_usd: None,
                    latency_ms: started.elapsed().as_millis() as i64,
                    success: false,
                    error: Some(format!("transport: {e}")),
                });
                e
            })
            .context("gateway embeddings request")?;

        let status = resp.status();
        let payload: Value = resp.json().await.context("decode embeddings json")?;
        let latency_ms = started.elapsed().as_millis() as i64;
        if !status.is_success() {
            crate::usage::record(crate::usage::UsageEvent {
                operation: "embed",
                provider: self.provider_tag.clone(),
                model: self.model.clone(),
                prompt_tokens: None,
                completion_tokens: None,
                cost_micro_usd: None,
                latency_ms,
                success: false,
                error: Some(format!("{} {}", status, payload)),
            });
            return Err(anyhow!("gateway embeddings error {}: {}", status, payload));
        }

        let usage = payload.get("usage");
        let prompt_tokens = usage
            .and_then(|u| u.get("prompt_tokens"))
            .and_then(|v| v.as_i64());
        let cost = usage
            .and_then(|u| u.get("cost"))
            .and_then(|v| v.as_f64())
            .and_then(crate::usage::usd_to_micro);
        crate::usage::record(crate::usage::UsageEvent {
            operation: "embed",
            provider: "gateway".into(),
            model: self.model.clone(),
            prompt_tokens,
            completion_tokens: None,
            cost_micro_usd: cost,
            latency_ms,
            success: true,
            error: None,
        });

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
            if v.len() != self.dim {
                return Err(anyhow!(
                    "embedding dim mismatch: expected {}, got {} (model `{}` may not support custom dim)",
                    self.dim,
                    v.len(),
                    self.model
                ));
            }
            out.push(v);
        }
        Ok(out)
    }
}
