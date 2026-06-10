-- ============================================================
--  Phase 3d · Retrieval signals for personal-embedding fine-tuning
-- ============================================================
--
-- Every time a search query produces results the user interacts with, we log:
--   - the query text (so we can re-embed offline)
--   - the ids of returned entities
--   - which ones the user clicked / dismissed / corrected
--
-- A nightly Python job in `tools/distill/embed_finetune.py` turns this into
-- contrastive pairs (query → positive entity, query → negative entity) and
-- trains a small projection adapter on top of the base embedding. The adapter
-- is then loaded at runtime via the EmbeddingProvider stack.
--
-- Schema: one row per *search query event*; per-entity actions live as JSON
-- arrays so a single query can grow new positive/negative signals over time
-- without exploding the row count.

CREATE TABLE retrieval_signals (
    id               TEXT PRIMARY KEY,            -- ULID
    query_text       TEXT NOT NULL,
    types_filter     TEXT,                        -- JSON array, may be null
    returned_ids     TEXT NOT NULL DEFAULT '[]',  -- JSON array of entity ids
    clicked_ids      TEXT NOT NULL DEFAULT '[]',  -- positives
    dismissed_ids    TEXT NOT NULL DEFAULT '[]',  -- explicit negatives
    corrected_ids    TEXT NOT NULL DEFAULT '[]',  -- entity user re-typed or merged (very strong positive)
    embedding_model  TEXT,                        -- whichever model produced the query embedding
    created_at       INTEGER NOT NULL,
    last_updated_at  INTEGER NOT NULL
);
CREATE INDEX idx_retrieval_signals_created ON retrieval_signals(created_at DESC);
