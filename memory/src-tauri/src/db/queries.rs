//! Typed SQL queries. Anything that touches the DB goes through here so the SQL
//! is in one place and reviewable.

use crate::importance;
use crate::schema::{
    Agent, AuditEntry, Entity, EntityType, ExtractionResult, Note, Permission, Relation,
};
use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use sqlx::Row;
use ulid::Ulid;

use super::{now_ms, Db};

fn is_transient_lock_error(e: &anyhow::Error) -> bool {
    let s = e.to_string().to_lowercase();
    s.contains("database is locked")
        || s.contains("(code: 5)")
        || s.contains("(code: 517)")
        || s.contains("busy")
}

/// Outcome of the in-transaction cleanup. The `sticky_kept` list is the
/// set of entities that *would* have been orphaned by the note removal
/// but are pinned via `is_sticky = 1` — the caller surfaces these to the
/// audit log so the user can see what survived the import-watcher churn.
#[derive(Debug, Default, Clone)]
pub struct ExtractionCleanupReport {
    pub entities_deleted: u64,
    pub relations_deleted: u64,
    pub sticky_kept: Vec<String>,
}

/// Remove the entities orphaned by deleting / re-processing this note,
/// plus the relations sourced from it. Caller owns the transaction so the
/// same cascade is shared by both `delete_note` and `cleanup_extraction_for_note`.
///
/// Sticky-aware: entities with `is_sticky = 1` whose only supporting note
/// is the one being removed are kept around — we just NULL out their
/// `source_note_id` so the FK reference doesn't dangle. This is the core
/// protection against ~/.claude/projects/*.md churn silently undoing
/// manual merges + corrections.
async fn cleanup_extraction_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    note_id: &str,
) -> Result<ExtractionCleanupReport> {
    // Sticky entities whose source_note is this one — keep them around but
    // detach the now-invalid FK. We need their ids to audit-log "sticky_kept"
    // back at the caller, so SELECT first and UPDATE explicitly.
    let sticky_candidates: Vec<(String,)> = sqlx::query_as(
        "SELECT e.id FROM entities e \
         WHERE e.source_note_id = ? AND e.is_sticky = 1",
    )
    .bind(note_id)
    .fetch_all(&mut **tx)
    .await
    .context("find sticky entities to keep")?;
    let sticky_kept: Vec<String> = sticky_candidates.into_iter().map(|(s,)| s).collect();

    if !sticky_kept.is_empty() {
        sqlx::query(
            "UPDATE entities SET source_note_id = NULL \
             WHERE source_note_id = ? AND is_sticky = 1",
        )
        .bind(note_id)
        .execute(&mut **tx)
        .await
        .context("null out sticky source_note_id")?;
    }

    // Entities whose ONLY supporting note was this one — no relation from any
    // other note touches them. We check *before* deleting the relations
    // because the existence check below needs them in place. `is_sticky = 1`
    // entities were already detached above and are excluded here so they
    // survive the cascade.
    let orphans: Vec<(String,)> = sqlx::query_as(
        "SELECT e.id FROM entities e \
         WHERE e.source_note_id = ? \
         AND e.is_sticky = 0 \
         AND NOT EXISTS ( \
           SELECT 1 FROM relations r \
           WHERE (r.from_entity = e.id OR r.to_entity = e.id) \
             AND r.source_note_id IS NOT NULL \
             AND r.source_note_id != ? \
         )",
    )
    .bind(note_id)
    .bind(note_id)
    .fetch_all(&mut **tx)
    .await
    .context("find orphan entities")?;

    let mut entities_deleted: u64 = 0;
    for (eid,) in &orphans {
        // Virtual `entity_vecs` (sqlite-vec vec0) doesn't honour FK cascades,
        // so wipe the embedding row manually. `entity_embedding_meta` is a
        // real table with ON DELETE CASCADE — that one cleans itself when
        // the entity row goes.
        let _ = sqlx::query("DELETE FROM entity_vecs WHERE entity_id = ?")
            .bind(eid)
            .execute(&mut **tx)
            .await;
        // Mitigation D: any alias pointing INTO this entity has to go too.
        // The legacy FK on `entity_id` cascades, but the new logical
        // `keep_id` column is plain — clear it explicitly so the routing
        // layer can't resurrect a deleted keep on the next extraction pass.
        let _ = sqlx::query("DELETE FROM entity_aliases WHERE keep_id = ?")
            .bind(eid)
            .execute(&mut **tx)
            .await;
        let res = sqlx::query("DELETE FROM entities WHERE id = ?")
            .bind(eid)
            .execute(&mut **tx)
            .await
            .context("delete orphan entity")?;
        entities_deleted += res.rows_affected();
    }

    let rel_res = sqlx::query("DELETE FROM relations WHERE source_note_id = ?")
        .bind(note_id)
        .execute(&mut **tx)
        .await
        .context("delete note's relations")?;

    Ok(ExtractionCleanupReport {
        entities_deleted,
        relations_deleted: rel_res.rows_affected(),
        sticky_kept,
    })
}

/// Result of a cascading note delete — returned by `Db::delete_note`.
/// `sticky_kept` lists entities that the cascade *would* have orphaned
/// but were pinned via `is_sticky = 1` (manual creation, manual edits,
/// or merge-keep side). The caller is expected to surface them to the
/// audit log so the user can see which manual work survived the churn.
#[derive(Debug, Default, Clone, serde::Serialize)]
pub struct DeleteNoteResult {
    pub note_deleted: u64,
    pub entities_deleted: u64,
    pub relations_deleted: u64,
    #[serde(default)]
    pub sticky_kept: Vec<String>,
}

/// Embedding side-channel passed into [`Db::save_extracted_with_router`] so
/// the routing layer can ask the same embedder the caller already holds —
/// avoids forcing `queries.rs` to know how to build a provider. The
/// `model_id` matches the `embedding_model_id` recorded on aliases; if the
/// live id doesn't match a recorded alias, no routing happens (Mitigation
/// B in the design doc).
pub struct RoutingEmbed<'a> {
    pub embedder: &'a std::sync::Arc<dyn crate::embed::EmbeddingProvider>,
    pub model_id: &'a str,
}

/// Pack a slice of f32 as little-endian bytes, the on-wire format sqlite-vec
/// expects for vec0 BLOB columns.
pub fn floats_to_bytes(v: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(v.len() * 4);
    for &f in v {
        out.extend_from_slice(&f.to_le_bytes());
    }
    out
}

/// Stable hash of the text we feed to the embedder. Used to skip re-embedding
/// when the entity hasn't actually changed.
pub fn embedding_text_hash(text: &str) -> String {
    let mut h = Sha256::new();
    h.update(text.as_bytes());
    hex::encode(h.finalize())
}

/// 16-hex-char prefix of SHA-256 over the raw little-endian embedding bytes.
/// Used as a pre-filter in alias routing — two embeddings produced by the
/// same model from the same text *should* hash-match. Truncating to 16 hex
/// chars (64 bits) keeps the alias row small and is still strong enough that
/// collisions inside a personal corpus are negligible.
///
/// The hash is deterministic across runs as long as the embedder is stable;
/// it is *not* a uniqueness contract — alias routing also gates on cosine
/// similarity, so a hash false-positive can never produce a false route.
pub fn embedding_bytes_hash(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    let full = hex::encode(h.finalize());
    full[..16].to_string()
}

/// Pack helper for the alias-routing pre-filter — same as
/// [`embedding_bytes_hash`] but takes a decoded f32 slice. Mirrors the
/// computation that the merge path would have done, so the round-trip
/// embed → bytes → hash is byte-exact.
pub fn embedding_vec_hash(v: &[f32]) -> String {
    embedding_bytes_hash(&floats_to_bytes(v))
}

/// Canonical embedding input for an entity: type + name + JSON properties.
/// Stable across runs.
pub fn entity_embed_text(entity_type: &str, name: &str, properties: &str) -> String {
    format!("{}: {}\n{}", entity_type, name, properties)
}

/// Diff produced by [`Db::update_entity`]. Each populated field is one
/// row written to `entity_edits`. Values are pre-truncated to
/// [`MAX_EDIT_VALUE_LEN`] so the audit + edit log can be inspected
/// without pulling megabyte-sized properties blobs.
#[derive(Debug, Default, Clone)]
pub struct EntityUpdateDiff {
    /// (before, after) pairs by field name. Only present when the
    /// patch actually changed the value (case-sensitive comparison
    /// for `name` / `type`; raw-string equality for `properties`).
    pub name: Option<(String, String)>,
    pub r#type: Option<(String, String)>,
    pub properties: Option<(String, String)>,
}

impl EntityUpdateDiff {
    pub fn is_empty(&self) -> bool {
        self.name.is_none() && self.r#type.is_none() && self.properties.is_none()
    }
}

/// Max characters stored per `value_before` / `value_after` cell in
/// `entity_edits`. The properties JSON can grow large; keeping each
/// diff side bounded means the audit history table stays cheap to
/// scan in the UI.
pub const MAX_EDIT_VALUE_LEN: usize = 1024;

fn truncate_for_audit(s: &str) -> String {
    if s.chars().count() <= MAX_EDIT_VALUE_LEN {
        return s.to_string();
    }
    // Char-aware truncation — slicing on byte indices in the middle of
    // a multi-byte char would panic at runtime.
    s.chars().take(MAX_EDIT_VALUE_LEN).collect()
}

// ----------------------------- Notes -----------------------------

impl Db {
    pub async fn insert_note(&self, text: &str, source: &str) -> Result<String> {
        let id = Ulid::new().to_string();
        let now = now_ms();
        sqlx::query(
            "INSERT INTO notes (id, text, source, created_at, processing_status) \
             VALUES (?, ?, ?, ?, 'pending')",
        )
        .bind(&id)
        .bind(text)
        .bind(source)
        .bind(now)
        .execute(self.pool())
        .await
        .context("insert note")?;
        Ok(id)
    }

    pub async fn mark_note_processed(&self, note_id: &str, error: Option<&str>) -> Result<()> {
        let status = if error.is_some() { "failed" } else { "done" };
        sqlx::query(
            "UPDATE notes SET processed_at = ?, processing_status = ?, extraction_error = ? \
             WHERE id = ?",
        )
        .bind(now_ms())
        .bind(status)
        .bind(error)
        .bind(note_id)
        .execute(self.pool())
        .await
        .context("update note status")?;
        Ok(())
    }

    /// Delete a note and clean up the entities/relations that **only** this
    /// note supported. An entity is kept if any relation from a *different*
    /// note also references it — that's a corroborating mention. Sticky
    /// entities (manual creation / manual edits / merge-keep side) survive
    /// the cascade with their `source_note_id` nulled.
    pub async fn delete_note(&self, note_id: &str) -> Result<DeleteNoteResult> {
        let mut tx = self.pool().begin().await.context("begin delete tx")?;
        let report = cleanup_extraction_in_tx(&mut tx, note_id).await?;

        let note_res = sqlx::query("DELETE FROM notes WHERE id = ?")
            .bind(note_id)
            .execute(&mut *tx)
            .await
            .context("delete note")?;

        tx.commit().await.context("commit delete tx")?;
        Ok(DeleteNoteResult {
            note_deleted: note_res.rows_affected(),
            entities_deleted: report.entities_deleted,
            relations_deleted: report.relations_deleted,
            sticky_kept: report.sticky_kept,
        })
    }

    /// Roll back the *extraction* attached to a note, leaving the raw note
    /// row in place. Used by `reprocess_note` so a re-extraction starts from
    /// a clean slate without duplicating entities. Same orphan-vs-corroborated
    /// logic as `delete_note`. Returns the full
    /// [`ExtractionCleanupReport`] so the import watcher can audit-log any
    /// sticky entities that survived the churn.
    pub async fn cleanup_extraction_for_note(
        &self,
        note_id: &str,
    ) -> Result<ExtractionCleanupReport> {
        let mut tx = self.pool().begin().await.context("begin cleanup tx")?;
        let report = cleanup_extraction_in_tx(&mut tx, note_id).await?;
        // Also reset the note's processing flags so it shows as `pending`
        // until the new extraction completes.
        sqlx::query(
            "UPDATE notes SET processed_at = NULL, processing_status = 'pending', \
                              extraction_error = NULL WHERE id = ?",
        )
        .bind(note_id)
        .execute(&mut *tx)
        .await
        .context("reset note flags")?;
        tx.commit().await.context("commit cleanup tx")?;
        Ok(report)
    }

    pub async fn list_notes(&self, limit: i64) -> Result<Vec<Note>> {
        let rows = sqlx::query_as::<_, Note>(
            "SELECT id, text, source, created_at, processed_at, processing_status, extraction_error \
             FROM notes ORDER BY created_at DESC LIMIT ?",
        )
        .bind(limit)
        .fetch_all(self.pool())
        .await
        .context("list notes")?;
        Ok(rows)
    }

