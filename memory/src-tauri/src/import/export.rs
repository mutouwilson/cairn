//! Phase 7 · V6 M2 — write a Cairn-managed block back into the user's
//! `~/.claude/CLAUDE.md` so Claude Code sees the memories Cairn knows.
//!
//! ## Markers
//!
//! We bracket our content with HTML comment markers that markdown renders
//! invisibly but a human reader can spot:
//!
//! ```markdown
//! <!-- cairn:start v1 -->
//! ... bullets ...
//! <!-- cairn:end -->
//! ```
//!
//! `v1` lets us evolve the format without breaking old blocks.
//!
//! ## 3-way merge (lightweight)
//!
//! We don't run a real merge tool. Per request the contract is:
//!
//! 1. Compute sha256 of the *current* block content on disk.
//! 2. Compare against `exported_blocks.last_written_sha256`.
//! 3. Mismatch → user edited inside the block → CONFLICT.
//!    Block missing while a prior row exists → CONFLICT.
//! 4. On conflict we refuse to overwrite unless the caller passes
//!    `force = true`. The preview surfaces both versions so the user can
//!    copy their edits to Cairn (via re-capture) before forcing.
//!
//! Content *outside* the markers is always preserved verbatim.

use crate::db::Db;
use crate::schema::Entity;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

pub const MARK_START: &str = "<!-- cairn:start v1 -->";
pub const MARK_END: &str = "<!-- cairn:end -->";
pub const DEFAULT_MAX_ENTITIES: usize = 50;
pub const EXPORTABLE_TYPES: &[&str] = &["Preference", "Goal", "Belief"];

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ConflictReason {
    /// Markers present but content sha differs from what we last wrote.
    UserEditedBlock,
    /// We had a row in `exported_blocks` but the markers are gone from disk.
    BlockRemoved,
    /// File on disk is missing but we had a prior row.
    FileRemoved,
}

#[derive(Debug, Clone, Default)]
pub struct ExportOptions {
    /// Override `$HOME`. Tests + dev.
    pub home: Option<PathBuf>,
    /// Skip the conflict gate. The caller is asserting "I've reviewed the
    /// diff; just overwrite."
    pub force: bool,
    /// Cap entity rows pulled. Defaults to `DEFAULT_MAX_ENTITIES`.
    pub max_entities: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExportPreview {
    pub dest_path: PathBuf,
    pub file_exists: bool,
    pub had_block: bool,
    pub current_block: Option<String>,
    pub proposed_block: String,
    pub conflict: Option<ConflictReason>,
    pub entity_count: usize,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ExportResult {
    pub written: bool,
    pub conflict: Option<ConflictReason>,
    pub entity_count: usize,
    pub bytes_written: usize,
    pub dest_path: Option<PathBuf>,
}

pub async fn export_preview(db: &Db, opts: ExportOptions) -> Result<ExportPreview> {
    let dest = resolve_dest(opts.home.as_deref());
    let existing = std::fs::read_to_string(&dest).ok();
    let file_exists = existing.is_some();
    let parts = existing.as_deref().map(split_block);
    let current_block = parts.as_ref().and_then(|p| p.inner.map(|s| s.to_string()));

    let prior = db.exported_block_by_path(&dest).await?;
    let conflict = detect_conflict(&prior, &current_block, file_exists);

    let entities =
        fetch_exportable_entities(db, opts.max_entities.unwrap_or(DEFAULT_MAX_ENTITIES)).await?;
    let proposed_block = render_block(&entities);

    Ok(ExportPreview {
        dest_path: dest,
        file_exists,
        had_block: current_block.is_some(),
        current_block,
        proposed_block,
        conflict,
        entity_count: entities.len(),
    })
}

pub async fn export_run(db: &Db, opts: ExportOptions) -> Result<ExportResult> {
    let dest = resolve_dest(opts.home.as_deref());
    let existing = std::fs::read_to_string(&dest).unwrap_or_default();
    let file_existed_before = !existing.is_empty() || dest.exists(); // empty file still "exists"

    let parts = split_block(&existing);
    let current_block = parts.inner.map(|s| s.to_string());

    let prior = db.exported_block_by_path(&dest).await?;
    let conflict = detect_conflict(&prior, &current_block, file_existed_before);

    if conflict.is_some() && !opts.force {
        return Ok(ExportResult {
            written: false,
            conflict,
            entity_count: 0,
            bytes_written: 0,
            dest_path: Some(dest),
        });
    }

    let entities =
        fetch_exportable_entities(db, opts.max_entities.unwrap_or(DEFAULT_MAX_ENTITIES)).await?;
    let block = render_block(&entities);
    let new_text = rebuild(parts, &block);

    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("mkdir {parent:?}"))?;
    }
    std::fs::write(&dest, &new_text).with_context(|| format!("write {dest:?}"))?;

    let sha = sha256_hex(block.as_bytes());
    let entity_ids: Vec<String> = entities.iter().map(|e| e.id.clone()).collect();
    db.upsert_exported_block(&dest, &sha, &block, &entity_ids)
        .await?;

