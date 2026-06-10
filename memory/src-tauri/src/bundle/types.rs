//! Wire format types for the cairn-bundle JSON payload. Versioned so future
//! formats can coexist; the `format` field is validated on import.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bundle {
    pub header: BundleHeader,
    pub notes: Vec<NoteRow>,
    pub entities: Vec<EntityRow>,
    pub relations: Vec<RelationRow>,
    pub themes: Vec<ThemeRow>,
    /// Base64 Ed25519 signature of the canonical-JSON-encoded {header + payload}.
    /// Empty string for unsigned bundles produced in degraded mode.
    pub signature: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundleHeader {
    pub format: String,
    pub bundle_id: String,
    pub created_at: i64,
    pub device_name: String,
    pub device_pubkey: String, // base64 of exporter's Ed25519 verifying key
    pub counts: BundleCounts,
    pub checksum: String, // SHA-256 hex of the payload (notes+entities+relations+themes)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundleCounts {
    pub notes: u32,
    pub entities: u32,
    pub relations: u32,
    pub themes: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NoteRow {
    pub id: String,
    pub text: String,
    pub source: String,
    pub created_at: i64,
    pub processed_at: Option<i64>,
    pub processing_status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityRow {
    pub id: String,
    #[serde(rename = "type")]
    pub r#type: String,
    pub name: String,
    pub properties: serde_json::Value,
    pub confidence: f64,
    pub source_note_id: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    pub strength: f64,
    pub access_count: i64,
    pub last_accessed: i64,
    pub base_importance: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelationRow {
    pub id: String,
    pub from_entity: String,
    pub to_entity: String,
    #[serde(rename = "type")]
    pub r#type: String,
    pub properties: Option<serde_json::Value>,
    pub confidence: f64,
    pub source_note_id: Option<String>,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThemeRow {
    pub id: String,
    pub topic: String,
    pub title: String,
    pub summary: String,
    pub evidence: Vec<String>,
    pub confidence: f64,
    pub created_at: i64,
    pub last_consolidated_at: i64,
}
