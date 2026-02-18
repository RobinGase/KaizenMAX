//! WebKeys Service Layer
//!
//! Business logic for virtual key management including fingerprint generation,
//! HMAC hashing, and validation.

use super::storage::WebKeysStorage;
use super::types::*;
use base32::Alphabet;
use chrono::Utc;
use hmac::{Hmac, Mac};
use rand::RngCore;
use sha2::Sha256;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Type alias for HMAC-SHA256
pub type HmacSha256 = Hmac<Sha256>;

/// Service configuration
#[derive(Debug, Clone)]
pub struct WebKeysServiceConfig {
    /// Server pepper for HMAC operations (must be 32+ bytes)
    pub server_pepper: String,
}

impl Default for WebKeysServiceConfig {
    fn default() -> Self {
        Self {
            server_pepper: std::env::var("WEBKEYS_SERVER_PEPPER")
                .unwrap_or_else(|_| "default-pepper-change-in-production".to_string()),
        }
    }
}

/// Virtual key service
#[derive(Clone)]
pub struct WebKeysService {
    storage: WebKeysStorage,
    config: WebKeysServiceConfig,
    rate_limiter: Arc<RwLock<RateLimiter>>,
}

/// Simple in-memory rate limiter (requests per minute)
#[derive(Debug, Default)]
struct RateLimiter {
    buckets: std::collections::HashMap<String, RateBucket>,
}

#[derive(Debug, Clone, Default)]
struct RateBucket {
    requests: Vec<chrono::DateTime<Utc>>,
}

impl WebKeysService {
    /// Create a new service with default config
    pub async fn new() -> Result<Self, String> {
        Self::with_config(WebKeysServiceConfig::default()).await
    }

    /// Create a new service with specific config
    pub async fn with_config(config: WebKeysServiceConfig) -> Result<Self, String> {
        let storage = WebKeysStorage::new().await?;
        Ok(Self {
            storage,
            config,
            rate_limiter: Arc::new(RwLock::new(RateLimiter::default())),
        })
    }

    /// Generate a new virtual key
    pub async fn create_virtual_key(
        &self,
        request: CreateVirtualKeyRequest,
    ) -> Result<VirtualKeyCreationResult, String> {
        // Validate request
        if request.name.trim().is_empty() {
            return Err("Key name is required".to_string());
        }

        if request.provider_binding_ids.is_empty() {
            return Err("At least one provider binding is required".to_string());
        }

        // Validate all binding IDs exist
        for binding_id in &request.provider_binding_ids {
            if self.storage.get_binding(binding_id).await.is_none() {
                return Err(format!("Provider binding '{}' not found", binding_id));
            }
        }

        // Validate default binding is in the list
        if let Some(ref default_id) = request.default_binding_id {
            if !request.provider_binding_ids.contains(default_id) {
                return Err("Default binding must be in the provider binding list".to_string());
            }
        }

        // Generate raw key
        let raw_key = generate_virtual_key();

        // Compute fingerprints
        let lookup_hash = compute_lookup_hash(&self.config.server_pepper, &raw_key);
        let fingerprint = compute_fingerprint(&self.config.server_pepper, &raw_key);
        let preview = create_key_preview(&raw_key);

        let now = Utc::now();
        let key_record = VirtualKeyRecord {
            id: uuid::Uuid::new_v4().to_string(),
            name: request.name.trim().to_string(),
            lookup_hash,
            fingerprint,
            preview,
            enabled: true,
            provider_binding_ids: request.provider_binding_ids,
            default_binding_id: request.default_binding_id,
            model_allowlist: request.model_allowlist,
            metadata: request.metadata,
            rate_limit_rpm: request.rate_limit_rpm,
            rate_limit_tpm: request.rate_limit_tpm,
            created_at: now,
            updated_at: now,
            last_used_at: None,
        };

        self.storage.store_key(key_record.clone()).await?;

        Ok(VirtualKeyCreationResult {
            raw_key,
            key_record: key_record.into(),
        })
    }

