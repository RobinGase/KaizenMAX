//! Encrypted Secret Vault
//!
//! Provides AES-256-GCM envelope encryption for provider API keys and OAuth tokens.
//! Secrets are encrypted immediately on receipt and never stored or returned in plaintext.
//!
//! Vault file format: JSON map of provider -> EncryptedEntry.
//! Master key source priority:
//! 1) ADMIN_VAULT_KEY env variable (base64-encoded 32-byte key)
//! 2) Managed key file at KAIZEN_VAULT_KEY_PATH (auto-generated on first run)

use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, KeyInit, OsRng},
};
use base64::{Engine, engine::general_purpose::STANDARD as B64};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, convert::TryInto, fs, path::PathBuf, sync::Arc};
use tokio::sync::RwLock;

/// Metadata returned to callers. Never contains the raw secret.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecretMetadata {
    pub provider: String,
    pub configured: bool,
    pub key_id: String,
    pub created_at: String,
    pub last_updated: String,
    pub last4: String,
    pub secret_type: String,
}

/// Internal encrypted entry persisted to disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct EncryptedEntry {
    /// Provider id (e.g. openai, anthropic).
    #[serde(default)]
    provider: String,
    /// Vault key slot identifier.
    #[serde(default = "default_key_id")]
    key_id: String,
    /// Base64-encoded ciphertext (AES-256-GCM).
    ciphertext: String,
    /// Base64-encoded 12-byte nonce.
    nonce: String,
    /// ISO 8601 timestamp when record was first created.
    #[serde(default)]
    created_at: String,
    /// Last 4 characters of the original plaintext (for masked display).
    last4: String,
    /// ISO 8601 timestamp of last update.
    last_updated: String,
    /// Type: "api_key", "oauth_access", "oauth_refresh", "oauth_client_secret".
    secret_type: String,
}

fn default_key_id() -> String {
    "vault-key-v1".to_string()
}

const CANONICAL_VAULT_PROVIDER: &str = "kaizen";

fn canonical_provider(provider: &str) -> String {
    let trimmed = provider.trim();
    if trimmed.eq_ignore_ascii_case(CANONICAL_VAULT_PROVIDER)
        || trimmed.eq_ignore_ascii_case("zeroclaw")
        || trimmed.eq_ignore_ascii_case("kai-zen")
    {
        CANONICAL_VAULT_PROVIDER.to_string()
    } else {
        trimmed.to_string()
    }
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

/// Runtime status for the vault subsystem.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VaultStatus {
    pub available: bool,
    pub key_source: String,
    pub vault_path: String,
    pub key_path: Option<String>,
    pub bootstrap_created: bool,
    pub error: Option<String>,
}

fn default_vault_path() -> PathBuf {
    PathBuf::from(
        std::env::var("KAIZEN_VAULT_PATH").unwrap_or_else(|_| "../data/vault.json".to_string()),
    )
}

fn default_key_path() -> PathBuf {
    PathBuf::from(
        std::env::var("KAIZEN_VAULT_KEY_PATH").unwrap_or_else(|_| "../data/vault.key".to_string()),
    )
}

fn parse_key_b64(value: &str, source: &str) -> Result<[u8; 32], String> {
    let key_bytes = B64
        .decode(value.trim())
        .map_err(|e| format!("{source} is not valid base64: {e}"))?;

    let key: [u8; 32] = key_bytes.as_slice().try_into().map_err(|_| {
        format!(
            "{source} must be exactly 32 bytes (got {})",
            key_bytes.len()
        )
    })?;

    Ok(key)
}

