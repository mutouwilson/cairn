//! Global hotkey + quick-capture window (Phase 4b).
//!
//! Default shortcut: ⌘⇧M (macOS) / Ctrl+Shift+M (Win/Linux). Override with
//! `CAIRN_HOTKEY` env, e.g. `CAIRN_HOTKEY="CmdOrCtrl+Shift+;"`.
//!
//! On press: focuses the existing quick-capture window if it's open, or
//! creates a small frameless one centred on the active display, loaded at
//! `/quick-capture`. The renderer is responsible for closing the window
//! once it has saved (`await getCurrentWebviewWindow().close()`).

use tauri::{App, AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_clipboard_manager::ClipboardExt;
use tauri_plugin_global_shortcut::{
    Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutEvent, ShortcutState,
};

pub const QUICK_CAPTURE_LABEL: &str = "quick-capture";

pub fn build_plugin<R: tauri::Runtime>() -> tauri::plugin::TauriPlugin<R> {
    tauri_plugin_global_shortcut::Builder::new()
        .with_handler(on_shortcut::<R>)
        .build()
}

pub fn register(app: &App) -> Result<(), Box<dyn std::error::Error>> {
    let empty = parse_env_shortcut("CAIRN_HOTKEY").unwrap_or_else(default_empty_shortcut);
    let clip = parse_env_shortcut("CAIRN_CLIP_HOTKEY").unwrap_or_else(default_clip_shortcut);
    app.global_shortcut().register(empty)?;
    if empty != clip {
        // Only register the clipboard variant if it doesn't collide.
        if let Err(e) = app.global_shortcut().register(clip) {
            tracing::warn!(error = %e, ?clip, "clip-capture shortcut not registered (collision?)");
        }
    }
    tracing::info!(?empty, ?clip, "global shortcuts registered (capture)");
    Ok(())
}

fn parse_env_shortcut(var: &str) -> Option<Shortcut> {
    let raw = std::env::var(var).ok()?;
    Shortcut::try_from(raw.as_str()).ok()
}

fn default_empty_shortcut() -> Shortcut {
    // ⌘⇧M / Ctrl+Shift+M — empty quick capture
    #[cfg(target_os = "macos")]
    let mods = Modifiers::SUPER | Modifiers::SHIFT;
    #[cfg(not(target_os = "macos"))]
    let mods = Modifiers::CONTROL | Modifiers::SHIFT;
    Shortcut::new(Some(mods), Code::KeyM)
}

fn default_clip_shortcut() -> Shortcut {
    // ⌘⇧K / Ctrl+Shift+K — prefilled from clipboard (Phase 5b)
    #[cfg(target_os = "macos")]
    let mods = Modifiers::SUPER | Modifiers::SHIFT;
    #[cfg(not(target_os = "macos"))]
    let mods = Modifiers::CONTROL | Modifiers::SHIFT;
    Shortcut::new(Some(mods), Code::KeyK)
}

fn on_shortcut<R: tauri::Runtime>(app: &AppHandle<R>, shortcut: &Shortcut, event: ShortcutEvent) {
    if event.state() != ShortcutState::Pressed {
        return;
    }
    let prefill = if shortcut.key == Code::KeyK {
        app.clipboard()
            .read_text()
            .unwrap_or_default()
            .chars()
            .take(8000)
            .collect::<String>()
    } else {
        String::new()
    };
    if let Err(e) = open_quick_capture(app, &prefill) {
        tracing::warn!(?e, "open quick capture failed");
    }
}

/// Open (or re-open) the quick-capture window with the given prefill text.
/// Re-used by the global hotkey path *and* the selection popover (Phase 6a).
pub fn open_quick_capture<R: tauri::Runtime>(
    app: &AppHandle<R>,
    prefill: &str,
) -> tauri::Result<()> {
    let url = if prefill.is_empty() {
        "quick-capture".to_string()
    } else {
        format!("quick-capture?prefill={}", urlencoding(prefill))
    };

    if let Some(win) = app.get_webview_window(QUICK_CAPTURE_LABEL) {
        // Close + re-open with the fresh URL rather than IPC-ing into the
        // existing renderer. Cheaper, and matches what the user expects.
        let _ = win.close();
    }
    WebviewWindowBuilder::new(app, QUICK_CAPTURE_LABEL, WebviewUrl::App(url.into()))
        .title("Cairn — Quick Capture")
        .inner_size(520.0, 220.0)
        .resizable(false)
        .always_on_top(true)
        .decorations(true)
        .center()
        .focused(true)
        .skip_taskbar(true)
        .build()?;
    Ok(())
}

/// Minimal URL-encoding for the prefill query param. We only encode the
/// characters that would actually break a `?prefill=…` URL — full percent
/// encoding is overkill for in-process navigation.
fn urlencoding(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 16);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}
