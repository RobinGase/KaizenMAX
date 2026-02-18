//! Provider and virtual-key types for KaizenMAX.
//!
//! Ported from kai-zen-tunnel `virtual_key/types.rs` and extended to match
//! KaizenMAX's provider surface (adds `AnthropicApi`, `GeminiCli`,
//! `NvidiaApi`, `OpenCode`).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// Re-use the canonical prefix from the webkeys surface.
pub use crate::webkeys::types::VIRTUAL_KEY_PREFIX;

/// Opaque provider identifier.
pub type ProviderId = String;

/// Opaque virtual-key identifier.
pub type VirtualKeyId = String;

// ---------------------------------------------------------------------------
// Provider classification
// ---------------------------------------------------------------------------

/// All provider variants understood by KaizenMAX.
///
/// Web variants use browser automation (chromiumoxide).
/// Api variants use HTTP.
/// Cli variants use subprocess invocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProviderType {
    /// ChatGPT web interface via embedded Chromium.
    ChatGptWeb,
    /// Gemini web interface via embedded Chromium.
    GeminiWeb,
    /// OpenAI HTTP API.
    OpenAiApi,
    /// Anthropic HTTP API.
    AnthropicApi,
    /// Google Gemini HTTP API.
    GeminiApi,
    /// NVIDIA API (OpenAI-compatible endpoint).
    NvidiaApi,
    /// Google Gemini CLI (subprocess, OAuth-based, largely free).
    GeminiCli,
    /// OpenCode AI CLI (subprocess).
    OpenCode,
    /// Generic OpenAI-compatible HTTP API.
    OpenAiCompatible,
}

impl ProviderType {
    /// Returns `true` if this provider uses browser automation.
    pub fn is_web(&self) -> bool {
        matches!(self, ProviderType::ChatGptWeb | ProviderType::GeminiWeb)
    }

    /// Returns `true` if this provider uses a direct HTTP API.
    pub fn is_api(&self) -> bool {
        matches!(
            self,
            ProviderType::OpenAiApi
                | ProviderType::AnthropicApi
                | ProviderType::GeminiApi
                | ProviderType::NvidiaApi
                | ProviderType::OpenAiCompatible
        )
    }

    /// Returns `true` if this provider invokes a local CLI subprocess.
    pub fn is_cli(&self) -> bool {
        matches!(self, ProviderType::GeminiCli | ProviderType::OpenCode)
    }

    /// Default browser profile directory name for web providers.
    /// Returns `None` for non-web providers.
    pub fn profile_dir(&self) -> Option<&'static str> {
        match self {
            ProviderType::ChatGptWeb => Some("chatgpt"),
            ProviderType::GeminiWeb => Some("gemini"),
            _ => None,
        }
    }

    /// CLI binary name for CLI providers.
    /// Returns `None` for non-CLI providers.
    pub fn cli_binary(&self) -> Option<&'static str> {
        match self {
            ProviderType::GeminiCli => Some("gemini"),
            ProviderType::OpenCode => Some("opencode"),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Authentication
// ---------------------------------------------------------------------------

/// How a provider authenticates requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AuthType {
    /// Browser session (cookies / localStorage).
    BrowserSession,
    /// Static API key.
    ApiKey,
    /// OAuth2 access token (stored in vault).
    OAuthToken,
    /// CLI handles auth internally (e.g., `gemini auth login`).
    CliManaged,
}

// ---------------------------------------------------------------------------
// Rate limiting
// ---------------------------------------------------------------------------

/// Per-provider rate limiting configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimit {
    /// Maximum requests per minute (`None` = unlimited).
    pub requests_per_minute: Option<u32>,
    /// Maximum requests per day (`None` = unlimited).
    pub requests_per_day: Option<u32>,
}

// ---------------------------------------------------------------------------
// Provider record
// ---------------------------------------------------------------------------

/// Full provider record (stored internally; contains encrypted secrets).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Provider {
    pub id: ProviderId,
    pub name: String,
    pub provider_type: ProviderType,
    pub auth_type: AuthType,
    pub enabled: bool,
    pub base_url: Option<String>,
    pub default_model: Option<String>,
    pub model_allowlist: Option<Vec<String>>,
    /// AES-256-GCM encrypted secret (API key or OAuth token).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secret_ciphertext: Option<String>,
    /// Last-4 hint for display: `"***a3f2"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secret_hint: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Public provider record — secrets stripped, `has_secret` flag added.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderPublic {
    pub id: ProviderId,
    pub name: String,
    pub provider_type: ProviderType,
    pub auth_type: AuthType,
    pub enabled: bool,
    pub base_url: Option<String>,
    pub default_model: Option<String>,
    pub model_allowlist: Option<Vec<String>>,
    pub has_secret: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<Provider> for ProviderPublic {
    fn from(p: Provider) -> Self {
        Self {
            id: p.id,
            name: p.name,
            provider_type: p.provider_type,
            auth_type: p.auth_type,
            enabled: p.enabled,
            base_url: p.base_url,
            default_model: p.default_model,
            model_allowlist: p.model_allowlist,
            has_secret: p.secret_ciphertext.is_some(),
            created_at: p.created_at,
            updated_at: p.updated_at,
        }
    }
}

// ---------------------------------------------------------------------------
// Virtual key record (provider-agnostic)
// ---------------------------------------------------------------------------

