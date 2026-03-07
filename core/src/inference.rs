//! LLM Provider Inference Module
//!
//! Supports Anthropic Messages API and OpenAI Chat Completions API.
//! Credentials are resolved at request time from the active local auth method.
//! Streaming is supported via SSE (Server-Sent Events) for both providers.

use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;

// ── Provider Enum ──────────────────────────────────────────────────────────

/// Supported inference providers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InferenceProvider {
    Anthropic,
    OpenAI,
    Gemini,
    Nvidia,
    #[serde(rename = "gemini-cli")]
    GeminiCli,
    #[serde(rename = "codex-cli")]
    CodexCli,
}

impl InferenceProvider {
    pub fn from_str_loose(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "anthropic" | "claude" => Some(Self::Anthropic),
            "openai" | "gpt" => Some(Self::OpenAI),
            "gemini" | "google" | "googleai" => Some(Self::Gemini),
            "nvidia" | "nim" => Some(Self::Nvidia),
            "gemini-cli" | "geminicli" | "google-cli" => Some(Self::GeminiCli),
            "codex-cli" | "codexcli" | "openai-cli" => Some(Self::CodexCli),
            _ => None,
        }
    }

    /// The provider key stem used by the local auth resolver.
    pub fn vault_key(&self) -> Option<&'static str> {
        match self {
            Self::Anthropic => Some("anthropic"),
            Self::OpenAI => Some("openai"),
            Self::Gemini => Some("gemini"),
            Self::Nvidia => Some("nvidia"),
            Self::GeminiCli => None,
            Self::CodexCli => None,
        }
    }

    /// Default model for each provider.
    pub fn default_model(&self) -> &'static str {
        match self {
            Self::Anthropic => "claude-sonnet-4-20250514",
            Self::OpenAI => "gpt-4o",
            Self::Gemini => "gemini-1.5-pro",
            Self::Nvidia => "nvidia/llama-3.3-nemotron-super-49b-v1",
            Self::GeminiCli => "gemini-2.5-flash",
            Self::CodexCli => "gpt-5.4",
        }
    }
}

#[derive(Clone)]
pub enum InferenceCredential {
    None,
    ApiKey(String),
    BearerToken {
        token: String,
        user_project: Option<String>,
    },
}

impl InferenceCredential {
    fn secret(&self) -> Option<&str> {
        match self {
            Self::None => None,
            Self::ApiKey(value) => Some(value.as_str()),
            Self::BearerToken { token, .. } => Some(token.as_str()),
        }
    }

    fn bearer(&self) -> Option<&str> {
        match self {
            Self::BearerToken { token, .. } => Some(token.as_str()),
            _ => None,
        }
    }

    fn user_project(&self) -> Option<&str> {
        match self {
            Self::BearerToken { user_project, .. } => user_project.as_deref(),
            _ => None,
        }
    }

    pub fn wipe(&mut self) {
        match self {
            Self::None => {}
            Self::ApiKey(value) => wipe_secret(value),
            Self::BearerToken { token, .. } => wipe_secret(token),
        }
    }
}

fn wipe_secret(secret: &mut String) {
    // Best-effort wipe for short-lived credential material.
    unsafe {
        secret.as_bytes_mut().fill(0);
    }
    secret.clear();
}

impl std::fmt::Display for InferenceProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Anthropic => write!(f, "anthropic"),
            Self::OpenAI => write!(f, "openai"),
            Self::Gemini => write!(f, "gemini"),
            Self::Nvidia => write!(f, "nvidia"),
            Self::GeminiCli => write!(f, "gemini-cli"),
            Self::CodexCli => write!(f, "codex-cli"),
        }
    }
}

// ── Message Types ──────────────────────────────────────────────────────────

/// A chat message in the conversation history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
    #[serde(default)]
    pub attachments: Vec<ChatAttachment>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatAttachment {
    pub name: String,
    pub media_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_base64: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview_url: Option<String>,
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

#[derive(Debug, Clone)]
pub enum LiveInferenceEvent {
    Token(String),
    Done {
        full_response: String,
        input_tokens: Option<u32>,
        output_tokens: Option<u32>,
        stop_reason: Option<String>,
    },
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
    content: AnthropicMessageContent,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
enum AnthropicMessageContent {
    Text(String),
    Blocks(Vec<AnthropicMessageBlock>),
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
enum AnthropicMessageBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "image")]
    Image { source: AnthropicImageSource },
}

