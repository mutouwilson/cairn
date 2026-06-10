//! Tauri IPC commands. The Next.js UI calls these via `invoke('name', { args })`.
//!
//! Convention:
//!   - All commands are `async` and return `Result<T, String>` so errors surface
//!     to the renderer as plain strings.
//!   - Long-running work (LLM extraction) is dispatched as a background task
//!     and notifies the renderer via the `note_processed` event.

use crate::mcp_bridge::McpBridgeStatus;
use crate::providers::{self, ProviderConfig, ProviderFamily, ProviderPreset, ProvidersConfig};
use crate::schema::{Agent, AuditEntry, Entity, Note, Permission, Relation};
use crate::selection_popover::{SelectionEvent, SelectionStatus};
use crate::AppState;
use serde::Serialize;
use serde_json::json;
use tauri::{Emitter, State, Window};

type CmdResult<T> = Result<T, String>;

fn err<E: ToString>(e: E) -> String {
    e.to_string()
}

// -------------------- Capture --------------------

#[derive(Serialize)]
pub struct CaptureResult {
    pub note_id: String,
}

#[tauri::command]
pub async fn capture_note(
    text: String,
    source: Option<String>,
    window: Window,
    state: State<'_, AppState>,
) -> CmdResult<CaptureResult> {
    let src = source.unwrap_or_else(|| "manual".into());
    let note_id = state.db.insert_note(&text, &src).await.map_err(err)?;
    let _ = window.emit(
        "note_captured",
        json!({ "note_id": note_id, "text": text, "source": src }),
    );

    // Snapshot the *current* live providers so a config change mid-extract
    // doesn't yank handles out from under us. Subsequent captures rebuild.
    let db = state.db.clone();
    let extractor = state.providers.extractor();
    let audit = state.audit.clone();
    let embedder = state.providers.embedder();
    let embedding_model_id = state.providers.embedding_model_id();
    let note_id_for_task = note_id.clone();
    let text_for_task = text.clone();
    let window_for_task = window.clone();

    tauri::async_runtime::spawn(async move {
        tracing::info!(note_id = %note_id_for_task, "extracting");
        match extractor.extract(&text_for_task).await {
            Ok(extracted) => {
                let routing_ctx = crate::db::queries::RoutingEmbed {
                    embedder: &embedder,
                    model_id: &embedding_model_id,
                };
                match db
                    .save_extracted_with_router(&note_id_for_task, &extracted, Some(routing_ctx))
                    .await
                {
                    Ok((entity_ids, routed)) => {
                        let _ = db.mark_note_processed(&note_id_for_task, None).await;

                        // Embed entities (best effort — falling back to BM25 is fine).
                        let embed_count = match crate::embed_pipeline::embed_entities(
                            &db,
                            &embedder,
                            &embedding_model_id,
                            &entity_ids,
                        )
                        .await
                        {
                            Ok(n) => n,
                            Err(e) => {
                                tracing::warn!(?e, "embed pipeline failed");
                                0
                            }
                        };

                        for (extracted_name, keep_id) in &routed.routed {
                            let _ = audit
                                .log(
                                    "system",
                                    "alias_routed",
                                    Some("alias_routed"),
                                    Some(&format!("note={} keep={}", note_id_for_task, keep_id)),
                                    &json!({
                                        "note_id": note_id_for_task,
                                        "keep_id": keep_id,
                                        "extracted_name": extracted_name,
                                    }),
                                    Some(1),
                                )
                                .await;
                        }

                        let _ = audit
                            .log(
                                "system",
                                "extract/ok",
                                Some("extract"),
                                Some(&format!("note={}", note_id_for_task)),
                                &json!({
                                    "note_id": note_id_for_task,
                                    "entities": entity_ids.len(),
                                    "embedded": embed_count,
                                    "relations": extracted.relations.len(),
                                    "alias_routed": routed.routed.len(),
                                }),
                                Some(entity_ids.len() as i64),
                            )
                            .await;
                        let _ = window_for_task.emit(
                            "note_processed",
                            json!({
                                "note_id": note_id_for_task,
                                "entity_ids": entity_ids,
                                "entity_count": extracted.entities.len(),
                                "embedded_count": embed_count,
                                "relation_count": extracted.relations.len(),
                                "alias_routed": routed.routed.len(),
                            }),
                        );
                    }
                    Err(e) => {
                        // Entities couldn't be persisted (e.g. SQLite WAL writer
                        // contention with cairn-mcp). The raw note row is already
                        // saved by `db.insert_note`; treat this as the raw
                        // fallback path so the user keeps their text and can
                        // re-process later via `reprocess_note` if they want
                        // typed entities.
                        tracing::warn!(?e, "save extracted failed — keeping note as raw");
                        let _ = db.mark_note_processed(&note_id_for_task, None).await;
                        let _ = audit
                        .log(
                            "system",
                            "extract/raw_fallback",
                            Some("extract"),
                            Some(&format!("note={}", note_id_for_task)),
                            &json!({ "note_id": note_id_for_task, "error": e.to_string(), "stage": "save_extracted" }),
                            Some(0),
                        )
                        .await;
                        let _ = window_for_task.emit(
                            "note_processed",
                            json!({
                                "note_id": note_id_for_task,
                                "entity_ids": Vec::<String>::new(),
                                "entity_count": 0,
                                "raw_fallback": true,
                                "stage": "save_extracted",
                            }),
                        );
                    }
                }
            }
            Err(e) => {
                // LLM couldn't parse the note into typed entities — the user
                // still saved this text intentionally, so we keep it as a
                // raw, BM25-searchable memory rather than marking it failed.
                tracing::warn!(?e, "extraction returned no structure; keeping as raw note");
                let _ = db.mark_note_processed(&note_id_for_task, None).await;
                let _ = audit
                    .log(
                        "system",
                        "extract/raw_fallback",
                        Some("extract"),
                        Some(&format!("note={}", note_id_for_task)),
                        &json!({ "note_id": note_id_for_task, "error": e.to_string() }),
                        Some(0),
                    )
                    .await;
                let _ = window_for_task.emit(
                    "note_processed",
                    json!({
                        "note_id": note_id_for_task,
                        "entity_ids": Vec::<String>::new(),
                        "entity_count": 0,
                        "raw_fallback": true,
                    }),
                );
            }
        }
    });

    Ok(CaptureResult { note_id })
}

/// Trigger a backfill of all entities whose embedding model has changed or
/// who have never been embedded. Useful after switching providers.
#[tauri::command]
pub async fn reembed_pending(state: State<'_, AppState>) -> CmdResult<i64> {
    let embedder = state.providers.embedder();
    let model_id = state.providers.embedding_model_id();
    let n = crate::embed_pipeline::embed_pending(&state.db, &embedder, &model_id, 256)
        .await
        .map_err(err)?;
    Ok(n as i64)
}

#[tauri::command]
pub async fn reprocess_note(
    note_id: String,
    window: Window,
    state: State<'_, AppState>,
) -> CmdResult<()> {
    let row: Option<(String,)> = sqlx::query_as("SELECT text FROM notes WHERE id = ?")
        .bind(&note_id)
        .fetch_optional(state.db.pool())
        .await
        .map_err(err)?;
    let Some((text,)) = row else {
        return Err("note not found".into());
    };

    reprocess_note_inner(note_id, text, "reprocess", &window, &state).await
}

