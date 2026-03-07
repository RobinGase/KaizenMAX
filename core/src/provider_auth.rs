use crate::{
    inference::{InferenceCredential, InferenceProvider},
    oauth_store::{self, GOOGLE_PROJECT_ENV_HINTS},
    settings::KaizenSettings,
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tokio::process::Command;

#[derive(Debug, Clone, Serialize)]
pub struct ProviderAuthStatus {
    pub provider: String,
    pub resolved_provider: String,
    pub native_alias: bool,
    pub auth_method: String,
    pub configured: bool,
    pub can_chat: bool,
    pub message: String,
    pub env_hints: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ZeroclawProviderAction {
    pub kind: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ZeroclawProviderOption {
    pub id: String,
    pub label: String,
    pub selected: bool,
    pub ready: bool,
    pub connected: bool,
    pub auth_kind: String,
    pub badge: String,
    pub summary: String,
    pub models: Vec<String>,
    pub primary_action: Option<ZeroclawProviderAction>,
    pub secondary_action: Option<ZeroclawProviderAction>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ZeroclawControlPlane {
    pub selected_provider: String,
    pub selected_model: String,
    pub ready: bool,
    pub headline: String,
    pub detail: String,
    pub providers: Vec<ZeroclawProviderOption>,
    pub available_models: Vec<String>,
}

pub async fn collect_provider_auth_statuses(settings: &KaizenSettings) -> Vec<ProviderAuthStatus> {
    let openai = openai_status();
    let anthropic = anthropic_status();
    let gemini = gemini_status();
    let gemini_cli = gemini_cli_status().await;
    let codex_cli = codex_cli_status().await;
    let nvidia = nvidia_status();

    let mut rows = vec![
        zeroclaw_status(
            settings,
            &[
                &openai,
                &anthropic,
                &gemini,
                &gemini_cli,
                &codex_cli,
                &nvidia,
            ],
        ),
        openai,
        anthropic,
        gemini,
        gemini_cli,
        codex_cli,
        nvidia,
    ];

    // Keep the native alias first so the configured runtime path is obvious in the UI.
    rows.sort_by_key(|row| if row.native_alias { 0 } else { 1 });
    rows
}

pub async fn provider_auth_status(provider: &str, settings: &KaizenSettings) -> ProviderAuthStatus {
    if is_native_alias(provider) {
        return zeroclaw_status(
            settings,
            &[
                &openai_status(),
                &anthropic_status(),
                &gemini_status(),
                &gemini_cli_status().await,
                &codex_cli_status().await,
                &nvidia_status(),
            ],
        );
    }

    match canonical_provider_id(provider) {
        Some("openai") => openai_status(),
        Some("anthropic") => anthropic_status(),
        Some("gemini") => gemini_status(),
        Some("gemini-cli") => gemini_cli_status().await,
        Some("codex-cli") => codex_cli_status().await,
        Some("nvidia") => nvidia_status(),
        Some(other) => unsupported_status(other),
        None => unsupported_status(provider.trim()),
    }
}

pub async fn zeroclaw_control_plane(settings: &KaizenSettings) -> ZeroclawControlPlane {
    let statuses = collect_provider_auth_statuses(settings).await;
    let mut provider_rows = Vec::new();

    for status in statuses.iter().filter(|row| !row.native_alias) {
        let models = provider_model_catalog(status.provider.as_str());
        let selected = canonical_provider_id(&settings.inference_provider) == Some(status.provider.as_str());
        let (badge, summary, primary_action, secondary_action) =
            zeroclaw_option_presentation(status);

        provider_rows.push(ZeroclawProviderOption {
            id: status.provider.clone(),
            label: provider_label(status.provider.as_str()).to_string(),
            selected,
            ready: status.can_chat,
            connected: status.configured,
            auth_kind: status.auth_method.clone(),
            badge,
            summary,
            models,
            primary_action,
            secondary_action,
        });
    }

    provider_rows.sort_by_key(|row| {
        (
            if row.selected { 0 } else if row.ready { 1 } else { 2 },
            row.label.clone(),
        )
    });

    let selected_provider = canonical_provider_id(&settings.inference_provider)
        .unwrap_or(settings.inference_provider.trim())
        .to_string();
    let selected_row = provider_rows
        .iter()
        .find(|row| row.id == selected_provider)
        .or_else(|| provider_rows.iter().find(|row| row.selected));
    let available_models = selected_row
        .map(|row| row.models.clone())
        .unwrap_or_else(|| provider_model_catalog(selected_provider.as_str()));
    let ready = selected_row.map(|row| row.ready).unwrap_or(false);

    let headline = if let Some(row) = selected_row {
        if row.ready {
            format!("Zeroclaw is ready with {}", row.label)
        } else if row.connected {
            format!("{} needs attention", row.label)
        } else {
            format!("Finish connecting {}", row.label)
        }
    } else {
        "Choose a provider for Zeroclaw".to_string()
    };

    let detail = if let Some(base) = statuses.iter().find(|row| row.provider == selected_provider) {
        user_facing_status_detail(base)
    } else {
        "Choose the provider you want Zeroclaw to use, then pick a model.".to_string()
    };

    ZeroclawControlPlane {
        selected_provider,
        selected_model: settings.inference_model.clone(),
        ready,
        headline,
        detail,
        providers: provider_rows,
        available_models,
    }
}

pub async fn resolve_credential(
    provider: InferenceProvider,
) -> Result<InferenceCredential, String> {
    match provider {
        InferenceProvider::Anthropic => env_api_key("ANTHROPIC_API_KEY")
            .map(InferenceCredential::ApiKey)
            .map_err(|_| "No Anthropic credential configured. Set ANTHROPIC_API_KEY.".to_string()),
        InferenceProvider::OpenAI => env_api_key("OPENAI_API_KEY")
            .map(|token| InferenceCredential::BearerToken {
                token,
                user_project: None,
            })
            .map_err(|_| "No OpenAI credential configured. Set OPENAI_API_KEY.".to_string()),
        InferenceProvider::Nvidia => env_api_key("NVIDIA_API_KEY")
            .map(|token| InferenceCredential::BearerToken {
                token,
                user_project: None,
            })
            .map_err(|_| "No NVIDIA credential configured. Set NVIDIA_API_KEY.".to_string()),
        InferenceProvider::Gemini => resolve_gemini_credential().await,
        InferenceProvider::GeminiCli => Ok(InferenceCredential::None),
        InferenceProvider::CodexCli => Ok(InferenceCredential::None),
    }
}

fn openai_status() -> ProviderAuthStatus {
    api_key_status(
        "openai",
        "OPENAI_API_KEY",
        "OpenAI API access uses API keys in the current build.",
    )
}

fn anthropic_status() -> ProviderAuthStatus {
    api_key_status(
        "anthropic",
        "ANTHROPIC_API_KEY",
        "Anthropic API access uses API keys in the current build.",
    )
}

fn nvidia_status() -> ProviderAuthStatus {
    api_key_status(
        "nvidia",
        "NVIDIA_API_KEY",
        "NVIDIA NIM access uses API keys in the current build.",
    )
}

fn gemini_status() -> ProviderAuthStatus {
    let local_oauth_status = match oauth_store::stored_gemini_oauth_status() {
        Ok(status) => Some(status),
        Err(error) => {
            let warning = format!("Stored Gemini OAuth session is unreadable: {error}");
            if let Some((env_name, _)) = first_present_env(["GEMINI_API_KEY", "GOOGLE_API_KEY"]) {
                return ProviderAuthStatus {
                    provider: "gemini".to_string(),
                    resolved_provider: "gemini".to_string(),
                    native_alias: false,
                    auth_method: "api_key_env".to_string(),
                    configured: true,
                    can_chat: true,
                    message: format!("Configured via {env_name}. {warning}"),
                    env_hints: gemini_env_hints(),
                };
            }

            if let Some((env_name, _)) =
                first_present_env(["GOOGLE_OAUTH_ACCESS_TOKEN", "GEMINI_OAUTH_ACCESS_TOKEN"])
            {
                let project_id = google_project_id();
                let can_chat = project_id.is_some();
                let message = if let Some(project_id) = project_id {
                    format!(
                        "Configured via {env_name} with Google project '{project_id}'. {warning}"
                    )
                } else {
                    format!(
                        "{env_name} is set, but Gemini OAuth also needs GOOGLE_CLOUD_PROJECT (or GOOGLE_PROJECT_ID / GCLOUD_PROJECT). {warning}"
                    )
                };

                return ProviderAuthStatus {
                    provider: "gemini".to_string(),
                    resolved_provider: "gemini".to_string(),
                    native_alias: false,
                    auth_method: "oauth_access_token_env".to_string(),
                    configured: true,
                    can_chat,
                    message,
                    env_hints: gemini_env_hints(),
                };
            }

            if google_adc_present() {
                let project_id = google_project_id();
                let can_chat = project_id.is_some();
                let message = if let Some(project_id) = project_id {
                    format!(
                        "Configured for Google ADC OAuth with project '{project_id}'. Run `gcloud auth application-default login` if token acquisition fails. {warning}"
                    )
                } else {
                    format!(
                        "Google ADC credentials were detected, but Gemini OAuth still needs GOOGLE_CLOUD_PROJECT (or GOOGLE_PROJECT_ID / GCLOUD_PROJECT). {warning}"
                    )
                };

                return ProviderAuthStatus {
                    provider: "gemini".to_string(),
                    resolved_provider: "gemini".to_string(),
                    native_alias: false,
                    auth_method: "oauth_adc".to_string(),
                    configured: true,
                    can_chat,
                    message,
                    env_hints: gemini_env_hints(),
                };
            }

            return ProviderAuthStatus {
                provider: "gemini".to_string(),
                resolved_provider: "gemini".to_string(),
                native_alias: false,
                auth_method: "local_oauth".to_string(),
                configured: true,
                can_chat: false,
                message: warning,
                env_hints: gemini_env_hints(),
            };
        }
    };

    if let Some(local_oauth_status) = local_oauth_status.as_ref() {
        if local_oauth_status.present && local_oauth_status.connected() {
            return ProviderAuthStatus {
                provider: "gemini".to_string(),
                resolved_provider: "gemini".to_string(),
                native_alias: false,
                auth_method: "local_oauth".to_string(),
                configured: true,
                can_chat: true,
                message: local_oauth_status.message.clone(),
                env_hints: gemini_env_hints(),
            };
        }
    }

    if let Some((env_name, _)) = first_present_env(["GEMINI_API_KEY", "GOOGLE_API_KEY"]) {
        let message = if let Some(local_oauth_status) = local_oauth_status.as_ref() {
            if local_oauth_status.present && !local_oauth_status.connected() {
                format!(
                    "Configured via {env_name}. Stored Gemini OAuth tokens are ignored: {}",
                    local_oauth_status.message
                )
            } else {
                format!("Configured via {env_name}.")
            }
        } else {
            format!("Configured via {env_name}.")
        };

        return ProviderAuthStatus {
            provider: "gemini".to_string(),
            resolved_provider: "gemini".to_string(),
            native_alias: false,
            auth_method: "api_key_env".to_string(),
            configured: true,
            can_chat: true,
            message,
            env_hints: gemini_env_hints(),
        };
    }

    if let Some((env_name, _)) =
        first_present_env(["GOOGLE_OAUTH_ACCESS_TOKEN", "GEMINI_OAUTH_ACCESS_TOKEN"])
    {
        let project_id = google_project_id();
        let can_chat = project_id.is_some();
        let message = if let Some(project_id) = project_id {
            format!("Configured via {env_name} with Google project '{project_id}'.")
        } else {
            format!(
                "{env_name} is set, but Gemini OAuth also needs GOOGLE_CLOUD_PROJECT (or GOOGLE_PROJECT_ID / GCLOUD_PROJECT)."
            )
        };

        return ProviderAuthStatus {
            provider: "gemini".to_string(),
            resolved_provider: "gemini".to_string(),
            native_alias: false,
            auth_method: "oauth_access_token_env".to_string(),
            configured: true,
            can_chat,
            message: append_gemini_local_oauth_warning(message, local_oauth_status.as_ref()),
            env_hints: gemini_env_hints(),
        };
    }

    if google_adc_present() {
        let project_id = google_project_id();
        let can_chat = project_id.is_some();
        let message = if let Some(project_id) = project_id {
            format!(
                "Configured for Google ADC OAuth with project '{project_id}'. Run `gcloud auth application-default login` if token acquisition fails."
            )
        } else {
            "Google ADC credentials were detected, but Gemini OAuth still needs GOOGLE_CLOUD_PROJECT (or GOOGLE_PROJECT_ID / GCLOUD_PROJECT).".to_string()
        };

        return ProviderAuthStatus {
            provider: "gemini".to_string(),
            resolved_provider: "gemini".to_string(),
            native_alias: false,
            auth_method: "oauth_adc".to_string(),
            configured: true,
            can_chat,
            message: append_gemini_local_oauth_warning(message, local_oauth_status.as_ref()),
            env_hints: gemini_env_hints(),
        };
    }

    if let Some(local_oauth_status) = local_oauth_status {
        if local_oauth_status.present {
            return ProviderAuthStatus {
                provider: "gemini".to_string(),
                resolved_provider: "gemini".to_string(),
                native_alias: false,
                auth_method: "local_oauth".to_string(),
                configured: true,
                can_chat: false,
                message: local_oauth_status.message,
                env_hints: gemini_env_hints(),
            };
        }
    }

    ProviderAuthStatus {
        provider: "gemini".to_string(),
        resolved_provider: "gemini".to_string(),
        native_alias: false,
        auth_method: "unconfigured".to_string(),
        configured: false,
        can_chat: false,
        message: "Set GEMINI_API_KEY / GOOGLE_API_KEY, connect Gemini OAuth in Providers & Auth, or configure Google ADC OAuth.".to_string(),
        env_hints: gemini_env_hints(),
    }
}

async fn gemini_cli_status() -> ProviderAuthStatus {
    let available = gemini_cli_available().await;
    let message = if available {
        "Gemini CLI detected. If this is first use, run `gemini` once to complete its local OAuth login."
            .to_string()
    } else {
        "Gemini CLI not found. Install `@google/gemini-cli` and complete local OAuth login."
            .to_string()
    };

    ProviderAuthStatus {
        provider: "gemini-cli".to_string(),
        resolved_provider: "gemini-cli".to_string(),
        native_alias: false,
        auth_method: "local_cli_oauth".to_string(),
        configured: available,
        can_chat: available,
        message,
        env_hints: vec!["PATH (gemini executable)".to_string()],
    }
}

#[derive(Debug, Deserialize)]
struct CodexAuthFile {
    #[serde(default)]
    auth_mode: Option<String>,
}

async fn codex_cli_status() -> ProviderAuthStatus {
    let available = codex_cli_available().await;
    if !available {
        return ProviderAuthStatus {
            provider: "codex-cli".to_string(),
            resolved_provider: "codex-cli".to_string(),
            native_alias: false,
            auth_method: "local_cli_oauth".to_string(),
            configured: false,
            can_chat: false,
            message:
                "Codex CLI not found. Install Codex CLI and run `codex login` to complete ChatGPT OAuth."
                    .to_string(),
            env_hints: codex_cli_hints(),
        };
    }

    let auth_mode = codex_cli_auth_mode();
    match codex_cli_login_status().await {
        Some(status) if status.to_ascii_lowercase().contains("logged in") => {
            let method = match auth_mode.as_deref() {
                Some("chatgpt") => "local_cli_oauth",
                Some("api_key") => "local_cli_api_key",
                _ => "local_cli_auth",
            };
            let detail = match auth_mode.as_deref() {
                Some("chatgpt") => "Codex CLI detected and logged in with ChatGPT OAuth.",
                Some("api_key") => "Codex CLI detected and logged in with an API key.",
                _ => "Codex CLI detected and logged in.",
            };

            ProviderAuthStatus {
                provider: "codex-cli".to_string(),
                resolved_provider: "codex-cli".to_string(),
                native_alias: false,
                auth_method: method.to_string(),
                configured: true,
                can_chat: true,
                message: format!("{detail} {}", status.trim()),
                env_hints: codex_cli_hints(),
            }
        }
        Some(status) => ProviderAuthStatus {
            provider: "codex-cli".to_string(),
            resolved_provider: "codex-cli".to_string(),
            native_alias: false,
            auth_method: "local_cli_oauth".to_string(),
            configured: false,
            can_chat: false,
            message: format!(
                "Codex CLI is installed, but login is not ready. {} Run `codex login`.",
                status.trim()
            ),
            env_hints: codex_cli_hints(),
        },
        None => {
            let suffix = if codex_auth_file_exists() {
                "Local Codex auth files were found, but `codex login status` did not report an active session."
            } else {
                "No local Codex auth session was detected."
            };

            ProviderAuthStatus {
                provider: "codex-cli".to_string(),
                resolved_provider: "codex-cli".to_string(),
                native_alias: false,
                auth_method: "local_cli_oauth".to_string(),
                configured: false,
                can_chat: false,
                message: format!("{suffix} Run `codex login` and complete ChatGPT OAuth."),
                env_hints: codex_cli_hints(),
            }
        }
    }
}

fn zeroclaw_status(
    settings: &KaizenSettings,
    concrete_rows: &[&ProviderAuthStatus],
) -> ProviderAuthStatus {
    let configured = settings.inference_provider.trim();
    let resolved = canonical_provider_id(configured).unwrap_or(configured);

    if let Some(base) = concrete_rows.iter().find(|row| row.provider == resolved) {
        return ProviderAuthStatus {
            provider: "zeroclaw".to_string(),
            resolved_provider: base.provider.clone(),
            native_alias: true,
            auth_method: base.auth_method.clone(),
            configured: base.configured,
            can_chat: base.can_chat,
            message: format!(
                "Zeroclaw routes to the configured provider '{}'. {}",
                base.provider, base.message
            ),
            env_hints: base.env_hints.clone(),
        };
    }

    ProviderAuthStatus {
        provider: "zeroclaw".to_string(),
        resolved_provider: configured.to_string(),
        native_alias: true,
        auth_method: "invalid_config".to_string(),
        configured: false,
        can_chat: false,
        message: format!(
            "Zeroclaw is mapped to '{}', but that is not a supported concrete provider. Set inference_provider to openai, anthropic, gemini, gemini-cli, codex-cli, or nvidia.",
            configured
        ),
        env_hints: vec![],
    }
}

fn unsupported_status(provider: &str) -> ProviderAuthStatus {
    ProviderAuthStatus {
        provider: provider.to_string(),
        resolved_provider: provider.to_string(),
        native_alias: false,
        auth_method: "unsupported".to_string(),
        configured: false,
        can_chat: false,
        message: format!("Provider '{provider}' is not supported in the current build."),
        env_hints: vec![],
    }
}

fn api_key_status(provider: &str, env_name: &str, missing_note: &str) -> ProviderAuthStatus {
    let configured = env_api_key(env_name).is_ok();
    let message = if configured {
        format!("Configured via {env_name}.")
    } else {
        format!("Set {env_name}. {missing_note}")
    };

    ProviderAuthStatus {
        provider: provider.to_string(),
        resolved_provider: provider.to_string(),
        native_alias: false,
        auth_method: "api_key_env".to_string(),
        configured,
        can_chat: configured,
        message,
        env_hints: vec![env_name.to_string()],
    }
}

fn env_api_key(name: &str) -> Result<String, ()> {
    std::env::var(name)
        .map(|value| value.trim().to_string())
        .ok()
        .filter(|value| !value.is_empty())
        .ok_or(())
}

fn first_present_env<const N: usize>(keys: [&str; N]) -> Option<(String, String)> {
    for key in keys {
        if let Ok(value) = std::env::var(key) {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                return Some((key.to_string(), trimmed.to_string()));
            }
        }
    }
    None
}

pub fn canonical_provider_id(value: &str) -> Option<&'static str> {
    match value.trim().to_ascii_lowercase().as_str() {
        "openai" | "gpt" | "codex" => Some("openai"),
        "anthropic" | "claude" => Some("anthropic"),
        "gemini" | "google" | "googleai" => Some("gemini"),
        "gemini-cli" | "geminicli" | "google-cli" => Some("gemini-cli"),
        "codex-cli" | "codexcli" | "openai-cli" => Some("codex-cli"),
        "nvidia" | "nim" => Some("nvidia"),
        _ => None,
    }
}

fn is_native_alias(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "zeroclaw" | "kaizen" | "kai-zen" | "native"
    )
}

pub fn provider_model_catalog(provider: &str) -> Vec<String> {
    match canonical_provider_id(provider).unwrap_or(provider.trim()) {
        "anthropic" => vec![
            "claude-sonnet-4-20250514".to_string(),
            "claude-3-7-sonnet-latest".to_string(),
            "claude-3-5-haiku-latest".to_string(),
        ],
        "openai" => vec![
            "gpt-5.4".to_string(),
            "gpt-5".to_string(),
            "gpt-4.1".to_string(),
            "gpt-4.1-mini".to_string(),
            "o3-mini".to_string(),
        ],
        "gemini" => vec![
            "gemini-2.5-pro".to_string(),
            "gemini-2.5-flash".to_string(),
            "gemini-2.0-flash".to_string(),
        ],
        "gemini-cli" => vec![
            "gemini-2.5-pro".to_string(),
            "gemini-2.5-flash".to_string(),
        ],
        "codex-cli" => vec![
            "gpt-5.4".to_string(),
            "gpt-5-codex".to_string(),
            "use-codex-config".to_string(),
        ],
        "nvidia" => vec![
            "meta/llama-3.1-70b-instruct".to_string(),
            "meta/llama-3.3-70b-instruct".to_string(),
            "mistralai/mixtral-8x7b-instruct-v0.1".to_string(),
        ],
        _ => Vec::new(),
    }
}

fn provider_label(provider: &str) -> &'static str {
    match provider {
        "anthropic" => "Claude",
        "openai" => "OpenAI",
        "gemini" => "Gemini",
        "gemini-cli" => "Gemini CLI",
        "codex-cli" => "Codex",
        "nvidia" => "NVIDIA",
        _ => "Provider",
    }
}

