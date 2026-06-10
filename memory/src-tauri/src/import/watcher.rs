//! Phase 7 · V6 M3 — auto-run `run_import` when memory files change on
//! disk. The watcher is **scanner-driven**: every watch target and every
//! filter decision comes from `super::scanner`, so adding a new source
//! kind (Cursor rules, AGENTS.md, …) requires only a scanner change.
//!
//! Topology + filter live in `scanner::{watch_targets, is_relevant_path,
//! affects_watch_topology}`. Today that yields a tight watch set on macOS:
//! `~/.claude` (Direct), `~/.claude/projects` (Direct), and each existing
//! `*/memory/` (Direct) — no recursive watch over the project tree, so the
//! hundreds of jsonl session transcripts under each `<encoded>/` dir don't
//! flood our callback.
//!
//! Dynamic re-watch: when a new project directory appears under
//! `projects/`, the next debounce batch fires `affects_watch_topology`, we
//! re-call `watch_targets()`, and any new `memory/` subdir gets a watch
//! added in place — no restart required.

use super::scanner::{self, WatchMode, WatchTarget};
use crate::audit::AuditLogger;
use crate::capture::processor::ProcessingContext;
use crate::db::Db;
use crate::providers::LiveProviders;
use anyhow::{anyhow, Result};
use notify::RecursiveMode;
use notify_debouncer_mini::{new_debouncer, DebounceEventResult, Debouncer};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Runtime};

const DEBOUNCE_WINDOW: Duration = Duration::from_secs(2);
const EVENT_NAME: &str = "import:changed";

#[derive(Debug, Default, Clone, Copy)]
struct Signal {
    /// At least one watched path looks like a memory file we care about.
    import: bool,
    /// At least one event suggests the watch topology may have grown —
    /// re-call `scanner::watch_targets` and subscribe to any new dirs.
    rescan: bool,
}

pub fn spawn<R: Runtime>(
    app: AppHandle<R>,
    db: Db,
    audit: AuditLogger,
    providers: Arc<LiveProviders>,
) -> Result<()> {
    let initial = scanner::watch_targets(None);
    if initial.is_empty() {
        tracing::info!("scanner returned no watch targets; import watcher idle");
        return Ok(());
    }

    let (tx, rx) = std::sync::mpsc::channel::<Signal>();
    let mut debouncer = new_debouncer(DEBOUNCE_WINDOW, {
        let tx = tx.clone();
        move |res: DebounceEventResult| match res {
            Ok(events) => {
                let mut sig = Signal::default();
                for e in &events {
                    if scanner::is_relevant_path(&e.path) {
                        sig.import = true;
                    }
                    if scanner::affects_watch_topology(&e.path) {
                        sig.rescan = true;
                    }
                }
                if sig.import || sig.rescan {
                    let _ = tx.send(sig);
                }
            }
            Err(err) => {
                tracing::warn!(error = %err, "import watcher fs error");
            }
        }
    })
    .map_err(|e| anyhow!("debouncer init failed: {e}"))?;

    let mut watched: HashSet<PathBuf> = HashSet::new();
    for t in &initial {
        if let Err(e) = subscribe(&mut debouncer, t) {
            tracing::warn!(?t.path, error = %e, "initial watch failed");
        } else {
            watched.insert(t.path.clone());
        }
    }
    tracing::info!(
        watch_count = watched.len(),
        debounce_ms = DEBOUNCE_WINDOW.as_millis() as u64,
        "import watcher armed"
    );

    // Hand the debouncer + receiver to a blocking task. `block_on` keeps
    // import runs serial.
    tauri::async_runtime::spawn_blocking(move || {
        let mut debouncer = debouncer;
        let mut watched = watched;
        while let Ok(first) = rx.recv() {
            // Coalesce extra ticks that piled up during the previous run.
            let mut sig = first;
            while let Ok(extra) = rx.try_recv() {
                sig.import |= extra.import;
                sig.rescan |= extra.rescan;
            }

            if sig.rescan {
                let new_dirs = refresh_watches(&mut debouncer, &mut watched);
                if !new_dirs.is_empty() {
                    tracing::info!(?new_dirs, "import watcher picked up new directories");
                    // New memory dir means almost certainly new files to
                    // ingest — force an import pass even if nothing in this
                    // batch already matched the relevance filter.
                    sig.import = true;
                }
            }

            if sig.import {
                if let Err(e) =
                    tauri::async_runtime::block_on(run_once(&app, &db, &audit, &providers))
                {
                    tracing::warn!(error = %e, "watcher-triggered import failed");
                }
            }
        }
        tracing::info!("import watcher channel closed; exiting");
    });

    Ok(())
}

fn subscribe(
    debouncer: &mut Debouncer<notify::RecommendedWatcher>,
    target: &WatchTarget,
) -> Result<()> {
    let mode = match target.mode {
        WatchMode::Direct => RecursiveMode::NonRecursive,
        WatchMode::Recursive => RecursiveMode::Recursive,
    };
    debouncer
        .watcher()
        .watch(&target.path, mode)
        .map_err(|e| anyhow!("watch {} failed: {e}", target.path.display()))?;
    Ok(())
}

fn refresh_watches(
    debouncer: &mut Debouncer<notify::RecommendedWatcher>,
    watched: &mut HashSet<PathBuf>,
) -> Vec<PathBuf> {
    let mut added = Vec::new();
    for t in scanner::watch_targets(None) {
        if watched.contains(&t.path) {
            continue;
        }
        let mode = match t.mode {
            WatchMode::Direct => RecursiveMode::NonRecursive,
            WatchMode::Recursive => RecursiveMode::Recursive,
        };
        match debouncer.watcher().watch(&t.path, mode) {
            Ok(()) => {
                added.push(t.path.clone());
                watched.insert(t.path);
            }
            Err(e) => {
                tracing::warn!(?t.path, error = %e, "add watch failed");
            }
        }
    }
    added
}

async fn run_once<R: Runtime>(
    app: &AppHandle<R>,
    db: &Db,
    audit: &AuditLogger,
    providers: &Arc<LiveProviders>,
) -> Result<()> {
    let extractor = providers.extractor();
    let embedder = providers.embedder();
    let ctx = ProcessingContext {
        db: db.clone(),
        extractor,
        embedder,
        audit: audit.clone(),
        embedding_model_id: providers.embedding_model_id(),
    };
    let summary = super::run_import(db, Some(&ctx), super::ImportOptions::default()).await?;
    tracing::info!(
        scanned = summary.scanned,
        imported = summary.imported,
        updated = summary.updated,
        skipped_dup = summary.skipped_dup,
        failed = summary.failed,
        "watcher import run"
    );
    if summary.imported + summary.updated > 0 {
        let _ = app.emit(EVENT_NAME, &summary);
    }
    Ok(())
}
