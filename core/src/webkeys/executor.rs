//! Provider Executors
//!
//! Abstraction for interacting with specific AI web clients (ChatGPT, Gemini).
//! Handles navigation, input submission, and response extraction.

use async_trait::async_trait;
use playwright::api::Page;
use std::time::Duration;

#[async_trait]
pub trait WebExecutor: Send + Sync {
    /// Navigate to the provider's chat interface
    async fn ensure_connected(&self, page: &Page) -> Result<(), String>;

    /// Send a user message
    async fn send_message(&self, page: &Page, message: &str) -> Result<(), String>;

    /// Wait for and extract the latest assistant response
    async fn wait_for_response(&self, page: &Page) -> Result<String, String>;
}

pub struct ChatGptExecutor;

#[async_trait]
impl WebExecutor for ChatGptExecutor {
    async fn ensure_connected(&self, page: &Page) -> Result<(), String> {
        // Only navigate if not already on the correct domain
        let url = page.url().unwrap_or_default();
        if !url.contains("chatgpt.com") {
            page.goto_builder("https://chatgpt.com/")
                // .wait_until(...) - LoadState enum issue, skip explicit wait for now or use default
                .goto()
                .await
                .map_err(|e| format!("Failed to navigate to ChatGPT: {}", e))?;
        }
        
        // Check for login selector or chat input
        // Placeholder selector check
        Ok(())
    }

    async fn send_message(&self, page: &Page, message: &str) -> Result<(), String> {
        let textarea = page.wait_for_selector_builder("#prompt-textarea")
            .timeout(5000.0)
            .wait_for_selector()
            .await
            .map_err(|_| "Could not find ChatGPT input area".to_string())?
            .ok_or("Input selector returned null")?;

        textarea.fill_builder(message).fill().await
            .map_err(|e| format!("Failed to fill input: {}", e))?;

        // Click send button (usually has data-testid="send-button")
        let send_btn = page.wait_for_selector_builder("[data-testid='send-button']")
            .timeout(2000.0)
            .wait_for_selector()
            .await
            .map_err(|_| "Send button not found".to_string())?
            .ok_or("Send button null")?;

        send_btn.click_builder().click().await
            .map_err(|e| format!("Failed to click send: {}", e))?;

        Ok(())
    }

    async fn wait_for_response(&self, page: &Page) -> Result<String, String> {
        // Wait for streaming to stop (send button reappears or stop button disappears)
        // This is a naive implementation; real one needs to stream chunks.
        tokio::time::sleep(Duration::from_secs(2)).await;
        
        // Extract last message
        let messages = page.query_selector_all("[data-message-author-role='assistant']")
            .await
            .map_err(|e| format!("Failed to query messages: {}", e))?;
            
        if let Some(last_msg) = messages.last() {
            return last_msg.inner_text().await
                .map_err(|e| format!("Failed to get text: {}", e));
        }
        
        Ok("No response found".to_string())
    }
}

pub struct GeminiExecutor;

#[async_trait]
impl WebExecutor for GeminiExecutor {
    async fn ensure_connected(&self, page: &Page) -> Result<(), String> {
        let url = page.url().unwrap_or_default();
        if !url.contains("gemini.google.com") {
            page.goto_builder("https://gemini.google.com/app")
                // .wait_until(...) - LoadState enum issue
                .goto()
                .await
                .map_err(|e| format!("Failed to navigate to Gemini: {}", e))?;
        }
        Ok(())
    }

    async fn send_message(&self, page: &Page, message: &str) -> Result<(), String> {
        // Gemini input selector often is a contenteditable div
        let editor = page.wait_for_selector_builder("div[contenteditable='true']")
            .timeout(5000.0)
            .wait_for_selector()
            .await
            .map_err(|_| "Could not find Gemini input area".to_string())?
            .ok_or("Input selector null")?;

        editor.fill_builder(message).fill().await
            .map_err(|e| format!("Failed to fill input: {}", e))?;
            
        editor.press_builder("Enter").press().await
            .map_err(|e| format!("Failed to press Enter: {}", e))?;

        Ok(())
    }

    async fn wait_for_response(&self, page: &Page) -> Result<String, String> {
        tokio::time::sleep(Duration::from_secs(3)).await;
        
        // Gemini message selectors vary; naive selector for now
        let responses = page.query_selector_all("model-response")
            .await
            .map_err(|e| format!("Failed to query responses: {}", e))?;
            
        if let Some(last) = responses.last() {
            return last.inner_text().await
                .map_err(|e| format!("Failed to get text: {}", e));
        }
        
        Ok("No response found".to_string())
    }
}
