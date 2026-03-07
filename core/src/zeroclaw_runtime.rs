use crate::{
    inference::InferenceProvider,
    openclaw_bridge,
    provider_auth::{self, ProviderAuthStatus},
    settings::KaizenSettings,
};
use serde::Serialize;
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ZeroclawActionHint {
    pub kind: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ZeroclawProviderOption {
    pub id: String,
    pub label: String,
    pub active: bool,
    pub configured: bool,
    pub ready: bool,
    pub auth_method: String,
    pub message: String,
    pub models: Vec<String>,
    pub auth_action: Option<ZeroclawActionHint>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ZeroclawToolStatus {
    pub id: String,
    pub label: String,
    pub category: String,
    pub available: bool,
    pub connected: bool,
    pub status: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ZeroclawRuntimeStatus {
    pub ready: bool,
    pub active_provider: String,
    pub active_model: String,
    pub connected_accounts: u32,
    pub message: String,
    pub providers: Vec<ZeroclawProviderOption>,
    pub tools: Vec<ZeroclawToolStatus>,
}

pub async fn collect_runtime_status(settings: &KaizenSettings) -> ZeroclawRuntimeStatus {
    let bridge_status = openclaw_bridge::status().await;
    let provider_rows = provider_auth::collect_provider_auth_statuses(settings).await;
    let provider_map: HashMap<_, _> = provider_rows
        .iter()
        .map(|row| (row.provider.as_str(), row))
        .collect();

    let active_provider = canonical_provider_id(&settings.inference_provider)
        .unwrap_or_else(|| settings.inference_provider.trim().to_ascii_lowercase());
    let active_model = if settings.inference_model.trim().is_empty() {
        InferenceProvider::from_str_loose(&active_provider)
            .map(|provider| provider.default_model().to_string())
            .unwrap_or_else(|| "default".to_string())
    } else {
        settings.inference_model.trim().to_string()
    };

    let providers = ordered_provider_ids()
        .iter()
        .filter_map(|provider_id| provider_map.get(provider_id))
        .map(|status| build_provider_option(status, &active_provider))
        .collect::<Vec<_>>();

    let connected_accounts = providers.iter().filter(|provider| provider.ready).count() as u32;
    let ready = providers
        .iter()
        .find(|provider| provider.id == active_provider)
        .map(|provider| provider.ready)
        .unwrap_or(false);
    let message = if ready {
        let mut base = format!(
            "Zeroclaw is ready. {} is active on {}.",
            provider_label(&active_provider),
            active_model
        );
        if bridge_status.enabled && bridge_status.cli_available {
            base.push_str(" OpenClaw fallback is available for missing tools.");
        }
        base
    } else {
        format!(
            "Zeroclaw is not ready yet. Connect {} to get started.",
            provider_label(&active_provider)
        )
    };

    ZeroclawRuntimeStatus {
        ready,
        active_provider: active_provider.clone(),
        active_model,
        connected_accounts,
        message,
        providers,
        tools: build_tool_statuses(
            provider_map.get(active_provider.as_str()).copied(),
            provider_map.get("github").copied(),
            &bridge_status,
        ),
    }
}

pub async fn collect_provider_options(settings: &KaizenSettings) -> Vec<ZeroclawProviderOption> {
    collect_runtime_status(settings).await.providers
}

pub async fn collect_tool_statuses(settings: &KaizenSettings) -> Vec<ZeroclawToolStatus> {
    collect_runtime_status(settings).await.tools
}

fn build_provider_option(
    status: &ProviderAuthStatus,
    active_provider: &str,
) -> ZeroclawProviderOption {
    let id = status.provider.clone();
    ZeroclawProviderOption {
        active: id == active_provider,
        configured: status.configured,
        ready: status.can_chat,
        auth_method: status.auth_method.clone(),
        message: short_provider_message(status),
        models: models_for_provider(&id),
        auth_action: auth_action_for_provider(status),
        label: provider_label(&id).to_string(),
        id,
    }
}

fn build_tool_statuses(
    active_provider: Option<&ProviderAuthStatus>,
    _github: Option<&ProviderAuthStatus>,
    bridge_status: &openclaw_bridge::OpenClawBridgeStatus,
) -> Vec<ZeroclawToolStatus> {
    let chat_ready = active_provider.map(|status| status.can_chat).unwrap_or(false);
    let chat_message = active_provider
        .map(short_provider_message)
        .unwrap_or_else(|| "Pick a provider for Zeroclaw first.".to_string());

    vec![
        ZeroclawToolStatus {
            id: "chat".to_string(),
            label: "Chat".to_string(),
            category: "core".to_string(),
            available: true,
            connected: chat_ready,
            status: if chat_ready { "ready" } else { "needs_setup" }.to_string(),
            message: chat_message,
        },
        planned_tool("shell", "Shell", "core"),
        planned_tool("files", "Files", "core"),
        openclaw_tool("browser", "Browser", "core", bridge_status, false),
        openclaw_tool("scheduler", "Scheduler", "core", bridge_status, false),
        openclaw_tool("sessions", "Sessions", "ops", bridge_status, false),
        planned_tool("gmail", "Gmail", "business"),
        planned_tool("leads", "Leads", "business"),
    ]
}

fn planned_tool(id: &str, label: &str, category: &str) -> ZeroclawToolStatus {
    ZeroclawToolStatus {
        id: id.to_string(),
        label: label.to_string(),
        category: category.to_string(),
        available: false,
        connected: false,
        status: "planned".to_string(),
        message: "Coming soon.".to_string(),
    }
}

fn openclaw_tool(
    id: &str,
    label: &str,
    category: &str,
    bridge_status: &openclaw_bridge::OpenClawBridgeStatus,
    requires_gateway: bool,
) -> ZeroclawToolStatus {
    let allowed = bridge_status
        .allowed_tools
        .iter()
        .any(|tool| tool.eq_ignore_ascii_case(id) || (id == "scheduler" && tool.eq_ignore_ascii_case("cron")));
    let available = bridge_status.enabled && bridge_status.cli_available && allowed;
    let connected = available && (!requires_gateway || bridge_status.gateway_reachable);
    let status = if connected {
        "ready"
    } else if available {
        "available"
    } else {
        "planned"
    };
    let message = if connected {
        "Available through OpenClaw fallback.".to_string()
    } else if available && requires_gateway {
        "Available through OpenClaw fallback when the OpenClaw gateway is running.".to_string()
    } else if available {
        "Available through OpenClaw fallback.".to_string()
    } else {
        "Coming soon.".to_string()
    };

    ZeroclawToolStatus {
        id: id.to_string(),
        label: label.to_string(),
        category: category.to_string(),
        available,
        connected,
        status: status.to_string(),
        message,
    }
}

fn auth_action_for_provider(status: &ProviderAuthStatus) -> Option<ZeroclawActionHint> {
    if status.can_chat {
        return None;
    }

    let (kind, label) = match status.provider.as_str() {
        "codex-cli" => ("local_cli", "Add Account"),
        "gemini-cli" => ("local_cli", "Add Account"),
        "gemini" => ("browser_oauth", "Connect"),
        "openai" | "anthropic" | "nvidia" => ("environment", "Set API Key"),
        _ => return None,
    };

    Some(ZeroclawActionHint {
        kind: kind.to_string(),
        label: label.to_string(),
    })
}

fn short_provider_message(status: &ProviderAuthStatus) -> String {
    match status.provider.as_str() {
        "codex-cli" if status.can_chat => "Signed in and ready.".to_string(),
        "codex-cli" => "Sign in with your Codex account.".to_string(),
        "gemini" if status.can_chat => "Connected and ready.".to_string(),
        "gemini" => "Connect Gemini to use it with Zeroclaw.".to_string(),
        "gemini-cli" if status.can_chat => "Gemini CLI is signed in.".to_string(),
        "gemini-cli" => "Sign in with Gemini CLI.".to_string(),
        "openai" if status.can_chat => "OpenAI API key detected.".to_string(),
        "openai" => "Set an OpenAI API key.".to_string(),
        "anthropic" if status.can_chat => "Anthropic API key detected.".to_string(),
        "anthropic" => "Set an Anthropic API key.".to_string(),
        "nvidia" if status.can_chat => "NVIDIA API key detected.".to_string(),
        "nvidia" => "Set an NVIDIA API key.".to_string(),
        _ => status.message.clone(),
    }
}

fn ordered_provider_ids() -> [&'static str; 6] {
    ["codex-cli", "gemini", "openai", "anthropic", "nvidia", "gemini-cli"]
}

fn provider_label(id: &str) -> &'static str {
    match id {
        "codex-cli" => "Codex",
        "gemini" => "Gemini",
        "openai" => "OpenAI",
        "anthropic" => "Anthropic",
        "nvidia" => "NVIDIA",
        "gemini-cli" => "Gemini CLI",
        _ => "Provider",
    }
}

fn canonical_provider_id(value: &str) -> Option<String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "zeroclaw" | "kaizen" | "kai-zen" | "native" => Some("codex-cli".to_string()),
        "codex" | "codex-cli" | "codexcli" | "openai-cli" => Some("codex-cli".to_string()),
        "gemini" | "google" | "googleai" => Some("gemini".to_string()),
        "gemini-cli" | "geminicli" | "google-cli" => Some("gemini-cli".to_string()),
        "openai" | "gpt" => Some("openai".to_string()),
        "anthropic" | "claude" => Some("anthropic".to_string()),
        "nvidia" | "nim" => Some("nvidia".to_string()),
        other if !other.is_empty() => Some(other.to_string()),
        _ => None,
    }
}

fn models_for_provider(provider: &str) -> Vec<String> {
    match provider {
        "codex-cli" => vec!["gpt-5.4", "gpt-5", "gpt-4.1"],
        "gemini" => vec!["gemini-2.5-pro", "gemini-2.5-flash", "gemini-1.5-pro"],
        "gemini-cli" => vec!["gemini-2.5-flash", "gemini-2.5-pro"],
        "openai" => vec!["gpt-5", "gpt-5-mini", "gpt-4.1"],
        "anthropic" => vec!["claude-sonnet-4-20250514", "claude-opus-4-20250514"],
        "nvidia" => vec![
            "nvidia/llama-3.3-nemotron-super-49b-v1",
            "meta/llama-3.1-405b-instruct",
        ],
        _ => vec![],
    }
    .into_iter()
    .map(str::to_string)
    .collect()
}
