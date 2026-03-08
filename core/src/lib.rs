//! Kaizen Gateway library
//!
//! Module structure for Kaizen MAX core runtime.

pub mod agents;
pub mod crystal_ball;
pub mod event_archive;
pub mod gate_engine;
pub mod inference;
pub mod oauth_store;
pub mod openclaw_bridge;
pub mod provider_auth;
pub mod providers;
pub mod settings;
pub mod worker_runtime;
pub mod zeroclaw_runtime;
pub mod zeroclaw_tools;
// vault module removed - now lives in standalone Kai-Vault repo
// see: D:\KaizenInnovations\Kai-Vault
