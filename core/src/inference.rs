//! LLM Provider Inference Module
//!
//! Supports Anthropic Messages API and OpenAI Chat Completions API.
//! API keys are decrypted from the vault at request time - never cached in memory.
//! Streaming is supported via SSE (Server-Sent Events) for both providers.

use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;

// ── Provider Enum ──────────────────────────────────────────────────────────

/// Supported inference providers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InferenceProvider {
    Anthropic,
    OpenAI,
}

impl InferenceProvider {
    pub fn from_str_loose(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "anthropic" | "claude" => Some(Self::Anthropic),
            "openai" | "gpt" => Some(Self::OpenAI),
            _ => None,
        }
    }

    /// The vault provider key to look up the API key.
    pub fn vault_key(&self) -> &'static str {
        match self {
            Self::Anthropic => "anthropic",
            Self::OpenAI => "openai",
        }
    }

    /// Default model for each provider.
    pub fn default_model(&self) -> &'static str {
        match self {
            Self::Anthropic => "claude-sonnet-4-20250514",
            Self::OpenAI => "gpt-4o",
        }
    }
}

impl std::fmt::Display for InferenceProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Anthropic => write!(f, "anthropic"),
            Self::OpenAI => write!(f, "openai"),
        }
    }
}

// ── Message Types ──────────────────────────────────────────────────────────

/// A chat message in the conversation history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

/// Request to the inference engine.
#[derive(Debug, Clone)]
pub struct InferenceRequest {
    pub provider: InferenceProvider,
    pub model: String,
    pub system_prompt: String,
    pub messages: Vec<ChatMessage>,
    pub max_tokens: u32,
    pub temperature: f32,
}

/// Response from the inference engine (non-streaming).
#[derive(Debug, Clone, Serialize)]
pub struct InferenceResponse {
    pub content: String,
    pub model: String,
    pub provider: String,
    pub input_tokens: Option<u32>,
    pub output_tokens: Option<u32>,
    pub stop_reason: Option<String>,
}

// ── Anthropic API Types ────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
struct AnthropicRequest {
    model: String,
    max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    system: String,
    messages: Vec<AnthropicMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize)]
struct AnthropicMessage {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct AnthropicResponse {
    content: Vec<AnthropicContentBlock>,
    model: String,
    #[serde(default)]
    stop_reason: Option<String>,
    usage: Option<AnthropicUsage>,
}

#[derive(Debug, Deserialize)]
pub struct AnthropicContentBlock {
    #[serde(rename = "type")]
    pub block_type: String,
    pub text: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AnthropicUsage {
    pub input_tokens: Option<u32>,
    pub output_tokens: Option<u32>,
}

// ── OpenAI API Types ───────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
struct OpenAIRequest {
    model: String,
    messages: Vec<OpenAIMessage>,
    max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize)]
struct OpenAIMessage {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct OpenAIResponse {
    choices: Vec<OpenAIChoice>,
    model: String,
    usage: Option<OpenAIUsage>,
}

#[derive(Debug, Deserialize)]
struct OpenAIChoice {
    message: OpenAIMessage,
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenAIUsage {
    prompt_tokens: Option<u32>,
    completion_tokens: Option<u32>,
}

// ── OpenAI Streaming Types ─────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct OpenAIStreamChunk {
    pub choices: Vec<OpenAIStreamChoice>,
}

#[derive(Debug, Deserialize)]
pub struct OpenAIStreamChoice {
    pub delta: OpenAIStreamDelta,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct OpenAIStreamDelta {
    pub content: Option<String>,
}

// ── Anthropic Streaming Types ──────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum AnthropicStreamEvent {
    #[serde(rename = "message_start")]
    MessageStart { message: AnthropicStreamMessage },
    #[serde(rename = "content_block_start")]
    ContentBlockStart {
        index: usize,
        content_block: AnthropicContentBlock,
    },
    #[serde(rename = "content_block_delta")]
    ContentBlockDelta {
        index: usize,
        delta: AnthropicDelta,
    },
    #[serde(rename = "content_block_stop")]
    ContentBlockStop { index: usize },
    #[serde(rename = "message_delta")]
    MessageDelta {
        delta: AnthropicMessageDelta,
        usage: Option<AnthropicDeltaUsage>,
    },
    #[serde(rename = "message_stop")]
    MessageStop {},
    #[serde(rename = "ping")]
    Ping {},
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Deserialize)]
pub struct AnthropicStreamMessage {
    pub model: Option<String>,
    pub usage: Option<AnthropicUsage>,
}

#[derive(Debug, Deserialize)]
pub struct AnthropicDelta {
    #[serde(rename = "type")]
    pub delta_type: String,
    pub text: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AnthropicMessageDelta {
    pub stop_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AnthropicDeltaUsage {
    pub output_tokens: Option<u32>,
}

// ── Error Types ────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct AnthropicErrorResponse {
    error: Option<AnthropicErrorDetail>,
}

#[derive(Debug, Deserialize)]
struct AnthropicErrorDetail {
    message: Option<String>,
    #[serde(rename = "type")]
    error_type: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenAIErrorResponse {
    error: Option<OpenAIErrorDetail>,
}

#[derive(Debug, Deserialize)]
struct OpenAIErrorDetail {
    message: Option<String>,
}

// ── Inference Client ───────────────────────────────────────────────────────

/// Stateless inference client. Creates HTTP requests on demand.
#[derive(Clone)]
pub struct InferenceClient {
    http: Client,
}

impl InferenceClient {
    pub fn new() -> Self {
        let http = Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .expect("Failed to build HTTP client");
        Self { http }
    }

