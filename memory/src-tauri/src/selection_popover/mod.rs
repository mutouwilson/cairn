//! Phase 6a — Selection popover supervisor.
//!
//! Coordinates the macOS-only Accessibility selection detector, the popover
//! webview window, and the Tauri IPC surface exposed to the React UI.
//!
//! Cross-platform: on non-macOS targets the supervisor still compiles, but
//! `set_enabled` is a no-op and `status()` reports `supported: false`. The
//! React renderer reads `supported` and renders the toggle as disabled with
//! a "macOS only" hint.

#[cfg(target_os = "macos")]
pub mod detector;
#[cfg(target_os = "macos")]
pub mod geometry;
#[cfg(target_os = "macos")]
pub mod panel;
#[cfg(target_os = "macos")]
pub mod permission;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;

// ---------------------------------------------------------------------------
// Public, cross-platform serde types — used by the renderer over IPC.
// ---------------------------------------------------------------------------

/// Screen rectangle in points (top-left origin of the primary display).
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ScreenRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// One selection event observed by the detector.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelectionEvent {
    pub text: String,
    pub rect: ScreenRect,
    pub source_pid: i32,
    pub source_bundle: Option<String>,
    pub source_app_name: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SelectionStatus {
    /// Platform supports the popover (i.e. we built the macOS detector).
    pub supported: bool,
    /// Whether the user has enabled the popover in settings.
    pub enabled: bool,
    /// Accessibility trust (macOS) — `false` on other platforms.
    pub permission_granted: bool,
}

// ---------------------------------------------------------------------------
// Persisted toggle. One small JSON file in the data dir avoids the need for
// a new migration just to remember a boolean.
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Default, Clone)]
struct PersistedConfig {
    enabled: bool,
}

fn persisted_path(data_dir: &Path) -> PathBuf {
    data_dir.join("selection_popover.json")
}

