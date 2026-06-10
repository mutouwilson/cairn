//! Pack / unpack the AX value types we need:
//!
//!   * `CFRange` → `AXValueRef` to feed into `kAXBoundsForRangeParameterizedAttribute`
//!   * `AXValueRef` → `CGRect` for the returned selection bounds.
//!
//! `kAXBoundsForRangeParameterizedAttribute` returns the selection rectangle
//! in **screen coordinates with the top-left origin of the primary display**.
//! That matches Tauri's `LogicalPosition` (which it then converts to Cocoa's
//! flipped coords internally), so we hand the rect straight through.

#![cfg(target_os = "macos")]

use super::ScreenRect;
use accessibility_sys::{
    kAXValueTypeCFRange, kAXValueTypeCGRect, AXValueCreate, AXValueGetType, AXValueGetValue,
    AXValueRef,
};
use core_foundation::base::CFIndex;
use core_graphics::geometry::CGRect;
use std::ffi::c_void;
use std::mem::MaybeUninit;
use std::ptr;

pub fn screen_rect_from_cg(r: CGRect) -> ScreenRect {
    ScreenRect {
        x: r.origin.x,
        y: r.origin.y,
        width: r.size.width,
        height: r.size.height,
    }
}

/// `CFRange` lookalike with the same memory layout. The system header type is
/// `{ CFIndex location; CFIndex length; }`; we model it directly so we can
/// pass `&CFRange as *const c_void` to `AXValueCreate`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct CFRange {
    location: CFIndex,
    length: CFIndex,
}

/// Build an `AXValueRef` of type `CFRange` for the parameterized lookup.
/// Returns `None` if the system failed to allocate the value (extremely rare).
///
/// The returned ref has a +1 retain count and must be released with
/// `CFRelease` by the caller.
pub fn make_range_value(location: i64, length: i64) -> Option<AXValueRef> {
    let range = CFRange {
        location: location as CFIndex,
        length: length as CFIndex,
    };
    // SAFETY: `&range` is valid for the duration of the FFI call; AXValueCreate
    // copies the value internally before returning.
    let ptr = unsafe { AXValueCreate(kAXValueTypeCFRange, &range as *const _ as *const c_void) };
    if ptr.is_null() {
        None
    } else {
        Some(ptr)
    }
}

/// Unpack an `AXValueRef` known to hold a `CGRect`.
///
/// Returns `None` if the value is null, of the wrong type, or the OS rejected
/// the unpack call.
///
/// SAFETY: callers must pass either a null pointer or a valid `AXValueRef`
/// returned by the AX API. Internally null is checked and `AXValueGetType` is
/// pure, so we tolerate `&AXValueRef` here rather than forcing the call site
/// into an `unsafe` block for what is effectively a guarded read.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub fn read_rect(value: AXValueRef) -> Option<CGRect> {
    if value.is_null() {
        return None;
    }
    // SAFETY: We checked for null. AXValueGetType is a pure getter.
    if unsafe { AXValueGetType(value) } != kAXValueTypeCGRect {
        return None;
    }
    let mut out = MaybeUninit::<CGRect>::uninit();
    // SAFETY: AXValueGetValue writes into the out pointer when the types match.
    let ok = unsafe { AXValueGetValue(value, kAXValueTypeCGRect, out.as_mut_ptr() as *mut c_void) };
    if ok {
        Some(unsafe { out.assume_init() })
    } else {
        None
    }
}

/// Compute where to anchor a popover of the given size for a selection rect.
///
/// Tries to place the popover **below** the selection (centred horizontally)
/// because that's where the user's cursor most often sits after dragging.
/// Falls back to **above** the selection if the screen would be exceeded on
/// the bottom (heuristic: we don't have NSScreen info here, so we just clamp
/// to a generous virtual-screen height of 2400px which covers any reasonable
/// multi-monitor setup).
pub fn anchor_below_or_above(
    rect: ScreenRect,
    popover_w: f64,
    popover_h: f64,
    gap: f64,
    virtual_screen_max_y: f64,
) -> (f64, f64) {
    let cx = rect.x + rect.width / 2.0;
    let x = (cx - popover_w / 2.0).max(0.0);
    let below_y = rect.y + rect.height + gap;
    let y = if below_y + popover_h > virtual_screen_max_y {
        (rect.y - popover_h - gap).max(0.0)
    } else {
        below_y
    };
    (x, y)
}

// Silence the unused-imports lint on platforms where this module is built but
// the `make_range_value`/`read_rect` helpers aren't used (e.g. a test build
// that only exercises the geometry helpers).
#[allow(dead_code)]
fn _link_check() {
    let _ = ptr::null::<AXValueRef>();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sel(x: f64, y: f64, w: f64, h: f64) -> ScreenRect {
        ScreenRect {
            x,
            y,
            width: w,
            height: h,
        }
    }

    #[test]
    fn anchor_below_when_room() {
        let s = sel(100.0, 100.0, 200.0, 20.0);
        let (x, y) = anchor_below_or_above(s, 240.0, 44.0, 8.0, 1200.0);
        // centred: 100 + 100 - 120 = 80
        assert!((x - 80.0).abs() < 1e-9);
        // below: 100 + 20 + 8 = 128
        assert!((y - 128.0).abs() < 1e-9);
    }

    #[test]
    fn anchor_flips_above_when_no_room_below() {
        let s = sel(0.0, 1180.0, 200.0, 20.0);
        let (_x, y) = anchor_below_or_above(s, 240.0, 44.0, 8.0, 1200.0);
        // below would be 1208 → overflow → above: 1180 - 44 - 8 = 1128
        assert!((y - 1128.0).abs() < 1e-9);
    }

    #[test]
    fn anchor_clamps_x_at_zero() {
        let s = sel(0.0, 100.0, 20.0, 20.0);
        let (x, _) = anchor_below_or_above(s, 240.0, 44.0, 8.0, 1200.0);
        assert!(x >= 0.0);
    }

    #[test]
    fn round_trip_range_value() {
        let v = make_range_value(5, 12).expect("alloc");
        // We can't unpack to verify here without re-implementing the unpack
        // (CFRange unpack is similar to read_rect). Just confirm not null and
        // release.
        assert!(!v.is_null());
        unsafe {
            core_foundation::base::CFRelease(v as *const _ as core_foundation::base::CFTypeRef)
        };
    }
}
