//! Retrieval-signal logging (Phase 3d).
//!
//! Pure DB layer; no embeddings stored here. The Python training script reads
//! these rows offline, re-embeds the queries through the same gateway, and
//! produces a projection adapter saved alongside the embedding model.

use crate::db::{now_ms, Db};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use sqlx::Row;
use ulid::Ulid;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct RetrievalSignal {
    pub id: String,
    pub query_text: String,
    pub types_filter: Option<String>, // JSON
    pub returned_ids: String,         // JSON
    pub clicked_ids: String,          // JSON
    pub dismissed_ids: String,        // JSON
    pub corrected_ids: String,        // JSON
    pub embedding_model: Option<String>,
    pub created_at: i64,
    pub last_updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogRetrievalArgs {
    pub query_text: String,
    pub types_filter: Option<Vec<String>>,
    pub returned_ids: Vec<String>,
    pub embedding_model: Option<String>,
}

impl Db {
    pub async fn log_retrieval(&self, args: &LogRetrievalArgs) -> Result<String> {
        let id = Ulid::new().to_string();
        let now = now_ms();
        let types_json = args
            .types_filter
            .as_ref()
            .map(|v| serde_json::to_string(v).unwrap_or_else(|_| "[]".into()));
        let returned_json =
            serde_json::to_string(&args.returned_ids).unwrap_or_else(|_| "[]".into());

        sqlx::query(
            "INSERT INTO retrieval_signals \
               (id, query_text, types_filter, returned_ids, embedding_model, created_at, last_updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(&args.query_text)
        .bind(types_json)
        .bind(returned_json)
        .bind(&args.embedding_model)
        .bind(now)
        .bind(now)
        .execute(self.pool())
        .await?;
        Ok(id)
    }

    pub async fn record_action(
        &self,
        signal_id: &str,
        kind: SignalAction,
        entity_id: &str,
    ) -> Result<()> {
        // Read-modify-write because we're appending into a JSON column.
        // SQLite is single-writer with WAL so the race window is small; we
        // serialise within the function with a transaction.
        let mut tx = self.pool().begin().await?;
        let column = match kind {
            SignalAction::Click => "clicked_ids",
            SignalAction::Dismiss => "dismissed_ids",
            SignalAction::Correct => "corrected_ids",
        };
        let row = sqlx::query(&format!(
            "SELECT {column} FROM retrieval_signals WHERE id = ?"
        ))
        .bind(signal_id)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(row) = row else {
            return Err(anyhow::anyhow!("signal id not found: {}", signal_id));
        };
        let current: String = row.try_get(column).unwrap_or_else(|_| "[]".into());
        let mut ids: Vec<String> = serde_json::from_str(&current).unwrap_or_default();
        if !ids.iter().any(|x| x == entity_id) {
            ids.push(entity_id.to_string());
        }
        let new_json = serde_json::to_string(&ids)?;
        sqlx::query(&format!(
            "UPDATE retrieval_signals SET {column} = ?, last_updated_at = ? WHERE id = ?"
        ))
        .bind(new_json)
        .bind(now_ms())
        .bind(signal_id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn list_signals(&self, limit: i64) -> Result<Vec<RetrievalSignal>> {
        let rows = sqlx::query_as::<_, RetrievalSignal>(
            "SELECT id, query_text, types_filter, returned_ids, clicked_ids, dismissed_ids, \
                    corrected_ids, embedding_model, created_at, last_updated_at \
             FROM retrieval_signals ORDER BY created_at DESC LIMIT ?",
        )
        .bind(limit)
        .fetch_all(self.pool())
        .await?;
        Ok(rows)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignalAction {
    Click,
    Dismiss,
    Correct,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn log_and_record_round_trip() {
        let dir = tempdir().unwrap();
        let db = Db::open(&dir.path().join("test.db")).await.unwrap();

        let sig_id = db
            .log_retrieval(&LogRetrievalArgs {
                query_text: "coffee".into(),
                types_filter: Some(vec!["Preference".into()]),
                returned_ids: vec!["e1".into(), "e2".into(), "e3".into()],
                embedding_model: Some("openai/text-embedding-3-small".into()),
            })
            .await
            .unwrap();

        db.record_action(&sig_id, SignalAction::Click, "e2")
            .await
            .unwrap();
        db.record_action(&sig_id, SignalAction::Click, "e2")
            .await
            .unwrap(); // idempotent
        db.record_action(&sig_id, SignalAction::Dismiss, "e3")
            .await
            .unwrap();
        db.record_action(&sig_id, SignalAction::Correct, "e1")
            .await
            .unwrap();

        let all = db.list_signals(10).await.unwrap();
        assert_eq!(all.len(), 1);
        let s = &all[0];

        let clicked: Vec<String> = serde_json::from_str(&s.clicked_ids).unwrap();
        let dismissed: Vec<String> = serde_json::from_str(&s.dismissed_ids).unwrap();
        let corrected: Vec<String> = serde_json::from_str(&s.corrected_ids).unwrap();

        assert_eq!(clicked, vec!["e2"], "duplicate click must be deduped");
        assert_eq!(dismissed, vec!["e3"]);
        assert_eq!(corrected, vec!["e1"]);
    }
}