fn zeroclaw_option_presentation(
    status: &ProviderAuthStatus,
) -> (
    String,
    String,
    Option<ZeroclawProviderAction>,
    Option<ZeroclawProviderAction>,
) {
    let badge = if status.can_chat {
        "Ready".to_string()
    } else if status.configured {
        "Needs attention".to_string()
    } else {
        "Not connected".to_string()
    };

    let summary = user_facing_status_detail(status);

    let primary_action = match status.provider.as_str() {
        "gemini" if status.can_chat => Some(ZeroclawProviderAction {
            kind: "refresh".to_string(),
            label: "Refresh sign-in".to_string(),
        }),
        "gemini" => Some(ZeroclawProviderAction {
            kind: "connect".to_string(),
            label: "Connect".to_string(),
        }),
        _ => None,
    };

    let secondary_action = match status.provider.as_str() {
        "gemini" if status.configured => Some(ZeroclawProviderAction {
            kind: "disconnect".to_string(),
            label: "Disconnect".to_string(),
        }),
        _ => None,
    };

    (badge, summary, primary_action, secondary_action)
}

fn user_facing_status_detail(status: &ProviderAuthStatus) -> String {
    match status.provider.as_str() {
        "codex-cli" => {
            if status.can_chat {
                "Ready on this device.".to_string()
            } else {
                "Sign in with Codex on this device to use it.".to_string()
            }
        }
        "gemini" => match status.auth_method.as_str() {
            "local_oauth" if status.can_chat => "Connected with Google sign-in.".to_string(),
            "local_oauth" => "Use Google sign-in to finish connecting Gemini.".to_string(),
            "api_key_env" => "Ready on this device.".to_string(),
            "oauth_access_token_env" | "oauth_adc" => "Ready on this device.".to_string(),
            _ => "Connect Gemini to use it with Zeroclaw.".to_string(),
        },
        "openai" | "anthropic" | "nvidia" => {
            if status.can_chat {
                "Ready on this device.".to_string()
            } else {
                "This provider becomes ready when its key is available on this device.".to_string()
            }
        }
        "gemini-cli" => {
            if status.can_chat {
                "Ready on this device.".to_string()
            } else {
                "Sign in with Gemini CLI on this device to use it.".to_string()
            }
        }
        _ => {
            if status.can_chat {
                "Ready.".to_string()
            } else {
                "Not ready yet.".to_string()
            }
        }
    }
}