    /// Fetch a batch of notes by id. Used by `/api/search` to surface each
    /// matched entity's source note as a picker option — handy when the
    /// query semantically matches the entity (e.g. "我家儿子") but its
    /// wording differs from the note text ("我们家娃").
    pub async fn notes_by_ids(&self, ids: &[String]) -> Result<Vec<Note>> {
        if ids.is_empty() {
            return Ok(vec![]);
        }
        let placeholders = std::iter::repeat("?")
            .take(ids.len())
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "SELECT id, text, source, created_at, processed_at, processing_status, extraction_error \
             FROM notes WHERE id IN ({placeholders})"
        );
        let mut q = sqlx::query_as::<_, Note>(&sql);
        for id in ids {
            q = q.bind(id);
        }
        let rows = q.fetch_all(self.pool()).await.context("notes_by_ids")?;
        Ok(rows)
    }

    pub async fn search_notes_bm25(&self, query: &str, limit: i64) -> Result<Vec<Note>> {
        // FTS5 BM25 ranking: lower bm25() == better.
        let rows = sqlx::query_as::<_, Note>(
            "SELECT n.id, n.text, n.source, n.created_at, n.processed_at, \
                    n.processing_status, n.extraction_error \
             FROM notes_fts f \
             JOIN notes n ON n.rowid = f.rowid \
             WHERE notes_fts MATCH ? \
             ORDER BY bm25(notes_fts) ASC, n.created_at DESC \
             LIMIT ?",
        )
        .bind(query)
        .bind(limit)
        .fetch_all(self.pool())
        .await
        .context("fts search notes")?;
        Ok(rows)
    }

    /// LIKE-based substring search over `notes.text`. The trigram tokenizer
    /// can't match queries shorter than 3 characters (which matters for CJK
    /// — a 2-char query like "家务" is common), so the retriever calls this
    /// as a fallback. Performance scales linearly with row count, but that's
    /// fine for personal-scale corpora.
    pub async fn search_notes_like(&self, raw_query: &str, limit: i64) -> Result<Vec<Note>> {
        if raw_query.trim().is_empty() {
            return Ok(vec![]);
        }
        let pat = format!(
            "%{}%",
            raw_query.trim().replace('%', "\\%").replace('_', "\\_")
        );
        let rows = sqlx::query_as::<_, Note>(
            "SELECT id, text, source, created_at, processed_at, \
                    processing_status, extraction_error \
             FROM notes \
             WHERE text LIKE ? ESCAPE '\\' \
             ORDER BY created_at DESC \
             LIMIT ?",
        )
        .bind(&pat)
        .bind(limit)
        .fetch_all(self.pool())
        .await
        .context("like search notes")?;
        Ok(rows)
    }

    /// Same fallback for entities — matches against name + properties.
    /// Returns `(Entity, score)` tuples to match the BM25 path's shape;
    /// LIKE has no real score, so we hand back a constant 1.0 (the caller's
    /// RRF only cares about rank order, not absolute score values).
    pub async fn search_entities_like(
        &self,
        raw_query: &str,
        types: Option<&[String]>,
        limit: i64,
    ) -> Result<Vec<(Entity, f64)>> {
        if raw_query.trim().is_empty() {
            return Ok(vec![]);
        }
        let pat = format!(
            "%{}%",
            raw_query.trim().replace('%', "\\%").replace('_', "\\_")
        );
        // sqlx-sqlite uses positional `?` only — no `?1` reuse. Bind the
        // pattern twice (once for `name`, once for `properties`).
        // Archived entities are intentionally excluded — they only come
        // back into search after the user explicitly restores them.
        let base = "SELECT id, type, name, properties, confidence, source_note_id, \
                           created_at, updated_at, strength, last_accessed, \
                           access_count, base_importance, is_sticky, archived_at \
                    FROM entities \
                    WHERE archived_at IS NULL \
                      AND (name LIKE ? ESCAPE '\\' OR properties LIKE ? ESCAPE '\\')";
        let rows: Vec<Entity> = if let Some(ts) = types.filter(|v| !v.is_empty()) {
            let placeholders = (0..ts.len()).map(|_| "?").collect::<Vec<_>>().join(",");
            let sql = format!(
                "{base} AND type IN ({placeholders}) \
                 ORDER BY updated_at DESC LIMIT ?"
            );
            let mut q = sqlx::query_as::<_, Entity>(&sql).bind(&pat).bind(&pat);
            for t in ts {
                q = q.bind(t);
            }
            q.bind(limit)
                .fetch_all(self.pool())
                .await
                .context("like search entities")?
        } else {
            sqlx::query_as::<_, Entity>(&format!("{base} ORDER BY updated_at DESC LIMIT ?"))
                .bind(&pat)
                .bind(&pat)
                .bind(limit)
                .fetch_all(self.pool())
                .await
                .context("like search entities")?
        };
        Ok(rows.into_iter().map(|e| (e, 1.0)).collect())
    }

    pub async fn recent_notes(&self, limit: i64) -> Result<Vec<Note>> {
        self.list_notes(limit).await
    }
}

// ----------------------------- Entities -----------------------------

impl Db {
    /// Insert or upsert entities + relations extracted from a note.
    /// Dedupe by (type, name COLLATE NOCASE). Updates strength on every touch.
    /// Persist an extraction's entities + relations. On a transient SQLite
    /// write conflict (e.g. `SQLITE_BUSY_SNAPSHOT` (code 517) when another
    /// connection has just committed a write) we retry up to 4 times with
    /// short exponential back-off — `busy_timeout` doesn't cover that error
    /// code, so the retry has to live at the application layer.
    pub async fn save_extracted(
        &self,
        note_id: &str,
        result: &ExtractionResult,
    ) -> Result<Vec<String>> {
        let mut delay_ms: u64 = 60;
        let mut attempts = 0;
        loop {
            match self.save_extracted_once(note_id, result).await {
                Ok(ids) => return Ok(ids),
                Err(e) if is_transient_lock_error(&e) && attempts < 4 => {
                    tracing::info!(
                        note_id,
                        attempts,
                        delay_ms,
                        "save_extracted hit transient SQLite lock; retrying"
                    );
                    tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                    attempts += 1;
                    delay_ms = (delay_ms * 2).min(800);
                }
                Err(e) => return Err(e),
            }
        }
    }

    async fn save_extracted_once(
        &self,
        note_id: &str,
        result: &ExtractionResult,
    ) -> Result<Vec<String>> {
        let (ids, _routed) = self.save_extracted_inner(note_id, result, None).await?;
        Ok(ids)
    }

    /// Persist an extraction *and* run alias routing first. Candidates that
    /// match a recorded `(name_lower, embedding_model_id)` alias and clear
    /// the cosine threshold against the keep's current embedding are merged
    /// into the keep instead of inserting a fresh row. This is the core fix
    /// for ~/.claude memory churn silently undoing manual merges.
    ///
    /// Returns `(saved_ids, routed)` where `saved_ids` includes both
    /// freshly-inserted and routed entities (so the caller's embedding +
    /// audit pipeline still treats them uniformly). `routed` is a map
    /// `extracted_name -> keep_id` describing every routing decision so the
    /// caller can audit-log them with the `"alias_routed"` action.
    ///
    /// If `embed_fn` is `None`, routing is disabled and behaviour matches
    /// [`Db::save_extracted`] — the tests and bundle-replay paths use this.
    pub async fn save_extracted_with_router(
        &self,
        note_id: &str,
        result: &ExtractionResult,
        embed_ctx: Option<RoutingEmbed<'_>>,
    ) -> Result<(Vec<String>, RoutingDecisions)> {
        let mut delay_ms: u64 = 60;
        let mut attempts = 0;
        loop {
            match self
                .save_extracted_inner(note_id, result, embed_ctx.as_ref())
                .await
            {
                Ok(pair) => return Ok(pair),
                Err(e) if is_transient_lock_error(&e) && attempts < 4 => {
                    tracing::info!(
                        note_id,
                        attempts,
                        delay_ms,
                        "save_extracted_with_router hit transient SQLite lock; retrying"
                    );
                    tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                    attempts += 1;
                    delay_ms = (delay_ms * 2).min(800);
                }
                Err(e) => return Err(e),
            }
        }
    }