#[derive(Debug, Serialize, Deserialize)]
struct AnthropicImageSource {
    #[serde(rename = "type")]
    source_type: String,
    media_type: String,
    data: String,
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
    content: OpenAIMessageContent,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
enum OpenAIMessageContent {
    Text(String),
    Parts(Vec<OpenAIContentPart>),
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
enum OpenAIContentPart {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "image_url")]
    ImageUrl { image_url: OpenAIImageUrl },
}

#[derive(Debug, Serialize, Deserialize)]
struct OpenAIImageUrl {
    url: String,
}

#[derive(Debug, Deserialize)]
struct OpenAIResponse {
    choices: Vec<OpenAIChoice>,
    model: String,
    usage: Option<OpenAIUsage>,
}

#[derive(Debug, Deserialize)]
struct OpenAIChoice {
    message: OpenAIResponseMessage,
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenAIResponseMessage {
    content: String,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
    #[serde(rename = "inlineData", skip_serializing_if = "Option::is_none")]
    inline_data: Option<GeminiInlineData>,
}

#[derive(Debug, Serialize)]
struct GeminiInlineData {
    #[serde(rename = "mimeType")]
    mime_type: String,
    data: String,
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
        credential: &InferenceCredential,
        request: &InferenceRequest,
    ) -> Result<InferenceResponse, String> {
        match request.provider {
            InferenceProvider::Anthropic => self.complete_anthropic(credential, request).await,
            InferenceProvider::OpenAI => self.complete_openai(credential, request).await,
            InferenceProvider::Gemini => self.complete_gemini(credential, request).await,
            InferenceProvider::Nvidia => self.complete_nvidia(credential, request).await,
            InferenceProvider::GeminiCli => self.complete_gemini_cli(request).await,
            InferenceProvider::CodexCli => self.complete_codex_cli(request).await,
        }
    }

    /// Start a streaming request. Returns the raw reqwest::Response for SSE parsing.
    pub async fn stream_raw(
        &self,
        credential: &InferenceCredential,
        request: &InferenceRequest,
    ) -> Result<reqwest::Response, String> {
        match request.provider {
            InferenceProvider::Anthropic => self.stream_anthropic_raw(credential, request).await,
            InferenceProvider::OpenAI => self.stream_openai_raw(credential, request).await,
            InferenceProvider::Gemini => self.stream_gemini_raw(credential, request).await,
            InferenceProvider::Nvidia => self.stream_nvidia_raw(credential, request).await,
            InferenceProvider::GeminiCli | InferenceProvider::CodexCli => Err(
                "CLI-backed providers do not expose SSE streaming here yet. Use non-streaming chat or switch to an API-backed provider for SSE streaming.".to_string(),
            ),
        }
    }

