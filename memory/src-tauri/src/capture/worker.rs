//! Background worker that drives every `capture_sources` row.
//!
//! On startup we read enabled sources and spawn one tokio task each. Each task
//! sleeps for `poll_interval_secs`, calls the kind-specific poller, records
//! the outcome on the row (last_polled_at / last_status / captured_count),
//! and loops. A failed call is recorded but never crashes the task —
//! the next tick will retry.
//!
//! Dynamic add/remove is supported via `notify_changed`: when the UI inserts
//! or disables a source, it bumps a tokio broadcast and the supervisor
//! restarts the relevant task.

use crate::capture::{email_imap, ics, CaptureSource};
use crate::db::Db;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

/// Supervisor handle. Cloneable; lives in `AppState` so IPC commands can
/// `restart()` after the user adds/removes/edits a source.
#[derive(Clone)]
pub struct CaptureSupervisor {
    db: Db,
    tasks: Arc<Mutex<HashMap<String, JoinHandle<()>>>>,
}

impl CaptureSupervisor {
    pub fn new(db: Db) -> Self {
        Self {
            db,
            tasks: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn restart_all(&self) {
        let sources = match self.db.list_capture_sources().await {
            Ok(s) => s,
            Err(e) => {
                tracing::error!(?e, "list capture sources failed");
                return;
            }
        };

        let mut tasks = self.tasks.lock().await;
        // Stop everything first.
        for (_, handle) in tasks.drain() {
            handle.abort();
        }

        for source in sources {
            if source.enabled == 0 {
                continue;
            }
            let db = self.db.clone();
            let id = source.id.clone();
            let handle = tokio::spawn(async move { run_source(db, source).await });
            tasks.insert(id, handle);
        }
        tracing::info!(active = tasks.len(), "capture workers spawned");
    }
}

async fn run_source(db: Db, source: CaptureSource) {
    let interval = Duration::from_secs(source.poll_interval_secs.max(60) as u64);
    let mut ticker = tokio::time::interval(interval);
    // Run once almost immediately so the user sees results on startup.
    ticker.tick().await;

    loop {
        let outcome = match source.kind.as_str() {
            "imap" => email_imap::poll_once(&db, &source).await,
            "ics" => ics::poll_once(&db, &source).await,
            other => Err(anyhow::anyhow!("unknown capture source kind: {other}")),
        };
        match outcome {
            Ok(out) => {
                tracing::info!(
                    source = %source.id,
                    kind = %source.kind,
                    label = %source.label,
                    captured = out.captured,
                    skipped = out.seen_skipped,
                    "poll ok"
                );
                let _ = db
                    .record_poll_result(&source.id, true, out.captured, None)
                    .await;
            }
            Err(e) => {
                tracing::warn!(source = %source.id, error = %e, "poll error");
                let _ = db
                    .record_poll_result(&source.id, false, 0, Some(&e.to_string()))
                    .await;
            }
        }
        ticker.tick().await;
    }
}