    async fn save_extracted_inner(
        &self,
        note_id: &str,
        result: &ExtractionResult,
        embed_ctx: Option<&RoutingEmbed<'_>>,
    ) -> Result<(Vec<String>, RoutingDecisions)> {
        let mut tx = self.pool().begin().await?;
        let now = now_ms();
        let mut saved_ids = Vec::new();
        let mut name_to_id = std::collections::HashMap::new();
        let mut routed = RoutingDecisions::default();

        for ext in &result.entities {
            if ext.confidence < 0.6 {
                continue; // skip low confidence per prompt rules
            }
            let Some(_typ) = EntityType::parse(&ext.r#type) else {
                tracing::warn!(?ext.r#type, "unknown entity type, skipping");
                continue;
            };
            let props_json = serde_json::to_string(&ext.properties)?;

            // Find existing entity by (type, name). This is the primary
            // dedup path that already merged "name COLLATE NOCASE" matches
            // onto a single row — alias routing only kicks in when *no*
            // exact-name match exists (e.g. a previous merge collapsed
            // "Mom" into "Helen" — the next note that says "Mom" no longer
            // finds a same-named row to reinforce).
            let existing: Option<(String, f64, i64)> = sqlx::query_as(
                "SELECT id, strength, access_count FROM entities \
                 WHERE type = ? AND name = ? COLLATE NOCASE LIMIT 1",
            )
            .bind(&ext.r#type)
            .bind(&ext.name)
            .fetch_optional(&mut *tx)
            .await?;

            let id = match existing {
                Some((existing_id, existing_strength, existing_count)) => {
                    // Strengthen on reinforcement
                    let new_strength =
                        importance::reinforce(existing_strength, existing_count as u32);
                    sqlx::query(
                        "UPDATE entities SET \
                           properties = json_patch(properties, ?), \
                           confidence = MAX(confidence, ?), \
                           updated_at = ?, \
                           last_accessed = ?, \
                           access_count = access_count + 1, \
                           strength = ? \
                         WHERE id = ?",
                    )
                    .bind(&props_json)
                    .bind(ext.confidence)
                    .bind(now)
                    .bind(now)
                    .bind(new_strength)
                    .bind(&existing_id)
                    .execute(&mut *tx)
                    .await?;
                    existing_id
                }
                None => {
                    // Alias routing: only attempted if the caller supplied
                    // an embedder + model id (production path). Tests +
                    // bundle replay leave it None and fall through to a
                    // plain insert.
                    let route_decision = if let Some(ctx) = embed_ctx {
                        let candidate_text = entity_embed_text(&ext.r#type, &ext.name, &props_json);
                        // We only embed when there's at least one alias
                        // row with matching name + model — otherwise the
                        // embedding call is wasted. Quick existence check
                        // first.
                        let name_lower = ext.name.to_lowercase();
                        let has_candidate: Option<(i64,)> = sqlx::query_as(
                            "SELECT 1 FROM entity_aliases \
                             WHERE name_lower = ? AND embedding_model_id = ? LIMIT 1",
                        )
                        .bind(&name_lower)
                        .bind(ctx.model_id)
                        .fetch_optional(&mut *tx)
                        .await?;
                        if has_candidate.is_some() {
                            match ctx.embedder.embed_one(&candidate_text).await {
                                Ok(v) if v.len() == ctx.embedder.dim() => {
                                    self.route_candidate_in_tx(
                                        &mut tx,
                                        &name_lower,
                                        &v,
                                        ctx.model_id,
                                    )
                                    .await?
                                }
                                Ok(_) => None,
                                Err(e) => {
                                    tracing::warn!(
                                        ?e,
                                        candidate = %ext.name,
                                        "alias-routing embed failed; falling through to insert"
                                    );
                                    None
                                }
                            }
                        } else {
                            None
                        }
                    } else {
                        None
                    };

                    if let Some((alias_id, keep_id)) = route_decision {
                        // Route: attach the note to the keep, bump the
                        // alias's routed_count, no new entity row.
                        self.record_alias_route_in_tx(&mut tx, &alias_id, &keep_id, note_id, now)
                            .await?;
                        routed.routed.insert(ext.name.clone(), keep_id.clone());
                        keep_id
                    } else {
                        let new_id = Ulid::new().to_string();
                        sqlx::query(
                            "INSERT INTO entities \
                               (id, type, name, properties, confidence, source_note_id, \
                                created_at, updated_at, strength, last_accessed, access_count, \
                                base_importance) \
                             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                        )
                        .bind(&new_id)
                        .bind(&ext.r#type)
                        .bind(&ext.name)
                        .bind(&props_json)
                        .bind(ext.confidence)
                        .bind(note_id)
                        .bind(now)
                        .bind(now)
                        .bind(1.0_f64) // initial S
                        .bind(now)
                        .bind(1_i64)
                        .bind(importance::base_importance_for_type(&ext.r#type))
                        .execute(&mut *tx)
                        .await?;
                        new_id
                    }
                }
            };
            name_to_id.insert(ext.name.clone(), id.clone());
            saved_ids.push(id);
        }

        // Relations (best-effort: skip if either endpoint missing)
        for rel in &result.relations {
            let (Some(from_id), Some(to_id)) = (name_to_id.get(&rel.from), name_to_id.get(&rel.to))
            else {
                continue;
            };
            let rid = Ulid::new().to_string();
            sqlx::query(
                "INSERT INTO relations (id, from_entity, to_entity, type, source_note_id, created_at) \
                 VALUES (?, ?, ?, ?, ?, ?)",
            )
            .bind(&rid)
            .bind(from_id)
            .bind(to_id)
            .bind(&rel.r#type)
            .bind(note_id)
            .bind(now)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok((saved_ids, routed))
    }

    pub async fn list_entities(
        &self,
        entity_type: Option<&str>,
        limit: i64,
    ) -> Result<Vec<Entity>> {
        let rows = if let Some(t) = entity_type {
            sqlx::query_as::<_, Entity>(
                "SELECT id, type, name, properties, confidence, source_note_id, \
                        created_at, updated_at, strength, last_accessed, access_count, base_importance, is_sticky, archived_at \
                 FROM entities WHERE type = ? ORDER BY updated_at DESC LIMIT ?",
            )
            .bind(t)
            .bind(limit)
            .fetch_all(self.pool())
            .await?
        } else {
            sqlx::query_as::<_, Entity>(
                "SELECT id, type, name, properties, confidence, source_note_id, \
                        created_at, updated_at, strength, last_accessed, access_count, base_importance, is_sticky, archived_at \
                 FROM entities ORDER BY updated_at DESC LIMIT ?",
            )
            .bind(limit)
            .fetch_all(self.pool())
            .await?
        };
        Ok(rows)
    }

    pub async fn get_entity(&self, id: &str) -> Result<Option<Entity>> {
        let row = sqlx::query_as::<_, Entity>(
            "SELECT id, type, name, properties, confidence, source_note_id, \
                    created_at, updated_at, strength, last_accessed, access_count, base_importance, is_sticky, archived_at \
             FROM entities WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(self.pool())
        .await?;
        Ok(row)
    }

    pub async fn entity_relations(&self, entity_id: &str) -> Result<Vec<Relation>> {
        let rows = sqlx::query_as::<_, Relation>(
            "SELECT id, from_entity, to_entity, type, properties, confidence, source_note_id, created_at \
             FROM relations WHERE from_entity = ? OR to_entity = ? ORDER BY created_at DESC",
        )
        .bind(entity_id)
        .bind(entity_id)
        .fetch_all(self.pool())
        .await?;
        Ok(rows)
    }

    /// Search entities by FTS5 BM25, optionally filtered by type.
    /// Returns (entity, bm25_score) so the caller can re-rank.
    pub async fn search_entities_bm25(
        &self,
        query: &str,
        types: Option<&[String]>,
        limit: i64,
    ) -> Result<Vec<(Entity, f64)>> {
        if query.trim().is_empty() {
            return Ok(vec![]);
        }
        let type_filter = match types {
            Some(ts) if !ts.is_empty() => {
                let placeholders = ts.iter().map(|_| "?").collect::<Vec<_>>().join(",");
                format!("AND e.type IN ({})", placeholders)
            }
            _ => String::new(),
        };

        // `entities_fts` is an FTS5 index that holds tombstones for deleted
        // rows in the underlying triggers; rows survive while their entity
        // does. We additionally drop archived entities so soft-deleted
        // memory doesn't surface in search.
        let sql = format!(
            "SELECT e.id, e.type, e.name, e.properties, e.confidence, e.source_note_id, \
                    e.created_at, e.updated_at, e.strength, e.last_accessed, e.access_count, \
                    e.base_importance, e.is_sticky, e.archived_at, bm25(entities_fts) AS bm25 \
             FROM entities_fts f \
             JOIN entities e ON e.rowid = f.rowid \
             WHERE entities_fts MATCH ? AND e.archived_at IS NULL {} \
             ORDER BY bm25 ASC LIMIT ?",
            type_filter
        );

        let mut q = sqlx::query(&sql).bind(query);
        if let Some(ts) = types {
            for t in ts {
                q = q.bind(t);
            }
        }
        q = q.bind(limit);
        let rows = q.fetch_all(self.pool()).await?;
        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            let entity = Entity {
                id: row.try_get("id")?,
                r#type: row.try_get("type")?,
                name: row.try_get("name")?,
                properties: row.try_get("properties")?,
                confidence: row.try_get("confidence")?,
                source_note_id: row.try_get("source_note_id")?,
                created_at: row.try_get("created_at")?,
                updated_at: row.try_get("updated_at")?,
                strength: row.try_get("strength")?,
                last_accessed: row.try_get("last_accessed")?,
                access_count: row.try_get("access_count")?,
                base_importance: row.try_get("base_importance")?,
                is_sticky: row.try_get("is_sticky")?,
                archived_at: row.try_get("archived_at")?,
            };
            let bm25: f64 = row.try_get("bm25")?;
            out.push((entity, bm25));
        }
        Ok(out)
    }

    /// Record an access — moves the Ebbinghaus needle.
    pub async fn touch_entity(&self, id: &str) -> Result<()> {
        let now = now_ms();
        sqlx::query(
            "UPDATE entities SET \
               last_accessed = ?, \
               access_count = access_count + 1, \
               strength = strength + 0.5 \
             WHERE id = ?",
        )
        .bind(now)
        .bind(id)
        .execute(self.pool())
        .await?;
        Ok(())
    }

    /// Apply a user-driven patch to an entity. Only `name`, `type`, and
    /// `properties` are mutable here — `source_note_id`, `created_at`,
    /// `confidence`, and the Ebbinghaus signals are owned by the
    /// extraction / retrieval layers, not the editor.
    ///
    /// Returns the updated [`Entity`] plus a diff that callers feed
    /// into `log_entity_edit` (one row per changed field). If no field
    /// changes value, the diff is empty and `updated_at` is left alone
    /// — no-op edits don't pollute the history table.
    pub async fn update_entity(
        &self,
        id: &str,
        new_name: Option<&str>,
        new_type: Option<&str>,
        new_properties: Option<&str>,
    ) -> Result<(Entity, EntityUpdateDiff)> {
        // We do the read inside a transaction so a concurrent writer
        // can't slip in between SELECT and UPDATE and produce a stale
        // before-image in the audit row.
        let mut tx = self.pool().begin().await.context("begin update tx")?;
        let current: Option<Entity> = sqlx::query_as::<_, Entity>(
            "SELECT id, type, name, properties, confidence, source_note_id, \
                    created_at, updated_at, strength, last_accessed, access_count, base_importance, is_sticky, archived_at \
             FROM entities WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&mut *tx)
        .await
        .context("load entity for update")?;
        let current = current.ok_or_else(|| anyhow::anyhow!("entity not found"))?;

        let mut diff = EntityUpdateDiff::default();
        if let Some(n) = new_name {
            if n != current.name {
                diff.name = Some((current.name.clone(), n.to_string()));
            }
        }
        if let Some(t) = new_type {
            if t != current.r#type {
                diff.r#type = Some((current.r#type.clone(), t.to_string()));
            }
        }
        if let Some(p) = new_properties {
            // Raw-string compare. We deliberately don't json-normalise
            // — if the user moved keys around it's still an edit worth
            // logging.
            if p != current.properties {
                diff.properties = Some((current.properties.clone(), p.to_string()));
            }
        }

        if diff.is_empty() {
            tx.commit().await.context("commit update tx (no-op)")?;
            return Ok((current, diff));
        }

        let now = now_ms();
        let final_name = diff
            .name
            .as_ref()
            .map(|(_, a)| a.as_str())
            .unwrap_or(&current.name);
        let final_type = diff
            .r#type
            .as_ref()
            .map(|(_, a)| a.as_str())
            .unwrap_or(&current.r#type);
        let final_props = diff
            .properties
            .as_ref()
            .map(|(_, a)| a.as_str())
            .unwrap_or(&current.properties);

        sqlx::query(
            "UPDATE entities SET name = ?, type = ?, properties = ?, updated_at = ? WHERE id = ?",
        )
        .bind(final_name)
        .bind(final_type)
        .bind(final_props)
        .bind(now)
        .bind(id)
        .execute(&mut *tx)
        .await
        .context("apply entity patch")?;

        let updated = Entity {
            id: current.id.clone(),
            r#type: final_type.to_string(),
            name: final_name.to_string(),
            properties: final_props.to_string(),
            confidence: current.confidence,
            source_note_id: current.source_note_id.clone(),
            created_at: current.created_at,
            updated_at: now,
            strength: current.strength,
            last_accessed: current.last_accessed,
            access_count: current.access_count,
            base_importance: current.base_importance,
            is_sticky: current.is_sticky,
            archived_at: current.archived_at,
        };
        tx.commit().await.context("commit update tx")?;
        Ok((updated, diff))
    }

    /// Insert a hand-crafted entity that doesn't come from extraction.
    /// `source_note_id = NULL` is the marker — combined with the
    /// `entity_edits` create row, it tells the rest of the system this
    /// entity is user-authored and should be treated with high trust.
    ///
    /// Callers handle embedding + audit + edit-log; this just owns the
    /// INSERT so the column list lives in one place.
    pub async fn insert_entity_manual(
        &self,
        entity_type: &str,
        name: &str,
        properties_json: &str,
        base_importance: f64,
    ) -> Result<Entity> {
        let id = Ulid::new().to_string();
        let now = now_ms();
        sqlx::query(
            "INSERT INTO entities \
                (id, type, name, properties, confidence, source_note_id, \
                 created_at, updated_at, strength, last_accessed, access_count, \
                 base_importance, is_sticky) \
             VALUES (?, ?, ?, ?, ?, NULL, ?, ?, ?, ?, ?, ?, 1)",
        )
        .bind(&id)
        .bind(entity_type)
        .bind(name)
        .bind(properties_json)
        .bind(1.0_f64) // user-asserted facts get full confidence
        .bind(now)
        .bind(now)
        .bind(1.0_f64) // initial S — same as extracted entities
        .bind(now)
        .bind(0_i64) // access_count starts at 0; touch_entity bumps on first read
        .bind(base_importance)
        .execute(self.pool())
        .await
        .context("insert manual entity")?;

        Ok(Entity {
            id,
            r#type: entity_type.to_string(),
            name: name.to_string(),
            properties: properties_json.to_string(),
            confidence: 1.0,
            source_note_id: None,
            created_at: now,
            updated_at: now,
            strength: 1.0,
            last_accessed: now,
            access_count: 0,
            base_importance,
            // Manual entities are sticky by default (mitigation §2) — they
            // survive cascade-delete from any note they later get attached
            // to via alias routing.
            is_sticky: 1,
            archived_at: None,
        })
    }

    /// Overwrite the body of a note. The `notes_fts_au` trigger (see
    /// `migrations/20260513000001_initial.sql` + the trigram re-install
    /// in `20260515000001_fts_trigram.sql`) keeps the FTS5 index in
    /// sync automatically, so callers don't need to re-index manually.
    pub async fn update_note_text(&self, note_id: &str, text: &str) -> Result<bool> {
        let res = sqlx::query("UPDATE notes SET text = ? WHERE id = ?")
            .bind(text)
            .bind(note_id)
            .execute(self.pool())
            .await
            .context("update note text")?;
        Ok(res.rows_affected() > 0)
    }

    /// Append one row to `entity_edits`. For update, call once per
    /// changed field with the truncated before/after pair. For create
    /// and delete, pass `field_changed = None` and `(before, after)
    /// = (None, None)`.
    pub async fn log_entity_edit(
        &self,
        entity_id: &str,
        action: &str,
        field_changed: Option<&str>,
        value_before: Option<&str>,
        value_after: Option<&str>,
    ) -> Result<()> {
        let id = Ulid::new().to_string();
        let before_truncated = value_before.map(truncate_for_audit);
        let after_truncated = value_after.map(truncate_for_audit);
        sqlx::query(
            "INSERT INTO entity_edits \
                (id, entity_id, action, field_changed, value_before, value_after, \
                 edited_by, edited_at) \
             VALUES (?, ?, ?, ?, ?, ?, 'user', ?)",
        )
        .bind(&id)
        .bind(entity_id)
        .bind(action)
        .bind(field_changed)
        .bind(before_truncated.as_deref())
        .bind(after_truncated.as_deref())
        .bind(now_ms())
        .execute(self.pool())
        .await
        .context("insert entity_edits row")?;
        Ok(())
    }

    /// Total number of manual edit rows (any action) for one entity.
    /// Used by callers that only need a single entity's count.
    pub async fn manual_edit_count(&self, entity_id: &str) -> Result<i64> {
        let row: Option<(i64,)> =
            sqlx::query_as("SELECT COUNT(*) FROM entity_edits WHERE entity_id = ?")
                .bind(entity_id)
                .fetch_optional(self.pool())
                .await
                .context("manual_edit_count")?;
        Ok(row.map(|r| r.0).unwrap_or(0))
    }

    /// Batch version: returns a map `entity_id -> count` for the
    /// supplied ids. Skips ids with zero edits (caller defaults to 0).
    /// Used by the retrieval layer so a search over N candidate
    /// entities doesn't fan out into N separate `COUNT(*)` queries.
    pub async fn manual_edit_counts(
        &self,
        entity_ids: &[String],
    ) -> Result<std::collections::HashMap<String, i64>> {
        if entity_ids.is_empty() {
            return Ok(std::collections::HashMap::new());
        }
        let placeholders = entity_ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!(
            "SELECT entity_id, COUNT(*) AS c FROM entity_edits \
             WHERE entity_id IN ({}) GROUP BY entity_id",
            placeholders
        );
        let mut q = sqlx::query(&sql);
        for id in entity_ids {
            q = q.bind(id);
        }
        let rows = q
            .fetch_all(self.pool())
            .await
            .context("manual_edit_counts batch")?;
        let mut out = std::collections::HashMap::with_capacity(rows.len());
        for row in rows {
            let id: String = row.try_get("entity_id")?;
            let c: i64 = row.try_get("c")?;
            out.insert(id, c);
        }
        Ok(out)
    }

    /// List recent edits for one entity, newest first. Backs the
    /// Entity Detail page's edit-history timeline. Values were already
    /// truncated to [`MAX_EDIT_VALUE_LEN`] at insertion time, so the
    /// rows are safe to ship as-is to the UI.
    pub async fn list_entity_edits(
        &self,
        entity_id: &str,
        limit: i64,
    ) -> Result<Vec<EntityEditRow>> {
        let rows = sqlx::query_as::<_, EntityEditRow>(
            "SELECT id, entity_id, action, field_changed, value_before, value_after, \
                    edited_by, edited_at \
             FROM entity_edits \
             WHERE entity_id = ? \
             ORDER BY edited_at DESC \
             LIMIT ?",
        )
        .bind(entity_id)
        .bind(limit)
        .fetch_all(self.pool())
        .await
        .context("list entity edits")?;
        Ok(rows)
    }
}

/// One row of `entity_edits` as the UI sees it. The column order matches
/// the SQL in [`Db::list_entity_edits`]; fields are pre-truncated to
/// [`MAX_EDIT_VALUE_LEN`] at write time.
#[derive(Debug, Clone, sqlx::FromRow, serde::Serialize)]
pub struct EntityEditRow {
    pub id: String,
    pub entity_id: String,
    pub action: String,
    pub field_changed: Option<String>,
    pub value_before: Option<String>,
    pub value_after: Option<String>,
    pub edited_by: String,
    pub edited_at: i64,
}

/// One row of `entity_aliases` as the UI / IPC sees it. `keep_id`
/// is the surviving entity an extraction should be routed onto when
/// a candidate matches the recorded `name_lower` *and* clears the
/// cosine threshold against the keep's current embedding under the
/// same `embedding_model_id`.
///
/// Persisted on every successful merge — see [`Db::merge_entities`].
#[derive(Debug, Clone, sqlx::FromRow, serde::Serialize)]
pub struct AliasRow {
    pub id: String,
    pub keep_id: String,
    pub name_lower: String,
    pub embedding_hash: Option<String>,
    pub embedding_model_id: Option<String>,
    pub created_at: i64,
    pub routed_count: i64,
}

/// Outcome of an alias-routed extraction step. The caller pipes the
/// `routed` map into the audit log so the user can see exactly which
/// re-extracted entities collapsed onto a manually-merged keep.
#[derive(Debug, Default, Clone)]
pub struct RoutingDecisions {
    /// `(extracted_name, candidate_lower) -> keep_id` for every entity
    /// that was routed onto an existing keep instead of inserted afresh.
    pub routed: std::collections::HashMap<String, String>,
}

impl Db {
    /// Pin an entity so it survives the import-watcher cascade. Idempotent.
    /// Called when:
    ///   - The user manually creates an entity (`create_entity` IPC).
    ///   - The user edits an entity (any post-create row in `entity_edits`).
    ///   - The entity ends up on the keep side of a merge.
    pub async fn set_entity_sticky(&self, entity_id: &str) -> Result<()> {
        sqlx::query("UPDATE entities SET is_sticky = 1 WHERE id = ?")
            .bind(entity_id)
            .execute(self.pool())
            .await
            .context("set entity sticky")?;
        Ok(())
    }

    /// Clear the sticky flag. Only callable from `remove_entity_alias`
    /// when an entity has no remaining aliases AND no manual edits —
    /// the spec is explicit that we don't override the user's intent here.
    pub async fn clear_entity_sticky(&self, entity_id: &str) -> Result<()> {
        sqlx::query("UPDATE entities SET is_sticky = 0 WHERE id = ?")
            .bind(entity_id)
            .execute(self.pool())
            .await
            .context("clear entity sticky")?;
        Ok(())
    }

    /// List every alias pointing at `entity_id`. Backs the Entity Detail
    /// "merged from" section + the unmerge UI. Newest first so the user
    /// sees the most recent merge at the top.
    pub async fn list_entity_aliases(&self, entity_id: &str) -> Result<Vec<AliasRow>> {
        let rows = sqlx::query_as::<_, AliasRow>(
            "SELECT id, keep_id, name_lower, embedding_hash, embedding_model_id, \
                    created_at, routed_count \
             FROM entity_aliases \
             WHERE keep_id = ? \
             ORDER BY created_at DESC",
        )
        .bind(entity_id)
        .fetch_all(self.pool())
        .await
        .context("list entity aliases")?;
        Ok(rows)
    }

    /// Look up a single alias row by id. Used by the unmerge IPC before
    /// deletion so we can audit-log the keep + name being unbound.
    pub async fn get_entity_alias(&self, alias_id: &str) -> Result<Option<AliasRow>> {
        let row = sqlx::query_as::<_, AliasRow>(
            "SELECT id, keep_id, name_lower, embedding_hash, embedding_model_id, \
                    created_at, routed_count \
             FROM entity_aliases \
             WHERE id = ?",
        )
        .bind(alias_id)
        .fetch_optional(self.pool())
        .await
        .context("get entity alias")?;
        Ok(row)
    }

    /// Delete one alias row by id. Returns `Ok(true)` if a row was
    /// removed. Called from the `remove_entity_alias` IPC.
    pub async fn delete_entity_alias(&self, alias_id: &str) -> Result<bool> {
        let res = sqlx::query("DELETE FROM entity_aliases WHERE id = ?")
            .bind(alias_id)
            .execute(self.pool())
            .await
            .context("delete entity alias")?;
        Ok(res.rows_affected() > 0)
    }

    /// Count aliases still pointing at `entity_id`. The unmerge IPC uses
    /// this to decide whether to clear `is_sticky` (only when zero
    /// remaining aliases *and* zero manual edits).
    pub async fn count_entity_aliases(&self, entity_id: &str) -> Result<i64> {
        let row: Option<(i64,)> =
            sqlx::query_as("SELECT COUNT(*) FROM entity_aliases WHERE keep_id = ?")
                .bind(entity_id)
                .fetch_optional(self.pool())
                .await
                .context("count entity aliases")?;
        Ok(row.map(|r| r.0).unwrap_or(0))
    }

    /// Look up the keep entities that have ever been aliased from the
    /// given `(name_lower, embedding_model_id)` pair. The routing layer
    /// will further gate matches by cosine similarity against the keep's
    /// current embedding — see [`AliasRouter::route_candidate`].
    async fn alias_keep_ids_for(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        name_lower: &str,
        embedding_model_id: &str,
    ) -> Result<Vec<(String, String)>> {
        // (alias_id, keep_id). Skipping rows with NULL embedding_model_id
        // is intentional — without a recorded model we cannot guarantee
        // the routing is safe across embedder changes.
        let rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT id, keep_id FROM entity_aliases \
             WHERE name_lower = ? \
               AND embedding_model_id = ? \
               AND keep_id IS NOT NULL",
        )
        .bind(name_lower)
        .bind(embedding_model_id)
        .fetch_all(&mut **tx)
        .await
        .context("alias_keep_ids_for")?;
        Ok(rows)
    }

    /// Routing pre-check used by `save_extracted_with_router`.
    ///
    /// Returns `Some(keep_id)` when the candidate matches an existing
    /// alias *and* the candidate embedding clears the cosine threshold
    /// (≥ `ALIAS_ROUTING_COSINE_MIN`) against the keep's *current*
    /// embedding under the same `embedding_model_id`. Otherwise returns
    /// `None` and the caller falls back to the normal extraction path.
    ///
    /// The model-id check is the critical cross-model guard: if the
    /// recorded `embedding_model_id` differs from the live one, the
    /// SELECT above returns no rows, so we never route across an
    /// embedder switch. The `embedding_hash` is recorded only as a
    /// debugging breadcrumb — it intentionally doesn't gate routing
    /// because a fresh re-embed of the same text can produce
    /// byte-identical vectors but a *fresh re-embed of a modified
    /// candidate* may diverge slightly even within the same model.
    async fn route_candidate_in_tx(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        name_lower: &str,
        candidate_vec: &[f32],
        embedding_model_id: &str,
    ) -> Result<Option<(String, String)>> {
        let alias_rows = self
            .alias_keep_ids_for(tx, name_lower, embedding_model_id)
            .await?;
        for (alias_id, keep_id) in alias_rows {
            // Pull the keep's current embedding; if it's missing (e.g. the
            // user re-embedded since the merge), we can't safely route.
            let row: Option<(Vec<u8>,)> =
                sqlx::query_as("SELECT embedding FROM entity_vecs WHERE entity_id = ?")
                    .bind(&keep_id)
                    .fetch_optional(&mut **tx)
                    .await
                    .context("load keep embedding for routing")?;
            let Some((bytes,)) = row else { continue };
            let keep_vec = bytes_to_floats(&bytes);
            if keep_vec.len() != candidate_vec.len() {
                // Dim mismatch — the keep was embedded under a different
                // model than the SELECT filter would have allowed, almost
                // certainly mid-flight provider swap. Skip rather than
                // mis-route.
                continue;
            }
            let sim = cosine_similarity(candidate_vec, &keep_vec);
            if sim.is_finite() && sim >= ALIAS_ROUTING_COSINE_MIN {
                return Ok(Some((alias_id, keep_id)));
            }
        }
        Ok(None)
    }

    /// Bump the recorded route count + last-accessed signal on the keep
    /// after a successful routing decision. Called inside the same tx
    /// that owns the routing so the bookkeeping is atomic.
    async fn record_alias_route_in_tx(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        alias_id: &str,
        keep_id: &str,
        note_id: &str,
        now: i64,
    ) -> Result<()> {
        sqlx::query("UPDATE entity_aliases SET routed_count = routed_count + 1 WHERE id = ?")
            .bind(alias_id)
            .execute(&mut **tx)
            .await
            .context("bump routed_count")?;
        // The freshly re-extracted note is the new corroborating source.
        // We don't blat the keep's `source_note_id` if it already has one
        // — keeping that pointer stable means existing UI deep-links keep
        // working — but DO bump access_count + last_accessed so the
        // retrieval layer treats this as a fresh signal.
        sqlx::query(
            "UPDATE entities SET \
               source_note_id = COALESCE(source_note_id, ?), \
               access_count = access_count + 1, \
               last_accessed = ? \
             WHERE id = ?",
        )
        .bind(note_id)
        .bind(now)
        .bind(keep_id)
        .execute(&mut **tx)
        .await
        .context("attach note to keep")?;
        Ok(())
    }
}

/// Cosine similarity threshold for accepting an alias route. Matches the
/// dedup-UI threshold (`DEDUP_DISTANCE_MAX = 0.15` → similarity ≥ 0.85)
/// so the contract is "if the user would have been offered this pair as
/// a dedup candidate today, the system also routes future extractions
/// onto the existing merge". Increasing it makes routing more
/// conservative; lowering it is dangerous because false routes silently
/// erase fresh entities.
pub const ALIAS_ROUTING_COSINE_MIN: f32 = 0.85;

// ----------------------------- Vector embeddings -----------------------------

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct EmbeddingMeta {
    pub entity_id: String,
    pub model: String,
    pub dim: i64,
    pub embedded_at: i64,
    pub text_hash: String,
}

impl Db {
    /// Persist an embedding for an entity. Upserts both the vec0 row and the
    /// metadata row inside a transaction so they never drift.
    pub async fn upsert_entity_embedding(
        &self,
        entity_id: &str,
        embedding: &[f32],
        model: &str,
        text_hash: &str,
    ) -> Result<()> {
        let bytes = floats_to_bytes(embedding);
        let mut tx = self.pool().begin().await?;

        // vec0 virtual tables don't support UPSERT directly; delete then insert.
        sqlx::query("DELETE FROM entity_vecs WHERE entity_id = ?")
            .bind(entity_id)
            .execute(&mut *tx)
            .await
            .context("delete old vec row")?;

        sqlx::query("INSERT INTO entity_vecs (entity_id, embedding) VALUES (?, ?)")
            .bind(entity_id)
            .bind(&bytes)
            .execute(&mut *tx)
            .await
            .context("insert vec row")?;

        sqlx::query(
            "INSERT INTO entity_embedding_meta (entity_id, model, dim, embedded_at, text_hash) \
             VALUES (?, ?, ?, ?, ?) \
             ON CONFLICT(entity_id) DO UPDATE SET \
               model = excluded.model, \
               dim = excluded.dim, \
               embedded_at = excluded.embedded_at, \
               text_hash = excluded.text_hash",
        )
        .bind(entity_id)
        .bind(model)
        .bind(embedding.len() as i64)
        .bind(now_ms())
        .bind(text_hash)
        .execute(&mut *tx)
        .await
        .context("upsert embedding meta")?;

        tx.commit().await?;
        Ok(())
    }

    /// Drop an entity's embedding (called when entity is deleted or
    /// before re-embedding under a different model/dim).
    pub async fn delete_entity_embedding(&self, entity_id: &str) -> Result<()> {
        let mut tx = self.pool().begin().await?;
        sqlx::query("DELETE FROM entity_vecs WHERE entity_id = ?")
            .bind(entity_id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM entity_embedding_meta WHERE entity_id = ?")
            .bind(entity_id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn get_embedding_meta(&self, entity_id: &str) -> Result<Option<EmbeddingMeta>> {
        let row = sqlx::query_as::<_, EmbeddingMeta>(
            "SELECT entity_id, model, dim, embedded_at, text_hash \
             FROM entity_embedding_meta WHERE entity_id = ?",
        )
        .bind(entity_id)
        .fetch_optional(self.pool())
        .await?;
        Ok(row)
    }

    /// Entities that have never been embedded OR whose embedding model/dim
    /// no longer matches the current provider.
    pub async fn entities_needing_embedding(
        &self,
        current_model: &str,
        current_dim: usize,
        limit: i64,
    ) -> Result<Vec<Entity>> {
        let rows = sqlx::query_as::<_, Entity>(
            "SELECT e.id, e.type, e.name, e.properties, e.confidence, e.source_note_id, \
                    e.created_at, e.updated_at, e.strength, e.last_accessed, e.access_count, e.base_importance \
             FROM entities e \
             LEFT JOIN entity_embedding_meta m ON m.entity_id = e.id \
             WHERE m.entity_id IS NULL OR m.model <> ? OR m.dim <> ? \
             ORDER BY e.updated_at DESC LIMIT ?",
        )
        .bind(current_model)
        .bind(current_dim as i64)
        .bind(limit)
        .fetch_all(self.pool())
        .await?;
        Ok(rows)
    }

    // ----------------------------- Semantic memories -----------------------------

    /// List active (non-superseded) semantic memories, newest-consolidated first.
    pub async fn list_active_themes(
        &self,
        limit: i64,
    ) -> Result<Vec<crate::consolidation::SemanticMemory>> {
        let rows = sqlx::query_as::<_, crate::consolidation::SemanticMemory>(
            "SELECT id, topic, title, summary, evidence, confidence, created_at, \
                    last_consolidated_at, superseded_by, strength, last_accessed, access_count \
             FROM semantic_memories WHERE superseded_by IS NULL \
             ORDER BY last_consolidated_at DESC LIMIT ?",
        )
        .bind(limit)
        .fetch_all(self.pool())
        .await?;
        Ok(rows)
    }

    pub async fn get_theme(
        &self,
        id: &str,
    ) -> Result<Option<crate::consolidation::SemanticMemory>> {
        let row = sqlx::query_as::<_, crate::consolidation::SemanticMemory>(
            "SELECT id, topic, title, summary, evidence, confidence, created_at, \
                    last_consolidated_at, superseded_by, strength, last_accessed, access_count \
             FROM semantic_memories WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(self.pool())
        .await?;
        Ok(row)
    }

    pub async fn touch_theme(&self, id: &str) -> Result<()> {
        sqlx::query(
            "UPDATE semantic_memories SET \
               last_accessed = ?, access_count = access_count + 1, strength = strength + 0.3 \
             WHERE id = ?",
        )
        .bind(now_ms())
        .bind(id)
        .execute(self.pool())
        .await?;
        Ok(())
    }

    pub async fn list_consolidation_runs(
        &self,
        limit: i64,
    ) -> Result<Vec<crate::consolidation::ConsolidationRun>> {
        let rows = sqlx::query_as::<_, crate::consolidation::ConsolidationRun>(
            "SELECT id, started_at, finished_at, trigger, topics_scanned, semantic_created, \
                    semantic_updated, error, status \
             FROM consolidation_runs ORDER BY started_at DESC LIMIT ?",
        )
        .bind(limit)
        .fetch_all(self.pool())
        .await?;
        Ok(rows)
    }

    /// kNN over embeddings. Returns (entity, cosine_distance) tuples sorted
    /// ascending (closer first). Optional entity-type filter is applied after
    /// vec0 returns its top-k.
    pub async fn knn_entities(
        &self,
        query_embedding: &[f32],
        k: i64,
        types: Option<&[String]>,
    ) -> Result<Vec<(Entity, f64)>> {
        let qbytes = floats_to_bytes(query_embedding);
        let rows = sqlx::query(
            "SELECT entity_id, distance FROM entity_vecs \
             WHERE embedding MATCH ? AND k = ? ORDER BY distance",
        )
        .bind(&qbytes)
        .bind(k)
        .fetch_all(self.pool())
        .await?;

        if rows.is_empty() {
            return Ok(vec![]);
        }

        let pairs: Vec<(String, f64)> = rows
            .into_iter()
            .map(|r| {
                let id: String = r.try_get("entity_id").unwrap_or_default();
                let dist: f64 = r.try_get("distance").unwrap_or(f64::INFINITY);
                (id, dist)
            })
            .collect();

        let ids: Vec<String> = pairs.iter().map(|(id, _)| id.clone()).collect();
        let id_placeholders = ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let type_filter = match types {
            Some(ts) if !ts.is_empty() => {
                let p = ts.iter().map(|_| "?").collect::<Vec<_>>().join(",");
                format!("AND type IN ({})", p)
            }
            _ => String::new(),
        };
        // Archived entities never come back via kNN — searchable again
        // only after the user explicitly restores them.
        let sql = format!(
            "SELECT id, type, name, properties, confidence, source_note_id, \
                    created_at, updated_at, strength, last_accessed, access_count, base_importance, is_sticky, archived_at \
             FROM entities WHERE id IN ({}) AND archived_at IS NULL {}",
            id_placeholders, type_filter
        );
        let mut q = sqlx::query_as::<_, Entity>(&sql);
        for id in &ids {
            q = q.bind(id);
        }
        if let Some(ts) = types {
            for t in ts {
                q = q.bind(t);
            }
        }
        let entities = q.fetch_all(self.pool()).await?;

        let dist_map: std::collections::HashMap<String, f64> = pairs.into_iter().collect();
        let mut out: Vec<(Entity, f64)> = entities
            .into_iter()
            .map(|e| {
                let d = dist_map.get(&e.id).copied().unwrap_or(f64::INFINITY);
                (e, d)
            })
            .collect();
        out.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        Ok(out)
    }
}

// ----------------------------- Agents + permissions -----------------------------

impl Db {
    pub async fn list_agents(&self) -> Result<Vec<Agent>> {
        let rows = sqlx::query_as::<_, Agent>(
            "SELECT id, display_name, public_key, added_at, last_seen, revoked_at \
             FROM agents ORDER BY added_at DESC",
        )
        .fetch_all(self.pool())
        .await?;
        Ok(rows)
    }

    /// Hard delete: removes the agent row and its `permissions` grants.
    /// Audit history under this agent_id stays (it's append-only). Used by
    /// the UI "remove agent" button to clean up stale rows like a renamed
    /// MCP client that left a "never connected" stub behind.
    pub async fn delete_agent(&self, id: &str) -> Result<u64> {
        let mut tx = self.pool().begin().await.context("begin delete_agent tx")?;
        sqlx::query("DELETE FROM permissions WHERE agent_id = ?")
            .bind(id)
            .execute(&mut *tx)
            .await
            .context("delete permissions for agent")?;
        let res = sqlx::query("DELETE FROM agents WHERE id = ?")
            .bind(id)
            .execute(&mut *tx)
            .await
            .context("delete agent row")?;
        tx.commit().await.context("commit delete_agent tx")?;
        Ok(res.rows_affected())
    }

    pub async fn upsert_agent(&self, id: &str, display_name: &str) -> Result<()> {
        sqlx::query(
            "INSERT INTO agents (id, display_name, added_at) VALUES (?, ?, ?) \
             ON CONFLICT(id) DO UPDATE SET display_name = excluded.display_name, last_seen = ?",
        )
        .bind(id)
        .bind(display_name)
        .bind(now_ms())
        .bind(now_ms())
        .execute(self.pool())
        .await?;
        Ok(())
    }

    pub async fn touch_agent(&self, id: &str) -> Result<()> {
        sqlx::query("UPDATE agents SET last_seen = ? WHERE id = ?")
            .bind(now_ms())
            .bind(id)
            .execute(self.pool())
            .await?;
        Ok(())
    }

    pub async fn list_permissions(&self, agent_id: &str) -> Result<Vec<Permission>> {
        let rows = sqlx::query_as::<_, Permission>(
            "SELECT agent_id, entity_type, scope, granted_at, expires_at \
             FROM permissions WHERE agent_id = ?",
        )
        .bind(agent_id)
        .fetch_all(self.pool())
        .await?;
        Ok(rows)
    }

    pub async fn grant_permission(
        &self,
        agent_id: &str,
        entity_type: &str,
        scope: &str,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO permissions (agent_id, entity_type, scope, granted_at) VALUES (?, ?, ?, ?) \
             ON CONFLICT(agent_id, entity_type) DO UPDATE SET scope = excluded.scope, granted_at = excluded.granted_at",
        )
        .bind(agent_id)
        .bind(entity_type)
        .bind(scope)
        .bind(now_ms())
        .execute(self.pool())
        .await?;
        Ok(())
    }

    pub async fn revoke_permission(&self, agent_id: &str, entity_type: &str) -> Result<()> {
        sqlx::query("DELETE FROM permissions WHERE agent_id = ? AND entity_type = ?")
            .bind(agent_id)
            .bind(entity_type)
            .execute(self.pool())
            .await?;
        Ok(())
    }

    /// Returns true if the agent has read access for the given entity_type.
    /// '*' permissions match all types.
    pub async fn has_read(&self, agent_id: &str, entity_type: &str) -> Result<bool> {
        let exists: Option<(String,)> = sqlx::query_as(
            "SELECT scope FROM permissions \
             WHERE agent_id = ? AND (entity_type = ? OR entity_type = '*') AND scope = 'read' \
             LIMIT 1",
        )
        .bind(agent_id)
        .bind(entity_type)
        .fetch_optional(self.pool())
        .await?;
        Ok(exists.is_some())
    }
}

// ----------------------------- Audit log -----------------------------

impl Db {
    pub async fn last_audit_hash(&self) -> Result<String> {
        let row: Option<(String,)> =
            sqlx::query_as("SELECT hash FROM audit_log ORDER BY seq DESC LIMIT 1")
                .fetch_optional(self.pool())
                .await?;
        Ok(row.map(|r| r.0).unwrap_or_else(|| "0".repeat(64)))
    }

    pub async fn insert_audit(&self, entry: &AuditEntry) -> Result<()> {
        sqlx::query(
            "INSERT INTO audit_log \
               (id, agent_id, action, tool_name, arguments, result_hash, result_count, \
                timestamp, prev_hash, hash, signature) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&entry.id)
        .bind(&entry.agent_id)
        .bind(&entry.action)
        .bind(&entry.tool_name)
        .bind(&entry.arguments)
        .bind(&entry.result_hash)
        .bind(entry.result_count)
        .bind(entry.timestamp)
        .bind(&entry.prev_hash)
        .bind(&entry.hash)
        .bind(&entry.signature)
        .execute(self.pool())
        .await?;
        Ok(())
    }

    pub async fn list_audit(&self, limit: i64) -> Result<Vec<AuditEntry>> {
        let rows = sqlx::query_as::<_, AuditEntry>(
            "SELECT seq, id, agent_id, action, tool_name, arguments, result_hash, result_count, \
                    timestamp, prev_hash, hash, signature \
             FROM audit_log ORDER BY seq DESC LIMIT ?",
        )
        .bind(limit)
        .fetch_all(self.pool())
        .await?;
        Ok(rows)
    }
}

// ----------------------------- Sync log (Phase 3c) -----------------------------

impl Db {
    #[allow(clippy::too_many_arguments)]
    pub async fn insert_sync_log(
        &self,
        direction: &str,
        bundle_id: &str,
        device_name: Option<&str>,
        device_pubkey: Option<&str>,
        counts: &crate::bundle::BundleCounts,
        checksum: &str,
        signature_ok: Option<bool>,
        conflicts: i64,
        errors_json: Option<&str>,
    ) -> Result<()> {
        let counts_json = serde_json::to_string(counts).unwrap_or_else(|_| "{}".into());
        sqlx::query(
            "INSERT INTO sync_log \
               (id, direction, bundle_id, device_name, device_pubkey, counts, checksum, \
                signature_ok, conflicts, errors, occurred_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(Ulid::new().to_string())
        .bind(direction)
        .bind(bundle_id)
        .bind(device_name)
        .bind(device_pubkey)
        .bind(counts_json)
        .bind(checksum)
        .bind(signature_ok.map(i64::from))
        .bind(conflicts)
        .bind(errors_json)
        .bind(now_ms())
        .execute(self.pool())
        .await?;
        Ok(())
    }
}

// ----------------------------- Audit key -----------------------------

impl Db {
    pub async fn load_audit_key(&self) -> Result<Option<(String, String)>> {
        let row: Option<(String, String)> =
            sqlx::query_as("SELECT public_key, private_key FROM audit_key WHERE id = 1")
                .fetch_optional(self.pool())
                .await?;
        Ok(row)
    }

    pub async fn save_audit_key(&self, public_b64: &str, private_b64: &str) -> Result<()> {
        sqlx::query(
            "INSERT INTO audit_key (id, public_key, private_key, created_at) VALUES (1, ?, ?, ?)",
        )
        .bind(public_b64)
        .bind(private_b64)
        .bind(now_ms())
        .execute(self.pool())
        .await?;
        Ok(())
    }
}

// ----------------------------- Exported blocks (Phase 7 · V6 M2) -----------------------------

#[derive(Debug, Clone)]
pub struct ExportedBlockRow {
    pub id: String,
    pub dest_path: String,
    pub last_written_sha256: String,
    pub last_block_content: String,
    pub entity_ids_json: String,
    pub last_written_at: i64,
}

impl Db {
    pub async fn exported_block_by_path(
        &self,
        path: &std::path::Path,
    ) -> Result<Option<ExportedBlockRow>> {
        let key = path.to_string_lossy().to_string();
        let row: Option<(String, String, String, String, String, i64)> = sqlx::query_as(
            "SELECT id, dest_path, last_written_sha256, last_block_content, \
                    entity_ids_json, last_written_at \
             FROM exported_blocks WHERE dest_path = ?",
        )
        .bind(&key)
        .fetch_optional(self.pool())
        .await
        .context("exported_block_by_path")?;
        Ok(row.map(|r| ExportedBlockRow {
            id: r.0,
            dest_path: r.1,
            last_written_sha256: r.2,
            last_block_content: r.3,
            entity_ids_json: r.4,
            last_written_at: r.5,
        }))
    }

    pub async fn upsert_exported_block(
        &self,
        path: &std::path::Path,
        sha256: &str,
        content: &str,
        entity_ids: &[String],
    ) -> Result<()> {
        let key = path.to_string_lossy().to_string();
        let now = now_ms();
        let ids_json = serde_json::to_string(entity_ids).unwrap_or_else(|_| "[]".to_string());
        sqlx::query(
            "INSERT INTO exported_blocks \
                 (id, dest_path, last_written_sha256, last_block_content, \
                  entity_ids_json, last_written_at) \
             VALUES (?, ?, ?, ?, ?, ?) \
             ON CONFLICT(dest_path) DO UPDATE SET \
                 last_written_sha256 = excluded.last_written_sha256, \
                 last_block_content = excluded.last_block_content, \
                 entity_ids_json = excluded.entity_ids_json, \
                 last_written_at = excluded.last_written_at",
        )
        .bind(Ulid::new().to_string())
        .bind(&key)
        .bind(sha256)
        .bind(content)
        .bind(&ids_json)
        .bind(now)
        .execute(self.pool())
        .await
        .context("upsert exported_block")?;
        Ok(())
    }
}

// ----------------------------- Imported docs (Phase 7 · V6 M1) -----------------------------

#[derive(Debug, Clone)]
pub struct ImportedDocRow {
    pub id: String,
    pub source_path: String,
    pub source_kind: String,
    pub content_sha256: String,
    pub note_id: String,
    pub first_imported_at: i64,
    pub last_imported_at: i64,
}

impl Db {
    pub async fn imported_doc_by_path(
        &self,
        path: &std::path::Path,
    ) -> Result<Option<ImportedDocRow>> {
        let key = path.to_string_lossy().to_string();
        let row: Option<(String, String, String, String, String, i64, i64)> = sqlx::query_as(
            "SELECT id, source_path, source_kind, content_sha256, note_id, \
                    first_imported_at, last_imported_at \
             FROM imported_docs WHERE source_path = ?",
        )
        .bind(&key)
        .fetch_optional(self.pool())
        .await
        .context("imported_doc_by_path")?;
        Ok(row.map(|r| ImportedDocRow {
            id: r.0,
            source_path: r.1,
            source_kind: r.2,
            content_sha256: r.3,
            note_id: r.4,
            first_imported_at: r.5,
            last_imported_at: r.6,
        }))
    }

    pub async fn upsert_imported_doc(
        &self,
        path: &std::path::Path,
        kind: crate::import::SourceKind,
        content_sha256: &str,
        note_id: &str,
    ) -> Result<()> {
        let key = path.to_string_lossy().to_string();
        let now = now_ms();
        let kind_str = kind.as_str();
        // UPSERT: insert fresh on first import, otherwise refresh sha/note_id/last_imported_at.
        // `first_imported_at` stays untouched on update so the dashboard can show "first seen".
        sqlx::query(
            "INSERT INTO imported_docs \
                 (id, source_path, source_kind, content_sha256, note_id, \
                  first_imported_at, last_imported_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT(source_path) DO UPDATE SET \
                 source_kind = excluded.source_kind, \
                 content_sha256 = excluded.content_sha256, \
                 note_id = excluded.note_id, \
                 last_imported_at = excluded.last_imported_at",
        )
        .bind(Ulid::new().to_string())
        .bind(&key)
        .bind(kind_str)
        .bind(content_sha256)
        .bind(note_id)
        .bind(now)
        .bind(now)
        .execute(self.pool())
        .await
        .context("upsert imported_doc")?;
        Ok(())
    }

    /// Cheap existence probe. The importer uses it to tell "still synced"
    /// (note alive) from "user deleted the imported note" (→ Unlinked).
    pub async fn note_exists(&self, note_id: &str) -> Result<bool> {
        let row: Option<(i64,)> = sqlx::query_as("SELECT 1 FROM notes WHERE id = ?")
            .bind(note_id)
            .fetch_optional(self.pool())
            .await
            .context("note_exists")?;
        Ok(row.is_some())
    }

    /// The persistent "do not sync" set: source paths the user has skipped.
    /// Returned as a HashSet for O(1) membership checks during a scan.
    pub async fn list_import_skips(&self) -> Result<std::collections::HashSet<String>> {
        let rows: Vec<(String,)> = sqlx::query_as("SELECT source_path FROM import_skips")
            .fetch_all(self.pool())
            .await
            .context("list_import_skips")?;
        Ok(rows.into_iter().map(|(p,)| p).collect())
    }

    /// Mark a file skipped (persistent) or clear its skip. `source_path` is
    /// stored verbatim (the scanner's own path string) so it matches what
    /// `run_import` compares against — see the resync path note in commands.
    pub async fn set_import_skip(&self, source_path: &str, skipped: bool) -> Result<()> {
        if skipped {
            sqlx::query(
                "INSERT OR IGNORE INTO import_skips (source_path, skipped_at) VALUES (?, ?)",
            )
            .bind(source_path)
            .bind(now_ms())
            .execute(self.pool())
            .await
            .context("add import_skip")?;
        } else {
            sqlx::query("DELETE FROM import_skips WHERE source_path = ?")
                .bind(source_path)
                .execute(self.pool())
                .await
                .context("remove import_skip")?;
        }
        Ok(())
    }
}

// ----------------------------- Maintenance -----------------------------

#[derive(Debug, Clone, serde::Serialize)]
pub struct VacuumOrphanReport {
    pub vec_rows_before: usize,
    pub orphan_vecs_deleted: usize,
    pub orphan_meta_deleted: usize,
    pub bytes_before: u64,
    pub bytes_after: u64,
}

impl Db {
    /// Auto-archive cutoff: an entity becomes eligible for sweeping if it
    /// hasn't been touched in this many milliseconds.
    /// 180 days expressed in ms.
    pub const AUTO_ARCHIVE_STALE_MS: i64 = 180 * 24 * 60 * 60 * 1000;

    /// Flip the soft-archive flag on a single entity. NULL means "active";
    /// any non-null timestamp means "archived at that instant". Setting
    /// archived = false restores the entity by clearing the column.
    pub async fn set_entity_archived(&self, id: &str, archived: bool) -> Result<()> {
        let value: Option<i64> = if archived { Some(now_ms()) } else { None };
        sqlx::query("UPDATE entities SET archived_at = ? WHERE id = ?")
            .bind(value)
            .bind(id)
            .execute(self.pool())
            .await
            .context("set_entity_archived")?;
        Ok(())
    }

    /// List archived entities, newest-archived first.
    pub async fn list_archived_entities(&self, limit: i64) -> Result<Vec<Entity>> {
        let rows = sqlx::query_as::<_, Entity>(
            "SELECT id, type, name, properties, confidence, source_note_id, \
                    created_at, updated_at, strength, last_accessed, access_count, base_importance, is_sticky, archived_at \
             FROM entities WHERE archived_at IS NOT NULL \
             ORDER BY archived_at DESC LIMIT ?",
        )
        .bind(limit)
        .fetch_all(self.pool())
        .await
        .context("list_archived_entities")?;
        Ok(rows)
    }

    /// Sweep stale, low-signal entities into the soft-archive bucket.
    ///
    /// Predicates (all must hold; archived entities are skipped):
    ///   - `archived_at IS NULL`
    ///   - `confidence < 0.3`
    ///   - `access_count = 0`
    ///   - `last_accessed < now - 180 days`
    ///   - no rows in `entity_edits` (user never touched it — hard rule)
    ///
    /// Returns the number of entities archived.
    pub async fn auto_archive_stale_entities(&self) -> Result<usize> {
        let now = now_ms();
        let cutoff = now - Self::AUTO_ARCHIVE_STALE_MS;
        let res = sqlx::query(
            "UPDATE entities SET archived_at = ? \
             WHERE archived_at IS NULL \
               AND confidence < 0.3 \
               AND access_count = 0 \
               AND last_accessed < ? \
               AND NOT EXISTS ( \
                   SELECT 1 FROM entity_edits WHERE entity_id = entities.id \
               )",
        )
        .bind(now)
        .bind(cutoff)
        .execute(self.pool())
        .await
        .context("auto_archive_stale_entities")?;
        Ok(res.rows_affected() as usize)
    }
}

// ----------------------------- Dedup candidates -----------------------------

/// One side of the merge UI: the surviving entity plus the one slated to be
/// dropped, with enough signal context for the user to pick consciously.
#[derive(Debug, Clone, serde::Serialize)]
pub struct DedupCandidate {
    pub keep_id: String,
    pub drop_id: String,
    /// Cosine similarity in [0, 1] (1.0 = identical).
    pub similarity: f32,
    pub keep_entity: Entity,
    pub drop_entity: Entity,
    pub keep_manual_edits: i64,
    pub keep_note_count: i64,
    pub keep_relation_count: i64,
    pub drop_manual_edits: i64,
    pub drop_note_count: i64,
    pub drop_relation_count: i64,
}

/// Outcome of a successful merge — surfaced to the UI for a "moved N items"
/// toast, and to the audit log for verification.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct MergeReport {
    pub relations_migrated: u64,
    pub edits_migrated: u64,
    pub vec_dropped: u64,
    pub alias_dropped: u64,
}

/// Cosine *distance* ceiling for surfacing a dedup pair. 0.15 ≈ similarity
/// ≥ 0.85. Tight enough that random nearby entities don't bubble up; loose
/// enough that the rephrasings the LLM produces ("Mom"/"妈") still cluster.
const DEDUP_DISTANCE_MAX: f32 = 0.15;

/// We pull the top-K nearest neighbours per entity and filter to same-type
/// candidates. K=3 keeps the surface small even with 1k entities.
const DEDUP_NEIGHBOURS_PER_ENTITY: i64 = 3;

/// Lower bound on either entity's confidence — pairs of junk × junk aren't
/// worth surfacing. Per the spec we drop only when *both* sides are below.
const DEDUP_CONFIDENCE_FLOOR: f64 = 0.3;

/// Cosine similarity between two equal-length float vectors. Returns NaN
/// if either is the zero vector.
fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return f32::NAN;
    }
    let mut dot = 0.0f32;
    let mut na = 0.0f32;
    let mut nb = 0.0f32;
    for i in 0..a.len() {
        dot += a[i] * b[i];
        na += a[i] * a[i];
        nb += b[i] * b[i];
    }
    if na == 0.0 || nb == 0.0 {
        return f32::NAN;
    }
    dot / (na.sqrt() * nb.sqrt())
}

/// Decode the little-endian byte format `entity_vecs` uses into a Vec<f32>.
fn bytes_to_floats(bytes: &[u8]) -> Vec<f32> {
    let mut out = Vec::with_capacity(bytes.len() / 4);
    for chunk in bytes.chunks_exact(4) {
        let arr = [chunk[0], chunk[1], chunk[2], chunk[3]];
        out.push(f32::from_le_bytes(arr));
    }
    out
}

/// Ranking heuristic for picking which side of a dedup pair to *keep*.
/// Higher score wins — base_importance scaled by log access plus a small
/// manual-edit lift so user-touched entities almost always survive.
fn dedup_final_score(base_importance: f64, access_count: i64, manual_edits: i64) -> f64 {
    let log_access = (1.0 + (access_count.max(0) as f64)).ln();
    let manual_bonus = 1.0 + (manual_edits.min(3) as f64) * 0.1;
    base_importance * (1.0 + log_access) * manual_bonus
}

impl Db {
    /// Surface up to `limit` dedup candidates ordered by descending similarity.
    /// See `DEDUP_DISTANCE_MAX` for the cosine threshold and the module
    /// comment for the full filter chain.
    pub async fn find_dedup_candidates(&self, limit: usize) -> Result<Vec<DedupCandidate>> {
        if limit == 0 {
            return Ok(vec![]);
        }

        // 1) Pull every vector row + the entity it points to. The 300-entity
        //    target keeps this comfortably small; if the corpus grows we can
        //    switch to a per-entity loop using `embedding MATCH ?`.
        // Archive filter — already-archived entities aren't candidates,
        // since they're not "in use" any more. The retrieval-side change
        // means they're also invisible to search, so surfacing them in
        // dedup would be confusing.
        let vec_rows: Vec<(String, Vec<u8>)> = sqlx::query_as(
            "SELECT v.entity_id, v.embedding FROM entity_vecs v \
             JOIN entities e ON e.id = v.entity_id \
             WHERE e.archived_at IS NULL",
        )
        .fetch_all(self.pool())
        .await
        .context("scan entity_vecs for dedup")?;

        if vec_rows.len() < 2 {
            return Ok(vec![]);
        }

        // Materialise the entity rows once and index by id so we can attach
        // signal context (notes/relations/edits) downstream without a fan-out.
        let entity_ids: Vec<String> = vec_rows.iter().map(|(id, _)| id.clone()).collect();
        let entity_placeholders = entity_ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let entity_sql = format!(
            "SELECT id, type, name, properties, confidence, source_note_id, \
                    created_at, updated_at, strength, last_accessed, access_count, base_importance, is_sticky, archived_at \
             FROM entities WHERE id IN ({})",
            entity_placeholders
        );
        let mut q = sqlx::query_as::<_, Entity>(&entity_sql);
        for id in &entity_ids {
            q = q.bind(id);
        }
        let entities = q
            .fetch_all(self.pool())
            .await
            .context("fetch entities for dedup")?;
        let by_id: std::collections::HashMap<String, Entity> =
            entities.into_iter().map(|e| (e.id.clone(), e)).collect();

        // Pre-decode embeddings.
        let embeddings: std::collections::HashMap<String, Vec<f32>> = vec_rows
            .into_iter()
            .filter_map(|(id, bytes)| {
                let v = bytes_to_floats(&bytes);
                if v.is_empty() {
                    None
                } else {
                    Some((id, v))
                }
            })
            .collect();

        // 2) Cosine-similarity scan to find top-K neighbours per entity.
        //    Same-type filter applied here. Symmetry handled by emitting one
        //    canonical (keep_id, drop_id) ordering below.
        let manual_edits = self
            .manual_edit_counts(&entity_ids)
            .await
            .context("manual_edit_counts for dedup")?;

        // For each entity, compute its full neighbour list, filter, then
        // keep top-K.
        let mut pairs: Vec<(String, String, f32)> = Vec::new();
        let ids_vec: Vec<&String> = embeddings.keys().collect();
        for i in 0..ids_vec.len() {
            let id_a = ids_vec[i];
            let Some(entity_a) = by_id.get(id_a) else {
                continue;
            };
            let Some(emb_a) = embeddings.get(id_a) else {
                continue;
            };
            let mut scored: Vec<(String, f32)> = Vec::new();
            for (j, id_b) in ids_vec.iter().copied().enumerate() {
                if i == j {
                    continue;
                }
                let Some(entity_b) = by_id.get(id_b) else {
                    continue;
                };
                if entity_a.r#type != entity_b.r#type {
                    continue;
                }
                let Some(emb_b) = embeddings.get(id_b) else {
                    continue;
                };
                let sim = cosine_similarity(emb_a, emb_b);
                if !sim.is_finite() {
                    continue;
                }
                let dist = 1.0 - sim;
                if dist > DEDUP_DISTANCE_MAX {
                    continue;
                }
                scored.push((id_b.clone(), sim));
            }
            scored.sort_by(|x, y| y.1.partial_cmp(&x.1).unwrap_or(std::cmp::Ordering::Equal));
            for (id_b, sim) in scored
                .into_iter()
                .take(DEDUP_NEIGHBOURS_PER_ENTITY as usize)
            {
                pairs.push((id_a.clone(), id_b, sim));
            }
        }

        // 3) Drop junk × junk (both below confidence floor).
        // 4) Canonicalise the pair direction by `dedup_final_score`.
        // 5) Deduplicate (keep_id, drop_id) across both orderings.
        let mut seen: std::collections::HashSet<(String, String)> =
            std::collections::HashSet::new();
        let mut canon_pairs: Vec<(String, String, f32)> = Vec::new();
        for (a, b, sim) in pairs {
            let ea = match by_id.get(&a) {
                Some(e) => e,
                None => continue,
            };
            let eb = match by_id.get(&b) {
                Some(e) => e,
                None => continue,
            };
            if ea.confidence < DEDUP_CONFIDENCE_FLOOR && eb.confidence < DEDUP_CONFIDENCE_FLOOR {
                continue;
            }
            let score_a = dedup_final_score(
                ea.base_importance,
                ea.access_count,
                manual_edits.get(&ea.id).copied().unwrap_or(0),
            );
            let score_b = dedup_final_score(
                eb.base_importance,
                eb.access_count,
                manual_edits.get(&eb.id).copied().unwrap_or(0),
            );
            // Higher score keeps; ties broken by id to make the order
            // deterministic regardless of HashMap iteration.
            let (keep, drop) = if score_a > score_b {
                (a.clone(), b.clone())
            } else if score_b > score_a {
                (b.clone(), a.clone())
            } else if a < b {
                (a.clone(), b.clone())
            } else {
                (b.clone(), a.clone())
            };
            if !seen.insert((keep.clone(), drop.clone())) {
                continue;
            }
            canon_pairs.push((keep, drop, sim));
        }

        // 6) Drop pairs the user has already rejected. We check both
        //    orderings — if the canonical pick has flipped since the
        //    rejection (e.g. one side picked up edits), we still honour it.
        if !canon_pairs.is_empty() {
            let mut rejected: std::collections::HashSet<(String, String)> =
                std::collections::HashSet::new();
            let rejected_rows: Vec<(String, String)> =
                sqlx::query_as("SELECT keep_id, drop_id FROM dedup_rejections")
                    .fetch_all(self.pool())
                    .await
                    .context("scan dedup_rejections")?;
            for (k, d) in rejected_rows {
                rejected.insert((k.clone(), d.clone()));
                rejected.insert((d, k));
            }
            canon_pairs.retain(|(k, d, _)| !rejected.contains(&(k.clone(), d.clone())));
        }

        // 7) Sort by similarity (descending) and cap.
        canon_pairs.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
        canon_pairs.truncate(limit);

        if canon_pairs.is_empty() {
            return Ok(vec![]);
        }

        // 8) Hydrate per-pair signal context. We batch the COUNT(*) queries
        //    so a 50-pair payload doesn't fan out into hundreds of round
        //    trips.
        let participant_ids: std::collections::HashSet<String> = canon_pairs
            .iter()
            .flat_map(|(k, d, _)| [k.clone(), d.clone()])
            .collect();
        let participants: Vec<String> = participant_ids.into_iter().collect();
        let note_counts = self.entity_note_counts(&participants).await?;
        let rel_counts = self.entity_relation_counts(&participants).await?;
        let edit_counts = self.manual_edit_counts(&participants).await?;

        let mut out = Vec::with_capacity(canon_pairs.len());
        for (keep_id, drop_id, sim) in canon_pairs {
            let Some(keep_entity) = by_id.get(&keep_id).cloned() else {
                continue;
            };
            let Some(drop_entity) = by_id.get(&drop_id).cloned() else {
                continue;
            };
            out.push(DedupCandidate {
                keep_id: keep_id.clone(),
                drop_id: drop_id.clone(),
                similarity: sim,
                keep_entity,
                drop_entity,
                keep_manual_edits: edit_counts.get(&keep_id).copied().unwrap_or(0),
                keep_note_count: note_counts.get(&keep_id).copied().unwrap_or(0),
                keep_relation_count: rel_counts.get(&keep_id).copied().unwrap_or(0),
                drop_manual_edits: edit_counts.get(&drop_id).copied().unwrap_or(0),
                drop_note_count: note_counts.get(&drop_id).copied().unwrap_or(0),
                drop_relation_count: rel_counts.get(&drop_id).copied().unwrap_or(0),
            });
        }
        Ok(out)
    }

    /// `entity_id -> count of notes this entity references` for the supplied
    /// ids. Returns 0 implicitly when no row is found.
    async fn entity_note_counts(
        &self,
        ids: &[String],
    ) -> Result<std::collections::HashMap<String, i64>> {
        if ids.is_empty() {
            return Ok(std::collections::HashMap::new());
        }
        // We count distinct supporting notes — both the entity's own
        // `source_note_id` *and* the notes that backed any incident
        // relation. That mirrors what the UI surfaces as "evidence".
        let placeholders = ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!(
            "WITH e_ids(id) AS ( \
                 SELECT value FROM ( \
                     SELECT NULL AS value LIMIT 0 \
                 ) UNION ALL \
                 SELECT id FROM (SELECT id FROM entities WHERE id IN ({0})) \
             ) \
             SELECT e.id AS entity_id, COUNT(DISTINCT n.id) AS c \
             FROM entities e \
             LEFT JOIN relations r ON (r.from_entity = e.id OR r.to_entity = e.id) \
             LEFT JOIN notes n ON ( \
                 n.id = e.source_note_id OR n.id = r.source_note_id \
             ) \
             WHERE e.id IN ({0}) \
             GROUP BY e.id",
            placeholders
        );
        let mut q = sqlx::query(&sql);
        for id in ids {
            q = q.bind(id);
        }
        let rows = q
            .fetch_all(self.pool())
            .await
            .context("entity_note_counts")?;
        let mut out = std::collections::HashMap::with_capacity(rows.len());
        for row in rows {
            let id: String = row.try_get("entity_id")?;
            let c: i64 = row.try_get("c")?;
            out.insert(id, c);
        }
        Ok(out)
    }

    /// `entity_id -> count of incident relations` for the supplied ids.
    async fn entity_relation_counts(
        &self,
        ids: &[String],
    ) -> Result<std::collections::HashMap<String, i64>> {
        if ids.is_empty() {
            return Ok(std::collections::HashMap::new());
        }
        let placeholders = ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!(
            "SELECT e.id AS entity_id, COUNT(r.id) AS c \
             FROM entities e \
             LEFT JOIN relations r ON (r.from_entity = e.id OR r.to_entity = e.id) \
             WHERE e.id IN ({}) \
             GROUP BY e.id",
            placeholders
        );
        let mut q = sqlx::query(&sql);
        for id in ids {
            q = q.bind(id);
        }
        let rows = q
            .fetch_all(self.pool())
            .await
            .context("entity_relation_counts")?;
        let mut out = std::collections::HashMap::with_capacity(rows.len());
        for row in rows {
            let id: String = row.try_get("entity_id")?;
            let c: i64 = row.try_get("c")?;
            out.insert(id, c);
        }
        Ok(out)
    }

    /// Merge `drop_id` into `keep_id` inside a single transaction.
    ///
    /// Semantics:
    ///   - Relations whose endpoints land on `drop_id` are rewritten to
    ///     `keep_id` (self-loops are possible but harmless).
    ///   - All `entity_edits` rows are reparented so the history survives.
    ///   - A synthetic `entity_edits` row is appended noting the merge.
    ///   - The drop entity's vec, embedding meta, alias, and main row are
    ///     deleted.
    ///
    /// vec0 limitation note: sqlite-vec's INSERT inside a transaction is
    /// committed even when the surrounding tx rolls back. DELETE on a
    /// non-existent row is harmless. Worst case on rollback: the drop's
    /// vec row may stick around without a corresponding entity, which the
    /// retrieval layer already tolerates via the JOIN-with-entities filter
    /// + the existing `vacuum_orphan_entity_vecs` cleanup.
    pub async fn merge_entities(&self, keep_id: &str, drop_id: &str) -> Result<MergeReport> {
        if keep_id == drop_id {
            anyhow::bail!("cannot merge an entity into itself");
        }
        let mut tx = self.pool().begin().await.context("begin merge tx")?;

        // Sanity-check both entities exist before we touch anything else.
        // We also grab the drop's name + the drop's embedding so we can
        // persist an alias row before the drop entity goes away.
        let keep_exists: Option<(String,)> = sqlx::query_as("SELECT id FROM entities WHERE id = ?")
            .bind(keep_id)
            .fetch_optional(&mut *tx)
            .await
            .context("check keep entity")?;
        if keep_exists.is_none() {
            anyhow::bail!("keep entity not found: {}", keep_id);
        }
        let drop_row: Option<(String,)> = sqlx::query_as("SELECT name FROM entities WHERE id = ?")
            .bind(drop_id)
            .fetch_optional(&mut *tx)
            .await
            .context("check drop entity")?;
        let Some((drop_name,)) = drop_row else {
            anyhow::bail!("drop entity not found: {}", drop_id);
        };

        let rel_from = sqlx::query("UPDATE relations SET from_entity = ? WHERE from_entity = ?")
            .bind(keep_id)
            .bind(drop_id)
            .execute(&mut *tx)
            .await
            .context("migrate from_entity")?;
        let rel_to = sqlx::query("UPDATE relations SET to_entity = ? WHERE to_entity = ?")
            .bind(keep_id)
            .bind(drop_id)
            .execute(&mut *tx)
            .await
            .context("migrate to_entity")?;
        let relations_migrated = rel_from.rows_affected() + rel_to.rows_affected();

        let edits = sqlx::query("UPDATE entity_edits SET entity_id = ? WHERE entity_id = ?")
            .bind(keep_id)
            .bind(drop_id)
            .execute(&mut *tx)
            .await
            .context("migrate entity_edits")?;
        let edits_migrated = edits.rows_affected();

        // Synthetic "merged from <drop_id>" audit row on the keep entity.
        // Written *inside* the transaction so it lives or dies with the
        // rest of the merge.
        let synth_id = Ulid::new().to_string();
        let before_trunc = truncate_for_audit(drop_id);
        let after_trunc = truncate_for_audit(keep_id);
        let now = now_ms();
        sqlx::query(
            "INSERT INTO entity_edits \
                (id, entity_id, action, field_changed, value_before, value_after, \
                 edited_by, edited_at) \
             VALUES (?, ?, 'update', 'merged_from', ?, ?, 'user', ?)",
        )
        .bind(&synth_id)
        .bind(keep_id)
        .bind(&before_trunc)
        .bind(&after_trunc)
        .bind(now)
        .execute(&mut *tx)
        .await
        .context("insert merge history row")?;

        // Snapshot the drop's embedding + model BEFORE we delete it. The
        // alias row records what the drop *was* — model id is critical so
        // a future re-extraction under a different embedder doesn't blindly
        // route a different model's vectors onto this keep.
        let drop_vec_bytes: Option<(Vec<u8>,)> =
            sqlx::query_as("SELECT embedding FROM entity_vecs WHERE entity_id = ?")
                .bind(drop_id)
                .fetch_optional(&mut *tx)
                .await
                .context("read drop embedding bytes")?;
        let drop_model_id: Option<(String,)> =
            sqlx::query_as("SELECT model FROM entity_embedding_meta WHERE entity_id = ?")
                .bind(drop_id)
                .fetch_optional(&mut *tx)
                .await
                .context("read drop embedding model")?;

        let embedding_hash = drop_vec_bytes.as_ref().map(|(b,)| embedding_bytes_hash(b));
        let embedding_model_id = drop_model_id.map(|(m,)| m);
        let name_lower = drop_name.to_lowercase();

        // Persist the alias row. `entity_id` is the surviving keep — that
        // keeps the legacy FK CASCADE behaviour intact for direct-delete
        // paths. `keep_id` is the new logical column the routing layer
        // queries by; they hold the same value.
        let alias_id = Ulid::new().to_string();
        sqlx::query(
            "INSERT INTO entity_aliases \
                (id, entity_id, alias, source, created_at, \
                 keep_id, name_lower, embedding_hash, embedding_model_id, routed_count) \
             VALUES (?, ?, ?, 'merge', ?, ?, ?, ?, ?, 0)",
        )
        .bind(&alias_id)
        .bind(keep_id)
        .bind(&name_lower)
        .bind(now)
        .bind(keep_id)
        .bind(&name_lower)
        .bind(embedding_hash.as_deref())
        .bind(embedding_model_id.as_deref())
        .execute(&mut *tx)
        .await
        .context("insert alias row")?;

        // The keep is now the canonical entity for this merge — pin it so
        // import-watcher churn can't cascade it away.
        sqlx::query("UPDATE entities SET is_sticky = 1 WHERE id = ?")
            .bind(keep_id)
            .execute(&mut *tx)
            .await
            .context("set is_sticky on keep")?;

        let vec_res = sqlx::query("DELETE FROM entity_vecs WHERE entity_id = ?")
            .bind(drop_id)
            .execute(&mut *tx)
            .await
            .context("delete drop vec")?;
        let vec_dropped = vec_res.rows_affected();

        sqlx::query("DELETE FROM entity_embedding_meta WHERE entity_id = ?")
            .bind(drop_id)
            .execute(&mut *tx)
            .await
            .context("delete drop embedding meta")?;

        // Drop any aliases that had been pointing AT the drop entity from
        // an earlier merge. Those routes can't survive — the keep_id they
        // referenced is about to disappear.
        let alias_res = sqlx::query("DELETE FROM entity_aliases WHERE keep_id = ?")
            .bind(drop_id)
            .execute(&mut *tx)
            .await
            .context("delete aliases pointing at drop")?;
        let alias_dropped = alias_res.rows_affected();

        sqlx::query("DELETE FROM entities WHERE id = ?")
            .bind(drop_id)
            .execute(&mut *tx)
            .await
            .context("delete drop entity")?;

        tx.commit().await.context("commit merge tx")?;
        Ok(MergeReport {
            relations_migrated,
            edits_migrated,
            vec_dropped,
            alias_dropped,
        })
    }

    /// Persist a user's "these are NOT duplicates" decision so the pair
    /// stops showing up in future candidate lists.
    pub async fn reject_dedup_pair(&self, keep_id: &str, drop_id: &str) -> Result<()> {
        let id = Ulid::new().to_string();
        let now = now_ms();
        // ON CONFLICT just refreshes the timestamp; the UNIQUE(keep, drop)
        // index gives us idempotency.
        sqlx::query(
            "INSERT INTO dedup_rejections (id, keep_id, drop_id, rejected_at) \
             VALUES (?, ?, ?, ?) \
             ON CONFLICT(keep_id, drop_id) DO UPDATE SET rejected_at = excluded.rejected_at",
        )
        .bind(&id)
        .bind(keep_id)
        .bind(drop_id)
        .bind(now)
        .execute(self.pool())
        .await
        .context("insert dedup_rejection")?;
        Ok(())
    }
}

// ----------------------------- Ranked listing -----------------------------

/// Half-life in days for the `list_entities_ranked` recency factor. ~6 weeks
/// — long enough that an entity from a month ago doesn't sink to the bottom,
/// short enough that year-old crud drops off.
const RANKED_RECENCY_HALF_LIFE_DAYS: f64 = 60.0;

const MS_PER_DAY: f64 = 24.0 * 60.0 * 60.0 * 1000.0;

/// One row of the dashboard's "entities ranked by signal" grid. Theme
/// metadata is included so the frontend can group entities under the
/// semantic memory they currently belong to (if any).
#[derive(Debug, Clone, serde::Serialize)]
pub struct RankedEntity {
    pub entity: Entity,
    pub final_score: f32,
    pub manual_edits: i64,
    pub theme_id: Option<String>,
    pub theme_title: Option<String>,
}

impl Db {
    /// Rank active entities by signal strength. Score formula:
    ///   `base_importance * (1 + ln(1 + access_count))
    ///       * exp(-days_since_last_accessed / 60)
    ///       * (1 + min(manual_edits, 3) * 0.1)`
    ///
    /// Excludes `archived_at IS NOT NULL`. Theme lookup is best-effort
    /// (semantic_memories store evidence as a JSON array of entity ids).
    pub async fn list_entities_ranked(&self, limit: i64) -> Result<Vec<RankedEntity>> {
        if limit <= 0 {
            return Ok(vec![]);
        }
        // Pull enough headroom in case some entities get filtered out — we
        // request `limit * 4` from the DB but cap the final result.
        let fetch_limit = (limit * 4).max(limit);
        let entities: Vec<Entity> = sqlx::query_as::<_, Entity>(
            "SELECT id, type, name, properties, confidence, source_note_id, \
                    created_at, updated_at, strength, last_accessed, access_count, base_importance, is_sticky, archived_at \
             FROM entities WHERE archived_at IS NULL \
             ORDER BY updated_at DESC LIMIT ?",
        )
        .bind(fetch_limit)
        .fetch_all(self.pool())
        .await
        .context("list_entities_ranked: fetch entities")?;

        if entities.is_empty() {
            return Ok(vec![]);
        }

        let ids: Vec<String> = entities.iter().map(|e| e.id.clone()).collect();
        let edit_counts = self
            .manual_edit_counts(&ids)
            .await
            .context("list_entities_ranked: edit counts")?;

        // Build entity_id -> (theme_id, theme_title) map by scanning the
        // active semantic_memories table once. `evidence` is a JSON array
        // of entity ids; we just parse + bucket. This is O(themes × ids)
        // but at human scale (≤ a couple hundred themes) it's a single
        // millisecond-level loop.
        let themes = self.list_active_themes(1024).await.unwrap_or_default();
        let mut by_entity: std::collections::HashMap<String, (String, String)> =
            std::collections::HashMap::new();
        for theme in &themes {
            let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&theme.evidence) else {
                continue;
            };
            let Some(arr) = parsed.as_array() else {
                continue;
            };
            for v in arr {
                let Some(eid) = v.as_str() else { continue };
                by_entity
                    .entry(eid.to_string())
                    .or_insert_with(|| (theme.id.clone(), theme.title.clone()));
            }
        }

        let now = now_ms();
        let mut out: Vec<RankedEntity> = entities
            .into_iter()
            .map(|e| {
                let manual_edits = edit_counts.get(&e.id).copied().unwrap_or(0);
                let log_access = (1.0 + (e.access_count.max(0) as f64)).ln();
                let elapsed_days = ((now - e.last_accessed).max(0) as f64) / MS_PER_DAY;
                let recency =
                    (-elapsed_days * std::f64::consts::LN_2 / RANKED_RECENCY_HALF_LIFE_DAYS).exp();
                let manual_bonus = 1.0 + (manual_edits.min(3) as f64) * 0.1;
                let score = e.base_importance * (1.0 + log_access) * recency * manual_bonus;
                let (theme_id, theme_title) = by_entity
                    .get(&e.id)
                    .map(|(id, title)| (Some(id.clone()), Some(title.clone())))
                    .unwrap_or((None, None));
                RankedEntity {
                    entity: e,
                    final_score: score as f32,
                    manual_edits,
                    theme_id,
                    theme_title,
                }
            })
            .collect();
        out.sort_by(|a, b| {
            b.final_score
                .partial_cmp(&a.final_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        out.truncate(limit as usize);
        Ok(out)
    }
}

// ----------------------------- Vacuum / orphan cleanup -----------------------------

impl Db {
    /// Remove `entity_vecs` rows whose `entity_id` no longer points to a
    /// live `entities` row, and the matching `entity_embedding_meta` rows.
    /// Then run `VACUUM` so sqlite actually shrinks the file (vec0 stores
    /// chunks in auxiliary tables that don't autoshrink on row delete).
    ///
    /// Built for the LoCoMo cleanup case where a bulk DELETE via the bare
    /// sqlite3 CLI couldn't touch vec0 (extension not loaded), leaving
    /// orphan vectors behind.
    pub async fn vacuum_orphan_entity_vecs(
        &self,
        db_path: &std::path::Path,
    ) -> Result<VacuumOrphanReport> {
        let bytes_before = std::fs::metadata(db_path).map(|m| m.len()).unwrap_or(0);

        // vec0 doesn't support `NOT IN (subquery)` reliably; diff in Rust.
        let vec_ids: Vec<(String,)> = sqlx::query_as("SELECT entity_id FROM entity_vecs")
            .fetch_all(self.pool())
            .await
            .context("scan entity_vecs")?;
        let entity_ids: Vec<(String,)> = sqlx::query_as("SELECT id FROM entities")
            .fetch_all(self.pool())
            .await
            .context("scan entities")?;
        let live: std::collections::HashSet<String> =
            entity_ids.into_iter().map(|(s,)| s).collect();

        let vec_rows_before = vec_ids.len();
        let mut orphan_vecs_deleted = 0usize;
        let mut orphan_meta_deleted = 0usize;

        for (eid,) in vec_ids {
            if live.contains(&eid) {
                continue;
            }
            let r = sqlx::query("DELETE FROM entity_vecs WHERE entity_id = ?")
                .bind(&eid)
                .execute(self.pool())
                .await
                .context("delete orphan vec")?;
            orphan_vecs_deleted += r.rows_affected() as usize;
            let r2 = sqlx::query("DELETE FROM entity_embedding_meta WHERE entity_id = ?")
                .bind(&eid)
                .execute(self.pool())
                .await
                .context("delete orphan meta")?;
            orphan_meta_deleted += r2.rows_affected() as usize;
        }

        // VACUUM can't run inside a transaction; sqlx executes each query
        // on its own connection, which is fine.
        sqlx::query("VACUUM")
            .execute(self.pool())
            .await
            .context("vacuum")?;

        let bytes_after = std::fs::metadata(db_path).map(|m| m.len()).unwrap_or(0);

        Ok(VacuumOrphanReport {
            vec_rows_before,
            orphan_vecs_deleted,
            orphan_meta_deleted,
            bytes_before,
            bytes_after,
        })
    }
}