/// Shared re-extraction spawn used by `reprocess_note` and the edit-driven
/// `update_note` flow. Cleans up previous extraction first, then dispatches
/// the LLM extractor on a background task. `flow_label` is the audit-log /
/// event source tag — `"reprocess"` or `"edit"`.
async fn reprocess_note_inner(
    note_id: String,
    text: String,
    flow_label: &'static str,
    window: &Window,
    state: &State<'_, AppState>,
) -> CmdResult<()> {
    // Roll back the entities + relations attached to this note so the new
    // extraction starts clean. The note row itself is preserved. Sticky
    // entities (manual creation / edits / merge-keeps) survive — `report`
    // carries their ids so we can audit-log the survivors below.
    let report = state
        .db
        .cleanup_extraction_for_note(&note_id)
        .await
        .map_err(err)?;
    tracing::info!(
        note_id,
        entities_deleted = report.entities_deleted,
        relations_deleted = report.relations_deleted,
        sticky_kept = report.sticky_kept.len(),
        flow_label,
        "cleared previous extraction"
    );
    for sticky_id in &report.sticky_kept {
        let _ = state
            .audit
            .log(
                "system",
                "sticky_kept",
                Some("sticky_kept"),
                Some(&format!("entity={} note={}", sticky_id, note_id)),
                &json!({
                    "entity_id": sticky_id,
                    "note_id": note_id,
                    "flow": flow_label,
                }),
                Some(1),
            )
            .await;
    }

    // Notify the UI that this note flipped back to pending so the timeline
    // shows the spinner while extraction runs.
    let _ = window.emit(
        "note_captured",
        json!({ "note_id": note_id, "text": text, "source": flow_label }),
    );

    let db = state.db.clone();
    let extractor = state.providers.extractor();
    let audit = state.audit.clone();
    let embedder = state.providers.embedder();
    let embedding_model_id = state.providers.embedding_model_id();
    let note_id_for_task = note_id.clone();
    let text_for_task = text;
    let window_for_task = window.clone();

    tauri::async_runtime::spawn(async move {
        tracing::info!(note_id = %note_id_for_task, flow_label, "re-extracting");
        match extractor.extract(&text_for_task).await {
            Ok(extracted) => {
                let routing_ctx = crate::db::queries::RoutingEmbed {
                    embedder: &embedder,
                    model_id: &embedding_model_id,
                };
                match db
                    .save_extracted_with_router(&note_id_for_task, &extracted, Some(routing_ctx))
                    .await
                {
                    Ok((entity_ids, routed)) => {
                        let _ = db.mark_note_processed(&note_id_for_task, None).await;
                        let embed_count = crate::embed_pipeline::embed_entities(
                            &db,
                            &embedder,
                            &embedding_model_id,
                            &entity_ids,
                        )
                        .await
                        .unwrap_or_else(|e| {
                            tracing::warn!(?e, "reprocess embed pipeline failed");
                            0
                        });
                        for (extracted_name, keep_id) in &routed.routed {
                            let _ = audit
                                .log(
                                    "system",
                                    "alias_routed",
                                    Some("alias_routed"),
                                    Some(&format!("note={} keep={}", note_id_for_task, keep_id)),
                                    &json!({
                                        "note_id": note_id_for_task,
                                        "keep_id": keep_id,
                                        "extracted_name": extracted_name,
                                        "flow": flow_label,
                                    }),
                                    Some(1),
                                )
                                .await;
                        }
                        let _ = audit
                            .log(
                                "system",
                                "extract/ok",
                                Some("extract"),
                                Some(&format!("note={} {}=1", note_id_for_task, flow_label)),
                                &json!({
                                    "note_id": note_id_for_task,
                                    "entities": entity_ids.len(),
                                    "embedded": embed_count,
                                    "relations": extracted.relations.len(),
                                    "alias_routed": routed.routed.len(),
                                    "flow": flow_label,
                                }),
                                Some(entity_ids.len() as i64),
                            )
                            .await;
                        let _ = window_for_task.emit(
                            "note_processed",
                            json!({
                                "note_id": note_id_for_task,
                                "entity_ids": entity_ids,
                                "entity_count": extracted.entities.len(),
                                "embedded_count": embed_count,
                                "relation_count": extracted.relations.len(),
                                "alias_routed": routed.routed.len(),
                                "flow": flow_label,
                            }),
                        );
                    }
                    Err(e) => {
                        tracing::warn!(?e, "reprocess save_extracted failed — keeping note as raw");
                        let _ = db.mark_note_processed(&note_id_for_task, None).await;
                        let _ = audit
                            .log(
                                "system",
                                "extract/raw_fallback",
                                Some("extract"),
                                Some(&format!("note={} {}=1", note_id_for_task, flow_label)),
                                &json!({
                                    "note_id": note_id_for_task,
                                    "error": e.to_string(),
                                    "stage": "save_extracted",
                                    "flow": flow_label,
                                }),
                                Some(0),
                            )
                            .await;
                        let _ = window_for_task.emit(
                            "note_processed",
                            json!({
                                "note_id": note_id_for_task,
                                "entity_ids": Vec::<String>::new(),
                                "entity_count": 0,
                                "raw_fallback": true,
                                "flow": flow_label,
                            }),
                        );
                    }
                }
            }
            Err(e) => {
                tracing::warn!(
                    ?e,
                    "reprocess extraction returned no structure — keeping as raw"
                );
                let _ = db.mark_note_processed(&note_id_for_task, None).await;
                let _ = audit
                    .log(
                        "system",
                        "extract/raw_fallback",
                        Some("extract"),
                        Some(&format!("note={} {}=1", note_id_for_task, flow_label)),
                        &json!({
                            "note_id": note_id_for_task,
                            "error": e.to_string(),
                            "flow": flow_label,
                        }),
                        Some(0),
                    )
                    .await;
                let _ = window_for_task.emit(
                    "note_processed",
                    json!({
                        "note_id": note_id_for_task,
                        "entity_ids": Vec::<String>::new(),
                        "entity_count": 0,
                        "raw_fallback": true,
                        "flow": flow_label,
                    }),
                );
            }
        }
    });
    Ok(())
}

// -------------------- Read paths --------------------

#[tauri::command]
pub async fn list_notes(limit: Option<i64>, state: State<'_, AppState>) -> CmdResult<Vec<Note>> {
    state.db.list_notes(limit.unwrap_or(50)).await.map_err(err)
}

#[tauri::command]
pub async fn list_entities(
    entity_type: Option<String>,
    limit: Option<i64>,
    state: State<'_, AppState>,
) -> CmdResult<Vec<Entity>> {
    state
        .db
        .list_entities(entity_type.as_deref(), limit.unwrap_or(100))
        .await
        .map_err(err)
}

#[derive(Serialize)]
pub struct EntityDetail {
    pub entity: Entity,
    pub relations: Vec<Relation>,
}

#[tauri::command]
pub async fn get_entity(id: String, state: State<'_, AppState>) -> CmdResult<EntityDetail> {
    let entity = state
        .db
        .get_entity(&id)
        .await
        .map_err(err)?
        .ok_or_else(|| "entity not found".to_string())?;
    let relations = state.db.entity_relations(&id).await.map_err(err)?;
    Ok(EntityDetail { entity, relations })
}