    pub fn stream_codex_cli_live(
        &self,
        request: &InferenceRequest,
    ) -> Result<mpsc::Receiver<Result<LiveInferenceEvent, String>>, String> {
        let prompt = self.build_codex_cli_prompt(request);
        let model = request.model.clone();
        let mut command = codex_cli_command();
        command
            .arg("exec")
            .arg("--json")
            .arg("--color")
            .arg("never")
            .arg("--skip-git-repo-check")
            .arg("--ephemeral");

        if !model.trim().is_empty() {
            command.arg("--model").arg(&model);
        }

        command
            .arg("-")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        let mut child = command.spawn().map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                "Codex CLI executable not found. Install Codex CLI and ensure `codex` is on PATH."
                    .to_string()
            } else {
                format!("Failed to launch Codex CLI: {e}")
            }
        })?;

        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| "Codex CLI stdin was unavailable".to_string())?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "Codex CLI stdout was unavailable".to_string())?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| "Codex CLI stderr was unavailable".to_string())?;

        let (tx, rx) = mpsc::channel(64);

        tokio::spawn(async move {
            if let Err(error) = stdin.write_all(prompt.as_bytes()).await {
                let _ = tx
                    .send(Err(format!("Failed to send prompt to Codex CLI: {error}")))
                    .await;
                return;
            }
            drop(stdin);

            let stderr_task = tokio::spawn(async move {
                let mut stderr_lines = BufReader::new(stderr).lines();
                let mut lines = Vec::new();
                loop {
                    match stderr_lines.next_line().await {
                        Ok(Some(line)) => {
                            let trimmed = line.trim();
                            if !trimmed.is_empty() {
                                lines.push(trimmed.to_string());
                            }
                        }
                        Ok(None) => break,
                        Err(error) => {
                            lines.push(format!("stderr read failed: {error}"));
                            break;
                        }
                    }
                }
                lines.join("\n")
            });

            let mut stdout_lines = BufReader::new(stdout).lines();
            let mut full_response = String::new();
            let mut input_tokens: Option<u32> = None;
            let mut output_tokens: Option<u32> = None;
            let mut stop_reason: Option<String> = None;

            loop {
                let line = match stdout_lines.next_line().await {
                    Ok(Some(line)) => line,
                    Ok(None) => break,
                    Err(error) => {
                        let _ = tx
                            .send(Err(format!("Failed to read Codex CLI output: {error}")))
                            .await;
                        return;
                    }
                };

                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }

                let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) else {
                    continue;
                };

                match value.get("type").and_then(|v| v.as_str()) {
                    Some("item.completed") => {
                        if let Some(item) = value.get("item") {
                            if item.get("type").and_then(|v| v.as_str()) == Some("agent_message") {
                                if let Some(text) = item.get("text").and_then(|v| v.as_str()) {
                                    let text = text.trim();
                                    if !text.is_empty() {
                                        full_response = text.to_string();
                                        for chunk in chunk_text_for_live_stream(text) {
                                            if tx.send(Ok(LiveInferenceEvent::Token(chunk))).await.is_err() {
                                                return;
                                            }
                                            tokio::time::sleep(Duration::from_millis(8)).await;
                                        }
                                    }
                                }
                            }
                        }
                    }
                    Some("turn.completed") => {
                        if let Some(usage) = value.get("usage") {
                            input_tokens = usage
                                .get("input_tokens")
                                .and_then(|v| v.as_u64())
                                .and_then(|v| u32::try_from(v).ok());
                            output_tokens = usage
                                .get("output_tokens")
                                .and_then(|v| v.as_u64())
                                .and_then(|v| u32::try_from(v).ok());
                        }
                    }
                    Some("turn.cancelled") => {
                        stop_reason = Some("cancelled".to_string());
                    }
                    Some("error") => {
                        let message = value
                            .get("message")
                            .and_then(|v| v.as_str())
                            .or_else(|| value.get("error").and_then(|v| v.as_str()))
                            .unwrap_or("unknown Codex CLI error");
                        let _ = tx.send(Err(format!("Codex CLI error: {message}"))).await;
                        return;
                    }
                    Some("turn.failed") => {
                        let message = value
                            .get("message")
                            .and_then(|v| v.as_str())
                            .unwrap_or("Codex CLI turn failed");
                        let _ = tx.send(Err(message.to_string())).await;
                        return;
                    }
                    _ => {}
                }
            }

            let status = match child.wait().await {
                Ok(status) => status,
                Err(error) => {
                    let _ = tx
                        .send(Err(format!("Failed to wait for Codex CLI: {error}")))
                        .await;
                    return;
                }
            };

            let stderr_text = match stderr_task.await {
                Ok(value) => value,
                Err(error) => format!("Failed to join Codex CLI stderr reader: {error}"),
            };

            if !status.success() {
                let code = status
                    .code()
                    .map(|code| code.to_string())
                    .unwrap_or_else(|| "terminated".to_string());
                let detail = if stderr_text.trim().is_empty() {
                    "unknown error".to_string()
                } else {
                    stderr_text.trim().to_string()
                };
                let _ = tx
                    .send(Err(format!(
                        "Codex CLI failed (exit code {code}): {detail}. Run `codex login` if OAuth is not configured."
                    )))
                    .await;
                return;
            }

            if full_response.trim().is_empty() {
                let detail = if stderr_text.trim().is_empty() {
                    "Codex CLI returned empty output".to_string()
                } else {
                    stderr_text.trim().to_string()
                };
                let _ = tx.send(Err(detail)).await;
                return;
            }

            let _ = tx
                .send(Ok(LiveInferenceEvent::Done {
                    full_response,
                    input_tokens,
                    output_tokens,
                    stop_reason,
                }))
                .await;
        });

        Ok(rx)
    }

    fn message_text_with_attachment_note(message: &ChatMessage) -> String {
        if message.attachments.is_empty() {
            return message.content.clone();
        }

        let mut text = message.content.clone();
        if !text.trim().is_empty() {
            text.push_str("\n\n");
        }
        text.push_str("Attached image context:\n");
        for attachment in &message.attachments {
            text.push_str("- ");
            text.push_str(&attachment.name);
            if !attachment.media_type.trim().is_empty() {
                text.push_str(" (");
                text.push_str(&attachment.media_type);
                text.push(')');
            }
            if attachment.data_base64.is_none() {
                text.push_str(" [metadata only]");
            }
            text.push('\n');
        }
        text.trim_end().to_string()
    }

    fn build_anthropic_message(&self, message: &ChatMessage) -> AnthropicMessage {
        let inline_images: Vec<&ChatAttachment> = message
            .attachments
            .iter()
            .filter(|attachment| attachment.data_base64.is_some())
            .collect();

        if inline_images.is_empty() || !message.role.eq_ignore_ascii_case("user") {
            return AnthropicMessage {
                role: message.role.clone(),
                content: AnthropicMessageContent::Text(Self::message_text_with_attachment_note(
                    message,
                )),
            };
        }

        let mut blocks = Vec::new();
        let text = if message.content.trim().is_empty() {
            "Review the attached image context and respond to the operator.".to_string()
        } else {
            message.content.clone()
        };
        blocks.push(AnthropicMessageBlock::Text { text });
        for attachment in inline_images {
            if let Some(data) = attachment.data_base64.clone() {
                blocks.push(AnthropicMessageBlock::Image {
                    source: AnthropicImageSource {
                        source_type: "base64".to_string(),
                        media_type: attachment.media_type.clone(),
                        data,
                    },
                });
            }
        }

        AnthropicMessage {
            role: message.role.clone(),
            content: AnthropicMessageContent::Blocks(blocks),
        }
    }

    fn build_openai_message(&self, message: &ChatMessage) -> OpenAIMessage {
        let inline_images: Vec<&ChatAttachment> = message
            .attachments
            .iter()
            .filter(|attachment| attachment.data_base64.is_some())
            .collect();

        if inline_images.is_empty() || !message.role.eq_ignore_ascii_case("user") {
            return OpenAIMessage {
                role: message.role.clone(),
                content: OpenAIMessageContent::Text(Self::message_text_with_attachment_note(
                    message,
                )),
            };
        }

        let mut parts = Vec::new();
        let text = if message.content.trim().is_empty() {
            "Review the attached image context and respond to the operator.".to_string()
        } else {
            message.content.clone()
        };
        parts.push(OpenAIContentPart::Text { text });
        for attachment in inline_images {
            if let Some(data) = attachment.data_base64.clone() {
                parts.push(OpenAIContentPart::ImageUrl {
                    image_url: OpenAIImageUrl {
                        url: format!("data:{};base64,{}", attachment.media_type, data),
                    },
                });
            }
        }

        OpenAIMessage {
            role: message.role.clone(),
            content: OpenAIMessageContent::Parts(parts),
        }
    }

    // ── Anthropic ──────────────────────────────────────────────────────────

    async fn complete_anthropic(
        &self,
        credential: &InferenceCredential,
        request: &InferenceRequest,
    ) -> Result<InferenceResponse, String> {
        let api_key = credential
            .secret()
            .ok_or("Anthropic requests need ANTHROPIC_API_KEY.".to_string())?;
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
                .map(|m| self.build_anthropic_message(m))
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
        credential: &InferenceCredential,
        request: &InferenceRequest,
    ) -> Result<reqwest::Response, String> {
        let api_key = credential
            .secret()
            .ok_or("Anthropic streaming needs ANTHROPIC_API_KEY.".to_string())?;
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
                .map(|m| self.build_anthropic_message(m))
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
            content: OpenAIMessageContent::Text(request.system_prompt.clone()),
        }];

        for m in &request.messages {
            messages.push(self.build_openai_message(m));
        }

        messages
    }

    async fn complete_openai(
        &self,
        credential: &InferenceCredential,
        request: &InferenceRequest,
    ) -> Result<InferenceResponse, String> {
        self.complete_openai_compatible(
            "https://api.openai.com/v1/chat/completions",
            "openai",
            credential,
            request,
        )
        .await
    }

    async fn complete_nvidia(
        &self,
        credential: &InferenceCredential,
        request: &InferenceRequest,
    ) -> Result<InferenceResponse, String> {
        self.complete_openai_compatible(
            "https://integrate.api.nvidia.com/v1/chat/completions",
            "nvidia",
            credential,
            request,
        )
        .await
    }

    async fn complete_openai_compatible(
        &self,
        endpoint: &str,
        provider_label: &str,
        credential: &InferenceCredential,
        request: &InferenceRequest,
    ) -> Result<InferenceResponse, String> {
        let token = credential.secret().ok_or(format!(
            "{provider_label} requests need a configured bearer credential."
        ))?;
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
            .header("Authorization", format!("Bearer {token}"))
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
        credential: &InferenceCredential,
        request: &InferenceRequest,
    ) -> Result<reqwest::Response, String> {
        self.stream_openai_compatible_raw(
            "https://api.openai.com/v1/chat/completions",
            "openai",
            credential,
            request,
        )
        .await
    }

    async fn stream_nvidia_raw(
        &self,
        credential: &InferenceCredential,
        request: &InferenceRequest,
    ) -> Result<reqwest::Response, String> {
        self.stream_openai_compatible_raw(
            "https://integrate.api.nvidia.com/v1/chat/completions",
            "nvidia",
            credential,
            request,
        )
        .await
    }

    async fn stream_openai_compatible_raw(
        &self,
        endpoint: &str,
        provider_label: &str,
        credential: &InferenceCredential,
        request: &InferenceRequest,
    ) -> Result<reqwest::Response, String> {
        let token = credential.secret().ok_or(format!(
            "{provider_label} streaming needs a configured bearer credential."
        ))?;
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
            .header("Authorization", format!("Bearer {token}"))
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
                    text: Some(request.system_prompt.clone()),
                    inline_data: None,
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
                parts: {
                    let inline_images: Vec<_> = m
                        .attachments
                        .iter()
                        .filter_map(|attachment| {
                            attachment.data_base64.as_ref().map(|data| GeminiPart {
                                text: None,
                                inline_data: Some(GeminiInlineData {
                                    mime_type: attachment.media_type.clone(),
                                    data: data.clone(),
                                }),
                            })
                        })
                        .collect();

                    if inline_images.is_empty() || !m.role.eq_ignore_ascii_case("user") {
                        vec![GeminiPart {
                            text: Some(Self::message_text_with_attachment_note(m)),
                            inline_data: None,
                        }]
                    } else {
                        let mut parts = vec![GeminiPart {
                            text: Some(if m.content.trim().is_empty() {
                                "Review the attached image context and respond to the operator."
                                    .to_string()
                            } else {
                                m.content.clone()
                            }),
                            inline_data: None,
                        }];
                        parts.extend(inline_images);
                        parts
                    }
                },
            })
            .collect();

        if contents.is_empty() {
            contents.push(GeminiContent {
                role: "user".to_string(),
                parts: vec![GeminiPart {
                    text: Some(String::new()),
                    inline_data: None,
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
        credential: &InferenceCredential,
        request: &InferenceRequest,
    ) -> Result<InferenceResponse, String> {
        let body = self.build_gemini_request(request);
        let endpoint = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent",
            request.model
        );
        let mut request_builder = self
            .http
            .post(endpoint)
            .header("content-type", "application/json");

        match credential {
            InferenceCredential::ApiKey(api_key) => {
                request_builder = request_builder.query(&[("key", api_key.as_str())]);
            }
            InferenceCredential::BearerToken { .. } => {
                let token = credential
                    .bearer()
                    .ok_or("Gemini OAuth requires a bearer token.".to_string())?;
                let user_project = credential.user_project().ok_or(
                    "Gemini OAuth requires GOOGLE_CLOUD_PROJECT (or GOOGLE_PROJECT_ID / GCLOUD_PROJECT)."
                        .to_string(),
                )?;
                request_builder = request_builder
                    .header("Authorization", format!("Bearer {token}"))
                    .header("x-goog-user-project", user_project);
            }
            InferenceCredential::None => {
                return Err(
                    "Gemini requires GEMINI_API_KEY / GOOGLE_API_KEY, or Google ADC OAuth."
                        .to_string(),
                );
            }
        }

        let resp = request_builder
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
        credential: &InferenceCredential,
        request: &InferenceRequest,
    ) -> Result<reqwest::Response, String> {
        let body = self.build_gemini_request(request);
        let endpoint = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:streamGenerateContent?alt=sse",
            request.model
        );
        let mut request_builder = self
            .http
            .post(endpoint)
            .header("content-type", "application/json");

        match credential {
            InferenceCredential::ApiKey(api_key) => {
                request_builder = request_builder.query(&[("key", api_key.as_str())]);
            }
            InferenceCredential::BearerToken { .. } => {
                let token = credential
                    .bearer()
                    .ok_or("Gemini OAuth requires a bearer token.".to_string())?;
                let user_project = credential.user_project().ok_or(
                    "Gemini OAuth requires GOOGLE_CLOUD_PROJECT (or GOOGLE_PROJECT_ID / GCLOUD_PROJECT)."
                        .to_string(),
                )?;
                request_builder = request_builder
                    .header("Authorization", format!("Bearer {token}"))
                    .header("x-goog-user-project", user_project);
            }
            InferenceCredential::None => {
                return Err(
                    "Gemini streaming requires GEMINI_API_KEY / GOOGLE_API_KEY, or Google ADC OAuth."
                        .to_string(),
                );
            }
        }

        let resp = request_builder
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

    // ── Gemini CLI (subprocess) ──────────────────────────────────────────

    fn build_gemini_cli_prompt(&self, request: &InferenceRequest) -> String {
        let mut prompt = String::new();

        if !request.system_prompt.trim().is_empty() {
            prompt.push_str("System instructions:\n");
            prompt.push_str(request.system_prompt.trim());
            prompt.push_str("\n\n");
        }

        prompt.push_str("Conversation:\n");
        for message in &request.messages {
            let role = if message.role.eq_ignore_ascii_case("assistant") {
                "assistant"
            } else {
                "user"
            };
            prompt.push_str(&format!(
                "{role}: {}\n",
                Self::message_text_with_attachment_note(message)
            ));
        }

        prompt.push_str("\nRespond as assistant only. Keep the answer concise and complete.");
        prompt
    }

    fn parse_gemini_cli_json(
        &self,
        stdout: &str,
    ) -> Result<(String, Option<u32>, Option<u32>, Option<String>), String> {
        let parse_value = serde_json::from_str::<serde_json::Value>(stdout).or_else(|_| {
            // Some toolchains prepend logs; try to parse the last JSON line.
            stdout
                .lines()
                .rev()
                .find_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
                .ok_or_else(|| serde_json::Error::io(std::io::Error::other("invalid json")))
        });

        let value = match parse_value {
            Ok(v) => v,
            Err(_) => {
                let text = stdout.trim();
                if text.is_empty() {
                    return Err("Gemini CLI returned empty output".to_string());
                }
                return Ok((text.to_string(), None, None, None));
            }
        };

        if let Some(err_obj) = value.get("error") {
            let err_text = err_obj
                .get("message")
                .and_then(|v| v.as_str())
                .or_else(|| err_obj.as_str())
                .unwrap_or("unknown Gemini CLI error")
                .to_string();
            return Err(format!("Gemini CLI error: {err_text}"));
        }

        let response_text = value
            .get("response")
            .and_then(|v| v.as_str())
            .or_else(|| value.get("text").and_then(|v| v.as_str()))
            .or_else(|| {
                value
                    .get("result")
                    .and_then(|r| r.get("response"))
                    .and_then(|v| v.as_str())
            })
            .unwrap_or("")
            .trim()
            .to_string();

        if response_text.is_empty() {
            return Err("Gemini CLI returned JSON without a response field".to_string());
        }

        let stats = value
            .get("stats")
            .cloned()
            .unwrap_or(serde_json::Value::Null);

        let read_u32 = |obj: &serde_json::Value, keys: &[&str]| -> Option<u32> {
            keys.iter().find_map(|k| {
                obj.get(k)
                    .and_then(|v| v.as_u64())
                    .and_then(|n| u32::try_from(n).ok())
            })
        };

        let input_tokens = read_u32(
            &stats,
            &[
                "input_tokens",
                "inputTokens",
                "prompt_tokens",
                "promptTokenCount",
            ],
        );
        let output_tokens = read_u32(
            &stats,
            &[
                "output_tokens",
                "outputTokens",
                "completion_tokens",
                "completionTokenCount",
                "candidateTokenCount",
                "candidatesTokenCount",
            ],
        );
        let stop_reason = stats
            .get("stop_reason")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        Ok((response_text, input_tokens, output_tokens, stop_reason))
    }

    async fn complete_gemini_cli(
        &self,
        request: &InferenceRequest,
    ) -> Result<InferenceResponse, String> {
        let prompt = self.build_gemini_cli_prompt(request);

        let output = Command::new("gemini")
            .arg("--model")
            .arg(&request.model)
            .arg("--output-format")
            .arg("json")
            .arg(prompt)
            .output()
            .await
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    "Gemini CLI executable not found. Install `@google/gemini-cli` and ensure `gemini` is on PATH.".to_string()
                } else {
                    format!("Failed to launch Gemini CLI: {e}")
                }
            })?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        if !output.status.success() {
            let code = output
                .status
                .code()
                .map(|c| c.to_string())
                .unwrap_or_else(|| "terminated".to_string());

            let detail = if !stderr.trim().is_empty() {
                stderr.trim().to_string()
            } else if !stdout.trim().is_empty() {
                stdout.trim().to_string()
            } else {
                "unknown error".to_string()
            };

            return Err(format!(
                "Gemini CLI failed (exit code {code}): {detail}. If this is first run, execute `gemini` once and complete OAuth login."
            ));
        }

        let (content, input_tokens, output_tokens, stop_reason) =
            self.parse_gemini_cli_json(&stdout)?;

        Ok(InferenceResponse {
            content,
            model: request.model.clone(),
            provider: "gemini-cli".to_string(),
            input_tokens,
            output_tokens,
            stop_reason,
        })
    }

    fn build_codex_cli_prompt(&self, request: &InferenceRequest) -> String {
        let mut prompt = String::new();

        if !request.system_prompt.trim().is_empty() {
            prompt.push_str("System instructions:\n");
            prompt.push_str(request.system_prompt.trim());
            prompt.push_str("\n\n");
        }

        prompt.push_str("Conversation:\n");
        for message in &request.messages {
            let role = if message.role.eq_ignore_ascii_case("assistant") {
                "assistant"
            } else {
                "user"
            };
            prompt.push_str(&format!(
                "{role}: {}\n",
                Self::message_text_with_attachment_note(message)
            ));
        }

        prompt.push_str(
            "\nReturn the assistant response only. Keep the answer concise, complete, and directly responsive to the latest user message.",
        );
        prompt
    }

    fn parse_codex_cli_json(
        &self,
        stdout: &str,
    ) -> Result<(String, Option<u32>, Option<u32>, Option<String>), String> {
        let mut response_text: Option<String> = None;
        let mut input_tokens: Option<u32> = None;
        let mut output_tokens: Option<u32> = None;
        let mut stop_reason: Option<String> = None;

        for line in stdout.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) else {
                continue;
            };

            match value.get("type").and_then(|v| v.as_str()) {
                Some("item.completed") => {
                    if let Some(item) = value.get("item") {
                        if item.get("type").and_then(|v| v.as_str()) == Some("agent_message") {
                            if let Some(text) = item.get("text").and_then(|v| v.as_str()) {
                                let text = text.trim();
                                if !text.is_empty() {
                                    response_text = Some(text.to_string());
                                }
                            }
                        }
                    }
                }
                Some("turn.completed") => {
                    if let Some(usage) = value.get("usage") {
                        input_tokens = usage
                            .get("input_tokens")
                            .and_then(|v| v.as_u64())
                            .and_then(|v| u32::try_from(v).ok());
                        output_tokens = usage
                            .get("output_tokens")
                            .and_then(|v| v.as_u64())
                            .and_then(|v| u32::try_from(v).ok());
                    }
                }
                Some("error") => {
                    let message = value
                        .get("message")
                        .and_then(|v| v.as_str())
                        .or_else(|| value.get("error").and_then(|v| v.as_str()))
                        .unwrap_or("unknown Codex CLI error");
                    return Err(format!("Codex CLI error: {message}"));
                }
                Some("turn.failed") => {
                    let message = value
                        .get("message")
                        .and_then(|v| v.as_str())
                        .unwrap_or("Codex CLI turn failed");
                    return Err(message.to_string());
                }
                Some("turn.cancelled") => {
                    stop_reason = Some("cancelled".to_string());
                }
                _ => {}
            }
        }

        if let Some(text) = response_text {
            return Ok((text, input_tokens, output_tokens, stop_reason));
        }

        let fallback = stdout.trim();
        if fallback.is_empty() {
            return Err("Codex CLI returned empty output".to_string());
        }

        Ok((
            fallback.to_string(),
            input_tokens,
            output_tokens,
            stop_reason,
        ))
    }

    async fn complete_codex_cli(
        &self,
        request: &InferenceRequest,
    ) -> Result<InferenceResponse, String> {
        let prompt = self.build_codex_cli_prompt(request);
        let mut command = codex_cli_command();
        command
            .arg("exec")
            .arg("--json")
            .arg("--color")
            .arg("never")
            .arg("--skip-git-repo-check")
            .arg("--ephemeral");

        if !request.model.trim().is_empty() {
            command.arg("--model").arg(&request.model);
        }

        command
            .arg("-")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        let mut child = command.spawn().map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                "Codex CLI executable not found. Install Codex CLI and ensure `codex` is on PATH."
                    .to_string()
            } else {
                format!("Failed to launch Codex CLI: {e}")
            }
        })?;

        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(prompt.as_bytes())
                .await
                .map_err(|e| format!("Failed to send prompt to Codex CLI: {e}"))?;
        }

        let output = child
            .wait_with_output()
            .await
            .map_err(|e| format!("Failed to wait for Codex CLI: {e}"))?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        if !output.status.success() {
            let code = output
                .status
                .code()
                .map(|c| c.to_string())
                .unwrap_or_else(|| "terminated".to_string());

            let detail = if !stderr.trim().is_empty() {
                stderr.trim().to_string()
            } else if !stdout.trim().is_empty() {
                stdout.trim().to_string()
            } else {
                "unknown error".to_string()
            };

            return Err(format!(
                "Codex CLI failed (exit code {code}): {detail}. Run `codex login` if OAuth is not configured."
            ));
        }

        let (content, input_tokens, output_tokens, stop_reason) =
            self.parse_codex_cli_json(&stdout)?;

        Ok(InferenceResponse {
            content,
            model: request.model.clone(),
            provider: "codex-cli".to_string(),
            input_tokens,
            output_tokens,
            stop_reason,
        })
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