fn load_entries(path: &PathBuf) -> Result<(HashMap<String, EncryptedEntry>, bool), String> {
    if path.exists() {
        let content =
            fs::read_to_string(path).map_err(|e| format!("Failed to read vault file: {e}"))?;
        let entries: HashMap<String, EncryptedEntry> = serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse vault file: {e}"))?;

        let mut normalized = HashMap::new();
        let mut normalized_with_sources: HashMap<String, (EncryptedEntry, bool)> = HashMap::new();
        let mut changed = false;

        // Backward compatibility:
        // - hydrate newer metadata fields when loading legacy vault files
        // - normalize legacy kaizen aliases to canonical provider key
        // - resolve alias collisions by newest RFC3339 last_updated
        for (provider_key, mut entry) in entries {
            let source_is_canonical = provider_key.eq_ignore_ascii_case(CANONICAL_VAULT_PROVIDER);

            if entry.provider.trim().is_empty() {
                entry.provider = provider_key.clone();
                changed = true;
            }
            if entry.key_id.trim().is_empty() {
                entry.key_id = default_key_id();
                changed = true;
            }
            if entry.created_at.trim().is_empty() {
                entry.created_at = entry.last_updated.clone();
                changed = true;
            }

            let canonical_key = canonical_provider(&provider_key);
            if canonical_key != provider_key {
                changed = true;
            }

            let canonical_entry_provider = canonical_provider(&entry.provider);
            if canonical_entry_provider != entry.provider {
                entry.provider = canonical_entry_provider;
                changed = true;
            }

            if canonical_key == CANONICAL_VAULT_PROVIDER
                && entry.provider != CANONICAL_VAULT_PROVIDER
            {
                entry.provider = CANONICAL_VAULT_PROVIDER.to_string();
                changed = true;
            }

            match normalized_with_sources.get(&canonical_key) {
                Some((existing, existing_source_is_canonical)) => {
                    let incoming_newer = entry.last_updated > existing.last_updated;
                    let same_time_but_prefer_incoming_canonical = entry.last_updated
                        == existing.last_updated
                        && source_is_canonical
                        && !*existing_source_is_canonical;

                    if incoming_newer || same_time_but_prefer_incoming_canonical {
                        normalized_with_sources.insert(canonical_key, (entry, source_is_canonical));
                    }
                    changed = true;
                }
                None => {
                    normalized_with_sources.insert(canonical_key, (entry, source_is_canonical));
                }
            }
        }

        for (provider, (entry, _)) in normalized_with_sources {
            normalized.insert(provider, entry);
        }

        Ok((normalized, changed))
    } else {
        Ok((HashMap::new(), false))
    }
}

impl SecretVault {
    /// Create a vault from environment configuration.
    /// Reads ADMIN_VAULT_KEY (base64-encoded 32-byte key) and KAIZEN_VAULT_PATH.
    pub fn from_env() -> Result<Self, String> {
        let key_b64 = std::env::var("ADMIN_VAULT_KEY")
            .map_err(|_| "ADMIN_VAULT_KEY not set. Cannot initialize secret vault.".to_string())?;

        let key = parse_key_b64(&key_b64, "ADMIN_VAULT_KEY")?;
        let cipher =
            Aes256Gcm::new_from_slice(&key).map_err(|e| format!("Failed to create cipher: {e}"))?;

        let path = default_vault_path();
        let (entries, was_normalized) = load_entries(&path)?;
        if was_normalized {
            persist_entries(&path, &entries)?;
        }

        Ok(Self {
            inner: Arc::new(RwLock::new(VaultInner {
                cipher,
                entries,
                path,
            })),
        })
    }

    /// Create a vault using env key if provided, otherwise auto-bootstrap a
    /// managed key file so users can configure providers fully from the UI.
    pub fn from_env_or_bootstrap() -> Result<(Self, VaultStatus), String> {
        let vault_path = default_vault_path();

        if let Ok(key_b64) = std::env::var("ADMIN_VAULT_KEY") {
            let key = parse_key_b64(&key_b64, "ADMIN_VAULT_KEY")?;
            let vault = Self::new(&key, vault_path.clone())?;
            let status = VaultStatus {
                available: true,
                key_source: "env".to_string(),
                vault_path: vault_path.display().to_string(),
                key_path: None,
                bootstrap_created: false,
                error: None,
            };
            return Ok((vault, status));
        }

        let key_path = default_key_path();
        let (key, bootstrap_created) = if key_path.exists() {
            let content = fs::read_to_string(&key_path)
                .map_err(|e| format!("Failed to read vault key file: {e}"))?;
            (
                parse_key_b64(content.trim(), "KAIZEN_VAULT_KEY_PATH file")?,
                false,
            )
        } else {
            if let Some(parent) = key_path.parent() {
                if !parent.exists() {
                    fs::create_dir_all(parent)
                        .map_err(|e| format!("Failed to create vault key directory: {e}"))?;
                }
            }

            let mut key = [0u8; 32];
            OsRng.fill_bytes(&mut key);
            let key_b64 = B64.encode(key);
            fs::write(&key_path, format!("{key_b64}\n"))
                .map_err(|e| format!("Failed to write vault key file: {e}"))?;
            (key, true)
        };

        let vault = Self::new(&key, vault_path.clone())?;
        let status = VaultStatus {
            available: true,
            key_source: "managed_file".to_string(),
            vault_path: vault_path.display().to_string(),
            key_path: Some(key_path.display().to_string()),
            bootstrap_created,
            error: None,
        };

        Ok((vault, status))
    }

