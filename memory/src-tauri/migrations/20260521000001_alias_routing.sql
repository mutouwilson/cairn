-- ============================================================
--  Phase 7 · #84 — Merge persistence (alias routing)
--
--  Extends the dormant `entity_aliases` table so it can route
--  re-extracted entities back onto manually-merged "keep" rows.
--  The existing schema is:
--
--    entity_aliases(id, entity_id, alias, source, created_at)
--    UNIQUE(alias COLLATE NOCASE)
--
--  semantically: entity_id IS the keep_id (FK to entities with
--  ON DELETE CASCADE — exactly what we want for #6 in the spec).
--  We add the new metadata columns alongside.
--
--  The legacy UNIQUE(alias) index has to go: under the new design
--  multiple drops can share a `name_lower` (e.g. two different
--  "Mom" entities merged into different keeps over time), so the
--  uniqueness assumption no longer holds. The table previously
--  had no inserts anywhere in the codebase so nothing relies on
--  the index being present.
-- ============================================================

DROP INDEX IF EXISTS idx_aliases_alias;

ALTER TABLE entity_aliases ADD COLUMN keep_id TEXT;
ALTER TABLE entity_aliases ADD COLUMN name_lower TEXT;
ALTER TABLE entity_aliases ADD COLUMN embedding_hash TEXT;
ALTER TABLE entity_aliases ADD COLUMN embedding_model_id TEXT;
ALTER TABLE entity_aliases ADD COLUMN routed_count INTEGER NOT NULL DEFAULT 0;

-- Backfill `keep_id` for any historical rows so the new SELECTs
-- that filter on `keep_id IS NOT NULL` don't need to special-case.
UPDATE entity_aliases SET keep_id = entity_id WHERE keep_id IS NULL;

CREATE INDEX IF NOT EXISTS idx_aliases_keep ON entity_aliases(keep_id);
CREATE INDEX IF NOT EXISTS idx_aliases_name ON entity_aliases(name_lower);
