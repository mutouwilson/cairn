//! Cryptographic audit log.
//!
//! Every MCP access becomes an append-only entry. Entries form a hash chain
//! (`prev_hash` → `hash`) so any tampering is detectable, and each entry is
//! individually signed with Ed25519 so even the chain itself can't be silently
//! rewritten by a process that knows the SQL schema but not the signing key.
//!
//! ## Hash computation
//!
//! ```text
//! canonical = JSON of {
//!   id, agent_id, action, tool_name, arguments,
//!   result_hash, result_count, timestamp, prev_hash
//! } sorted by key
//!
//! hash      = SHA-256(canonical)         // hex
//! signature = base64(Ed25519(hash_bytes))
//! ```
//!
//! ## Verification
//!
//! For any contiguous range of audit entries:
//! 1. Recompute each entry's hash from its canonical form.
//! 2. Check `prev_hash` equals the previous entry's `hash` (or zeros at genesis).
//! 3. Verify each `signature` against the public key.
//!
//! The user (or an auditor) can fetch the public key once and then independently
//! verify that the server has not tampered with history.

use crate::db::{now_ms, Db};
#[cfg(test)]
use crate::keystore::KeyStoreService;
use crate::keystore::{self, AUDIT_KEY_ACCOUNT};
use crate::schema::AuditEntry;
use anyhow::{anyhow, Context, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use ed25519_dalek::{Signer, SigningKey, Verifier, VerifyingKey};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use ulid::Ulid;

#[derive(Clone)]
pub struct AuditLogger {
    db: Db,
    signing: Arc<SigningKey>,
    verifying: VerifyingKey,
}

impl AuditLogger {
    /// Load the signing key from the OS keychain (preferred) or the legacy
    /// `audit_key` SQL table (fallback). Generates a fresh key on first run.
    /// Migrates legacy keys into the keychain transparently — see
    /// `keystore::KeyStoreService::load_or_create`.
    pub async fn init(db: Db) -> Result<Self> {
        let service = keystore::audit_service(db.clone());
        let signing = service
            .load_or_create(AUDIT_KEY_ACCOUNT)
            .await
            .context("load_or_create audit key")?;
        let verifying = signing.verifying_key();
        Ok(Self {
            db,
            signing: Arc::new(signing),
            verifying,
        })
    }

    /// Test-only constructor that forces an in-process keystore so unit tests
    /// don't touch the real OS keychain.
    #[cfg(test)]
    pub async fn init_with_service(db: Db, service: KeyStoreService) -> Result<Self> {
        let signing = service.load_or_create(AUDIT_KEY_ACCOUNT).await?;
        let verifying = signing.verifying_key();
        Ok(Self {
            db,
            signing: Arc::new(signing),
            verifying,
        })
    }

    pub fn public_key_b64(&self) -> String {
        B64.encode(self.verifying.as_bytes())
    }

    /// Used by the bundle module to sign exports. Kept narrow so the
    /// signing key cannot escape this struct directly.
    pub(crate) fn signing_for_export(&self) -> &SigningKey {
        &self.signing
    }

    /// Append an entry. Returns the new entry.
    pub async fn log(
        &self,
        agent_id: &str,
        action: &str,
        tool_name: Option<&str>,
        arguments: Option<&str>,
        result_json: &Value,
        result_count: Option<i64>,
    ) -> Result<AuditEntry> {
        let id = Ulid::new().to_string();
        let timestamp = now_ms();
        let prev_hash = self.db.last_audit_hash().await?;
        let result_hash = sha256_hex(result_json.to_string().as_bytes());

        let canonical = canonical_json(
            &id,
            agent_id,
            action,
            tool_name,
            arguments,
            &result_hash,
            result_count,
            timestamp,
            &prev_hash,
        );
        let hash = sha256_hex(canonical.as_bytes());
        let sig = self.signing.sign(hash.as_bytes());
        let signature = B64.encode(sig.to_bytes());

        let entry = AuditEntry {
            seq: 0, // assigned by DB
            id,
            agent_id: agent_id.to_string(),
            action: action.to_string(),
            tool_name: tool_name.map(str::to_string),
            arguments: arguments.map(str::to_string),
            result_hash,
            result_count,
            timestamp,
            prev_hash,
            hash,
            signature,
        };
        self.db.insert_audit(&entry).await?;
        Ok(entry)
    }

    /// Verify the integrity of the most-recent `limit` entries.
    /// Returns the first inconsistency found, or `Ok(())` if everything checks out.
    pub async fn verify_recent(&self, limit: i64) -> Result<()> {
        let mut entries = self.db.list_audit(limit).await?;
        entries.reverse(); // chronological

        let mut expected_prev = if let Some(first) = entries.first() {
            // recover what prev should be: read seq-1 from DB if it exists
            self.db.last_audit_hash_before(first.seq).await?
        } else {
            "0".repeat(64)
        };

        for entry in &entries {
            if entry.prev_hash != expected_prev {
                return Err(anyhow!(
                    "chain break at seq {} (id {}): prev_hash mismatch",
                    entry.seq,
                    entry.id
                ));
            }
            let canonical = canonical_json(
                &entry.id,
                &entry.agent_id,
                &entry.action,
                entry.tool_name.as_deref(),
                entry.arguments.as_deref(),
                &entry.result_hash,
                entry.result_count,
                entry.timestamp,
                &entry.prev_hash,
            );
            let recomputed = sha256_hex(canonical.as_bytes());
            if recomputed != entry.hash {
                return Err(anyhow!(
                    "hash mismatch at seq {} (id {})",
                    entry.seq,
                    entry.id
                ));
            }
            let sig_bytes = B64.decode(&entry.signature).context("decode signature")?;
            if sig_bytes.len() != 64 {
                return Err(anyhow!("bad signature length at seq {}", entry.seq));
            }
            let mut sb = [0u8; 64];
            sb.copy_from_slice(&sig_bytes);
            let sig = ed25519_dalek::Signature::from_bytes(&sb);
            self.verifying
                .verify(entry.hash.as_bytes(), &sig)
                .map_err(|_| anyhow!("signature invalid at seq {}", entry.seq))?;
            expected_prev = entry.hash.clone();
        }
        Ok(())
    }
}

// Canonicalisation requires all nine fields by design — they are exactly the
// hashed surface. Refactoring into a struct just adds wiring for no gain.
#[allow(clippy::too_many_arguments)]
fn canonical_json(
    id: &str,
    agent_id: &str,
    action: &str,
    tool_name: Option<&str>,
    arguments: Option<&str>,
    result_hash: &str,
    result_count: Option<i64>,
    timestamp: i64,
    prev_hash: &str,
) -> String {
    // Manually sorted by key for deterministic canonical form.
    let v = json!({
        "action": action,
        "agent_id": agent_id,
        "arguments": arguments,
        "id": id,
        "prev_hash": prev_hash,
        "result_count": result_count,
        "result_hash": result_hash,
        "timestamp": timestamp,
        "tool_name": tool_name,
    });
    v.to_string()
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    hex::encode(h.finalize())
}

// Helper used by verify_recent: get the hash immediately before a given seq.
impl Db {
    pub async fn last_audit_hash_before(&self, seq: i64) -> Result<String> {
        let row: Option<(String,)> =
            sqlx::query_as("SELECT hash FROM audit_log WHERE seq < ? ORDER BY seq DESC LIMIT 1")
                .bind(seq)
                .fetch_optional(self.pool())
                .await?;
        Ok(row.map(|r| r.0).unwrap_or_else(|| "0".repeat(64)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keystore::{db_fallback::DbKeyStore, KeyStoreService};
    use tempfile::tempdir;

    /// Build an AuditLogger that only uses DbKeyStore — never touches the
    /// real OS keychain. Used by every unit test in this module.
    async fn db_only_logger(db: Db) -> AuditLogger {
        let store = std::sync::Arc::new(DbKeyStore::new(db.clone()));
        let service = KeyStoreService::new(store.clone(), store);
        AuditLogger::init_with_service(db, service).await.unwrap()
    }

    #[tokio::test]
    async fn chain_verifies() {
        let dir = tempdir().unwrap();
        let db = Db::open(&dir.path().join("test.db")).await.unwrap();
        let logger = db_only_logger(db.clone()).await;

        for i in 0..5 {
            logger
                .log(
                    "claude-desktop",
                    "tools/call",
                    Some("search_memory"),
                    Some(r#"{"query":"test"}"#),
                    &json!({ "i": i }),
                    Some(i as i64),
                )
                .await
                .unwrap();
        }
        logger.verify_recent(10).await.unwrap();
    }

    #[tokio::test]
    async fn tamper_detected() {
        let dir = tempdir().unwrap();
        let db = Db::open(&dir.path().join("test.db")).await.unwrap();
        let logger = db_only_logger(db.clone()).await;

        logger
            .log(
                "claude-desktop",
                "tools/call",
                Some("search_memory"),
                None,
                &json!({}),
                Some(0),
            )
            .await
            .unwrap();

        // Tamper directly in SQL.
        sqlx::query("UPDATE audit_log SET arguments = 'TAMPERED' WHERE seq = 1")
            .execute(db.pool())
            .await
            .unwrap();

        assert!(logger.verify_recent(10).await.is_err());
    }
}
