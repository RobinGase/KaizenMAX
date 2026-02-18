//! WebKeys Storage Layer
//!
//! Persists virtual keys and provider bindings to disk.
//! Uses JSON files under ~/.kaizen/webkeys/

use super::types::*;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::sync::RwLock;

const STORE_VERSION: u32 = 1;
const VIRTUAL_KEYS_FILE: &str = "virtual-keys.json";
const PROVIDER_BINDINGS_FILE: &str = "provider-bindings.json";
const USAGE_FILE: &str = "usage.json";

/// Storage file format for virtual keys
#[derive(Debug, Clone, Serialize, Deserialize)]
struct VirtualKeysStore {
    version: u32,
    keys: HashMap<String, VirtualKeyRecord>,
}

/// Storage file format for provider bindings
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ProviderBindingsStore {
    version: u32,
    bindings: HashMap<String, WebProviderBinding>,
}

/// Storage file format for usage stats
#[derive(Debug, Clone, Serialize, Deserialize)]
struct UsageStore {
    version: u32,
    usage: HashMap<String, VirtualKeyUsage>,
}

/// Thread-safe storage handle
#[derive(Clone)]
pub struct WebKeysStorage {
    inner: Arc<RwLock<StorageInner>>,
}

struct StorageInner {
    data_dir: PathBuf,
    in_memory: bool,
    keys: HashMap<String, VirtualKeyRecord>,
    bindings: HashMap<String, WebProviderBinding>,
    usage: HashMap<String, VirtualKeyUsage>,
}

impl WebKeysStorage {
    /// Create storage at default location
    pub async fn new() -> Result<Self, String> {
        let data_dir = default_data_dir();
        Self::with_dir(data_dir).await
    }

    /// Create storage at specific directory
    pub async fn with_dir(data_dir: PathBuf) -> Result<Self, String> {
        fs::create_dir_all(&data_dir)
            .map_err(|e| format!("Failed to create webkeys data directory: {}", e))?;

        let inner = StorageInner {
            data_dir,
            in_memory: false,
            keys: HashMap::new(),
            bindings: HashMap::new(),
            usage: HashMap::new(),
        };

        let storage = Self {
            inner: Arc::new(RwLock::new(inner)),
        };

        storage.load_all().await?;
        Ok(storage)
    }

    /// Load all data from disk
    async fn load_all(&self) -> Result<(), String> {
        let mut inner = self.inner.write().await;

        // Load virtual keys
        let keys_path = inner.data_dir.join(VIRTUAL_KEYS_FILE);
        if keys_path.exists() {
            let content = fs::read_to_string(&keys_path)
                .map_err(|e| format!("Failed to read virtual keys: {}", e))?;
            let store: VirtualKeysStore = serde_json::from_str(&content)
                .map_err(|e| format!("Failed to parse virtual keys: {}", e))?;
            inner.keys = store.keys;
        }

        // Load provider bindings
        let bindings_path = inner.data_dir.join(PROVIDER_BINDINGS_FILE);
        if bindings_path.exists() {
            let content = fs::read_to_string(&bindings_path)
                .map_err(|e| format!("Failed to read provider bindings: {}", e))?;
            let store: ProviderBindingsStore = serde_json::from_str(&content)
                .map_err(|e| format!("Failed to parse provider bindings: {}", e))?;
            inner.bindings = store.bindings;
        }

        // Load usage stats
        let usage_path = inner.data_dir.join(USAGE_FILE);
        if usage_path.exists() {
            let content = fs::read_to_string(&usage_path)
                .map_err(|e| format!("Failed to read usage stats: {}", e))?;
            let store: UsageStore = serde_json::from_str(&content)
                .map_err(|e| format!("Failed to parse usage stats: {}", e))?;
            inner.usage = store.usage;
        }

        Ok(())
    }

