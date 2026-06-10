//! Accessibility (AX) permission check + nudge.
//!
//! Cairn needs AX trust to read the focused element's selected text out of
//! any application. Without it, every `AXUIElementCopy*` call returns
//! `kAXErrorAPIDisabled` (`-25211`) and we get nothing back.
//!
//! We can't *grant* AX permission programmatically — only the user can do
//! that in System Settings. What we can do:
//!   * Call `AXIsProcessTrustedWithOptions(...)` with the prompt option set
//!     once per session, which triggers the OS sheet the first time.
//!   * Open the Accessibility pane directly via the `x-apple.systempreferences`
//!     URL when the user clicks "Open System Settings" in our UI.
//!
//! The prompt behavior depends on the bundle id: ad-hoc unsigned debug builds
//! prompt every time the binary hash changes, signed release builds prompt
//! once. This is a TCC restriction, not something we can paper over.

#![cfg(target_os = "macos")]

use accessibility_sys::AXIsProcessTrustedWithOptions;
use core_foundation::base::TCFType;
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::CFDictionary;
use core_foundation::string::CFString;

const PROMPT_KEY: &str = "AXTrustedCheckOptionPrompt";

/// Returns true if Cairn is currently in the system's AX trust list.
///
/// When `prompt` is `true`, the first call also pops the standard "<App> would
/// like to control this computer using accessibility features" sheet. The
/// sheet appears once per bundle identity; subsequent calls with `prompt:true`
/// are no-ops if already trusted.
pub fn is_trusted(prompt: bool) -> bool {
    let key = CFString::from_static_string(PROMPT_KEY);
    let value: CFBoolean = if prompt {
        CFBoolean::true_value()
    } else {
        CFBoolean::false_value()
    };
    let pairs = [(key, value)];
    let options = CFDictionary::from_CFType_pairs(&pairs);
    // SAFETY: CFDictionary ref stays valid for the duration of the call.
    unsafe { AXIsProcessTrustedWithOptions(options.as_concrete_TypeRef()) }
}

/// Open `Privacy & Security → Accessibility` directly via the standard
/// system-preferences URL scheme. Best-effort; ignored if the `open` binary
/// is missing.
pub fn open_settings_pane() -> std::io::Result<()> {
    std::process::Command::new("open")
        .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
        .status()
        .map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_trusted_does_not_panic() {
        // We don't assert true/false — depends on the test runner's TCC
        // state. The important thing is that the FFI signature lines up and
        // the call returns without UB.
        let _ = is_trusted(false);
    }
}
