-- Phase 6d follow-up: switch notes_fts and entities_fts to the `trigram`
-- tokenizer so Chinese / Japanese / Korean queries actually match indexed
-- content. `unicode61` only splits on space + punctuation, which leaves
-- CJK paragraphs as single tokens — searching "家务" never matched the
-- note "今天做了家务".
--
-- The trigram tokenizer indexes every overlapping 3-character window of
-- the source string. It works for Latin too ("research" matches a query
-- for "esea") so it strictly dominates unicode61 for a search-everywhere
-- product like Cairn. Trade-off: ~3× index size and slightly fuzzier
-- ranking — acceptable for personal-scale corpora.
--
-- This migration:
--   1) drops the old FTS tables (and their auto-maintenance triggers).
--   2) recreates them with `tokenize='trigram'`.
--   3) re-populates from `notes` / `entities` so existing rows stay
--      searchable.
--   4) reinstalls the after-INSERT / after-UPDATE / after-DELETE triggers
--      that keep the FTS in sync.

-- --- notes -------------------------------------------------------------
DROP TRIGGER IF EXISTS notes_fts_ai;
DROP TRIGGER IF EXISTS notes_fts_ad;
DROP TRIGGER IF EXISTS notes_fts_au;
DROP TABLE IF EXISTS notes_fts;

CREATE VIRTUAL TABLE notes_fts USING fts5(
    text,
    content='notes',
    content_rowid='rowid',
    tokenize='trigram'
);

INSERT INTO notes_fts(rowid, text) SELECT rowid, text FROM notes;

CREATE TRIGGER notes_fts_ai AFTER INSERT ON notes BEGIN
    INSERT INTO notes_fts(rowid, text) VALUES (new.rowid, new.text);
END;
CREATE TRIGGER notes_fts_ad AFTER DELETE ON notes BEGIN
    INSERT INTO notes_fts(notes_fts, rowid, text) VALUES ('delete', old.rowid, old.text);
END;
CREATE TRIGGER notes_fts_au AFTER UPDATE ON notes BEGIN
    INSERT INTO notes_fts(notes_fts, rowid, text) VALUES ('delete', old.rowid, old.text);
    INSERT INTO notes_fts(rowid, text) VALUES (new.rowid, new.text);
END;

-- --- entities ----------------------------------------------------------
DROP TRIGGER IF EXISTS entities_fts_ai;
DROP TRIGGER IF EXISTS entities_fts_ad;
DROP TRIGGER IF EXISTS entities_fts_au;
DROP TABLE IF EXISTS entities_fts;

CREATE VIRTUAL TABLE entities_fts USING fts5(
    name,
    properties,
    content='entities',
    content_rowid='rowid',
    tokenize='trigram'
);

INSERT INTO entities_fts(rowid, name, properties)
SELECT rowid, name, properties FROM entities;

CREATE TRIGGER entities_fts_ai AFTER INSERT ON entities BEGIN
    INSERT INTO entities_fts(rowid, name, properties) VALUES (new.rowid, new.name, new.properties);
END;
CREATE TRIGGER entities_fts_ad AFTER DELETE ON entities BEGIN
    INSERT INTO entities_fts(entities_fts, rowid, name, properties)
        VALUES ('delete', old.rowid, old.name, old.properties);
END;
CREATE TRIGGER entities_fts_au AFTER UPDATE ON entities BEGIN
    INSERT INTO entities_fts(entities_fts, rowid, name, properties)
        VALUES ('delete', old.rowid, old.name, old.properties);
    INSERT INTO entities_fts(rowid, name, properties) VALUES (new.rowid, new.name, new.properties);
END;