    /// Save all data to disk (no-op for in-memory stores).
    async fn save_all(&self) -> Result<(), String> {
        let inner = self.inner.read().await;
        if inner.in_memory {
            return Ok(());
        }

        // Save virtual keys
        let keys_store = VirtualKeysStore {
            version: STORE_VERSION,
            keys: inner.keys.clone(),
        };
        let keys_json = serde_json::to_string_pretty(&keys_store)
            .map_err(|e| format!("Failed to serialize virtual keys: {}", e))?;
        fs::write(inner.data_dir.join(VIRTUAL_KEYS_FILE), keys_json)
            .map_err(|e| format!("Failed to write virtual keys: {}", e))?;

        // Save provider bindings
        let bindings_store = ProviderBindingsStore {
            version: STORE_VERSION,
            bindings: inner.bindings.clone(),
        };
        let bindings_json = serde_json::to_string_pretty(&bindings_store)
            .map_err(|e| format!("Failed to serialize provider bindings: {}", e))?;
        fs::write(inner.data_dir.join(PROVIDER_BINDINGS_FILE), bindings_json)
            .map_err(|e| format!("Failed to write provider bindings: {}", e))?;

        // Save usage stats
        let usage_store = UsageStore {
            version: STORE_VERSION,
            usage: inner.usage.clone(),
        };
        let usage_json = serde_json::to_string_pretty(&usage_store)
            .map_err(|e| format!("Failed to serialize usage stats: {}", e))?;
        fs::write(inner.data_dir.join(USAGE_FILE), usage_json)
            .map_err(|e| format!("Failed to write usage stats: {}", e))?;

        Ok(())
    }

    // --- Virtual Key Operations ---

    /// Get all virtual keys
    pub async fn list_keys(&self) -> Vec<VirtualKeyRecord> {
        let inner = self.inner.read().await;
        inner.keys.values().cloned().collect()
    }

    /// Get a virtual key by ID
    pub async fn get_key(&self, id: &str) -> Option<VirtualKeyRecord> {
        let inner = self.inner.read().await;
        inner.keys.get(id).cloned()
    }

    /// Get a virtual key by lookup hash
    pub async fn get_key_by_hash(&self, lookup_hash: &str) -> Option<VirtualKeyRecord> {
        let inner = self.inner.read().await;
        inner
            .keys
            .values()
            .find(|k| k.lookup_hash == lookup_hash)
            .cloned()
    }

    /// Store a virtual key
    pub async fn store_key(&self, key: VirtualKeyRecord) -> Result<(), String> {
        {
            let mut inner = self.inner.write().await;
            inner.keys.insert(key.id.clone(), key);
        }
        self.save_all().await
    }

    /// Delete a virtual key
    pub async fn delete_key(&self, id: &str) -> Result<bool, String> {
        {
            let mut inner = self.inner.write().await;
            if inner.keys.remove(id).is_none() {
                return Ok(false);
            }
            // Also remove usage stats
            inner.usage.remove(id);
        }
        self.save_all().await?;
        Ok(true)
    }

    // --- Provider Binding Operations ---

    /// Get all provider bindings
    pub async fn list_bindings(&self) -> Vec<WebProviderBinding> {
        let inner = self.inner.read().await;
        inner.bindings.values().cloned().collect()
    }

    /// Get a provider binding by ID
    pub async fn get_binding(&self, id: &str) -> Option<WebProviderBinding> {
        let inner = self.inner.read().await;
        inner.bindings.get(id).cloned()
    }

    /// Store a provider binding
    pub async fn store_binding(&self, binding: WebProviderBinding) -> Result<(), String> {
        {
            let mut inner = self.inner.write().await;
            inner.bindings.insert(binding.id.clone(), binding);
        }
        self.save_all().await
    }

