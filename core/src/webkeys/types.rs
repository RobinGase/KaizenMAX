//! WebKeys Virtual Key System
//!
//! Provides virtual key management for ChatGPT/Gemini web clients.
//! Keys are hashed at rest with explicit fingerprints for observability.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Virtual key prefix
pub const VIRTUAL_KEY_PREFIX: &str = "sk-vt-";

/// Provider types for web clients
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WebProviderType {
    ChatGptWeb,
    GeminiWeb,
}

impl WebProviderType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ChatGptWeb => "chatgpt_web",
            Self::GeminiWeb => "gemini_web",
        }
    }
}

impl std::str::FromStr for WebProviderType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "chatgpt_web" => Ok(Self::ChatGptWeb),
            "gemini_web" => Ok(Self::GeminiWeb),
            _ => Err(format!("Unknown web provider type: {}", s)),
        }
    }
}

/// Web provider account binding
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebProviderBinding {
    pub id: String,
    pub provider_type: WebProviderType,
    pub account_id: String,
    pub profile_path: String,
    pub display_name: String,
    pub enabled: bool,
    pub binding_fingerprint: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_used_at: Option<DateTime<Utc>>,
}

/// Virtual key record (stored in persistence)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VirtualKeyRecord {
    pub id: String,
    pub name: String,
    /// HMAC-SHA256 hash of raw key (hex)
    pub lookup_hash: String,
    /// Fingerprint for logs/UI: vkfp_<base32[0..16]>
    pub fingerprint: String,
    /// Preview for UI: sk-vt-...<last4>
    pub preview: String,
    pub enabled: bool,
    /// Provider binding IDs this key can access
    pub provider_binding_ids: Vec<String>,
    /// Default binding ID (fallback)
    pub default_binding_id: Option<String>,
    /// Optional model allowlist
    pub model_allowlist: Option<Vec<String>>,
    /// Optional metadata
    pub metadata: Option<HashMap<String, String>>,
    /// Rate limit: requests per minute
    pub rate_limit_rpm: Option<u32>,
    /// Rate limit: tokens per minute  
    pub rate_limit_tpm: Option<u32>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_used_at: Option<DateTime<Utc>>,
}

/// Virtual key creation result (raw key shown once)
#[derive(Debug, Clone, Serialize)]
pub struct VirtualKeyCreationResult {
    pub raw_key: String,
    pub key_record: VirtualKeyPublicRecord,
}

/// Public virtual key record (excludes sensitive fields)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VirtualKeyPublicRecord {
    pub id: String,
    pub name: String,
    pub fingerprint: String,
    pub preview: String,
    pub enabled: bool,
    pub provider_binding_ids: Vec<String>,
    pub default_binding_id: Option<String>,
    pub model_allowlist: Option<Vec<String>>,
    pub metadata: Option<HashMap<String, String>>,
    pub rate_limit_rpm: Option<u32>,
    pub rate_limit_tpm: Option<u32>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_used_at: Option<DateTime<Utc>>,
}

impl From<VirtualKeyRecord> for VirtualKeyPublicRecord {
    fn from(record: VirtualKeyRecord) -> Self {
        Self {
            id: record.id,
            name: record.name,
            fingerprint: record.fingerprint,
            preview: record.preview,
            enabled: record.enabled,
            provider_binding_ids: record.provider_binding_ids,
            default_binding_id: record.default_binding_id,
            model_allowlist: record.model_allowlist,
            metadata: record.metadata,
            rate_limit_rpm: record.rate_limit_rpm,
            rate_limit_tpm: record.rate_limit_tpm,
            created_at: record.created_at,
            updated_at: record.updated_at,
            last_used_at: record.last_used_at,
        }
    }
}

/// Provider binding public record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebProviderBindingPublicRecord {
    pub id: String,
    pub provider_type: String,
    pub account_id: String,
    pub display_name: String,
    pub enabled: bool,
    pub binding_fingerprint: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_used_at: Option<DateTime<Utc>>,
}

