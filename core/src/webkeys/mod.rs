//! WebKeys Module
//!
//! Virtual key system for AI web-client quota abstraction.
//! Provides: key lifecycle (create/rotate/revoke/verify), provider binding,
//! rate limiting, and the OpenAI-compatible /v1/* surface.
//!
//! Browser automation uses chromiumoxide (no Playwright, no Node.js).

pub mod auth;
pub mod browser;
pub mod gemini_executor;
pub mod runtime;
pub mod runtime_contract;
pub mod selectors;
pub mod service;
pub mod session_health;
pub mod storage;
pub mod types;

// Re-export main types
pub use browser::BrowserManager;
pub use runtime::WebkeysRuntime;
pub use service::{WebKeysService, WebKeysServiceConfig};
pub use storage::WebKeysStorage;
pub use types::*;