    /// Rotate a virtual key (generates new raw key)
    pub async fn rotate_virtual_key(&self, key_id: &str) -> Result<VirtualKeyCreationResult, String> {
        let existing = self
            .storage
            .get_key(key_id)
            .await
            .ok_or_else(|| "Key not found".to_string())?;

        // Generate new raw key
        let raw_key = generate_virtual_key();

        // Compute new fingerprints
        let lookup_hash = compute_lookup_hash(&self.config.server_pepper, &raw_key);
        let fingerprint = compute_fingerprint(&self.config.server_pepper, &raw_key);
        let preview = create_key_preview(&raw_key);

        let mut updated = existing;
        updated.lookup_hash = lookup_hash;
        updated.fingerprint = fingerprint;
        updated.preview = preview;
        updated.updated_at = Utc::now();

        self.storage.store_key(updated.clone()).await?;

        Ok(VirtualKeyCreationResult {
            raw_key,
            key_record: updated.into(),
        })
    }

    /// List all virtual keys
    pub async fn list_virtual_keys(&self) -> Vec<VirtualKeyPublicRecord> {
        self.storage
            .list_keys()
            .await
            .into_iter()
            .map(|k| k.into())
            .collect()
    }

    /// Get a virtual key by ID
    pub async fn get_virtual_key(&self, id: &str) -> Option<VirtualKeyPublicRecord> {
        self.storage.get_key(id).await.map(|k| k.into())
    }

    /// Update a virtual key
    pub async fn update_virtual_key(
        &self,
        id: &str,
        request: UpdateVirtualKeyRequest,
    ) -> Result<VirtualKeyPublicRecord, String> {
        let mut existing = self
            .storage
            .get_key(id)
            .await
            .ok_or_else(|| "Key not found".to_string())?;

        if let Some(name) = request.name {
            if name.trim().is_empty() {
                return Err("Key name cannot be empty".to_string());
            }
            existing.name = name.trim().to_string();
        }

        if let Some(enabled) = request.enabled {
            existing.enabled = enabled;
        }

        if let Some(binding_ids) = request.provider_binding_ids {
            if binding_ids.is_empty() {
                return Err("At least one provider binding is required".to_string());
            }
            // Validate all binding IDs exist
            for binding_id in &binding_ids {
                if self.storage.get_binding(binding_id).await.is_none() {
                    return Err(format!("Provider binding '{}' not found", binding_id));
                }
            }
            existing.provider_binding_ids = binding_ids;
        }

        if let Some(default_id) = request.default_binding_id {
            if !existing.provider_binding_ids.contains(&default_id) {
                return Err("Default binding must be in the provider binding list".to_string());
            }
            existing.default_binding_id = Some(default_id);
        }

        if let Some(allowlist) = request.model_allowlist {
            existing.model_allowlist = if allowlist.is_empty() {
                None
            } else {
                Some(allowlist)
            };
        }

        if let Some(metadata) = request.metadata {
            existing.metadata = if metadata.is_empty() { None } else { Some(metadata) };
        }

        if let Some(rpm) = request.rate_limit_rpm {
            existing.rate_limit_rpm = Some(rpm);
        }

        if let Some(tpm) = request.rate_limit_tpm {
            existing.rate_limit_tpm = Some(tpm);
        }

        existing.updated_at = Utc::now();

        self.storage.store_key(existing.clone()).await?;
        Ok(existing.into())
    }

    /// Delete a virtual key
    pub async fn delete_virtual_key(&self, id: &str) -> Result<bool, String> {
        self.storage.delete_key(id).await
    }

    /// Verify a virtual key and get binding
    pub async fn verify_virtual_key(
        &self,
        raw_key: &str,
        preferred_binding_id: Option<&str>,
    ) -> Result<(VirtualKeyPublicRecord, String), String> {
        let lookup_hash = compute_lookup_hash(&self.config.server_pepper, raw_key);

        let key = self
            .storage
            .get_key_by_hash(&lookup_hash)
            .await
            .ok_or_else(|| "Invalid virtual key".to_string())?;

        if !key.enabled {
            return Err("Virtual key is disabled".to_string());
        }

        // Check rate limits
        if let Some(rpm) = key.rate_limit_rpm {
            if self.is_rate_limited(&key.id, rpm, 60).await {
                return Err(format!(
                    "Rate limit exceeded: {} requests per minute",
                    rpm
                ));
            }
        }

        // Select binding
        let binding_id = if let Some(preferred) = preferred_binding_id {
            if key.provider_binding_ids.contains(&preferred.to_string()) {
                preferred.to_string()
            } else {
                return Err(format!(
                    "Preferred binding '{}' not authorized for this key",
                    preferred
                ));
            }
        } else {
            key.default_binding_id
                .clone()
                .or_else(|| key.provider_binding_ids.first().cloned())
                .ok_or_else(|| "No provider bindings available".to_string())?
        };

        // Verify binding exists and is enabled
        let binding = self
            .storage
            .get_binding(&binding_id)
            .await
            .ok_or_else(|| "Provider binding not found".to_string())?;

        if !binding.enabled {
            return Err("Provider binding is disabled".to_string());
        }

        // Update last used
        let mut updated = key.clone();
        updated.last_used_at = Some(Utc::now());
        self.storage.store_key(updated).await?;

        Ok((key.into(), binding_id))
    }

