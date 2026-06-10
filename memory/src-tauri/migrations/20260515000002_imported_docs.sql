-- Phase 7 · V6 M1 — track dev-tool memory files we've imported so re-runs are idempotent.
--
-- Key is `source_path` (one row per file on disk). `content_sha256` lets us
-- skip re-import when the file hasn't changed since last sync. `note_id` is
-- a soft reference — if the user deletes the note in Cairn, we keep the row
-- so a re-import is a no-op (treat as "this file was imported once"); to
-- force a fresh import the user removes the row via UI (M2).
CREATE TABLE IF NOT EXISTS imported_docs (
    id                TEXT PRIMARY KEY,
    source_path       TEXT NOT NULL UNIQUE,
    source_kind       TEXT NOT NULL,
    content_sha256    TEXT NOT NULL,
    note_id           TEXT NOT NULL,
    first_imported_at INTEGER NOT NULL,
    last_imported_at  INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_imported_docs_kind ON imported_docs(source_kind);
CREATE INDEX IF NOT EXISTS idx_imported_docs_note ON imported_docs(note_id);
