//! Fallback `KeyStore` backed by the existing `audit_key` SQLite table.
//!
//! Used:
//!   - in environments without a working OS keyring (CI, headless server,
//!     locked-down container);
//!   - to migrate legacy installs whose audit key still lives in SQL;
//!   - during tests, where we want a self-contained in-process secret store.

use crate::db::{now_ms, Db};
use crate::keystore::KeyStore;
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD as B64, Engine as _};

pub struct DbKeyStore {
    db: Db,
}

impl DbKeyStore {
    pub fn new(db: Db) -> Self {
        Self { db }
    }
}

#[async_trait]
impl KeyStore for DbKeyStore {
    fn name(&self) -> &'static str {
        "db"
    }

    async fn load_seed(&self, account: &str) -> Result<Option<[u8; 32]>> {
        // Only the well-known "audit-signing-key" lives in the legacy table.
        if account != crate::keystore::AUDIT_KEY_ACCOUNT {
            return Ok(None);
        }
        let Some((_, priv_b64)) = self.db.load_audit_key().await? else {
            return Ok(None);
        };
        let bytes = B64
            .decode(priv_b64)
            .map_err(|e| anyhow!("decode key: {e}"))?;
        if bytes.len() != 32 {
            return Err(anyhow!("legacy key has wrong length: {}", bytes.len()));
        }
        let mut seed = [0u8; 32];
        seed.copy_from_slice(&bytes);
        Ok(Some(seed))
    }

    async fn save_seed(&self, account: &str, seed: &[u8; 32]) -> Result<()> {
        if account != crate::keystore::AUDIT_KEY_ACCOUNT {
            return Err(anyhow!("DbKeyStore only stores the audit key"));
        }
        let signing = ed25519_dalek::SigningKey::from_bytes(seed);
        let pub_b64 = B64.encode(signing.verifying_key().as_bytes());
        let priv_b64 = B64.encode(seed);
        // INSERT-OR-REPLACE so we can save during migration without DELETE.
        sqlx::query(
            "INSERT INTO audit_key (id, public_key, private_key, created_at) VALUES (1, ?, ?, ?) \
             ON CONFLICT(id) DO UPDATE SET public_key = excluded.public_key, \
                                            private_key = excluded.private_key, \
                                            created_at = excluded.created_at",
        )
        .bind(&pub_b64)
        .bind(&priv_b64)
        .bind(now_ms())
        .execute(self.db.pool())
        .await?;
        Ok(())
    }

    async fn delete(&self, account: &str) -> Result<()> {
        if account != crate::keystore::AUDIT_KEY_ACCOUNT {
            return Ok(());
        }
        sqlx::query("DELETE FROM audit_key WHERE id = 1")
            .execute(self.db.pool())
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn db_keystore_round_trip() {
        let dir = tempdir().unwrap();
        let db = Db::open(&dir.path().join("test.db")).await.unwrap();
        let store = DbKeyStore::new(db);
        let account = crate::keystore::AUDIT_KEY_ACCOUNT;

        assert!(store.load_seed(account).await.unwrap().is_none());
        let seed = [7u8; 32];
        store.save_seed(account, &seed).await.unwrap();
        assert_eq!(store.load_seed(account).await.unwrap(), Some(seed));
        store.delete(account).await.unwrap();
        assert!(store.load_seed(account).await.unwrap().is_none());
    }
}
