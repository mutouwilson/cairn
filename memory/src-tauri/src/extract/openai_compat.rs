//! Shared OpenAI-compatible chat-completions client used by both the Vercel AI
//! Gateway provider and the on-device (llama.cpp / MLX server) provider.
//!
//! All structured-output reliability lives here:
//!   - Function calling with a single `save_extraction` tool whose
//!     `parameters` is our `EXTRACTION_SCHEMA`.
//!   - `tool_choice` pinned to that tool.
//!   - The same few-shot pairs we use for the Anthropic provider, transcoded
//!     into the OpenAI message shape.
//!   - Returned JSON is re-validated against the schema before being parsed.

use crate::extract::prompt::{
    EXTRACTION_SCHEMA, FEW_SHOT_TOOL_1, FEW_SHOT_TOOL_2, FEW_SHOT_USER_1, FEW_SHOT_USER_2, SYSTEM,
};
use crate::extract::validate;
use crate::schema::ExtractionResult;
use anyhow::{anyhow, Context, Result};
use reqwest::Client;
use serde_json::{json, Value};

pub const TOOL_NAME: &str = "save_extraction";
const MAX_RETRIES: u32 = 2;

#[derive(Clone)]
pub struct OpenAiCompatConfig {
    /// Base URL without trailing slash, e.g. `https://ai-gateway.vercel.sh/v1`
    pub base_url: String,
    /// Auth header `Bearer <token>`. Empty string for unauthenticated local servers.
    pub api_key: String,
    /// Model id passed through to the gateway. For gateway: `provider/model-id`.
    pub model: String,
    /// `info`-level tag used in logs, e.g. "gateway" or "local".
    pub label: &'static str,
}

/// Generic OpenAI-compatible chat-completions call that expects exactly one
/// `tool_call`. Returns the parsed `function.arguments` JSON.
///
/// Used by both the extraction pipeline (`save_extraction` tool) and the
/// hierarchical-memory consolidation pipeline (`save_themes` tool).
pub async fn invoke_chat_tool(
    client: &Client,
    cfg: &OpenAiCompatConfig,
    body: Value,
) -> Result<Value> {
    let mut last_err: Option<anyhow::Error> = None;
    for attempt in 0..=MAX_RETRIES {
        match call_once(client, cfg, &body).await {
            Ok(v) => return Ok(v),
            Err(e) => {
                tracing::warn!(provider = cfg.label, attempt, "call failed: {e}");
                last_err = Some(e);
                tokio::time::sleep(std::time::Duration::from_millis(500 * (1u64 << attempt))).await;
            }
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow!("openai-compat call failed after retries")))
}

pub async fn extract(
    client: &Client,
    cfg: &OpenAiCompatConfig,
    text: &str,
) -> Result<ExtractionResult> {
    let body = build_request_body(&cfg.model, text);

    let mut last_err: Option<anyhow::Error> = None;
    for attempt in 0..=MAX_RETRIES {
        match call_once(client, cfg, &body).await {
            Ok(json_value) => match validate::validate(&json_value) {
                Ok(()) => {
                    let parsed: ExtractionResult = serde_json::from_value(json_value)
                        .context("deserialize ExtractionResult")?;
                    return Ok(parsed);
                }
                Err(e) => {
                    tracing::warn!(
                        provider = cfg.label,
                        attempt,
                        "schema validation failed: {e}"
                    );
                    last_err = Some(e);
                }
            },
            Err(e) => {
                tracing::warn!(provider = cfg.label, attempt, "call failed: {e}");
                last_err = Some(e);
                tokio::time::sleep(std::time::Duration::from_millis(500 * (1u64 << attempt))).await;
            }
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow!("extraction failed after retries")))
}