/// Full virtual key record (stored internally; key_hash is secret).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VirtualKey {
    pub id: VirtualKeyId,
    pub name: String,
    /// HMAC-SHA256 of raw key (for constant-time verification).
    pub key_hash: String,
    /// Display preview: `"sk-vt-...a3f2"`.
    pub key_preview: String,
    /// Log-safe fingerprint: `"vkfp_<base32>"`.
    pub fingerprint: String,
    pub enabled: bool,
    /// Provider IDs this key can access.
    pub provider_ids: Vec<ProviderId>,
    /// Default provider when not specified in request.
    pub default_provider_id: Option<ProviderId>,
    /// Model allowlist (`None` = all models allowed).
    pub model_allowlist: Option<Vec<String>>,
    pub rate_limit: Option<RateLimit>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_used_at: Option<DateTime<Utc>>,
}

/// Public virtual key record (key_hash excluded).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VirtualKeyPublic {
    pub id: VirtualKeyId,
    pub name: String,
    pub key_preview: String,
    pub fingerprint: String,
    pub enabled: bool,
    pub provider_ids: Vec<ProviderId>,
    pub default_provider_id: Option<ProviderId>,
    pub model_allowlist: Option<Vec<String>>,
    pub rate_limit: Option<RateLimit>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_used_at: Option<DateTime<Utc>>,
}

impl From<VirtualKey> for VirtualKeyPublic {
    fn from(vk: VirtualKey) -> Self {
        Self {
            id: vk.id,
            name: vk.name,
            key_preview: vk.key_preview,
            fingerprint: vk.fingerprint,
            enabled: vk.enabled,
            provider_ids: vk.provider_ids,
            default_provider_id: vk.default_provider_id,
            model_allowlist: vk.model_allowlist,
            rate_limit: vk.rate_limit,
            created_at: vk.created_at,
            updated_at: vk.updated_at,
            last_used_at: vk.last_used_at,
        }
    }
}

// ---------------------------------------------------------------------------
// Result / input types
// ---------------------------------------------------------------------------

/// Returned when a virtual key is created (includes the one-time raw key).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateVirtualKeyResult {
    pub raw_key: String,
    pub virtual_key: VirtualKeyPublic,
}

/// Input for creating or updating a provider record.
#[derive(Debug, Clone, Deserialize)]
pub struct CreateProviderInput {
    pub id: Option<ProviderId>,
    pub name: String,
    pub provider_type: ProviderType,
    pub auth_type: AuthType,
    pub enabled: Option<bool>,
    pub base_url: Option<String>,
    pub default_model: Option<String>,
    pub model_allowlist: Option<Vec<String>>,
    /// Plaintext secret (will be encrypted on write).
    pub secret: Option<String>,
}

/// Input for creating a virtual key.
#[derive(Debug, Clone, Deserialize)]
pub struct CreateVirtualKeyInput {
    pub name: String,
    pub provider_ids: Vec<ProviderId>,
    pub default_provider_id: Option<ProviderId>,
    pub model_allowlist: Option<Vec<String>>,
    pub rate_limit: Option<RateLimit>,
}

/// Result of verifying a presented raw key.
#[derive(Debug, Clone)]
pub struct VirtualKeyVerification {
    pub virtual_key: VirtualKeyPublic,
    pub provider_ids: Vec<ProviderId>,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_type_web_classification() {
        assert!(ProviderType::ChatGptWeb.is_web());
        assert!(ProviderType::GeminiWeb.is_web());
        assert!(!ProviderType::OpenAiApi.is_web());
        assert!(!ProviderType::GeminiCli.is_web());
    }

    #[test]
    fn provider_type_api_classification() {
        assert!(ProviderType::OpenAiApi.is_api());
        assert!(ProviderType::AnthropicApi.is_api());
        assert!(ProviderType::GeminiApi.is_api());
        assert!(ProviderType::NvidiaApi.is_api());
        assert!(!ProviderType::GeminiWeb.is_api());
        assert!(!ProviderType::GeminiCli.is_api());
    }

    #[test]
    fn provider_type_cli_classification() {
        assert!(ProviderType::GeminiCli.is_cli());
        assert!(ProviderType::OpenCode.is_cli());
        assert!(!ProviderType::OpenAiApi.is_cli());
    }

    #[test]
    fn profile_dir_only_for_web() {
        assert_eq!(ProviderType::ChatGptWeb.profile_dir(), Some("chatgpt"));
        assert_eq!(ProviderType::GeminiWeb.profile_dir(), Some("gemini"));
        assert_eq!(ProviderType::OpenAiApi.profile_dir(), None);
        assert_eq!(ProviderType::GeminiCli.profile_dir(), None);
    }

    #[test]
    fn cli_binary_only_for_cli() {
        assert_eq!(ProviderType::GeminiCli.cli_binary(), Some("gemini"));
        assert_eq!(ProviderType::OpenCode.cli_binary(), Some("opencode"));
        assert_eq!(ProviderType::OpenAiApi.cli_binary(), None);
    }

    #[test]
    fn provider_to_public_strips_secret() {
        let p = Provider {
            id: "p1".to_string(),
            name: "Test".to_string(),
            provider_type: ProviderType::OpenAiApi,
            auth_type: AuthType::ApiKey,
            enabled: true,
            base_url: None,
            default_model: None,
            model_allowlist: None,
            secret_ciphertext: Some("encrypted_blob".to_string()),
            secret_hint: Some("***1234".to_string()),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        let pub_p: ProviderPublic = p.into();
        assert!(pub_p.has_secret);
    }

    #[test]
    fn provider_to_public_no_secret() {
        let p = Provider {
            id: "p2".to_string(),
            name: "Gemini Web".to_string(),
            provider_type: ProviderType::GeminiWeb,
            auth_type: AuthType::BrowserSession,
            enabled: true,
            base_url: None,
            default_model: None,
            model_allowlist: None,
            secret_ciphertext: None,
            secret_hint: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        let pub_p: ProviderPublic = p.into();
        assert!(!pub_p.has_secret);
    }
}
