//! Plain HTTP REST API exposed alongside the MCP SSE bridge.
//!
//! Designed for callers that don't (or can't) speak MCP — the browser
//! extension being the primary user. Endpoints intentionally mirror a
//! subset of the MCP tools so the extension can be a thin layer on top:
//!
//!   * `POST /api/capture` — save a note (text + source + metadata).
//!   * `GET  /api/search?q=…&limit=…` — hybrid (BM25 + vector + RRF) search.
//!   * `GET  /api/recent?limit=…` — most-recent notes (for the sidebar).
//!   * `GET  /api/themes?limit=…` — top semantic memories.
//!   * `GET  /api/status` — quick "is cairn alive?" probe for the extension.
//!
//! Auth: none (CORS allow-any). Same threat model as the SSE bridge: lives
//! on `127.0.0.1`, intended for the user's own machine. If you tunnel it
//! publicly, the URL is the secret.

use crate::audit::AuditLogger;
use crate::capture::processor::{process_note, ProcessingContext};
use crate::db::Db;
use crate::providers::LiveProviders;
use crate::retrieval::Retriever;
use crate::schema::Note;
use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;

#[derive(Clone)]
pub struct ApiState {
    pub db: Db,
    pub audit: AuditLogger,
    pub retriever: Retriever,
    /// Live provider handles. `/api/capture` reads them on every request so
    /// settings changes take effect without restarting Cairn. `None` here
    /// (used by the standalone `cairn-mcp --transport sse` binary) means
    /// capture just stores raw notes without extraction.
    pub providers: Option<Arc<LiveProviders>>,
}

pub fn router(state: ApiState) -> Router {
    Router::new()
        .route("/api/status", get(status))
        .route("/api/capture", post(capture))
        .route("/api/search", get(search))
        .route("/api/recent", get(recent))
        .route("/api/themes", get(themes))
        .with_state(Arc::new(state))
}

// ---------- /api/status ----------

#[derive(Serialize)]
struct Status {
    ok: bool,
    server: &'static str,
    version: &'static str,
}

async fn status() -> Json<Status> {
    Json(Status {
        ok: true,
        server: "cairn",
        version: env!("CARGO_PKG_VERSION"),
    })
}

// ---------- /api/capture ----------

#[derive(Deserialize)]
struct CapturePayload {
    text: String,
    /// e.g. `"chatgpt"`, `"claude.ai"`, `"selection:com.google.Chrome"`,
    /// `"manual"`. Stored verbatim as the `notes.source` column.
    #[serde(default)]
    source: Option<String>,
    /// Optional metadata bag. We currently merge known keys into the source
    /// string so they survive: `url`, `title`, `agent`, `conversation_id`,
    /// `message_id`. Everything else gets dropped into the audit log only.
    #[serde(default)]
    metadata: Option<Value>,
}

#[derive(Serialize)]
struct CaptureResponse {
    note_id: String,
    source: String,
}

async fn capture(
    State(state): State<Arc<ApiState>>,
    Json(payload): Json<CapturePayload>,
) -> Result<Json<CaptureResponse>, ApiError> {
    if payload.text.trim().is_empty() {
        return Err(ApiError::bad_request("text is empty"));
    }

    let source = derive_source(payload.source.as_deref(), payload.metadata.as_ref());
    let note_id = state
        .db
        .insert_note(&payload.text, &source)
        .await
        .map_err(ApiError::internal)?;

    let _ = state
        .audit
        .log(
            "system",
            "capture/api",
            Some("capture"),
            Some(&format!("note={} source={}", note_id, source)),
            &json!({
                "note_id": note_id,
                "source": source,
                "metadata": payload.metadata,
                "bytes": payload.text.len(),
            }),
            Some(1),
        )
        .await;

    // Schedule LLM extraction so the note becomes structured (entities +
    // relations + embeddings), same as the Tauri capture_note path. Reads
    // the live provider handles every call, so a settings change is in
    // effect immediately — no Cairn restart needed.
    if let Some(providers) = state.providers.clone() {
        let ctx = ProcessingContext {
            db: state.db.clone(),
            extractor: providers.extractor(),
            embedder: providers.embedder(),
            audit: state.audit.clone(),
            embedding_model_id: providers.embedding_model_id(),
        };
        let note_id_for_task = note_id.clone();
        let text_for_task = payload.text.clone();
        tokio::spawn(async move {
            if let Err(e) = process_note(&ctx, &note_id_for_task, &text_for_task).await {
                tracing::warn!(error = %e, note_id = %note_id_for_task, "api capture: extraction failed");
            }
        });
    }

    Ok(Json(CaptureResponse { note_id, source }))
}

