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
    Gemini,
    Nvidia,
}

impl InferenceProvider {
    pub fn from_str_loose(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "anthropic" | "claude" => Some(Self::Anthropic),
            "openai" | "gpt" => Some(Self::OpenAI),
            "gemini" | "google" | "googleai" => Some(Self::Gemini),
            "nvidia" | "nim" => Some(Self::Nvidia),
            _ => None,
        }
    }

    /// The vault provider key to look up the API key.
    pub fn vault_key(&self) -> &'static str {
        match self {
            Self::Anthropic => "anthropic",
            Self::OpenAI => "openai",
            Self::Gemini => "gemini",
            Self::Nvidia => "nvidia",
        }
    }

    /// Default model for each provider.
    pub fn default_model(&self) -> &'static str {
        match self {
            Self::Anthropic => "claude-sonnet-4-20250514",
            Self::OpenAI => "gpt-4o",
            Self::Gemini => "gemini-1.5-pro",
            Self::Nvidia => "nvidia/llama-3.3-nemotron-super-49b-v1",
        }
    }
}

impl std::fmt::Display for InferenceProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Anthropic => write!(f, "anthropic"),
            Self::OpenAI => write!(f, "openai"),
            Self::Gemini => write!(f, "gemini"),
            Self::Nvidia => write!(f, "nvidia"),
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

// ── Gemini API Types ───────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
struct GeminiRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    system_instruction: Option<GeminiInstruction>,
    contents: Vec<GeminiContent>,
    #[serde(rename = "generationConfig")]
    generation_config: GeminiGenerationConfig,
}

#[derive(Debug, Serialize)]
struct GeminiInstruction {
    parts: Vec<GeminiPart>,
}

#[derive(Debug, Serialize)]
struct GeminiContent {
    role: String,
    parts: Vec<GeminiPart>,
}

#[derive(Debug, Serialize)]
struct GeminiPart {
    text: String,
}

#[derive(Debug, Serialize)]
struct GeminiGenerationConfig {
    #[serde(rename = "maxOutputTokens")]
    max_output_tokens: u32,
    temperature: f32,
}

#[derive(Debug, Deserialize)]
struct GeminiResponse {
    candidates: Option<Vec<GeminiCandidate>>,
    #[serde(rename = "usageMetadata")]
    usage_metadata: Option<GeminiUsageMetadata>,
}

