//! Hybrid retrieval over notes + entities.
//!
//! Phase 1: BM25 + importance + recency.
//! Phase 2d (current): adds **vector kNN** as a second ranking stream and
//! fuses it with BM25 using **Reciprocal Rank Fusion** (Cormack et al., 2009).
//!
//! Composite per-entity score:
//!     final = 0.55 * rrf(bm25, vector) + 0.25 * importance + 0.20 * recency
//!
//! `rrf` is in [0, 1] after min-max normalisation across the candidate set.
//! `importance` is the Ebbinghaus score from `importance.rs`.
//! `recency` is exp-decay with a 7-day half-life.
//!
//! Notes are still BM25-only because we don't embed raw note text in Phase 2.
//! (Adding note embeddings is a Phase 3 trade-off — double the storage for
//! marginal recall on lookups that the entity layer already covers.)

use crate::db::{now_ms, Db};
use crate::embed::EmbeddingProvider;
use crate::importance;
use crate::schema::{Entity, Note};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

const RECENCY_HALF_LIFE_MS: f64 = 7.0 * 24.0 * 60.0 * 60.0 * 1000.0;
const RRF_K: f64 = 60.0;
const WEIGHT_FUSED: f64 = 0.55;
const WEIGHT_IMPORTANCE: f64 = 0.25;
const WEIGHT_RECENCY: f64 = 0.20;

/// Trust boost for manually-edited entities (Phase 7 · #81).
///
/// `final_score *= 1 + min(manual_edits, MANUAL_EDIT_CAP) * MANUAL_EDIT_STEP`.
///
/// The cap stops a single much-edited entity from dominating; the step
/// is small enough that BM25/vector quality still leads. Default
/// 3 × 0.10 = up to 30% extra weight.
const MANUAL_EDIT_STEP: f64 = 0.10;
const MANUAL_EDIT_CAP: i64 = 3;

/// Cosine-distance ceiling for vector hits. Lower = stricter match.
/// 0.65 corresponds to cosine similarity ≥ 0.35 — keeps semantically
/// adjacent entities while filtering "random" noise from low-similarity
/// neighbours. Override at runtime via `CAIRN_VECTOR_DIST_MAX`.
const VECTOR_DIST_MAX_DEFAULT: f64 = 0.65;

fn vector_dist_max() -> f64 {
    std::env::var("CAIRN_VECTOR_DIST_MAX")
        .ok()
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(VECTOR_DIST_MAX_DEFAULT)
}

static VECTOR_DIST_MAX: once_cell::sync::Lazy<f64> = once_cell::sync::Lazy::new(vector_dist_max);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrievedEntity {
    pub entity: Entity,
    pub bm25_score: f64,
    /// Cosine similarity in [0, 1] (1 - cosine_distance). NaN if no vector hit.
    pub vector_score: f64,
    pub rrf_score: f64,
    pub importance_score: f64,
    pub recency_score: f64,
    /// Multiplicative boost from manual edits — `1.0` when the user
    /// hasn't edited the entity, up to `1 + MANUAL_EDIT_CAP * MANUAL_EDIT_STEP`
    /// (30% with current constants).
    #[serde(default = "default_trust_boost")]
    pub trust_boost: f64,
    /// How many `entity_edits` rows back the boost, capped at `MANUAL_EDIT_CAP`.
    #[serde(default)]
    pub manual_edits: i64,
    pub final_score: f64,
}

