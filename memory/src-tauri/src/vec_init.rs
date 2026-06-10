//! One-shot registration of the `sqlite-vec` extension as a SQLite auto-extension.
//!
//! Must run **before** any `sqlx` pool opens a connection. We protect with
//! `std::sync::Once` so multiple binary entry points (Tauri app + standalone
//! MCP binary) can both call it safely.

use std::os::raw::{c_char, c_int};
use std::sync::Once;

static INIT: Once = Once::new();

type ExtensionEntry = unsafe extern "C" fn(
    *mut libsqlite3_sys::sqlite3,
    *mut *mut c_char,
    *const libsqlite3_sys::sqlite3_api_routines,
) -> c_int;

/// Register sqlite-vec as an auto-extension. Idempotent across processes /
/// threads via `std::sync::Once`.
pub fn register() {
    INIT.call_once(|| {
        // SAFETY: We transmute the sqlite-vec entry-point fn pointer to the type
        // libsqlite3-sys expects. Both crates wrap the same underlying C ABI
        // (SQLite's extension contract) but expose their own Rust types for
        // `sqlite3`, `sqlite3_api_routines`, etc. The transmute is sound
        // because the layouts are identical (they're FFI bindings of the same
        // C structs).
        unsafe {
            let entry: ExtensionEntry =
                std::mem::transmute(sqlite_vec::sqlite3_vec_init as *const ());
            let rc = libsqlite3_sys::sqlite3_auto_extension(Some(entry));
            if rc != libsqlite3_sys::SQLITE_OK {
                tracing::error!(
                    "sqlite3_auto_extension(sqlite_vec) returned non-zero: {}",
                    rc
                );
            } else {
                tracing::info!("sqlite-vec auto-extension registered");
            }
        }
    });
}
