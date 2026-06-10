//! Phase 7 · V6 — Import dev-tool memory files into Cairn.
//!
//! Per user constraint P-2 ("only user-level memory"), this module covers
//! exclusively files under `$HOME`:
//!   * `~/.claude/CLAUDE.md`             — global Claude Code memory
//!   * `~/.claude/projects/*/memory/*.md` — per-session auto memory
//!
//! In-repo files (`<repo>/CLAUDE.md`, `.cursor/rules/`, `AGENTS.md`) are
//! intentionally NOT scanned — they're project-level and would inject
//! per-project tagging that Cairn doesn't model.
//!
//! Milestones (see `research/22-v6-implementation-plan.md` §3):
//!   * **M0** — spike, single source, no dedup.
//!   * **M1 (this commit)** — scanner enumerates all user-level sources,
//!     per-kind parser dispatch, sha256-based dedup via `imported_docs`
//!     table.
//!   * **M2 (W4)** — reverse export with managed blocks + 3-way merge.

pub mod export;
pub mod parsers;
pub mod scanner;
pub mod watcher;

use crate::capture::processor::{process_note, ProcessingContext};
use crate::db::Db;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SourceKind {
    /// `~/.claude/CLAUDE.md`
    ClaudeMdGlobal,
    /// `~/.claude/projects/<encoded>/memory/*.md`
    ClaudeAutoMemory,
    /// `~/.cursor/skills-cursor/<name>/SKILL.md` — user-authored Cursor skills.
    CursorSkill,
    /// `~/.cursor/plans/*.plan.md` — research / implementation plans.
    CursorPlan,
    /// `~/.codex/memories/*.md` — Codex CLI user-level memory. Forward-compat;
    /// dir is empty in the wild as of 2026-05 but Codex may start writing.
    CodexMemory,
}

