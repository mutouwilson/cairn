//! Local (on-device) extraction provider.
//!
//! Speaks the same OpenAI-compatible chat-completions API as the gateway. Designed
//! to talk to a `llama-server` (from llama.cpp), an MLX-backed server, LM Studio,
//! or Ollama's OpenAI-compatible endpoint.
//!
//! Env:
//!   - `CAIRN_LOCAL_ENDPOINT` (default: `http://localhost:8080/v1`)
//!   - `CAIRN_LOCAL_MODEL`    (default: `qwen2.5-1.5b-extract`)
//!   - `CAIRN_LOCAL_API_KEY`  (optional; many local servers ignore auth)
//!
//! Phase 2 plugs in the distilled Qwen 2.5 1.5B model produced by
//! `tools/distill/`. The model is fine-tuned to call `save_extraction` directly
//! so we use the same function-calling code path as the gateway.

use crate::extract::openai_compat::{extract as oa_extract, OpenAiCompatConfig};
use crate::extract::ExtractionProvider;
use crate::schema::ExtractionResult;
use anyhow::Result;
use async_trait::async_trait;

pub const DEFAULT_ENDPOINT: &str = "http://localhost:8080/v1";
pub const DEFAULT_MODEL: &str = "qwen2.5-1.5b-extract";

pub struct LocalProvider {
    client: reqwest::Client,
    cfg: OpenAiCompatConfig,
}

impl LocalProvider {
    pub fn new(model: String, endpoint: String) -> Result<Self> {
        let api_key = std::env::var("CAIRN_LOCAL_API_KEY").unwrap_or_default();
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()?;
        Ok(Self {
            client,
            cfg: OpenAiCompatConfig {
                base_url: endpoint,
                api_key,
                model,
                label: "local",
            },
        })
    }
}

#[async_trait]
impl ExtractionProvider for LocalProvider {
    fn name(&self) -> &'static str {
        "local"
    }
    async fn extract(&self, text: &str) -> Result<ExtractionResult> {
        oa_extract(&self.client, &self.cfg, text).await
    }
}
