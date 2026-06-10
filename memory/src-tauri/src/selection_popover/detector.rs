//! AX-based selection detector.
//!
//! On each tick we walk:
//!
//! ```text
//! AXUIElementCreateSystemWide
//!   → kAXFocusedUIElementAttribute        (focused UI element)
//!     → kAXSelectedTextAttribute          (raw selection string)
//!     → kAXSelectedTextRangeAttribute     (CFRange in the document)
//!     → kAXBoundsForRangeParameterizedAttribute  (CGRect on screen)
//! ```
//!
//! and emit a `SelectionEvent` when the selected text **changes** to a
//! non-empty value. We skip ticks while the left mouse button is held
//! (the user is mid-drag) so the popover doesn't flash during selection.
//!
//! Why polling instead of a CGEventTap on mouse-up:
//!   * No extra `Input Monitoring` permission needed (AX is already required).
//!   * Selection isn't always finished on mouse-up — keyboard-driven
//!     selection (shift+arrow, ⌘A) wouldn't fire anyway.
//!   * The poll cost is ~one AX round-trip per 250 ms when the user isn't
//!     selecting; the focused-element walk is cheap on the system-wide
//!     element.

#![cfg(target_os = "macos")]

use super::geometry::{make_range_value, read_rect, screen_rect_from_cg};
use super::SelectionEvent;
use accessibility_sys::{
    kAXBoundsForRangeParameterizedAttribute, kAXFocusedUIElementAttribute,
    kAXSelectedTextAttribute, kAXSelectedTextRangeAttribute, kAXValueTypeCFRange,
    AXUIElementCopyAttributeValue, AXUIElementCopyParameterizedAttributeValue,
    AXUIElementCreateApplication, AXUIElementCreateSystemWide, AXUIElementRef,
    AXUIElementSetAttributeValue, AXValueGetType, AXValueGetValue, AXValueRef,
};
use core_foundation::base::{CFIndex, CFRelease, CFTypeRef, TCFType};
use core_foundation::boolean::CFBoolean;
use core_foundation::string::{CFString, CFStringRef};
use core_graphics::display::CGDisplay;
use core_graphics::event::{CGEvent, CGEventFlags, CGEventTapLocation, CGKeyCode};
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
use objc2_app_kit::{NSEvent, NSPasteboard, NSPasteboardTypeString};
use objc2_foundation::NSString;
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use std::collections::HashSet;
use std::ffi::c_void;
use std::mem::MaybeUninit;
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Chromium-family bundles that need a one-shot "enable accessibility" nudge
/// before they expose their text widgets via AX. Chrome turns AX on lazily
/// to save CPU; without this, reading kAXSelectedText returns nothing.
const CHROMIUM_BUNDLES: &[&str] = &[
    "com.google.Chrome",
    "com.google.Chrome.canary",
    "com.google.Chrome.beta",
    "com.google.Chrome.dev",
    "com.microsoft.edgemac",
    "com.microsoft.edgemac.Beta",
    "com.microsoft.edgemac.Dev",
    "com.microsoft.edgemac.Canary",
    "com.brave.Browser",
    "com.brave.Browser.beta",
    "com.brave.Browser.nightly",
    "com.vivaldi.Vivaldi",
    "com.operasoftware.Opera",
    "company.thebrowser.Browser", // Arc
    "com.kagi.kagimacOS",         // Orion (Chromium-based recent versions)
];

/// Track PIDs we've already nudged so we don't spam `AXUIElementSetAttribute`
/// every tick.
static AX_ACTIVATED_PIDS: Lazy<Mutex<HashSet<i32>>> = Lazy::new(|| Mutex::new(HashSet::new()));

