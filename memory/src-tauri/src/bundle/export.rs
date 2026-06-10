//! Export the current DB state as a signed cairn-bundle.

use crate::audit::AuditLogger;
use crate::bundle::types::*;
use crate::bundle::BUNDLE_FORMAT;
use crate::db::{now_ms, Db};
use anyhow::{Context, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use ed25519_dalek::Signer;
use sha2::{Digest, Sha256};
use sqlx::Row;
use ulid::Ulid;

pub async fn export(db: &Db, audit: &AuditLogger, device_name: &str) -> Result<Bundle> {
    let notes = collect_notes(db).await?;
    let entities = collect_entities(db).await?;
    let relations = collect_relations(db).await?;
    let themes = collect_themes(db).await?;

    let counts = BundleCounts {
        notes: notes.len() as u32,
        entities: entities.len() as u32,
        relations: relations.len() as u32,
        themes: themes.len() as u32,
    };

    // Canonical payload = JSON of (notes, entities, relations, themes) with
    // keys sorted at the outer level. Inner JSON is `serde_json` default which
    // is already stable for our concrete types.
    let payload = serde_json::json!({
        "entities": &entities,
        "notes": &notes,
        "relations": &relations,
        "themes": &themes,
    });
    let payload_bytes = serde_json::to_vec(&payload)?;
    let checksum = sha256_hex(&payload_bytes);

    let header = BundleHeader {
        format: BUNDLE_FORMAT.to_string(),
        bundle_id: Ulid::new().to_string(),
        created_at: now_ms(),
        device_name: device_name.to_string(),
        device_pubkey: audit.public_key_b64(),
        counts: counts.clone(),
        checksum: checksum.clone(),
    };

    // Sign canonical (header + checksum) so trivial header tampering breaks the sig.
    let header_bytes = serde_json::to_vec(&header)?;
    let mut hasher = Sha256::new();
    hasher.update(&header_bytes);
    hasher.update(&payload_bytes);
    let to_sign = hasher.finalize();
    let signature = audit.sign_bytes(&to_sign);

    db.insert_sync_log(
        "export",
        &header.bundle_id,
        Some(device_name),
        Some(&audit.public_key_b64()),
        &counts,
        &checksum,
        None, // signature_ok N/A for exports
        0,
        None,
    )
    .await
    .context("insert export sync_log")?;

    Ok(Bundle {
        header,
        notes,
        entities,
        relations,
        themes,
        signature,
    })
}

async fn collect_notes(db: &Db) -> Result<Vec<NoteRow>> {
    let rows = sqlx::query(
        "SELECT id, text, source, created_at, processed_at, processing_status \
         FROM notes ORDER BY created_at",
    )
    .fetch_all(db.pool())
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| NoteRow {
            id: r.try_get("id").unwrap_or_default(),
            text: r.try_get("text").unwrap_or_default(),
            source: r.try_get("source").unwrap_or_default(),
            created_at: r.try_get("created_at").unwrap_or(0),
            processed_at: r.try_get("processed_at").ok(),
            processing_status: r.try_get("processing_status").unwrap_or_default(),
        })
        .collect())
}

async fn collect_entities(db: &Db) -> Result<Vec<EntityRow>> {
    let rows = sqlx::query(
        "SELECT id, type, name, properties, confidence, source_note_id, created_at, \
                updated_at, strength, access_count, last_accessed, base_importance \
         FROM entities ORDER BY created_at",
    )
    .fetch_all(db.pool())
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| {
            let props_str: String = r.try_get("properties").unwrap_or_else(|_| "{}".into());
            EntityRow {
                id: r.try_get("id").unwrap_or_default(),
                r#type: r.try_get("type").unwrap_or_default(),
                name: r.try_get("name").unwrap_or_default(),
                properties: serde_json::from_str(&props_str).unwrap_or(serde_json::Value::Null),
                confidence: r.try_get("confidence").unwrap_or(0.0),
                source_note_id: r.try_get("source_note_id").ok(),
                created_at: r.try_get("created_at").unwrap_or(0),
                updated_at: r.try_get("updated_at").unwrap_or(0),
                strength: r.try_get("strength").unwrap_or(1.0),
                access_count: r.try_get("access_count").unwrap_or(1),
                last_accessed: r.try_get("last_accessed").unwrap_or(0),
                base_importance: r.try_get("base_importance").unwrap_or(0.5),
            }
        })
        .collect())
}

async fn collect_relations(db: &Db) -> Result<Vec<RelationRow>> {
    let rows = sqlx::query(
        "SELECT id, from_entity, to_entity, type, properties, confidence, \
                source_note_id, created_at \
         FROM relations ORDER BY created_at",
    )
    .fetch_all(db.pool())
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| {
            let props: Option<String> = r.try_get("properties").ok();
            RelationRow {
                id: r.try_get("id").unwrap_or_default(),
                from_entity: r.try_get("from_entity").unwrap_or_default(),
                to_entity: r.try_get("to_entity").unwrap_or_default(),
                r#type: r.try_get("type").unwrap_or_default(),
                properties: props.and_then(|s| serde_json::from_str(&s).ok()),
                confidence: r.try_get("confidence").unwrap_or(1.0),
                source_note_id: r.try_get("source_note_id").ok(),
                created_at: r.try_get("created_at").unwrap_or(0),
            }
        })
        .collect())
}

async fn collect_themes(db: &Db) -> Result<Vec<ThemeRow>> {
    let rows = sqlx::query(
        "SELECT id, topic, title, summary, evidence, confidence, created_at, last_consolidated_at \
         FROM semantic_memories WHERE superseded_by IS NULL ORDER BY created_at",
    )
    .fetch_all(db.pool())
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| {
            let ev_str: String = r.try_get("evidence").unwrap_or_else(|_| "[]".into());
            ThemeRow {
                id: r.try_get("id").unwrap_or_default(),
                topic: r.try_get("topic").unwrap_or_default(),
                title: r.try_get("title").unwrap_or_default(),
                summary: r.try_get("summary").unwrap_or_default(),
                evidence: serde_json::from_str(&ev_str).unwrap_or_default(),
                confidence: r.try_get("confidence").unwrap_or(0.0),
                created_at: r.try_get("created_at").unwrap_or(0),
                last_consolidated_at: r.try_get("last_consolidated_at").unwrap_or(0),
            }
        })
        .collect())
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    hex::encode(h.finalize())
}

// Sign helper lives on AuditLogger because the key already does.
impl crate::audit::AuditLogger {
    pub fn sign_bytes(&self, msg: &[u8]) -> String {
        let sig = self.signing_for_export().sign(msg);
        B64.encode(sig.to_bytes())
    }
}
