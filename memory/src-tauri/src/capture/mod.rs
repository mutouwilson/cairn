//! Capture-surface plumbing (Phase 5).
//!
//! Each "capture source" is a row in the `capture_sources` table and a tokio
//! task spawned by `worker::spawn_all`. Two kinds today:
//!
//! - `imap` — poll an IMAP mailbox for messages in a configured folder.
//! - `ics`  — poll an iCalendar feed URL for `VEVENT` entries.
//!
//! The contract is narrow: each kind exposes an async `poll_once(db, source,
//! secret) -> Result<PollOutcome>` and the shared worker loop handles
//! cadence, dedupe (via `capture_seen`), status writes, and Cairn audit logging.

pub mod email_imap;
pub mod ics;
pub mod processor;
pub mod worker;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::db::{now_ms, Db};

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct CaptureSource {
    pub id: String,
    pub kind: String,
    pub label: String,
    pub config: String, // JSON
    pub secret_account: Option<String>,
    pub enabled: i64,
    pub poll_interval_secs: i64,
    pub last_polled_at: Option<i64>,
    pub last_status: Option<String>,
    pub last_error: Option<String>,
    pub captured_count: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PollOutcome {
    pub captured: u32,
    pub seen_skipped: u32,
}

impl Db {
    pub async fn list_capture_sources(&self) -> Result<Vec<CaptureSource>> {
        let rows = sqlx::query_as::<_, CaptureSource>(
            "SELECT id, kind, label, config, secret_account, enabled, poll_interval_secs, \
                    last_polled_at, last_status, last_error, captured_count, created_at, updated_at \
             FROM capture_sources ORDER BY created_at DESC",
        )
        .fetch_all(self.pool())
        .await?;
        Ok(rows)
    }

    pub async fn upsert_capture_source(
        &self,
        id: Option<String>,
        kind: &str,
        label: &str,
        config: &str,
        secret_account: Option<&str>,
        poll_interval_secs: i64,
    ) -> Result<String> {
        let now = now_ms();
        let id = id.unwrap_or_else(|| ulid::Ulid::new().to_string());
        sqlx::query(
            "INSERT INTO capture_sources \
               (id, kind, label, config, secret_account, poll_interval_secs, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT(id) DO UPDATE SET \
               label = excluded.label, \
               config = excluded.config, \
               secret_account = excluded.secret_account, \
               poll_interval_secs = excluded.poll_interval_secs, \
               updated_at = excluded.updated_at",
        )
        .bind(&id)
        .bind(kind)
        .bind(label)
        .bind(config)
        .bind(secret_account)
        .bind(poll_interval_secs)
        .bind(now)
        .bind(now)
        .execute(self.pool())
        .await?;
        Ok(id)
    }

    pub async fn delete_capture_source(&self, id: &str) -> Result<()> {
        sqlx::query("DELETE FROM capture_sources WHERE id = ?")
            .bind(id)
            .execute(self.pool())
            .await?;
        Ok(())
    }

    pub async fn set_capture_source_enabled(&self, id: &str, enabled: bool) -> Result<()> {
        sqlx::query("UPDATE capture_sources SET enabled = ?, updated_at = ? WHERE id = ?")
            .bind(if enabled { 1_i64 } else { 0_i64 })
            .bind(now_ms())
            .bind(id)
            .execute(self.pool())
            .await?;
        Ok(())
    }

    pub async fn record_poll_result(
        &self,
        id: &str,
        ok: bool,
        captured: u32,
        error: Option<&str>,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE capture_sources SET \
               last_polled_at = ?, \
               last_status = ?, \
               last_error = ?, \
               captured_count = captured_count + ?, \
               updated_at = ? \
             WHERE id = ?",
        )
        .bind(now_ms())
        .bind(if ok { "ok" } else { "error" })
        .bind(error)
        .bind(captured as i64)
        .bind(now_ms())
        .bind(id)
        .execute(self.pool())
        .await?;
        Ok(())
    }

    pub async fn has_seen(&self, source_id: &str, item_uid: &str) -> Result<bool> {
        let row: Option<(String,)> = sqlx::query_as(
            "SELECT item_uid FROM capture_seen WHERE source_id = ? AND item_uid = ?",
        )
        .bind(source_id)
        .bind(item_uid)
        .fetch_optional(self.pool())
        .await?;
        Ok(row.is_some())
    }

    pub async fn mark_seen(&self, source_id: &str, item_uid: &str) -> Result<()> {
        sqlx::query(
            "INSERT OR IGNORE INTO capture_seen (source_id, item_uid, seen_at) \
             VALUES (?, ?, ?)",
        )
        .bind(source_id)
        .bind(item_uid)
        .bind(now_ms())
        .execute(self.pool())
        .await?;
        Ok(())
    }
}

/// IMAP-password-friendly store: the existing `KeyStore` trait deals with
/// 32-byte seeds; passwords can be longer. We use the underlying `keyring`
/// crate directly via these helpers, keyed by service+account.
pub async fn store_password(account: &str, password: &str) -> Result<()> {
    let acc = account.to_string();
    let pw = password.to_string();
    tokio::task::spawn_blocking(move || {
        let entry = keyring::Entry::new("so.cairn.app", &acc)?;
        entry.set_password(&pw)?;
        Ok::<_, anyhow::Error>(())
    })
    .await?
}

pub async fn load_password(account: &str) -> Result<String> {
    let acc = account.to_string();
    tokio::task::spawn_blocking(move || {
        let entry = keyring::Entry::new("so.cairn.app", &acc)?;
        Ok::<_, anyhow::Error>(entry.get_password()?)
    })
    .await?
}

pub async fn delete_password(account: &str) -> Result<()> {
    let acc = account.to_string();
    tokio::task::spawn_blocking(move || {
        let entry = keyring::Entry::new("so.cairn.app", &acc)?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(anyhow::anyhow!(e)),
        }
    })
    .await?
}
