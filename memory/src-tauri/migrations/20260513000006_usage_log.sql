-- ============================================================
--  Phase 4c · Cost / latency telemetry
-- ============================================================
--
-- One row per outbound LLM/embedding call. Cost is stored in micro-USD
-- (integer micro-dollars) so we never need to wrangle floats for sums.
--
-- The application code populates `cost_micro_usd` whenever the upstream
-- response carries a `usage.cost` field (Vercel AI Gateway does; Anthropic
-- direct does not — we leave NULL and compute later via a price table if
-- ever needed).

CREATE TABLE usage_log (
    id                TEXT PRIMARY KEY,        -- ULID
    operation         TEXT NOT NULL,           -- extract | embed | consolidate
    provider          TEXT NOT NULL,           -- gateway | anthropic | local
    model             TEXT NOT NULL,
    prompt_tokens     INTEGER,
    completion_tokens INTEGER,
    cost_micro_usd    INTEGER,                 -- nullable; gateway reports this directly
    latency_ms        INTEGER NOT NULL,
    success           INTEGER NOT NULL,        -- 0/1
    error             TEXT,
    occurred_at       INTEGER NOT NULL
);
CREATE INDEX idx_usage_log_time ON usage_log(occurred_at DESC);
CREATE INDEX idx_usage_log_op ON usage_log(operation, occurred_at DESC);
