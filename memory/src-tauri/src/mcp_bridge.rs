//! Phase 6b — in-app supervisor for the remote MCP (SSE) server.
//!
//! Lets the user turn the SSE bridge on/off from `/settings` without
//! restarting Cairn. The server runs in-process using the GUI's existing
//! `Db`, `AuditLogger`, and `Retriever` — there is no second SQLite handle
//! to race with the GUI's own writes.
//!
//! Persistence sits next to `selection_popover.json` in the data dir so a
//! single bool doesn't need its own migration.

use crate::audit::AuditLogger;
use crate::db::Db;
use crate::mcp;
use crate::providers::LiveProviders;
use crate::retrieval::Retriever;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::oneshot;

pub const DEFAULT_PORT: u16 = 7717;

#[derive(Debug, Clone, Serialize)]
pub struct McpBridgeStatus {
    pub enabled: bool,
    pub running: bool,
    pub port: u16,
    pub local_url: String,
}

#[derive(Serialize, Deserialize, Clone)]
struct PersistedConfig {
    enabled: bool,
    port: u16,
}

impl Default for PersistedConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            port: DEFAULT_PORT,
        }
    }
}

pub struct McpBridge {
    data_dir: PathBuf,
    db: Db,
    audit: AuditLogger,
    retriever: Retriever,
    providers: Arc<LiveProviders>,
    inner: Mutex<Inner>,
}

struct Inner {
    enabled: bool,
    port: u16,
    handle: Option<tauri::async_runtime::JoinHandle<()>>,
    shutdown_tx: Option<oneshot::Sender<()>>,
}

impl McpBridge {
    pub fn new(
        data_dir: PathBuf,
        db: Db,
        audit: AuditLogger,
        retriever: Retriever,
        providers: Arc<LiveProviders>,
    ) -> Arc<Self> {
        let cfg = load_persisted(&data_dir);
        Arc::new(Self {
            data_dir,
            db,
            audit,
            retriever,
            providers,
            inner: Mutex::new(Inner {
                enabled: cfg.enabled,
                port: cfg.port,
                handle: None,
                shutdown_tx: None,
            }),
        })
    }

    pub fn status(&self) -> McpBridgeStatus {
        let inner = self.inner.lock();
        McpBridgeStatus {
            enabled: inner.enabled,
            running: inner.handle.is_some(),
            port: inner.port,
            local_url: format!("http://127.0.0.1:{}/sse", inner.port),
        }
    }

    /// Restore the persisted state at app boot — start the server if the
    /// user left the toggle on last session.
    pub fn boot(self: &Arc<Self>) {
        let enabled = self.inner.lock().enabled;
        if enabled {
            self.start_worker();
        }
    }

    pub fn set_enabled(self: &Arc<Self>, enabled: bool) -> McpBridgeStatus {
        self.inner.lock().enabled = enabled;
        save_persisted(
            &self.data_dir,
            &PersistedConfig {
                enabled,
                port: self.inner.lock().port,
            },
        );
        if enabled {
            self.start_worker();
        } else {
            self.stop_worker();
        }
        self.status()
    }

    fn start_worker(self: &Arc<Self>) {
        let mut inner = self.inner.lock();
        if inner.handle.is_some() {
            return; // already running
        }
        let port = inner.port;
        let addr: SocketAddr = match format!("127.0.0.1:{}", port).parse() {
            Ok(a) => a,
            Err(e) => {
                tracing::error!(?e, "invalid SSE bind addr");
                return;
            }
        };
        let ctx = mcp::McpContext::new(self.db.clone(), self.audit.clone(), self.retriever.clone());
        let providers = self.providers.clone();
        let (tx, rx) = oneshot::channel::<()>();
        let handle = tauri::async_runtime::spawn(async move {
            if let Err(e) = mcp::serve_sse_with_shutdown(ctx, addr, Some(providers), async move {
                let _ = rx.await;
            })
            .await
            {
                tracing::warn!(error = %e, ?addr, "MCP SSE server task ended");
            }
        });
        inner.handle = Some(handle);
        inner.shutdown_tx = Some(tx);
        tracing::info!(?addr, "MCP bridge started");
    }

    fn stop_worker(self: &Arc<Self>) {
        let (tx, handle) = {
            let mut inner = self.inner.lock();
            (inner.shutdown_tx.take(), inner.handle.take())
        };
        if let Some(tx) = tx {
            let _ = tx.send(());
        }
        if let Some(h) = handle {
            // Don't await — let it drain naturally.
            drop(h);
        }
        tracing::info!("MCP bridge stop requested");
    }
}

fn persisted_path(data_dir: &Path) -> PathBuf {
    data_dir.join("mcp_bridge.json")
}

fn load_persisted(data_dir: &Path) -> PersistedConfig {
    std::fs::read_to_string(persisted_path(data_dir))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_persisted(data_dir: &Path, cfg: &PersistedConfig) {
    if let Ok(json) = serde_json::to_string_pretty(cfg) {
        let _ = std::fs::write(persisted_path(data_dir), json);
    }
}
