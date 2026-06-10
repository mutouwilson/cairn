-- ============================================================
--  Cairn Memory · Initial schema
--  Personal Memory / Context OS
-- ============================================================

PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;

-- ---------------- Notes ----------------
CREATE TABLE notes (
    id                 TEXT PRIMARY KEY,             -- ULID
    text               TEXT NOT NULL,
    source             TEXT NOT NULL,                -- manual | hotkey | email | voice | api
    created_at         INTEGER NOT NULL,             -- unix ms
    processed_at       INTEGER,                      -- when extraction finished
    processing_status  TEXT NOT NULL DEFAULT 'pending', -- pending | done | failed
    extraction_error   TEXT
);
CREATE INDEX idx_notes_created ON notes(created_at DESC);
CREATE INDEX idx_notes_status  ON notes(processing_status);

-- FTS5 BM25 index over notes
CREATE VIRTUAL TABLE notes_fts USING fts5(
    text,
    content='notes',
    content_rowid='rowid',
    tokenize='unicode61 remove_diacritics 2'
);

CREATE TRIGGER notes_fts_ai AFTER INSERT ON notes BEGIN
    INSERT INTO notes_fts(rowid, text) VALUES (new.rowid, new.text);
END;
CREATE TRIGGER notes_fts_ad AFTER DELETE ON notes BEGIN
    INSERT INTO notes_fts(notes_fts, rowid, text) VALUES('delete', old.rowid, old.text);
END;
CREATE TRIGGER notes_fts_au AFTER UPDATE ON notes BEGIN
    INSERT INTO notes_fts(notes_fts, rowid, text) VALUES('delete', old.rowid, old.text);
    INSERT INTO notes_fts(rowid, text) VALUES (new.rowid, new.text);
END;

-- ---------------- Entities ----------------
-- 8 types: Person | Event | Preference | Belief | Goal | Asset | Skill | Location
CREATE TABLE entities (
    id               TEXT PRIMARY KEY,
    type             TEXT NOT NULL,
    name             TEXT NOT NULL,
    properties       TEXT NOT NULL DEFAULT '{}',     -- JSON
    confidence       REAL NOT NULL,                  -- LLM confidence 0..1
    source_note_id   TEXT,
    created_at       INTEGER NOT NULL,
    updated_at       INTEGER NOT NULL,
    -- Ebbinghaus importance state
    strength         REAL    NOT NULL DEFAULT 1.0,   -- S in R = exp(-Δt/S)
    last_accessed    INTEGER NOT NULL,
    access_count     INTEGER NOT NULL DEFAULT 1,
    base_importance  REAL    NOT NULL DEFAULT 0.5,
    FOREIGN KEY (source_note_id) REFERENCES notes(id) ON DELETE SET NULL
);
CREATE INDEX idx_entities_type     ON entities(type);
CREATE INDEX idx_entities_name     ON entities(name COLLATE NOCASE);
CREATE INDEX idx_entities_updated  ON entities(updated_at DESC);
CREATE INDEX idx_entities_strength ON entities(strength);

CREATE VIRTUAL TABLE entities_fts USING fts5(
    name, properties,
    content='entities',
    content_rowid='rowid',
    tokenize='unicode61 remove_diacritics 2'
);
CREATE TRIGGER entities_fts_ai AFTER INSERT ON entities BEGIN
    INSERT INTO entities_fts(rowid, name, properties) VALUES (new.rowid, new.name, new.properties);
END;
CREATE TRIGGER entities_fts_ad AFTER DELETE ON entities BEGIN
    INSERT INTO entities_fts(entities_fts, rowid, name, properties)
    VALUES('delete', old.rowid, old.name, old.properties);
END;
CREATE TRIGGER entities_fts_au AFTER UPDATE ON entities BEGIN
    INSERT INTO entities_fts(entities_fts, rowid, name, properties)
    VALUES('delete', old.rowid, old.name, old.properties);
    INSERT INTO entities_fts(rowid, name, properties) VALUES (new.rowid, new.name, new.properties);
END;

