//! Cryptographic utilities for providers and virtual keys.
//!
//! Provides:
//! - AES-256-GCM encryption / decryption of provider secrets
//! - HMAC-SHA256 hashing + constant-time verification of virtual-key tokens
//! - Base32 fingerprint generation (`vkfp_<16 chars>`)
//! - Key-preview generation (`sk-vt-...XXXX`)
//! - Virtual key generation (`sk-vt-<32-char URL-safe base64>`)
//!
//! Keys are loaded from environment variables
//! (`KAIZENMAX_ENCRYPTION_KEY`, `KAIZENMAX_HMAC_PEPPER`).
//! If absent, random keys are generated per-process (warn on startup).

use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, KeyInit, OsRng},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use data_encoding::BASE32_NOPAD;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::fmt::Write as FmtWrite;

use crate::providers::types::VIRTUAL_KEY_PREFIX;

// Re-export OsRng for fill_bytes calls; it implements RngCore.
use aes_gcm::aead::rand_core::RngCore;

type HmacSha256 = Hmac<Sha256>;

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum CryptoError {
    #[error("invalid ciphertext format")]
    InvalidFormat,
    #[error("decryption failed: {0}")]
    DecryptionFailed(String),
    #[error("invalid key length")]
    InvalidKeyLength,
}

// ---------------------------------------------------------------------------
// CryptoService
// ---------------------------------------------------------------------------

/// Cryptographic service for providers and virtual keys.
///
/// Create one instance at startup and share it via `Arc`.
pub struct CryptoService {
    /// AES-256 master encryption key (32 bytes).
    encryption_key: [u8; 32],
    /// HMAC pepper (server-side secret, 32 bytes).
    hmac_pepper: [u8; 32],
}

impl CryptoService {
    /// Create from environment variables, generating random keys if absent.
    pub fn new() -> Self {
        let encryption_key = Self::load_or_generate("KAIZENMAX_ENCRYPTION_KEY");
        let hmac_pepper = Self::load_or_generate("KAIZENMAX_HMAC_PEPPER");
        Self {
            encryption_key,
            hmac_pepper,
        }
    }

    /// Load a 32-byte key from an env var (URL-safe base64) or generate one.
    fn load_or_generate(env_var: &str) -> [u8; 32] {
        if let Ok(val) = std::env::var(env_var) {
            if let Ok(bytes) = URL_SAFE_NO_PAD.decode(val.trim()) {
                if bytes.len() == 32 {
                    let mut key = [0u8; 32];
                    key.copy_from_slice(&bytes);
                    return key;
                }
            }
        }
        tracing::warn!(
            "{} not set or invalid — using a random ephemeral key. \
             Set this env var for persistence across restarts.",
            env_var
        );
        let mut key = [0u8; 32];
        OsRng.fill_bytes(&mut key);
        key
    }

    // -----------------------------------------------------------------------
    // Hashing / verification
    // -----------------------------------------------------------------------

    /// Compute HMAC-SHA256 of `raw_key` using the server pepper, hex-encoded.
    pub fn hash_virtual_key(&self, raw_key: &str) -> String {
        let mut mac = <HmacSha256 as Mac>::new_from_slice(&self.hmac_pepper)
            .expect("HMAC accepts any key size");
        mac.update(raw_key.as_bytes());
        let bytes = mac.finalize().into_bytes();

        let mut hex = String::with_capacity(bytes.len() * 2);
        for b in bytes.iter() {
            let _ = write!(hex, "{:02x}", b);
        }
        hex
    }

    /// Constant-time comparison of `raw_key` against a stored hash.
    pub fn verify_key_hash(&self, raw_key: &str, expected_hash: &str) -> bool {
        let computed = self.hash_virtual_key(raw_key);
        constant_time_eq::constant_time_eq(computed.as_bytes(), expected_hash.as_bytes())
    }

    // -----------------------------------------------------------------------
    // Fingerprint / preview
    // -----------------------------------------------------------------------

    /// Generate a log-safe fingerprint: `"vkfp_<16 uppercase base32 chars>"`.
    pub fn fingerprint_virtual_key(&self, raw_key: &str) -> String {
        let mut mac = <HmacSha256 as Mac>::new_from_slice(&self.hmac_pepper)
            .expect("HMAC accepts any key size");
        mac.update(raw_key.as_bytes());
        let bytes = mac.finalize().into_bytes();

        // Encode first 10 bytes (80 bits) in base32 → 16 chars.
        let b32 = BASE32_NOPAD.encode(&bytes[..10]);
        format!("vkfp_{}", &b32[..16.min(b32.len())])
    }

    /// Generate a display preview: `"sk-vt-...XXXX"` (last 4 chars of key).
    pub fn create_key_preview(raw_key: &str) -> String {
        if raw_key.len() < 8 {
            return format!("{}...", raw_key);
        }
        let last4 = &raw_key[raw_key.len() - 4..];
        format!("sk-vt-...{}", last4)
    }

    /// Create a secret hint for display: `"***XXXX"` (last 4 chars).
    pub fn create_secret_hint(secret: &str) -> String {
        let s = secret.trim();
        if s.len() <= 4 {
            format!("***{}", s)
        } else {
            format!("***{}", &s[s.len() - 4..])
        }
    }

    // -----------------------------------------------------------------------
    // Key generation
    // -----------------------------------------------------------------------

