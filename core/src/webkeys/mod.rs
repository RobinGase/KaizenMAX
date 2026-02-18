//! WebKeys Module
//!
//! Virtual key system for ChatGPT/Gemini web clients.
//! Provides secure key management with explicit fingerprints.

pub mod browser;
pub mod executor;
pub mod service;
pub mod storage;
pub mod types;

// Re-export main types
pub use types::*;
pub use service::{WebKeysService, WebKeysServiceConfig};
pub use storage::WebKeysStorage;
pub use browser::BrowserManager;
pub use executor::{WebExecutor, ChatGptExecutor, GeminiExecutor};
