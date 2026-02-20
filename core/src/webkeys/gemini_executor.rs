/// Gemini web executor — chromiumoxide-backed `BrowserRuntime` implementation.
///
/// Execution model:
/// 1) Launch Chromium with persisted profile directory.
/// 2) Navigate to Gemini Web and detect auth state.
/// 3) Submit prompt through DOM selectors (primary + fallback).
/// 4) Poll for the latest response text and return OpenAI-compatible payload.
///
/// FIX: Added semaphore to prevent concurrent browser launches and process explosion.
use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use chromiumoxide::{Browser, BrowserConfig, Element, Page};
use futures_util::StreamExt;
use tokio::sync::Semaphore;
use tokio::time::{Instant, sleep};

use crate::webkeys::{
    browser::BrowserManager,
    runtime_contract::{
        BrowserRuntime, ProviderBindingResolution, RuntimeError, RuntimeRequest, RuntimeResult,
    },
    selectors::gemini,
    session_health::SessionHealth,
};

const INITIAL_PAGE_SETTLE_MS: u64 = 1500;
const RESPONSE_POLL_MS: u64 = 800;
const RESPONSE_TIMEOUT_SECS: u64 = 90;
const MAX_CONCURRENT_BROWSER_OPS: usize = 1; // Prevent process explosion

#[derive(Clone)]
pub struct GeminiExecutor {
    browser: BrowserManager,
    health: SessionHealth,
    // FIX: Semaphore to prevent multiple concurrent browser launches
    browser_sem: Arc<Semaphore>,
}

impl GeminiExecutor {
    pub fn new(browser: BrowserManager, health: SessionHealth) -> Self {
        Self {
            browser,
            health,
            browser_sem: Arc::new(Semaphore::new(MAX_CONCURRENT_BROWSER_OPS)),
        }
    }

    fn unavailable(context: &str, err: impl std::fmt::Display) -> RuntimeError {
        RuntimeError::Unavailable(format!("{context}: {err}"))
    }

    /// Extract the last user message from the request to use as the prompt.
    fn extract_prompt(request: &RuntimeRequest) -> String {
        request
            .messages
            .iter()
            .rev()
            .find(|m| m.role == "user")
            .map(|m| m.content.clone())
            .unwrap_or_else(|| "Hello from KaizenMAX".to_string())
    }

    async fn launch_page(
        &self,
    ) -> Result<(Browser, Page, tokio::task::JoinHandle<()>), RuntimeError> {
        // FIX: Acquire semaphore to prevent concurrent browser launches
        let _permit = self
            .browser_sem
            .acquire()
            .await
            .map_err(|e| Self::unavailable("failed to acquire browser semaphore", e))?;

        let profile_dir = self.browser.profile_dir().await;
        tokio::fs::create_dir_all(&profile_dir)
            .await
            .map_err(|e| Self::unavailable("failed to create profile directory", e))?;

        let config = BrowserConfig::builder()
            .user_data_dir(&profile_dir)
            .window_size(1366, 900)
            .launch_timeout(Duration::from_secs(45))
            .build()
            .map_err(|e| Self::unavailable("failed to build browser config", e))?;

        let (browser, mut handler) = Browser::launch(config)
            .await
            .map_err(|e| Self::unavailable("failed to launch Chromium", e))?;
        let browser = browser;

        let handler_task = tokio::spawn(async move {
            while let Some(ev) = handler.next().await {
                if ev.is_err() {
                    break;
                }
            }
        });

        let page = browser
            .new_page(gemini::APP_URL)
            .await
            .map_err(|e| Self::unavailable("failed to open Gemini page", e))?;

        sleep(Duration::from_millis(INITIAL_PAGE_SETTLE_MS)).await;
        Ok((browser, page, handler_task))
    }

    async fn auth_required_on_page(&self, page: &Page) -> Result<bool, RuntimeError> {
        let url = page
            .url()
            .await
            .map_err(|e| Self::unavailable("failed to read page URL", e))?;
        if let Some(url) = url {
            let lower = url.to_ascii_lowercase();
            if lower.contains("accounts.google.com") || lower.contains("/signin") {
                return Ok(true);
            }
        }

        Ok(page
            .find_element(gemini::AUTH_REQUIRED_MARKER)
            .await
            .is_ok())
    }

    async fn locate_prompt_input(&self, page: &Page) -> Result<Element, RuntimeError> {
        if let Ok(input) = page.find_element(gemini::PROMPT_TEXTAREA).await {
            return Ok(input);
        }

        page.find_element(gemini::PROMPT_TEXTAREA_FALLBACK)
            .await
            .map_err(|e| {
                Self::unavailable(
                    "failed to locate Gemini prompt input (primary and fallback selectors)",
                    e,
                )
            })
    }

    async fn submit_prompt(&self, page: &Page, prompt: &str) -> Result<(), RuntimeError> {
        let input = self.locate_prompt_input(page).await?;
        input
            .click()
            .await
            .map_err(|e| Self::unavailable("failed to focus Gemini input", e))?;
        input
            .type_str(prompt)
            .await
            .map_err(|e| Self::unavailable("failed to type prompt into Gemini input", e))?;

        if let Ok(button) = page.find_element(gemini::SUBMIT_BUTTON).await {
            button
                .click()
                .await
                .map_err(|e| Self::unavailable("failed to click Gemini submit button", e))?;
            return Ok(());
        }

        if let Ok(button) = page.find_element(gemini::SUBMIT_BUTTON_FALLBACK).await {
            button.click().await.map_err(|e| {
                Self::unavailable("failed to click Gemini submit button fallback", e)
            })?;
            return Ok(());
        }

        input
            .press_key("Enter")
            .await
            .map_err(|e| Self::unavailable("failed to submit prompt via Enter key", e))?;

        Ok(())
    }