    /// Create a vault with an explicit key and path (for testing).
    pub fn new(key: &[u8; 32], path: PathBuf) -> Result<Self, String> {
        let cipher =
            Aes256Gcm::new_from_slice(key).map_err(|e| format!("Failed to create cipher: {e}"))?;

        let (entries, was_normalized) = load_entries(&path)?;
        if was_normalized {
            persist_entries(&path, &entries)?;
        }

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
        let provider_key = canonical_provider(provider);

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
            provider: provider_key.clone(),
            key_id: default_key_id(),
            ciphertext: B64.encode(&ciphertext),
            nonce: B64.encode(nonce_bytes),
            created_at: inner
                .entries
                .get(&provider_key)
                .map(|prev| prev.created_at.clone())
                .filter(|v| !v.is_empty())
                .unwrap_or_else(|| now.clone()),
            last4: last4.clone(),
            last_updated: now.clone(),
            secret_type: secret_type.to_string(),
        };

        inner.entries.insert(provider_key.clone(), entry);
        persist(&inner)?;

        Ok(SecretMetadata {
            provider: provider_key.clone(),
            configured: true,
            key_id: default_key_id(),
            created_at: inner
                .entries
                .get(&provider_key)
                .map(|v| v.created_at.clone())
                .filter(|v| !v.is_empty())
                .unwrap_or_else(|| now.clone()),
            last_updated: now,
            last4,
            secret_type: secret_type.to_string(),
        })
    }

    /// Decrypt and return a secret in memory. Used internally only, never exposed via API.
    pub async fn decrypt(&self, provider: &str) -> Result<String, String> {
        let inner = self.inner.read().await;
        let provider_key = canonical_provider(provider);
        let entry = inner
            .entries
            .get(&provider_key)
            .ok_or_else(|| format!("No secret stored for provider: {provider}"))?;

        let ciphertext = B64
            .decode(&entry.ciphertext)
            .map_err(|e| format!("Failed to decode ciphertext: {e}"))?;
        let nonce_bytes = B64
            .decode(&entry.nonce)
            .map_err(|e| format!("Failed to decode nonce: {e}"))?;

        let nonce = Nonce::from_slice(&nonce_bytes);
        let mut plaintext = inner
            .cipher
            .decrypt(nonce, ciphertext.as_ref())
            .map_err(|e| format!("Decryption failed: {e}"))?;

        let result = std::str::from_utf8(&plaintext)
            .map_err(|e| format!("Decrypted value is not valid UTF-8: {e}"))?
            .to_string();

        // Best-effort wipe of plaintext bytes once materialized into String.
        plaintext.fill(0);
        Ok(result)
    }

    /// Remove a stored secret and persist.
    pub async fn revoke(&self, provider: &str) -> Result<(), String> {
        let mut inner = self.inner.write().await;
        let provider_key = canonical_provider(provider);
        inner.entries.remove(&provider_key);
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
                provider: canonical_provider(provider),
                configured: true,
                key_id: if entry.key_id.is_empty() {
                    default_key_id()
                } else {
                    entry.key_id.clone()
                },
                created_at: if entry.created_at.is_empty() {
                    entry.last_updated.clone()
                } else {
                    entry.created_at.clone()
                },
                last_updated: entry.last_updated.clone(),
                last4: entry.last4.clone(),
                secret_type: entry.secret_type.clone(),
            })
            .collect()
    }

    /// Check if a provider has a stored secret.
    pub async fn has(&self, provider: &str) -> bool {
        let inner = self.inner.read().await;
        let provider_key = canonical_provider(provider);
        inner.entries.contains_key(&provider_key)
    }
}