#[derive(Debug, Deserialize)]
struct GeminiCandidate {
    content: Option<GeminiResponseContent>,
    #[serde(rename = "finishReason")]
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GeminiResponseContent {
    parts: Option<Vec<GeminiResponsePart>>,
}

#[derive(Debug, Deserialize)]
struct GeminiResponsePart {
    text: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GeminiUsageMetadata {
    #[serde(rename = "promptTokenCount")]
    prompt_token_count: Option<u32>,
    #[serde(rename = "candidatesTokenCount")]
    candidates_token_count: Option<u32>,
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
    ContentBlockDelta { index: usize, delta: AnthropicDelta },
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

#[derive(Debug, Deserialize)]
struct GeminiErrorEnvelope {
    error: Option<GeminiErrorDetail>,
}

#[derive(Debug, Deserialize)]
struct GeminiErrorDetail {
    message: Option<String>,
    status: Option<String>,
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
            InferenceProvider::Gemini => self.complete_gemini(api_key, request).await,
            InferenceProvider::Nvidia => self.complete_nvidia(api_key, request).await,
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
            InferenceProvider::Gemini => self.stream_gemini_raw(api_key, request).await,
            InferenceProvider::Nvidia => self.stream_nvidia_raw(api_key, request).await,
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

    // ── OpenAI / NVIDIA (OpenAI-compatible APIs) ──────────────────────────

    fn build_openai_messages(&self, request: &InferenceRequest) -> Vec<OpenAIMessage> {
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

        messages
    }

    async fn complete_openai(
        &self,
        api_key: &str,
        request: &InferenceRequest,
    ) -> Result<InferenceResponse, String> {
        self.complete_openai_compatible(
            "https://api.openai.com/v1/chat/completions",
            "openai",
            api_key,
            request,
        )
        .await
    }

    async fn complete_nvidia(
        &self,
        api_key: &str,
        request: &InferenceRequest,
    ) -> Result<InferenceResponse, String> {
        self.complete_openai_compatible(
            "https://integrate.api.nvidia.com/v1/chat/completions",
            "nvidia",
            api_key,
            request,
        )
        .await
    }

    async fn complete_openai_compatible(
        &self,
        endpoint: &str,
        provider_label: &str,
        api_key: &str,
        request: &InferenceRequest,
    ) -> Result<InferenceResponse, String> {
        let body = OpenAIRequest {
            model: request.model.clone(),
            messages: self.build_openai_messages(request),
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
            .post(endpoint)
            .header("Authorization", format!("Bearer {api_key}"))
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("{} request failed: {e}", provider_label.to_uppercase()))?;

        let status = resp.status();
        if !status.is_success() {
            let err_text = resp.text().await.unwrap_or_default();
            if let Ok(err_json) = serde_json::from_str::<OpenAIErrorResponse>(&err_text) {
                if let Some(detail) = err_json.error {
                    return Err(format!(
                        "{} API error ({}): {}",
                        provider_label,
                        status,
                        detail.message.unwrap_or_default()
                    ));
                }
            }
            return Err(format!(
                "{} API error ({}): {}",
                provider_label, status, err_text
            ));
        }

        let api_resp: OpenAIResponse = resp
            .json()
            .await
            .map_err(|e| format!("Failed to parse {provider_label} response: {e}"))?;

        let choice = api_resp
            .choices
            .first()
            .ok_or(format!("{provider_label} returned no choices"))?;

        let (input_tokens, output_tokens) = api_resp
            .usage
            .map(|u| (u.prompt_tokens, u.completion_tokens))
            .unwrap_or((None, None));

        Ok(InferenceResponse {
            content: choice.message.content.clone(),
            model: api_resp.model,
            provider: provider_label.to_string(),
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
        self.stream_openai_compatible_raw(
            "https://api.openai.com/v1/chat/completions",
            "openai",
            api_key,
            request,
        )
        .await
    }

    async fn stream_nvidia_raw(
        &self,
        api_key: &str,
        request: &InferenceRequest,
    ) -> Result<reqwest::Response, String> {
        self.stream_openai_compatible_raw(
            "https://integrate.api.nvidia.com/v1/chat/completions",
            "nvidia",
            api_key,
            request,
        )
        .await
    }

    async fn stream_openai_compatible_raw(
        &self,
        endpoint: &str,
        provider_label: &str,
        api_key: &str,
        request: &InferenceRequest,
    ) -> Result<reqwest::Response, String> {
        let body = OpenAIRequest {
            model: request.model.clone(),
            messages: self.build_openai_messages(request),
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
            .post(endpoint)
            .header("Authorization", format!("Bearer {api_key}"))
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("{provider_label} stream request failed: {e}"))?;

        let status = resp.status();
        if !status.is_success() {
            let err_text = resp.text().await.unwrap_or_default();
            return Err(format!(
                "{provider_label} stream error ({}): {}",
                status, err_text
            ));
        }

        Ok(resp)
    }

    // ── Gemini ─────────────────────────────────────────────────────────────

    fn build_gemini_request(&self, request: &InferenceRequest) -> GeminiRequest {
        let system_instruction = if request.system_prompt.trim().is_empty() {
            None
        } else {
            Some(GeminiInstruction {
                parts: vec![GeminiPart {
                    text: request.system_prompt.clone(),
                }],
            })
        };

        let mut contents: Vec<GeminiContent> = request
            .messages
            .iter()
            .map(|m| GeminiContent {
                role: if m.role.eq_ignore_ascii_case("assistant") {
                    "model".to_string()
                } else {
                    "user".to_string()
                },
                parts: vec![GeminiPart {
                    text: m.content.clone(),
                }],
            })
            .collect();

        if contents.is_empty() {
            contents.push(GeminiContent {
                role: "user".to_string(),
                parts: vec![GeminiPart {
                    text: String::new(),
                }],
            });
        }

        GeminiRequest {
            system_instruction,
            contents,
            generation_config: GeminiGenerationConfig {
                max_output_tokens: request.max_tokens,
                temperature: request.temperature,
            },
        }
    }

    async fn complete_gemini(
        &self,
        api_key: &str,
        request: &InferenceRequest,
    ) -> Result<InferenceResponse, String> {
        let endpoint = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
            request.model, api_key
        );

        let body = self.build_gemini_request(request);
        let resp = self
            .http
            .post(endpoint)
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("Gemini request failed: {e}"))?;

        let status = resp.status();
        if !status.is_success() {
            let err_text = resp.text().await.unwrap_or_default();
            if let Ok(err_json) = serde_json::from_str::<GeminiErrorEnvelope>(&err_text) {
                if let Some(detail) = err_json.error {
                    return Err(format!(
                        "Gemini API error ({}): {} {}",
                        status,
                        detail.status.unwrap_or_default(),
                        detail.message.unwrap_or_default()
                    ));
                }
            }
            return Err(format!("Gemini API error ({}): {}", status, err_text));
        }

        let api_resp: GeminiResponse = resp
            .json()
            .await
            .map_err(|e| format!("Failed to parse Gemini response: {e}"))?;

        let mut content = String::new();
        let mut stop_reason = None;
        if let Some(candidates) = api_resp.candidates.as_ref() {
            for candidate in candidates {
                if stop_reason.is_none() {
                    stop_reason = candidate.finish_reason.clone();
                }
                if let Some(parts) = candidate.content.as_ref().and_then(|c| c.parts.as_ref()) {
                    for part in parts {
                        if let Some(text) = part.text.as_ref() {
                            content.push_str(text);
                        }
                    }
                }
            }
        }

        let (input_tokens, output_tokens) = api_resp
            .usage_metadata
            .map(|u| (u.prompt_token_count, u.candidates_token_count))
            .unwrap_or((None, None));

        Ok(InferenceResponse {
            content,
            model: request.model.clone(),
            provider: "gemini".to_string(),
            input_tokens,
            output_tokens,
            stop_reason,
        })
    }

    async fn stream_gemini_raw(
        &self,
        api_key: &str,
        request: &InferenceRequest,
    ) -> Result<reqwest::Response, String> {
        let endpoint = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:streamGenerateContent?alt=sse&key={}",
            request.model, api_key
        );

        let body = self.build_gemini_request(request);
        let resp = self
            .http
            .post(endpoint)
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("Gemini stream request failed: {e}"))?;

        let status = resp.status();
        if !status.is_success() {
            let err_text = resp.text().await.unwrap_or_default();
            return Err(format!("Gemini stream error ({}): {}", status, err_text));
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
        assert_eq!(
            InferenceProvider::from_str_loose("gemini"),
            Some(InferenceProvider::Gemini)
        );
        assert_eq!(
            InferenceProvider::from_str_loose("google"),
            Some(InferenceProvider::Gemini)
        );
        assert_eq!(
            InferenceProvider::from_str_loose("nvidia"),
            Some(InferenceProvider::Nvidia)
        );
        assert_eq!(InferenceProvider::from_str_loose("unknown"), None);
    }

    #[test]
    fn test_provider_vault_key() {
        assert_eq!(InferenceProvider::Anthropic.vault_key(), "anthropic");
        assert_eq!(InferenceProvider::OpenAI.vault_key(), "openai");
        assert_eq!(InferenceProvider::Gemini.vault_key(), "gemini");
        assert_eq!(InferenceProvider::Nvidia.vault_key(), "nvidia");
    }

    #[test]
    fn test_provider_default_model() {
        assert!(
            InferenceProvider::Anthropic
                .default_model()
                .contains("claude")
        );
        assert!(InferenceProvider::OpenAI.default_model().contains("gpt"));
        assert!(InferenceProvider::Gemini.default_model().contains("gemini"));
        assert!(InferenceProvider::Nvidia.default_model().contains("nvidia"));
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