fn google_project_id() -> Option<String> {
    first_present_env(GOOGLE_PROJECT_ENV_HINTS).map(|(_, value)| value)
}

fn google_adc_present() -> bool {
    if let Ok(path) = std::env::var("GOOGLE_APPLICATION_CREDENTIALS") {
        if path_exists(path.as_str()) {
            return true;
        }
    }

    google_adc_candidates()
        .iter()
        .any(|candidate| candidate.exists())
}

fn google_adc_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    if let Ok(appdata) = std::env::var("APPDATA") {
        candidates.push(
            PathBuf::from(appdata)
                .join("gcloud")
                .join("application_default_credentials.json"),
        );
    }

    if let Ok(user_profile) = std::env::var("USERPROFILE") {
        candidates.push(
            PathBuf::from(&user_profile)
                .join(".config")
                .join("gcloud")
                .join("application_default_credentials.json"),
        );
        candidates.push(
            PathBuf::from(user_profile)
                .join("AppData")
                .join("Roaming")
                .join("gcloud")
                .join("application_default_credentials.json"),
        );
    }

    if let Ok(home) = std::env::var("HOME") {
        candidates.push(
            PathBuf::from(&home)
                .join(".config")
                .join("gcloud")
                .join("application_default_credentials.json"),
        );
        candidates.push(
            PathBuf::from(home)
                .join("Library")
                .join("Application Support")
                .join("gcloud")
                .join("application_default_credentials.json"),
        );
    }

    candidates
}