fn build_request_body(model: &str, text: &str) -> Value {
    let tools = json!([
        {
            "type": "function",
            "function": {
                "name": TOOL_NAME,
                "description": "Save extracted entities and relations from the user's note.",
                "parameters": *EXTRACTION_SCHEMA,
            }
        }
    ]);

    // Few-shot in OpenAI structured-tool-call shape.
    let messages = json!([
        { "role": "system", "content": SYSTEM },
        { "role": "user",   "content": FEW_SHOT_USER_1 },
        {
            "role": "assistant",
            "content": null,
            "tool_calls": [{
                "id": "fewshot_1",
                "type": "function",
                "function": {
                    "name": TOOL_NAME,
                    "arguments": serde_json::to_string(&*FEW_SHOT_TOOL_1).unwrap()
                }
            }]
        },
        { "role": "tool", "tool_call_id": "fewshot_1", "content": "ok" },
        { "role": "user", "content": FEW_SHOT_USER_2 },
        {
            "role": "assistant",
            "content": null,
            "tool_calls": [{
                "id": "fewshot_2",
                "type": "function",
                "function": {
                    "name": TOOL_NAME,
                    "arguments": serde_json::to_string(&*FEW_SHOT_TOOL_2).unwrap()
                }
            }]
        },
        { "role": "tool", "tool_call_id": "fewshot_2", "content": "ok" },
        { "role": "user", "content": text }
    ]);

    json!({
        "model": model,
        "messages": messages,
        "tools": tools,
        "tool_choice": {
            "type": "function",
            "function": { "name": TOOL_NAME }
        },
        "temperature": 0.0,
        "max_tokens": 2048,
    })
}

pub(crate) async fn call_once(
    client: &Client,
    cfg: &OpenAiCompatConfig,
    body: &Value,
) -> Result<Value> {
    call_once_op(client, cfg, body, "extract").await
}

/// Same call as `call_once` but lets the caller tag the operation for
/// telemetry. Returned value is the parsed tool arguments JSON.
pub(crate) async fn call_once_op(
    client: &Client,
    cfg: &OpenAiCompatConfig,
    body: &Value,
    operation: &'static str,
) -> Result<Value> {
    let url = format!("{}/chat/completions", cfg.base_url.trim_end_matches('/'));
    let mut req = client
        .post(&url)
        .header("content-type", "application/json")
        .json(body);
    if !cfg.api_key.is_empty() {
        req = req.header("authorization", format!("Bearer {}", cfg.api_key));
    }

    let started = std::time::Instant::now();
    let resp_res = req.send().await;
    let resp = match resp_res {
        Ok(r) => r,
        Err(e) => {
            crate::usage::record(crate::usage::UsageEvent {
                operation,
                provider: cfg.label.to_string(),
                model: cfg.model.clone(),
                prompt_tokens: None,
                completion_tokens: None,
                cost_micro_usd: None,
                latency_ms: started.elapsed().as_millis() as i64,
                success: false,
                error: Some(format!("transport: {e}")),
            });
            return Err(e).with_context(|| format!("{} request", cfg.label));
        }
    };
    let status = resp.status();
    let payload: Value = resp.json().await.context("decode response json")?;
    let latency_ms = started.elapsed().as_millis() as i64;
    if !status.is_success() {
        crate::usage::record(crate::usage::UsageEvent {
            operation,
            provider: cfg.label.to_string(),
            model: cfg.model.clone(),
            prompt_tokens: None,
            completion_tokens: None,
            cost_micro_usd: None,
            latency_ms,
            success: false,
            error: Some(format!("{} {}", status, payload)),
        });
        return Err(anyhow!("{} error {}: {}", cfg.label, status, payload));
    }

    let usage = payload.get("usage");
    let prompt_tokens = usage
        .and_then(|u| u.get("prompt_tokens"))
        .and_then(|v| v.as_i64());
    let completion_tokens = usage
        .and_then(|u| u.get("completion_tokens"))
        .and_then(|v| v.as_i64());
    let cost = usage
        .and_then(|u| u.get("cost"))
        .and_then(|v| v.as_f64())
        .and_then(crate::usage::usd_to_micro);
    crate::usage::record(crate::usage::UsageEvent {
        operation,
        provider: cfg.label.to_string(),
        model: cfg.model.clone(),
        prompt_tokens,
        completion_tokens,
        cost_micro_usd: cost,
        latency_ms,
        success: true,
        error: None,
    });

    let tool_call = payload
        .pointer("/choices/0/message/tool_calls/0/function/arguments")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("no tool_call.function.arguments in response: {}", payload))?;

    let parsed: Value = serde_json::from_str(tool_call)
        .with_context(|| format!("parse tool args: {}", tool_call))?;
    Ok(parsed)
}