fn persist(inner: &VaultInner) -> Result<(), String> {
    persist_entries(&inner.path, &inner.entries)
}

fn persist_entries(
    path: &PathBuf,
    entries: &HashMap<String, EncryptedEntry>,
) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        if !parent.exists() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create vault directory: {e}"))?;
        }
    }

    let json =
        serde_json::to_string_pretty(entries).map_err(|e| format!("Serialize error: {e}"))?;

    let tmp_path = path.with_extension("json.tmp");
    fs::write(&tmp_path, &json).map_err(|e| format!("Failed to write vault tmp: {e}"))?;
    fs::rename(&tmp_path, path).map_err(|e| format!("Failed to rename vault file: {e}"))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        env::temp_dir,
        sync::{Mutex, OnceLock},
    };

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

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

    fn test_key_path(name: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        temp_dir().join(format!("vault-key-test-{name}-{nanos}.key"))
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
        assert_eq!(meta.key_id, "vault-key-v1");
        assert!(!meta.created_at.is_empty());

        let decrypted = vault.decrypt("openai").await.unwrap();
        assert_eq!(decrypted, "sk-test-1234567890abcdef");

        // Verify on-disk file contains no plaintext
        let disk_content = fs::read_to_string(&path).unwrap();
        assert!(!disk_content.contains("sk-test-1234567890abcdef"));
        assert!(disk_content.contains("openai"));
        assert!(disk_content.contains("key_id"));
        assert!(disk_content.contains("created_at"));
        assert!(disk_content.contains("nonce"));
        assert!(disk_content.contains("ciphertext"));

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
        assert_eq!(list[0].key_id, "vault-key-v1");
        assert!(!list[0].created_at.is_empty());

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
    async fn test_kaizen_alias_parity_store_decrypt_has_revoke() {
        let path = test_vault_path("kaizen-alias-parity");
        let key = test_key();
        let vault = SecretVault::new(&key, path.clone()).unwrap();

        let meta = vault
            .store("zeroclaw", "kz-alias-secret-ABCD", "api_key")
            .await
            .unwrap();
        assert_eq!(meta.provider, "kaizen");

        assert!(vault.has("kaizen").await);
        assert!(vault.has("zeroclaw").await);
        assert!(vault.has("kai-zen").await);

        assert_eq!(
            vault.decrypt("kaizen").await.unwrap(),
            "kz-alias-secret-ABCD"
        );
        assert_eq!(
            vault.decrypt("zeroclaw").await.unwrap(),
            "kz-alias-secret-ABCD"
        );
        assert_eq!(
            vault.decrypt("kai-zen").await.unwrap(),
            "kz-alias-secret-ABCD"
        );

        let list = vault.list().await;
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].provider, "kaizen");

        vault.revoke("kai-zen").await.unwrap();
        assert!(!vault.has("kaizen").await);
        assert!(vault.decrypt("zeroclaw").await.is_err());

        fs::remove_file(&path).ok();
    }

    #[tokio::test]
    async fn test_migrates_legacy_alias_key_to_canonical_on_load() {
        let path = test_vault_path("legacy-migration");
        let key = test_key();

        {
            let vault = SecretVault::new(&key, path.clone()).unwrap();
            vault
                .store("kaizen", "kz-migrate-secret-1234", "api_key")
                .await
                .unwrap();
        }

        let mut raw_entries: HashMap<String, EncryptedEntry> =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        let mut canonical_entry = raw_entries.remove("kaizen").unwrap();
        canonical_entry.provider = "zeroclaw".to_string();
        raw_entries.insert("zeroclaw".to_string(), canonical_entry);
        fs::write(&path, serde_json::to_string_pretty(&raw_entries).unwrap()).unwrap();

        let vault = SecretVault::new(&key, path.clone()).unwrap();
        assert!(vault.has("kaizen").await);
        assert_eq!(
            vault.decrypt("kai-zen").await.unwrap(),
            "kz-migrate-secret-1234"
        );

        let list = vault.list().await;
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].provider, "kaizen");

        let persisted: HashMap<String, EncryptedEntry> =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert!(persisted.contains_key("kaizen"));
        assert!(!persisted.contains_key("zeroclaw"));

        fs::remove_file(&path).ok();
    }

    #[tokio::test]
    async fn test_migration_collision_keeps_newest_last_updated() {
        let path = test_vault_path("legacy-collision");
        let source_old_path = test_vault_path("legacy-collision-old-src");
        let source_new_path = test_vault_path("legacy-collision-new-src");
        let key = test_key();

        {
            let vault = SecretVault::new(&key, source_old_path.clone()).unwrap();
            vault
                .store("kaizen", "kz-old-secret-1111", "api_key")
                .await
                .unwrap();
        }
        {
            let vault = SecretVault::new(&key, source_new_path.clone()).unwrap();
            vault
                .store("kaizen", "kz-new-secret-2222", "api_key")
                .await
                .unwrap();
        }

        let mut old_entries: HashMap<String, EncryptedEntry> =
            serde_json::from_str(&fs::read_to_string(&source_old_path).unwrap()).unwrap();
        let mut new_entries: HashMap<String, EncryptedEntry> =
            serde_json::from_str(&fs::read_to_string(&source_new_path).unwrap()).unwrap();

        let mut canonical_old = old_entries.remove("kaizen").unwrap();
        canonical_old.last_updated = "2026-01-01T00:00:00Z".to_string();
        canonical_old.created_at = "2026-01-01T00:00:00Z".to_string();

        let mut legacy_new = new_entries.remove("kaizen").unwrap();
        legacy_new.provider = "zeroclaw".to_string();
        legacy_new.last_updated = "2026-01-02T00:00:00Z".to_string();
        legacy_new.created_at = "2026-01-02T00:00:00Z".to_string();

        let mut collision_entries = HashMap::new();
        collision_entries.insert("kaizen".to_string(), canonical_old);
        collision_entries.insert("zeroclaw".to_string(), legacy_new);
        fs::write(
            &path,
            serde_json::to_string_pretty(&collision_entries).unwrap(),
        )
        .unwrap();

        let vault = SecretVault::new(&key, path.clone()).unwrap();
        assert_eq!(vault.decrypt("kaizen").await.unwrap(), "kz-new-secret-2222");

        let persisted: HashMap<String, EncryptedEntry> =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(persisted.len(), 1);
        assert!(persisted.contains_key("kaizen"));
        assert!(!persisted.contains_key("zeroclaw"));

        fs::remove_file(&path).ok();
        fs::remove_file(&source_old_path).ok();
        fs::remove_file(&source_new_path).ok();
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

    #[test]
    fn test_bootstrap_generates_managed_key_file() {
        let _guard = env_lock().lock().unwrap();
        let vault_path = test_vault_path("bootstrap-managed");
        let key_path = test_key_path("bootstrap-managed");

        unsafe {
            std::env::remove_var("ADMIN_VAULT_KEY");
            std::env::set_var("KAIZEN_VAULT_PATH", vault_path.display().to_string());
            std::env::set_var("KAIZEN_VAULT_KEY_PATH", key_path.display().to_string());
        }

        let (_, status) = SecretVault::from_env_or_bootstrap().unwrap();
        assert!(status.available);
        assert_eq!(status.key_source, "managed_file");
        assert!(status.bootstrap_created);
        assert!(key_path.exists());

        unsafe {
            std::env::remove_var("KAIZEN_VAULT_PATH");
            std::env::remove_var("KAIZEN_VAULT_KEY_PATH");
        }
        fs::remove_file(&key_path).ok();
        fs::remove_file(&vault_path).ok();
    }
}
