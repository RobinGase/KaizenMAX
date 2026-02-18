/// Browser session state manager (chromiumoxide backend).
///
/// Tracks per-provider profile directories, authentication state, and
/// restart counters.  The actual chromiumoxide `Browser` handle is owned
/// by the executor that uses it; this struct is the shared state that the
/// health checker and executor both read/write.
use std::{path::PathBuf, sync::Arc};
use tokio::sync::RwLock;

#[derive(Debug, Clone)]
pub struct BrowserState {
    pub profile_dir: PathBuf,
    pub authenticated: bool,
    pub restart_count: u32,
}

/// Shared, cloneable handle to a browser session's mutable state.
#[derive(Clone)]
pub struct BrowserManager {
    state: Arc<RwLock<BrowserState>>,
}

impl BrowserManager {
    /// Create a new manager for the given profile directory.
    pub fn new(profile_dir: impl Into<PathBuf>) -> Self {
        Self {
            state: Arc::new(RwLock::new(BrowserState {
                profile_dir: profile_dir.into(),
                authenticated: false,
                restart_count: 0,
            })),
        }
    }

    pub async fn profile_dir(&self) -> PathBuf {
        self.state.read().await.profile_dir.clone()
    }

    pub async fn is_authenticated(&self) -> bool {
        self.state.read().await.authenticated
    }

    pub async fn mark_authenticated(&self, authenticated: bool) {
        self.state.write().await.authenticated = authenticated;
    }

    /// Increment restart counter (called after a browser crash/recovery).
    pub async fn restart(&self) {
        self.state.write().await.restart_count += 1;
    }

    pub async fn restart_count(&self) -> u32 {
        self.state.read().await.restart_count
    }
}

#[cfg(test)]
mod tests {
    use super::BrowserManager;

    #[tokio::test]
    async fn profile_reuse_and_restart_tracking() {
        let manager = BrowserManager::new("profiles/gemini");
        assert!(manager.profile_dir().await.ends_with("profiles/gemini"));
        assert!(!manager.is_authenticated().await);
        assert_eq!(manager.restart_count().await, 0);

        manager.mark_authenticated(true).await;
        assert!(manager.is_authenticated().await);

        manager.restart().await;
        manager.restart().await;
        assert_eq!(manager.restart_count().await, 2);
    }
}