    /// Delete a provider binding
    pub async fn delete_binding(&self, id: &str) -> Result<bool, String> {
        {
            let mut inner = self.inner.write().await;
            // Check if any keys reference this binding
            let in_use = inner
                .keys
                .values()
                .any(|k| k.provider_binding_ids.contains(&id.to_string()));
            if in_use {
                return Err("Cannot delete binding: still referenced by virtual keys".to_string());
            }
            if inner.bindings.remove(id).is_none() {
                return Ok(false);
            }
        }
        self.save_all().await?;
        Ok(true)
    }

    // --- Usage Operations ---

    /// Get usage stats for a key
    pub async fn get_usage(&self, key_id: &str) -> VirtualKeyUsage {
        let inner = self.inner.read().await;
        inner.usage.get(key_id).cloned().unwrap_or_default()
    }

    /// Record usage for a key
    pub async fn record_usage(
        &self,
        key_id: &str,
        input_tokens: u64,
        output_tokens: u64,
    ) -> Result<(), String> {
        {
            let mut inner = self.inner.write().await;
            let usage = inner.usage.entry(key_id.to_string()).or_default();
            usage.total_requests += 1;
            usage.total_tokens_input += input_tokens;
            usage.total_tokens_output += output_tokens;
            usage.last_request_at = Some(chrono::Utc::now());
        }
        self.save_all().await
    }

    /// Reset usage stats for a key
    pub async fn reset_usage(&self, key_id: &str) -> Result<(), String> {
        {
            let mut inner = self.inner.write().await;
            inner.usage.remove(key_id);
        }
        self.save_all().await
    }

    /// Create a transient in-memory store (for tests and ephemeral scenarios).
    /// No files are read or written.
    pub async fn new_in_memory() -> Self {
        let inner = StorageInner {
            data_dir: std::env::temp_dir().join("kaizen_webkeys_inmem"),
            in_memory: true,
            keys: HashMap::new(),
            bindings: HashMap::new(),
            usage: HashMap::new(),
        };
        Self {
            inner: Arc::new(RwLock::new(inner)),
        }
    }
}

fn default_data_dir() -> PathBuf {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string());
    Path::new(&home).join(".kaizen").join("webkeys")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_storage_crud() {
        let temp_dir = TempDir::new().unwrap();
        let storage = WebKeysStorage::with_dir(temp_dir.path().to_path_buf())
            .await
            .unwrap();

        // Test virtual key storage
        let key = VirtualKeyRecord {
            id: "test-key-1".to_string(),
            name: "Test Key".to_string(),
            lookup_hash: "abc123".to_string(),
            fingerprint: "vkfp_ABCD1234".to_string(),
            preview: "sk-vt-...wxyz".to_string(),
            enabled: true,
            provider_binding_ids: vec!["binding-1".to_string()],
            default_binding_id: Some("binding-1".to_string()),
            model_allowlist: None,
            metadata: None,
            rate_limit_rpm: Some(60),
            rate_limit_tpm: Some(10000),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            last_used_at: None,
        };

        storage.store_key(key.clone()).await.unwrap();
        let retrieved = storage.get_key(&key.id).await.unwrap();
        assert_eq!(retrieved.id, key.id);

        // Test provider binding storage
        let binding = WebProviderBinding {
            id: "binding-1".to_string(),
            provider_type: WebProviderType::ChatGptWeb,
            account_id: "user@example.com".to_string(),
            profile_path: "/path/to/profile".to_string(),
            display_name: "My ChatGPT".to_string(),
            enabled: true,
            binding_fingerprint: "bkfp_XYZ789".to_string(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            last_used_at: None,
        };

        storage.store_binding(binding.clone()).await.unwrap();
        let retrieved_binding = storage.get_binding(&binding.id).await.unwrap();
        assert_eq!(retrieved_binding.id, binding.id);

        // Test deletion fails when key references binding
        let result = storage.delete_binding(&binding.id).await;
        assert!(result.is_err());

        // Delete key first, then binding
        storage.delete_key(&key.id).await.unwrap();
        let result = storage.delete_binding(&binding.id).await.unwrap();
        assert!(result);
    }
}