    Ok(ExportResult {
        written: true,
        conflict: None,
        entity_count: entities.len(),
        bytes_written: new_text.len(),
        dest_path: Some(dest),
    })
}

fn resolve_dest(home_override: Option<&Path>) -> PathBuf {
    let home = home_override.map(|p| p.to_path_buf()).unwrap_or_else(|| {
        directories::UserDirs::new()
            .map(|d| d.home_dir().to_path_buf())
            .unwrap_or_else(|| PathBuf::from("/tmp/cairn-no-home"))
    });
    home.join(".claude").join("CLAUDE.md")
}

fn detect_conflict(
    prior: &Option<crate::db::queries::ExportedBlockRow>,
    current_block: &Option<String>,
    file_exists: bool,
) -> Option<ConflictReason> {
    let Some(p) = prior else { return None }; // first export — never a conflict
    if !file_exists {
        return Some(ConflictReason::FileRemoved);
    }
    match current_block {
        None => Some(ConflictReason::BlockRemoved),
        Some(s) => {
            let sha = sha256_hex(s.as_bytes());
            if sha == p.last_written_sha256 {
                None
            } else {
                Some(ConflictReason::UserEditedBlock)
            }
        }
    }
}

/// Pull entities Cairn would like to surface. We pick a generous superset
/// (Preference / Goal / Belief), sort by an importance proxy
/// (`base_importance × log(1+access_count)`), and cap.
async fn fetch_exportable_entities(db: &Db, max: usize) -> Result<Vec<Entity>> {
    let mut all: Vec<Entity> = Vec::new();
    for ty in EXPORTABLE_TYPES {
        let rows = db.list_entities(Some(ty), 500).await?;
        all.extend(rows);
    }
    all.sort_by(|a, b| {
        let sa = a.base_importance * ((a.access_count as f64 + 1.0).ln() + 1.0);
        let sb = b.base_importance * ((b.access_count as f64 + 1.0).ln() + 1.0);
        sb.partial_cmp(&sa).unwrap_or(std::cmp::Ordering::Equal)
    });
    all.truncate(max);
    Ok(all)
}

