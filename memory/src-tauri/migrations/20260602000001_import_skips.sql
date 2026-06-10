-- Persistent per-file "do not sync" list for the import scanner.
--
-- A discovered file whose `source_path` is in this table is shown as
-- SKIPPED and is never imported — neither by a manual Apply nor by the
-- background watcher — until the user unskips it (clears the row).
--
-- Decoupled from `imported_docs` on purpose: a file can be skipped before
-- it has ever been imported (a brand-new NEW file the user doesn't want),
-- so the skip can't hang off an imported_docs row.
CREATE TABLE IF NOT EXISTS import_skips (
    source_path TEXT PRIMARY KEY,
    skipped_at  INTEGER NOT NULL
);
