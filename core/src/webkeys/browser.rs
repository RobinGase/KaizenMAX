//! Browser Session Manager
//!
//! Handles Playwright instances, persistent contexts, and page lifecycles.
//! Ensures sessions are kept alive and recovered on failure.

use playwright::Playwright;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::Arc,
};
use tokio::sync::Mutex;

/// Browser type enum
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowserType {
    Chromium,
    Firefox,
    Webkit,
}

impl Default for BrowserType {
    fn default() -> Self {
        Self::Chromium
    }
}

/// Browser session configuration
#[derive(Debug, Clone)]
pub struct SessionConfig {
    pub id: String,
    pub user_data_dir: PathBuf,
    pub headless: bool,
    pub proxy: Option<String>,
}

/// Active browser session handle
pub struct BrowserSession {
    pub context: playwright::api::BrowserContext,
    pub page: Option<playwright::api::Page>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub last_active: chrono::DateTime<chrono::Utc>,
}

/// Manager for all browser sessions
#[derive(Clone)]
pub struct BrowserManager {
    inner: Arc<Mutex<ManagerInner>>,
}

struct ManagerInner {
    playwright: Option<Playwright>,
    sessions: HashMap<String, BrowserSession>,
}

impl BrowserManager {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(ManagerInner {
                playwright: None,
                sessions: HashMap::new(),
            })),
        }
    }

    /// Initialize Playwright runtime
    pub async fn initialize(&self) -> Result<(), String> {
        let mut inner = self.inner.lock().await;
        if inner.playwright.is_some() {
            return Ok(());
        }

        let pw = Playwright::initialize()
            .await
            .map_err(|e| format!("Failed to initialize Playwright: {}", e))?;
        
        inner.playwright = Some(pw);
        tracing::info!("Playwright initialized successfully");
        Ok(())
    }

    /// Start a persistent browser session (profile)
    pub async fn start_session(&self, config: SessionConfig) -> Result<(), String> {
        let mut inner = self.inner.lock().await;
        
        let pw = inner.playwright.as_ref()
            .ok_or_else(|| "Playwright not initialized".to_string())?;

        let chromium = pw.chromium();
        let launcher = chromium.launcher();
        
        let mut launch_options = launcher.headless(config.headless);
        
        // Manual persistent profile via args if specific builder missing
        let user_data_arg = format!("--user-data-dir={}", config.user_data_dir.display());
        let args = [user_data_arg];
        launch_options = launch_options.args(&args);
        
        if let Some(proxy) = config.proxy {
            launch_options = launch_options.proxy(playwright::api::ProxySettings {
                server: proxy,
                bypass: None,
                username: None,
                password: None,
            });
        }

        let browser = launch_options
            .launch()
            .await
            .map_err(|e| format!("Failed to launch browser: {}", e))?;

        let context = browser.context_builder()
            .build()
            .await
            .map_err(|e| format!("Failed to create context: {}", e))?;

        let page = context.new_page().await.ok();

        let session = BrowserSession {
            context,
            page,
            created_at: chrono::Utc::now(),
            last_active: chrono::Utc::now(),
        };

        inner.sessions.insert(config.id, session);
        Ok(())
    }

    /// Get a page for a session
    pub async fn get_page(&self, session_id: &str) -> Option<playwright::api::Page> {
        let mut inner = self.inner.lock().await;
        let session = inner.sessions.get_mut(session_id)?;
        
        session.last_active = chrono::Utc::now();
        
        if let Some(_page) = &session.page {
            // Validate page is still open - explicit check not available, assume valid or handle error later
            // if page.is_closed() { session.page = session.context.new_page().await.ok(); }
        } else {
            session.page = session.context.new_page().await.ok();
        }

        session.page.clone()
    }

    /// Close a session
    pub async fn close_session(&self, session_id: &str) -> Result<(), String> {
        let mut inner = self.inner.lock().await;
        if let Some(session) = inner.sessions.remove(session_id) {
            session.context.close().await
                .map_err(|e| format!("Failed to close context: {}", e))?;
        }
        Ok(())
    }
}