const POLL_IDLE_MS: u64 = 220;
const POLL_HOT_MS: u64 = 380;
/// While a popover is on screen we poll much faster. Click-outside dismissal
/// keys off the mouse-down *edge* (`now_down && !prev_down`); at 220–380 ms a
/// quick click can land entirely between two samples and be missed, leaving
/// the popover stuck (the "clicking elsewhere doesn't close it" bug). 80 ms
/// reliably catches a normal click without meaningful idle cost — it only
/// applies for the few seconds a popover is actually up.
const POLL_SHOWN_MS: u64 = 80;
/// Anything past this is suspicious (e.g. someone selecting an entire 20MB
/// file). Truncate to avoid passing a giant string through IPC and the LLM.
const MAX_SELECTION_BYTES: usize = 16 * 1024;
/// Virtual key for `C`.
const KEY_CODE_C: CGKeyCode = 8;
/// How long to wait for the target app to react to our synthetic ⌘C and
/// publish to the system pasteboard.
const PASTEBOARD_WAIT_MS: u64 = 90;

/// Long-running poll loop. Returns when `cancel` flips to `true` or when the
/// tokio runtime drops us.
///
/// Strategy:
///   * Every tick walks AX — catches selections in Cocoa-native and
///     Chromium-with-AX-activated apps even when made via keyboard.
///   * Additionally, on a left-mouse `down→up` edge with non-zero drag
///     distance, if AX returned nothing we fall back to a synthetic ⌘C
///     against the system pasteboard (PopClip / Doubao technique). This
///     lifts the popover into Sublime, JetBrains, Electron apps, etc.
///   * Mouse state is sampled at the same cadence; we keep the previous
///     down position so we can tell a drag from a click.
pub async fn run<R, F>(app: tauri::AppHandle<R>, cancel: Arc<AtomicBool>, mut on_change: F)
where
    R: tauri::Runtime,
    F: FnMut(&tauri::AppHandle<R>, Option<SelectionEvent>) + Send + 'static,
{
    use std::time::Instant;
    let our_pid = std::process::id() as i32;
    let mut last_text: Option<String> = None;
    let mut last_source = LastSource::None;
    let mut prev_mouse_down = false;
    let mut mouse_down_pos: Option<(f64, f64)> = None;
    let mut last_mouse_down_at: Option<Instant> = None;
    let mut double_click_pending = false;
    let mut dismissed_text: Option<String> = None;
    let mut dismissed_at: Option<Instant> = None;
    // After a dismiss we briefly suppress re-firing the *same* text so AX
    // polling doesn't bounce the popover straight back. Kept short so that
    // deliberately re-selecting the same phrase feels responsive rather than
    // dead (the "sometimes it doesn't appear" complaint).
    const DISMISS_SUPPRESS: Duration = Duration::from_millis(1500);
    const DOUBLE_CLICK_MS: u128 = 600;

    while !cancel.load(Ordering::Relaxed) {
        let now_mouse_down = mouse_left_down();
        let now_pos = mouse_location_top_left();
        let now = Instant::now();

        // Mouse-down edge.
        if now_mouse_down && !prev_mouse_down {
            mouse_down_pos = now_pos;
            double_click_pending = last_mouse_down_at
                .map(|t| now.duration_since(t).as_millis() < DOUBLE_CLICK_MS)
                .unwrap_or(false);
            last_mouse_down_at = Some(now);

            // Click-outside dismissal: if a popover is currently showing AND
            // the user just clicked anywhere outside its rect, hide it. This
            // is the only signal we have for "user moved on" when the source
            // app doesn't expose selection via AX (Chrome / Sublime).
            if let (Some(pos), true) = (now_pos, last_text.is_some()) {
                let outside = match popover_logical_rect(&app) {
                    Some((px, py, pw, ph)) => {
                        !(pos.0 >= px && pos.0 < px + pw && pos.1 >= py && pos.1 < py + ph)
                    }
                    None => false, // No popover up → nothing to hide.
                };
                if outside {
                    dismissed_text = last_text.take();
                    dismissed_at = Some(now);
                    last_source = LastSource::None;
                    on_change(&app, None);
                }
            }
        }

        let mouse_just_released = prev_mouse_down && !now_mouse_down;
        let drag_distance = match (mouse_just_released, mouse_down_pos, now_pos) {
            (true, Some((dx, dy)), Some((ux, uy))) => {
                ((ux - dx).powi(2) + (uy - dy).powi(2)).sqrt()
            }
            _ => 0.0,
        };
        prev_mouse_down = now_mouse_down;

        if now_mouse_down {
            tokio::time::sleep(Duration::from_millis(POLL_IDLE_MS)).await;
            continue;
        }

        let try_clipboard = mouse_just_released && (drag_distance > 4.0 || double_click_pending);
        if mouse_just_released {
            double_click_pending = false;
        }

        let snap = match tokio::task::spawn_blocking(move || read_selection_blocking(try_clipboard))
            .await
        {
            Ok(opt) => opt,
            Err(e) => {
                tracing::warn!(?e, "spawn_blocking join failed in selection detector");
                tokio::time::sleep(Duration::from_millis(POLL_HOT_MS)).await;
                continue;
            }
        };

        match snap {
            None => {
                // Selection cleared (only observable for AX-sourced popovers).
                if matches!(last_source, LastSource::Ax) {
                    last_text = None;
                    last_source = LastSource::None;
                    dismissed_text = None;
                    dismissed_at = None;
                    on_change(&app, None);
                }
                // A clipboard-sourced popover gets no AX-clear signal, so it
                // stays up here with `last_text` still set — poll fast so we
                // catch the click-outside that dismisses it.
                let nap = if last_text.is_some() { POLL_SHOWN_MS } else { POLL_IDLE_MS };
                tokio::time::sleep(Duration::from_millis(nap)).await;
            }
            Some(mut ev) => {
                if ev.source_pid == our_pid {
                    tokio::time::sleep(Duration::from_millis(POLL_IDLE_MS)).await;
                    continue;
                }
                if ev.text.len() > MAX_SELECTION_BYTES {
                    ev.text.truncate(MAX_SELECTION_BYTES);
                }
                let is_clipboard = ev.rect.width == 0.0 && ev.rect.height == 0.0;
                let source = if is_clipboard {
                    LastSource::Clipboard
                } else {
                    LastSource::Ax
                };
                // Suppress re-fire of the exact text the user just dismissed
                // for `DISMISS_SUPPRESS` so AX polling doesn't bring the
                // popover right back. Different text resets the lock.
                let suppressed = matches!(&dismissed_text, Some(d) if d == &ev.text)
                    && dismissed_at
                        .map(|t| now.duration_since(t) < DISMISS_SUPPRESS)
                        .unwrap_or(false);
                if !suppressed {
                    let text_changed = match &last_text {
                        Some(t) => t != &ev.text,
                        None => true,
                    };
                    if is_clipboard || text_changed {
                        dismissed_text = None;
                        dismissed_at = None;
                        last_text = Some(ev.text.clone());
                        last_source = source;
                        on_change(&app, Some(ev));
                    }
                }
                // Popover up (or just shown) → poll fast to catch click-outside;
                // otherwise fall back to the hot re-check cadence.
                let nap = if last_text.is_some() { POLL_SHOWN_MS } else { POLL_HOT_MS };
                tokio::time::sleep(Duration::from_millis(nap)).await;
            }
        }
    }
    on_change(&app, None);
    tracing::info!("selection detector stopped");
}