impl From<WebProviderBinding> for WebProviderBindingPublicRecord {
    fn from(binding: WebProviderBinding) -> Self {
        Self {
            id: binding.id,
            provider_type: binding.provider_type.as_str().to_string(),
            account_id: binding.account_id,
            display_name: binding.display_name,
            enabled: binding.enabled,
            binding_fingerprint: binding.binding_fingerprint,
            created_at: binding.created_at,
            updated_at: binding.updated_at,
            last_used_at: binding.last_used_at,
        }
    }
}

/// Usage statistics for a virtual key
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VirtualKeyUsage {
    pub total_requests: u64,
    pub total_tokens_input: u64,
    pub total_tokens_output: u64,
    pub last_request_at: Option<DateTime<Utc>>,
}

/// Request context for validation
#[derive(Debug, Clone)]
pub struct ValidationContext {
    pub binding_id: String,
    pub model: Option<String>,
    pub estimated_tokens: Option<u32>,
}

/// Validation result
#[derive(Debug, Clone)]
pub enum ValidationResult {
    Valid { binding_id: String },
    Invalid { reason: String },
    RateLimited { retry_after_secs: u64 },
}

/// Create virtual key request
#[derive(Debug, Clone, Deserialize)]
pub struct CreateVirtualKeyRequest {
    pub name: String,
    pub provider_binding_ids: Vec<String>,
    pub default_binding_id: Option<String>,
    pub model_allowlist: Option<Vec<String>>,
    pub metadata: Option<HashMap<String, String>>,
    pub rate_limit_rpm: Option<u32>,
    pub rate_limit_tpm: Option<u32>,
}

/// Update virtual key request
#[derive(Debug, Clone, Deserialize)]
pub struct UpdateVirtualKeyRequest {
    pub name: Option<String>,
    pub enabled: Option<bool>,
    pub provider_binding_ids: Option<Vec<String>>,
    pub default_binding_id: Option<String>,
    pub model_allowlist: Option<Vec<String>>,
    pub metadata: Option<HashMap<String, String>>,
    pub rate_limit_rpm: Option<u32>,
    pub rate_limit_tpm: Option<u32>,
}

/// Create provider binding request
#[derive(Debug, Clone, Deserialize)]
pub struct CreateProviderBindingRequest {
    pub provider_type: WebProviderType,
    pub account_id: String,
    pub display_name: String,
    pub profile_path: String,
}

/// Update provider binding request
#[derive(Debug, Clone, Deserialize)]
pub struct UpdateProviderBindingRequest {
    pub display_name: Option<String>,
    pub enabled: Option<bool>,
}

/// Verify virtual key request
#[derive(Debug, Clone, Deserialize)]
pub struct VerifyVirtualKeyRequest {
    pub raw_key: String,
    pub preferred_binding_id: Option<String>,
}

/// Verify virtual key response
#[derive(Debug, Clone, Serialize)]
pub struct VerifyVirtualKeyResponse {
    pub valid: bool,
    pub key: Option<VirtualKeyPublicRecord>,
    pub selected_binding_id: Option<String>,
    pub error: Option<String>,
}

// ---------------------------------------------------------------------------
// OpenAI-compatible chat completion types (used by /v1/chat/completions)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChatCompletionRequest {
    pub model: Option<String>,
    pub messages: Vec<ChatMessage>,
    pub stream: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChatCompletionResponse {
    pub id: String,
    pub object: String,
    pub created: i64,
    pub model: String,
    pub choices: Vec<ChatChoice>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChatChoice {
    pub index: usize,
    pub message: ChatMessageResponse,
    pub finish_reason: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChatMessageResponse {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelsResponse {
    pub object: String,
    pub data: Vec<ModelEntry>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelEntry {
    pub id: String,
    pub object: String,
    pub owned_by: String,
}

// ---------------------------------------------------------------------------
// OpenAI-compatible error envelope (used by auth.rs and mod.rs)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct OpenAiErrorEnvelope {
    pub error: OpenAiErrorBody,
}

#[derive(Debug, Clone, Serialize)]
pub struct OpenAiErrorBody {
    pub message: String,
    pub r#type: String,
    pub code: String,
}