-- ---------------- Aliases ----------------
CREATE TABLE entity_aliases (
    id          TEXT PRIMARY KEY,
    entity_id   TEXT NOT NULL,
    alias       TEXT NOT NULL,
    source      TEXT NOT NULL,                       -- auto | user
    created_at  INTEGER NOT NULL,
    FOREIGN KEY (entity_id) REFERENCES entities(id) ON DELETE CASCADE
);
CREATE UNIQUE INDEX idx_aliases_alias ON entity_aliases(alias COLLATE NOCASE);

-- ---------------- Relations ----------------
CREATE TABLE relations (
    id               TEXT PRIMARY KEY,
    from_entity      TEXT NOT NULL,
    to_entity        TEXT NOT NULL,
    type             TEXT NOT NULL,
    properties       TEXT,
    confidence       REAL NOT NULL DEFAULT 1.0,
    source_note_id   TEXT,
    created_at       INTEGER NOT NULL,
    FOREIGN KEY (from_entity)    REFERENCES entities(id) ON DELETE CASCADE,
    FOREIGN KEY (to_entity)      REFERENCES entities(id) ON DELETE CASCADE,
    FOREIGN KEY (source_note_id) REFERENCES notes(id)    ON DELETE SET NULL
);
CREATE INDEX idx_relations_from ON relations(from_entity);
CREATE INDEX idx_relations_to   ON relations(to_entity);
CREATE INDEX idx_relations_type ON relations(type);

-- ---------------- Agents (MCP clients) ----------------
CREATE TABLE agents (
    id            TEXT PRIMARY KEY,                  -- e.g. 'claude-desktop'
    display_name  TEXT NOT NULL,
    public_key    TEXT,                              -- base64 Ed25519 (Phase 2)
    added_at      INTEGER NOT NULL,
    last_seen     INTEGER,
    revoked_at    INTEGER
);

-- ---------------- Permissions matrix ----------------
CREATE TABLE permissions (
    agent_id      TEXT NOT NULL,
    entity_type   TEXT NOT NULL,                     -- '*' = all types
    scope         TEXT NOT NULL,                     -- read | write | none
    granted_at    INTEGER NOT NULL,
    expires_at    INTEGER,
    PRIMARY KEY (agent_id, entity_type),
    FOREIGN KEY (agent_id) REFERENCES agents(id) ON DELETE CASCADE
);

-- ---------------- Audit Log (Merkle chain + Ed25519) ----------------
CREATE TABLE audit_log (
    seq           INTEGER PRIMARY KEY AUTOINCREMENT,
    id            TEXT    NOT NULL UNIQUE,
    agent_id      TEXT    NOT NULL,
    action        TEXT    NOT NULL,                  -- initialize|tools/list|tools/call|denied
    tool_name     TEXT,
    arguments     TEXT,                              -- JSON, truncated to 4kb
    result_hash   TEXT    NOT NULL,                  -- SHA-256(result_json)
    result_count  INTEGER,
    timestamp     INTEGER NOT NULL,
    prev_hash     TEXT    NOT NULL,                  -- hash of prev row
    hash          TEXT    NOT NULL,                  -- SHA-256 of canonical fields
    signature     TEXT    NOT NULL                   -- Ed25519(hash), base64
);
CREATE INDEX idx_audit_agent ON audit_log(agent_id);
CREATE INDEX idx_audit_ts    ON audit_log(timestamp DESC);

-- ---------------- Audit signing key (singleton) ----------------
CREATE TABLE audit_key (
    id           INTEGER PRIMARY KEY CHECK (id = 1),
    public_key   TEXT    NOT NULL,                   -- base64
    private_key  TEXT    NOT NULL,                   -- PKCS8 base64 (Phase 2: OS keychain)
    created_at   INTEGER NOT NULL
);

-- ---------------- Seed: default Agent permissions for "claude-desktop" ----------------
INSERT INTO agents (id, display_name, added_at)
VALUES ('claude-desktop', 'Claude Desktop', strftime('%s', 'now') * 1000);

-- Default to read-only for Preferences (lowest sensitivity)
INSERT INTO permissions (agent_id, entity_type, scope, granted_at)
VALUES ('claude-desktop', 'Preference', 'read', strftime('%s', 'now') * 1000);
