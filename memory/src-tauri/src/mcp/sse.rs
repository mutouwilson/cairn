//! MCP server over SSE/HTTP transport.
//!
//! Lets remote MCP clients (ChatGPT's "Connectors", anything else that speaks
//! the MCP SSE flavour) connect to Cairn over a public URL. We deliberately
//! support the older two-endpoint SSE shape (GET /sse + POST /messages) since
//! that's what the ChatGPT custom MCP UI expects today.
//!
//! Flow per client session:
//!   1. Client opens `GET /sse`. We allocate a session id, register an mpsc
//!      sender, and reply with an SSE stream. The first SSE event has
//!      `event: endpoint` whose data is the POST URL for that session, e.g.
//!      `/messages?session_id=<uuid>`.
//!   2. Client POSTs JSON-RPC frames to `/messages?session_id=<uuid>`. We
//!      dispatch via `process_message`, push the response into the session's
//!      sender, and return `202 Accepted`.
//!   3. The SSE stream pumps every queued response back to the client as
//!      `event: message` frames.
//!   4. Disconnect cleans up the session.
//!
//! **Auth is intentionally off here** (the "未授权 / None" mode in ChatGPT's
//! form). The tunnel URL is the secret — treat it like a bearer token.

// `crate::api` is no longer mounted here — see `crate::api_server`.
use crate::mcp::server::{process_message, McpContext};
use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::sse::{Event, KeepAlive, Sse},
    routing::{get, post},
    Json, Router,
};
use parking_lot::Mutex;
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::StreamExt;
use tower_http::cors::{Any, CorsLayer};

/// Per-session state. `agent_id` is captured from the `?agent_id=…` query
/// param the client passed when opening the SSE stream — Cursor / others
/// identify themselves that way. Empty means "use the bridge's default
/// (env-configured) agent id".
#[derive(Clone)]
struct SessionState {
    sender: mpsc::Sender<Value>,
    agent_id: Option<String>,
}

type SessionMap = Arc<Mutex<HashMap<String, SessionState>>>;

#[derive(Clone)]
struct AppState {
    ctx: Arc<McpContext>,
    sessions: SessionMap,
}

pub async fn serve_sse(ctx: McpContext, addr: SocketAddr) -> anyhow::Result<()> {
    serve_sse_with_shutdown(ctx, addr, None, std::future::pending::<()>()).await
}

/// Build the shared `ApiState` from a `McpContext` + optional live
/// providers handle. Used by both the in-app bridge (where providers are
/// always present) and the standalone `cairn-mcp` binary (where they
/// aren't — that build is read-only).
fn build_api_state(
    ctx: &McpContext,
    providers: Option<std::sync::Arc<crate::providers::LiveProviders>>,
) -> crate::api::ApiState {
    crate::api::ApiState {
        db: ctx.db.clone(),
        audit: ctx.audit.clone(),
        retriever: ctx.retriever.clone(),
        providers,
    }
}

/// Same as [`serve_sse`] but stops cleanly when `shutdown` completes.
/// Used by the in-app supervisor (`mcp_bridge`) so the user can toggle
/// the remote MCP server on and off from /settings without restarting Cairn.
///
/// `providers` is the live extraction + embedding handle. When supplied,
/// the `/api/capture` HTTP endpoint runs the full extraction pipeline on
/// every new note — the browser extension and any other HTTP caller get
/// behaviour identical to the Tauri `capture_note` path, and provider
/// changes from `/settings` apply immediately. The standalone
/// `cairn-mcp --transport sse` binary leaves it `None` because it doesn't
/// own the embedder / extractor — those live with the GUI.
pub async fn serve_sse_with_shutdown<F>(
    ctx: McpContext,
    addr: SocketAddr,
    providers: Option<std::sync::Arc<crate::providers::LiveProviders>>,
    shutdown: F,
) -> anyhow::Result<()>
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    // `providers` is still accepted for ABI parity with the standalone
    // `cairn-mcp` binary that used to bundle the API on this listener.
    // In-app, the API now runs on its own port from `api_server::boot`.
    let _ = build_api_state(&ctx, providers);
    let state = AppState {
        ctx: Arc::new(ctx),
        sessions: Arc::new(Mutex::new(HashMap::new())),
    };

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        .route("/sse", get(sse_handler))
        .route("/messages", post(messages_handler))
        .route("/health", get(|| async { "ok" }))
        .with_state(state)
        .layer(cors);

    tracing::info!(?addr, "MCP SSE server listening — endpoint at /sse");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown)
        .await?;
    tracing::info!(?addr, "MCP SSE server stopped");
    Ok(())
}

