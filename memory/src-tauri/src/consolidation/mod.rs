//! Hierarchical memory: turns clusters of episodic entities into stable
//! semantic summaries on a cadence.
//!
//! See `migrations/20260513000003_hierarchical_memory.sql` for the storage
//! design, and `docs/ARCHITECTURE.md` for the why.
//!
//! Components:
//!   - `topics`: discover which episodic clusters are ripe for consolidation.
//!   - `summarize`: schema-constrained LLM call producing one summary per topic.
//!   - `run`: orchestrator. Each invocation writes one `consolidation_runs` row.
//!   - `worker`: tokio background task that calls `run` on a cadence.

pub mod run;
pub mod summarize;
pub mod topics;
pub mod worker;

pub use run::run_consolidation;
pub use worker::spawn_worker;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsolidationStats {
    pub run_id: String,
    pub topics_scanned: i64,
    pub semantic_created: i64,
    pub semantic_updated: i64,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct SemanticMemory {
    pub id: String,
    pub topic: String,
    pub title: String,
    pub summary: String,
    pub evidence: String, // JSON array of entity ids
    pub confidence: f64,
    pub created_at: i64,
    pub last_consolidated_at: i64,
    pub superseded_by: Option<String>,
    pub strength: f64,
    pub last_accessed: i64,
    pub access_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct ConsolidationRun {
    pub id: String,
    pub started_at: i64,
    pub finished_at: Option<i64>,
    pub trigger: String,
    pub topics_scanned: i64,
    pub semantic_created: i64,
    pub semantic_updated: i64,
    pub error: Option<String>,
    pub status: String,
}
