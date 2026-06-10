//! Phase 6d — user-facing AI provider configuration.
//!
//! Replaces the env-only setup (`AI_GATEWAY_API_KEY` + `CAIRN_*_MODEL`) with a
//! UI-driven JSON file at `~/Library/Application Support/Cairn/providers.json`.
//! Two independent slots:
//!
//!   * `extract` — chat-completion provider for entity / relation extraction.
//!   * `embed`   — embedding provider for vector search.
//!
//! Either slot can be `None` → that capability is disabled. With both `None`,
//! Cairn runs as a pure BM25 logbook: capture works, full-text search works,
//! audit works, but there are no entities, themes, or vector hits.
//!
//! Most of the 10 supported families speak the OpenAI chat-completions API
//! shape. Anthropic has its own Messages API. Gemini exposes both a native
//! API and an OpenAI-compat shim under `/v1beta/openai/`; we use the shim so
//! everything routes through one client.

use crate::embed::{gateway::GatewayEmbeddingProvider, EmbeddingProvider};
use crate::extract::{
    anthropic::AnthropicProvider, gateway::GatewayProvider, ExtractionProvider, NullProvider,
};
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// JSON file name in the data dir.
pub const PROVIDERS_FILE: &str = "providers.json";

/// Top-level config persisted to `providers.json`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProvidersConfig {
    /// Provider for entity / relation extraction. `None` → AI extraction
    /// disabled (BM25-only mode).
    #[serde(default)]
    pub extract: Option<ProviderConfig>,
    /// Provider for embeddings (vector search). `None` → vector search
    /// disabled, falls back to BM25 alone.
    #[serde(default)]
    pub embed: Option<ProviderConfig>,
}

/// One provider slot: family + key + model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub family: ProviderFamily,
    /// API key / bearer token. Stored in plain JSON for now (TODO: keychain).
    pub api_key: String,
    /// Model id passed through to the API. UI prefills from preset.
    pub model: String,
    /// Override the family's default base URL. Almost always `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url_override: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProviderFamily {
    VercelGateway,
    OpenRouter,
    OpenAi,
    Anthropic,
    Gemini,
    Doubao,
    Minimax,
    Moonshot,
    Glm,
    Qwen,
}

/// Static info Cairn needs to build a working provider client + render the
/// settings UI for a given family.
#[derive(Debug, Clone, Copy)]
pub struct ProviderPreset {
    pub family: ProviderFamily,
    pub display_name: &'static str,
    pub api_base: &'static str,
    pub default_extract_model: &'static str,
    /// `None` when the provider doesn't have a public embeddings API. UI
    /// then nudges the user to pick a different embedding provider.
    pub default_embed_model: Option<&'static str>,
    /// `true` when /v1/chat/completions is the same OpenAI-compatible shape
    /// we already drive with `OpenAiCompatProvider`. Only Anthropic is `false`.
    pub openai_compatible_chat: bool,
}

pub const PRESETS: &[ProviderPreset] = &[
    ProviderPreset {
        family: ProviderFamily::VercelGateway,
        display_name: "Vercel AI Gateway",
        api_base: "https://ai-gateway.vercel.sh/v1",
        default_extract_model: "anthropic/claude-haiku-4-5-20251001",
        default_embed_model: Some("openai/text-embedding-3-small"),
        openai_compatible_chat: true,
    },
    ProviderPreset {
        family: ProviderFamily::OpenRouter,
        display_name: "OpenRouter",
        api_base: "https://openrouter.ai/api/v1",
        default_extract_model: "anthropic/claude-3.5-haiku",
        default_embed_model: None, // OpenRouter is chat-only
        openai_compatible_chat: true,
    },
    ProviderPreset {
        family: ProviderFamily::OpenAi,
        display_name: "OpenAI",
        api_base: "https://api.openai.com/v1",
        default_extract_model: "gpt-4o-mini",
        default_embed_model: Some("text-embedding-3-small"),
        openai_compatible_chat: true,
    },
    ProviderPreset {
        family: ProviderFamily::Anthropic,
        display_name: "Anthropic (Claude)",
        api_base: "https://api.anthropic.com",
        default_extract_model: "claude-haiku-4-5-20251001",
        default_embed_model: None, // Anthropic doesn't ship embeddings
        openai_compatible_chat: false,
    },
    ProviderPreset {
        family: ProviderFamily::Gemini,
        // Google's OpenAI-compatible shim — same shape we already drive.
        display_name: "Google Gemini",
        api_base: "https://generativelanguage.googleapis.com/v1beta/openai",
        default_extract_model: "gemini-2.0-flash",
        default_embed_model: Some("text-embedding-004"),
        openai_compatible_chat: true,
    },
    ProviderPreset {
        family: ProviderFamily::Doubao,
        display_name: "豆包 (字节 Ark)",
        api_base: "https://ark.cn-beijing.volces.com/api/v3",
        default_extract_model: "doubao-1-5-pro-32k-250115",
        default_embed_model: Some("doubao-embedding-text-240715"),
        openai_compatible_chat: true,
    },
    ProviderPreset {
        family: ProviderFamily::Minimax,
        display_name: "Minimax",
        api_base: "https://api.minimax.chat/v1",
        default_extract_model: "abab6.5s-chat",
        default_embed_model: Some("embo-01"),
        openai_compatible_chat: true,
    },
    ProviderPreset {
        family: ProviderFamily::Moonshot,
        display_name: "Kimi (Moonshot)",
        api_base: "https://api.moonshot.cn/v1",
        default_extract_model: "moonshot-v1-8k",
        default_embed_model: None, // Moonshot has no public embedding API
        openai_compatible_chat: true,
    },
    ProviderPreset {
        family: ProviderFamily::Glm,
        display_name: "智谱 GLM",
        api_base: "https://open.bigmodel.cn/api/paas/v4",
        default_extract_model: "glm-4-flash",
        default_embed_model: Some("embedding-3"),
        openai_compatible_chat: true,
    },
    ProviderPreset {
        family: ProviderFamily::Qwen,
        display_name: "通义千问 (DashScope)",
        api_base: "https://dashscope.aliyuncs.com/compatible-mode/v1",
        default_extract_model: "qwen-plus",
        default_embed_model: Some("text-embedding-v3"),
        openai_compatible_chat: true,
    },
];

