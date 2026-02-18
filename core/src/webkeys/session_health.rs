/// Session health guard for browser-backed executors.
///
/// Tracks restart count and authentication state.  Executors call
/// `ensure_ready` before every request; if the session has exceeded its
/// restart budget or is not yet authenticated the call is rejected rather
/// than silently misfiring.
use crate::webkeys::{browser::BrowserManager, runtime_contract::RuntimeError};

#[derive(Clone)]
pub struct SessionHealth {
    browser: BrowserManager,
    max_restarts: u32,
}

impl SessionHealth {
    pub fn new(browser: BrowserManager, max_restarts: u32) -> Self {
        Self {
            browser,
            max_restarts,
        }
    }

    /// Returns `Ok(())` iff the session is authenticated and within its
    /// restart budget.
    pub async fn ensure_ready(&self) -> Result<(), RuntimeError> {
        if self.browser.restart_count().await > self.max_restarts {
            return Err(RuntimeError::Unavailable(
                "session restart limit exceeded".to_string(),
            ));
        }

        if !self.browser.is_authenticated().await {
            return Err(RuntimeError::AuthRequired);
        }

        Ok(())
    }

    /// Increment the restart counter (called by executors after recovery).
    pub async fn restart_and_recover(&self) {
        self.browser.restart().await;
    }
}

#[cfg(test)]
mod tests {
    use super::SessionHealth;
    use crate::webkeys::browser::BrowserManager;

    #[tokio::test]
    async fn requires_auth_before_ready() {
        let browser = BrowserManager::new("profiles/gemini");
        let health = SessionHealth::new(browser.clone(), 2);

        assert!(health.ensure_ready().await.is_err());
        browser.mark_authenticated(true).await;
        assert!(health.ensure_ready().await.is_ok());
    }

    #[tokio::test]
    async fn fails_after_max_restarts() {
        let browser = BrowserManager::new("profiles/gemini");
        browser.mark_authenticated(true).await;
        let health = SessionHealth::new(browser.clone(), 1);

        assert!(health.ensure_ready().await.is_ok());

        browser.restart().await;
        assert!(health.ensure_ready().await.is_ok());

        // One restart above limit
        browser.restart().await;
        assert!(health.ensure_ready().await.is_err());
    }
}
