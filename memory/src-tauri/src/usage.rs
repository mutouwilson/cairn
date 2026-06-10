//! Cost + latency telemetry (Phase 4c).
//!
//! A process-global `OnceLock<Db>` is installed at `AppState::init` and the
//! LLM / embedding call sites push `UsageEvent`s into it. Records are written
//! asynchronously from a detached task so the recording path never blocks the
//! call that generated the event.
//!
//! Gateway-attributed costs come straight from `response.usage.cost` (the
//! Vercel Gateway always returns it). Anthropic-direct and local providers
//! leave `cost_micro_usd` NULL — a future price-table lookup can fill that in.

use crate::db::{now_ms, Db};
use anyhow::Result;
use once_cell::sync::OnceCell;
use serde::{Deserialize, Serialize};
use sqlx::Row;
use ulid::Ulid;

static SINK: OnceCell<Db> = OnceCell::new();

pub fn install(db: Db) {
    let _ = SINK.set(db);
}

/// Push an event into the global sink. Returns immediately; the write happens
/// in a detached tokio task so a slow DB never adds to user-visible latency.
pub fn record(event: UsageEvent) {
    let Some(db) = SINK.get() else {
        return;
    };
    let db = db.clone();
    tokio::spawn(async move {
        if let Err(e) = db.log_usage(&event).await {
            tracing::warn!(?e, "usage log write failed");
        }
    });
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageEvent {
    pub operation: &'static str, // extract | embed | consolidate
    pub provider: String,
    pub model: String,
    pub prompt_tokens: Option<i64>,
    pub completion_tokens: Option<i64>,
    /// Micro-USD: 1_000_000 = $1.00.
    pub cost_micro_usd: Option<i64>,
    pub latency_ms: i64,
    pub success: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct UsageRow {
    pub id: String,
    pub operation: String,
    pub provider: String,
    pub model: String,
    pub prompt_tokens: Option<i64>,
    pub completion_tokens: Option<i64>,
    pub cost_micro_usd: Option<i64>,
    pub latency_ms: i64,
    pub success: i64,
    pub error: Option<String>,
    pub occurred_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageDaySummary {
    pub day: String, // YYYY-MM-DD UTC
    pub operation: String,
    pub provider: String,
    pub calls: i64,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub cost_micro_usd: i64,
    pub p50_latency_ms: i64,
}

impl Db {
    pub async fn log_usage(&self, e: &UsageEvent) -> Result<()> {
        sqlx::query(
            "INSERT INTO usage_log \
               (id, operation, provider, model, prompt_tokens, completion_tokens, \
                cost_micro_usd, latency_ms, success, error, occurred_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(Ulid::new().to_string())
        .bind(e.operation)
        .bind(&e.provider)
        .bind(&e.model)
        .bind(e.prompt_tokens)
        .bind(e.completion_tokens)
        .bind(e.cost_micro_usd)
        .bind(e.latency_ms)
        .bind(if e.success { 1_i64 } else { 0_i64 })
        .bind(&e.error)
        .bind(now_ms())
        .execute(self.pool())
        .await?;
        Ok(())
    }

    pub async fn list_usage(&self, limit: i64) -> Result<Vec<UsageRow>> {
        let rows = sqlx::query_as::<_, UsageRow>(
            "SELECT id, operation, provider, model, prompt_tokens, completion_tokens, \
                    cost_micro_usd, latency_ms, success, error, occurred_at \
             FROM usage_log ORDER BY occurred_at DESC LIMIT ?",
        )
        .bind(limit)
        .fetch_all(self.pool())
        .await?;
        Ok(rows)
    }

    /// Per-day × operation × provider rollup over the last `days` days.
    pub async fn usage_summary(&self, days: i64) -> Result<Vec<UsageDaySummary>> {
        // SQLite's `strftime('%Y-%m-%d', t/1000, 'unixepoch')` is reliable for ms timestamps.
        let rows = sqlx::query(
            "WITH bucketed AS ( \
                 SELECT strftime('%Y-%m-%d', occurred_at/1000, 'unixepoch') AS day, \
                        operation, provider, prompt_tokens, completion_tokens, \
                        cost_micro_usd, latency_ms \
                 FROM usage_log \
                 WHERE occurred_at >= ((strftime('%s','now') - ? * 86400) * 1000) \
             ) \
             SELECT day, operation, provider, \
                    COUNT(*) AS calls, \
                    COALESCE(SUM(prompt_tokens), 0)     AS pt, \
                    COALESCE(SUM(completion_tokens), 0) AS ct, \
                    COALESCE(SUM(cost_micro_usd), 0)    AS cost_micro, \
                    -- crude median via avg-of-ordered-window is too heavy in pure SQL;
                    -- we approximate with avg until we need true p50.
                    CAST(AVG(latency_ms) AS INTEGER) AS p50 \
             FROM bucketed \
             GROUP BY day, operation, provider \
             ORDER BY day DESC, operation, provider",
        )
        .bind(days)
        .fetch_all(self.pool())
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| UsageDaySummary {
                day: r.try_get("day").unwrap_or_default(),
                operation: r.try_get("operation").unwrap_or_default(),
                provider: r.try_get("provider").unwrap_or_default(),
                calls: r.try_get("calls").unwrap_or(0),
                prompt_tokens: r.try_get("pt").unwrap_or(0),
                completion_tokens: r.try_get("ct").unwrap_or(0),
                cost_micro_usd: r.try_get("cost_micro").unwrap_or(0),
                p50_latency_ms: r.try_get("p50").unwrap_or(0),
            })
            .collect())
    }
}

/// Helper used by call sites to parse `usage.cost` (USD, float) into our
/// integer micro-USD representation without lossy rounding for tiny costs.
pub fn usd_to_micro(usd: f64) -> Option<i64> {
    if !usd.is_finite() || usd < 0.0 {
        return None;
    }
    Some((usd * 1_000_000.0).round() as i64)
}