fn path_exists(path: &str) -> bool {
    let trimmed = path.trim();
    !trimmed.is_empty() && Path::new(trimmed).exists()
}

async fn gemini_cli_available() -> bool {
    match Command::new("gemini").arg("--version").output().await {
        Ok(output) => output.status.success() || !output.stderr.is_empty(),
        Err(_) => false,
    }
}

async fn codex_cli_available() -> bool {
    match codex_cli_command().arg("--version").output().await {
        Ok(output) => {
            output.status.success() || !output.stdout.is_empty() || !output.stderr.is_empty()
        }
        Err(_) => false,
    }
}

async fn codex_cli_login_status() -> Option<String> {
    let output = codex_cli_command()
        .args(["login", "status"])
        .output()
        .await
        .ok()?;

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let text = if !stdout.is_empty() { stdout } else { stderr };
    if text.is_empty() { None } else { Some(text) }
}

fn codex_cli_auth_mode() -> Option<String> {
    let path = codex_auth_file_candidates()
        .into_iter()
        .find(|path| path.exists())?;
    let text = std::fs::read_to_string(path).ok()?;
    let parsed = serde_json::from_str::<CodexAuthFile>(&text).ok()?;
    parsed
        .auth_mode
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
}

fn codex_auth_file_exists() -> bool {
    codex_auth_file_candidates()
        .into_iter()
        .any(|path| path.exists())
}