    /// Non-streaming completion. Returns the full response once done.
    pub async fn complete(
        &self,
        api_key: &str,
        request: &InferenceRequest,
    ) -> Result<InferenceResponse, String> {
        match request.provider {
            InferenceProvider::Anthropic => self.complete_anthropic(api_key, request).await,
            InferenceProvider::OpenAI => self.complete_openai(api_key, request).await,
        }
    }

    /// Start a streaming request. Returns the raw reqwest::Response for SSE parsing.
    pub async fn stream_raw(
        &self,
        api_key: &str,
        request: &InferenceRequest,
    ) -> Result<reqwest::Response, String> {
        match request.provider {
            InferenceProvider::Anthropic => self.stream_anthropic_raw(api_key, request).await,
            InferenceProvider::OpenAI => self.stream_openai_raw(api_key, request).await,
        }
    }

    // ── Anthropic ──────────────────────────────────────────────────────────

    async fn complete_anthropic(
        &self,
        api_key: &str,
        request: &InferenceRequest,
    ) -> Result<InferenceResponse, String> {
        let body = AnthropicRequest {
            model: request.model.clone(),
            max_tokens: request.max_tokens,
            temperature: if request.temperature > 0.0 {
                Some(request.temperature)
            } else {
                None
            },
            system: request.system_prompt.clone(),
            messages: request
                .messages
                .iter()
                .map(|m| AnthropicMessage {
                    role: m.role.clone(),
                    content: m.content.clone(),
                })
                .collect(),
            stream: None,
        };

        let resp = self
            .http
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("Anthropic request failed: {e}"))?;

        let status = resp.status();
        if !status.is_success() {
            let err_text = resp.text().await.unwrap_or_default();
            if let Ok(err_json) = serde_json::from_str::<AnthropicErrorResponse>(&err_text) {
                if let Some(detail) = err_json.error {
                    return Err(format!(
                        "Anthropic API error ({}): {} - {}",
                        status,
                        detail.error_type.unwrap_or_default(),
                        detail.message.unwrap_or_default()
                    ));
                }
            }
            return Err(format!("Anthropic API error ({}): {}", status, err_text));
        }

        let api_resp: AnthropicResponse = resp
            .json()
            .await
            .map_err(|e| format!("Failed to parse Anthropic response: {e}"))?;

        let content = api_resp
            .content
            .iter()
            .filter(|b| b.block_type == "text")
            .filter_map(|b| b.text.as_deref())
            .collect::<Vec<_>>()
            .join("");

        let (input_tokens, output_tokens) = api_resp
            .usage
            .map(|u| (u.input_tokens, u.output_tokens))
            .unwrap_or((None, None));

        Ok(InferenceResponse {
            content,
            model: api_resp.model,
            provider: "anthropic".to_string(),
            input_tokens,
            output_tokens,
            stop_reason: api_resp.stop_reason,
        })
    }

    async fn stream_anthropic_raw(
        &self,
        api_key: &str,
        request: &InferenceRequest,
    ) -> Result<reqwest::Response, String> {
        let body = AnthropicRequest {
            model: request.model.clone(),
            max_tokens: request.max_tokens,
            temperature: if request.temperature > 0.0 {
                Some(request.temperature)
            } else {
                None
            },
            system: request.system_prompt.clone(),
            messages: request
                .messages
                .iter()
                .map(|m| AnthropicMessage {
                    role: m.role.clone(),
                    content: m.content.clone(),
                })
                .collect(),
            stream: Some(true),
        };

        let resp = self
            .http
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("Anthropic stream request failed: {e}"))?;

        let status = resp.status();
        if !status.is_success() {
            let err_text = resp.text().await.unwrap_or_default();
            return Err(format!("Anthropic stream error ({}): {}", status, err_text));
        }

        Ok(resp)
    }

    // ── OpenAI ─────────────────────────────────────────────────────────────