/// Returns the popover's screen rect in **logical points** (top-left origin)
/// if the window exists and is visible. Logical coords match
/// `mouse_location_top_left`, so we can directly test
/// "is the mouse inside the popover?".
fn popover_logical_rect<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> Option<(f64, f64, f64, f64)> {
    use tauri::Manager;
    let win = app.get_webview_window(crate::selection_popover::panel::LABEL)?;
    if !win.is_visible().unwrap_or(false) {
        return None;
    }
    let pos = win.outer_position().ok()?;
    let size = win.outer_size().ok()?;
    let scale = win.scale_factor().ok()?;
    Some((
        pos.x as f64 / scale,
        pos.y as f64 / scale,
        size.width as f64 / scale,
        size.height as f64 / scale,
    ))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LastSource {
    None,
    Ax,
    Clipboard,
}

// ---------------------------------------------------------------------------
// Synchronous AX walk.
// ---------------------------------------------------------------------------

fn read_selection_blocking(try_clipboard_fallback: bool) -> Option<SelectionEvent> {
    if mouse_left_down() {
        return None;
    }
    if let Some(ev) = unsafe { read_selection_unsafe() } {
        return Some(ev);
    }
    if try_clipboard_fallback {
        return clipboard_sniff();
    }
    None
}

unsafe fn read_selection_unsafe() -> Option<SelectionEvent> {
    // Front-most app first — gives us the bundle id needed to nudge Chromium
    // browsers into enabling AX before we try to read their selected text.
    let (source_pid, source_bundle, source_app_name) = frontmost_app().unwrap_or((0, None, None));
    if source_pid > 0 {
        ensure_app_ax_enabled(source_pid, source_bundle.as_deref());
    }

    let system_wide = AXUIElementCreateSystemWide();
    if system_wide.is_null() {
        return None;
    }
    let _system_wide = CFGuard::new(system_wide as CFTypeRef);

    let focused = copy_attribute(system_wide, kAXFocusedUIElementAttribute)?;
    let _focused_guard = CFGuard::new(focused);
    let focused_elem = focused as AXUIElementRef;

    // Selected text.
    let text_ptr = copy_attribute(focused_elem, kAXSelectedTextAttribute)?;
    // wrap_under_create_rule absorbs the +1 from the Copy* call.
    let cf_text = CFString::wrap_under_create_rule(text_ptr as CFStringRef);
    let text = cf_text.to_string();
    if text.trim().is_empty() {
        return None;
    }

    // Selected range (CFRange packed in an AXValue).
    let range_value = copy_attribute(focused_elem, kAXSelectedTextRangeAttribute)?;
    let _range_guard = CFGuard::new(range_value);
    let (loc, len) = read_cf_range(range_value as AXValueRef)?;
    if len <= 0 {
        return None;
    }

    // Pack a new CFRange into an AXValue to pass back as the parameter.
    let range_param = make_range_value(loc, len)?;
    let _param_guard = CFGuard::new(range_param as CFTypeRef);

    // Bounds-for-range.
    let bounds_ptr = copy_parameterized(
        focused_elem,
        kAXBoundsForRangeParameterizedAttribute,
        range_param as CFTypeRef,
    )?;
    let _bounds_guard = CFGuard::new(bounds_ptr);
    let rect = read_rect(bounds_ptr as AXValueRef)?;
    if rect.size.width <= 0.0 || rect.size.height <= 0.0 {
        return None;
    }

    Some(SelectionEvent {
        text,
        rect: screen_rect_from_cg(rect),
        source_pid,
        source_bundle,
        source_app_name,
    })
}

unsafe fn copy_attribute(elem: AXUIElementRef, attr: &'static str) -> Option<CFTypeRef> {
    let cf_attr = CFString::from_static_string(attr);
    let mut out: CFTypeRef = ptr::null();
    let err =
        AXUIElementCopyAttributeValue(elem, cf_attr.as_concrete_TypeRef(), &mut out as *mut _);
    if err == accessibility_sys::kAXErrorSuccess && !out.is_null() {
        Some(out)
    } else {
        None
    }
}

unsafe fn copy_parameterized(
    elem: AXUIElementRef,
    attr: &'static str,
    param: CFTypeRef,
) -> Option<CFTypeRef> {
    let cf_attr = CFString::from_static_string(attr);
    let mut out: CFTypeRef = ptr::null();
    let err = AXUIElementCopyParameterizedAttributeValue(
        elem,
        cf_attr.as_concrete_TypeRef(),
        param,
        &mut out as *mut _,
    );
    if err == accessibility_sys::kAXErrorSuccess && !out.is_null() {
        Some(out)
    } else {
        None
    }
}

/// Unpack `CFRange` from an AXValueRef known to be of `kAXValueTypeCFRange`.
unsafe fn read_cf_range(value: AXValueRef) -> Option<(i64, i64)> {
    if value.is_null() {
        return None;
    }
    if AXValueGetType(value) != kAXValueTypeCFRange {
        return None;
    }
    #[repr(C)]
    struct RawRange {
        location: CFIndex,
        length: CFIndex,
    }
    let mut raw = MaybeUninit::<RawRange>::uninit();
    let ok = AXValueGetValue(value, kAXValueTypeCFRange, raw.as_mut_ptr() as *mut c_void);
    if !ok {
        return None;
    }
    let r = raw.assume_init();
    Some((r.location as i64, r.length as i64))
}

// ---------------------------------------------------------------------------
// Clipboard sniff fallback (Sublime / JetBrains / Electron / other non-AX apps).
//
// On a mouse-up after a drag, if the AX walk returned no selection we save the
// current pasteboard, synthesise ⌘C, give the target app a brief moment to
// publish to the pasteboard, read it back, then restore the saved value. This
// is the same technique PopClip / Doubao use to handle apps that draw their
// own text widgets.
// ---------------------------------------------------------------------------

fn clipboard_sniff() -> Option<SelectionEvent> {
    let our_pid = std::process::id() as i32;
    let (pid, bundle, name) = frontmost_app().unwrap_or((0, None, None));
    if pid == 0 || pid == our_pid {
        return None;
    }

    let snap = save_clipboard();
    send_cmd_c();
    std::thread::sleep(Duration::from_millis(PASTEBOARD_WAIT_MS));
    let new_count = pasteboard_change_count();
    let new_text = if new_count != snap.change_count {
        read_clipboard_text()
    } else {
        None
    };
    // Always restore, even if we got something useful.
    restore_clipboard(&snap);

    let text = match new_text {
        Some(t) if !t.trim().is_empty() => t,
        _ => return None,
    };

    let (mx, my) = mouse_location_top_left()?;
    // Synthesise a 0-height anchor rect at the mouse cursor so the existing
    // anchor logic places the popover just below the cursor.
    let rect = crate::selection_popover::ScreenRect {
        x: mx,
        y: my,
        width: 0.0,
        height: 0.0,
    };
    tracing::info!(
        target: "cairn_lib::selection_popover",
        bundle = bundle.as_deref().unwrap_or("?"),
        text_len = text.len(),
        "clipboard-sniff selection captured"
    );
    Some(SelectionEvent {
        text,
        rect,
        source_pid: pid,
        source_bundle: bundle,
        source_app_name: name,
    })
}

struct ClipboardSnapshot {
    text: Option<String>,
    change_count: i64,
}

fn save_clipboard() -> ClipboardSnapshot {
    objc2::rc::autoreleasepool(|_| unsafe {
        let pb = NSPasteboard::generalPasteboard();
        ClipboardSnapshot {
            text: pb
                .stringForType(NSPasteboardTypeString)
                .map(|s| s.to_string()),
            change_count: pb.changeCount() as i64,
        }
    })
}

fn restore_clipboard(snap: &ClipboardSnapshot) {
    objc2::rc::autoreleasepool(|_| unsafe {
        let pb = NSPasteboard::generalPasteboard();
        pb.clearContents();
        if let Some(t) = &snap.text {
            let ns = NSString::from_str(t);
            let _ = pb.setString_forType(&ns, NSPasteboardTypeString);
        }
    });
}

fn read_clipboard_text() -> Option<String> {
    objc2::rc::autoreleasepool(|_| unsafe {
        NSPasteboard::generalPasteboard()
            .stringForType(NSPasteboardTypeString)
            .map(|s| s.to_string())
    })
}

fn pasteboard_change_count() -> i64 {
    objc2::rc::autoreleasepool(|_| unsafe {
        NSPasteboard::generalPasteboard().changeCount() as i64
    })
}

fn send_cmd_c() {
    let src = match CGEventSource::new(CGEventSourceStateID::CombinedSessionState) {
        Ok(s) => s,
        Err(_) => return,
    };
    if let Ok(down) = CGEvent::new_keyboard_event(src.clone(), KEY_CODE_C, true) {
        down.set_flags(CGEventFlags::CGEventFlagCommand);
        down.post(CGEventTapLocation::HID);
    }
    if let Ok(up) = CGEvent::new_keyboard_event(src, KEY_CODE_C, false) {
        up.set_flags(CGEventFlags::CGEventFlagCommand);
        up.post(CGEventTapLocation::HID);
    }
}

/// Return the current mouse cursor location in **top-left origin** points,
/// matching the AX/Tauri-logical coordinate space.
///
/// `NSEvent.mouseLocation` returns bottom-left origin (Cocoa convention); we
/// flip using the primary display's height (`CGDisplay::main()` is thread-
/// safe; `NSScreen::mainScreen()` would require the main thread).
fn mouse_location_top_left() -> Option<(f64, f64)> {
    let pt = unsafe { objc2::rc::autoreleasepool(|_| NSEvent::mouseLocation()) };
    let main_h = CGDisplay::main().bounds().size.height;
    Some((pt.x, main_h - pt.y))
}

// ---------------------------------------------------------------------------
// Chromium AX activation.
// ---------------------------------------------------------------------------

/// Browsers in the Chromium family only enable their AX tree on demand. By
/// writing `AXManualAccessibility` (and the older `AXEnhancedUserInterface`)
/// to the app element, we force them to expose text widgets so our
/// `kAXSelectedText` reads return a value. PopClip and a11y utilities like
/// VoiceOver use the same trick.
fn ensure_app_ax_enabled(pid: i32, bundle: Option<&str>) {
    let Some(b) = bundle else { return };
    if !CHROMIUM_BUNDLES.contains(&b) {
        return;
    }
    {
        let set = AX_ACTIVATED_PIDS.lock();
        if set.contains(&pid) {
            return;
        }
    }
    unsafe {
        let app = AXUIElementCreateApplication(pid);
        if app.is_null() {
            return;
        }
        let true_val = CFBoolean::true_value();
        let true_ref = true_val.as_concrete_TypeRef() as CFTypeRef;
        let manual = CFString::from_static_string("AXManualAccessibility");
        let _ = AXUIElementSetAttributeValue(app, manual.as_concrete_TypeRef(), true_ref);
        let enhanced = CFString::from_static_string("AXEnhancedUserInterface");
        let _ = AXUIElementSetAttributeValue(app, enhanced.as_concrete_TypeRef(), true_ref);
        CFRelease(app as CFTypeRef);
    }
    AX_ACTIVATED_PIDS.lock().insert(pid);
    tracing::info!(
        target: "cairn_lib::selection_popover",
        pid, bundle = b,
        "enabled Chromium AX (AXManualAccessibility + AXEnhancedUserInterface)"
    );
}

// ---------------------------------------------------------------------------
// Mouse-button polling (Quartz event source).
// ---------------------------------------------------------------------------

const COMBINED_SESSION_STATE: u32 = 0;
const MOUSE_BUTTON_LEFT: u32 = 0;

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventSourceButtonState(state_id: u32, button: u32) -> bool;
}

