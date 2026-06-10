//! Integration tests for the bundle pipeline.
//! Two empty DBs, write data into A, export → import into B, verify.

use crate::audit::AuditLogger;
use crate::bundle::{export::export, import::import};
use crate::db::Db;
use crate::keystore::{db_fallback::DbKeyStore, KeyStoreService};
use crate::schema::{ExtractedEntity, ExtractedRelation, ExtractionResult};
use base64::Engine as _;
use serde_json::json;
use std::sync::Arc;
use tempfile::tempdir;

async fn db_only_audit(db: Db) -> AuditLogger {
    let store = Arc::new(DbKeyStore::new(db.clone()));
    let service = KeyStoreService::new(store.clone(), store);
    AuditLogger::init_with_service(db, service).await.unwrap()
}

async fn seed_db(db: &Db) {
    let note_id = db
        .insert_note("ate ramen with mom", "manual")
        .await
        .unwrap();
    db.save_extracted(
        &note_id,
        &ExtractionResult {
            entities: vec![
                ExtractedEntity {
                    r#type: "Person".into(),
                    name: "mom".into(),
                    properties: json!({ "relationship": "mother" }),
                    confidence: 0.95,
                },
                ExtractedEntity {
                    r#type: "Preference".into(),
                    name: "ramen".into(),
                    properties: json!({ "domain": "food" }),
                    confidence: 0.9,
                },
            ],
            relations: vec![ExtractedRelation {
                from: "mom".into(),
                to: "ramen".into(),
                r#type: "ate_with".into(),
            }],
        },
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn roundtrip_export_import_preserves_data() {
    let dir_a = tempdir().unwrap();
    let db_a = Db::open(&dir_a.path().join("a.db")).await.unwrap();
    let audit_a = db_only_audit(db_a.clone()).await;
    seed_db(&db_a).await;

    let bundle = export(&db_a, &audit_a, "device-a").await.unwrap();
    assert_eq!(bundle.header.counts.notes, 1);
    assert_eq!(bundle.header.counts.entities, 2);
    assert_eq!(bundle.header.counts.relations, 1);

    let dir_b = tempdir().unwrap();
    let db_b = Db::open(&dir_b.path().join("b.db")).await.unwrap();
    let report = import(&db_b, &bundle, true).await.unwrap();

    assert!(
        report.signature_ok,
        "signature must verify across a clean roundtrip"
    );
    assert_eq!(report.imported_notes, 1);
    assert_eq!(report.imported_entities, 2);
    assert_eq!(report.imported_relations, 1);
    assert_eq!(report.conflicts, 0);
    assert!(
        report.errors.is_empty(),
        "no errors expected: {:?}",
        report.errors
    );

    // Verify the actual rows landed.
    let notes_b = db_b.list_notes(10).await.unwrap();
    let entities_b = db_b.list_entities(None, 100).await.unwrap();
    assert_eq!(notes_b.len(), 1);
    assert_eq!(entities_b.len(), 2);
    let names: Vec<_> = entities_b.iter().map(|e| e.name.clone()).collect();
    assert!(names.contains(&"mom".to_string()));
    assert!(names.contains(&"ramen".to_string()));
}

#[tokio::test]
async fn lww_keeps_newer_local_entity() {
    let dir_a = tempdir().unwrap();
    let db_a = Db::open(&dir_a.path().join("a.db")).await.unwrap();
    let audit_a = db_only_audit(db_a.clone()).await;
    seed_db(&db_a).await;
    let bundle = export(&db_a, &audit_a, "device-a").await.unwrap();

    // Set up B with the SAME entity ids (via an import first) so LWW can
    // actually fire on a per-id basis.
    let dir_b = tempdir().unwrap();
    let db_b = Db::open(&dir_b.path().join("b.db")).await.unwrap();
    let first = import(&db_b, &bundle, true).await.unwrap();
    assert_eq!(first.imported_entities, 2, "first import seeds B");

    // Now B "edits" its entities — bump updated_at far past A's snapshot.
    sqlx::query("UPDATE entities SET updated_at = updated_at + 999999")
        .execute(db_b.pool())
        .await
        .unwrap();

    // Re-import the same bundle. Every entity must lose LWW because B is newer.
    let second = import(&db_b, &bundle, true).await.unwrap();
    assert_eq!(
        second.imported_entities, 0,
        "all remote entities must lose LWW"
    );
    assert!(
        second.conflicts >= 2,
        "expected ≥2 LWW conflicts, got {}",
        second.conflicts
    );
}

#[tokio::test]
async fn tampered_payload_fails_checksum() {
    let dir_a = tempdir().unwrap();
    let db_a = Db::open(&dir_a.path().join("a.db")).await.unwrap();
    let audit_a = db_only_audit(db_a.clone()).await;
    seed_db(&db_a).await;
    let mut bundle = export(&db_a, &audit_a, "device-a").await.unwrap();

    // Tamper a note's text — checksum should mismatch on import.
    bundle.notes[0].text = "tampered".into();

    let dir_b = tempdir().unwrap();
    let db_b = Db::open(&dir_b.path().join("b.db")).await.unwrap();
    let err = import(&db_b, &bundle, true).await.unwrap_err();
    assert!(
        format!("{err}").contains("checksum"),
        "expected checksum failure, got: {err}"
    );
}

#[tokio::test]
async fn tampered_signature_does_not_verify() {
    let dir_a = tempdir().unwrap();
    let db_a = Db::open(&dir_a.path().join("a.db")).await.unwrap();
    let audit_a = db_only_audit(db_a.clone()).await;
    seed_db(&db_a).await;
    let mut bundle = export(&db_a, &audit_a, "device-a").await.unwrap();

    // Replace signature with a valid-length-but-wrong value (base64 of 64 zero bytes).
    bundle.signature = base64::engine::general_purpose::STANDARD.encode([0u8; 64]);

    let dir_b = tempdir().unwrap();
    let db_b = Db::open(&dir_b.path().join("b.db")).await.unwrap();
    let report = import(&db_b, &bundle, true).await.unwrap();
    assert!(
        !report.signature_ok,
        "fake signature must fail verification"
    );
    // Data still imports (we record the bad sig but don't refuse — the caller
    // decides what to do with `signature_ok=false`).
    assert!(report.imported_notes >= 1);
}