impl ProviderFamily {
    pub fn preset(self) -> &'static ProviderPreset {
        PRESETS
            .iter()
            .find(|p| p.family == self)
            .expect("every ProviderFamily must have a preset")
    }
}

impl ProviderConfig {
    /// Base URL the client should hit — `base_url_override` wins over the
    /// preset.
    pub fn base_url(&self) -> String {
        self.base_url_override
            .clone()
            .unwrap_or_else(|| self.family.preset().api_base.to_string())
    }

    /// Build the extractor for this config.
    pub fn build_extractor(&self) -> Result<Arc<dyn ExtractionProvider>> {
        if self.api_key.trim().is_empty() {
            return Err(anyhow!("api_key is empty"));
        }
        match self.family {
            ProviderFamily::Anthropic => Ok(Arc::new(AnthropicProvider::with_key(
                self.model.clone(),
                Some(self.api_key.clone()),
            ))),
            // Every other family speaks OpenAI-compatible chat-completions.
            // `GatewayProvider::with_config` is the shared client — same code,
            // different base URL / token.
            _ => {
                let provider = GatewayProvider::with_config(
                    self.api_key.clone(),
                    self.model.clone(),
                    self.base_url(),
                    family_static_label(self.family),
                )?;
                Ok(Arc::new(provider))
            }
        }
    }

    /// Build the embedder for this config. Errors if the family has no
    /// public embedding API (caller should let the user pick a different
    /// embedding provider).
    pub fn build_embedder(&self, dim: usize) -> Result<Arc<dyn EmbeddingProvider>> {
        if self.api_key.trim().is_empty() {
            return Err(anyhow!("api_key is empty"));
        }
        let preset = self.family.preset();
        if preset.default_embed_model.is_none() {
            return Err(anyhow!(
                "{} does not provide an embedding API — pick a different embedding provider",
                preset.display_name
            ));
        }
        // All families with embedding support speak the OpenAI embeddings
        // shape. The same client just needs a different base URL + token.
        let provider = GatewayEmbeddingProvider::with_config(
            self.api_key.clone(),
            self.model.clone(),
            dim,
            self.base_url(),
            family_static_label(self.family).to_string(),
        )?;
        Ok(Arc::new(provider))
    }
}

/// Stable short label per family. Used as the `provider` tag in usage
/// telemetry so the `/usage` page can break costs down by provider.
fn family_static_label(f: ProviderFamily) -> &'static str {
    match f {
        ProviderFamily::VercelGateway => "vercel-gateway",
        ProviderFamily::OpenRouter => "openrouter",
        ProviderFamily::OpenAi => "openai",
        ProviderFamily::Anthropic => "anthropic",
        ProviderFamily::Gemini => "gemini",
        ProviderFamily::Doubao => "doubao",
        ProviderFamily::Minimax => "minimax",
        ProviderFamily::Moonshot => "moonshot",
        ProviderFamily::Glm => "glm",
        ProviderFamily::Qwen => "qwen",
    }
}

// ---------- persistence ----------

pub fn config_path(data_dir: &Path) -> PathBuf {
    data_dir.join(PROVIDERS_FILE)
}

