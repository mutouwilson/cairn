//! LLM extraction layer.
//!
//! Pluggable `ExtractionProvider` trait. Three real implementations:
//!   - `GatewayProvider` — Vercel AI Gateway (OpenAI-compatible, routes any model).
//!   - `AnthropicProvider` — direct Anthropic Messages API (legacy fallback).
//!   - `LocalProvider` — on-device llama.cpp / MLX / LM Studio / Ollama.
//!
//! Selection priority (`build_default`):
//!   1. Honour `CAIRN_EXTRACTION_PROVIDER` if set explicitly.
//!   2. Otherwise prefer `gateway` if `AI_GATEWAY_API_KEY` is present.
//!   3. Otherwise prefer `anthropic` if `ANTHROPIC_API_KEY` is present.
//!   4. Otherwise NullProvider (no-op, never crashes the app).

pub mod anthropic;
pub mod gateway;
pub mod local;
pub mod openai_compat;
pub mod prompt;
pub mod validate;

use crate::schema::ExtractionResult;
use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;

#[async_trait]
pub trait ExtractionProvider: Send + Sync {
    fn name(&self) -> &'static str;
    async fn extract(&self, text: &str) -> Result<ExtractionResult>;
}

pub fn build_default() -> Arc<dyn ExtractionProvider> {
    let explicit = std::env::var("CAIRN_EXTRACTION_PROVIDER")
        .ok()
        .map(|s| s.to_lowercase());

    let chosen = match explicit.as_deref() {
        Some("gateway") | Some("vercel") => "gateway",
        Some("anthropic") | Some("claude") => "anthropic",
        Some("local") | Some("ondevice") | Some("on-device") => "local",
        Some(other) if !other.is_empty() => {
            tracing::warn!(
                provider = other,
                "unknown CAIRN_EXTRACTION_PROVIDER; auto-selecting"
            );
            ""
        }
        _ => "",
    };

    let resolved = if chosen.is_empty() {
        if std::env::var("AI_GATEWAY_API_KEY").is_ok() || std::env::var("AI_GATEWAY_TOKEN").is_ok()
        {
            "gateway"
        } else if std::env::var("ANTHROPIC_API_KEY").is_ok() {
            "anthropic"
        } else {
            ""
        }
    } else {
        chosen
    };

    let model = std::env::var("CAIRN_EXTRACTION_MODEL").ok();

    match resolved {
        "gateway" => {
            let m = model.unwrap_or_else(|| gateway::DEFAULT_MODEL.to_string());
            match gateway::GatewayProvider::new(m.clone()) {
                Ok(p) => {
                    tracing::info!(provider = "gateway", model = %m, "extraction ready");
                    return Arc::new(p);
                }
                Err(e) => tracing::error!(error = %e, "gateway init failed"),
            }
        }
        "anthropic" => {
            let m = model.unwrap_or_else(|| "claude-haiku-4-5-20251001".to_string());
            tracing::info!(provider = "anthropic", model = %m, "extraction ready");
            return Arc::new(anthropic::AnthropicProvider::new(m));
        }
        "local" => {
            let m = model.unwrap_or_else(|| local::DEFAULT_MODEL.to_string());
            let endpoint = std::env::var("CAIRN_LOCAL_ENDPOINT")
                .unwrap_or_else(|_| local::DEFAULT_ENDPOINT.to_string());
            match local::LocalProvider::new(m.clone(), endpoint.clone()) {
                Ok(p) => {
                    tracing::info!(
                        provider = "local",
                        model = %m,
                        endpoint = %endpoint,
                        "extraction ready"
                    );
                    return Arc::new(p);
                }
                Err(e) => tracing::error!(error = %e, "local init failed"),
            }
        }
        _ => {}
    }

    tracing::warn!("no extraction credentials; using NullProvider (extraction will no-op)");
    Arc::new(NullProvider)
}

pub struct NullProvider;

#[async_trait]
impl ExtractionProvider for NullProvider {
    fn name(&self) -> &'static str {
        "null"
    }
    async fn extract(&self, _text: &str) -> Result<ExtractionResult> {
        Ok(ExtractionResult {
            entities: vec![],
            relations: vec![],
        })
    }
}
