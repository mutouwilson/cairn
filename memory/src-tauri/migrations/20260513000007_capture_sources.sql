-- ============================================================
--  Phase 5 · Capture surfaces (passive sources)
-- ============================================================
--
-- Generic table for background-pollable capture sources. Each row drives one
-- tokio worker task. Today we ship two kinds:
--   - imap : poll an IMAP mailbox for a labelled/folder of messages.
--   - ics  : poll a public/secret iCalendar feed URL for events.
--
-- Secrets (IMAP passwords) live in the OS keychain (service `so.cairn.app`,
-- account = the `secret_account` field on the row). The DB only holds
-- non-sensitive metadata + last-status, so it can be safely included in
-- exports / bundles in a future iteration.

CREATE TABLE capture_sources (
    id                   TEXT PRIMARY KEY,            -- ULID
    kind                 TEXT NOT NULL,               -- imap | ics
    label                TEXT NOT NULL,               -- user-visible
    config               TEXT NOT NULL,               -- JSON; shape depends on kind
    secret_account       TEXT,                        -- keychain account name; nullable for unauthenticated sources
    enabled              INTEGER NOT NULL DEFAULT 1,
    poll_interval_secs   INTEGER NOT NULL DEFAULT 300,
    last_polled_at       INTEGER,
    last_status          TEXT,                        -- ok | error | running
    last_error           TEXT,
    captured_count       INTEGER NOT NULL DEFAULT 0,  -- cumulative items ingested
    created_at           INTEGER NOT NULL,
    updated_at           INTEGER NOT NULL
);
CREATE INDEX idx_capture_sources_kind ON capture_sources(kind);
CREATE INDEX idx_capture_sources_enabled ON capture_sources(enabled);

-- Per-source dedupe state: a content hash of every ingested item so we never
-- re-import the same email / event twice across poll cycles.
CREATE TABLE capture_seen (
    source_id   TEXT NOT NULL,
    item_uid    TEXT NOT NULL,           -- IMAP UID, ICS UID, etc.
    seen_at     INTEGER NOT NULL,
    PRIMARY KEY (source_id, item_uid),
    FOREIGN KEY (source_id) REFERENCES capture_sources(id) ON DELETE CASCADE
);
CREATE INDEX idx_capture_seen_time ON capture_seen(seen_at DESC);