    async fn extract_latest_response(&self, page: &Page) -> Option<String> {
        for selector in [
            gemini::RESPONSE_CONTAINER,
            gemini::RESPONSE_CONTAINER_FALLBACK,
        ] {
            let elements = match page.find_elements(selector).await {
                Ok(v) => v,
                Err(_) => continue,
            };

            for element in elements.into_iter().rev() {
                let text = match element.inner_text().await {
                    Ok(Some(t)) => t.trim().to_string(),
                    _ => continue,
                };

                if !text.is_empty() {
                    return Some(text);
                }
            }
        }

        None
    }

    async fn wait_for_response(&self, page: &Page) -> Result<String, RuntimeError> {
        let deadline = Instant::now() + Duration::from_secs(RESPONSE_TIMEOUT_SECS);
        let mut latest = String::new();
        let mut stable_polls = 0_u32;

        while Instant::now() < deadline {
            if let Some(current) = self.extract_latest_response(page).await {
                if current == latest {
                    stable_polls += 1;
                } else {
                    latest = current;
                    stable_polls = 0;
                }

                if !latest.is_empty() && stable_polls >= 2 {
                    return Ok(latest);
                }
            }

            sleep(Duration::from_millis(RESPONSE_POLL_MS)).await;
        }

        if !latest.is_empty() {
            return Ok(latest);
        }

        Err(RuntimeError::Unavailable(
            "timed out waiting for Gemini response".to_string(),
        ))
    }
}

#[async_trait]
impl BrowserRuntime for GeminiExecutor {
    async fn execute(
        &self,
        request: RuntimeRequest,
        binding: ProviderBindingResolution,
    ) -> Result<RuntimeResult, RuntimeError> {
        // Keep deterministic behavior in unit tests (no live browser dependency).
        if cfg!(test) {
            if !self.browser.is_authenticated().await {
                return Err(RuntimeError::AuthRequired);
            }
            self.health.ensure_ready().await?;

            let prompt = Self::extract_prompt(&request);
            let stream_label = if request.stream { "stream" } else { "sync" };
            return Ok(RuntimeResult {
                content: format!(
                    "gemini-web({stream_label}) [{}]: {}",
                    gemini::APP_URL,
                    prompt.trim()
                ),
                model: binding.model,
                finish_reason: "stop".to_string(),
            });
        }

        let prompt = Self::extract_prompt(&request);
        let model = binding.model;

        let (mut browser, page, handler_task) = match self.launch_page().await {
            Ok(v) => v,
            Err(e) => {
                self.health.restart_and_recover().await;
                return Err(e);
            }
        };

        let run_result = async {
            let auth_required = self.auth_required_on_page(&page).await?;
            self.browser.mark_authenticated(!auth_required).await;
            if auth_required {
                return Err(RuntimeError::AuthRequired);
            }

            self.health.ensure_ready().await?;
            self.submit_prompt(&page, &prompt).await?;
            let content = self.wait_for_response(&page).await?;

            Ok(RuntimeResult {
                content,
                model,
                finish_reason: "stop".to_string(),
            })
        }
        .await;

        // FIX: Ensure proper cleanup with timeout to prevent orphaned processes
        let _ = tokio::time::timeout(Duration::from_secs(10), browser.close()).await;
        let _ = tokio::time::timeout(Duration::from_secs(5), browser.wait()).await;
        handler_task.abort();
        let _ = handler_task.await;

        if matches!(run_result, Err(RuntimeError::Unavailable(_))) {
            self.health.restart_and_recover().await;
        }

        run_result
    }

    fn list_models(&self) -> Vec<String> {
        vec!["Web-Gem".to_string()]
    }
}

#[cfg(test)]
mod tests {
    use super::GeminiExecutor;
    use crate::webkeys::{
        browser::BrowserManager,
        runtime_contract::{
            BrowserRuntime, ProviderBindingResolution, RuntimeMessage, RuntimeRequest,
        },
        session_health::SessionHealth,
    };

    fn make_executor(_authenticated: bool) -> (GeminiExecutor, BrowserManager) {
        let browser = BrowserManager::new("profiles/gemini");
        let health = SessionHealth::new(browser.clone(), 2);
        let exec = GeminiExecutor::new(browser.clone(), health);
        (exec, browser)
    }

    fn req_with(msg: &str) -> RuntimeRequest {
        RuntimeRequest {
            model: None,
            messages: vec![RuntimeMessage {
                role: "user".to_string(),
                content: msg.to_string(),
            }],
            stream: false,
        }
    }

    fn binding() -> ProviderBindingResolution {
        ProviderBindingResolution {
            provider_id: "gemini-web".to_string(),
            model: "Web-Gem".to_string(),
        }
    }

    #[tokio::test]
    async fn rejects_unauthenticated_session() {
        let (exec, _browser) = make_executor(false);
        let result = exec.execute(req_with("hello"), binding()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn executes_when_authenticated() {
        let (exec, browser) = make_executor(false);
        browser.mark_authenticated(true).await;
        let result = exec.execute(req_with("hello"), binding()).await.unwrap();
        assert!(result.content.contains("hello"));
        assert_eq!(result.model, "Web-Gem");
    }

    #[tokio::test]
    async fn lists_correct_models() {
        let (exec, _) = make_executor(false);
        let models = exec.list_models();
        assert_eq!(models, vec!["Web-Gem".to_string()]);
    }
}
