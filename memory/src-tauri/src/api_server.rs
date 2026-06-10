//! Phase 7 · Ext — Standalone always-on HTTP API server.
//!
//! Decoupled from the MCP SSE bridge: the browser extension and any other
//! local HTTP caller only need `/api/*` (capture / search / status). MCP
//! over SSE is a separate concern and stays opt-in on its own port.
//!
//! ### Surface
//!
//! - Default bind: `127.0.0.1:7716` (env override `CAIRN_API_PORT`).
//! - Always boots at app start; if the port is busy the server logs a warning
//!   and tries the next 4 ports so dev / multi-instance is forgiving.
//! - Optional bearer auth: if `CAIRN_API_TOKEN` is set, every request must
//!   carry `Authorization: Bearer <token>`. Unset = open to localhost (the
//!   v0.1 default, easier to bootstrap).
//!
//! ### Endpoint handshake file
//!
//! Cairn writes the live `{host, port, token?}` to
//! `<data_dir>/api_endpoint.json` (mode 0600) on every successful bind. The
//! extension can later support an "import endpoint" button that pastes from
//! this file; for now users configure the port + optional token manually in
//! extension options.

use crate::audit::AuditLogger;
use crate::db::Db;
use crate::providers::LiveProviders;
use crate::retrieval::Retriever;
use axum::{
    extract::Request,
    http::{header, StatusCode},
    middleware::{self, Next},
    response::Response,
};
use serde::Serialize;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};

pub const DEFAULT_PORT: u16 = 7716;
pub const ENDPOINT_FILE: &str = "api_endpoint.json";

#[derive(Debug, Serialize)]
pub struct Endpoint {
    pub version: u32,
    pub host: String,
    pub port: u16,
    /// Empty string means no auth.
    pub token: String,
    pub started_at: i64,
}

/// Boot the API server on the first port we can bind, trying
/// `[base, base+1, …, base+3]`. Returns the chosen address.
///
/// The handle is owned by tokio — if the function-scope task exits, the
/// server stops. Caller should keep the spawned task alive for the lifetime
/// of the app.
pub async fn boot(
    data_dir: PathBuf,
    db: Db,
    audit: AuditLogger,
    retriever: Retriever,
    providers: Option<Arc<LiveProviders>>,
) -> anyhow::Result<SocketAddr> {
    let base_port = std::env::var("CAIRN_API_PORT")
        .ok()
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(DEFAULT_PORT);
    let token = std::env::var("CAIRN_API_TOKEN").unwrap_or_default();

    let api_state = crate::api::ApiState {
        db,
        audit,
        retriever,
        providers,
    };

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let auth_token = Arc::new(token.clone());
    let mut router = crate::api::router(api_state);
    if !token.is_empty() {
        router = router.layer(middleware::from_fn(move |req, next| {
            let t = auth_token.clone();
            async move { check_token(t, req, next).await }
        }));
    }
    let app = router.layer(cors);

    // Try a small port range so a stale process or another Cairn instance
    // doesn't crash the boot path. 4 retries is enough for dev.
    let mut last_err: Option<std::io::Error> = None;
    for offset in 0..4 {
        let candidate = SocketAddr::from(([127, 0, 0, 1], base_port + offset));
        match tokio::net::TcpListener::bind(candidate).await {
            Ok(listener) => {
                tracing::info!(addr = %candidate, has_token = !token.is_empty(), "API server listening");
                write_endpoint_file(&data_dir, &candidate, &token).ok();
                let app = app.clone();
                tokio::spawn(async move {
                    if let Err(e) = axum::serve(listener, app).await {
                        tracing::error!(error = %e, "API server died");
                    }
                });
                return Ok(candidate);
            }
            Err(e) => {
                tracing::warn!(addr = %candidate, error = %e, "bind failed, trying next port");
                last_err = Some(e);
            }
        }
    }
    Err(anyhow::anyhow!(
        "could not bind API server on ports {}..{}: {}",
        base_port,
        base_port + 3,
        last_err
            .map(|e| e.to_string())
            .unwrap_or_else(|| "unknown".into()),
    ))
}

async fn check_token(
    expected: Arc<String>,
    req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    // Allow `/api/status` without auth so the extension can fail soft and
    // tell the user "you need a token" instead of returning a useless 401.
    if req.uri().path() == "/api/status" {
        return Ok(next.run(req).await);
    }
    let got = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .unwrap_or("");
    if got == expected.as_str() {
        Ok(next.run(req).await)
    } else {
        Err(StatusCode::UNAUTHORIZED)
    }
}

fn write_endpoint_file(
    data_dir: &std::path::Path,
    addr: &SocketAddr,
    token: &str,
) -> anyhow::Result<()> {
    let payload = Endpoint {
        version: 1,
        host: addr.ip().to_string(),
        port: addr.port(),
        token: token.to_string(),
        started_at: chrono::Utc::now().timestamp(),
    };
    let path = data_dir.join(ENDPOINT_FILE);
    std::fs::create_dir_all(data_dir).ok();
    let s = serde_json::to_string_pretty(&payload)?;
    std::fs::write(&path, s)?;
    // Tighten perms to 0600 so other users on a multi-user mac can't read
    // the token. No-op on platforms without unix permissions.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&path)?.permissions();
        perms.set_mode(0o600);
        std::fs::set_permissions(&path, perms)?;
    }
    Ok(())
}
