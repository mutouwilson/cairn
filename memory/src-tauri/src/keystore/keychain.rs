//! `keyring`-crate backed implementation. Wraps the synchronous `keyring`
//! API in `tokio::task::spawn_blocking` so it composes with the rest of the
//! async stack.
//!
//! Service name: `so.cairn.app` — matches the Tauri bundle identifier so the
//! same OS ACL grants apply whether you launched via the GUI or via the
//! `cairn-mcp` subprocess.

use crate::keystore::KeyStore;
use anyhow::{anyhow, Result};
use async_trait::async_trait;

const SERVICE: &str = "so.cairn.app";

pub struct KeychainKeyStore {
    service: String,
}

impl Default for KeychainKeyStore {
    fn default() -> Self {
        Self {
            service: SERVICE.to_string(),
        }
    }
}

impl KeychainKeyStore {
    pub fn with_service(service: impl Into<String>) -> Self {
        Self {
            service: service.into(),
        }
    }
}

#[async_trait]
impl KeyStore for KeychainKeyStore {
    fn name(&self) -> &'static str {
        "keychain"
    }

    async fn load_seed(&self, account: &str) -> Result<Option<[u8; 32]>> {
        let service = self.service.clone();
        let account = account.to_string();
        tokio::task::spawn_blocking(move || {
            let entry = keyring::Entry::new(&service, &account)?;
            match entry.get_secret() {
                Ok(bytes) if bytes.len() == 32 => {
                    let mut seed = [0u8; 32];
                    seed.copy_from_slice(&bytes);
                    Ok(Some(seed))
                }
                Ok(other) => Err(anyhow!(
                    "keychain secret has wrong length: got {}, expected 32",
                    other.len()
                )),
                Err(keyring::Error::NoEntry) => Ok(None),
                Err(e) => Err(anyhow!("keychain load failed: {e}")),
            }
        })
        .await?
    }

    async fn save_seed(&self, account: &str, seed: &[u8; 32]) -> Result<()> {
        let service = self.service.clone();
        let account = account.to_string();
        let bytes = seed.to_vec();
        tokio::task::spawn_blocking(move || {
            let entry = keyring::Entry::new(&service, &account)?;
            entry
                .set_secret(&bytes)
                .map_err(|e| anyhow!("keychain save failed: {e}"))
        })
        .await?
    }

    async fn delete(&self, account: &str) -> Result<()> {
        let service = self.service.clone();
        let account = account.to_string();
        tokio::task::spawn_blocking(move || {
            let entry = keyring::Entry::new(&service, &account)?;
            match entry.delete_credential() {
                Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
                Err(e) => Err(anyhow!("keychain delete failed: {e}")),
            }
        })
        .await?
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unique_account() -> String {
        use std::time::{SystemTime, UNIX_EPOCH};
        format!(
            "cairn-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        )
    }

    #[tokio::test]
    async fn round_trip_macos() {
        // Only meaningful on macOS where the test runner has user keychain access.
        if std::env::var("CAIRN_KEYCHAIN_TEST_DISABLE").is_ok() {
            return;
        }
        let store = KeychainKeyStore::with_service("so.cairn.test");
        let account = unique_account();

        let initial = store.load_seed(&account).await.unwrap();
        assert!(initial.is_none(), "test account should not exist yet");

        let seed = [9u8; 32];
        store.save_seed(&account, &seed).await.unwrap();

        let loaded = store.load_seed(&account).await.unwrap();
        assert_eq!(loaded, Some(seed));

        store.delete(&account).await.unwrap();
        let after = store.load_seed(&account).await.unwrap();
        assert!(after.is_none(), "delete should have removed the entry");
    }
}
