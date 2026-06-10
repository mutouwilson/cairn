-- Phase 7 · V6 M2 — track the Cairn-managed block we've written into each
-- destination file (currently only `~/.claude/CLAUDE.md`).
--
-- We record the sha256 of the block CONTENT (the markdown between the
-- `<!-- cairn:start -->` / `<!-- cairn:end -->` markers) the last time we
-- wrote it. On the next export we compute the sha of whatever's currently
-- between the markers; a mismatch means the user edited inside the block
-- and we refuse to overwrite without `force = true`.
--
-- `last_block_content` is kept for the conflict-resolution UI: when the
-- user edits inside the block, the UI can show both versions side-by-side.
CREATE TABLE IF NOT EXISTS exported_blocks (
    id                  TEXT PRIMARY KEY,
    dest_path           TEXT NOT NULL UNIQUE,
    last_written_sha256 TEXT NOT NULL,
    last_block_content  TEXT NOT NULL,
    entity_ids_json     TEXT NOT NULL,
    last_written_at     INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_exported_blocks_path ON exported_blocks(dest_path);
