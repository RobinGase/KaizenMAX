//! Encrypted Secret Vault
//!
//! Provides AES-256-GCM envelope encryption for provider API keys and OAuth tokens.
//! Secrets are encrypted immediately on receipt and never stored or returned in plaintext.
//!
//! Vault file format: JSON map of provider -> EncryptedEntry.
//! Master key source: ADMIN_VAULT_KEY env variable (base64-encoded 32-byte key).

use aes_gcm::{
    aead::{Aead, KeyInit, OsRng},
    Aes256Gcm, Nonce,
};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs,
    path::PathBuf,
    sync::Arc,
};
use tokio::sync::RwLock;

/// Metadata returned to callers. Never contains the raw secret.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecretMetadata {
    pub provider: String,
    pub configured: bool,
    pub last_updated: String,
    pub last4: String,
    pub secret_type: String,
}

/// Internal encrypted entry persisted to disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct EncryptedEntry {
    /// Base64-encoded ciphertext (AES-256-GCM).
    ciphertext: String,
    /// Base64-encoded 12-byte nonce.
    nonce: String,
    /// Last 4 characters of the original plaintext (for masked display).
    last4: String,
    /// ISO 8601 timestamp of last update.
    last_updated: String,
    /// Type: "api_key", "oauth_access", "oauth_refresh", "oauth_client_secret".
    secret_type: String,
}

/// Thread-safe vault handle.
#[derive(Clone)]
pub struct SecretVault {
    inner: Arc<RwLock<VaultInner>>,
}

struct VaultInner {
    cipher: Aes256Gcm,
    entries: HashMap<String, EncryptedEntry>,
    path: PathBuf,
}

impl SecretVault {
    /// Create a vault from environment configuration.
    /// Reads ADMIN_VAULT_KEY (base64-encoded 32-byte key) and KAIZEN_VAULT_PATH.
    pub fn from_env() -> Result<Self, String> {
        let key_b64 = std::env::var("ADMIN_VAULT_KEY")
            .map_err(|_| "ADMIN_VAULT_KEY not set. Cannot initialize secret vault.".to_string())?;

        let key_bytes = B64
            .decode(key_b64.trim())
            .map_err(|e| format!("ADMIN_VAULT_KEY is not valid base64: {e}"))?;

        if key_bytes.len() != 32 {
            return Err(format!(
                "ADMIN_VAULT_KEY must be exactly 32 bytes (got {})",
                key_bytes.len()
            ));
        }

        let cipher = Aes256Gcm::new_from_slice(&key_bytes)
            .map_err(|e| format!("Failed to create cipher: {e}"))?;

        let vault_path = std::env::var("KAIZEN_VAULT_PATH")
            .unwrap_or_else(|_| "../data/vault.json".to_string());
        let path = PathBuf::from(vault_path);

        let entries = if path.exists() {
            let content = fs::read_to_string(&path)
                .map_err(|e| format!("Failed to read vault file: {e}"))?;
            serde_json::from_str(&content)
                .map_err(|e| format!("Failed to parse vault file: {e}"))?
        } else {
            HashMap::new()
        };

        Ok(Self {
            inner: Arc::new(RwLock::new(VaultInner {
                cipher,
                entries,
                path,
            })),
        })
    }

    /// Create a vault with an explicit key and path (for testing).
    pub fn new(key: &[u8; 32], path: PathBuf) -> Result<Self, String> {
        let cipher = Aes256Gcm::new_from_slice(key)
            .map_err(|e| format!("Failed to create cipher: {e}"))?;

        let entries = if path.exists() {
            let content = fs::read_to_string(&path)
                .map_err(|e| format!("Failed to read vault file: {e}"))?;
            serde_json::from_str(&content)
                .map_err(|e| format!("Failed to parse vault file: {e}"))?
        } else {
            HashMap::new()
        };

        Ok(Self {
            inner: Arc::new(RwLock::new(VaultInner {
                cipher,
                entries,
                path,
            })),
        })
    }

    /// Store a secret. Encrypts immediately and persists to disk.
    pub async fn store(
        &self,
        provider: &str,
        plaintext: &str,
        secret_type: &str,
    ) -> Result<SecretMetadata, String> {
        let mut inner = self.inner.write().await;

        let mut nonce_bytes = [0u8; 12];
        OsRng.fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);

        let ciphertext = inner
            .cipher
            .encrypt(nonce, plaintext.as_bytes())
            .map_err(|e| format!("Encryption failed: {e}"))?;

        let last4 = if plaintext.len() >= 4 {
            plaintext[plaintext.len() - 4..].to_string()
        } else {
            "****".to_string()
        };

        let now = chrono::Utc::now().to_rfc3339();

        let entry = EncryptedEntry {
            ciphertext: B64.encode(&ciphertext),
            nonce: B64.encode(nonce_bytes),
            last4: last4.clone(),
            last_updated: now.clone(),
            secret_type: secret_type.to_string(),
        };

        inner.entries.insert(provider.to_string(), entry);
        persist(&inner)?;