fn mouse_left_down() -> bool {
    // SAFETY: pure FFI, no pointers passed.
    unsafe { CGEventSourceButtonState(COMBINED_SESSION_STATE, MOUSE_BUTTON_LEFT) }
}

// ---------------------------------------------------------------------------
// Frontmost-application identity.
// ---------------------------------------------------------------------------

fn frontmost_app() -> Option<(i32, Option<String>, Option<String>)> {
    use objc2_app_kit::NSWorkspace;
    objc2::rc::autoreleasepool(|_| unsafe {
        let workspace = NSWorkspace::sharedWorkspace();
        let app = workspace.frontmostApplication()?;
        let pid = app.processIdentifier();
        let bid = app.bundleIdentifier().map(|s| s.to_string());
        let name = app.localizedName().map(|s| s.to_string());
        Some((pid, bid, name))
    })
}

// ---------------------------------------------------------------------------
// Manual CFType release guard for refs that don't have a Rust wrapper.
// ---------------------------------------------------------------------------

struct CFGuard(CFTypeRef);

impl CFGuard {
    fn new(p: CFTypeRef) -> Self {
        CFGuard(p)
    }
}

impl Drop for CFGuard {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: we own the +1 from a Copy* / Create* call.
            unsafe { CFRelease(self.0) };
        }
    }
}
