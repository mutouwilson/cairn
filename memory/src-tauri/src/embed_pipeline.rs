//! Orchestrates embedding of entities after extraction. Lives outside `db/`
//! because it spans two services (DB + embedder).
//!
//! The pipeline is idempotent and incremental:
//!   - Skip entities whose `(text_hash, model)` is unchanged.
//!   - Re-embed when the model id changes (handled by
//!     `Db::entities_needing_embedding`).
//!
//! Batches embedding calls per pipeline invocation to amortise API latency.

use crate::db::queries::{embedding_text_hash, entity_embed_text};
use crate::db::Db;
use crate::embed::EmbeddingProvider;
use anyhow::Result;
use std::sync::Arc;

/// Embed (or re-embed) the entities referenced by `entity_ids`.
/// Returns the number of vectors actually written. Unchanged entities are
/// skipped without an API call.
pub async fn embed_entities(
    db: &Db,
    embedder: &Arc<dyn EmbeddingProvider>,
    model_id: &str,
    entity_ids: &[String],
) -> Result<usize> {
    if entity_ids.is_empty() {
        return Ok(0);
    }

    let mut to_embed_texts: Vec<String> = Vec::new();
    let mut to_embed_meta: Vec<(String, String)> = Vec::new(); // (entity_id, text_hash)

    for id in entity_ids {
        let Some(entity) = db.get_entity(id).await? else {
            continue;
        };
        let text = entity_embed_text(&entity.r#type, &entity.name, &entity.properties);
        let hash = embedding_text_hash(&text);
        match db.get_embedding_meta(&entity.id).await? {
            Some(meta)
                if meta.text_hash == hash
                    && meta.model == model_id
                    && (meta.dim as usize) == embedder.dim() =>
            {
                // up-to-date, skip
                continue;
            }
            _ => {
                to_embed_texts.push(text);
                to_embed_meta.push((entity.id, hash));
            }
        }
    }

    if to_embed_texts.is_empty() {
        return Ok(0);
    }

    let vectors = embedder.embed_batch(&to_embed_texts).await?;
    if vectors.len() != to_embed_meta.len() {
        anyhow::bail!(
            "embedding count mismatch: requested {} got {}",
            to_embed_meta.len(),
            vectors.len()
        );
    }

    for ((id, hash), vec) in to_embed_meta.iter().zip(vectors.iter()) {
        db.upsert_entity_embedding(id, vec, model_id, hash).await?;
    }
    Ok(to_embed_meta.len())
}

/// Drain the backlog of entities that have never been embedded or were embedded
/// under a different model/dim. Capped at `batch` per invocation.
pub async fn embed_pending(
    db: &Db,
    embedder: &Arc<dyn EmbeddingProvider>,
    model_id: &str,
    batch: i64,
) -> Result<usize> {
    let pending = db
        .entities_needing_embedding(model_id, embedder.dim(), batch)
        .await?;
    if pending.is_empty() {
        return Ok(0);
    }
    let ids: Vec<String> = pending.into_iter().map(|e| e.id).collect();
    embed_entities(db, embedder, model_id, &ids).await
}
