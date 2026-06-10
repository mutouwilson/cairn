//! Tauri webview window that backs the selection popover.
//!
//! Window characteristics:
//!   * Borderless, transparent, shadowed, always-on-top.
//!   * `focused = false` + `accept_first_mouse = true` so the popover can
//!     receive clicks without stealing focus from the user's source app.
//!   * Single instance keyed by the label `LABEL` — we reuse it across shows.
//!
//! The selection payload is **not** passed via URL. The renderer calls
//! `get_current_selection` over IPC once it mounts and re-fetches whenever
//! `selection_show` fires. This avoids re-encoding text in a URL on every
//! show.

#![cfg(target_os = "macos")]

use super::geometry::anchor_below_or_above;
use super::SelectionEvent;
use objc2_app_kit::{NSApplicationActivationOptions, NSRunningApplication};
use tauri::{AppHandle, Emitter, LogicalPosition, Manager, WebviewUrl, WebviewWindowBuilder};

pub const LABEL: &str = "selection-popover";
pub const POPOVER_W: f64 = 296.0;
pub const POPOVER_H: f64 = 46.0;
const ANCHOR_GAP: f64 = 8.0;

pub fn show<R: tauri::Runtime>(app: &AppHandle<R>, event: &SelectionEvent) -> tauri::Result<()> {
    let virtual_h = virtual_screen_height(app, event.rect.x, event.rect.y);
    let (x, y) = anchor_below_or_above(event.rect, POPOVER_W, POPOVER_H, ANCHOR_GAP, virtual_h);

    if let Some(win) = app.get_webview_window(LABEL) {
        win.set_position(LogicalPosition::new(x, y))?;
        let was_visible = win.is_visible().unwrap_or(false);
        if !was_visible {
            win.show()?;
        }
        let _ = win.set_always_on_top(true);
        let _ = app.emit_to(LABEL, "selection_show", event.clone());
        tracing::info!(
            target: "cairn_lib::selection_popover",
            x, y, was_visible, "popover reused — set_position + show"
        );
        return Ok(());
    }

    let win = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("selection-popover".into()))
        .title("Cairn — Save Selection")
        .inner_size(POPOVER_W, POPOVER_H)
        .position(x, y)
        .resizable(false)
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .focused(false)
        .accept_first_mouse(true)
        .shadow(false)
        .transparent(true)
        .visible(true)
        .build()?;

    // Re-assert always-on-top after build (some Tauri/Cocoa versions need it).
    let _ = win.set_always_on_top(true);
    if let Ok(pos) = win.outer_position() {
        tracing::info!(
            target: "cairn_lib::selection_popover",
            requested_x = x,
            requested_y = y,
            actual_x = pos.x,
            actual_y = pos.y,
            "popover built — first show"
        );
    }
    Ok(())
}

pub fn hide<R: tauri::Runtime>(app: &AppHandle<R>) {
    if let Some(win) = app.get_webview_window(LABEL) {
        let _ = win.hide();
    }
}

/// Return keyboard focus to the app the selection came from.
///
/// Clicking any popover button activates Cairn: AppKit activates the app that
/// owns a clicked window, and `accept_first_mouse` only delivers the click —
/// it does **not** suppress that activation. With Cairn now frontmost, hiding
/// the borderless popover leaves Cairn's main window as the topmost window of
/// the active app, so it jumps to the front (the "× pops the main window"
/// bug). Re-activating the source app pushes Cairn back behind and keeps the
/// user where they were — the same behaviour PopClip/Doubao give.
///
/// `pid <= 0` (unknown source) and a since-quit app are both no-ops.
pub fn reactivate_source_app(pid: i32) {
    if pid <= 0 {
        return;
    }
    objc2::rc::autoreleasepool(|_| unsafe {
        if let Some(app) = NSRunningApplication::runningApplicationWithProcessIdentifier(pid) {
            // Empty options = "activate, bring its key window forward". This
            // works because we call it while Cairn is the active app (the
            // click just activated us), which macOS treats as a cooperative
            // hand-off rather than a background app stealing focus.
            let _ = app.activateWithOptions(NSApplicationActivationOptions::empty());
        }
    });
}

fn virtual_screen_height<R: tauri::Runtime>(app: &AppHandle<R>, x: f64, y: f64) -> f64 {
    if let Ok(Some(m)) = app.monitor_from_point(x, y) {
        return m.size().height as f64 / m.scale_factor();
    }
    if let Ok(Some(m)) = app.primary_monitor() {
        return m.size().height as f64 / m.scale_factor();
    }
    2400.0
}
