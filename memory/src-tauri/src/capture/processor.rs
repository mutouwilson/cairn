//! Shared post-capture pipeline.
//!
//! Both `commands::capture_note` (user-typed) and the Phase 5 capture workers
//! (email / calendar) end up with a `notes` row in state `pending`. This
//! module is the single function they both call to drive that note through
//! extraction → entity save → embedding → audit, with consistent error
//! handling and processing-status book-keeping.

use crate::audit::AuditLogger;
use crate::db::Db;
use crate::embed::EmbeddingProvider;
use crate::extract::ExtractionProvider;
use anyhow::Result;
use serde_json::json;
use std::sync::Arc;

#[derive(Clone)]
pub struct ProcessingContext {
    pub db: Db,
    pub extractor: Arc<dyn ExtractionProvider>,
    pub embedder: Arc<dyn EmbeddingProvider>,
    pub audit: AuditLogger,
    pub embedding_model_id: String,
}

#[derive(Debug, Default)]
pub struct ProcessOutcome {
    pub entity_ids: Vec<String>,
    pub embedded: usize,
    pub relation_count: usize,
}

/// Drive a single note through the full pipeline.
///
/// Failure model:
///   * **LLM extraction error** (model returned junk JSON, hit a 429, hit a
///     network blip…) → the note is kept verbatim, status flips to `done`,
///     and we drop a `extract/raw_fallback` audit row so the failure is
///     traceable. The text is the user's deliberate save — it has value
///     even with no typed structure.
///   * **DB save error** during `save_extracted` → that's a real bug; the
///     note is marked `failed` with the error.
pub async fn process_note(
    ctx: &ProcessingContext,
    note_id: &str,
    text: &str,
) -> Result<ProcessOutcome> {
    let extracted = match ctx.extractor.extract(text).await {
        Ok(e) => e,
        Err(e) => {
            tracing::warn!(note_id, error = %e, "extraction failed — keeping note as raw");
            let _ = ctx.db.mark_note_processed(note_id, None).await;
            let _ = ctx
                .audit
                .log(
                    "system",
                    "extract/raw_fallback",
                    Some("extract"),
                    Some(&format!("note={note_id}")),
                    &json!({ "note_id": note_id, "error": e.to_string() }),
                    Some(0),
                )
                .await;
            return Ok(ProcessOutcome::default());
        }
    };

    // Alias-routed save: every candidate that matches a recorded
    // `(name_lower, embedding_model_id)` alias *and* clears the cosine
    // threshold against the keep's current embedding is folded into the
    // keep instead of inserting a fresh row. The routing decisions feed
    // an `alias_routed` audit row each so the user can see exactly which
    // re-extracted entities collapsed back onto a manual merge.
    let routing_ctx = crate::db::queries::RoutingEmbed {
        embedder: &ctx.embedder,
        model_id: &ctx.embedding_model_id,
    };
    let (entity_ids, routed) = match ctx
        .db
        .save_extracted_with_router(note_id, &extracted, Some(routing_ctx))
        .await
    {
        Ok(pair) => pair,
        Err(e) => {
            tracing::warn!(note_id, error = %e, "save_extracted failed — keeping note as raw");
            let _ = ctx.db.mark_note_processed(note_id, None).await;
            let _ = ctx
                .audit
                .log(
                    "system",
                    "extract/raw_fallback",
                    Some("extract"),
                    Some(&format!("note={note_id}")),
                    &json!({ "note_id": note_id, "error": e.to_string(), "stage": "save_extracted" }),
                    Some(0),
                )
                .await;
            return Ok(ProcessOutcome::default());
        }
    };

    let _ = ctx.db.mark_note_processed(note_id, None).await;

    let embedded = crate::embed_pipeline::embed_entities(
        &ctx.db,
        &ctx.embedder,
        &ctx.embedding_model_id,
        &entity_ids,
    )
    .await
    .unwrap_or_else(|e| {
        tracing::warn!(
            ?e,
            "embed pipeline failed; vector search degrades for these entities"
        );
        0
    });

    for (extracted_name, keep_id) in &routed.routed {
        let _ = ctx
            .audit
            .log(
                "system",
                "alias_routed",
                Some("alias_routed"),
                Some(&format!("note={note_id} keep={keep_id}")),
                &json!({
                    "note_id": note_id,
                    "keep_id": keep_id,
                    "extracted_name": extracted_name,
                }),
                Some(1),
            )
            .await;
    }

    let _ = ctx
        .audit
        .log(
            "system",
            "extract/ok",
            Some("extract"),
            Some(&format!("note={note_id}")),
            &json!({
                "note_id": note_id,
                "entities": entity_ids.len(),
                "embedded": embedded,
                "relations": extracted.relations.len(),
                "alias_routed": routed.routed.len(),
            }),
            Some(entity_ids.len() as i64),
        )
        .await;

    Ok(ProcessOutcome {
        entity_ids,
        embedded,
        relation_count: extracted.relations.len(),
    })
}