impl SourceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            SourceKind::ClaudeMdGlobal => "claude-md-global",
            SourceKind::ClaudeAutoMemory => "claude-auto",
            SourceKind::CursorSkill => "cursor-skill",
            SourceKind::CursorPlan => "cursor-plan",
            SourceKind::CodexMemory => "codex-memory",
        }
    }

    pub fn source_string(self, file_name: &str) -> String {
        format!("import:{}:{}", self.as_str(), file_name)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ImportedDocSummary {
    pub source_path: PathBuf,
    pub source_kind: SourceKind,
    pub note_id: String,
    pub bytes: usize,
    /// Frontmatter-derived display name when available, else file stem.
    pub display_name: Option<String>,
    pub status: DocStatus,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum DocStatus {
    /// New file we just wrote.
    Imported,
    /// File matched an existing imported_docs row with the same sha256 — skipped.
    Unchanged,
    /// File was imported before, but the note it produced has since been
    /// deleted by the user. We keep the `imported_docs` row so the UI can
    /// show this as "Unlinked" and offer a per-file re-sync. NOT re-imported
    /// automatically — the deletion was deliberate; the user opts back in
    /// via the per-row re-sync button.
    Unlinked,
    /// File matched an existing row but content changed — updated.
    Updated,
    /// User marked this file "do not sync" (persistent `import_skips` row).
    /// Never imported by Apply or the watcher until the user unskips it.
    Skipped,
    /// `dry_run = true` was set; the row above describes what would happen.
    DryRun,
    /// Read or parse error.
    Failed,
    /// File was empty after trim — skipped.
    Empty,
}

#[derive(Debug, Default, Serialize)]
pub struct ImportSummary {
    pub scanned: usize,
    pub imported: usize,
    pub updated: usize,
    pub skipped_empty: usize,
    pub skipped_dup: usize,
    /// Files the user marked "do not sync" (persistent skip).
    pub skipped: usize,
    /// Files whose imported note was deleted by the user — surfaced as
    /// `DocStatus::Unlinked`, not re-imported automatically.
    pub unlinked: usize,
    pub failed: usize,
    pub docs: Vec<ImportedDocSummary>,
}

#[derive(Debug, Default, Clone)]
pub struct ImportOptions {
    /// Override `$HOME`. Only used by tests + the optional `root` IPC arg
    /// (kept for backwards compat with M0 callers).
    pub home: Option<PathBuf>,
    /// `true` returns what *would* happen, no DB writes.
    pub dry_run: bool,
    /// Absolute paths the user deselected in the preview. Skipped silently
    /// on Apply (not even surfaced as `skipped_empty` — the user already
    /// told us, no need to re-explain).
    pub exclude_paths: Vec<PathBuf>,
    /// Paths to force-reimport even when the content hash is unchanged
    /// (the per-file "re-sync" button). Bypasses the dedup short-circuit so
    /// a deleted/unlinked note can be pulled back, or extraction re-run.
    pub force_paths: Vec<PathBuf>,
    /// When non-empty, restrict the run to exactly these paths. Lets the
    /// re-sync button touch a single file instead of re-scanning everything.
    pub only_paths: Vec<PathBuf>,
}

/// M1 entry point. Walks every user-level source the scanner knows about,
/// dispatches to the right parser, dedups via `imported_docs.content_sha256`,
/// and (when `dry_run = false`) inserts a note + schedules extraction.
pub async fn run_import(
    db: &Db,
    processing: Option<&ProcessingContext>,
    opts: ImportOptions,
) -> Result<ImportSummary> {
    use std::collections::HashSet;
    let exclude: HashSet<PathBuf> = opts.exclude_paths.iter().cloned().collect();
    let force: HashSet<PathBuf> = opts.force_paths.iter().cloned().collect();
    let only: HashSet<PathBuf> = opts.only_paths.iter().cloned().collect();
    let discovered: Vec<scanner::DiscoveredFile> = scanner::scan_user_sources(opts.home.as_deref())
        .into_iter()
        .filter(|f| !exclude.contains(&f.path))
        .filter(|f| only.is_empty() || only.contains(&f.path))
        .collect();
    // Persistent "do not sync" set — highest priority: a skipped file is
    // never read or imported (manual Apply or watcher) until unskipped.
    let skip_set = db.list_import_skips().await.unwrap_or_default();
    tracing::info!(
        files = discovered.len(),
        dry_run = opts.dry_run,
        skips = skip_set.len(),
        "import scanner discovered"
    );

    let mut summary = ImportSummary::default();

    for file in discovered {
        summary.scanned += 1;
        // Skip wins over everything, including force_paths/only_paths — a
        // skipped file is never re-imported. The UI never wires a re-sync
        // button onto SKIPPED rows, so force can't collide with skip in
        // practice; this short-circuit is the documented invariant should a
        // future caller try (unskip first, then resync).
        if skip_set.contains(file.path.to_string_lossy().as_ref()) {
            summary.skipped += 1;
            summary.docs.push(ImportedDocSummary {
                source_path: file.path.clone(),
                source_kind: file.kind,
                note_id: String::new(),
                bytes: 0,
                display_name: None,
                status: DocStatus::Skipped,
            });
            continue;
        }
        let bytes = match std::fs::read_to_string(&file.path) {
            Ok(b) => b,
            Err(e) => {
                tracing::warn!(?file.path, error = %e, "read failed");
                summary.failed += 1;
                summary.docs.push(ImportedDocSummary {
                    source_path: file.path.clone(),
                    source_kind: file.kind,
                    note_id: String::new(),
                    bytes: 0,
                    display_name: None,
                    status: DocStatus::Failed,
                });
                continue;
            }
        };
        if bytes.trim().is_empty() {
            summary.skipped_empty += 1;
            summary.docs.push(ImportedDocSummary {
                source_path: file.path.clone(),
                source_kind: file.kind,
                note_id: String::new(),
                bytes: 0,
                display_name: None,
                status: DocStatus::Empty,
            });
            continue;
        }

        let sha = sha256_hex(bytes.as_bytes());
        let (note_text, display_name) = render_note(&file, &bytes);

        // Dedup check against imported_docs (live DB only, dry_run preview
        // still flags the file as Unchanged so the user can see how many
        // would actually be re-imported).
        let prior = match db.imported_doc_by_path(&file.path).await {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!(?file.path, error = %e, "imported_doc lookup failed");
                None
            }
        };
        let forced = force.contains(&file.path);
        let sha_match = prior
            .as_ref()
            .map(|row| row.content_sha256 == sha)
            .unwrap_or(false);
        // Content unchanged and not a force-resync → don't re-import. But
        // distinguish two cases for the UI: the note still exists ("synced")
        // vs. the user deleted it ("unlinked"). Both skip auto-import; the
        // user re-syncs an unlinked file explicitly via the per-row button.
        if sha_match && !forced {
            let note_alive = match prior.as_ref() {
                // On lookup error, assume alive — never falsely flag Unlinked.
                Some(p) => db.note_exists(&p.note_id).await.unwrap_or(true),
                None => false,
            };
            // Unchanged keeps the live note id so the UI can show it; Unlinked
            // has no live note, so hand back an empty id (like the Failed/Empty
            // arms) — never surface a dangling reference to a deleted note.
            let (status, note_id) = if note_alive {
                summary.skipped_dup += 1;
                (
                    DocStatus::Unchanged,
                    prior.as_ref().map(|r| r.note_id.clone()).unwrap_or_default(),
                )
            } else {
                summary.unlinked += 1;
                (DocStatus::Unlinked, String::new())
            };
            summary.docs.push(ImportedDocSummary {
                source_path: file.path.clone(),
                source_kind: file.kind,
                note_id,
                bytes: bytes.len(),
                display_name,
                status,
            });
            continue;
        }

        if opts.dry_run {
            summary.docs.push(ImportedDocSummary {
                source_path: file.path.clone(),
                source_kind: file.kind,
                note_id: "(dry-run)".to_string(),
                bytes: bytes.len(),
                display_name,
                status: DocStatus::DryRun,
            });
            continue;
        }

        // Live import: insert (or replace) the note + upsert imported_docs.
        // If `prior` exists with a different sha, we delete the old note so
        // the user doesn't accumulate stale duplicates per source file.
        // Sticky entities pinned to the old note (manual merges + manual
        // edits) survive the cascade and get logged to audit so the user
        // can audit what the import-watcher churn left behind.
        if let Some(p) = &prior {
            match db.delete_note(&p.note_id).await {
                Ok(report) => {
                    if let Some(ctx) = processing {
                        for sticky_id in &report.sticky_kept {
                            let _ = ctx
                                .audit
                                .log(
                                    "system",
                                    "sticky_kept",
                                    Some("sticky_kept"),
                                    Some(&format!("entity={} note={}", sticky_id, p.note_id)),
                                    &serde_json::json!({
                                        "entity_id": sticky_id,
                                        "note_id": p.note_id,
                                        "via": "import_replace",
                                    }),
                                    Some(1),
                                )
                                .await;
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!(error = %e, old_note = %p.note_id, "stale note delete failed");
                }
            }
        }

        let file_name = file
            .path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown.md");
        let source = file.kind.source_string(file_name);

        let note_id = match db.insert_note(&note_text, &source).await {
            Ok(id) => id,
            Err(e) => {
                tracing::warn!(?file.path, error = %e, "insert_note failed");
                summary.failed += 1;
                summary.docs.push(ImportedDocSummary {
                    source_path: file.path.clone(),
                    source_kind: file.kind,
                    note_id: String::new(),
                    bytes: bytes.len(),
                    display_name,
                    status: DocStatus::Failed,
                });
                continue;
            }
        };

        if let Err(e) = db
            .upsert_imported_doc(&file.path, file.kind, &sha, &note_id)
            .await
        {
            tracing::warn!(error = %e, "imported_doc upsert failed; continuing");
        }

        if let Some(ctx) = processing {
            let ctx = ctx.clone();
            let note_id_clone = note_id.clone();
            let text_clone = note_text.clone();
            tokio::spawn(async move {
                if let Err(e) = process_note(&ctx, &note_id_clone, &text_clone).await {
                    tracing::warn!(error = %e, note_id = %note_id_clone, "import extraction failed");
                }
            });
        }

        let status = if prior.is_some() {
            summary.updated += 1;
            DocStatus::Updated
        } else {
            summary.imported += 1;
            DocStatus::Imported
        };
        summary.docs.push(ImportedDocSummary {
            source_path: file.path.clone(),
            source_kind: file.kind,
            note_id,
            bytes: bytes.len(),
            display_name,
            status,
        });
    }

    Ok(summary)
}

fn sha256_hex(bytes: &[u8]) -> String {
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

/// Build the (text-to-store, display-name) pair Cairn shows + extracts from.
fn render_note(file: &scanner::DiscoveredFile, raw: &str) -> (String, Option<String>) {
    match file.kind {
        SourceKind::ClaudeAutoMemory => {
            let parsed = parsers::claude_auto::parse(raw);
            let mut buf = String::new();
            if let Some(name) = &parsed.frontmatter_name {
                buf.push_str("Title: ");
                buf.push_str(name);
                buf.push_str("\n\n");
            }
            if let Some(desc) = &parsed.frontmatter_description {
                buf.push_str(desc);
                buf.push_str("\n\n");
            }
            buf.push_str(parsed.body.trim());
            (buf, parsed.frontmatter_name)
        }
        SourceKind::ClaudeMdGlobal => {
            let parsed = parsers::claude_md::parse(raw);
            let display = parsed.frontmatter_title.clone();
            (parsed.body, display)
        }
        SourceKind::CursorSkill | SourceKind::CursorPlan | SourceKind::CodexMemory => {
            // These three all use YAML-frontmatter+markdown bodies and don't
            // need bespoke parsing. We reuse claude_auto's frontmatter
            // splitter to pull `name` for display, then surface the WHOLE
            // raw file (frontmatter included as a code block) so the LLM
            // extractor can pick up structured fields like Cursor plan's
            // `todos:` list or a SKILL's tool-list metadata.
            let parsed = parsers::claude_auto::parse(raw);
            let display = parsed.frontmatter_name.clone().or_else(|| {
                file.path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .map(|s| s.trim_end_matches(".plan").to_string())
            });
            (raw.trim().to_string(), display)
        }
    }
}
