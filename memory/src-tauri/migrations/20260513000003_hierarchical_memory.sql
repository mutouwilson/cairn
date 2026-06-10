-- ============================================================
--  Phase 2c · Hierarchical memory + sleep consolidation
-- ============================================================
--
-- Episodic memory (existing `entities` table) is what the LLM extracts from
-- raw notes — concrete, time-anchored, often noisy. Over time we want to
-- consolidate clusters of related episodic memories into stable themes —
-- semantic memories — that an agent can ask about directly without scrolling
-- raw events.
--
-- The background `consolidation::Worker` runs on a cadence
-- (`CAIRN_CONSOLIDATION_INTERVAL_SECS`, default 900 s = 15 min). Each run:
--   1. Picks "topics" with ≥3 supporting episodic entities since the previous
--      consolidation. Topics are typed strings, e.g. `person:妈`,
--      `domain:coffee`, `goal:learn-French`.
--   2. Calls an LLM (`CAIRN_CONSOLIDATION_MODEL`) with the topic's
--      episodic context to produce a short structured summary.
--   3. Upserts one row per topic into `semantic_memories`. Older rows for the
--      same topic are kept and linked via `superseded_by` so history is
--      auditable.

CREATE TABLE semantic_memories (
    id                    TEXT PRIMARY KEY,         -- ULID
    topic                 TEXT NOT NULL,            -- canonical key, e.g. 'person:妈'
    title                 TEXT NOT NULL,            -- short UI label
    summary               TEXT NOT NULL,            -- 2–6 sentence LLM summary
    evidence              TEXT NOT NULL,            -- JSON array of entity_ids
    confidence            REAL NOT NULL,
    created_at            INTEGER NOT NULL,
    last_consolidated_at  INTEGER NOT NULL,
    superseded_by         TEXT,                     -- if a newer row replaced this
    strength              REAL NOT NULL DEFAULT 1.0,
    last_accessed         INTEGER NOT NULL,
    access_count          INTEGER NOT NULL DEFAULT 0,
    FOREIGN KEY (superseded_by) REFERENCES semantic_memories(id) ON DELETE SET NULL
);

-- "Active" semantic memories are those not superseded. Uniqueness is enforced
-- via a partial index so historical rows can keep the same topic key.
CREATE UNIQUE INDEX idx_semantic_topic_active
    ON semantic_memories(topic) WHERE superseded_by IS NULL;
CREATE INDEX idx_semantic_consolidated
    ON semantic_memories(last_consolidated_at DESC);

-- Audit trail for the consolidation worker itself. Surfaced in the UI so users
-- can see why summaries are (or aren't) appearing.
CREATE TABLE consolidation_runs (
    id                TEXT PRIMARY KEY,             -- ULID
    started_at        INTEGER NOT NULL,
    finished_at       INTEGER,
    trigger           TEXT NOT NULL,                -- scheduled | manual | startup
    topics_scanned    INTEGER NOT NULL DEFAULT 0,
    semantic_created  INTEGER NOT NULL DEFAULT 0,
    semantic_updated  INTEGER NOT NULL DEFAULT 0,
    error             TEXT,
    status            TEXT NOT NULL                 -- running | done | failed
);
CREATE INDEX idx_consolidation_started ON consolidation_runs(started_at DESC);
