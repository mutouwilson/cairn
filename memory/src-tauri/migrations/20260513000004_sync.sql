-- ============================================================
--  Phase 3c · Bundle-based sync
-- ============================================================
--
-- Today we ship a signed JSON bundle format that captures notes / entities /
-- relations / themes and can be exported, hand-carried (file copy, AirDrop,
-- whatever), and imported on another device. Per-row LWW conflict resolution
-- keyed by ULID + updated_at.
--
-- Phase 4 adds:
--   - a real-time CRDT layer (yrs) for character-level note co-editing;
--   - a peer-to-peer transport (Iroh or libp2p) so bundles are exchanged
--     without manual file shuffle.
--
-- The `sync_log` table is the audit surface for both flows.

CREATE TABLE sync_log (
    id              TEXT PRIMARY KEY,             -- ULID
    direction       TEXT NOT NULL,                -- export | import
    bundle_id       TEXT NOT NULL,                -- ULID embedded in the bundle
    device_name     TEXT,                         -- optional human label
    device_pubkey   TEXT,                         -- base64 Ed25519 public key
    counts          TEXT NOT NULL,                -- JSON {notes, entities, relations, themes}
    checksum        TEXT NOT NULL,                -- SHA-256 of unsigned bundle body
    signature_ok    INTEGER,                      -- 0/1 for imports; null for exports
    conflicts       INTEGER NOT NULL DEFAULT 0,   -- rows where remote was older than local
    errors          TEXT,                         -- nullable JSON array of messages
    occurred_at     INTEGER NOT NULL
);
CREATE INDEX idx_sync_log_time ON sync_log(occurred_at DESC);