fn default_trust_boost() -> f64 {
    1.0
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrievedNote {
    pub note: Note,
    pub bm25_score: f64,
    pub recency_score: f64,
    pub final_score: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrievalResult {
    pub query: String,
    pub entities: Vec<RetrievedEntity>,
    pub notes: Vec<RetrievedNote>,
    /// Diagnostic counts so the UI can show "5 from BM25, 4 from vector, 3 overlapping".
    pub diagnostics: RetrievalDiagnostics,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RetrievalDiagnostics {
    pub bm25_hits: usize,
    pub vector_hits: usize,
    pub fused_hits: usize,
    pub vector_path_skipped_reason: Option<String>,
    /// Number of entities in the result set whose score was lifted by
    /// the manual-edit trust boost. UI surfaces this as "N edited
    /// entities boosted".
    #[serde(default)]
    pub trust_boost_applied: usize,
}

#[derive(Clone)]
pub struct Retriever {
    db: Db,
    /// Either a fixed embedder (set via `Retriever::with_embedder`, used by
    /// tests + the standalone cairn-mcp binary) or a live handle that
    /// resolves to the current user-configured embedder on every search.
    /// Live lets `/settings` provider changes apply without restart.
    embedder_src: EmbedderSrc,
}

#[derive(Clone)]
enum EmbedderSrc {
    Fixed(Arc<dyn EmbeddingProvider>),
    Live(Arc<crate::providers::LiveProviders>),
}

impl Retriever {
    pub fn new(db: Db, embedder: Arc<dyn EmbeddingProvider>) -> Self {
        Self {
            db,
            embedder_src: EmbedderSrc::Fixed(embedder),
        }
    }

    pub fn with_live_providers(db: Db, providers: Arc<crate::providers::LiveProviders>) -> Self {
        Self {
            db,
            embedder_src: EmbedderSrc::Live(providers),
        }
    }

    fn embedder(&self) -> Arc<dyn EmbeddingProvider> {
        match &self.embedder_src {
            EmbedderSrc::Fixed(e) => e.clone(),
            EmbedderSrc::Live(p) => p.embedder(),
        }
    }

    pub async fn search(
        &self,
        query: &str,
        types: Option<Vec<String>>,
        entity_limit: i64,
        note_limit: i64,
    ) -> Result<RetrievalResult> {
        let now = now_ms();
        let mut diagnostics = RetrievalDiagnostics::default();

        // ---- Stream 1: BM25 over entities ----
        // Trigram FTS5 doesn't index tokens shorter than 3 chars, so a
        // common CJK query like "家务" (2 chars) gets zero hits. Detect
        // short tokens and fall back to direct LIKE substring search.
        let needs_like_fallback = needs_like_fallback(query);
        let fts_query = build_fts_query(query);
        let bm25_entities = if needs_like_fallback {
            match self
                .db
                .search_entities_like(query, types.as_deref(), entity_limit)
                .await
            {
                Ok(v) => v,
                Err(e) => {
                    tracing::warn!(?e, "entity like search failed");
                    vec![]
                }
            }
        } else if !fts_query.is_empty() {
            match self
                .db
                .search_entities_bm25(&fts_query, types.as_deref(), entity_limit)
                .await
            {
                Ok(v) => v,
                Err(e) => {
                    tracing::warn!(?e, "entity bm25 search failed");
                    vec![]
                }
            }
        } else {
            vec![]
        };
        diagnostics.bm25_hits = bm25_entities.len();

        // ---- Stream 2: Vector kNN over entities ----
        let embedder = self.embedder();
        let vector_entities = if query.trim().is_empty() {
            vec![]
        } else {
            match embedder.embed_one(query).await {
                Ok(qv) => match self
                    .db
                    .knn_entities(&qv, entity_limit, types.as_deref())
                    .await
                {
                    Ok(v) => v,
                    Err(e) => {
                        tracing::warn!(?e, "vector kNN failed");
                        diagnostics.vector_path_skipped_reason = Some(format!("knn: {e}"));
                        vec![]
                    }
                },
                Err(e) => {
                    diagnostics.vector_path_skipped_reason = Some(format!("embed: {e}"));
                    vec![]
                }
            }
        };
        // Drop hits below the cosine-similarity floor so a vague query
        // doesn't drag random entities into the result list. sqlite-vec
        // returns cosine *distance* (1 - sim), so the threshold is the
        // distance below which we accept. VECTOR_DIST_MAX corresponds to
        // cosine_sim ≥ 0.35 — high recall, removes obvious noise.
        let vector_entities: Vec<(Entity, f64)> = vector_entities
            .into_iter()
            .filter(|(_, dist)| dist.is_finite() && *dist <= *VECTOR_DIST_MAX)
            .collect();
        diagnostics.vector_hits = vector_entities.len();

        // ---- Fuse via RRF ----
        let fused = rrf_fuse(&bm25_entities, &vector_entities);
        diagnostics.fused_hits = fused.len();
        let rrf_max = fused
            .iter()
            .map(|f| f.rrf)
            .fold(f64::NEG_INFINITY, f64::max);
        let rrf_min = fused.iter().map(|f| f.rrf).fold(f64::INFINITY, f64::min);

        let mut entities: Vec<RetrievedEntity> = fused
            .into_iter()
            .map(
                |FusedRow {
                     entity,
                     bm25,
                     vec_distance,
                     rrf,
                 }| {
                    let imp = importance::importance(
                        entity.base_importance,
                        entity.access_count,
                        entity.strength,
                        entity.last_accessed,
                        now,
                    );
                    let recency = recency_score(entity.updated_at, now);
                    let rrf_norm = if (rrf_max - rrf_min).abs() < f64::EPSILON {
                        1.0
                    } else {
                        (rrf - rrf_min) / (rrf_max - rrf_min)
                    };
                    let base_score = WEIGHT_FUSED * rrf_norm
                        + WEIGHT_IMPORTANCE * imp
                        + WEIGHT_RECENCY * recency;
                    let vector_score = if vec_distance.is_finite() {
                        1.0 - vec_distance
                    } else {
                        f64::NAN
                    };
                    RetrievedEntity {
                        entity,
                        bm25_score: bm25,
                        vector_score,
                        rrf_score: rrf_norm,
                        importance_score: imp,
                        recency_score: recency,
                        // Default trust state — the batch lookup below
                        // overwrites these for any entity with edits.
                        trust_boost: 1.0,
                        manual_edits: 0,
                        final_score: base_score,
                    }
                },
            )
            .collect();

        // Apply the manual-edit trust signal. One batch query covers
        // the whole candidate set so we don't fan out N COUNT(*) calls.
        if !entities.is_empty() {
            let ids: Vec<String> = entities.iter().map(|e| e.entity.id.clone()).collect();
            match self.db.manual_edit_counts(&ids).await {
                Ok(counts) => {
                    for ent in entities.iter_mut() {
                        if let Some(&n) = counts.get(&ent.entity.id) {
                            let capped = n.min(MANUAL_EDIT_CAP);
                            let boost = 1.0 + (capped as f64) * MANUAL_EDIT_STEP;
                            ent.manual_edits = capped;
                            ent.trust_boost = boost;
                            ent.final_score *= boost;
                            diagnostics.trust_boost_applied += 1;
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!(?e, "manual_edit_counts batch failed; skipping trust boost");
                }
            }
        }

        entities.sort_by(|a, b| {
            b.final_score
                .partial_cmp(&a.final_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        entities.truncate(entity_limit as usize);

        // ---- Notes (BM25 / LIKE) ----
        let notes_raw = if needs_like_fallback {
            self.db
                .search_notes_like(query, note_limit)
                .await
                .unwrap_or_default()
        } else if !fts_query.is_empty() {
            self.db
                .search_notes_bm25(&fts_query, note_limit)
                .await
                .unwrap_or_default()
        } else {
            vec![]
        };

        let mut notes: Vec<RetrievedNote> = notes_raw
            .into_iter()
            .map(|note| {
                let recency = recency_score(note.created_at, now);
                let final_score = 0.6 * 1.0 + 0.4 * recency; // flat BM25 prior for now
                RetrievedNote {
                    note,
                    bm25_score: 0.0,
                    recency_score: recency,
                    final_score,
                }
            })
            .collect();
        notes.sort_by(|a, b| {
            b.final_score
                .partial_cmp(&a.final_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        notes.truncate(note_limit as usize);

        Ok(RetrievalResult {
            query: query.to_string(),
            entities,
            notes,
            diagnostics,
        })
    }
}

struct FusedRow {
    entity: Entity,
    bm25: f64,
    vec_distance: f64,
    rrf: f64,
}

/// Reciprocal Rank Fusion of two entity rankings keyed by entity id.
fn rrf_fuse(bm25: &[(Entity, f64)], vectors: &[(Entity, f64)]) -> Vec<FusedRow> {
    let mut by_id: HashMap<String, FusedRow> = HashMap::new();

    for (rank, (entity, bm25_score)) in bm25.iter().enumerate() {
        let contrib = 1.0 / (RRF_K + (rank + 1) as f64);
        by_id
            .entry(entity.id.clone())
            .and_modify(|f| {
                f.rrf += contrib;
                f.bm25 = *bm25_score;
            })
            .or_insert(FusedRow {
                entity: entity.clone(),
                bm25: *bm25_score,
                vec_distance: f64::INFINITY,
                rrf: contrib,
            });
    }
    for (rank, (entity, distance)) in vectors.iter().enumerate() {
        let contrib = 1.0 / (RRF_K + (rank + 1) as f64);
        by_id
            .entry(entity.id.clone())
            .and_modify(|f| {
                f.rrf += contrib;
                f.vec_distance = *distance;
            })
            .or_insert(FusedRow {
                entity: entity.clone(),
                bm25: f64::INFINITY,
                vec_distance: *distance,
                rrf: contrib,
            });
    }
    by_id.into_values().collect()
}

/// FTS5 query: whitespace-split tokens, OR'd. Empty input → empty query.
/// Returns `true` when the query contains any whitespace-separated token
/// shorter than 3 characters. The trigram FTS5 tokenizer doesn't index
/// or match those (typical CJK case: "家务"), so we route them through the
/// LIKE-substring fallback instead.
fn needs_like_fallback(q: &str) -> bool {
    let trimmed = q.trim();
    if trimmed.is_empty() {
        return false;
    }
    trimmed.split_whitespace().any(|t| t.chars().count() < 3)
}

fn build_fts_query(q: &str) -> String {
    let tokens: Vec<String> = q
        .split_whitespace()
        .filter(|t| !t.is_empty())
        .map(|t| {
            let escaped = t.replace('"', "\"\"");
            format!("\"{}\"", escaped)
        })
        .collect();
    if tokens.is_empty() {
        return String::new();
    }
    tokens.join(" OR ")
}

fn recency_score(ts_ms: i64, now_ms: i64) -> f64 {
    let elapsed = (now_ms - ts_ms).max(0) as f64;
    (-elapsed * std::f64::consts::LN_2 / RECENCY_HALF_LIFE_MS).exp()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fts_query_or_join() {
        assert_eq!(build_fts_query("妈 膝盖"), "\"妈\" OR \"膝盖\"");
    }

    #[test]
    fn fts_query_empty() {
        assert_eq!(build_fts_query(""), "");
    }

    #[test]
    fn recency_now_is_one() {
        let now = 1_000_000_000_000;
        assert!((recency_score(now, now) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn recency_half_life() {
        let now = 1_000_000_000_000;
        let week_ago = now - 7 * 24 * 60 * 60 * 1000;
        let r = recency_score(week_ago, now);
        assert!((r - 0.5).abs() < 0.01);
    }

    fn make_entity(id: &str, name: &str) -> Entity {
        Entity {
            id: id.to_string(),
            r#type: "Preference".to_string(),
            name: name.to_string(),
            properties: "{}".to_string(),
            confidence: 1.0,
            source_note_id: None,
            created_at: 0,
            updated_at: 0,
            strength: 1.0,
            last_accessed: 0,
            access_count: 1,
            base_importance: 0.5,
            is_sticky: 0,
            archived_at: None,
        }
    }

    #[test]
    fn rrf_promotes_double_hits() {
        let e1 = make_entity("a", "alpha");
        let e2 = make_entity("b", "beta");
        let e3 = make_entity("c", "gamma");

        // BM25: a, b
        let bm25 = vec![(e1.clone(), -10.0), (e2.clone(), -8.0)];
        // Vector: c, a
        let vectors = vec![(e3.clone(), 0.10), (e1.clone(), 0.20)];

        let fused = rrf_fuse(&bm25, &vectors);
        let mut by_id: HashMap<String, f64> =
            fused.iter().map(|f| (f.entity.id.clone(), f.rrf)).collect();
        // a is in both → highest RRF
        assert!(by_id["a"] > by_id["b"]);
        assert!(by_id["a"] > by_id["c"]);
        // count of fused rows == unique entities
        assert_eq!(by_id.len(), 3);
        let _ = by_id.remove("a");
    }
}
