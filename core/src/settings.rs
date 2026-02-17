//! Settings loader
//!
//! Loads configuration from config/defaults.json with .env overrides.
//! Maps to the feature toggles defined in Section 7 of the implementation plan.

use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KaizenSettings {
    pub runtime_engine: String,
    #[serde(default)]
    pub openclaw_compat_enabled: bool,
    #[serde(default)]
    pub auto_spawn_subagents: bool,
    #[serde(default = "default_max_subagents")]
    pub max_subagents: u32,
    #[serde(default = "default_true")]
    pub main_chat_pinned: bool,
    #[serde(default = "default_closed")]
    pub new_agent_chat_default_state: String,
    #[serde(default = "default_true")]
    pub allow_direct_user_to_subagent_chat: bool,
    #[serde(default = "default_true")]
    pub crystal_ball_enabled: bool,
    #[serde(default)]
    pub crystal_ball_default_open: bool,
    #[serde(default = "default_true")]
    pub hard_gates_enabled: bool,
    #[serde(default = "default_true")]
    pub require_human_smoke_test_before_deploy: bool,
    #[serde(default = "default_true")]
    pub provider_inference_only: bool,
}

fn default_max_subagents() -> u32 {
    5
}
fn default_true() -> bool {
    true
}
fn default_closed() -> String {
    "closed".to_string()
}

impl KaizenSettings {
    /// Load settings from a JSON file path.
    pub fn from_file(path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let content = std::fs::read_to_string(path)?;
        let settings: Self = serde_json::from_str(&content)?;
        Ok(settings)
    }

    /// Apply environment variable overrides (ADMIN_ prefix convention).
    pub fn apply_env_overrides(&mut self) {
        if let Ok(val) = std::env::var("ADMIN_HARD_GATES_ENABLED") {
            self.hard_gates_enabled = val.parse().unwrap_or(self.hard_gates_enabled);
        }
        if let Ok(val) = std::env::var("ADMIN_MAX_SUBAGENTS") {
            self.max_subagents = val.parse().unwrap_or(self.max_subagents);
        }
        if let Ok(val) = std::env::var("ADMIN_AUTO_SPAWN") {
            self.auto_spawn_subagents = val.parse().unwrap_or(self.auto_spawn_subagents);
        }
        if let Ok(val) = std::env::var("ADMIN_REQUIRE_HUMAN_SMOKE_TEST") {
            self.require_human_smoke_test_before_deploy = val
                .parse()
                .unwrap_or(self.require_human_smoke_test_before_deploy);
        }
        if let Ok(val) = std::env::var("ADMIN_PROVIDER_INFERENCE_ONLY") {
            self.provider_inference_only = val.parse().unwrap_or(self.provider_inference_only);
        }
    }
}
