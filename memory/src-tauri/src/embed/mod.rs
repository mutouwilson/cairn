//! Embedding layer for vector retrieval.
//!
//! Pluggable `EmbeddingProvider` trait. Two real implementations:
//!   - `GatewayEmbeddingProvider` — Vercel AI Gateway `/v1/embeddings`
//!     (OpenAI-compatible). Routes by `provider/model` id.
//!   - `LocalEmbeddingProvider` — same protocol against a local server
//!     (llama-server / MLX / Ollama). Latency optimised for local hosting.
//!
//! Selection priority mirrors the extraction layer:
//!   1. Honour `CAIRN_EMBEDDING_PROVIDER` if set.
//!   2. Prefer `gateway` if `AI_GATEWAY_API_KEY` set.
//!   3. Otherwise NullEmbeddingProvider.
//!
//! Dimension is locked at process start via `CAIRN_EMBEDDING_DIM` (default
//! 1024) so the on-disk vec0 table doesn't move. Changing the dim across runs
//! requires re-embedding all entities — see `commands::reembed_all`.

pub mod adapter;
pub mod gateway;
pub mod local;

use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;

pub const DEFAULT_DIM: usize = 1024;

#[async_trait]
pub trait EmbeddingProvider: Send + Sync {
    fn name(&self) -> &'static str;
    fn dim(&self) -> usize;
    async fn embed_batch(&self, inputs: &[String]) -> Result<Vec<Vec<f32>>>;

    async fn embed_one(&self, input: &str) -> Result<Vec<f32>> {
        let mut out = self.embed_batch(&[input.to_string()]).await?;
        out.pop()
            .ok_or_else(|| anyhow::anyhow!("embedding provider returned 0 vectors"))
    }
}

pub fn build_default() -> Arc<dyn EmbeddingProvider> {
    build_default_with_adapter_dir(None)
}

/// Build the embedding stack and optionally layer the personal adapter from
/// the given data dir (Phase 4d).
pub fn build_default_with_adapter_dir(
    adapter_dir: Option<&std::path::Path>,
) -> Arc<dyn EmbeddingProvider> {
    let base = build_base();
    if let Some(dir) = adapter_dir {
        let path = dir.join("adapter.safetensors");
        match adapter::EmbeddingAdapter::load_optional(&path, base.dim()) {
            Ok(adapter) => {
                return Arc::new(adapter::AdaptedEmbeddingProvider::wrap(base, adapter));
            }
            Err(e) => tracing::warn!(?path, error = %e, "personal embedding adapter ignored"),
        }
    }
    base
}

fn build_base() -> Arc<dyn EmbeddingProvider> {
    let dim = std::env::var("CAIRN_EMBEDDING_DIM")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(DEFAULT_DIM);

    let explicit = std::env::var("CAIRN_EMBEDDING_PROVIDER")
        .ok()
        .map(|s| s.to_lowercase());

    let chosen = match explicit.as_deref() {
        Some("gateway") | Some("vercel") => "gateway",
        Some("local") | Some("ondevice") | Some("on-device") => "local",
        Some(other) if !other.is_empty() => {
            tracing::warn!(
                provider = other,
                "unknown CAIRN_EMBEDDING_PROVIDER; auto-selecting"
            );
            ""
        }
        _ => "",
    };

    let resolved = if !chosen.is_empty() {
        chosen
    } else if std::env::var("AI_GATEWAY_API_KEY").is_ok()
        || std::env::var("AI_GATEWAY_TOKEN").is_ok()
    {
        "gateway"
    } else {
        ""
    };

    let model = std::env::var("CAIRN_EMBEDDING_MODEL").ok();

    match resolved {
        "gateway" => {
            let m = model.unwrap_or_else(|| gateway::DEFAULT_MODEL.to_string());
            match gateway::GatewayEmbeddingProvider::new(m.clone(), dim) {
                Ok(p) => {
                    tracing::info!(provider = "gateway", model = %m, dim, "embedding ready");
                    return Arc::new(p);
                }
                Err(e) => tracing::error!(error = %e, "gateway embedding init failed"),
            }
        }
        "local" => {
            let m = model.unwrap_or_else(|| local::DEFAULT_MODEL.to_string());
            let endpoint = std::env::var("CAIRN_LOCAL_EMBEDDING_ENDPOINT")
                .unwrap_or_else(|_| local::DEFAULT_ENDPOINT.to_string());
            match local::LocalEmbeddingProvider::new(m.clone(), endpoint.clone(), dim) {
                Ok(p) => {
                    tracing::info!(
                        provider = "local",
                        model = %m,
                        endpoint = %endpoint,
                        dim,
                        "embedding ready"
                    );
                    return Arc::new(p);
                }
                Err(e) => tracing::error!(error = %e, "local embedding init failed"),
            }
        }
        _ => {}
    }

    tracing::warn!(dim, "no embedding credentials; vector retrieval disabled");
    Arc::new(NullEmbeddingProvider { dim })
}

pub struct NullEmbeddingProvider {
    pub dim: usize,
}

impl NullEmbeddingProvider {
    pub fn new(dim: usize) -> Self {
        Self { dim }
    }
}

#[async_trait]
impl EmbeddingProvider for NullEmbeddingProvider {
    fn name(&self) -> &'static str {
        "null"
    }
    fn dim(&self) -> usize {
        self.dim
    }
    async fn embed_batch(&self, _inputs: &[String]) -> Result<Vec<Vec<f32>>> {
        Err(anyhow::anyhow!(
            "no embedding provider configured (set CAIRN_EMBEDDING_PROVIDER or AI_GATEWAY_API_KEY)"
        ))
    }
}