#[derive(Serialize)]
pub struct SearchPayload {
    pub query: String,
    pub entities: Vec<crate::retrieval::RetrievedEntity>,
    pub notes: Vec<crate::retrieval::RetrievedNote>,
    pub diagnostics: crate::retrieval::RetrievalDiagnostics,
}

#[tauri::command]
pub async fn search(
    query: String,
    types: Option<Vec<String>>,
    limit: Option<i64>,
    state: State<'_, AppState>,
) -> CmdResult<SearchPayload> {
    let limit = limit.unwrap_or(20);
    let res = state
        .retriever
        .search(&query, types, limit, limit)
        .await
        .map_err(err)?;
    Ok(SearchPayload {
        query: res.query,
        entities: res.entities,
        notes: res.notes,
        diagnostics: res.diagnostics,
    })
}

// -------------------- Agents + permissions --------------------

#[tauri::command]
pub async fn list_agents(state: State<'_, AppState>) -> CmdResult<Vec<Agent>> {
    state.db.list_agents().await.map_err(err)
}

#[tauri::command]
pub async fn add_agent(
    id: String,
    display_name: String,
    state: State<'_, AppState>,
) -> CmdResult<()> {
    state.db.upsert_agent(&id, &display_name).await.map_err(err)
}

#[tauri::command]
pub async fn delete_agent(id: String, state: State<'_, AppState>) -> CmdResult<u64> {
    state.db.delete_agent(&id).await.map_err(err)
}

#[tauri::command]
pub async fn list_permissions(
    agent_id: String,
    state: State<'_, AppState>,
) -> CmdResult<Vec<Permission>> {
    state.db.list_permissions(&agent_id).await.map_err(err)
}

#[tauri::command]
pub async fn grant_permission(
    agent_id: String,
    entity_type: String,
    scope: String,
    state: State<'_, AppState>,
) -> CmdResult<()> {
    if !matches!(scope.as_str(), "read" | "write" | "none") {
        return Err("scope must be read|write|none".into());
    }
    state
        .db
        .grant_permission(&agent_id, &entity_type, &scope)
        .await
        .map_err(err)
}

#[tauri::command]
pub async fn revoke_permission(
    agent_id: String,
    entity_type: String,
    state: State<'_, AppState>,
) -> CmdResult<()> {
    state
        .db
        .revoke_permission(&agent_id, &entity_type)
        .await
        .map_err(err)
}

// -------------------- Audit --------------------

#[tauri::command]
pub async fn list_audit(
    limit: Option<i64>,
    state: State<'_, AppState>,
) -> CmdResult<Vec<AuditEntry>> {
    state.db.list_audit(limit.unwrap_or(100)).await.map_err(err)
}

#[derive(Serialize)]
pub struct AuditVerification {
    pub ok: bool,
    pub message: String,
    pub entries_checked: i64,
}

#[tauri::command]
pub async fn verify_audit(
    limit: Option<i64>,
    state: State<'_, AppState>,
) -> CmdResult<AuditVerification> {
    let limit = limit.unwrap_or(500);
    match state.audit.verify_recent(limit).await {
        Ok(()) => Ok(AuditVerification {
            ok: true,
            message: "chain valid".into(),
            entries_checked: limit,
        }),
        Err(e) => Ok(AuditVerification {
            ok: false,
            message: e.to_string(),
            entries_checked: limit,
        }),
    }
}

#[tauri::command]
pub async fn audit_public_key(state: State<'_, AppState>) -> CmdResult<String> {
    Ok(state.audit.public_key_b64())
}

// -------------------- Hierarchical memory --------------------

#[tauri::command]
pub async fn list_themes(
    limit: Option<i64>,
    state: State<'_, AppState>,
) -> CmdResult<Vec<crate::consolidation::SemanticMemory>> {
    state
        .db
        .list_active_themes(limit.unwrap_or(200))
        .await
        .map_err(err)
}

#[tauri::command]
pub async fn get_theme(
    id: String,
    state: State<'_, AppState>,
) -> CmdResult<crate::consolidation::SemanticMemory> {
    state
        .db
        .get_theme(&id)
        .await
        .map_err(err)?
        .ok_or_else(|| "theme not found".to_string())
}

#[tauri::command]
pub async fn list_consolidation_runs(
    limit: Option<i64>,
    state: State<'_, AppState>,
) -> CmdResult<Vec<crate::consolidation::ConsolidationRun>> {
    state
        .db
        .list_consolidation_runs(limit.unwrap_or(50))
        .await
        .map_err(err)
}

#[tauri::command]
pub async fn trigger_consolidation(
    state: State<'_, AppState>,
) -> CmdResult<crate::consolidation::ConsolidationStats> {
    let Some(svc) = state.consolidation.clone() else {
        return Err("consolidation services unavailable (no AI_GATEWAY_API_KEY)".into());
    };
    crate::consolidation::run_consolidation(&state.db, &svc.client, &svc.cfg, "manual")
        .await
        .map_err(err)
}

// -------------------- Sync bundle (Phase 3c) --------------------

#[derive(Serialize)]
pub struct ExportBundleResult {
    pub bundle_id: String,
    pub path: String,
    pub bytes: u64,
}

#[tauri::command]
pub async fn export_bundle(
    path: String,
    device_name: Option<String>,
    state: State<'_, AppState>,
) -> CmdResult<ExportBundleResult> {
    let device = device_name
        .unwrap_or_else(|| std::env::var("CAIRN_DEVICE_NAME").unwrap_or_else(|_| "unknown".into()));
    let bundle = crate::bundle::export::export(&state.db, &state.audit, &device)
        .await
        .map_err(err)?;
    let json = serde_json::to_vec_pretty(&bundle).map_err(err)?;
    let bytes = json.len() as u64;
    let bundle_id = bundle.header.bundle_id.clone();
    std::fs::write(&path, &json).map_err(err)?;
    Ok(ExportBundleResult {
        bundle_id,
        path,
        bytes,
    })
}

#[tauri::command]
pub async fn import_bundle(
    path: String,
    verify_signature: Option<bool>,
    state: State<'_, AppState>,
) -> CmdResult<crate::bundle::import::ImportReport> {
    let bytes = std::fs::read(&path).map_err(err)?;
    let bundle: crate::bundle::types::Bundle = serde_json::from_slice(&bytes).map_err(err)?;
    crate::bundle::import::import(&state.db, &bundle, verify_signature.unwrap_or(true))
        .await
        .map_err(err)
}

// -------------------- Retrieval signals (Phase 3d) --------------------

#[tauri::command]
pub async fn log_retrieval(
    query_text: String,
    types: Option<Vec<String>>,
    returned_ids: Vec<String>,
    state: State<'_, AppState>,
) -> CmdResult<String> {
    state
        .db
        .log_retrieval(&crate::signals::LogRetrievalArgs {
            query_text,
            types_filter: types,
            returned_ids,
            embedding_model: Some(state.providers.embedding_model_id()),
        })
        .await
        .map_err(err)
}

#[tauri::command]
pub async fn record_signal_action(
    signal_id: String,
    action: String,
    entity_id: String,
    state: State<'_, AppState>,
) -> CmdResult<()> {
    let kind = match action.as_str() {
        "click" => crate::signals::SignalAction::Click,
        "dismiss" => crate::signals::SignalAction::Dismiss,
        "correct" => crate::signals::SignalAction::Correct,
        other => return Err(format!("unknown signal action: {other}")),
    };
    state
        .db
        .record_action(&signal_id, kind, &entity_id)
        .await
        .map_err(err)
}

