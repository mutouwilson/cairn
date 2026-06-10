-- ============================================================
--  Editable memory — track every manual edit to an entity.
--
--  Each user-driven create/update/delete on an entity appends one row
--  here (per field, for updates). The retrieval layer reads
--  `manual_edit_count(entity_id)` and applies a multiplicative trust
--  boost: entities the user has explicitly cared enough about to edit
--  rank higher.
--
--  `field_changed`/`value_before`/`value_after` are NULL for create
--  (nothing to diff against) and for delete (the row's already gone).
-- ============================================================

CREATE TABLE IF NOT EXISTS entity_edits (
    id              TEXT PRIMARY KEY,
    entity_id       TEXT NOT NULL,
    action          TEXT NOT NULL CHECK(action IN ('create','update','delete')),
    field_changed   TEXT,         -- "name" | "type" | "properties" | NULL for create/delete
    value_before    TEXT,         -- truncated to 1024 chars at the application layer
    value_after     TEXT,
    edited_by       TEXT NOT NULL DEFAULT 'user',
    edited_at       INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_entity_edits_entity ON entity_edits(entity_id);
CREATE INDEX IF NOT EXISTS idx_entity_edits_time   ON entity_edits(edited_at);