/// `GET /sse` — open an SSE stream. We synthesise a session id, register
/// the sender, and send the `endpoint` event so the client knows where to
/// POST its JSON-RPC.
#[derive(Deserialize)]
struct SseConnectQuery {
    /// Identifier the client passes to be tracked as a distinct agent
    /// (e.g. `cursor`, `windsurf`). Optional — when absent we fall back to
    /// the bridge's env-configured default.
    agent_id: Option<String>,
}

async fn sse_handler(
    State(state): State<AppState>,
    Query(q): Query<SseConnectQuery>,
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>> {
    let session_id = ulid::Ulid::new().to_string();
    let (tx, rx) = mpsc::channel::<Value>(64);
    let agent_id = q
        .agent_id
        .as_ref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    state.sessions.lock().insert(
        session_id.clone(),
        SessionState {
            sender: tx,
            agent_id: agent_id.clone(),
        },
    );
    tracing::info!(%session_id, ?agent_id, "SSE session opened");

    // Make sure the agent row exists so future touch_agent calls actually
    // bump last_seen (UPDATE on a missing row is a no-op).
    if let Some(id) = agent_id {
        let db = state.ctx.db.clone();
        tokio::spawn(async move {
            if let Err(e) = db.upsert_agent(&id, &id).await {
                tracing::warn!(?e, agent_id = %id, "upsert_agent failed");
            }
        });
    }

    // First event: tell the client the POST URL for this session.
    let endpoint = format!("/messages?session_id={}", session_id);
    let endpoint_evt = std::iter::once(Ok(Event::default().event("endpoint").data(endpoint)));

    let sessions_for_cleanup = state.sessions.clone();
    let session_for_cleanup = session_id.clone();

    let message_stream = ReceiverStream::new(rx).map(|msg| {
        let body = serde_json::to_string(&msg).unwrap_or_else(|_| "{}".to_string());
        Ok::<Event, Infallible>(Event::default().event("message").data(body))
    });

    let combined = tokio_stream::iter(endpoint_evt)
        .chain(message_stream)
        .chain({
            // Cleanup runs when the stream is dropped.
            futures::stream::once(async move {
                sessions_for_cleanup.lock().remove(&session_for_cleanup);
                tracing::info!(session_id = %session_for_cleanup, "SSE session closed");
                Ok::<Event, Infallible>(Event::default().event("close").data(""))
            })
        });

    Sse::new(combined).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keep-alive"),
    )
}

#[derive(Deserialize)]
struct MessagesQuery {
    session_id: String,
}

/// `POST /messages?session_id=...` — receive a JSON-RPC frame, dispatch via
/// the shared `process_message`, and (for non-notification messages) shove
/// the response into the matching session's SSE sender.
async fn messages_handler(
    State(state): State<AppState>,
    Query(q): Query<MessagesQuery>,
    Json(request): Json<Value>,
) -> StatusCode {
    let session = match state.sessions.lock().get(&q.session_id).cloned() {
        Some(s) => s,
        None => {
            tracing::warn!(session_id = %q.session_id, "POST for unknown session");
            return StatusCode::NOT_FOUND;
        }
    };

    // If the SSE client identified itself (`?agent_id=…`), override the
    // bridge-wide default for the duration of this call so audit + touch
    // attribute to the right agent.
    let ctx_for_call: Arc<McpContext> = if let Some(id) = session.agent_id.as_ref() {
        let mut overridden = (*state.ctx).clone();
        overridden.default_agent_id = id.clone();
        Arc::new(overridden)
    } else {
        state.ctx.clone()
    };

    if let Some(response) = process_message(&ctx_for_call, &request).await {
        if session.sender.send(response).await.is_err() {
            tracing::warn!(session_id = %q.session_id, "session channel closed before response could be delivered");
            return StatusCode::GONE;
        }
    }
    StatusCode::ACCEPTED
}
