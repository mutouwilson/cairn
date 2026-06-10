//! Core domain types — kept in sync with `migrations/20260513000001_initial.sql`.
//!
//! These are the canonical Rust representations of every entity in the system.
//! All TS types in `src/lib/types.ts` mirror these exactly.

use serde::{Deserialize, Serialize};
use std::fmt;

/// 8 entity types. Adding a new one requires:
///  1. add variant here
///  2. add to `ALL` array
///  3. add prompt example in `extract/prompt.rs`
///  4. document in `docs/SCHEMA.md`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EntityType {
    Person,
    Event,
    Preference,
    Belief,
    Goal,
    Asset,
    Skill,
    Location,
}

impl EntityType {
    pub const ALL: [EntityType; 8] = [
        Self::Person,
        Self::Event,
        Self::Preference,
        Self::Belief,
        Self::Goal,
        Self::Asset,
        Self::Skill,
        Self::Location,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Person => "Person",
            Self::Event => "Event",
            Self::Preference => "Preference",
            Self::Belief => "Belief",
            Self::Goal => "Goal",
            Self::Asset => "Asset",
            Self::Skill => "Skill",
            Self::Location => "Location",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|t| t.as_str() == s)
    }

    /// Sensitivity level used for default permission policy.
    /// Higher = more sensitive = stricter default.
    pub fn sensitivity(&self) -> u8 {
        match self {
            Self::Preference | Self::Skill => 1,
            Self::Goal | Self::Location | Self::Asset => 2,
            Self::Event => 3,
            Self::Person | Self::Belief => 4,
        }
    }
}

impl fmt::Display for EntityType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Note {
    pub id: String,
    pub text: String,
    pub source: String,
    pub created_at: i64,
    pub processed_at: Option<i64>,
    pub processing_status: String,
    pub extraction_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Entity {
    pub id: String,
    #[sqlx(rename = "type")]
    pub r#type: String,
    pub name: String,
    pub properties: String, // JSON string; decode on read in client layer
    pub confidence: f64,
    pub source_note_id: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    pub strength: f64,
    pub last_accessed: i64,
    pub access_count: i64,
    pub base_importance: f64,
    /// 0 / 1 — when `1`, the entity survives cascade-delete from its source
    /// note (manual edit / merge keep-side / manual create). Added in
    /// `20260521000002_entity_sticky.sql`.
    #[serde(default)]
    pub is_sticky: i64,
    /// Soft-archive timestamp (ms). `NULL` when active. Added in
    /// `20260520000002_archive.sql`. Archived entities are excluded from
    /// retrieval but remain reachable via the admin restore path.
    #[serde(default)]
    pub archived_at: Option<i64>,
}

impl Entity {
    pub fn properties_value(&self) -> serde_json::Value {
        serde_json::from_str(&self.properties).unwrap_or(serde_json::json!({}))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Relation {
    pub id: String,
    pub from_entity: String,
    pub to_entity: String,
    #[sqlx(rename = "type")]
    pub r#type: String,
    pub properties: Option<String>,
    pub confidence: f64,
    pub source_note_id: Option<String>,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Agent {
    pub id: String,
    pub display_name: String,
    pub public_key: Option<String>,
    pub added_at: i64,
    pub last_seen: Option<i64>,
    pub revoked_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Permission {
    pub agent_id: String,
    pub entity_type: String,
    pub scope: String, // read | write | none
    pub granted_at: i64,
    pub expires_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct AuditEntry {
    pub seq: i64,
    pub id: String,
    pub agent_id: String,
    pub action: String,
    pub tool_name: Option<String>,
    pub arguments: Option<String>,
    pub result_hash: String,
    pub result_count: Option<i64>,
    pub timestamp: i64,
    pub prev_hash: String,
    pub hash: String,
    pub signature: String,
}

// ---------------- Draft types used by the extraction layer ----------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedEntity {
    #[serde(rename = "type")]
    pub r#type: String,
    pub name: String,
    #[serde(default)]
    pub properties: serde_json::Value,
    pub confidence: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedRelation {
    pub from: String,
    pub to: String,
    #[serde(rename = "type")]
    pub r#type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractionResult {
    #[serde(default)]
    pub entities: Vec<ExtractedEntity>,
    #[serde(default)]
    pub relations: Vec<ExtractedRelation>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entity_type_roundtrip() {
        for t in EntityType::ALL {
            assert_eq!(EntityType::parse(t.as_str()), Some(t));
        }
    }

    #[test]
    fn entity_type_sensitivity_ordering() {
        assert!(EntityType::Preference.sensitivity() < EntityType::Person.sensitivity());
    }
}
