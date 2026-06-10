-- ============================================================
--  Phase 2d · Vector retrieval
-- ============================================================
--
-- sqlite-vec extension is registered globally via `sqlite3_auto_extension`
-- before the pool is opened (see `lib.rs::register_sqlite_vec`). The vec0
-- virtual table provides indexed kNN search over fixed-dim float vectors.
--
-- Dimension is locked at 1024 to match the default
-- `openai/text-embedding-3-small` (truncated) and most modern open-weight
-- embedders. Changing the dim is a breaking migration; users must drop and
-- recreate this table and re-embed every entity. See docs/ARCHITECTURE.md.

CREATE VIRTUAL TABLE entity_vecs USING vec0(
    entity_id TEXT PRIMARY KEY,
    embedding FLOAT[1024] distance_metric=cosine
);

-- Track which model produced each embedding so we can detect drift
-- and trigger a re-embed when the user changes provider/model.
CREATE TABLE entity_embedding_meta (
    entity_id      TEXT PRIMARY KEY,
    model          TEXT NOT NULL,
    dim            INTEGER NOT NULL,
    embedded_at    INTEGER NOT NULL,
    text_hash      TEXT NOT NULL,            -- SHA-256(name + properties), so we re-embed on real change
    FOREIGN KEY (entity_id) REFERENCES entities(id) ON DELETE CASCADE
);
CREATE INDEX idx_entity_embed_model ON entity_embedding_meta(model);
