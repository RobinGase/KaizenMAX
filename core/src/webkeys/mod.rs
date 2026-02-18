//! WebKeys Module
//!
//! Virtual key system for ChatGPT/Gemini web clients.
//! Provides secure key management with explicit fingerprints.

pub mod service;
pub mod storage;
pub mod types;

// Re-export main types
pub use types::*;
pub use service::WebKeysService;
pub use storage::WebKeysStorage;
