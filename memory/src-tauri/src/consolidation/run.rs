//! Single consolidation invocation. Idempotent: writes one row to
//! `consolidation_runs`, then per-topic upserts to `semantic_memories`.

use crate::consolidation::summarize::{summarise, SummaryRequest};
use crate::consolidation::topics::{self, TopicCandidate};
use crate::consolidation::ConsolidationStats;
use crate::db::{now_ms, Db};
use crate::extract::openai_compat::OpenAiCompatConfig;
use crate::schema::Entity;
use anyhow::Result;
use serde_json::Value;
use ulid::Ulid;

pub async fn run_consolidation(
    db: &Db,
    client: &reqwest::Client,
    cfg: &OpenAiCompatConfig,
    trigger: &str,
) -> Result<ConsolidationStats> {
    let run_id = Ulid::new().to_string();
    let started_at = now_ms();
    // Finalize rows a previous process left in 'running' (crash / quit / the
    // historic panic path) so the history never accumulates zombies. A truly
    // concurrent live run self-heals: its finish_run overwrites this status.
    sqlx::query(
        "UPDATE consolidation_runs SET status = 'aborted', finished_at = ?, \
           error = COALESCE(error, 'interrupted — process exited mid-run') \
         WHERE status = 'running'",
    )
    .bind(started_at)
    .execute(db.pool())
    .await?;
    sqlx::query(
        "INSERT INTO consolidation_runs (id, started_at, trigger, status) VALUES (?, ?, ?, 'running')",
    )
    .bind(&run_id)
    .bind(started_at)
    .bind(trigger)
    .execute(db.pool())
    .await?;

    let mut stats = ConsolidationStats {
        run_id: run_id.clone(),
        topics_scanned: 0,
        semantic_created: 0,
        semantic_updated: 0,
        errors: vec![],
    };

    let topics = match topics::discover(db).await {
        Ok(t) => t,
        Err(e) => {
            finish_run(db, &run_id, &stats, Some(&e.to_string()), "failed").await?;
            return Err(e);
        }
    };

    for cand in topics {
        stats.topics_scanned += 1;
        // Each topic runs in its own task: a panic must fail THIS topic and
        // surface in `errors`, not silently kill the whole run — that's how
        // runs used to wedge in status='running' forever.
        let t_db = db.clone();
        let t_client = client.clone();
        let t_cfg = cfg.clone();
        let t_cand = cand.clone();
        let joined =
            tokio::spawn(async move { consolidate_topic(&t_db, &t_client, &t_cfg, &t_cand).await })
                .await;
        match joined {
            Ok(Ok(created)) => {
                if created {
                    stats.semantic_created += 1;
                } else {
                    stats.semantic_updated += 1;
                }
            }
            Ok(Err(e)) => {
                tracing::warn!(topic = %cand.key, error = %e, "topic consolidation failed");
                stats.errors.push(format!("{}: {}", cand.key, e));
            }
            Err(join_err) => {
                tracing::error!(topic = %cand.key, "topic task panicked: {join_err}");
                stats
                    .errors
                    .push(format!("{}: panicked: {}", cand.key, join_err));
            }
        }
    }

    finish_run(db, &run_id, &stats, None, "done").await?;
    Ok(stats)
}

