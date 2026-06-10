-- ============================================================
--  Dedup rejections — "the user said these two are NOT duplicates".
--
--  When the dedup-candidate UI surfaces a pair and the user explicitly
--  rejects the merge, persist that decision so the same pair doesn't
--  re-appear on the next scan. The pair direction does matter: the
--  algorithm picks a canonical (keep_id, drop_id) order based on
--  `final_score`, so storing both orderings is unnecessary as long as
--  the candidate query checks the canonical pairing.
-- ============================================================

CREATE TABLE IF NOT EXISTS dedup_rejections (
    id              TEXT PRIMARY KEY,
    keep_id         TEXT NOT NULL,
    drop_id         TEXT NOT NULL,
    rejected_at     INTEGER NOT NULL,
    UNIQUE(keep_id, drop_id)
);
CREATE INDEX IF NOT EXISTS idx_dedup_rejections_pair ON dedup_rejections(keep_id, drop_id);
