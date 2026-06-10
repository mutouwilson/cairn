//! Cairn portable bundle format (Phase 3c).
//!
//! A bundle is a single JSON file (`*.cairn-bundle.json`) capturing the
//! shareable parts of a user's memory:
//!   - notes
//!   - entities
//!   - relations
//!   - themes (semantic memories — derived, but worth copying for warm-start)
//!
//! It explicitly **does not** carry:
//!   - the audit log (per-device cryptographic chain)
//!   - the audit signing key (per-device identity)
//!   - the embeddings (large + provider-specific; re-embed on import)
//!   - the agent permission matrix (per-device security policy)
//!
//! Every bundle is signed by the exporting device's audit key so the recipient
//! can verify origin even when the transport is untrusted. Import uses per-row
//! last-writer-wins keyed by `ulid + updated_at`.

pub mod export;
pub mod import;
pub mod types;

#[cfg(test)]
mod tests;

pub use types::{Bundle, BundleCounts, BundleHeader, EntityRow, NoteRow, RelationRow, ThemeRow};

pub const BUNDLE_FORMAT: &str = "cairn-bundle/v1";
