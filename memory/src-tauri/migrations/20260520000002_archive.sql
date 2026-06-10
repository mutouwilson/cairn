-- ============================================================
--  Soft archive for entities.
--
--  `archived_at` (unix ms) is NULL for active entities and set to the
--  timestamp when the user archived them (or when the auto-archive
--  consolidation tick scooped them up). Retrieval filters
--  `archived_at IS NULL` so archived entities don't pollute search;
--  they can be restored via `set_entity_archived(id, false)`.
-- ============================================================

ALTER TABLE entities ADD COLUMN archived_at INTEGER;
CREATE INDEX IF NOT EXISTS idx_entities_archived_at ON entities(archived_at);