#[tauri::command]
pub async fn list_retrieval_signals(
    limit: Option<i64>,
    state: State<'_, AppState>,
) -> CmdResult<Vec<crate::signals::RetrievalSignal>> {
    state
        .db
        .list_signals(limit.unwrap_or(200))
        .await
        .map_err(err)
}

// -------------------- Usage telemetry (Phase 4c) --------------------

#[tauri::command]
pub async fn list_usage(
    limit: Option<i64>,
    state: State<'_, AppState>,
) -> CmdResult<Vec<crate::usage::UsageRow>> {
    state.db.list_usage(limit.unwrap_or(200)).await.map_err(err)
}

#[tauri::command]
pub async fn usage_summary(
    days: Option<i64>,
    state: State<'_, AppState>,
) -> CmdResult<Vec<crate::usage::UsageDaySummary>> {
    state
        .db
        .usage_summary(days.unwrap_or(14))
        .await
        .map_err(err)
}

// -------------------- Capture sources (Phase 5) --------------------

#[tauri::command]
pub async fn list_capture_sources(
    state: State<'_, AppState>,
) -> CmdResult<Vec<crate::capture::CaptureSource>> {
    state.db.list_capture_sources().await.map_err(err)
}

#[derive(serde::Deserialize)]
pub struct AddImapArgs {
    pub label: String,
    pub host: String,
    pub port: Option<u16>,
    pub username: String,
    pub password: String,
    pub folder: Option<String>,
    pub poll_interval_secs: Option<i64>,
}

#[tauri::command]
pub async fn add_imap_source(args: AddImapArgs, state: State<'_, AppState>) -> CmdResult<String> {
    let cfg = serde_json::json!({
        "host": args.host,
        "port": args.port.unwrap_or(993),
        "username": args.username,
        "folder": args.folder.unwrap_or_else(|| "Cairn".into()),
    });
    let secret_account = format!("imap-{}", ulid::Ulid::new());
    crate::capture::store_password(&secret_account, &args.password)
        .await
        .map_err(err)?;
    let id = state
        .db
        .upsert_capture_source(
            None,
            "imap",
            &args.label,
            &cfg.to_string(),
            Some(&secret_account),
            args.poll_interval_secs.unwrap_or(300),
        )
        .await
        .map_err(err)?;
    state.capture.restart_all().await;
    Ok(id)
}

#[derive(serde::Deserialize)]
pub struct AddIcsArgs {
    pub label: String,
    pub url: String,
    pub poll_interval_secs: Option<i64>,
}

#[tauri::command]
pub async fn add_ics_source(args: AddIcsArgs, state: State<'_, AppState>) -> CmdResult<String> {
    let cfg = serde_json::json!({ "url": args.url });
    let id = state
        .db
        .upsert_capture_source(
            None,
            "ics",
            &args.label,
            &cfg.to_string(),
            None,
            args.poll_interval_secs.unwrap_or(1800),
        )
        .await
        .map_err(err)?;
    state.capture.restart_all().await;
    Ok(id)
}

#[tauri::command]
pub async fn set_capture_source_enabled(
    id: String,
    enabled: bool,
    state: State<'_, AppState>,
) -> CmdResult<()> {
    state
        .db
        .set_capture_source_enabled(&id, enabled)
        .await
        .map_err(err)?;
    state.capture.restart_all().await;
    Ok(())
}

#[tauri::command]
pub async fn delete_capture_source(id: String, state: State<'_, AppState>) -> CmdResult<()> {
    // Resolve secret account first (so we can clean its keychain entry).
    let sources = state.db.list_capture_sources().await.map_err(err)?;
    if let Some(src) = sources.iter().find(|s| s.id == id) {
        if let Some(acc) = &src.secret_account {
            let _ = crate::capture::delete_password(acc).await;
        }
    }
    state.db.delete_capture_source(&id).await.map_err(err)?;
    state.capture.restart_all().await;
    Ok(())
}

#[tauri::command]
pub async fn delete_note(note_id: String, state: State<'_, AppState>) -> CmdResult<()> {
    let result = state.db.delete_note(&note_id).await.map_err(err)?;
    if result.note_deleted > 0 {
        let _ = state
            .audit
            .log(
                "system",
                "note/delete",
                Some("note"),
                Some(&format!("note={}", note_id)),
                &json!({
                    "note_id": note_id,
                    "entities_deleted": result.entities_deleted,
                    "relations_deleted": result.relations_deleted,
                    "sticky_kept": result.sticky_kept.len(),
                }),
                Some(1),
            )
            .await;
        for sticky_id in &result.sticky_kept {
            let _ = state
                .audit
                .log(
                    "system",
                    "sticky_kept",
                    Some("sticky_kept"),
                    Some(&format!("entity={} note={}", sticky_id, note_id)),
                    &json!({
                        "entity_id": sticky_id,
                        "note_id": note_id,
                        "via": "delete_note",
                    }),
                    Some(1),
                )
                .await;
        }
    }
    Ok(())
}

// -------------------- Editable memory (Phase 7 · #81) --------------------

fn default_true() -> bool {
    true
}

#[derive(serde::Deserialize)]
pub struct UpdateEntityArgs {
    pub id: String,
    /// Patch — only non-None fields are written. Fields not in this list
    /// (created_at, source_note_id, etc.) cannot be changed via this IPC.
    pub name: Option<String>,
    pub entity_type: Option<String>,
    /// Stringified JSON. If present, replaces the whole properties blob.
    pub properties: Option<String>,
}

/// Apply a user edit to an entity. Re-embeds when name or properties
/// changed (so semantic search reflects the new wording), logs one row
/// per changed field to `entity_edits`, and audits the action.
#[tauri::command]
pub async fn update_entity(
    args: UpdateEntityArgs,
    state: State<'_, AppState>,
) -> CmdResult<Entity> {
    // Light validation — sqlite-side CHECK would also catch this but a
    // clear early error message is friendlier than a constraint
    // violation surfaced through map_err.
    if let Some(t) = &args.entity_type {
        if crate::schema::EntityType::parse(t).is_none() {
            return Err(format!("unknown entity_type: {t}"));
        }
    }
    if let Some(p) = &args.properties {
        if serde_json::from_str::<serde_json::Value>(p).is_err() {
            return Err("properties must be valid JSON".into());
        }
    }

    let (updated, diff) = state
        .db
        .update_entity(
            &args.id,
            args.name.as_deref(),
            args.entity_type.as_deref(),
            args.properties.as_deref(),
        )
        .await
        .map_err(err)?;

    if diff.is_empty() {
        // No-op edit — don't pollute the edit log, don't re-embed, but
        // still tell the caller the current state so the UI can render
        // a coherent next frame.
        return Ok(updated);
    }

    // Log every changed field. Doing this before re-embedding keeps
    // the audit trail accurate even if the embedder call later fails.
    for (field, before, after) in [
        ("name", diff.name.as_ref()),
        ("type", diff.r#type.as_ref()),
        ("properties", diff.properties.as_ref()),
    ]
    .into_iter()
    .filter_map(|(f, opt)| opt.map(|(b, a)| (f, b.as_str(), a.as_str())))
    {
        if let Err(e) = state
            .db
            .log_entity_edit(&args.id, "update", Some(field), Some(before), Some(after))
            .await
        {
            // Best effort — losing one edit-log row shouldn't fail the
            // user-visible update.
            tracing::warn!(?e, field, "log_entity_edit failed");
        }
    }

    // Re-embed when search-visible text changed. `entity_embed_text`
    // already concatenates type + name + properties, so any of the
    // three drifting requires a fresh vector.
    if diff.name.is_some() || diff.properties.is_some() || diff.r#type.is_some() {
        let embedder = state.providers.embedder();
        let model_id = state.providers.embedding_model_id();
        if let Err(e) = crate::embed_pipeline::embed_entities(
            &state.db,
            &embedder,
            &model_id,
            std::slice::from_ref(&args.id),
        )
        .await
        {
            tracing::warn!(?e, entity_id = %args.id, "re-embed after edit failed");
        }
    }

    // The user just corrected this entity → mark it sticky so the next
    // import-watcher cascade can't silently un-correct it. Idempotent.
    if let Err(e) = state.db.set_entity_sticky(&args.id).await {
        tracing::warn!(?e, entity_id = %args.id, "set_entity_sticky after edit failed");
    }

    let _ = state
        .audit
        .log(
            "user",
            "edit_entity",
            Some("update"),
            Some(&args.id),
            &json!({
                "entity_id": args.id,
                "fields_changed": diff_field_list(&diff),
            }),
            Some(1),
        )
        .await;

    Ok(updated)
}