/// Format the chosen entities as the markdown bullet list that goes between
/// our markers. Always returns text ending in `\n` so concatenation is safe.
pub fn render_block(entities: &[Entity]) -> String {
    let mut out = String::new();
    out.push_str(
        "<!-- Cairn-managed memories. Edit Cairn (not this block) — \
         changes inside the markers will be flagged as conflicts on next sync. -->\n\n",
    );
    if entities.is_empty() {
        out.push_str("_(Cairn has no exportable memories yet.)_\n");
        return out;
    }
    for e in entities {
        let snippet = property_snippet(&e.properties);
        if snippet.is_empty() {
            out.push_str(&format!("- **{}**: {}\n", e.r#type, e.name));
        } else {
            out.push_str(&format!("- **{}** — {}: {}\n", e.r#type, e.name, snippet));
        }
    }
    out
}

fn property_snippet(properties_json: &str) -> String {
    let v: serde_json::Value =
        serde_json::from_str(properties_json).unwrap_or(serde_json::Value::Null);
    let obj = match v {
        serde_json::Value::Object(o) => o,
        _ => return String::new(),
    };
    // Prefer human-friendly keys first; fall back to any string value.
    for key in [
        "statement",
        "description",
        "summary",
        "note",
        "preference",
        "goal",
    ] {
        if let Some(val) = obj.get(key).and_then(|v| v.as_str()) {
            return truncate(val, 240);
        }
    }
    for (_k, v) in &obj {
        if let Some(s) = v.as_str() {
            return truncate(s, 240);
        }
    }
    String::new()
}

fn truncate(s: &str, max_chars: usize) -> String {
    let n = s.chars().count();
    if n <= max_chars {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max_chars).collect();
        out.push('…');
        out
    }
}

#[derive(Debug)]
pub struct Parts<'a> {
    pub before: &'a str,
    pub inner: Option<&'a str>,
    pub after: &'a str,
}

/// Split a markdown document into (before-block, block-inner, after-block).
/// When no markers are present, `inner == None` and `before == whole_text`.
pub fn split_block(text: &str) -> Parts<'_> {
    if let Some(start_i) = text.find(MARK_START) {
        let header_end = start_i + MARK_START.len();
        let line_end = text[header_end..]
            .find('\n')
            .map(|i| header_end + i + 1)
            .unwrap_or(text.len());
        if let Some(end_rel) = text[line_end..].find(MARK_END) {
            let end_i = line_end + end_rel;
            let after_close = end_i + MARK_END.len();
            // Eat the trailing newline after the end marker so the next
            // round-trip doesn't accumulate blank lines.
            let after = text[after_close..]
                .strip_prefix('\n')
                .unwrap_or(&text[after_close..]);
            return Parts {
                before: &text[..start_i],
                inner: Some(&text[line_end..end_i]),
                after,
            };
        }
    }
    Parts {
        before: text,
        inner: None,
        after: "",
    }
}

pub fn rebuild(parts: Parts<'_>, block: &str) -> String {
    let wrapped = format!("{MARK_START}\n{block}{MARK_END}\n");
    let mut out = String::new();
    if parts.inner.is_some() {
        out.push_str(parts.before);
        out.push_str(&wrapped);
        out.push_str(parts.after);
    } else {
        let trimmed = parts.before.trim_end_matches('\n');
        out.push_str(trimmed);
        if !trimmed.is_empty() {
            out.push_str("\n\n");
        }
        out.push_str(&wrapped);
    }
    out
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    let out = h.finalize();
    let mut s = String::with_capacity(64);
    for b in out {
        use std::fmt::Write;
        let _ = write!(&mut s, "{b:02x}");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_block_with_no_markers_returns_none() {
        let txt = "Just plain markdown.\nNo markers here.";
        let p = split_block(txt);
        assert!(p.inner.is_none());
        assert_eq!(p.before, txt);
        assert_eq!(p.after, "");
    }

    #[test]
    fn split_block_extracts_inner_and_outside() {
        let txt = "Header text.\n\n<!-- cairn:start v1 -->\n- A\n- B\n<!-- cairn:end -->\nFooter.";
        let p = split_block(txt);
        let inner = p.inner.expect("should find inner");
        assert!(inner.contains("- A"));
        assert!(inner.contains("- B"));
        assert!(p.before.starts_with("Header"));
        assert_eq!(p.after, "Footer.");
    }

    #[test]
    fn rebuild_replaces_existing_block() {
        let txt = "Pre\n<!-- cairn:start v1 -->\nold\n<!-- cairn:end -->\nPost";
        let parts = split_block(txt);
        let out = rebuild(parts, "new\n");
        assert!(out.contains("new"));
        assert!(!out.contains("old"));
        assert!(out.starts_with("Pre"));
        assert!(out.ends_with("Post"));
    }

    #[test]
    fn rebuild_appends_when_no_marker_present() {
        let txt = "Just user content.\n";
        let parts = split_block(txt);
        let out = rebuild(parts, "new block\n");
        assert!(out.starts_with("Just user content."));
        assert!(out.contains(MARK_START));
        assert!(out.contains("new block"));
        assert!(out.contains(MARK_END));
    }

    #[test]
    fn rebuild_handles_empty_input() {
        let parts = split_block("");
        let out = rebuild(parts, "first block\n");
        assert!(out.starts_with(MARK_START));
        assert!(out.contains("first block"));
    }

    #[test]
    fn detect_conflict_no_prior() {
        let conflict = detect_conflict(&None, &Some("x".into()), true);
        assert!(conflict.is_none());
    }

    #[test]
    fn detect_conflict_user_edited() {
        let prior = crate::db::queries::ExportedBlockRow {
            id: "1".into(),
            dest_path: "/tmp/x".into(),
            last_written_sha256: sha256_hex(b"original"),
            last_block_content: "original".into(),
            entity_ids_json: "[]".into(),
            last_written_at: 0,
        };
        let conflict = detect_conflict(&Some(prior), &Some("edited".into()), true);
        assert_eq!(conflict, Some(ConflictReason::UserEditedBlock));
    }

    #[test]
    fn detect_conflict_block_removed() {
        let prior = crate::db::queries::ExportedBlockRow {
            id: "1".into(),
            dest_path: "/tmp/x".into(),
            last_written_sha256: sha256_hex(b"original"),
            last_block_content: "original".into(),
            entity_ids_json: "[]".into(),
            last_written_at: 0,
        };
        let conflict = detect_conflict(&Some(prior), &None, true);
        assert_eq!(conflict, Some(ConflictReason::BlockRemoved));
    }

    #[test]
    fn render_block_handles_empty() {
        let block = render_block(&[]);
        assert!(block.contains("no exportable memories"));
    }

    #[test]
    fn render_block_formats_entities() {
        let e = Entity {
            id: "e1".into(),
            r#type: "Preference".into(),
            name: "pnpm > npm".into(),
            properties: r#"{"statement":"Use pnpm instead of npm"}"#.into(),
            confidence: 0.9,
            source_note_id: None,
            created_at: 0,
            updated_at: 0,
            strength: 1.0,
            last_accessed: 0,
            access_count: 0,
            base_importance: 0.5,
            is_sticky: 0,
            archived_at: None,
        };
        let block = render_block(&[e]);
        assert!(block.contains("Preference"));
        assert!(block.contains("pnpm > npm"));
        assert!(block.contains("Use pnpm instead of npm"));
    }

    #[test]
    fn round_trip_preserves_outside_content() {
        let original =
            "# Header\n\nsome notes\n\n<!-- cairn:start v1 -->\nfirst\n<!-- cairn:end -->\n\nfooter\n";
        let parts = split_block(original);
        let out = rebuild(parts, "second\n");
        // outside parts must still be there
        assert!(out.contains("# Header"));
        assert!(out.contains("some notes"));
        assert!(out.contains("footer"));
        assert!(out.contains("second"));
        assert!(!out.contains("first"));
        // no duplicate blank lines accumulating
        assert!(!out.contains("\n\n\n\n"));
    }
}