    /// Generate a new random virtual key (`sk-vt-<32-char URL-safe base64>`).
    pub fn generate_virtual_key() -> String {
        let mut bytes = [0u8; 24];
        OsRng.fill_bytes(&mut bytes);
        format!("{}{}", VIRTUAL_KEY_PREFIX, URL_SAFE_NO_PAD.encode(bytes))
    }

    // -----------------------------------------------------------------------
    // AES-256-GCM encryption
    // -----------------------------------------------------------------------

    /// Encrypt `plaintext` with AES-256-GCM.
    ///
    /// Output format: `"<iv_b64>.<ciphertext_b64>"` where both parts are
    /// URL-safe base64 without padding.
    pub fn encrypt_secret(&self, plaintext: &str) -> Result<String, CryptoError> {
        let cipher = Aes256Gcm::new_from_slice(&self.encryption_key)
            .map_err(|_| CryptoError::InvalidKeyLength)?;

        let mut nonce_bytes = [0u8; 12];
        OsRng.fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);

        let ciphertext = cipher
            .encrypt(nonce, plaintext.as_bytes())
            .map_err(|e| CryptoError::DecryptionFailed(e.to_string()))?;

        Ok(format!(
            "{}.{}",
            URL_SAFE_NO_PAD.encode(nonce_bytes),
            URL_SAFE_NO_PAD.encode(ciphertext)
        ))
    }

    /// Decrypt a ciphertext produced by [`encrypt_secret`].
    pub fn decrypt_secret(&self, ciphertext: &str) -> Result<String, CryptoError> {
        let mut parts = ciphertext.splitn(2, '.');
        let iv_b64 = parts.next().ok_or(CryptoError::InvalidFormat)?;
        let ct_b64 = parts.next().ok_or(CryptoError::InvalidFormat)?;

        let iv_bytes = URL_SAFE_NO_PAD
            .decode(iv_b64)
            .map_err(|_| CryptoError::InvalidFormat)?;
        let ct_bytes = URL_SAFE_NO_PAD
            .decode(ct_b64)
            .map_err(|_| CryptoError::InvalidFormat)?;

        if iv_bytes.len() != 12 {
            return Err(CryptoError::InvalidFormat);
        }

        let cipher = Aes256Gcm::new_from_slice(&self.encryption_key)
            .map_err(|_| CryptoError::InvalidKeyLength)?;

        let nonce = Nonce::from_slice(&iv_bytes);
        let plaintext = cipher
            .decrypt(nonce, ct_bytes.as_ref())
            .map_err(|e| CryptoError::DecryptionFailed(e.to_string()))?;

        String::from_utf8(plaintext).map_err(|e| CryptoError::DecryptionFailed(e.to_string()))
    }
}

impl Default for CryptoService {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_key_has_correct_prefix() {
        let key = CryptoService::generate_virtual_key();
        assert!(key.starts_with(VIRTUAL_KEY_PREFIX), "key={key}");
        assert!(key.len() > VIRTUAL_KEY_PREFIX.len() + 8);
    }

    #[test]
    fn key_preview_format() {
        let key = "sk-vt-abc123def456";
        let preview = CryptoService::create_key_preview(key);
        assert!(preview.starts_with("sk-vt-..."), "preview={preview}");
        assert!(preview.ends_with("f456"), "preview={preview}");
    }

    #[test]
    fn hash_is_deterministic() {
        let cs = CryptoService::new();
        let h1 = cs.hash_virtual_key("test-key");
        let h2 = cs.hash_virtual_key("test-key");
        assert_eq!(h1, h2);
    }

    #[test]
    fn verify_correct_key_passes() {
        let cs = CryptoService::new();
        let key = "sk-vt-testkey123";
        let hash = cs.hash_virtual_key(key);
        assert!(cs.verify_key_hash(key, &hash));
    }

    #[test]
    fn verify_wrong_key_fails() {
        let cs = CryptoService::new();
        let hash = cs.hash_virtual_key("sk-vt-correct");
        assert!(!cs.verify_key_hash("sk-vt-wrong", &hash));
    }

    #[test]
    fn fingerprint_format() {
        let cs = CryptoService::new();
        let fp = cs.fingerprint_virtual_key("sk-vt-test");
        assert!(fp.starts_with("vkfp_"), "fp={fp}");
        assert_eq!(fp.len(), 21, "fp={fp}"); // "vkfp_" (5) + 16
    }

    #[test]
    fn encrypt_decrypt_roundtrip() {
        let cs = CryptoService::new();
        let secret = "my-super-secret-api-key-12345";
        let enc = cs.encrypt_secret(secret).unwrap();
        let dec = cs.decrypt_secret(&enc).unwrap();
        assert_eq!(dec, secret);
    }

    #[test]
    fn tampered_ciphertext_fails() {
        let cs = CryptoService::new();
        let enc = cs.encrypt_secret("hello").unwrap();
        // Corrupt the ciphertext portion.
        let corrupted = enc.replace('.', ".ZZZ");
        assert!(cs.decrypt_secret(&corrupted).is_err());
    }

    #[test]
    fn secret_hint_last4() {
        assert_eq!(
            CryptoService::create_secret_hint("sk-abcdef1234"),
            "***1234"
        );
    }

    #[test]
    fn secret_hint_short() {
        assert_eq!(CryptoService::create_secret_hint("abc"), "***abc");
    }
}