fn diff_field_list(diff: &crate::db::queries::EntityUpdateDiff) -> Vec<&'static str> {
    let mut out = Vec::new();
    if diff.name.is_some() {
        out.push("name");
    }
    if diff.r#type.is_some() {
        out.push("type");
    }
    if diff.properties.is_some() {
        out.push("properties");
    }
    out
}

#[derive(serde::Deserialize)]
pub struct UpdateNoteArgs {
    pub id: String,
    pub text: String,
    /// If `true`, also re-runs the extractor pipeline on the new text.
    /// Default true; setting false is escape hatch for FTS-only updates.
    #[serde(default = "default_true")]
    pub reextract: bool,
}

/// Overwrite a note's text. The `notes_fts_au` trigger handles
/// re-indexing for free; if `reextract` is true we also wipe the old
/// extraction and spawn the same flow `reprocess_note` uses so derived
/// entities catch up with the new wording.
#[tauri::command]
pub async fn update_note(
    args: UpdateNoteArgs,
    window: Window,
    state: State<'_, AppState>,
) -> CmdResult<()> {
    let updated = state
        .db
        .update_note_text(&args.id, &args.text)
        .await
        .map_err(err)?;
    if !updated {
        return Err("note not found".into());
    }

    let _ = state
        .audit
        .log(
            "user",
            "edit_note",
            Some("update"),
            Some(&args.id),
            &json!({
                "note_id": args.id,
                "reextract": args.reextract,
                "text_len": args.text.chars().count(),
            }),
            Some(1),
        )
        .await;

    if args.reextract {
        // Reuses the same cleanup + spawn that `reprocess_note` uses,
        // so derived entities stay aligned with the new wording.
        reprocess_note_inner(args.id, args.text, "edit", &window, &state).await?;
    }
    Ok(())
}

#[derive(serde::Deserialize)]
pub struct CreateEntityArgs {
    pub entity_type: String,
    pub name: String,
    /// Optional detail. If present, stored as `{ statement: detail }` in properties.
    pub detail: Option<String>,
}

/// Insert a user-authored entity. Bypasses the LLM extractor entirely
/// — confidence is set to 1.0 and `source_note_id` is NULL so the
/// retrieval layer can recognise this as a manually-asserted fact.
#[tauri::command]
pub async fn create_entity(
    args: CreateEntityArgs,
    state: State<'_, AppState>,
) -> CmdResult<Entity> {
    if crate::schema::EntityType::parse(&args.entity_type).is_none() {
        return Err(format!("unknown entity_type: {}", args.entity_type));
    }
    let name = args.name.trim();
    if name.is_empty() {
        return Err("name cannot be empty".into());
    }

    let properties_json = match args.detail.as_ref().filter(|s| !s.trim().is_empty()) {
        Some(d) => json!({ "statement": d }).to_string(),
        None => "{}".to_string(),
    };

    // Manually-created entities get a slightly higher prior than the
    // typed-extraction baseline. Picked 0.6 so a hand-curated entity
    // still ranks below "Person" (0.80) but above the LLM-derived
    // average — matches the spec.
    let entity = state
        .db
        .insert_entity_manual(&args.entity_type, name, &properties_json, 0.6)
        .await
        .map_err(err)?;

    // Best-effort embedding so semantic search picks the new entity up
    // immediately. Without it the entity is still BM25-searchable.
    let embedder = state.providers.embedder();
    let model_id = state.providers.embedding_model_id();
    if let Err(e) = crate::embed_pipeline::embed_entities(
        &state.db,
        &embedder,
        &model_id,
        std::slice::from_ref(&entity.id),
    )
    .await
    {
        tracing::warn!(?e, entity_id = %entity.id, "initial embed for manual entity failed");
    }

    if let Err(e) = state
        .db
        .log_entity_edit(&entity.id, "create", None, None, None)
        .await
    {
        tracing::warn!(?e, entity_id = %entity.id, "log_entity_edit (create) failed");
    }

    // Manual creation is one of the three sticky triggers from the spec —
    // pin it so import-watcher churn can't cascade-delete it. Idempotent,
    // so safe to set unconditionally even if a future caller re-uses
    // `insert_entity_manual` for non-sticky inserts.
    if let Err(e) = state.db.set_entity_sticky(&entity.id).await {
        tracing::warn!(?e, entity_id = %entity.id, "set_entity_sticky for manual create failed");
    }

    let _ = state
        .audit
        .log(
            "user",
            "create_entity",
            Some("create"),
            Some(&entity.id),
            &json!({
                "entity_id": entity.id,
                "type": entity.r#type,
                "name": entity.name,
                "has_detail": args.detail.as_ref().map(|d| !d.trim().is_empty()).unwrap_or(false),
            }),
            Some(1),
        )
        .await;

    Ok(entity)
}

// -------------------- Edit history (Phase 7 · #82) --------------------

/// List recent manual edits to an entity for the timeline UI on the
/// Entity Detail page. Newest-first. `value_before` / `value_after`
/// arrive pre-truncated to [`crate::db::queries::MAX_EDIT_VALUE_LEN`].
#[tauri::command]
pub async fn list_entity_edits(
    entity_id: String,
    limit: Option<i64>,
    state: State<'_, AppState>,
) -> CmdResult<Vec<crate::db::queries::EntityEditRow>> {
    state
        .db
        .list_entity_edits(&entity_id, limit.unwrap_or(50))
        .await
        .map_err(err)
}

/// Batch count of manual edits for a set of entity ids. One SQL query
/// regardless of fan-out — the entities grid uses this to pin the
/// "corrected N×" badge on cards. Entities with zero edits are simply
/// absent from the returned map; callers default to 0.
#[tauri::command]
pub async fn entity_edit_counts(
    ids: Vec<String>,
    state: State<'_, AppState>,
) -> CmdResult<std::collections::HashMap<String, i64>> {
    state.db.manual_edit_counts(&ids).await.map_err(err)
}

