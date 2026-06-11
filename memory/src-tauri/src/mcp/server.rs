//! JSON-RPC 2.0 over stdio implementation of the MCP server.
//!
//! Lifecycle:
//!   1. Client → `initialize` → Server replies with capabilities.
//!   2. Client → `notifications/initialized` (notification, no response).
//!   3. Client → `tools/list` → Server replies with `list_tools()` payload.
//!   4. Client → `tools/call` (repeated) → Server runs tool, audits, replies.
//!
//! All log output goes to stderr because stdout is the protocol transport.

use crate::audit::AuditLogger;
use crate::db::Db;
use crate::mcp::tools;
use crate::retrieval::Retriever;
use anyhow::Result;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

const PROTOCOL_VERSION: &str = "2025-11-25";
const SERVER_NAME: &str = "cairn";
const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Clone)]
pub struct McpContext {
    pub db: Db,
    pub retriever: Retriever,
    pub audit: AuditLogger,
    /// Default agent_id when client doesn't supply one (e.g. Claude Desktop launches
    /// the binary directly without identifying itself). Configured via env.
    pub default_agent_id: String,
}

impl McpContext {
    pub fn new(db: Db, audit: AuditLogger, retriever: Retriever) -> Self {
        let default_agent_id =
            std::env::var("CAIRN_AGENT_ID").unwrap_or_else(|_| "claude-desktop".to_string());
        Self {
            db,
            retriever,
            audit,
            default_agent_id,
        }
    }
}

pub async fn serve_stdio(ctx: McpContext) -> Result<()> {
    let stdin = tokio::io::stdin();
    let mut reader = BufReader::new(stdin).lines();
    let mut stdout = tokio::io::stdout();

    tracing::info!("MCP stdio server up");

    while let Some(line) = reader.next_line().await? {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let request: Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(error=%e, "bad json on stdin");
                continue;
            }
        };

        if let Some(response) = process_message(&ctx, &request).await {
            let serialised = serde_json::to_string(&response)?;
            stdout.write_all(serialised.as_bytes()).await?;
            stdout.write_all(b"\n").await?;
            stdout.flush().await?;
        }
    }
    Ok(())
}

/// Transport-agnostic dispatcher. Returns `Some(response)` for requests
/// (anything with an `id`) and `None` for notifications. Used by both the
/// stdio and SSE transports.
pub async fn process_message(ctx: &McpContext, request: &Value) -> Option<Value> {
    let is_notification = request.get("id").is_none();
    let method = request
        .get("method")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let id = request.get("id").cloned();
    let params = request.get("params").cloned().unwrap_or(Value::Null);

    if is_notification {
        handle_notification(&method, &params).await;
        return None;
    }

    let response = match handle_request(ctx, &method, &params).await {
        Ok(result) => json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": result
        }),
        Err(err) => json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": {
                "code": -32000,
                "message": err.to_string()
            }
        }),
    };
    Some(response)
}

async fn handle_notification(method: &str, params: &Value) {
    tracing::debug!(method, ?params, "notification");
    // currently a no-op for `notifications/initialized` etc.
}

async fn handle_request(ctx: &McpContext, method: &str, params: &Value) -> Result<Value> {
    match method {
        "initialize" => Ok(json!({
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": {
                "tools": { "listChanged": false }
            },
            "serverInfo": {
                "name": SERVER_NAME,
                "version": SERVER_VERSION
            },
            "instructions": "Cairn — the owner's personal memory. Markers of where they've been, structured by AI, exposed to you under explicit permissions. Use `search_memory` first for any question about the owner's life, preferences, people, or goals. Use `get_themes` for higher-signal questions about recurring topics."
        })),
        "tools/list" => {
            // Audit the listing so we know which agents probed the surface.
            let _ = ctx
                .audit
                .log(
                    &ctx.default_agent_id,
                    "tools/list",
                    None,
                    None,
                    &Value::Null,
                    None,
                )
                .await;
            let _ = ctx.db.touch_agent(&ctx.default_agent_id).await;
            Ok(tools::list_tools())
        }
        "tools/call" => handle_tools_call(ctx, params).await,
        "ping" => Ok(json!({})),
        other => Err(anyhow::anyhow!("method not supported: {}", other)),
    }
}

async fn handle_tools_call(ctx: &McpContext, params: &Value) -> Result<Value> {
    let name = params
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("missing tool name"))?;
    let arguments = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));

    let agent_id = &ctx.default_agent_id;
    ctx.db.touch_agent(agent_id).await.ok();

    let args_str = serde_json::to_string(&arguments).unwrap_or_default();
    let args_truncated = if args_str.len() > 4096 {
        // Snap to a char boundary — a raw `&args_str[..4096]` panics when a
        // multi-byte char (CJK note content) straddles the cut.
        let mut end = 4096;
        while !args_str.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}...[truncated]", &args_str[..end])
    } else {
        args_str
    };

    let outcome = tools::call_tool(
        &ctx.db,
        &ctx.retriever,
        &ctx.audit,
        agent_id,
        name,
        &arguments,
    )
    .await;

    let (response_content, audit_action, result_for_audit, count_for_audit) = match outcome {
        Ok(o) => {
            let payload = serde_json::to_string_pretty(&o.result)?;
            (
                json!({
                    "content": [
                        { "type": "text", "text": payload }
                    ],
                    "isError": false
                }),
                "tools/call",
                o.result,
                o.result_count,
            )
        }
        Err(e) => (
            json!({
                "content": [
                    { "type": "text", "text": format!("error: {}", e) }
                ],
                "isError": true
            }),
            "tools/call/error",
            json!({ "error": e.to_string() }),
            None,
        ),
    };

    let _ = ctx
        .audit
        .log(
            agent_id,
            audit_action,
            Some(name),
            Some(&args_truncated),
            &result_for_audit,
            count_for_audit,
        )
        .await;

    Ok(response_content)
}
