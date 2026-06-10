//! Background tokio worker that runs consolidation on a cadence.
//!
//! Cadence is `CAIRN_CONSOLIDATION_INTERVAL_SECS` (default 900 s).
//! Set to 0 to disable. The first run fires after one interval, not immediately,
//! so the first window has a chance to accumulate entities.

use crate::consolidation::run::run_consolidation;
use crate::db::Db;
use crate::extract::openai_compat::OpenAiCompatConfig;
use std::time::Duration;

pub fn spawn_worker(db: Db, client: reqwest::Client, cfg: OpenAiCompatConfig) {
    let interval = std::env::var("CAIRN_CONSOLIDATION_INTERVAL_SECS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(900);
    if interval == 0 {
        tracing::info!("consolidation worker disabled (interval=0)");
        return;
    }
    tracing::info!(interval_secs = interval, "consolidation worker spawned");
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(interval));
        // Skip the immediate first tick.
        ticker.tick().await;
        loop {
            ticker.tick().await;
            match run_consolidation(&db, &client, &cfg, "scheduled").await {
                Ok(stats) => tracing::info!(
                    topics = stats.topics_scanned,
                    created = stats.semantic_created,
                    updated = stats.semantic_updated,
                    errors = stats.errors.len(),
                    "consolidation run done"
                ),
                Err(e) => tracing::error!(?e, "consolidation run failed"),
            }
            // Piggy-back the stale-entity sweep on the same cadence. The
            // sweep is cheap (a single UPDATE) and only touches entities
            // the user has never edited / accessed for ~6 months, so
            // running it every tick is harmless.
            match db.auto_archive_stale_entities().await {
                Ok(0) => {}
                Ok(n) => tracing::info!(archived = n, "auto-archived stale entities"),
                Err(e) => tracing::warn!(?e, "auto_archive_stale_entities tick failed"),
            }
        }
    });
}
