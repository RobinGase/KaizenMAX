//! Settings loader
//!
//! Loads configuration from config/defaults.json with .env overrides.
//! Maps to the feature toggles defined in Section 7 of the implementation plan.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

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
    #[serde(default = "default_true")]
    pub credentials_ui_enabled: bool,
    #[serde(default = "default_true")]
    pub oauth_ui_enabled: bool,
    #[serde(default = "default_true")]
    pub agent_name_editable_after_spawn: bool,
    #[serde(default = "default_encrypted_vault")]
    pub secrets_storage_mode: String,
    #[serde(default)]
    pub write_plaintext_secrets_to_env: bool,
    #[serde(default = "default_true")]
    pub show_only_masked_secrets_in_ui: bool,
    #[serde(default)]
    pub mattermost_url: String,
    #[serde(default)]
    pub mattermost_channel_id: String,
    #[serde(default = "default_inference_provider")]
    pub inference_provider: String,
    #[serde(default = "default_inference_model")]
    pub inference_model: String,
    #[serde(default = "default_inference_max_tokens")]
    pub inference_max_tokens: u32,
    #[serde(default = "default_inference_temperature")]
    pub inference_temperature: f32,
}

fn default_encrypted_vault() -> String {
    "encrypted_vault".to_string()
}
fn default_inference_provider() -> String {
    "anthropic".to_string()
}
fn default_inference_model() -> String {
    "claude-sonnet-4-20250514".to_string()
}
fn default_inference_max_tokens() -> u32 {
    4096
}
fn default_inference_temperature() -> f32 {
    0.7
}