fn load_persisted(data_dir: &Path) -> PersistedConfig {
    let p = persisted_path(data_dir);
    std::fs::read_to_string(&p)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_persisted(data_dir: &Path, cfg: &PersistedConfig) {
    let p = persisted_path(data_dir);
    if let Ok(json) = serde_json::to_string_pretty(cfg) {
        let _ = std::fs::write(&p, json);
    }
}

// ---------------------------------------------------------------------------
// Supervisor.
// ---------------------------------------------------------------------------

pub struct SelectionPopover {
    data_dir: PathBuf,
    inner: Mutex<Inner>,
}

struct Inner {
    enabled: bool,
    last: Option<SelectionEvent>,
    #[cfg(target_os = "macos")]
    cancel: Option<Arc<std::sync::atomic::AtomicBool>>,
    #[cfg(target_os = "macos")]
    worker: Option<tauri::async_runtime::JoinHandle<()>>,
    #[cfg(target_os = "macos")]
    trust_watcher: Option<tauri::async_runtime::JoinHandle<()>>,
}

impl SelectionPopover {
    pub fn new(data_dir: PathBuf) -> Arc<Self> {
        let cfg = load_persisted(&data_dir);
        Arc::new(Self {
            data_dir,
            inner: Mutex::new(Inner {
                enabled: cfg.enabled,
                last: None,
                #[cfg(target_os = "macos")]
                cancel: None,
                #[cfg(target_os = "macos")]
                worker: None,
                #[cfg(target_os = "macos")]
                trust_watcher: None,
            }),
        })
    }

    pub fn status(&self) -> SelectionStatus {
        let enabled = self.inner.lock().enabled;
        SelectionStatus {
            supported: cfg!(target_os = "macos"),
            enabled,
            permission_granted: current_permission(),
        }
    }

    pub fn current_selection(&self) -> Option<SelectionEvent> {
        self.inner.lock().last.clone()
    }

    pub fn clear_current(&self) {
        self.inner.lock().last = None;
    }

    /// Hide the popover window and clear the cached selection. Called from
    /// IPC when the user picks an action (Save / Save+Note / dismiss).
    pub fn dismiss<R: tauri::Runtime>(&self, app: &tauri::AppHandle<R>) {
        self.clear_current();
        #[cfg(target_os = "macos")]
        panel::hide(app);
        #[cfg(not(target_os = "macos"))]
        let _ = app;
    }

    pub fn request_permission(&self) -> SelectionStatus {
        #[cfg(target_os = "macos")]
        {
            // Pop the system sheet on first call; subsequent calls are no-ops.
            let _ = permission::is_trusted(true);
            if !permission::is_trusted(false) {
                let _ = permission::open_settings_pane();
            }
        }
        self.status()
    }

    /// Enable or disable the detector.
    ///
    /// Returns the **resulting** status. If the user enables but AX trust is
    /// missing, we leave `enabled = true` in memory + persist (so the
    /// detector boots automatically once they grant access) but the worker
    /// stays parked.
    pub fn set_enabled<R: tauri::Runtime>(
        self: &Arc<Self>,
        app: &tauri::AppHandle<R>,
        enabled: bool,
    ) -> SelectionStatus {
        // Always run through the start/stop path so a re-toggle can pick up
        // a freshly-granted AX permission without needing an app restart.
        self.inner.lock().enabled = enabled;
        save_persisted(&self.data_dir, &PersistedConfig { enabled });
        tracing::info!(target: "cairn_lib::selection_popover", enabled, "set_enabled called");

        #[cfg(target_os = "macos")]
        {
            if enabled {
                // Trigger the OS sheet so Cairn gets registered with the
                // Accessibility TCC list. If trust is already granted this
                // is a cheap no-op.
                let prompted = permission::is_trusted(true);
                tracing::info!(
                    target: "cairn_lib::selection_popover",
                    trusted_after_prompt = prompted,
                    "AX prompt fired"
                );
                if permission::is_trusted(false) {
                    self.start_worker(app.clone());
                } else {
                    let _ = permission::open_settings_pane();
                    tracing::warn!(
                        target: "cairn_lib::selection_popover",
                        "AX trust missing — opened System Settings. \
                         Re-toggle once Cairn is enabled there."
                    );
                }
            } else {
                self.stop_worker(app);
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = app;
        }

        self.status()
    }

    /// Called once during app setup. Boots the worker if the persisted toggle
    /// is on AND we already have AX trust (we never silently prompt at
    /// launch — that's reserved for an explicit user action).
    ///
    /// Also kicks off a low-rate AX-trust watcher: macOS caches TCC trust at
    /// the moment a process queries it, and toggling Accessibility in System
    /// Settings doesn't notify us. The watcher re-polls every 3 s and
    /// start/stops the detector when trust flips, so the user gets working
    /// popovers without needing to restart Cairn.
    pub fn boot<R: tauri::Runtime>(self: &Arc<Self>, app: &tauri::AppHandle<R>) {
        let enabled = self.inner.lock().enabled;
        if !enabled {
            return;
        }
        #[cfg(target_os = "macos")]
        {
            if permission::is_trusted(false) {
                self.start_worker(app.clone());
            } else {
                tracing::info!("selection popover enabled but AX trust missing — waiting for user");
            }
            self.spawn_trust_watcher(app.clone());
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = app;
        }
    }

    /// Poll `AXIsProcessTrustedWithOptions` every 3 s and reconcile detector
    /// state when it changes. Cheap (one system call) and gives the user a
    /// "just works after toggling permission" experience.
    #[cfg(target_os = "macos")]
    fn spawn_trust_watcher<R: tauri::Runtime>(self: &Arc<Self>, app: tauri::AppHandle<R>) {
        {
            let mut inner = self.inner.lock();
            if inner.trust_watcher.is_some() {
                return;
            }
            let supervisor = Arc::clone(self);
            let handle = tauri::async_runtime::spawn(async move {
                let mut last = permission::is_trusted(false);
                loop {
                    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                    let now = permission::is_trusted(false);
                    if now == last {
                        continue;
                    }
                    last = now;
                    let enabled = supervisor.inner.lock().enabled;
                    if !enabled {
                        continue;
                    }
                    if now {
                        tracing::info!("AX trust granted — starting selection detector");
                        supervisor.start_worker(app.clone());
                    } else {
                        tracing::info!("AX trust revoked — stopping selection detector");
                        supervisor.stop_worker(&app);
                    }
                }
            });
            inner.trust_watcher = Some(handle);
        }
    }

    #[cfg(target_os = "macos")]
    fn start_worker<R: tauri::Runtime>(self: &Arc<Self>, app: tauri::AppHandle<R>) {
        use std::sync::atomic::AtomicBool;

        let mut inner = self.inner.lock();
        if inner.worker.is_some() {
            return;
        }
        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_for_task = cancel.clone();
        let supervisor = Arc::clone(self);

        let handle = tauri::async_runtime::spawn(async move {
            detector::run(app, cancel_for_task, move |app, ev| match ev {
                Some(ev) => {
                    tracing::info!(
                        target: "cairn_lib::selection_popover",
                        text_len = ev.text.len(),
                        rect_x = ev.rect.x,
                        rect_y = ev.rect.y,
                        rect_w = ev.rect.width,
                        rect_h = ev.rect.height,
                        bundle = ev.source_bundle.as_deref().unwrap_or("?"),
                        "selection observed — showing popover"
                    );
                    supervisor.inner.lock().last = Some(ev.clone());
                    if let Err(e) = panel::show(app, &ev) {
                        tracing::warn!(error = %e, "show selection popover failed");
                    }
                }
                None => {
                    supervisor.inner.lock().last = None;
                    panel::hide(app);
                }
            })
            .await;
        });

        inner.cancel = Some(cancel);
        inner.worker = Some(handle);
        tracing::info!("selection detector started");
    }

    #[cfg(target_os = "macos")]
    fn stop_worker<R: tauri::Runtime>(&self, app: &tauri::AppHandle<R>) {
        use std::sync::atomic::Ordering;

        let (cancel, worker) = {
            let mut inner = self.inner.lock();
            inner.last = None;
            (inner.cancel.take(), inner.worker.take())
        };
        if let Some(c) = cancel {
            c.store(true, Ordering::Relaxed);
        }
        if let Some(h) = worker {
            h.abort();
        }
        panel::hide(app);
        tracing::info!("selection detector stopped");
    }
}

#[cfg(target_os = "macos")]
fn current_permission() -> bool {
    permission::is_trusted(false)
}

#[cfg(not(target_os = "macos"))]
fn current_permission() -> bool {
    false
}
