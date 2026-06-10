-- ============================================================
--  Phase 7 · #84 — Sticky flag on entities
--
--  is_sticky = 1 means the entity survives cascade deletion when
--  its source note is replaced by the import watcher. Auto-set on:
--    - manual creation (create_entity IPC)
--    - any post-create row in entity_edits (user edited it)
--    - being the keep side of a merge (alias points at it)
-- ============================================================

ALTER TABLE entities ADD COLUMN is_sticky INTEGER NOT NULL DEFAULT 0;
CREATE INDEX IF NOT EXISTS idx_entities_sticky ON entities(is_sticky);

-- Backfill: any entity that's already been the keep side of a
-- merge (one or more alias rows pointing at it) or has manual
-- edits beyond the initial 'create' counts as sticky retroactively.
-- Empty-keep_id rows are ignored (legacy `entity_id`-only rows
-- were already backfilled in the previous migration).
UPDATE entities SET is_sticky = 1
 WHERE id IN (SELECT DISTINCT keep_id FROM entity_aliases WHERE keep_id IS NOT NULL);

UPDATE entities SET is_sticky = 1
 WHERE id IN (
     SELECT entity_id FROM entity_edits
      GROUP BY entity_id
      HAVING SUM(CASE WHEN action <> 'create' THEN 1 ELSE 0 END) > 0
 );

-- Manually-authored entities (source_note_id IS NULL + a 'create'
-- edit row authored by the user) are sticky by definition.
UPDATE entities SET is_sticky = 1
 WHERE source_note_id IS NULL
   AND id IN (
       SELECT entity_id FROM entity_edits WHERE action = 'create' AND edited_by = 'user'
   );