async fn consolidate_topic(
    db: &Db,
    client: &reqwest::Client,
    cfg: &OpenAiCompatConfig,
    cand: &TopicCandidate,
) -> Result<bool> {
    let snippets: Vec<String> = cand.evidence.iter().map(format_evidence_snippet).collect();
    let req = SummaryRequest {
        topic_key: cand.key.clone(),
        topic_title: cand.title.clone(),
        center_kind: cand.center_entity.as_ref().map(|e| e.r#type.clone()),
        center_name: cand.center_entity.as_ref().map(|e| e.name.clone()),
        evidence_snippets: snippets,
    };
    let resp = summarise(client, cfg, &req).await?;
    let evidence_ids: Vec<String> = cand.evidence.iter().map(|e| e.id.clone()).collect();
    upsert_semantic(
        db,
        &cand.key,
        &resp.title,
        &resp.summary,
        resp.confidence,
        &evidence_ids,
    )
    .await
}

fn format_evidence_snippet(e: &Entity) -> String {
    let props: Value = serde_json::from_str(&e.properties).unwrap_or(Value::Null);
    let mut s = format!("[{}] {}", e.r#type, e.name);
    if let Some(obj) = props.as_object() {
        let bits: Vec<String> = obj
            .iter()
            .take(4)
            .map(|(k, v)| format!("{}={}", k, value_brief(v)))
            .collect();
        if !bits.is_empty() {
            s.push_str(&format!(" ({})", bits.join(", ")));
        }
    }
    s
}

fn value_brief(v: &Value) -> String {
    match v {
        Value::Null => "null".into(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => {
            // Truncate on a char boundary. Byte-slicing (`&s[..60]`) panics
            // the moment a multi-byte char straddles the cut — Chinese
            // property values hit this constantly and the panic silently
            // killed the whole consolidation task.
            if s.chars().count() > 30 {
                let head: String = s.chars().take(30).collect();
                format!("\"{}…\"", head)
            } else {
                format!("\"{}\"", s)
            }
        }
        other => other.to_string(),
    }
}

/// Upsert a semantic memory. Returns `Ok(true)` if a new row was created,
/// `Ok(false)` if an existing active row was updated. Older versions of the
/// same topic remain in the table and are linked via `superseded_by`.
async fn upsert_semantic(
    db: &Db,
    topic: &str,
    title: &str,
    summary: &str,
    confidence: f64,
    evidence_ids: &[String],
) -> Result<bool> {
    let evidence_json = serde_json::to_string(evidence_ids)?;
    let now = now_ms();

    let existing: Option<(String,)> = sqlx::query_as(
        "SELECT id FROM semantic_memories WHERE topic = ? AND superseded_by IS NULL",
    )
    .bind(topic)
    .fetch_optional(db.pool())
    .await?;

    let new_id = Ulid::new().to_string();
    let created = existing.is_none();
    let mut tx = db.pool().begin().await?;
    // Superseding is wedged both ways under immediate checks: the partial
    // unique index (one active row per topic) rejects INSERT-before-UPDATE,
    // and the superseded_by → id foreign key rejects UPDATE-before-INSERT
    // (the new id doesn't exist yet). Defer FK enforcement to COMMIT, then
    // run UPDATE → INSERT: at commit time the old row is inactive (unique
    // index happy) and the new id exists (FK happy). Re-consolidation of an
    // existing topic was silently failing on one of the two constraints
    // since the feature shipped.
    sqlx::query("PRAGMA defer_foreign_keys = ON")
        .execute(&mut *tx)
        .await?;
    if let Some((old_id,)) = existing {
        sqlx::query("UPDATE semantic_memories SET superseded_by = ? WHERE id = ?")
            .bind(&new_id)
            .bind(&old_id)
            .execute(&mut *tx)
            .await?;
    }
    sqlx::query(
        "INSERT INTO semantic_memories \
           (id, topic, title, summary, evidence, confidence, created_at, last_consolidated_at, \
            last_accessed) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&new_id)
    .bind(topic)
    .bind(title)
    .bind(summary)
    .bind(&evidence_json)
    .bind(confidence)
    .bind(now)
    .bind(now)
    .bind(now)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(created)
}

async fn finish_run(
    db: &Db,
    run_id: &str,
    stats: &ConsolidationStats,
    error: Option<&str>,
    status: &str,
) -> Result<()> {
    sqlx::query(
        "UPDATE consolidation_runs SET \
           finished_at = ?, topics_scanned = ?, semantic_created = ?, semantic_updated = ?, \
           error = ?, status = ? \
         WHERE id = ?",
    )
    .bind(now_ms())
    .bind(stats.topics_scanned)
    .bind(stats.semantic_created)
    .bind(stats.semantic_updated)
    .bind(error)
    .bind(status)
    .bind(run_id)
    .execute(db.pool())
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    /// Re-consolidating an existing topic was wedged both ways (partial
    /// unique index vs superseded_by FK) until upsert deferred FK checks.
    /// Locks the supersede chain + the created/updated return semantics.
    #[tokio::test]
    async fn upsert_supersedes_existing_topic() {
        let dir = tempdir().unwrap();
        let db = Db::open(&dir.path().join("test.db")).await.unwrap();

        let created = upsert_semantic(&db, "person:妈", "v1", "first summary", 0.9, &[])
            .await
            .unwrap();
        assert!(created, "first upsert must report created");

        let created = upsert_semantic(&db, "person:妈", "v2", "second summary", 0.95, &[])
            .await
            .unwrap();
        assert!(!created, "second upsert must report updated, not created");

        let (active, total): (i64, i64) = (
            sqlx::query_scalar(
                "SELECT COUNT(*) FROM semantic_memories WHERE superseded_by IS NULL",
            )
            .fetch_one(db.pool())
            .await
            .unwrap(),
            sqlx::query_scalar("SELECT COUNT(*) FROM semantic_memories")
                .fetch_one(db.pool())
                .await
                .unwrap(),
        );
        assert_eq!(active, 1, "exactly one active row per topic");
        assert_eq!(total, 2, "old version kept in the chain");

        let (title, chained): (String, i64) = sqlx::query_as(
            "SELECT a.title, EXISTS(
                 SELECT 1 FROM semantic_memories old
                 WHERE old.superseded_by = a.id
             )
             FROM semantic_memories a WHERE a.superseded_by IS NULL",
        )
        .fetch_one(db.pool())
        .await
        .unwrap();
        assert_eq!(title, "v2", "active row is the new version");
        assert_eq!(chained, 1, "old row points at the new one");
    }

    /// `&s[..60]` byte-slicing panicked mid-char on CJK values and silently
    /// killed whole consolidation runs.
    #[test]
    fn value_brief_truncates_cjk_on_char_boundary() {
        let long_cjk = "记".repeat(31);
        let out = value_brief(&Value::String(long_cjk));
        assert!(out.ends_with("…\""), "long values get an ellipsis: {out}");
        assert_eq!(out.chars().filter(|c| *c == '记').count(), 30);

        let short = value_brief(&Value::String("短值".into()));
        assert_eq!(short, "\"短值\"");
    }
}
