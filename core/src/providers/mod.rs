//! Providers module — canonical type registry for all AI provider integrations.
//!
//! # Structure
//! - `types` — `ProviderType`, `Provider`, `ProviderPublic`, `AuthType`,
//!             `RateLimit`, virtual-key types, and input / result shapes.
//! - `crypto` — `CryptoService`: AES-256-GCM encryption, HMAC key hashing,
//!              fingerprint and preview generation.

pub mod crypto;
pub mod types;

// Flat re-exports for convenience.
pub use crypto::{CryptoError, CryptoService};
pub use types::{
    AuthType, CreateProviderInput, CreateVirtualKeyInput, CreateVirtualKeyResult, Provider,
    ProviderId, ProviderPublic, ProviderType, RateLimit, VIRTUAL_KEY_PREFIX, VirtualKey,
    VirtualKeyId, VirtualKeyPublic, VirtualKeyVerification,
};
