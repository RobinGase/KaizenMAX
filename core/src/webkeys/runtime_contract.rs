/// Frozen runtime contract for the WebKeys browser execution layer.
///
/// No signature changes without explicit approval.
/// This trait is the integration boundary between the key/routing layer
/// and the browser execution backends (Gemini, ChatGPT, API key proxies).
use async_trait::async_trait;

#[derive(Debug, Clone)]
pub struct RuntimeRequest {
    pub model: Option<String>,
    pub messages: Vec<RuntimeMessage>,
    pub stream: bool,
}

#[derive(Debug, Clone)]
pub struct RuntimeMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone)]
pub struct RuntimeResult {
    pub content: String,
    pub model: String,
    pub finish_reason: String,
}

#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    #[error("authentication required for provider")]
    AuthRequired,
    #[error("runtime unavailable: {0}")]
    Unavailable(String),
    #[error("invalid runtime request: {0}")]
    InvalidRequest(String),
}

#[derive(Debug, Clone)]
pub struct ProviderBindingResolution {
    pub provider_id: String,
    pub model: String,
}

#[async_trait]
pub trait BrowserRuntime: Send + Sync {
    async fn execute(
        &self,
        request: RuntimeRequest,
        binding: ProviderBindingResolution,
    ) -> Result<RuntimeResult, RuntimeError>;

    fn list_models(&self) -> Vec<String>;
}
