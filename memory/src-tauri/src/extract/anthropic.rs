//! Anthropic Claude extraction provider.
//!
//! Uses Claude's `tool_use` API for schema-constrained decoding:
//! we define a `save_extraction` tool whose `input_schema` mirrors our
//! `EXTRACTION_SCHEMA`, and force `tool_choice` to that tool. Claude is
//! therefore guaranteed to produce JSON that matches the schema (modulo
//! the JSON-Schema features the API supports). We re-validate locally
//! as defence in depth.

use crate::extract::prompt::{
    EXTRACTION_SCHEMA, FEW_SHOT_TOOL_1, FEW_SHOT_TOOL_2, FEW_SHOT_USER_1, FEW_SHOT_USER_2, SYSTEM,
};
use crate::extract::validate;
use crate::extract::ExtractionProvider;
use crate::schema::ExtractionResult;
use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use serde_json::{json, Value};

const API_URL: &str = "https://api.anthropic.com/v1/messages";
const TOOL_NAME: &str = "save_extraction";
const MAX_RETRIES: u32 = 2;

pub struct AnthropicProvider {
    client: reqwest::Client,
    model: String,
    api_key: Option<String>,
}

impl AnthropicProvider {
    pub fn new(model: String) -> Self {
        Self::with_key(model, None)
    }

    pub fn with_key(model: String, api_key: Option<String>) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .expect("build reqwest client");
        Self {
            client,
            model,
            api_key,
        }
    }

    fn build_request(&self, text: &str) -> Value {
        let tools = json!([
            {
                "name": TOOL_NAME,
                "description": "Save extracted entities and relations from the user's note.",
                "input_schema": *EXTRACTION_SCHEMA,
            }
        ]);

        let messages = json!([
            { "role": "user", "content": FEW_SHOT_USER_1 },
            {
                "role": "assistant",
                "content": [{
                    "type": "tool_use",
                    "id": "toolu_few_shot_1",
                    "name": TOOL_NAME,
                    "input": *FEW_SHOT_TOOL_1
                }]
            },
            {
                "role": "user",
                "content": [{
                    "type": "tool_result",
                    "tool_use_id": "toolu_few_shot_1",
                    "content": "ok"
                }]
            },
            { "role": "user", "content": FEW_SHOT_USER_2 },
            {
                "role": "assistant",
                "content": [{
                    "type": "tool_use",
                    "id": "toolu_few_shot_2",
                    "name": TOOL_NAME,
                    "input": *FEW_SHOT_TOOL_2
                }]
            },
            {
                "role": "user",
                "content": [{
                    "type": "tool_result",
                    "tool_use_id": "toolu_few_shot_2",
                    "content": "ok"
                }]
            },
            { "role": "user", "content": text }
        ]);

        json!({
            "model": self.model,
            "max_tokens": 2048,
            "system": SYSTEM,
            "tools": tools,
            "tool_choice": { "type": "tool", "name": TOOL_NAME },
            "messages": messages,
        })
    }

    async fn call_once(&self, text: &str) -> Result<Value> {
        let key = match &self.api_key {
            Some(k) if !k.is_empty() => k.clone(),
            _ => std::env::var("ANTHROPIC_API_KEY").context("ANTHROPIC_API_KEY not set")?,
        };
        let body = self.build_request(text);

        let resp = self
            .client
            .post(API_URL)
            .header("x-api-key", key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .context("anthropic request")?;

        let status = resp.status();
        let body: Value = resp.json().await.context("decode anthropic response")?;
        if !status.is_success() {
            return Err(anyhow!("anthropic error {}: {}", status, body));
        }

        // Find the tool_use block
        let blocks = body
            .get("content")
            .and_then(|c| c.as_array())
            .ok_or_else(|| anyhow!("no content blocks: {}", body))?;
        for b in blocks {
            if b.get("type").and_then(|v| v.as_str()) == Some("tool_use")
                && b.get("name").and_then(|v| v.as_str()) == Some(TOOL_NAME)
            {
                if let Some(input) = b.get("input") {
                    return Ok(input.clone());
                }
            }
        }
        Err(anyhow!(
            "model did not call {} tool; got: {}",
            TOOL_NAME,
            body
        ))
    }
}

#[async_trait]
impl ExtractionProvider for AnthropicProvider {
    fn name(&self) -> &'static str {
        "anthropic"
    }

    async fn extract(&self, text: &str) -> Result<ExtractionResult> {
        let mut last_err: Option<anyhow::Error> = None;
        for attempt in 0..=MAX_RETRIES {
            match self.call_once(text).await {
                Ok(json_value) => {
                    if let Err(e) = validate::validate(&json_value) {
                        tracing::warn!(?attempt, "schema validation failed: {e}");
                        last_err = Some(e);
                        continue;
                    }
                    let parsed: ExtractionResult = serde_json::from_value(json_value)
                        .context("deserialize ExtractionResult")?;
                    return Ok(parsed);
                }
                Err(e) => {
                    tracing::warn!(?attempt, "anthropic call failed: {e}");
                    last_err = Some(e);
                    tokio::time::sleep(std::time::Duration::from_millis(500 * (1u64 << attempt)))
                        .await;
                }
            }
        }
        Err(last_err.unwrap_or_else(|| anyhow!("extraction failed after retries")))
    }
}