pub fn load(data_dir: &Path) -> ProvidersConfig {
    let p = config_path(data_dir);
    std::fs::read_to_string(&p)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save(data_dir: &Path, cfg: &ProvidersConfig) -> Result<()> {
    let p = config_path(data_dir);
    let json = serde_json::to_string_pretty(cfg)?;
    std::fs::write(&p, json)?;
    Ok(())
}

// ---------- backwards-compat: env fallback ----------

/// If `providers.json` doesn't exist yet, fall back to env vars so existing
/// `.env` setups keep working without explicit migration.
pub fn config_or_env_default(data_dir: &Path) -> ProvidersConfig {
    let from_file = load(data_dir);
    if from_file.extract.is_some() || from_file.embed.is_some() {
        return from_file;
    }
    // No file yet — synthesise from env vars.
    let mut cfg = ProvidersConfig::default();
    if let Ok(key) =
        std::env::var("AI_GATEWAY_API_KEY").or_else(|_| std::env::var("AI_GATEWAY_TOKEN"))
    {
        let model = std::env::var("CAIRN_EXTRACTION_MODEL").unwrap_or_else(|_| {
            ProviderFamily::VercelGateway
                .preset()
                .default_extract_model
                .to_string()
        });
        cfg.extract = Some(ProviderConfig {
            family: ProviderFamily::VercelGateway,
            api_key: key.clone(),
            model,
            base_url_override: None,
        });
        let embed_model = std::env::var("CAIRN_EMBEDDING_MODEL").unwrap_or_else(|_| {
            ProviderFamily::VercelGateway
                .preset()
                .default_embed_model
                .unwrap()
                .to_string()
        });
        cfg.embed = Some(ProviderConfig {
            family: ProviderFamily::VercelGateway,
            api_key: key,
            model: embed_model,
            base_url_override: None,
        });
    }
    cfg
}

/// Hot-swappable bundle of (extractor, embedder, embedding_model_id).
///
/// All long-lived callers (Retriever, McpBridge, ApiState, capture pipeline)
/// hold an `Arc<LiveProviders>` and re-fetch the current provider for every
/// operation. Provider config changes from `/settings` rebuild the inner
/// `Arc`s and swap them in atomically — no Cairn restart required.
pub struct LiveProviders {
    inner: parking_lot::RwLock<LiveInner>,
    embedding_dim: usize,
}

struct LiveInner {
    extractor: Arc<dyn crate::extract::ExtractionProvider>,
    embedder: Arc<dyn crate::embed::EmbeddingProvider>,
    embedding_model_id: String,
}

impl LiveProviders {
    pub fn new(cfg: &ProvidersConfig, embedding_dim: usize) -> Arc<Self> {
        let (extractor, embedder, embedding_model_id) = build_runtime(cfg, embedding_dim);
        let embedder = embedder.unwrap_or_else(|| {
            Arc::new(crate::embed::NullEmbeddingProvider::new(embedding_dim))
                as Arc<dyn crate::embed::EmbeddingProvider>
        });
        Arc::new(Self {
            inner: parking_lot::RwLock::new(LiveInner {
                extractor,
                embedder,
                embedding_model_id,
            }),
            embedding_dim,
        })
    }

    pub fn extractor(&self) -> Arc<dyn crate::extract::ExtractionProvider> {
        self.inner.read().extractor.clone()
    }

    pub fn embedder(&self) -> Arc<dyn crate::embed::EmbeddingProvider> {
        self.inner.read().embedder.clone()
    }

    pub fn embedding_model_id(&self) -> String {
        self.inner.read().embedding_model_id.clone()
    }

    /// Rebuild from the given config and atomically swap the live handles.
    /// Returns `Ok(())` always — failed extractor/embedder builds fall
    /// back to no-op providers (BM25-only mode) instead of erroring.
    pub fn reload(&self, cfg: &ProvidersConfig) {
        let (extractor, embedder, embedding_model_id) = build_runtime(cfg, self.embedding_dim);
        let embedder = embedder.unwrap_or_else(|| {
            Arc::new(crate::embed::NullEmbeddingProvider::new(self.embedding_dim))
                as Arc<dyn crate::embed::EmbeddingProvider>
        });
        let mut w = self.inner.write();
        w.extractor = extractor;
        w.embedder = embedder;
        w.embedding_model_id = embedding_model_id;
    }
}

/// Build the (extractor, embedder, embedding_model_id) trio from a config.
/// Errors are swallowed by falling back to no-op providers so the GUI always
/// boots — the user can fix the config in /settings without restarting.
pub fn build_runtime(
    cfg: &ProvidersConfig,
    embedding_dim: usize,
) -> (
    Arc<dyn ExtractionProvider>,
    Option<Arc<dyn EmbeddingProvider>>,
    String,
) {
    let extractor = cfg
        .extract
        .as_ref()
        .and_then(|c| match c.build_extractor() {
            Ok(p) => Some(p),
            Err(e) => {
                tracing::warn!(error = %e, family = ?c.family, "extractor build failed");
                None
            }
        })
        .unwrap_or_else(|| Arc::new(NullProvider) as Arc<dyn ExtractionProvider>);

    let (embedder, model_id) = cfg
        .embed
        .as_ref()
        .and_then(|c| match c.build_embedder(embedding_dim) {
            Ok(e) => Some((e, c.model.clone())),
            Err(e) => {
                tracing::warn!(error = %e, family = ?c.family, "embedder build failed");
                None
            }
        })
        .map(|(e, m)| (Some(e), m))
        .unwrap_or_else(|| (None, String::new()));

    (extractor, embedder, model_id)
}