fn codex_auth_file_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    if let Ok(home) = std::env::var("HOME") {
        candidates.push(PathBuf::from(home).join(".codex").join("auth.json"));
    }

    if let Ok(user_profile) = std::env::var("USERPROFILE") {
        candidates.push(PathBuf::from(user_profile).join(".codex").join("auth.json"));
    }

    candidates
}

fn codex_cli_hints() -> Vec<String> {
    vec![
        "PATH (codex executable)".to_string(),
        "~/.codex/auth.json".to_string(),
        "Run `codex login`".to_string(),
    ]
}

fn codex_cli_command() -> Command {
    if cfg!(windows) {
        let mut command = Command::new("cmd");
        command.arg("/C").arg("codex");
        command
    } else {
        Command::new("codex")
    }
}

async fn resolve_gemini_credential() -> Result<InferenceCredential, String> {
    let mut local_oauth_error: Option<String> = None;
    match oauth_store::load_or_refresh_gemini_tokens().await {
        Ok(Some(tokens)) => {
            return Ok(InferenceCredential::BearerToken {
                token: tokens.access_token,
                user_project: Some(tokens.project_id),
            });
        }
        Ok(None) => {}
        Err(error) => local_oauth_error = Some(error),
    }

    if let Some((_, value)) = first_present_env(["GEMINI_API_KEY", "GOOGLE_API_KEY"]) {
        return Ok(InferenceCredential::ApiKey(value));
    }

    if let Some((_, token)) =
        first_present_env(["GOOGLE_OAUTH_ACCESS_TOKEN", "GEMINI_OAUTH_ACCESS_TOKEN"])
    {
        let user_project = google_project_id().ok_or_else(|| {
            "Gemini OAuth is configured with an access token, but GOOGLE_CLOUD_PROJECT (or GOOGLE_PROJECT_ID / GCLOUD_PROJECT) is missing.".to_string()
        })?;

        return Ok(InferenceCredential::BearerToken {
            token,
            user_project: Some(user_project),
        });
    }

    if google_adc_present() {
        let user_project = google_project_id().ok_or_else(|| {
            "Gemini ADC OAuth was detected, but GOOGLE_CLOUD_PROJECT (or GOOGLE_PROJECT_ID / GCLOUD_PROJECT) is missing.".to_string()
        })?;

        let token = google_adc_access_token().await?;
        return Ok(InferenceCredential::BearerToken {
            token,
            user_project: Some(user_project),
        });
    }

    if let Some(error) = local_oauth_error {
        return Err(error);
    }

    Err("No Gemini credential configured. Set GEMINI_API_KEY / GOOGLE_API_KEY, connect Gemini OAuth in Providers & Auth, or configure Google ADC OAuth.".to_string())
}

