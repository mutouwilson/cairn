//! LLM-driven topic summarisation. Schema-constrained via tool calling, same
//! transport as the extraction layer.

use crate::extract::openai_compat::{call_once_op, OpenAiCompatConfig};
use anyhow::{anyhow, Context, Result};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

const TOOL_NAME: &str = "save_topic_summary";

const SYSTEM: &str = "You are a memory consolidation worker. Given a topic and \
a list of episodic memories (Person/Event/Preference/Belief/Goal/Asset/Skill/Location) \
about that topic, produce ONE concise semantic summary. Speak about the owner's life \
factually: do not invent details. Capture trends and tensions when they are visible. \
Output 2–6 sentences via the `save_topic_summary` tool. Set `confidence` to reflect \
how consistent the evidence is.";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SummaryRequest {
    pub topic_key: String,
    pub topic_title: String,
    pub center_kind: Option<String>, // "Person" | "Goal" | None
    pub center_name: Option<String>,
    pub evidence_snippets: Vec<String>, // pre-formatted "[Type] Name → props"
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SummaryResponse {
    pub title: String,
    pub summary: String,
    pub confidence: f64,
}

static SCHEMA: Lazy<Value> = Lazy::new(|| {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["title", "summary", "confidence"],
        "properties": {
            "title": {
                "type": "string",
                "minLength": 1,
                "maxLength": 120,
                "description": "Short UI-facing label, ≤ 80 chars"
            },
            "summary": {
                "type": "string",
                "minLength": 1,
                "maxLength": 1500,
                "description": "2–6 sentence factual summary"
            },
            "confidence": {
                "type": "number",
                "minimum": 0.0,
                "maximum": 1.0
            }
        }
    })
});

pub async fn summarise(
    client: &reqwest::Client,
    cfg: &OpenAiCompatConfig,
    request: &SummaryRequest,
) -> Result<SummaryResponse> {
    let user_msg = build_user_message(request);

    let tools = json!([
        {
            "type": "function",
            "function": {
                "name": TOOL_NAME,
                "description": "Save the consolidated summary for one topic.",
                "parameters": *SCHEMA,
            }
        }
    ]);

    let body = json!({
        "model": cfg.model,
        "messages": [
            { "role": "system", "content": SYSTEM },
            { "role": "user", "content": user_msg }
        ],
        "tools": tools,
        "tool_choice": { "type": "function", "function": { "name": TOOL_NAME } },
        "temperature": 0.2,
        "max_tokens": 800,
    });

    // Use the operation-tagged variant so usage_log gets `operation=consolidate`.
    let raw = call_once_op(client, cfg, &body, "consolidate").await?;
    if let Some(arr) = raw.get("confidence").and_then(|v| v.as_f64()) {
        if !(0.0..=1.0).contains(&arr) {
            return Err(anyhow!("confidence {} out of range", arr));
        }
    }
    let parsed: SummaryResponse =
        serde_json::from_value(raw).context("deserialize SummaryResponse")?;
    Ok(parsed)
}

fn build_user_message(req: &SummaryRequest) -> String {
    let mut s = String::new();
    s.push_str(&format!("Topic: {} ({})\n", req.topic_title, req.topic_key));
    if let (Some(kind), Some(name)) = (&req.center_kind, &req.center_name) {
        s.push_str(&format!("Centred on: [{}] {}\n", kind, name));
    }
    s.push_str("\nEpisodic evidence:\n");
    for (i, ev) in req.evidence_snippets.iter().enumerate() {
        s.push_str(&format!("{}. {}\n", i + 1, ev));
    }
    s
}