// -------------------- Selection popover (Phase 6a) --------------------

#[tauri::command]
pub async fn selection_status(state: State<'_, AppState>) -> CmdResult<SelectionStatus> {
    Ok(state.selection.status())
}

#[tauri::command]
pub async fn selection_set_enabled(
    enabled: bool,
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> CmdResult<SelectionStatus> {
    Ok(state.selection.set_enabled(&app, enabled))
}

#[tauri::command]
pub async fn selection_request_permission(
    state: State<'_, AppState>,
) -> CmdResult<SelectionStatus> {
    Ok(state.selection.request_permission())
}

#[tauri::command]
pub async fn selection_get_current(
    state: State<'_, AppState>,
) -> CmdResult<Option<SelectionEvent>> {
    Ok(state.selection.current_selection())
}

#[tauri::command]
pub async fn selection_dismiss(app: tauri::AppHandle, state: State<'_, AppState>) -> CmdResult<()> {
    // Read the source PID before `dismiss` clears the cached selection so we
    // can bounce focus back (otherwise clicking × surfaces Cairn's main
    // window — see `panel::reactivate_source_app`).
    #[cfg(target_os = "macos")]
    let source_pid = state.selection.current_selection().map(|c| c.source_pid);
    state.selection.dismiss(&app);
    #[cfg(target_os = "macos")]
    if let Some(pid) = source_pid {
        crate::selection_popover::panel::reactivate_source_app(pid);
    }
    Ok(())
}

/// "Save + note" action — opens the existing quick-capture window prefilled
/// with the selection text. Closes the popover first so focus flows to the
/// editor.
#[tauri::command]
pub async fn selection_save_with_note(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> CmdResult<()> {
    let text = state
        .selection
        .current_selection()
        .map(|c| c.text)
        .unwrap_or_default();
    state.selection.dismiss(&app);
    if text.is_empty() {
        return Ok(());
    }
    crate::global_shortcut::open_quick_capture(&app, &text).map_err(err)?;
    Ok(())
}

// -------------------- Remote MCP bridge (Phase 6b) --------------------

// -------------------- Import (Phase 7 · V6 M1) --------------------

#[derive(serde::Deserialize, Default)]
pub struct ImportClaudeAutoArgs {
    /// Override `$HOME`. Mostly useful for tests + dev. M0 called this
    /// `root` (meaning `~/.claude/projects`); we widened it to `home`
    /// since M1 scans both `~/.claude/CLAUDE.md` and the projects tree.
    /// Old callers can still pass `root` and the scanner handles either.
    #[serde(default, alias = "root")]
    pub home: Option<String>,
    /// `true` returns counts + previews without writing to the DB.
    #[serde(default)]
    pub dry_run: bool,
    /// Absolute paths the user deselected in the preview (per-file
    /// opt-out). On Apply these are silently skipped.
    #[serde(default)]
    pub exclude_paths: Vec<String>,
    /// Paths to force-reimport even if unchanged — the user-checked Unlinked
    /// rows being pulled back. Bypasses the content-hash dedup.
    #[serde(default)]
    pub force_paths: Vec<String>,
}

#[tauri::command]
pub async fn import_claude_auto_run(
    args: ImportClaudeAutoArgs,
    state: State<'_, AppState>,
) -> CmdResult<crate::import::ImportSummary> {
    // Build a one-shot ProcessingContext from live providers so each
    // imported note runs the full extract → save → embed pipeline. With
    // BM25-only mode (no providers) we still flat-save the notes; they're
    // searchable via FTS / LIKE.
    let extractor = state.providers.extractor();
    let embedder = state.providers.embedder();
    let processing = crate::capture::processor::ProcessingContext {
        db: state.db.clone(),
        extractor,
        embedder,
        audit: state.audit.clone(),
        embedding_model_id: state.providers.embedding_model_id(),
    };
    let opts = crate::import::ImportOptions {
        home: args.home.as_deref().map(std::path::PathBuf::from),
        dry_run: args.dry_run,
        exclude_paths: args
            .exclude_paths
            .iter()
            .map(std::path::PathBuf::from)
            .collect(),
        force_paths: args
            .force_paths
            .iter()
            .map(std::path::PathBuf::from)
            .collect(),
        only_paths: vec![],
    };
    let summary = crate::import::run_import(&state.db, Some(&processing), opts)
        .await
        .map_err(err)?;
    Ok(summary)
}

/// Validate a discovered-file path against the user-level memory roots the
/// scanner walks (`~/.claude`, `~/.cursor/skills-cursor`, `~/.cursor/plans`,
/// `~/.codex/memories`), returning the canonicalized path. Shared by
/// `read_import_source_file` (preview) and `import_resync_file` (force
/// re-import) so neither can be coerced into an arbitrary-file read/import —
/// the renderer is sandboxed but the host page's JS could still invoke us.
fn resolve_import_path(path: &str) -> Result<std::path::PathBuf, String> {
    let p = std::path::PathBuf::from(path);
    let home = directories::UserDirs::new()
        .map(|d| d.home_dir().to_path_buf())
        .ok_or_else(|| "no home dir".to_string())?;
    let allowed = [
        home.join(".claude"),
        home.join(".cursor").join("skills-cursor"),
        home.join(".cursor").join("plans"),
        home.join(".codex").join("memories"),
    ];
    let canon = p.canonicalize().map_err(err)?;
    if !allowed.iter().any(|root| canon.starts_with(root)) {
        return Err(format!("path outside allowed roots: {}", path));
    }
    Ok(canon)
}

/// Read a discovered file's contents for the Import-page preview pane,
/// capped at 16 KB. Path is validated via `resolve_import_path`.
#[tauri::command]
pub async fn read_import_source_file(path: String) -> CmdResult<String> {
    let canon = resolve_import_path(&path)?;
    let bytes = std::fs::read(&canon).map_err(err)?;
    const MAX: usize = 16 * 1024;
    let truncated = if bytes.len() > MAX {
        let mut head = bytes[..MAX].to_vec();
        head.extend_from_slice(
            b"\n\n[\xe2\x80\xa6 truncated; file is larger than 16KB \xe2\x80\xa6]",
        );
        head
    } else {
        bytes
    };
    String::from_utf8(truncated).map_err(|e| format!("not utf-8: {e}"))
}

/// Force a single discovered file back into Cairn, bypassing content-hash
/// dedup. Backs the per-row "re-sync" button — pulls back an Unlinked file
/// (note was deleted) or re-runs extraction on an unchanged one. Returns the
/// summary for that one file. Path is validated against the same allow-list
/// as the preview reader.
#[tauri::command]
pub async fn import_resync_file(
    path: String,
    state: State<'_, AppState>,
) -> CmdResult<crate::import::ImportSummary> {
    // Security: reject anything outside the user-level memory roots.
    resolve_import_path(&path)?;
    // Match the scanner's own (non-canonicalized) path for force/only — on
    // macOS canonicalize resolves to /System/Volumes/Data/... which wouldn't
    // equal the scanner's home-joined /Users/... paths, so the filter would
    // miss. The path came from the scanner's output via the UI, so it's
    // already in the scanner's coordinate space.
    let raw = std::path::PathBuf::from(&path);

    let extractor = state.providers.extractor();
    let embedder = state.providers.embedder();
    let processing = crate::capture::processor::ProcessingContext {
        db: state.db.clone(),
        extractor,
        embedder,
        audit: state.audit.clone(),
        embedding_model_id: state.providers.embedding_model_id(),
    };
    let opts = crate::import::ImportOptions {
        home: None,
        dry_run: false,
        exclude_paths: vec![],
        force_paths: vec![raw.clone()],
        only_paths: vec![raw],
    };
    let summary = crate::import::run_import(&state.db, Some(&processing), opts)
        .await
        .map_err(err)?;
    Ok(summary)
}

/// Mark a discovered file "do not sync" (persistent) or clear that flag.
/// `skipped = true` adds it to the skip list (status → SKIPPED, never
/// imported by Apply or the watcher); `false` removes it so the file rejoins
/// the normal sync flow. Path is validated against the same allow-list as the
/// preview reader; the scanner-coordinate path is stored verbatim so
/// `run_import`'s membership check matches.
#[tauri::command]
pub async fn import_set_skip(
    path: String,
    skipped: bool,
    state: State<'_, AppState>,
) -> CmdResult<()> {
    resolve_import_path(&path)?; // security gate (canonical result unused)
    state.db.set_import_skip(&path, skipped).await.map_err(err)?;
    Ok(())
}

// -------------------- Maintenance --------------------

#[tauri::command]
pub async fn vacuum_orphan_vecs(
    state: State<'_, AppState>,
) -> CmdResult<crate::db::queries::VacuumOrphanReport> {
    let db_path = state.data_dir.join("memory.db");
    state
        .db
        .vacuum_orphan_entity_vecs(&db_path)
        .await
        .map_err(err)
}

// -------------------- Export (Phase 7 · V6 M2) --------------------

#[derive(serde::Deserialize, Default)]
pub struct ExportArgs {
    #[serde(default)]
    pub home: Option<String>,
    /// Set true to overwrite even if Cairn detected user edits inside its block.
    #[serde(default)]
    pub force: bool,
    /// Cap how many entities make it into the managed block.
    #[serde(default)]
    pub max_entities: Option<usize>,
}

fn build_export_options(args: ExportArgs) -> crate::import::export::ExportOptions {
    crate::import::export::ExportOptions {
        home: args.home.as_deref().map(std::path::PathBuf::from),
        force: args.force,
        max_entities: args.max_entities,
    }
}

#[tauri::command]
pub async fn export_preview(
    args: ExportArgs,
    state: State<'_, AppState>,
) -> CmdResult<crate::import::export::ExportPreview> {
    let opts = build_export_options(args);
    crate::import::export::export_preview(&state.db, opts)
        .await
        .map_err(err)
}

#[tauri::command]
pub async fn export_run(
    args: ExportArgs,
    state: State<'_, AppState>,
) -> CmdResult<crate::import::export::ExportResult> {
    let opts = build_export_options(args);
    crate::import::export::export_run(&state.db, opts)
        .await
        .map_err(err)
}

// -------------------- AI provider config (Phase 6d) --------------------

#[derive(serde::Serialize)]
pub struct ProvidersView {
    pub config: ProvidersConfig,
    pub presets: Vec<ProviderPresetView>,
    /// True when the running process is still using the previous provider
    /// values — i.e. the saved config differs from what was loaded at boot.
    /// UI shows a "Restart Cairn to apply" hint when this is `true`.
    pub restart_required: bool,
}

#[derive(serde::Serialize)]
pub struct ProviderPresetView {
    pub family: ProviderFamily,
    pub display_name: &'static str,
    pub api_base: &'static str,
    pub default_extract_model: &'static str,
    pub default_embed_model: Option<&'static str>,
    pub supports_embedding: bool,
}

fn presets_view() -> Vec<ProviderPresetView> {
    providers::PRESETS
        .iter()
        .map(|p: &ProviderPreset| ProviderPresetView {
            family: p.family,
            display_name: p.display_name,
            api_base: p.api_base,
            default_extract_model: p.default_extract_model,
            default_embed_model: p.default_embed_model,
            supports_embedding: p.default_embed_model.is_some(),
        })
        .collect()
}

#[tauri::command]
pub async fn providers_get(state: State<'_, AppState>) -> CmdResult<ProvidersView> {
    let saved = providers::load(&state.data_dir);
    let restart_required = saved.extract.as_ref().map(|c| (c.family, c.model.clone()))
        != state.boot_extract_summary.clone();
    Ok(ProvidersView {
        config: saved,
        presets: presets_view(),
        restart_required,
    })
}

#[tauri::command]
pub async fn providers_save(
    config: ProvidersConfig,
    state: State<'_, AppState>,
) -> CmdResult<ProvidersView> {
    providers::save(&state.data_dir, &config).map_err(err)?;
    // Hot-swap the live extractor + embedder so the change takes effect
    // immediately — no Cairn restart required.
    state.providers.reload(&config);
    tracing::info!(
        extract = ?config.extract.as_ref().map(|c| c.family),
        embed = ?config.embed.as_ref().map(|c| c.family),
        "providers reloaded"
    );
    Ok(ProvidersView {
        config,
        presets: presets_view(),
        // After reload, the running instance matches what's saved.
        restart_required: false,
    })
}

#[derive(serde::Serialize)]
pub struct ProviderTestResult {
    pub ok: bool,
    pub message: String,
}

#[tauri::command]
pub async fn providers_test(slot: String, config: ProviderConfig) -> CmdResult<ProviderTestResult> {
    let result = match slot.as_str() {
        "extract" => match config.build_extractor() {
            Ok(prov) => match prov.extract("ping").await {
                Ok(_) => ProviderTestResult {
                    ok: true,
                    message: "extract OK".into(),
                },
                Err(e) => ProviderTestResult {
                    ok: false,
                    message: format!("extract failed: {e}"),
                },
            },
            Err(e) => ProviderTestResult {
                ok: false,
                message: format!("build failed: {e}"),
            },
        },
        "embed" => match config.build_embedder(1024) {
            Ok(prov) => match prov.embed_batch(&["ping".to_string()]).await {
                Ok(_) => ProviderTestResult {
                    ok: true,
                    message: "embed OK".into(),
                },
                Err(e) => ProviderTestResult {
                    ok: false,
                    message: format!("embed failed: {e}"),
                },
            },
            Err(e) => ProviderTestResult {
                ok: false,
                message: format!("build failed: {e}"),
            },
        },
        other => ProviderTestResult {
            ok: false,
            message: format!("unknown slot: {other}"),
        },
    };
    Ok(result)
}

// -------------------- Dedup + ranking + archive (Phase 7 · #83) --------------------

/// Pull up to `limit` dedup candidate pairs. Cosine similarity is computed
/// in Rust over `entity_vecs` bytes (see `find_dedup_candidates` for the
/// full filter chain). Audited so the user can prove the surface was
/// generated, not just guessed at.
#[tauri::command]
pub async fn list_dedup_candidates(
    limit: i64,
    state: State<'_, AppState>,
) -> CmdResult<Vec<crate::db::queries::DedupCandidate>> {
    let limit_usize = limit.max(0) as usize;
    let candidates = state
        .db
        .find_dedup_candidates(limit_usize)
        .await
        .map_err(err)?;
    let _ = state
        .audit
        .log(
            "user",
            "dedup/list_candidates",
            Some("dedup"),
            Some(&format!("limit={}", limit)),
            &json!({ "count": candidates.len() }),
            Some(candidates.len() as i64),
        )
        .await;
    Ok(candidates)
}

#[derive(serde::Deserialize)]
pub struct MergeEntitiesArgs {
    pub keep_id: String,
    pub drop_id: String,
}

/// Merge `drop_id` into `keep_id` (relations + edits reparented, vec /
/// alias / row deleted). Idempotent for already-merged pairs only insofar
/// as the second call will fail with "drop entity not found".
#[tauri::command]
pub async fn merge_entities(
    args: MergeEntitiesArgs,
    state: State<'_, AppState>,
) -> CmdResult<crate::db::queries::MergeReport> {
    if args.keep_id == args.drop_id {
        return Err("cannot merge an entity into itself".into());
    }
    let report = state
        .db
        .merge_entities(&args.keep_id, &args.drop_id)
        .await
        .map_err(err)?;
    let _ = state
        .audit
        .log(
            "user",
            "dedup/merge",
            Some("merge"),
            Some(&format!("keep={} drop={}", args.keep_id, args.drop_id)),
            &json!({
                "keep_id": args.keep_id,
                "drop_id": args.drop_id,
                "relations_migrated": report.relations_migrated,
                "edits_migrated": report.edits_migrated,
                "vec_dropped": report.vec_dropped,
                "alias_dropped": report.alias_dropped,
            }),
            Some(report.relations_migrated as i64),
        )
        .await;
    Ok(report)
}

#[derive(serde::Deserialize)]
pub struct RejectDedupPairArgs {
    pub keep_id: String,
    pub drop_id: String,
}

/// Mark a candidate pair as "not duplicates" so it stops appearing in
/// future `list_dedup_candidates` calls.
#[tauri::command]
pub async fn reject_dedup_pair(
    args: RejectDedupPairArgs,
    state: State<'_, AppState>,
) -> CmdResult<()> {
    state
        .db
        .reject_dedup_pair(&args.keep_id, &args.drop_id)
        .await
        .map_err(err)?;
    let _ = state
        .audit
        .log(
            "user",
            "dedup/reject",
            Some("dedup"),
            Some(&format!("keep={} drop={}", args.keep_id, args.drop_id)),
            &json!({
                "keep_id": args.keep_id,
                "drop_id": args.drop_id,
            }),
            Some(1),
        )
        .await;
    Ok(())
}

/// List the alias rows pointing at `entity_id`. Each row records a prior
/// merge — frontend uses this to surface "Includes: alpha, beta, …" on
/// the Entity Detail page plus to power the unmerge UI.
#[tauri::command]
pub async fn list_entity_aliases(
    entity_id: String,
    state: State<'_, AppState>,
) -> CmdResult<Vec<crate::db::queries::AliasRow>> {
    state.db.list_entity_aliases(&entity_id).await.map_err(err)
}

/// Remove a single alias row. Used by the unmerge UI when the user
/// decides an earlier merge was wrong. Also clears `is_sticky` on the
/// keep when there are no remaining aliases AND no manual edits — the
/// spec is explicit that we never override the user's intent if they've
/// actually corrected the entity. Idempotent on already-deleted rows.
#[tauri::command]
pub async fn remove_entity_alias(alias_id: String, state: State<'_, AppState>) -> CmdResult<()> {
    let Some(row) = state.db.get_entity_alias(&alias_id).await.map_err(err)? else {
        return Ok(()); // Nothing to do; treat as idempotent.
    };
    let _ = state.db.delete_entity_alias(&alias_id).await.map_err(err)?;
    let remaining = state
        .db
        .count_entity_aliases(&row.keep_id)
        .await
        .unwrap_or(1);
    let manual_edits = state.db.manual_edit_count(&row.keep_id).await.unwrap_or(0);
    if remaining == 0 && manual_edits == 0 {
        if let Err(e) = state.db.clear_entity_sticky(&row.keep_id).await {
            tracing::warn!(?e, keep = %row.keep_id, "clear_entity_sticky after unmerge failed");
        }
    }
    let _ = state
        .audit
        .log(
            "user",
            "alias_unmerge",
            Some("alias_unmerge"),
            Some(&format!("alias={} keep={}", alias_id, row.keep_id)),
            &json!({
                "alias_id": alias_id,
                "keep_id": row.keep_id,
                "name_lower": row.name_lower,
                "remaining_aliases": remaining,
                "manual_edits": manual_edits,
            }),
            Some(1),
        )
        .await;
    Ok(())
}

/// Signal-ranked entity listing for the "entities at a glance" dashboard.
/// Excludes archived entities. Score formula lives in
/// `queries::list_entities_ranked`.
#[tauri::command]
pub async fn list_entities_ranked(
    limit: Option<i64>,
    state: State<'_, AppState>,
) -> CmdResult<Vec<crate::db::queries::RankedEntity>> {
    state
        .db
        .list_entities_ranked(limit.unwrap_or(100))
        .await
        .map_err(err)
}

#[derive(serde::Deserialize)]
pub struct SetEntityArchivedArgs {
    pub id: String,
    pub archived: bool,
}

/// Flip an entity's soft-archive flag. Archive hides from retrieval +
/// the dashboard listing; restore brings it back.
#[tauri::command]
pub async fn set_entity_archived(
    args: SetEntityArchivedArgs,
    state: State<'_, AppState>,
) -> CmdResult<()> {
    state
        .db
        .set_entity_archived(&args.id, args.archived)
        .await
        .map_err(err)?;
    let _ = state
        .audit
        .log(
            "user",
            if args.archived {
                "entity/archive"
            } else {
                "entity/restore"
            },
            Some("archive"),
            Some(&args.id),
            &json!({
                "entity_id": args.id,
                "archived": args.archived,
            }),
            Some(1),
        )
        .await;
    Ok(())
}

#[tauri::command]
pub async fn list_archived_entities(
    limit: Option<i64>,
    state: State<'_, AppState>,
) -> CmdResult<Vec<Entity>> {
    state
        .db
        .list_archived_entities(limit.unwrap_or(100))
        .await
        .map_err(err)
}

/// Manual trigger for the stale-entity sweep. Returns the count of
/// entities archived in this run. The consolidation worker fires this
/// on its tick — this IPC is the UI's "force a sweep" button.
#[tauri::command]
pub async fn auto_archive_now(state: State<'_, AppState>) -> CmdResult<i64> {
    let n = state.db.auto_archive_stale_entities().await.map_err(err)?;
    let _ = state
        .audit
        .log(
            "user",
            "entity/auto_archive",
            Some("archive"),
            None,
            &json!({ "archived": n, "trigger": "manual" }),
            Some(n as i64),
        )
        .await;
    Ok(n as i64)
}

#[tauri::command]
pub async fn mcp_bridge_status(state: State<'_, AppState>) -> CmdResult<McpBridgeStatus> {
    Ok(state.mcp_bridge.status())
}

#[tauri::command]
pub async fn mcp_bridge_set_enabled(
    enabled: bool,
    state: State<'_, AppState>,
) -> CmdResult<McpBridgeStatus> {
    Ok(state.mcp_bridge.set_enabled(enabled))
}