    /// Check if rate limited
    async fn is_rate_limited(&self, key_id: &str, limit: u32, window_secs: u64) -> bool {
        let now = Utc::now();
        let window_start = now - chrono::Duration::seconds(window_secs as i64);

        let mut limiter = self.rate_limiter.write().await;
        let bucket = limiter.buckets.entry(key_id.to_string()).or_default();

        // Remove old requests outside window
        bucket.requests.retain(|&t| t > window_start);

        if bucket.requests.len() >= limit as usize {
            return true;
        }

        bucket.requests.push(now);
        false
    }

    // --- Provider Binding Operations ---

    /// Create a provider binding
    pub async fn create_provider_binding(
        &self,
        request: CreateProviderBindingRequest,
    ) -> Result<WebProviderBindingPublicRecord, String> {
        if request.account_id.trim().is_empty() {
            return Err("Account ID is required".to_string());
        }

        if request.display_name.trim().is_empty() {
            return Err("Display name is required".to_string());
        }

        if request.profile_path.trim().is_empty() {
            return Err("Profile path is required".to_string());
        }

        // Compute binding fingerprint
        let binding_fp_input = format!(
            "{}|{}|{}",
            request.provider_type.as_str(),
            request.account_id,
            request.profile_path
        );
        let binding_fingerprint = compute_binding_fingerprint(&self.config.server_pepper, &binding_fp_input);

        let now = Utc::now();
        let binding = WebProviderBinding {
            id: uuid::Uuid::new_v4().to_string(),
            provider_type: request.provider_type,
            account_id: request.account_id.trim().to_string(),
            profile_path: request.profile_path.trim().to_string(),
            display_name: request.display_name.trim().to_string(),
            enabled: true,
            binding_fingerprint,
            created_at: now,
            updated_at: now,
            last_used_at: None,
        };

        self.storage.store_binding(binding.clone()).await?;
        Ok(binding.into())
    }

    /// List all provider bindings
    pub async fn list_provider_bindings(&self) -> Vec<WebProviderBindingPublicRecord> {
        self.storage
            .list_bindings()
            .await
            .into_iter()
            .map(|b| b.into())
            .collect()
    }

    /// Get a provider binding by ID (public record)
    pub async fn get_provider_binding(&self, id: &str) -> Option<WebProviderBindingPublicRecord> {
        self.storage.get_binding(id).await.map(|b| b.into())
    }

    /// Get a full provider binding by ID (internal use)
    pub async fn get_provider_binding_full(&self, id: &str) -> Option<WebProviderBinding> {
        self.storage.get_binding(id).await
    }

    /// Update a provider binding
    pub async fn update_provider_binding(
        &self,
        id: &str,
        request: UpdateProviderBindingRequest,
    ) -> Result<WebProviderBindingPublicRecord, String> {
        let mut existing = self
            .storage
            .get_binding(id)
            .await
            .ok_or_else(|| "Binding not found".to_string())?;

        if let Some(display_name) = request.display_name {
            if display_name.trim().is_empty() {
                return Err("Display name cannot be empty".to_string());
            }
            existing.display_name = display_name.trim().to_string();
        }

        if let Some(enabled) = request.enabled {
            existing.enabled = enabled;
        }

        existing.updated_at = Utc::now();

        self.storage.store_binding(existing.clone()).await?;
        Ok(existing.into())
    }

    /// Delete a provider binding
    pub async fn delete_provider_binding(&self, id: &str) -> Result<bool, String> {
        self.storage.delete_binding(id).await
    }

    // --- Usage Operations ---

    /// Record usage for a key
    pub async fn record_usage(
        &self,
        key_id: &str,
        input_tokens: u64,
        output_tokens: u64,
    ) -> Result<(), String> {
        self.storage
            .record_usage(key_id, input_tokens, output_tokens)
            .await
    }

    /// Get usage stats for a key
    pub async fn get_usage(&self, key_id: &str) -> VirtualKeyUsage {
        self.storage.get_usage(key_id).await
    }
}