fn codex_cli_command() -> Command {
    if cfg!(windows) {
        let mut command = Command::new("cmd");
        command.arg("/C").arg("codex");
        command
    } else {
        Command::new("codex")
    }
}

fn chunk_text_for_live_stream(text: &str) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut current = String::new();

    for ch in text.chars() {
        current.push(ch);
        let should_flush = current.len() >= 28
            || ch == '\n'
            || (ch.is_whitespace() && current.len() >= 14);
        if should_flush {
            chunks.push(current.clone());
            current.clear();
        }
    }

    if !current.is_empty() {
        chunks.push(current);
    }

    chunks
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
        assert_eq!(
            InferenceProvider::from_str_loose("gemini-cli"),
            Some(InferenceProvider::GeminiCli)
        );
        assert_eq!(
            InferenceProvider::from_str_loose("codex-cli"),
            Some(InferenceProvider::CodexCli)
        );
        assert_eq!(InferenceProvider::from_str_loose("unknown"), None);
    }

    #[test]
    fn test_provider_vault_key() {
        assert_eq!(InferenceProvider::Anthropic.vault_key(), Some("anthropic"));
        assert_eq!(InferenceProvider::OpenAI.vault_key(), Some("openai"));
        assert_eq!(InferenceProvider::Gemini.vault_key(), Some("gemini"));
        assert_eq!(InferenceProvider::Nvidia.vault_key(), Some("nvidia"));
        assert_eq!(InferenceProvider::GeminiCli.vault_key(), None);
        assert_eq!(InferenceProvider::CodexCli.vault_key(), None);
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
        assert!(
            InferenceProvider::GeminiCli
                .default_model()
                .contains("gemini")
        );
        assert!(InferenceProvider::CodexCli.default_model().contains("gpt"));
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