    async fn complete_openai(
        &self,
        api_key: &str,
        request: &InferenceRequest,
    ) -> Result<InferenceResponse, String> {
        let mut messages = vec![OpenAIMessage {
            role: "system".to_string(),
            content: request.system_prompt.clone(),
        }];

        for m in &request.messages {
            messages.push(OpenAIMessage {
                role: m.role.clone(),
                content: m.content.clone(),
            });
        }

        let body = OpenAIRequest {
            model: request.model.clone(),
            messages,
            max_tokens: request.max_tokens,
            temperature: if request.temperature > 0.0 {
                Some(request.temperature)
            } else {
                None
            },
            stream: None,
        };

        let resp = self
            .http
            .post("https://api.openai.com/v1/chat/completions")
            .header("Authorization", format!("Bearer {api_key}"))
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("OpenAI request failed: {e}"))?;

        let status = resp.status();
        if !status.is_success() {
            let err_text = resp.text().await.unwrap_or_default();
            if let Ok(err_json) = serde_json::from_str::<OpenAIErrorResponse>(&err_text) {
                if let Some(detail) = err_json.error {
                    return Err(format!(
                        "OpenAI API error ({}): {}",
                        status,
                        detail.message.unwrap_or_default()
                    ));
                }
            }
            return Err(format!("OpenAI API error ({}): {}", status, err_text));
        }

        let api_resp: OpenAIResponse = resp
            .json()
            .await
            .map_err(|e| format!("Failed to parse OpenAI response: {e}"))?;

        let choice = api_resp
            .choices
            .first()
            .ok_or("OpenAI returned no choices")?;

        let (input_tokens, output_tokens) = api_resp
            .usage
            .map(|u| (u.prompt_tokens, u.completion_tokens))
            .unwrap_or((None, None));

        Ok(InferenceResponse {
            content: choice.message.content.clone(),
            model: api_resp.model,
            provider: "openai".to_string(),
            input_tokens,
            output_tokens,
            stop_reason: choice.finish_reason.clone(),
        })
    }

    async fn stream_openai_raw(
        &self,
        api_key: &str,
        request: &InferenceRequest,
    ) -> Result<reqwest::Response, String> {
        let mut messages = vec![OpenAIMessage {
            role: "system".to_string(),
            content: request.system_prompt.clone(),
        }];

        for m in &request.messages {
            messages.push(OpenAIMessage {
                role: m.role.clone(),
                content: m.content.clone(),
            });
        }

        let body = OpenAIRequest {
            model: request.model.clone(),
            messages,
            max_tokens: request.max_tokens,
            temperature: if request.temperature > 0.0 {
                Some(request.temperature)
            } else {
                None
            },
            stream: Some(true),
        };

        let resp = self
            .http
            .post("https://api.openai.com/v1/chat/completions")
            .header("Authorization", format!("Bearer {api_key}"))
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("OpenAI stream request failed: {e}"))?;

        let status = resp.status();
        if !status.is_success() {
            let err_text = resp.text().await.unwrap_or_default();
            return Err(format!("OpenAI stream error ({}): {}", status, err_text));
        }

        Ok(resp)
    }
}

/// Load the Kaizen system prompt from the template file.
/// Falls back to a minimal prompt if the file is not found.
pub fn load_system_prompt() -> String {
    for candidate in &[
        "../contexts/templates/kaizen_system_prompt.md",
        "contexts/templates/kaizen_system_prompt.md",
    ] {
        if let Ok(content) = std::fs::read_to_string(candidate) {
            return content;
        }
    }

    // Fallback minimal prompt
    "You are Kaizen, the primary planner/reasoner for Kaizen MAX. \
     You help users with software engineering tasks. \
     You can orchestrate sub-agents only when the user explicitly asks. \
     Keep responses concise and actionable."
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_provider_from_str() {
        assert_eq!(
            InferenceProvider::from_str_loose("anthropic"),
            Some(InferenceProvider::Anthropic)
        );
        assert_eq!(
            InferenceProvider::from_str_loose("claude"),
            Some(InferenceProvider::Anthropic)
        );
        assert_eq!(
            InferenceProvider::from_str_loose("openai"),
            Some(InferenceProvider::OpenAI)
        );
        assert_eq!(
            InferenceProvider::from_str_loose("gpt"),
            Some(InferenceProvider::OpenAI)
        );
        assert_eq!(InferenceProvider::from_str_loose("unknown"), None);
    }

    #[test]
    fn test_provider_vault_key() {
        assert_eq!(InferenceProvider::Anthropic.vault_key(), "anthropic");
        assert_eq!(InferenceProvider::OpenAI.vault_key(), "openai");
    }

    #[test]
    fn test_provider_default_model() {
        assert!(InferenceProvider::Anthropic
            .default_model()
            .contains("claude"));
        assert!(InferenceProvider::OpenAI.default_model().contains("gpt"));
    }

    #[test]
    fn test_load_system_prompt_fallback() {
        // In test environment, template may not be at expected path.
        // Verify that fallback is non-empty.
        let prompt = load_system_prompt();
        assert!(!prompt.is_empty());
        assert!(prompt.contains("Kaizen"));
    }

    #[test]
    fn test_inference_client_creation() {
        let _client = InferenceClient::new();
        // Just verify it doesn't panic
    }
}