impl Default for KaizenSettings {
    fn default() -> Self {
        Self {
            runtime_engine: "zeroclaw".to_string(),
            openclaw_compat_enabled: false,
            auto_spawn_subagents: false,
            max_subagents: 5,
            main_chat_pinned: true,
            new_agent_chat_default_state: "closed".to_string(),
            allow_direct_user_to_subagent_chat: true,
            crystal_ball_enabled: true,
            crystal_ball_default_open: false,
            hard_gates_enabled: true,
            require_human_smoke_test_before_deploy: true,
            provider_inference_only: true,
            credentials_ui_enabled: true,
            oauth_ui_enabled: true,
            agent_name_editable_after_spawn: true,
            secrets_storage_mode: "encrypted_vault".to_string(),
            write_plaintext_secrets_to_env: false,
            show_only_masked_secrets_in_ui: true,
            mattermost_url: String::new(),
            mattermost_channel_id: String::new(),
            inference_provider: "anthropic".to_string(),
            inference_model: "claude-sonnet-4-20250514".to_string(),
            inference_max_tokens: 4096,
            inference_temperature: 0.7,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct SettingsPatch {
    pub runtime_engine: Option<String>,
    pub openclaw_compat_enabled: Option<bool>,
    pub auto_spawn_subagents: Option<bool>,
    pub max_subagents: Option<u32>,
    pub main_chat_pinned: Option<bool>,
    pub new_agent_chat_default_state: Option<String>,
    pub allow_direct_user_to_subagent_chat: Option<bool>,
    pub crystal_ball_enabled: Option<bool>,
    pub crystal_ball_default_open: Option<bool>,
    pub hard_gates_enabled: Option<bool>,
    pub require_human_smoke_test_before_deploy: Option<bool>,
    pub provider_inference_only: Option<bool>,
    pub credentials_ui_enabled: Option<bool>,
    pub oauth_ui_enabled: Option<bool>,
    pub agent_name_editable_after_spawn: Option<bool>,
    pub secrets_storage_mode: Option<String>,
    pub write_plaintext_secrets_to_env: Option<bool>,
    pub show_only_masked_secrets_in_ui: Option<bool>,
    pub mattermost_url: Option<String>,
    pub mattermost_channel_id: Option<String>,
    pub inference_provider: Option<String>,
    pub inference_model: Option<String>,
    pub inference_max_tokens: Option<u32>,
    pub inference_temperature: Option<f32>,
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

    /// Load settings from config/defaults.json candidates with env overrides.
    pub fn load_from_workspace() -> Self {
        let mut settings = Self::default();

        for candidate in settings_path_candidates() {
            if !candidate.exists() {
                continue;
            }

            match Self::from_file(&candidate) {
                Ok(file_settings) => {
                    tracing::info!("Loaded settings from {}", candidate.display());
                    settings = file_settings;
                    break;
                }
                Err(err) => {
                    tracing::warn!(
                        "Failed to parse settings file {}: {}",
                        candidate.display(),
                        err
                    );
                }
            }
        }

        settings.apply_env_overrides();
        settings
    }

    /// Apply environment variable overrides (ADMIN_ prefix convention).
    pub fn apply_env_overrides(&mut self) {
        if let Ok(val) = std::env::var("RUNTIME_ENGINE") {
            self.runtime_engine = val;
        }
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
        if let Ok(val) = std::env::var("KAIZEN_INFERENCE_PROVIDER") {
            self.inference_provider = val;
        }
        if let Ok(val) = std::env::var("KAIZEN_INFERENCE_MODEL") {
            self.inference_model = val;
        }
        if let Ok(val) = std::env::var("KAIZEN_INFERENCE_MAX_TOKENS") {
            self.inference_max_tokens = val.parse().unwrap_or(self.inference_max_tokens);
        }
        if let Ok(val) = std::env::var("KAIZEN_INFERENCE_TEMPERATURE") {
            self.inference_temperature = val.parse().unwrap_or(self.inference_temperature);
        }
        if let Ok(val) = std::env::var("MATTERMOST_URL") {
            self.mattermost_url = val;
        }
        if let Ok(val) = std::env::var("MATTERMOST_CHANNEL_ID") {
            self.mattermost_channel_id = val;
        } else if let Ok(val) = std::env::var("CRYSTAL_BALL_CHANNEL") {
            self.mattermost_channel_id = val;
        }
    }

    pub fn apply_patch(&mut self, patch: SettingsPatch) {
        if let Some(value) = patch.runtime_engine {
            self.runtime_engine = value;
        }
        if let Some(value) = patch.openclaw_compat_enabled {
            self.openclaw_compat_enabled = value;
        }
        if let Some(value) = patch.auto_spawn_subagents {
            self.auto_spawn_subagents = value;
        }
        if let Some(value) = patch.max_subagents {
            self.max_subagents = value;
        }
        if let Some(value) = patch.main_chat_pinned {
            self.main_chat_pinned = value;
        }
        if let Some(value) = patch.new_agent_chat_default_state {
            self.new_agent_chat_default_state = value;
        }
        if let Some(value) = patch.allow_direct_user_to_subagent_chat {
            self.allow_direct_user_to_subagent_chat = value;
        }
        if let Some(value) = patch.crystal_ball_enabled {
            self.crystal_ball_enabled = value;
        }
        if let Some(value) = patch.crystal_ball_default_open {
            self.crystal_ball_default_open = value;
        }
        if let Some(value) = patch.hard_gates_enabled {
            self.hard_gates_enabled = value;
        }
        if let Some(value) = patch.require_human_smoke_test_before_deploy {
            self.require_human_smoke_test_before_deploy = value;
        }
        if let Some(value) = patch.provider_inference_only {
            self.provider_inference_only = value;
        }
        if let Some(value) = patch.credentials_ui_enabled {
            self.credentials_ui_enabled = value;
        }
        if let Some(value) = patch.oauth_ui_enabled {
            self.oauth_ui_enabled = value;
        }
        if let Some(value) = patch.agent_name_editable_after_spawn {
            self.agent_name_editable_after_spawn = value;
        }
        if let Some(value) = patch.secrets_storage_mode {
            self.secrets_storage_mode = value;
        }
        if let Some(value) = patch.write_plaintext_secrets_to_env {
            self.write_plaintext_secrets_to_env = value;
        }
        if let Some(value) = patch.show_only_masked_secrets_in_ui {
            self.show_only_masked_secrets_in_ui = value;
        }
        if let Some(value) = patch.mattermost_url {
            self.mattermost_url = value;
        }
        if let Some(value) = patch.mattermost_channel_id {
            self.mattermost_channel_id = value;
        }
        if let Some(value) = patch.inference_provider {
            self.inference_provider = value;
        }
        if let Some(value) = patch.inference_model {
            self.inference_model = value;
        }
        if let Some(value) = patch.inference_max_tokens {
            self.inference_max_tokens = value;
        }
        if let Some(value) = patch.inference_temperature {
            self.inference_temperature = value;
        }
    }

    /// Persist current settings to the workspace defaults file so UI updates
    /// survive restarts without manual file editing.
    pub fn persist_to_workspace(&self) -> Result<PathBuf, String> {
        let path = settings_write_path();
        if let Some(parent) = path.parent() {
            if !parent.exists() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("Failed to create settings directory: {e}"))?;
            }
        }

        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("Failed to serialize settings: {e}"))?;

        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, json).map_err(|e| format!("Failed to write settings tmp: {e}"))?;
        std::fs::rename(&tmp, &path)
            .map_err(|e| format!("Failed to persist settings file: {e}"))?;

        Ok(path)
    }
}

fn settings_path_candidates() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    if let Ok(path) = std::env::var("KAIZEN_SETTINGS_PATH") {
        paths.push(PathBuf::from(path));
    }

    paths.push(PathBuf::from("../config/defaults.json"));
    paths.push(PathBuf::from("config/defaults.json"));

    paths
}

fn settings_write_path() -> PathBuf {
    if let Ok(path) = std::env::var("KAIZEN_SETTINGS_PATH") {
        return PathBuf::from(path);
    }

    let workspace_path = PathBuf::from("../config/defaults.json");
    if workspace_path.exists() {
        return workspace_path;
    }

    let local_path = PathBuf::from("config/defaults.json");
    if local_path.exists() {
        return local_path;
    }

    workspace_path
}
