//! Import a cairn-bundle into the local DB with per-row LWW conflict resolution.
//!
//! Verification:
//!   - format string matches `BUNDLE_FORMAT`
//!   - checksum of the payload matches the header
//!   - signature verifies under `header.device_pubkey`
//!
//! Conflict resolution (per-row):
//!   - notes / themes:   replace if remote.created_at > local.created_at
//!   - entities:         replace if remote.updated_at > local.updated_at
//!   - relations:        insert if absent (no in-place update)
//!
//! Audit: every import writes a `sync_log` row with counts and conflict count.

use crate::bundle::types::{Bundle, BundleCounts};
use crate::bundle::BUNDLE_FORMAT;
use crate::db::{now_ms, Db};
use anyhow::{anyhow, bail, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportReport {
    pub bundle_id: String,
    pub imported_notes: u32,
    pub imported_entities: u32,
    pub imported_relations: u32,
    pub imported_themes: u32,
    pub conflicts: u32,
    pub signature_ok: bool,
    pub errors: Vec<String>,
}

pub async fn import(db: &Db, bundle: &Bundle, verify_signature: bool) -> Result<ImportReport> {
    if bundle.header.format != BUNDLE_FORMAT {
        bail!("unsupported bundle format: {}", bundle.header.format);
    }

    // Re-compute payload checksum and verify.
    let payload = serde_json::json!({
        "entities": &bundle.entities,
        "notes": &bundle.notes,
        "relations": &bundle.relations,
        "themes": &bundle.themes,
    });
    let payload_bytes = serde_json::to_vec(&payload)?;
    let computed = sha256_hex(&payload_bytes);
    if computed != bundle.header.checksum {
        bail!(
            "bundle checksum mismatch: header says {}, computed {}",
            bundle.header.checksum,
            computed
        );
    }

    let signature_ok = if verify_signature {
        verify_bundle_signature(bundle, &payload_bytes)?
    } else {
        false
    };

    let mut report = ImportReport {
        bundle_id: bundle.header.bundle_id.clone(),
        imported_notes: 0,
        imported_entities: 0,
        imported_relations: 0,
        imported_themes: 0,
        conflicts: 0,
        signature_ok,
        errors: vec![],
    };

    for note in &bundle.notes {
        match upsert_note_lww(db, note).await {
            Ok(true) => report.imported_notes += 1,
            Ok(false) => report.conflicts += 1,
            Err(e) => report.errors.push(format!("note {}: {}", note.id, e)),
        }
    }
    for ent in &bundle.entities {
        match upsert_entity_lww(db, ent).await {
            Ok(true) => report.imported_entities += 1,
            Ok(false) => report.conflicts += 1,
            Err(e) => report.errors.push(format!("entity {}: {}", ent.id, e)),
        }
    }
    for rel in &bundle.relations {
        match insert_relation_if_absent(db, rel).await {
            Ok(true) => report.imported_relations += 1,
            Ok(false) => report.conflicts += 1,
            Err(e) => report.errors.push(format!("relation {}: {}", rel.id, e)),
        }
    }
    for theme in &bundle.themes {
        match upsert_theme_lww(db, theme).await {
            Ok(true) => report.imported_themes += 1,
            Ok(false) => report.conflicts += 1,
            Err(e) => report.errors.push(format!("theme {}: {}", theme.id, e)),
        }
    }

    let counts = BundleCounts {
        notes: report.imported_notes,
        entities: report.imported_entities,
        relations: report.imported_relations,
        themes: report.imported_themes,
    };
    let errors_json = if report.errors.is_empty() {
        None
    } else {
        serde_json::to_string(&report.errors).ok()
    };
    db.insert_sync_log(
        "import",
        &bundle.header.bundle_id,
        Some(&bundle.header.device_name),
        Some(&bundle.header.device_pubkey),
        &counts,
        &bundle.header.checksum,
        Some(signature_ok),
        report.conflicts as i64,
        errors_json.as_deref(),
    )
    .await?;

    Ok(report)
}

fn verify_bundle_signature(bundle: &Bundle, payload_bytes: &[u8]) -> Result<bool> {
    if bundle.signature.is_empty() {
        return Ok(false);
    }
    let header_bytes = serde_json::to_vec(&bundle.header)?;
    let mut hasher = Sha256::new();
    hasher.update(&header_bytes);
    hasher.update(payload_bytes);
    let to_verify = hasher.finalize();

    let pub_bytes = B64
        .decode(&bundle.header.device_pubkey)
        .map_err(|e| anyhow!("decode device_pubkey: {e}"))?;
    if pub_bytes.len() != 32 {
        bail!("device_pubkey wrong length: {}", pub_bytes.len());
    }
    let mut pk = [0u8; 32];
    pk.copy_from_slice(&pub_bytes);
    let vk = VerifyingKey::from_bytes(&pk).map_err(|e| anyhow!("decode pubkey: {e}"))?;

    let sig_bytes = B64
        .decode(&bundle.signature)
        .map_err(|e| anyhow!("decode signature: {e}"))?;
    if sig_bytes.len() != 64 {
        bail!("signature wrong length: {}", sig_bytes.len());
    }
    let mut sb = [0u8; 64];
    sb.copy_from_slice(&sig_bytes);
    let sig = Signature::from_bytes(&sb);
    Ok(vk.verify(&to_verify, &sig).is_ok())
}

async fn upsert_note_lww(db: &Db, note: &crate::bundle::types::NoteRow) -> Result<bool> {
    let existing: Option<(i64,)> = sqlx::query_as("SELECT created_at FROM notes WHERE id = ?")
        .bind(&note.id)
        .fetch_optional(db.pool())
        .await?;
    if let Some((local_ts,)) = existing {
        if note.created_at <= local_ts {
            return Ok(false);
        }
    }
    sqlx::query(
        "INSERT INTO notes (id, text, source, created_at, processed_at, processing_status) \
         VALUES (?, ?, ?, ?, ?, ?) \
         ON CONFLICT(id) DO UPDATE SET text = excluded.text, source = excluded.source, \
                                       created_at = excluded.created_at, \
                                       processed_at = excluded.processed_at, \
                                       processing_status = excluded.processing_status",
    )
    .bind(&note.id)
    .bind(&note.text)
    .bind(&note.source)
    .bind(note.created_at)
    .bind(note.processed_at)
    .bind(&note.processing_status)
    .execute(db.pool())
    .await?;
    Ok(true)
}

async fn upsert_entity_lww(db: &Db, ent: &crate::bundle::types::EntityRow) -> Result<bool> {
    let existing: Option<(i64,)> = sqlx::query_as("SELECT updated_at FROM entities WHERE id = ?")
        .bind(&ent.id)
        .fetch_optional(db.pool())
        .await?;
    if let Some((local_ts,)) = existing {
        if ent.updated_at <= local_ts {
            return Ok(false);
        }
    }
    let props = serde_json::to_string(&ent.properties)?;
    sqlx::query(
        "INSERT INTO entities (id, type, name, properties, confidence, source_note_id, \
                               created_at, updated_at, strength, access_count, last_accessed, \
                               base_importance) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
         ON CONFLICT(id) DO UPDATE SET \
             type = excluded.type, name = excluded.name, \
             properties = excluded.properties, confidence = excluded.confidence, \
             source_note_id = excluded.source_note_id, updated_at = excluded.updated_at, \
             strength = excluded.strength, access_count = excluded.access_count, \
             last_accessed = excluded.last_accessed, base_importance = excluded.base_importance",
    )
    .bind(&ent.id)
    .bind(&ent.r#type)
    .bind(&ent.name)
    .bind(&props)
    .bind(ent.confidence)
    .bind(&ent.source_note_id)
    .bind(ent.created_at)
    .bind(ent.updated_at)
    .bind(ent.strength)
    .bind(ent.access_count)
    .bind(ent.last_accessed)
    .bind(ent.base_importance)
    .execute(db.pool())
    .await?;
    Ok(true)
}

async fn insert_relation_if_absent(
    db: &Db,
    rel: &crate::bundle::types::RelationRow,
) -> Result<bool> {
    let existing: Option<(String,)> = sqlx::query_as("SELECT id FROM relations WHERE id = ?")
        .bind(&rel.id)
        .fetch_optional(db.pool())
        .await?;
    if existing.is_some() {
        return Ok(false);
    }
    let props = rel.properties.as_ref().map(|v| v.to_string());
    sqlx::query(
        "INSERT INTO relations (id, from_entity, to_entity, type, properties, confidence, \
                                 source_note_id, created_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&rel.id)
    .bind(&rel.from_entity)
    .bind(&rel.to_entity)
    .bind(&rel.r#type)
    .bind(&props)
    .bind(rel.confidence)
    .bind(&rel.source_note_id)
    .bind(rel.created_at)
    .execute(db.pool())
    .await?;
    Ok(true)
}

async fn upsert_theme_lww(db: &Db, theme: &crate::bundle::types::ThemeRow) -> Result<bool> {
    let existing: Option<(i64,)> =
        sqlx::query_as("SELECT last_consolidated_at FROM semantic_memories WHERE id = ?")
            .bind(&theme.id)
            .fetch_optional(db.pool())
            .await?;
    if let Some((local_ts,)) = existing {
        if theme.last_consolidated_at <= local_ts {
            return Ok(false);
        }
    }
    let evidence = serde_json::to_string(&theme.evidence)?;
    sqlx::query(
        "INSERT INTO semantic_memories \
           (id, topic, title, summary, evidence, confidence, created_at, last_consolidated_at, \
            last_accessed) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?) \
         ON CONFLICT(id) DO UPDATE SET \
             topic = excluded.topic, title = excluded.title, \
             summary = excluded.summary, evidence = excluded.evidence, \
             confidence = excluded.confidence, last_consolidated_at = excluded.last_consolidated_at",
    )
    .bind(&theme.id)
    .bind(&theme.topic)
    .bind(&theme.title)
    .bind(&theme.summary)
    .bind(&evidence)
    .bind(theme.confidence)
    .bind(theme.created_at)
    .bind(theme.last_consolidated_at)
    .bind(now_ms())
    .execute(db.pool())
    .await?;
    Ok(true)
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    hex::encode(h.finalize())
}