fn gemini_env_hints() -> Vec<String> {
    vec![
        "GEMINI_API_KEY".to_string(),
        "GOOGLE_API_KEY".to_string(),
        "GOOGLE_OAUTH_CLIENT_ID".to_string(),
        "KAIZEN_GEMINI_OAUTH_CLIENT_ID".to_string(),
        "GOOGLE_CLOUD_PROJECT".to_string(),
        "GOOGLE_APPLICATION_CREDENTIALS".to_string(),
    ]
}

fn append_gemini_local_oauth_warning(
    message: String,
    local_oauth_status: Option<&oauth_store::StoredGeminiOAuthStatus>,
) -> String {
    if let Some(local_oauth_status) = local_oauth_status {
        if local_oauth_status.present && !local_oauth_status.connected() {
            return format!(
                "{} Stored Gemini OAuth tokens are ignored: {}",
                message, local_oauth_status.message
            );
        }
    }

    message
}

async fn google_adc_access_token() -> Result<String, String> {
    let output = Command::new("gcloud")
        .args(["auth", "application-default", "print-access-token"])
        .output()
        .await
        .map_err(|e| {
            format!(
                "Google ADC credentials were detected, but `gcloud` could not be started: {e}. Install Google Cloud CLI or set GOOGLE_OAUTH_ACCESS_TOKEN."
            )
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(format!(
            "Google ADC token acquisition failed. Run `gcloud auth application-default login`. Details: {}",
            if stderr.is_empty() {
                "no stderr output".to_string()
            } else {
                stderr
            }
        ));
    }

    let token = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if token.is_empty() {
        return Err(
            "Google ADC returned an empty access token. Run `gcloud auth application-default login` again.".to_string(),
        );
    }

    Ok(token)
}
