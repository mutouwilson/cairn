//! Vercel AI Gateway extraction provider.
//!
//! Talks the OpenAI-compatible API at `https://ai-gateway.vercel.sh/v1` and
//! routes by `provider/model` ids (e.g. `anthropic/claude-haiku-4-5-20251001`,
//! `openai/gpt-5-mini`, `google/gemini-2.5-flash`).
//!
//! Auth: `AI_GATEWAY_API_KEY` (preferred) or `AI_GATEWAY_TOKEN`.
//! Override base URL via `AI_GATEWAY_BASE_URL` for self-hosted gateways or
//! private routes.

use crate::extract::openai_compat::{extract as oa_extract, OpenAiCompatConfig};
use crate::extract::ExtractionProvider;
use crate::schema::ExtractionResult;
use anyhow::{Context, Result};
use async_trait::async_trait;

pub const DEFAULT_BASE_URL: &str = "https://ai-gateway.vercel.sh/v1";
pub const DEFAULT_MODEL: &str = "anthropic/claude-haiku-4-5-20251001";

pub struct GatewayProvider {
    client: reqwest::Client,
    cfg: OpenAiCompatConfig,
}

impl GatewayProvider {
    pub fn new(model: String) -> Result<Self> {
        let api_key = std::env::var("AI_GATEWAY_API_KEY")
            .or_else(|_| std::env::var("AI_GATEWAY_TOKEN"))
            .context("AI_GATEWAY_API_KEY (or AI_GATEWAY_TOKEN) not set")?;
        let base_url =
            std::env::var("AI_GATEWAY_BASE_URL").unwrap_or_else(|_| DEFAULT_BASE_URL.to_string());
        Self::with_config(api_key, model, base_url, "gateway")
    }

    /// Build an extractor against an arbitrary OpenAI-compatible
    /// chat-completions endpoint. Used by the multi-provider settings
    /// (Phase 6d) to drive OpenAI / OpenRouter / GLM / Qwen / Doubao /
    /// Minimax / Moonshot / Gemini's compat shim through the same client.
    pub fn with_config(
        api_key: String,
        model: String,
        base_url: String,
        label: &'static str,
    ) -> Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .build()?;
        Ok(Self {
            client,
            cfg: OpenAiCompatConfig {
                base_url,
                api_key,
                model,
                label,
            },
        })
    }
}

#[async_trait]
impl ExtractionProvider for GatewayProvider {
    fn name(&self) -> &'static str {
        "gateway"
    }
    async fn extract(&self, text: &str) -> Result<ExtractionResult> {
        oa_extract(&self.client, &self.cfg, text).await
    }
}