/// Generate a new virtual key
fn generate_virtual_key() -> String {
    let mut random_bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut random_bytes);
    let random_part = base64_url::encode(&random_bytes);
    format!("{}{}", VIRTUAL_KEY_PREFIX, random_part)
}

/// Compute lookup hash (HMAC-SHA256, hex)
fn compute_lookup_hash(server_pepper: &str, raw_key: &str) -> String {
    let mut mac = HmacSha256::new_from_slice(server_pepper.as_bytes())
        .expect("HMAC can take key of any size");
    mac.update(raw_key.as_bytes());
    let result = mac.finalize();
    hex::encode(result.into_bytes())
}

/// Compute fingerprint (vkfp_<base32[0..16]>)
fn compute_fingerprint(server_pepper: &str, raw_key: &str) -> String {
    let hash = compute_lookup_hash(server_pepper, raw_key);
    let hash_bytes = hex::decode(&hash).expect("Valid hex");
    let base32_encoded = base32::encode(Alphabet::Rfc4648 { padding: false }, &hash_bytes);
    let fingerprint_part: String = base32_encoded.chars().take(16).collect();
    format!("vkfp_{}", fingerprint_part)
}

/// Create key preview (sk-vt-...<last4>)
fn create_key_preview(raw_key: &str) -> String {
    if raw_key.len() < 8 {
        return format!("{}...****", VIRTUAL_KEY_PREFIX);
    }
    let last4 = &raw_key[raw_key.len() - 4..];
    format!("{}...{}", VIRTUAL_KEY_PREFIX, last4)
}

/// Compute binding fingerprint (bkfp_<base32[0..16]>)
fn compute_binding_fingerprint(server_pepper: &str, binding_input: &str) -> String {
    let mut mac = HmacSha256::new_from_slice(server_pepper.as_bytes())
        .expect("HMAC can take key of any size");
    mac.update(binding_input.as_bytes());
    let result = mac.finalize();
    let hash_bytes = result.into_bytes();
    let base32_encoded = base32::encode(Alphabet::Rfc4648 { padding: false }, &hash_bytes);
    let fingerprint_part: String = base32_encoded.chars().take(16).collect();
    format!("bkfp_{}", fingerprint_part)
}

/// Base64 URL encoding (no padding)
mod base64_url {
    pub fn encode(input: &[u8]) -> String {
        use base64::Engine;
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(input)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_key_generation() {
        let key = generate_virtual_key();
        assert!(key.starts_with(VIRTUAL_KEY_PREFIX));
        assert!(key.len() > VIRTUAL_KEY_PREFIX.len());
    }

    #[test]
    fn test_fingerprint_determinism() {
        let pepper = "test-pepper-12345678901234567890";
        let raw_key = "sk-vt-testkey123";

        let fp1 = compute_fingerprint(pepper, raw_key);
        let fp2 = compute_fingerprint(pepper, raw_key);

        assert_eq!(fp1, fp2);
        assert!(fp1.starts_with("vkfp_"));
        assert_eq!(fp1.len(), 21); // "vkfp_" + 16 chars
    }

    #[test]
    fn test_fingerprint_uniqueness() {
        let pepper = "test-pepper-12345678901234567890";
        let key1 = "sk-vt-key1";
        let key2 = "sk-vt-key2";

        let fp1 = compute_fingerprint(pepper, key1);
        let fp2 = compute_fingerprint(pepper, key2);

        assert_ne!(fp1, fp2);
    }

    #[test]
    fn test_lookup_hash() {
        let pepper = "test-pepper";
        let raw_key = "sk-vt-test";

        let hash1 = compute_lookup_hash(pepper, raw_key);
        let hash2 = compute_lookup_hash(pepper, raw_key);

        assert_eq!(hash1, hash2);
        assert_eq!(hash1.len(), 64); // SHA-256 hex = 64 chars
    }

    #[test]
    fn test_key_preview() {
        let key = "sk-vt-abcdefghijklmnopqrstuvwxyz";
        let preview = create_key_preview(key);

        assert!(preview.starts_with(VIRTUAL_KEY_PREFIX));
        assert!(preview.contains("..."));
        assert!(preview.ends_with("wxyz"));
    }

    #[test]
    fn test_binding_fingerprint() {
        let pepper = "test-pepper";
        let input = "chatgpt_web|user@example.com|/path/to/profile";

        let fp = compute_binding_fingerprint(pepper, input);

        assert!(fp.starts_with("bkfp_"));
        assert_eq!(fp.len(), 21); // "bkfp_" + 16 chars
    }
}
