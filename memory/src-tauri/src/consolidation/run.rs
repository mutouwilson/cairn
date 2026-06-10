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
        match consolidate_topic(db, client, cfg, &cand).await {
            Ok(created) => {
                if created {
                    stats.semantic_created += 1;
                } else {
                    stats.semantic_updated += 1;
                }
            }
            Err(e) => {
                tracing::warn!(topic = %cand.key, error = %e, "topic consolidation failed");
                stats.errors.push(format!("{}: {}", cand.key, e));
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
            if s.len() > 60 {
                format!("\"{}…\"", &s[..60])
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
    let mut tx = db.pool().begin().await?;
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
    Ok(true)
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