/// Build the canonical `notes.source` string from the caller's `source` hint
/// plus structured metadata. Keeps URL / agent / message id discoverable in
/// the timeline without us inventing a new column.
fn derive_source(source: Option<&str>, metadata: Option<&Value>) -> String {
    let base = source.unwrap_or("api").trim();
    let mut parts: Vec<String> = Vec::new();
    if let Some(meta) = metadata.and_then(|v| v.as_object()) {
        for key in ["agent", "conversation_id", "message_id", "url", "title"] {
            if let Some(val) = meta.get(key).and_then(|v| v.as_str()) {
                if !val.is_empty() {
                    parts.push(format!("{key}={}", truncate(val, 200)));
                }
            }
        }
    }
    if parts.is_empty() {
        base.to_string()
    } else {
        format!("{base} · {}", parts.join(" "))
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max).collect();
        out.push('…');
        out
    }
}

// ---------- /api/search ----------

#[derive(Deserialize)]
struct SearchQuery {
    q: String,
    #[serde(default)]
    limit: Option<i64>,
    /// Optional comma-separated entity type filter, e.g. `"Person,Goal"`.
    #[serde(default)]
    types: Option<String>,
}

async fn search(
    State(state): State<Arc<ApiState>>,
    Query(q): Query<SearchQuery>,
) -> Result<Json<Value>, ApiError> {
    let limit = q.limit.unwrap_or(20).clamp(1, 100);
    let types: Option<Vec<String>> = q
        .types
        .as_deref()
        .map(|s| {
            s.split(',')
                .map(|p| p.trim().to_string())
                .filter(|p| !p.is_empty())
                .collect()
        })
        .filter(|v: &Vec<String>| !v.is_empty());

    let res = state
        .retriever
        .search(&q.q, types, limit, limit)
        .await
        .map_err(ApiError::internal)?;

    // The picker also lets the user inject the *full* source note for a
    // matched entity. Retrieval may hit the entity (via name vector) without
    // hitting the note's BM25/vector, especially when the wording differs
    // ("我家儿子" vs "我们家娃"), so we pull missing source notes here and
    // tack them onto the response. Keep entity-derived notes after the
    // directly-matched ones so picker ranking still favours direct hits.
    let mut notes_out: Vec<Value> = res
        .notes
        .iter()
        .map(|n| serde_json::to_value(n).unwrap_or_else(|_| json!(null)))
        .collect();
    let existing_ids: std::collections::HashSet<&str> =
        res.notes.iter().map(|n| n.note.id.as_str()).collect();
    let mut wanted: Vec<String> = Vec::new();
    let mut entity_scores: std::collections::HashMap<String, f64> =
        std::collections::HashMap::new();
    for re in &res.entities {
        let Some(nid) = re.entity.source_note_id.as_deref() else {
            continue;
        };
        if existing_ids.contains(nid) {
            continue;
        }
        if !entity_scores.contains_key(nid) {
            wanted.push(nid.to_string());
        }
        // Same note can back multiple entities; keep the strongest score.
        let s = re.final_score;
        entity_scores
            .entry(nid.to_string())
            .and_modify(|prev| {
                if s > *prev {
                    *prev = s;
                }
            })
            .or_insert(s);
    }
    if !wanted.is_empty() {
        let extra = state
            .db
            .notes_by_ids(&wanted)
            .await
            .map_err(ApiError::internal)?;
        for note in extra {
            let score = entity_scores.get(&note.id).copied().unwrap_or(0.0);
            notes_out.push(json!({
                "note": note,
                "bm25_score": 0.0,
                "recency_score": 0.0,
                "final_score": score,
                "via_entity": true,
            }));
        }
    }

    Ok(Json(json!({
        "query": res.query,
        "entities": res.entities,
        "notes": notes_out,
        "diagnostics": res.diagnostics,
    })))
}

// ---------- /api/recent ----------

#[derive(Deserialize)]
struct RecentQuery {
    #[serde(default)]
    limit: Option<i64>,
}

async fn recent(
    State(state): State<Arc<ApiState>>,
    Query(q): Query<RecentQuery>,
) -> Result<Json<Vec<Note>>, ApiError> {
    let limit = q.limit.unwrap_or(20).clamp(1, 200);
    let notes = state
        .db
        .list_notes(limit)
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(notes))
}

// ---------- /api/themes ----------

async fn themes(
    State(state): State<Arc<ApiState>>,
    Query(q): Query<RecentQuery>,
) -> Result<Json<Value>, ApiError> {
    let limit = q.limit.unwrap_or(20).clamp(1, 100);
    let rows = state
        .db
        .list_active_themes(limit)
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(json!({ "themes": rows })))
}

// ---------- error type ----------

struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn bad_request(msg: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: msg.into(),
        }
    }
    fn internal<E: std::fmt::Display>(e: E) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: e.to_string(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        (self.status, Json(json!({ "error": self.message }))).into_response()
    }
}
