/// WebkeysRuntime — routes an authenticated chat request to the correct
/// `BrowserRuntime` backend and resolves the provider/model binding.
use std::sync::Arc;

use crate::webkeys::runtime_contract::{
    BrowserRuntime, ProviderBindingResolution, RuntimeError, RuntimeMessage, RuntimeRequest,
    RuntimeResult,
};
use crate::webkeys::types::ChatCompletionRequest;

#[derive(Clone)]
pub struct WebkeysRuntime {
    runtime: Arc<dyn BrowserRuntime>,
    default_provider: String,
    default_model: String,
}

impl WebkeysRuntime {
    pub fn new(
        runtime: Arc<dyn BrowserRuntime>,
        default_provider: impl Into<String>,
        default_model: impl Into<String>,
    ) -> Self {
        Self {
            runtime,
            default_provider: default_provider.into(),
            default_model: default_model.into(),
        }
    }

    /// List available model identifiers from the underlying runtime.
    pub fn models(&self) -> Vec<String> {
        self.runtime.list_models()
    }

    /// Execute a chat completion request through the bound runtime.
    pub async fn execute_chat(
        &self,
        request: ChatCompletionRequest,
    ) -> Result<RuntimeResult, RuntimeError> {
        if request.messages.is_empty() {
            return Err(RuntimeError::InvalidRequest(
                "messages must not be empty".to_string(),
            ));
        }

        let model = request.model.unwrap_or_else(|| self.default_model.clone());

        let binding = ProviderBindingResolution {
            provider_id: self.default_provider.clone(),
            model,
        };

        let runtime_request = RuntimeRequest {
            model: Some(binding.model.clone()),
            messages: request
                .messages
                .into_iter()
                .map(|m| RuntimeMessage {
                    role: m.role,
                    content: m.content,
                })
                .collect(),
            stream: request.stream.unwrap_or(false),
        };

        self.runtime.execute(runtime_request, binding).await
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use async_trait::async_trait;

    use super::WebkeysRuntime;
    use crate::webkeys::{
        runtime_contract::{
            BrowserRuntime, ProviderBindingResolution, RuntimeError, RuntimeRequest, RuntimeResult,
        },
        types::{ChatCompletionRequest, ChatMessage},
    };

    struct FakeRuntime;

    #[async_trait]
    impl BrowserRuntime for FakeRuntime {
        async fn execute(
            &self,
            _request: RuntimeRequest,
            binding: ProviderBindingResolution,
        ) -> Result<RuntimeResult, RuntimeError> {
            Ok(RuntimeResult {
                content: "ok".to_string(),
                model: binding.model,
                finish_reason: "stop".to_string(),
            })
        }

        fn list_models(&self) -> Vec<String> {
            vec!["Web-Gem".to_string()]
        }
    }

    #[tokio::test]
    async fn rejects_empty_messages() {
        let rt = WebkeysRuntime::new(Arc::new(FakeRuntime), "gemini-web", "Web-Gem");
        let result = rt
            .execute_chat(ChatCompletionRequest {
                model: None,
                messages: vec![],
                stream: Some(false),
            })
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn routes_to_default_model_when_unspecified() {
        let rt = WebkeysRuntime::new(Arc::new(FakeRuntime), "gemini-web", "Web-Gem");
        let result = rt
            .execute_chat(ChatCompletionRequest {
                model: None,
                messages: vec![ChatMessage {
                    role: "user".to_string(),
                    content: "hello".to_string(),
                }],
                stream: Some(false),
            })
            .await
            .unwrap();
        assert_eq!(result.model, "Web-Gem");
    }
}