        Ok(SecretMetadata {
            provider: provider.to_string(),
            configured: true,
            last_updated: now,
            last4,
            secret_type: secret_type.to_string(),
        })
    }

    /// Decrypt and return a secret in memory. Used internally only, never exposed via API.
    pub async fn decrypt(&self, provider: &str) -> Result<String, String> {
        let inner = self.inner.read().await;
        let entry = inner
            .entries
            .get(provider)
            .ok_or_else(|| format!("No secret stored for provider: {provider}"))?;

        let ciphertext = B64
            .decode(&entry.ciphertext)
            .map_err(|e| format!("Failed to decode ciphertext: {e}"))?;
        let nonce_bytes = B64
            .decode(&entry.nonce)
            .map_err(|e| format!("Failed to decode nonce: {e}"))?;

        let nonce = Nonce::from_slice(&nonce_bytes);
        let plaintext = inner
            .cipher
            .decrypt(nonce, ciphertext.as_ref())
            .map_err(|e| format!("Decryption failed: {e}"))?;

        String::from_utf8(plaintext).map_err(|e| format!("Decrypted value is not valid UTF-8: {e}"))
    }

    /// Remove a stored secret and persist.
    pub async fn revoke(&self, provider: &str) -> Result<(), String> {
        let mut inner = self.inner.write().await;
        inner.entries.remove(provider);
        persist(&inner)?;
        Ok(())
    }

    /// List all stored secrets as masked metadata. Never returns raw values.
    pub async fn list(&self) -> Vec<SecretMetadata> {
        let inner = self.inner.read().await;
        inner
            .entries
            .iter()
            .map(|(provider, entry)| SecretMetadata {
                provider: provider.clone(),
                configured: true,
                last_updated: entry.last_updated.clone(),
                last4: entry.last4.clone(),
                secret_type: entry.secret_type.clone(),
            })
            .collect()
    }

    /// Check if a provider has a stored secret.
    pub async fn has(&self, provider: &str) -> bool {
        let inner = self.inner.read().await;
        inner.entries.contains_key(provider)
    }
}

fn persist(inner: &VaultInner) -> Result<(), String> {
    if let Some(parent) = inner.path.parent() {
        if !parent.exists() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create vault directory: {e}"))?;
        }
    }

    let json =
        serde_json::to_string_pretty(&inner.entries).map_err(|e| format!("Serialize error: {e}"))?;

    let tmp_path = inner.path.with_extension("json.tmp");
    fs::write(&tmp_path, &json).map_err(|e| format!("Failed to write vault tmp: {e}"))?;
    fs::rename(&tmp_path, &inner.path).map_err(|e| format!("Failed to rename vault file: {e}"))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env::temp_dir;

    fn test_vault_path(name: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        temp_dir().join(format!("vault-test-{name}-{nanos}.json"))
    }

    fn test_key() -> [u8; 32] {
        let mut key = [0u8; 32];
        rand::RngCore::fill_bytes(&mut OsRng, &mut key);
        key
    }

    #[tokio::test]
    async fn test_store_and_decrypt() {
        let path = test_vault_path("store-decrypt");
        let key = test_key();
        let vault = SecretVault::new(&key, path.clone()).unwrap();

        let meta = vault
            .store("openai", "sk-test-1234567890abcdef", "api_key")
            .await
            .unwrap();

        assert!(meta.configured);
        assert_eq!(meta.last4, "cdef");
        assert_eq!(meta.provider, "openai");

        let decrypted = vault.decrypt("openai").await.unwrap();
        assert_eq!(decrypted, "sk-test-1234567890abcdef");

        // Verify on-disk file contains no plaintext
        let disk_content = fs::read_to_string(&path).unwrap();
        assert!(!disk_content.contains("sk-test-1234567890abcdef"));
        assert!(disk_content.contains("openai"));

        fs::remove_file(&path).ok();
    }

    #[tokio::test]
    async fn test_list_returns_masked_only() {
        let path = test_vault_path("list-masked");
        let key = test_key();
        let vault = SecretVault::new(&key, path.clone()).unwrap();

        vault
            .store("anthropic", "sk-ant-secret-value-9999", "api_key")
            .await
            .unwrap();

        let list = vault.list().await;
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].provider, "anthropic");
        assert_eq!(list[0].last4, "9999");
        assert!(list[0].configured);

        // Ensure the list serialization contains no raw secret
        let json = serde_json::to_string(&list).unwrap();
        assert!(!json.contains("sk-ant-secret-value-9999"));

        fs::remove_file(&path).ok();
    }

    #[tokio::test]
    async fn test_revoke_removes_secret() {
        let path = test_vault_path("revoke");
        let key = test_key();
        let vault = SecretVault::new(&key, path.clone()).unwrap();

        vault
            .store("openai", "sk-revoke-me", "api_key")
            .await
            .unwrap();
        assert!(vault.has("openai").await);

        vault.revoke("openai").await.unwrap();
        assert!(!vault.has("openai").await);

        let result = vault.decrypt("openai").await;
        assert!(result.is_err());

        fs::remove_file(&path).ok();
    }

    #[tokio::test]
    async fn test_persistence_across_loads() {
        let path = test_vault_path("persist");
        let key = test_key();

        {
            let vault = SecretVault::new(&key, path.clone()).unwrap();
            vault
                .store("openai", "sk-persist-test-abcd", "api_key")
                .await
                .unwrap();
        }

        // Re-open vault from same file and key
        let vault2 = SecretVault::new(&key, path.clone()).unwrap();
        let decrypted = vault2.decrypt("openai").await.unwrap();
        assert_eq!(decrypted, "sk-persist-test-abcd");

        fs::remove_file(&path).ok();
    }

    #[tokio::test]
    async fn test_wrong_key_cannot_decrypt() {
        let path = test_vault_path("wrong-key");
        let key1 = test_key();
        let key2 = test_key();

        {
            let vault = SecretVault::new(&key1, path.clone()).unwrap();
            vault
                .store("openai", "sk-secret-only-key1", "api_key")
                .await
                .unwrap();
        }

        let vault2 = SecretVault::new(&key2, path.clone()).unwrap();
        let result = vault2.decrypt("openai").await;
        assert!(result.is_err());

        fs::remove_file(&path).ok();
    }
}
