//! Pluggable secret-storage layer.
//!
//! Phase 3a moves the Ed25519 audit signing key out of the SQL `audit_key`
//! table into the OS keychain (macOS Keychain Services / Linux libsecret /
//! Windows Credential Manager) via the cross-platform `keyring` crate.
//!
//! Design:
//!   - `KeyStore` trait — pure I/O over an opaque 32-byte seed.
//!   - `KeychainKeyStore` — primary, OS-native.
//!   - `DbKeyStore` — fallback for systems without a keyring (CI, headless
//!     servers, containers) and the legacy on-disk location.
//!   - `KeyStoreService::load_or_create` — try primary, migrate from fallback
//!     if needed, generate fresh otherwise. Always returns a `SigningKey`.
//!
//! The trait is intentionally narrow: only the *audit* key lives here today,
//! but the same surface area will host the sqlcipher passphrase in Phase 3b.

pub mod db_fallback;
pub mod keychain;

use anyhow::Result;
use async_trait::async_trait;
use ed25519_dalek::SigningKey;
use std::sync::Arc;

pub const AUDIT_KEY_ACCOUNT: &str = "audit-signing-key";

#[async_trait]
pub trait KeyStore: Send + Sync {
    fn name(&self) -> &'static str;
    async fn load_seed(&self, account: &str) -> Result<Option<[u8; 32]>>;
    async fn save_seed(&self, account: &str, seed: &[u8; 32]) -> Result<()>;
    async fn delete(&self, account: &str) -> Result<()>;
}

pub struct KeyStoreService {
    primary: Arc<dyn KeyStore>,
    fallback: Arc<dyn KeyStore>,
}

impl KeyStoreService {
    pub fn new(primary: Arc<dyn KeyStore>, fallback: Arc<dyn KeyStore>) -> Self {
        Self { primary, fallback }
    }

    /// Get or create the seed for `account`. If the primary store has it,
    /// return that. Otherwise migrate from the fallback (and delete from the
    /// fallback on a verified primary write). Otherwise generate a fresh seed
    /// and persist to whichever store is healthy.
    ///
    /// Probes are wrapped in a `KEYSTORE_PROBE_TIMEOUT` budget — if the OS
    /// keychain is sitting on an "Allow access" sheet that nobody's clicking
    /// (e.g. a headless `cairn-mcp --transport sse` launch), we fall through
    /// to the DB fallback instead of hanging the process.
    pub async fn load_or_create(&self, account: &str) -> Result<SigningKey> {
        let primary_load = primary_with_timeout(&*self.primary, account).await;
        if let Ok(Some(seed)) = primary_load {
            tracing::debug!(store = self.primary.name(), account, "audit key loaded");
            return Ok(SigningKey::from_bytes(&seed));
        }

        if let Ok(Some(seed)) = self.fallback.load_seed(account).await {
            // Attempt to push back into the primary, but don't get stuck if it
            // would prompt again — fall back to "stay on DB" silently.
            let save_attempt = tokio::time::timeout(
                KEYSTORE_PROBE_TIMEOUT,
                self.primary.save_seed(account, &seed),
            )
            .await;
            match save_attempt {
                Ok(Ok(())) => {
                    let _ = self.fallback.delete(account).await;
                    tracing::info!(
                        from = self.fallback.name(),
                        to = self.primary.name(),
                        account,
                        "migrated audit key into keychain"
                    );
                }
                Ok(Err(e)) => {
                    tracing::warn!(error = %e, "primary keystore unavailable; staying on fallback");
                }
                Err(_) => {
                    tracing::warn!(
                        timeout_ms = KEYSTORE_PROBE_TIMEOUT.as_millis() as u64,
                        "primary keystore save timed out; staying on fallback"
                    );
                }
            }
            return Ok(SigningKey::from_bytes(&seed));
        }

        let mut csprng = rand::rngs::OsRng;
        let signing = SigningKey::generate(&mut csprng);
        let seed = signing.to_bytes();
        let save_attempt = tokio::time::timeout(
            KEYSTORE_PROBE_TIMEOUT,
            self.primary.save_seed(account, &seed),
        )
        .await;
        match save_attempt {
            Ok(Ok(())) => tracing::info!(
                store = self.primary.name(),
                account,
                "generated fresh audit key"
            ),
            Ok(Err(e)) => {
                tracing::warn!(error = %e, "primary store unavailable; persisting to fallback");
                self.fallback.save_seed(account, &seed).await?;
            }
            Err(_) => {
                tracing::warn!(
                    timeout_ms = KEYSTORE_PROBE_TIMEOUT.as_millis() as u64,
                    "primary store save timed out; persisting to fallback"
                );
                self.fallback.save_seed(account, &seed).await?;
            }
        }
        Ok(signing)
    }
}

const KEYSTORE_PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);

async fn primary_with_timeout(store: &dyn KeyStore, account: &str) -> Result<Option<[u8; 32]>> {
    match tokio::time::timeout(KEYSTORE_PROBE_TIMEOUT, store.load_seed(account)).await {
        Ok(res) => res,
        Err(_) => {
            tracing::warn!(
                timeout_ms = KEYSTORE_PROBE_TIMEOUT.as_millis() as u64,
                "primary keystore load timed out (likely an unanswered OS prompt) — falling back"
            );
            Ok(None)
        }
    }
}

pub fn audit_service(db: crate::db::Db) -> KeyStoreService {
    KeyStoreService::new(
        Arc::new(keychain::KeychainKeyStore::default()),
        Arc::new(db_fallback::DbKeyStore::new(db)),
    )
}
