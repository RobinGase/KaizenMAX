#![allow(non_snake_case)]
#![allow(unused_mut)]

use std::collections::{HashMap, HashSet};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use dioxus::desktop::{
    window, Config as DesktopConfig, LogicalPosition, LogicalSize, WeakDesktopContext,
    WindowBuilder,
};
use dioxus::html::point_interaction::InteractionLocation;
use dioxus::prelude::*;
use reqwest::{Client, RequestBuilder};
use serde::{Deserialize, Serialize};
use tokio::time::sleep;

const API_BASE: &str = "http://127.0.0.1:9100";
const MIN_PANEL_WIDTH: f64 = 380.0;
const MIN_PANEL_HEIGHT: f64 = 200.0;
const MIN_DETACHED_WIDTH: f64 = 420.0;
const MIN_DETACHED_HEIGHT: f64 = 320.0;
const MIN_PANEL_X: f64 = -1000.0;
const MIN_PANEL_Y: f64 = 0.0;
const FLOAT_DRAG_ZONE_HEIGHT: f64 = 64.0;
const KAIZEN_CHAT_MODES: [&str; 5] = ["yolo", "build", "plan", "reason", "orchestrator"];
const SUBAGENT_CHAT_MODES: [&str; 2] = ["build", "plan"];
const CHAT_MODEL_PRESETS: [(&str, &str, &str); 9] = [
    ("kai-zen", "Kai-Zen (Native Toggle)", "native"),
    ("openai", "GPT-5.3 Codex", "gpt-5.3-codex"),
    ("openai", "GPT-4.1 Mini", "gpt-4.1-mini"),
    ("anthropic", "Claude Sonnet 4", "claude-sonnet-4-20250514"),
    (
        "anthropic",
        "Claude 3.5 Sonnet",
        "claude-3-5-sonnet-20241022",
    ),
    ("gemini", "Gemini 2.5 Pro", "gemini-2.5-pro"),
    ("gemini", "Gemini 2.5 Flash", "gemini-2.5-flash"),
    (
        "nvidia",
        "Nemotron Super 49B",
        "nvidia/llama-3.3-nemotron-super-49b-v1",
    ),
    ("nvidia", "Llama 3.1 70B", "meta/llama-3.1-70b-instruct"),
];

fn model_value(provider: &str, model: &str) -> String {
    format!("{provider}|{model}")
}

fn parse_model_value(value: &str) -> Option<(String, String)> {
    let mut parts = value.splitn(2, '|');
    let provider = parts.next()?.trim();
    let model = parts.next()?.trim();
    if provider.is_empty() || model.is_empty() {
        return None;
    }
    Some((provider.to_string(), model.to_string()))
}

fn normalized_model_values(selected: Option<&Vec<String>>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut values = Vec::new();

    if let Some(existing) = selected {
        for raw in existing {
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                continue;
            }
            if seen.insert(trimmed.to_string()) {
                values.push(trimmed.to_string());
            }
        }
    }

    values
}

fn model_values_with_default_fallback(values: &[String], default_chat_model: &str) -> Vec<String> {
    if values.is_empty() {
        vec![default_chat_model.to_string()]
    } else {
        values.to_vec()
    }
}

#[derive(Clone, Debug, Serialize)]
struct ChatModelTarget {
    provider: String,
    model: String,
}

fn model_targets_from_values(values: &[String]) -> Vec<ChatModelTarget> {
    let mut seen = HashSet::new();
    let mut targets = Vec::new();

    for value in values {
        if let Some((provider, model)) = parse_model_value(value) {
            let key = format!("{provider}|{model}");
            if seen.insert(key) {
                targets.push(ChatModelTarget { provider, model });
            }
        }
    }

    targets
}

fn runtime_label(provider: Option<&str>, model: Option<&str>) -> String {
    if let Some(p) = provider {
        let normalized = p.trim().to_ascii_lowercase();
        if matches!(
            normalized.as_str(),
            "kai-zen" | "kaizen" | "zeroclaw" | "native"
        ) {
            return "Kai-Zen (runtime default)".to_string();
        }
    }

    match (provider, model) {
        (Some(p), Some(m)) => format!("{p} / {m}"),
        (Some(p), None) => p.to_string(),
        (None, Some(m)) => m.to_string(),
        (None, None) => "runtime default".to_string(),
    }
}

fn viewport_size() -> (f64, f64) {
    let size = window().inner_size();
    (size.width as f64, size.height as f64)
}

fn clamp_floating_layout(layout: &mut PanelLayout, viewport_w: f64, viewport_h: f64) {
    let min_x = -layout.width * 0.8;
    let max_x = viewport_w - layout.width * 0.2;
    let min_y = -layout.height * 0.8;
    let max_y = viewport_h - layout.height * 0.2;
    layout.x = layout.x.clamp(min_x, max_x);
    layout.y = layout.y.clamp(min_y, max_y);
}

fn native_detach_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        let enable = match std::env::var("KAIZEN_ENABLE_NATIVE_DETACH") {
            Ok(raw) => {
                let value = raw.trim().to_ascii_lowercase();
                matches!(value.as_str(), "1" | "true" | "yes" | "on")
            }
            Err(_) => false,
        };

        let unsafe_ack = match std::env::var("KAIZEN_NATIVE_DETACH_UNSAFE_ACK") {
            Ok(raw) => {
                let value = raw.trim().to_ascii_lowercase();
                matches!(value.as_str(), "1" | "true" | "yes" | "on")
            }
            Err(_) => false,
        };

        enable && unsafe_ack
    })
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum AgentStatus {
    Idle,
    Active,
    Blocked,
    ReviewPending,
    Done,
}

impl AgentStatus {
    fn label(&self) -> &'static str {
        match self {
            Self::Idle => "Idle",
            Self::Active => "Working",
            Self::Blocked => "Blocked",
            Self::ReviewPending => "Review",
            Self::Done => "Complete",
        }
    }

    fn dot(&self) -> &'static str {
        match self {
            Self::Idle => "#7b8ea3",
            Self::Active => "#4dd88a",
            Self::Blocked => "#e8665a",
            Self::ReviewPending => "#f0c24e",
            Self::Done => "#5cc9a7",
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
struct ApiAgent {
    id: String,
    name: String,
    task_id: String,
    status: AgentStatus,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum GateState {
    Plan,
    Execute,
    Review,
    HumanSmokeTest,
    Deploy,
    Complete,
}

impl GateState {
    fn label(&self) -> &'static str {
        match self {
            Self::Plan => "Planning",
            Self::Execute => "Execution",
            Self::Review => "Review",
            Self::HumanSmokeTest => "Human Check",
            Self::Deploy => "Deployment",
            Self::Complete => "Complete",
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
struct GateConditions {
    plan_defined: bool,
    plan_acknowledged: bool,
    execution_artifacts_present: bool,
    passed_reasoners_test: bool,
    kaizen_review_approved: bool,
    human_smoke_test_passed: bool,
    deploy_validation_passed: bool,
}

#[derive(Clone, Debug, Deserialize)]
struct GateSnapshot {
    current_state: GateState,
    conditions: GateConditions,
}

#[derive(Clone, Debug, Deserialize)]
struct CrystalBallEvent {
    source_actor: String,
    target_actor: String,
    message: String,
}

#[derive(Clone, Debug, Deserialize)]
struct ChatResponse {
    reply: String,
    provider: Option<String>,
    model: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
struct ChatRequest<'a> {
    message: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    agent_id: Option<&'a str>,
    clear_history: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    provider: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    model: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    mode: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    selected_models: Option<Vec<ChatModelTarget>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    wrap_mode: Option<bool>,
}

#[derive(Clone, Debug, Serialize)]
struct SpawnAgentRequest<'a> {
    agent_name: &'a str,
    task_id: &'a str,
    objective: &'a str,
    user_requested: bool,
}

#[derive(Clone, Debug, Deserialize)]
struct ChatHistoryResponse {
    #[allow(dead_code)]
    conversation_key: String,
    messages: Vec<ChatHistoryMessage>,
}

#[derive(Clone, Debug, Deserialize)]
struct ChatHistoryMessage {
    role: String,
    content: String,
}

#[derive(Clone, Debug, Deserialize)]
struct KaizenSettings {
    runtime_engine: String,
    auto_spawn_subagents: bool,
    max_subagents: u32,
    allow_direct_user_to_subagent_chat: bool,
    hard_gates_enabled: bool,
    require_human_smoke_test_before_deploy: bool,
    provider_inference_only: bool,
    credentials_ui_enabled: bool,
    #[serde(default)]
    oauth_ui_enabled: bool,
    agent_name_editable_after_spawn: bool,
    show_only_masked_secrets_in_ui: bool,
    mattermost_url: String,
    mattermost_channel_id: String,
    selected_github_repo: String,
    inference_provider: String,
    inference_model: String,
    inference_max_tokens: u32,
    inference_temperature: f32,
}

#[derive(Clone, Debug, Serialize)]
struct SettingsPatchRequest {
    runtime_engine: Option<String>,
    auto_spawn_subagents: Option<bool>,
    max_subagents: Option<u32>,
    allow_direct_user_to_subagent_chat: Option<bool>,
    hard_gates_enabled: Option<bool>,
    require_human_smoke_test_before_deploy: Option<bool>,
    provider_inference_only: Option<bool>,
    credentials_ui_enabled: Option<bool>,
    oauth_ui_enabled: Option<bool>,
    agent_name_editable_after_spawn: Option<bool>,
    show_only_masked_secrets_in_ui: Option<bool>,
    mattermost_url: Option<String>,
    mattermost_channel_id: Option<String>,
    selected_github_repo: Option<String>,
    inference_provider: Option<String>,
    inference_model: Option<String>,
    inference_max_tokens: Option<u32>,
    inference_temperature: Option<f32>,
}

#[derive(Clone, Debug, Deserialize)]
struct VaultStatus {
    available: bool,
    key_source: String,
    vault_path: String,
    #[allow(dead_code)]
    key_path: Option<String>,
    #[allow(dead_code)]
    bootstrap_created: bool,
    error: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
struct SecretMetadata {
    provider: String,
    configured: bool,
    #[allow(dead_code)]
    key_id: String,
    #[allow(dead_code)]
    created_at: String,
    last_updated: String,
    last4: String,
    #[allow(dead_code)]
    secret_type: String,
}

#[derive(Clone, Debug, Deserialize)]
struct SecretTestResult {
    #[allow(dead_code)]
    provider: String,
    #[allow(dead_code)]
    configured: bool,
    test_passed: bool,
    error: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
struct GitHubStatusResponse {
    authenticated: bool,
    host: String,
    login: Option<String>,
    #[allow(dead_code)]
    token_source: Option<String>,
    #[allow(dead_code)]
    scopes: Vec<String>,
    #[allow(dead_code)]
    git_protocol: Option<String>,
    error: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
struct GitHubRepoSummary {
    name_with_owner: String,
    #[allow(dead_code)]
    is_private: bool,
    #[allow(dead_code)]
    updated_at: String,
    #[allow(dead_code)]
    url: String,
    #[allow(dead_code)]
    viewer_permission: String,
}

#[derive(Clone, Debug, Deserialize)]
struct GitHubReposResponse {
    connected: bool,
    repos: Vec<GitHubRepoSummary>,
    error: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
struct OAuthStatusResponse {
    provider: String,
    supported: bool,
    connected: bool,
    access_token_configured: bool,
    refresh_token_configured: bool,
    message: String,
}

#[derive(Clone, Debug, Deserialize)]
struct GoogleOAuthAccountPublic {
    account_id: String,
    email: Option<String>,
    scope: Option<String>,
    expires_at: Option<String>,
    updated_at: String,
    has_refresh_token: bool,
}

#[derive(Clone, Debug, Deserialize)]
struct GoogleOAuthStatusResponse {
    #[allow(dead_code)]
    provider: String,
    connected: bool,
    account_count: usize,
    accounts: Vec<GoogleOAuthAccountPublic>,
}

#[derive(Clone, Debug, Deserialize)]
struct GoogleOAuthStartResponse {
    #[allow(dead_code)]
    provider: String,
    redirect_url: String,
    state_token: String,
    #[allow(dead_code)]
    redirect_uri: String,
}

#[derive(Clone, Debug, Serialize)]
struct StoreSecretRequest<'a> {
    value: &'a str,
    secret_type: &'a str,
}

#[derive(Clone, Debug)]
struct Dashboard {
    agents: Vec<ApiAgent>,
    gates: GateSnapshot,
    events: Vec<CrystalBallEvent>,
}

#[derive(Clone, Debug)]
struct UiMsg {
    role: String,
    content: String,
}

impl UiMsg {
    fn new(role: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: role.into(),
            content: content.into(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SettingsTab {
    General,
    Workspaces,
    Agents,
    Integrations,
    Models,
    Security,
    Advanced,
}

const SECRET_PROVIDERS: [(&str, &str); 5] = [
    ("openai", "OpenAI"),
    ("anthropic", "Anthropic"),
    ("gemini", "Google Gemini"),
    ("nvidia", "NVIDIA"),
    ("opencode", "OpenCode"),
];

const OAUTH_PROVIDERS: [(&str, &str); 2] = [("openai", "OpenAI/Codex"), ("anthropic", "Anthropic")];

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum PanelMode {
    Docked,
    Floating,
    Detached,
}

impl PanelMode {
    fn label(&self) -> &'static str {
        match self {
            Self::Docked => "Docked",
            Self::Floating => "Floating",
            Self::Detached => "Detached",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
struct PanelLayout {
    mode: PanelMode,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    z: u32,
}

#[derive(Clone, Debug)]
struct DragState {
    panel_key: String,
    mouse_start_x: f64,
    mouse_start_y: f64,
    panel_start_x: f64,
    panel_start_y: f64,
}

#[derive(Clone, Debug)]
struct ResizeState {
    panel_key: String,
    handle: ResizeHandle,
    mouse_start_x: f64,
    mouse_start_y: f64,
    panel_start_x: f64,
    panel_start_y: f64,
    panel_start_w: f64,
    panel_start_h: f64,
}

#[derive(Clone, Copy, Debug)]
enum ResizeHandle {
    North,
    South,
    East,
    West,
    NorthEast,
    NorthWest,
    SouthEast,
    SouthWest,
}

impl ResizeHandle {
    fn class_name(self) -> &'static str {
        match self {
            Self::North => "n",
            Self::South => "s",
            Self::East => "e",
            Self::West => "w",
            Self::NorthEast => "ne",
            Self::NorthWest => "nw",
            Self::SouthEast => "se",
            Self::SouthWest => "sw",
        }
    }

    fn affects_left(self) -> bool {
        matches!(self, Self::West | Self::NorthWest | Self::SouthWest)
    }

    fn affects_right(self) -> bool {
        matches!(self, Self::East | Self::NorthEast | Self::SouthEast)
    }

    fn affects_top(self) -> bool {
        matches!(self, Self::North | Self::NorthEast | Self::NorthWest)
    }

    fn affects_bottom(self) -> bool {
        matches!(self, Self::South | Self::SouthEast | Self::SouthWest)
    }
}

#[derive(Clone, Debug)]
struct PanelCardData {
    key: String,
    title: String,
    subtitle: String,
    agent: Option<ApiAgent>,
    edge_accent: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
struct WorkspaceTile {
    id: String,
    name: String,
    path: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct PersistedWorkspaces {
    paths: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DestructiveAction {
    Stop,
    Clear,
    Remove,
}

impl DestructiveAction {
    fn confirm_label(&self) -> &'static str {
        match self {
            Self::Stop => "Stop agent",
            Self::Clear => "Clear chat",
            Self::Remove => "Remove agent",
        }
    }

    fn confirmation_text(&self, target: &str) -> String {
        match self {
            Self::Stop => format!("Stop {target}? You can message it again later."),
            Self::Clear => format!("Clear chat for {target}? This removes the current messages."),
            Self::Remove => format!("Remove {target}? This cannot be undone."),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PendingPanelAction {
    action: DestructiveAction,
    panel_key: String,
    panel_title: String,
    agent_id: String,
}

#[derive(Props, Clone, PartialEq)]
struct DetachedWindowProps {
    panel_key: String,
    title: String,
    subtitle: String,
    agent_id: Option<String>,
    admin_token: Option<String>,
    default_chat_model: String,
}

fn ui_log_file_path() -> Option<PathBuf> {
    let appdata = std::env::var("APPDATA").ok()?;
    Some(
        PathBuf::from(appdata)
            .join("KaizenMAX")
            .join("ui-dioxus.log"),
    )
}

fn append_ui_diagnostic(message: &str) {
    let Some(path) = ui_log_file_path() else {
        return;
    };

    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "{} {}", now_s(), message);
    }
}

fn install_ui_panic_hook() {
    let default_hook = std::panic::take_hook();

    std::panic::set_hook(Box::new(move |panic_info| {
        let location = panic_info
            .location()
            .map(|loc| format!("{}:{}", loc.file(), loc.line()))
            .unwrap_or_else(|| "unknown".to_string());

        let message = if let Some(s) = panic_info.payload().downcast_ref::<&str>() {
            (*s).to_string()
        } else if let Some(s) = panic_info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "non-string panic payload".to_string()
        };

        let backtrace = std::backtrace::Backtrace::force_capture();
        append_ui_diagnostic(&format!(
            "panic location={location} message={message} backtrace={backtrace}"
        ));

        default_hook(panic_info);
    }));
}

fn main() {
    install_ui_panic_hook();
    append_ui_diagnostic("ui process started");
    dioxus::LaunchBuilder::desktop().launch(App);
}

#[component]
fn App() -> Element {
    let admin_token: Option<String> = std::env::var("ADMIN_API_TOKEN")
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty());

    let mut agents = use_signal(Vec::<ApiAgent>::new);
    let mut gates = use_signal(|| None::<GateSnapshot>);
    let mut events = use_signal(Vec::<CrystalBallEvent>::new);
    let mut info = use_signal(|| Some("Connecting...".to_string()));
    let mut error = use_signal(|| None::<String>);
    let mut drafts = use_signal(HashMap::<String, String>::new);
    let mut messages = use_signal(HashMap::<String, Vec<UiMsg>>::new);
    let mut settings_current = use_signal(|| None::<KaizenSettings>);
    let mut settings_draft = use_signal(|| None::<KaizenSettings>);
    let mut settings_busy = use_signal(|| false);
    let mut vault_status = use_signal(|| None::<VaultStatus>);
    let mut secret_meta = use_signal(HashMap::<String, SecretMetadata>::new);
    let mut secret_inputs = use_signal(HashMap::<String, String>::new);
    let mut secret_feedback = use_signal(HashMap::<String, String>::new);
    let mut gh_status = use_signal(|| None::<GitHubStatusResponse>);
    let mut gh_repos = use_signal(Vec::<GitHubRepoSummary>::new);
    let mut oauth_status = use_signal(HashMap::<String, OAuthStatusResponse>::new);
    let mut google_oauth_status = use_signal(|| None::<GoogleOAuthStatusResponse>);
    let mut google_oauth_redirect = use_signal(|| None::<String>);

    let mut left_open = use_signal(|| true);
    let mut settings_open = use_signal(|| false);
    let mut settings_tab = use_signal(|| SettingsTab::General);

    let mut panel_layouts = use_signal(load_layouts);
    let mut next_z = use_signal(|| 100u32);
    let mut detached_windows = use_signal(HashMap::<String, WeakDesktopContext>::new);
    let mut drag_state = use_signal(|| None::<DragState>);
    let mut resize_state = use_signal(|| None::<ResizeState>);
    let mut sending_by_panel = use_signal(HashMap::<String, bool>::new);
    let mut composer_models_by_panel = use_signal(HashMap::<String, Vec<String>>::new);
    let mut model_picker_open_by_panel = use_signal(HashMap::<String, bool>::new);
    let mut composer_mode_by_panel = use_signal(HashMap::<String, String>::new);
    let mut panel_action_confirm = use_signal(|| None::<PendingPanelAction>);

    let mut workspace_tiles = use_signal(load_workspace_tiles);
    let mut local_workspace_input = use_signal(String::new);
    let mut active_workspace_id = use_signal(|| "kaizen-max".to_string());

    {
        let token = admin_token.clone();
        let mut agents_sig = agents;
        let mut gates_sig = gates;
        let mut events_sig = events;
        let mut info_sig = info;
        let mut error_sig = error;
        let mut messages_sig = messages;
        let mut layouts_sig = panel_layouts;
        let mut z_sig = next_z;
        let mut detached_sig = detached_windows;

        use_future(move || {
            let token = token.clone();
            async move {
                loop {
                    match refresh_dashboard_state(
                        token.clone(),
                        agents_sig,
                        gates_sig,
                        events_sig,
                        messages_sig,
                        layouts_sig,
                        z_sig,
                        detached_sig,
                    )
                    .await
                    {
                        Ok(()) => {
                            info_sig.set(Some("Connected".into()));
                            error_sig.set(None);
                        }
                        Err(_) => {}
                    }
                    sleep(Duration::from_secs(3)).await;
                }
            }
        });
    }

    {
        let token = admin_token.clone();
        let mut settings_current_sig = settings_current;
        let mut settings_draft_sig = settings_draft;
        let mut vault_sig = vault_status;
        let mut secrets_sig = secret_meta;
        let mut gh_status_sig = gh_status;
        let mut gh_repos_sig = gh_repos;
        let mut oauth_sig = oauth_status;
        let mut google_oauth_sig = google_oauth_status;
        use_future(move || {
            let token = token.clone();
            async move {
                loop {
                    let _ = refresh_settings_bundle(
                        token.as_deref(),
                        settings_current_sig,
                        settings_draft_sig,
                        vault_sig,
                        secrets_sig,
                        gh_status_sig,
                        gh_repos_sig,
                        oauth_sig,
                        google_oauth_sig,
                    )
                    .await;

                    sleep(Duration::from_secs(12)).await;
                }
            }
        });
    }

    {
        let mut layouts_sig = panel_layouts;
        let mut z_sig = next_z;
        let mut detached_sig = detached_windows;
        use_future(move || async move {
            loop {
                let closed_keys: Vec<String> = detached_sig
                    .read()
                    .iter()
                    .filter_map(|(key, handle)| {
                        if handle.upgrade().is_none() {
                            Some(key.clone())
                        } else {
                            None
                        }
                    })
                    .collect();

                if !closed_keys.is_empty() {
                    let mut detached_map = detached_sig.write();
                    let mut layouts_map = layouts_sig.read().clone();
                    let mut changed = false;

                    for key in closed_keys {
                        detached_map.remove(&key);
                        if let Some(layout) = layouts_map.get_mut(&key) {
                            if layout.mode == PanelMode::Detached {
                                layout.mode = PanelMode::Floating;
                                layout.width = layout.width.max(MIN_PANEL_WIDTH);
                                layout.height = layout.height.max(MIN_PANEL_HEIGHT);
                                layout.z = bump_z(&mut z_sig);
                                changed = true;
                            }
                        }
                    }

                    drop(detached_map);

                    if changed {
                        layouts_sig.set(layouts_map.clone());
                        persist_layouts(&layouts_map);
                    }
                }

                sleep(Duration::from_millis(700)).await;
            }
        });
    }

    let a_snap = agents.read().clone();
    let g_snap = gates.read().clone();
    let ev_snap = events.read().clone();
    let d_snap = drafts.read().clone();
    let m_snap = messages.read().clone();
    let sending_snap = sending_by_panel.read().clone();
    let composer_models_snap = composer_models_by_panel.read().clone();
    let model_picker_open_snap = model_picker_open_by_panel.read().clone();
    let composer_mode_snap = composer_mode_by_panel.read().clone();
    let layouts_snap = panel_layouts.read().clone();
    let settings_snap = settings_draft.read().clone();
    let settings_is_saving = *settings_busy.read();
    let vault_snap = vault_status.read().clone();
    let secret_meta_snap = secret_meta.read().clone();
    let secret_inputs_snap = secret_inputs.read().clone();
    let secret_feedback_snap = secret_feedback.read().clone();
    let gh_status_snap = gh_status.read().clone();
    let gh_repos_snap = gh_repos.read().clone();
    let oauth_snap = oauth_status.read().clone();
    let google_oauth_snap = google_oauth_status.read().clone();
    let google_oauth_redirect_snap = google_oauth_redirect.read().clone();
    let selected_repo = settings_snap
        .as_ref()
        .map(|s| s.selected_github_repo.clone())
        .unwrap_or_default();
    let default_chat_model = settings_snap
        .as_ref()
        .map(|s| model_value(&s.inference_provider, &s.inference_model))
        .unwrap_or_else(|| model_value("openai", "gpt-5.3-codex"));

    let gate_label = g_snap
        .as_ref()
        .map(|g| g.current_state.label())
        .unwrap_or("Planning");
    let sidebar_open = *left_open.read();
    let show_settings = *settings_open.read();
    let active_tab = *settings_tab.read();
    let workspaces_snap = workspace_tiles.read().clone();
    let local_workspace_input_value = local_workspace_input.read().clone();
    let pending_panel_action = panel_action_confirm.read().clone();
    let active_workspace = active_workspace_id.read().clone();
    let active_workspace_label = workspaces_snap
        .iter()
        .find(|ws| ws.id == active_workspace)
        .map(|ws| ws.name.clone())
        .unwrap_or_else(|| active_workspace.clone());
    let profile_name = std::env::var("USERNAME").unwrap_or_else(|_| "Local Operator".to_string());
    let profile_device =
        std::env::var("COMPUTERNAME").unwrap_or_else(|_| "This Device".to_string());
    let profile_initials = profile_name
        .split_whitespace()
        .filter_map(|part| part.chars().next())
        .take(2)
        .collect::<String>()
        .to_uppercase();
    let profile_badge = if profile_initials.is_empty() {
        "OP".to_string()
    } else {
        profile_initials.clone()
    };
    let interaction_active = drag_state.read().is_some() || resize_state.read().is_some();
    let shell_class = if interaction_active {
        "shell is-dragging"
    } else {
        "shell"
    };

    let mut all_panels: Vec<PanelCardData> = vec![PanelCardData {
        key: "kaizen".to_string(),
        title: "Kaizen".to_string(),
        subtitle: format!("Primary planner | {gate_label}"),
        agent: None,
        edge_accent: false,
    }];
    for (idx, agent) in a_snap.iter().enumerate() {
        all_panels.push(PanelCardData {
            key: agent.id.clone(),
            title: agent.name.clone(),
            subtitle: format!("{} | {}", agent.task_id, agent.status.label()),
            agent: Some(agent.clone()),
            edge_accent: idx == 0,
        });
    }

    let mut docked_panels: Vec<(PanelCardData, PanelLayout)> = Vec::new();
    let mut floating_panels: Vec<(PanelCardData, PanelLayout)> = Vec::new();

    for (idx, panel) in all_panels.iter().enumerate() {
        let layout = layouts_snap
            .get(&panel.key)
            .cloned()
            .unwrap_or_else(|| default_panel_layout(idx));

        match layout.mode {
            PanelMode::Docked => docked_panels.push((panel.clone(), layout)),
            PanelMode::Floating => floating_panels.push((panel.clone(), layout)),
            PanelMode::Detached => {}
        }
    }

    floating_panels.sort_by_key(|(_, layout)| layout.z);

    rsx! {
        style { {CSS} }

        div {
            class: "{shell_class}",
            onmousemove: {
                let mut drag_sig = drag_state;
                let mut resize_sig = resize_state;
                let mut layouts_sig = panel_layouts;
                move |evt: Event<MouseData>| {
                    let point = evt.data().client_coordinates();
                    let (viewport_w, viewport_h) = viewport_size();

                    if let Some(dragging) = drag_sig.read().clone() {
                        let dx = point.x - dragging.mouse_start_x;
                        let dy = point.y - dragging.mouse_start_y;
                        let mut layouts_map = layouts_sig.read().clone();
                        if let Some(layout) = layouts_map.get_mut(&dragging.panel_key) {
                            layout.x = dragging.panel_start_x + dx;
                            layout.y = dragging.panel_start_y + dy;
                            clamp_floating_layout(layout, viewport_w, viewport_h);
                        }
                        layouts_sig.set(layouts_map);
                    } else if let Some(resizing) = resize_sig.read().clone() {
                        let dx = point.x - resizing.mouse_start_x;
                        let dy = point.y - resizing.mouse_start_y;
                        let mut layouts_map = layouts_sig.read().clone();
                        if let Some(layout) = layouts_map.get_mut(&resizing.panel_key) {
                            if resizing.handle.affects_right() {
                                let max_width = (viewport_w - layout.x).max(MIN_PANEL_WIDTH);
                                layout.width =
                                    (resizing.panel_start_w + dx).clamp(MIN_PANEL_WIDTH, max_width);
                            }

                            if resizing.handle.affects_left() {
                                let max_x =
                                    (resizing.panel_start_x + resizing.panel_start_w - MIN_PANEL_WIDTH)
                                        .max(MIN_PANEL_X);
                                let next_x = (resizing.panel_start_x + dx).clamp(MIN_PANEL_X, max_x);
                                let moved = next_x - resizing.panel_start_x;
                                layout.x = next_x;
                                layout.width = (resizing.panel_start_w - moved).max(MIN_PANEL_WIDTH);
                            }

                            if resizing.handle.affects_bottom() {
                                let max_height = (viewport_h - layout.y).max(MIN_PANEL_HEIGHT);
                                layout.height =
                                    (resizing.panel_start_h + dy).clamp(MIN_PANEL_HEIGHT, max_height);
                            }

                            if resizing.handle.affects_top() {
                                let max_y =
                                    (resizing.panel_start_y + resizing.panel_start_h - MIN_PANEL_HEIGHT)
                                        .max(MIN_PANEL_Y);
                                let next_y = (resizing.panel_start_y + dy).clamp(MIN_PANEL_Y, max_y);
                                let moved = next_y - resizing.panel_start_y;
                                layout.y = next_y;
                                layout.height = (resizing.panel_start_h - moved).max(MIN_PANEL_HEIGHT);
                            }

                            clamp_floating_layout(layout, viewport_w, viewport_h);
                        }
                        layouts_sig.set(layouts_map);
                    }
                }
            },
            onmouseup: {
                let mut drag_sig = drag_state;
                let mut resize_sig = resize_state;
                let mut layouts_sig = panel_layouts;
                move |_| {
                    let was_dragging = drag_sig.read().is_some();
                    let was_resizing = resize_sig.read().is_some();
                    if was_dragging || was_resizing {
                        drag_sig.set(None);
                        resize_sig.set(None);
                        persist_layouts(&layouts_sig.read());
                    }
                }
            },
            header { class: "top-bar",
                div { class: "top-left",
                    button { class: "toggle-btn",
                        onclick: move |_| {
                            let v = *left_open.read();
                            left_open.set(!v);
                        },
                        if sidebar_open { "<" } else { ">" }
                    }
                    div { class: "brand-lockup",
                        div { class: "brand-mark" }
                        div { class: "brand-text",
                            h1 { class: "logo", "Kaizen MAX" }
                            span { class: "brand-sub", "Agent Workspace" }
                        }
                    }
                    span { class: "top-meta", "{gate_label} | {a_snap.len()} agents | {ev_snap.len()} events" }
                }
                div { class: "top-right",
                    button {
                        class: "btn btn-accent",
                        title: "Advance workflow through Plan -> Execute -> Review -> Human Smoke Test -> Deploy -> Complete",
                        onclick: {
                            let token = admin_token.clone();
                            let mut info_sig = info;
                            let mut error_sig = error;
                            let mut agents_sig = agents;
                            let mut gates_sig = gates;
                            let mut events_sig = events;
                            let mut messages_sig = messages;
                            let mut layouts_sig = panel_layouts;
                            let mut z_sig = next_z;
                            let mut detached_sig = detached_windows;
                            move |_| {
                                let token = token.clone();
                                spawn(async move {
                                    match advance_gate(token.as_deref()).await {
                                        Ok(()) => {
                                            info_sig.set(Some("Workflow advanced".into()));
                                            error_sig.set(None);
                                        }
                                        Err(e) => {
                                            error_sig.set(Some(e));
                                            return;
                                        }
                                    }

                                    if let Err(e) = refresh_dashboard_state(
                                        token,
                                        agents_sig,
                                        gates_sig,
                                        events_sig,
                                        messages_sig,
                                        layouts_sig,
                                        z_sig,
                                        detached_sig,
                                    )
                                    .await
                                    {
                                        error_sig.set(Some(e));
                                    }
                                });
                            }
                        },
                        "Advance Workflow"
                    }
                }
            }

            if let Some(e) = error.read().as_ref() {
                div { class: "banner err", "{e}" }
            }
            if let Some(i) = info.read().as_ref() {
                div { class: "banner ok", "{i}" }
            }

            div { class: "body",
                if sidebar_open {
                    aside { class: "left",
                        div { class: "sb-head",
                            h3 { class: "sb-title", "Workspace Hub" }
                        }
                        div { class: "sb-section",
                            h4 { class: "sb-label", "Local Workspaces" }
                            p { class: "sb-hint", "Attach a local folder to switch context quickly." }
                            div { class: "workspace-add-row",
                                input {
                                    class: "s-input workspace-path-input",
                                    r#type: "text",
                                    value: "{local_workspace_input_value}",
                                    placeholder: "C:\\projects\\my-workspace",
                                    oninput: {
                                        let mut local_workspace_input = local_workspace_input;
                                        move |e: Event<FormData>| {
                                            local_workspace_input.set(e.value());
                                        }
                                    },
                                }
                                button {
                                    class: "btn btn-sm btn-sec",
                                    onclick: {
                                        let mut workspace_tiles = workspace_tiles;
                                        let mut active_workspace_id = active_workspace_id;
                                        let mut local_workspace_input = local_workspace_input;
                                        let mut info_sig = info;
                                        let mut error_sig = error;
                                        move |_| {
                                            let path = local_workspace_input.read().trim().to_string();
                                            if path.is_empty() {
                                                error_sig.set(Some("Enter a local path first".to_string()));
                                                return;
                                            }

                                            let mut tiles = workspace_tiles.read().clone();
                                            if let Some(existing) = tiles
                                                .iter()
                                                .find(|tile| tile.path.as_deref() == Some(path.as_str()))
                                            {
                                                active_workspace_id.set(existing.id.clone());
                                                local_workspace_input.set(String::new());
                                                info_sig.set(Some("Workspace already attached".to_string()));
                                                error_sig.set(None);
                                                return;
                                            }

                                            let tile = workspace_tile_from_path(&path);
                                            active_workspace_id.set(tile.id.clone());
                                            tiles.push(tile);
                                            workspace_tiles.set(tiles.clone());
                                            persist_workspace_tiles(&tiles);
                                            local_workspace_input.set(String::new());
                                            info_sig.set(Some("Workspace attached".to_string()));
                                            error_sig.set(None);
                                        }
                                    },
                                    "Attach"
                                }
                            }
                            div { class: "workspace-taskbar",
                                for ws in workspaces_snap.iter() {
                                    button {
                                        class: if ws.id == active_workspace { "ws-pill ws-pill-active" } else { "ws-pill" },
                                        onclick: {
                                            let id = ws.id.clone();
                                            let mut active_workspace_id = active_workspace_id;
                                            move |_| {
                                                active_workspace_id.set(id.clone());
                                            }
                                        },
                                        span { class: "ws-pill-name", "{ws.name}" }
                                        if let Some(path) = ws.path.as_ref() {
                                            span { class: "ws-pill-meta", "{path}" }
                                        } else {
                                            span { class: "ws-pill-meta", "{a_snap.len()} agents" }
                                        }
                                    }
                                }
                            }
                        }
                        div { class: "sb-spacer" }
                        div { class: "sb-bottom-stack",
                        div { class: "sb-section git-section",
                            h4 { class: "sb-label", "GitHub" }
                            if let Some(status) = gh_status_snap.as_ref() {
                                if status.authenticated {
                                    p { class: "sb-hint", {format!(
                                        "Connected as {} on {}",
                                        status
                                            .login
                                            .clone()
                                            .unwrap_or_else(|| "unknown".to_string()),
                                        status.host
                                    )} }
                                } else {
                                    p { class: "sb-hint", "Not authenticated. Run gh auth login in terminal." }
                                }
                                if let Some(err) = status.error.as_ref() {
                                    p { class: "sb-hint", "Status: {err}" }
                                }
                            } else {
                                p { class: "sb-hint", "Checking GitHub connection..." }
                            }

                            if !gh_repos_snap.is_empty() {
                                label { class: "sb-hint", "Repository" }
                                select {
                                    class: "s-input",
                                    value: "{selected_repo}",
                                    onchange: {
                                        let token = admin_token.clone();
                                        let mut settings_draft_sig = settings_draft;
                                        let mut settings_current_sig = settings_current;
                                        let mut info_sig = info;
                                        let mut error_sig = error;
                                        move |e: Event<FormData>| {
                                            let repo = e.value();
                                            let current_draft = settings_draft_sig.read().clone();
                                            if let Some(mut draft) = current_draft {
                                                draft.selected_github_repo = repo.clone();
                                                settings_draft_sig.set(Some(draft.clone()));
                                                settings_current_sig.set(Some(draft));
                                            }

                                            let token = token.clone();
                                            spawn(async move {
                                                match patch_selected_repo_api(&repo, token.as_deref()).await {
                                                    Ok(_) => {
                                                        info_sig.set(Some("Selected repo saved".to_string()));
                                                        error_sig.set(None);
                                                    }
                                                    Err(err) => error_sig.set(Some(err)),
                                                }
                                            });
                                        }
                                    },
                                    option { value: "", "Select repository" }
                                    for repo in gh_repos_snap.iter() {
                                        option { value: "{repo.name_with_owner}", "{repo.name_with_owner}" }
                                    }
                                }
                            } else {
                                p { class: "sb-hint", "No repositories returned yet." }
                            }

                            button {
                                class: "btn btn-sm btn-sec",
                                onclick: {
                                    let token = admin_token.clone();
                                    let mut settings_current_sig = settings_current;
                                    let mut settings_draft_sig = settings_draft;
                                    let mut vault_sig = vault_status;
                                    let mut secrets_sig = secret_meta;
                                    let mut gh_status_sig = gh_status;
                                    let mut gh_repos_sig = gh_repos;
                                    let mut oauth_sig = oauth_status;
                                    let mut google_oauth_sig = google_oauth_status;
                                    let mut settings_open_sig = settings_open;
                                    move |_| {
                                        settings_open_sig.set(true);
                                        let token = token.clone();
                                        spawn(async move {
                                            let _ = refresh_settings_bundle(
                                                token.as_deref(),
                                                settings_current_sig,
                                                settings_draft_sig,
                                                vault_sig,
                                                secrets_sig,
                                                gh_status_sig,
                                                gh_repos_sig,
                                                oauth_sig,
                                                google_oauth_sig,
                                            ).await;
                                        });
                                    }
                                },
                                "Settings"
                            }
                        }
                        div { class: "account-hero",
                            div { class: "account-top",
                                div { class: "account-avatar", "{profile_badge}" }
                                div { class: "account-meta",
                                    strong { class: "account-name", "{profile_name}" }
                                    p { class: "account-sub", "Personal Local Profile" }
                                }
                            }
                            div { class: "account-chip-row",
                                span { class: "account-chip", "{profile_device}" }
                                span { class: "account-chip", "Workspace: {active_workspace_label}" }
                            }
                            button {
                                class: "btn btn-sm btn-sec account-open-btn",
                                onclick: move |_| { settings_open.set(true); },
                                "Open profile settings"
                            }
                        }
                        }
                    }
                }

                main { class: "center",
                    div { class: "grid",
                        for (panel, layout) in docked_panels.iter() {
                            {card(
                                &panel.key,
                                &panel.title,
                                &panel.subtitle,
                                panel.agent.as_ref(),
                                panel.edge_accent,
                                layout.mode,
                                &m_snap,
                                &sending_snap,
                                &composer_models_snap,
                                &model_picker_open_snap,
                                &composer_mode_snap,
                                &d_snap,
                                &mut drafts,
                                &mut messages,
                                &mut sending_by_panel,
                                &mut composer_models_by_panel,
                                &mut model_picker_open_by_panel,
                                &mut composer_mode_by_panel,
                                &mut info,
                                &mut error,
                                &default_chat_model,
                                &admin_token,
                                &mut agents,
                                &mut panel_layouts,
                                &mut next_z,
                                &mut detached_windows,
                                &mut drag_state,
                                &mut panel_action_confirm,
                            )}
                        }

                        div {
                            class: "card card-add",
                            onclick: {
                                let token = admin_token.clone();
                                let mut agents_sig = agents;
                                let mut gates_sig = gates;
                                let mut events_sig = events;
                                let mut messages_sig = messages;
                                let mut layouts_sig = panel_layouts;
                                let mut z_sig = next_z;
                                let mut detached_sig = detached_windows;
                                let mut info_sig = info;
                                let mut error_sig = error;
                                move |_| {
                                    let token = token.clone();
                                    let n = agents_sig.read().len() + 1;
                                    let name = format!("Agent-{n}");
                                    let tid = format!("task-{:03}", now_s() % 1000);
                                    let obj = format!("{name} handles assigned tasks.");

                                    spawn(async move {
                                        match spawn_agent_api(&name, &tid, &obj, token.as_deref()).await {
                                            Ok(created) => {
                                                info_sig.set(Some(format!("Spawned {}", created.name)));
                                                error_sig.set(None);
                                            }
                                            Err(e) => {
                                                error_sig.set(Some(e));
                                                return;
                                            }
                                        }

                                        if let Err(e) = refresh_dashboard_state(
                                            token,
                                            agents_sig,
                                            gates_sig,
                                            events_sig,
                                            messages_sig,
                                            layouts_sig,
                                            z_sig,
                                            detached_sig,
                                        )
                                        .await
                                        {
                                            error_sig.set(Some(e));
                                        }
                                    });
                                }
                            },
                            div { class: "add-inner",
                                span { class: "add-icon", "+" }
                                span { "Add Agent" }
                            }
                        }
                    }
                }

                aside { class: "right",
                    div { class: "overview-head",
                        h3 { class: "sb-title", "Workspace Overview" }
                        p { class: "sb-sub", "Monitor agent progress, workflow phase, and recent orchestration activity." }
                    }

                    div { class: "sb-section",
                        h3 { class: "sb-title", "Agents" }
                        p { class: "sb-sub", "Live status for active and idle agents in this workspace." }
                        if a_snap.is_empty() {
                            p { class: "sb-hint", "No agents yet. Click + to spawn." }
                        }
                        for agent in a_snap.iter() {
                            div { class: "agent-row",
                                span { class: "a-dot", style: "background:{agent.status.dot()};" }
                                div { class: "a-info",
                                    strong { "{agent.name}" }
                                    span { class: "a-meta", "{agent.task_id} | {agent.status.label()}" }
                                }
                            }
                        }
                    }

                    div { class: "sb-section",
                        h3 { class: "sb-title", "Workflow" }
                        p { class: "sb-sub", "Current gate phase and completion checklist for release readiness." }
                        if let Some(ref g) = g_snap {
                            p { class: "wf-step", "Step: {g.current_state.label()}" }
                            {checklist(&g.conditions)}
                        }
                    }

                    div { class: "sb-section",
                        h3 { class: "sb-title", "Activity" }
                        p { class: "sb-sub", "Recent events emitted by Kaizen and sub-agent operations." }
                        if ev_snap.is_empty() {
                            p { class: "sb-hint", "No events yet." }
                        }
                        for ev in ev_snap.iter().rev().take(8) {
                            div { class: "ev-row",
                                span { class: "ev-actors", "{ev.source_actor} -> {ev.target_actor}" }
                                span { class: "ev-msg", "{ev.message}" }
                            }
                        }
                    }
                }
            }

            div { class: "floating-layer",
                if interaction_active {
                    div { class: "drag-scrim" }
                }
                for (panel, layout) in floating_panels.iter() {
                    div {
                        class: "floating-frame",
                        key: "float-{panel.key}",
                        style: "left:{layout.x}px;top:{layout.y}px;width:{layout.width}px;height:{layout.height}px;z-index:{layout.z};",
                        onmousedown: {
                            let k = panel.key.clone();
                            let mut z_sig = next_z;
                            let mut layouts_sig = panel_layouts;
                            move |_| {
                                let mut map = layouts_sig.read().clone();
                                if let Some(existing) = map.get_mut(&k) {
                                    existing.z = bump_z(&mut z_sig);
                                    layouts_sig.set(map);
                                }
                            }
                        },
                        {card(
                            &panel.key,
                            &panel.title,
                            &panel.subtitle,
                            panel.agent.as_ref(),
                            panel.edge_accent,
                            PanelMode::Floating,
                            &m_snap,
                            &sending_snap,
                            &composer_models_snap,
                            &model_picker_open_snap,
                            &composer_mode_snap,
                            &d_snap,
                            &mut drafts,
                            &mut messages,
                            &mut sending_by_panel,
                            &mut composer_models_by_panel,
                            &mut model_picker_open_by_panel,
                            &mut composer_mode_by_panel,
                            &mut info,
                            &mut error,
                            &default_chat_model,
                            &admin_token,
                            &mut agents,
                            &mut panel_layouts,
                            &mut next_z,
                            &mut detached_windows,
                            &mut drag_state,
                            &mut panel_action_confirm,
                        )}
                        for handle in [
                            ResizeHandle::North,
                            ResizeHandle::South,
                            ResizeHandle::East,
                            ResizeHandle::West,
                            ResizeHandle::NorthEast,
                            ResizeHandle::NorthWest,
                            ResizeHandle::SouthEast,
                            ResizeHandle::SouthWest,
                        ] {
                            div {
                                class: "floating-resize-handle floating-resize-{handle.class_name()}",
                                onmousedown: {
                                    let k = panel.key.clone();
                                    let l = layout.clone();
                                    let mut resize_sig = resize_state;
                                    let mut z_sig = next_z;
                                    let mut layouts_sig = panel_layouts;
                                    move |evt: Event<MouseData>| {
                                        evt.stop_propagation();
                                        let point = evt.data().client_coordinates();
                                        let mut map = layouts_sig.read().clone();
                                        if let Some(existing) = map.get_mut(&k) {
                                            existing.z = bump_z(&mut z_sig);
                                        }
                                        layouts_sig.set(map);
                                        resize_sig.set(Some(ResizeState {
                                            panel_key: k.clone(),
                                            handle,
                                            mouse_start_x: point.x,
                                            mouse_start_y: point.y,
                                            panel_start_x: l.x,
                                            panel_start_y: l.y,
                                            panel_start_w: l.width,
                                            panel_start_h: l.height,
                                        }));
                                    }
                                }
                            }
                        }
                    }
                }
            }

            if show_settings {
                div {
                    class: "modal-overlay",
                    onclick: move |_| { settings_open.set(false); },
                    div {
                        class: "modal",
                        onclick: move |e: Event<MouseData>| { e.stop_propagation(); },
                        div { class: "modal-header",
                            h2 { "Settings" }
                            div { class: "modal-actions",
                                button {
                                    class: "btn btn-accent btn-sm",
                                    disabled: settings_snap.is_none() || settings_is_saving,
                                    onclick: {
                                        let token = admin_token.clone();
                                        let mut settings_draft_sig = settings_draft;
                                        let mut settings_current_sig = settings_current;
                                        let mut settings_busy_sig = settings_busy;
                                        let mut vault_sig = vault_status;
                                        let mut secrets_sig = secret_meta;
                                        let mut gh_status_sig = gh_status;
                                        let mut gh_repos_sig = gh_repos;
                                        let mut oauth_sig = oauth_status;
                                        let mut google_oauth_sig = google_oauth_status;
                                        let mut info_sig = info;
                                        let mut error_sig = error;
                                        move |_| {
                                            let Some(draft) = settings_draft_sig.read().clone() else {
                                                error_sig.set(Some("Settings are not loaded yet".to_string()));
                                                return;
                                            };

                                            settings_busy_sig.set(true);
                                            let token = token.clone();
                                            spawn(async move {
                                                match patch_settings_api(&draft, token.as_deref()).await {
                                                    Ok(updated) => {
                                                        settings_current_sig.set(Some(updated.clone()));
                                                        settings_draft_sig.set(Some(updated));
                                                        info_sig.set(Some("Settings saved".to_string()));
                                                        error_sig.set(None);
                                                        let _ = refresh_settings_bundle(
                                                            token.as_deref(),
                                                            settings_current_sig,
                                                            settings_draft_sig,
                                                            vault_sig,
                                                            secrets_sig,
                                                            gh_status_sig,
                                                            gh_repos_sig,
                                                            oauth_sig,
                                                            google_oauth_sig,
                                                        ).await;
                                                    }
                                                    Err(err) => error_sig.set(Some(err)),
                                                }
                                                settings_busy_sig.set(false);
                                            });
                                        }
                                    },
                                    if settings_is_saving { "Saving..." } else { "Save Settings" }
                                }
                                button {
                                    class: "modal-close",
                                    onclick: move |_| { settings_open.set(false); },
                                    "X"
                                }
                            }
                        }
                        div { class: "modal-body",
                            nav { class: "tabs",
                                {tab_btn("General", SettingsTab::General, active_tab, &mut settings_tab)}
                                {tab_btn("Workspaces", SettingsTab::Workspaces, active_tab, &mut settings_tab)}
                                {tab_btn("Agents", SettingsTab::Agents, active_tab, &mut settings_tab)}
                                {tab_btn("Integrations", SettingsTab::Integrations, active_tab, &mut settings_tab)}
                                {tab_btn("Models", SettingsTab::Models, active_tab, &mut settings_tab)}
                                {tab_btn("Security", SettingsTab::Security, active_tab, &mut settings_tab)}
                                {tab_btn("Advanced", SettingsTab::Advanced, active_tab, &mut settings_tab)}
                            }
                            div { class: "tab-content",
                                if let Some(cfg) = settings_snap.as_ref() {
                                    match active_tab {
                                        SettingsTab::General => rsx! {
                                            h3 { "General Settings" }
                                            p { class: "sb-hint", "Edit runtime defaults, then click Save Settings." }
                                            div { class: "setting-row",
                                                label { "Runtime Engine" }
                                                select {
                                                    class: "s-input",
                                                    value: "{cfg.runtime_engine}",
                                                    onchange: {
                                                        let mut settings_draft_sig = settings_draft;
                                                        move |e: Event<FormData>| {
                                                            let current_draft = settings_draft_sig.read().clone();
                                                            if let Some(mut draft) = current_draft {
                                                                draft.runtime_engine = e.value();
                                                                settings_draft_sig.set(Some(draft));
                                                            }
                                                        }
                                                    },
                                                    option { value: "zeroclaw", "ZeroClaw" }
                                                    option { value: "openclaw_compat", "OpenClaw Compatibility" }
                                                }
                                            }

                                            if cfg.oauth_ui_enabled {
                                                div { class: "setting-row oauth-row",
                                                strong { "Google OAuth (WebKeys)" }
                                                if let Some(status) = google_oauth_snap.clone() {
                                                    p { class: "sb-hint", {format!("Connected accounts: {}", status.account_count)} }
                                                    p { class: "sb-hint", {format!("Overall connected: {}", if status.connected { "yes" } else { "no" })} }
                                                } else {
                                                    p { class: "sb-hint", "Google OAuth status loading..." }
                                                }

                                                if let Some(url) = google_oauth_redirect_snap.clone() {
                                                    p { class: "sb-hint", "Open this URL to complete Google OAuth login:" }
                                                    a {
                                                        class: "sb-link",
                                                        href: "{url}",
                                                        target: "_blank",
                                                        rel: "noopener noreferrer",
                                                        "{url}"
                                                    }
                                                }

                                                div { class: "inline-actions",
                                                    button {
                                                        class: "btn btn-sm btn-sec",
                                                        onclick: {
                                                            let token = admin_token.clone();
                                                            let mut info_sig = info;
                                                            let mut error_sig = error;
                                                            let mut google_redirect_sig = google_oauth_redirect;
                                                            move |_| {
                                                                let token = token.clone();
                                                                spawn(async move {
                                                                    match google_oauth_start_api(token.as_deref()).await {
                                                                        Ok(start) => {
                                                                            google_redirect_sig.set(Some(start.redirect_url.clone()));
                                                                            info_sig.set(Some(format!(
                                                                                "Google OAuth start ready (state {}). Open the URL to continue.",
                                                                                start.state_token
                                                                            )));
                                                                            error_sig.set(None);
                                                                        }
                                                                        Err(err) => error_sig.set(Some(err)),
                                                                    }
                                                                });
                                                            }
                                                        },
                                                        "Connect Google"
                                                    }
                                                    button {
                                                        class: "btn btn-sm btn-sec",
                                                        onclick: {
                                                            let token = admin_token.clone();
                                                            let mut info_sig = info;
                                                            let mut error_sig = error;
                                                            let mut google_oauth_sig = google_oauth_status;
                                                            move |_| {
                                                                let token = token.clone();
                                                                spawn(async move {
                                                                    match fetch_google_oauth_status_api(token.as_deref()).await {
                                                                        Ok(status) => {
                                                                            let count = status.account_count;
                                                                            google_oauth_sig.set(Some(status));
                                                                            info_sig.set(Some(format!("Refreshed Google OAuth status ({} accounts)", count)));
                                                                            error_sig.set(None);
                                                                        }
                                                                        Err(err) => error_sig.set(Some(err)),
                                                                    }
                                                                });
                                                            }
                                                        },
                                                        "Refresh Accounts"
                                                    }
                                                }

                                                if let Some(status) = google_oauth_snap.clone() {
                                                    for account in status.accounts {
                                                        div { class: "oauth-account-card",
                                                            strong {
                                                                "{account.email.clone().unwrap_or_else(|| account.account_id.clone())}"
                                                            }
                                                            p {
                                                                class: "sb-hint",
                                                                "Account ID: {account.account_id}"
                                                            }
                                                            p {
                                                                class: "sb-hint",
                                                                {format!(
                                                                    "Scope: {} | Refresh token: {}",
                                                                    account.scope.clone().unwrap_or_else(|| "(unknown)".to_string()),
                                                                    if account.has_refresh_token { "yes" } else { "no" }
                                                                )}
                                                            }
                                                            if let Some(exp) = account.expires_at.clone() {
                                                                p { class: "sb-hint", "Token expires: {exp}" }
                                                            }
                                                            p { class: "sb-hint", "Updated: {account.updated_at}" }
                                                            div { class: "inline-actions",
                                                                button {
                                                                    class: "btn btn-sm btn-danger",
                                                                    onclick: {
                                                                        let account_id = account.account_id.clone();
                                                                        let token = admin_token.clone();
                                                                        let mut info_sig = info;
                                                                        let mut error_sig = error;
                                                                        let mut google_oauth_sig = google_oauth_status;
                                                                        move |_| {
                                                                            let account_id = account_id.clone();
                                                                            let token = token.clone();
                                                                            spawn(async move {
                                                                                match google_oauth_disconnect_account_api(&account_id, token.as_deref()).await {
                                                                                    Ok(()) => {
                                                                                        info_sig.set(Some(format!("Disconnected Google account {}", account_id)));
                                                                                        error_sig.set(None);
                                                                                        match fetch_google_oauth_status_api(token.as_deref()).await {
                                                                                            Ok(status) => google_oauth_sig.set(Some(status)),
                                                                                            Err(err) => error_sig.set(Some(err)),
                                                                                        }
                                                                                    }
                                                                                    Err(err) => error_sig.set(Some(err)),
                                                                                }
                                                                            });
                                                                        }
                                                                    },
                                                                    "Disconnect"
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                                }
                                            }
                                        },
                                        SettingsTab::Workspaces => rsx! {
                                            h3 { "Workspace and GitHub" }
                                            p { class: "sb-hint", "Select the connected repository used by Workspace Hub." }
                                            div { class: "setting-row",
                                                label { "Selected Repository" }
                                                select {
                                                    class: "s-input",
                                                    value: "{cfg.selected_github_repo}",
                                                    onchange: {
                                                        let mut settings_draft_sig = settings_draft;
                                                        move |e: Event<FormData>| {
                                                            let current_draft = settings_draft_sig.read().clone();
                                                            if let Some(mut draft) = current_draft {
                                                                draft.selected_github_repo = e.value();
                                                                settings_draft_sig.set(Some(draft));
                                                            }
                                                        }
                                                    },
                                                    option { value: "", "Select repository" }
                                                    for repo in gh_repos_snap.iter() {
                                                        option { value: "{repo.name_with_owner}", "{repo.name_with_owner}" }
                                                    }
                                                }
                                            }
                                        },
                                        SettingsTab::Agents => rsx! {
                                            h3 { "Agent and Workflow" }
                                            p { class: "sb-hint", "Configure sub-agent limits and interaction policy." }
                                            div { class: "setting-row",
                                                label { "Max Sub-Agents" }
                                                input {
                                                    class: "s-input",
                                                    r#type: "number",
                                                    min: "1",
                                                    max: "20",
                                                    value: "{cfg.max_subagents}",
                                                    oninput: {
                                                        let mut settings_draft_sig = settings_draft;
                                                        move |e: Event<FormData>| {
                                                            if let Ok(v) = e.value().parse::<u32>() {
                                                                let current_draft = settings_draft_sig.read().clone();
                                                                if let Some(mut draft) = current_draft {
                                                                    draft.max_subagents = v.clamp(1, 20);
                                                                    settings_draft_sig.set(Some(draft));
                                                                }
                                                            }
                                                        }
                                                    },
                                                }

                                                label { "Auto-Spawn Sub-Agents" }
                                                select {
                                                    class: "s-input",
                                                    value: "{bool_to_str(cfg.auto_spawn_subagents)}",
                                                    onchange: {
                                                        let mut settings_draft_sig = settings_draft;
                                                        move |e: Event<FormData>| {
                                                            let current_draft = settings_draft_sig.read().clone();
                                                            if let Some(mut draft) = current_draft {
                                                                draft.auto_spawn_subagents = parse_bool(&e.value());
                                                                settings_draft_sig.set(Some(draft));
                                                            }
                                                        }
                                                    },
                                                    option { value: "false", "Disabled" }
                                                    option { value: "true", "Enabled" }
                                                }

                                                label { "Direct User -> Sub-Agent Chat" }
                                                select {
                                                    class: "s-input",
                                                    value: "{bool_to_str(cfg.allow_direct_user_to_subagent_chat)}",
                                                    onchange: {
                                                        let mut settings_draft_sig = settings_draft;
                                                        move |e: Event<FormData>| {
                                                            let current_draft = settings_draft_sig.read().clone();
                                                            if let Some(mut draft) = current_draft {
                                                                draft.allow_direct_user_to_subagent_chat = parse_bool(&e.value());
                                                                settings_draft_sig.set(Some(draft));
                                                            }
                                                        }
                                                    },
                                                    option { value: "true", "Enabled" }
                                                    option { value: "false", "Disabled" }
                                                }

                                                label { "Rename Agents After Spawn" }
                                                select {
                                                    class: "s-input",
                                                    value: "{bool_to_str(cfg.agent_name_editable_after_spawn)}",
                                                    onchange: {
                                                        let mut settings_draft_sig = settings_draft;
                                                        move |e: Event<FormData>| {
                                                            let current_draft = settings_draft_sig.read().clone();
                                                            if let Some(mut draft) = current_draft {
                                                                draft.agent_name_editable_after_spawn = parse_bool(&e.value());
                                                                settings_draft_sig.set(Some(draft));
                                                            }
                                                        }
                                                    },
                                                    option { value: "true", "Enabled" }
                                                    option { value: "false", "Disabled" }
                                                }
                                            }
                                        },
                                        SettingsTab::Integrations => rsx! {
                                            h3 { "Integrations and OAuth" }
                                            p { class: "sb-hint", "OAuth controls: OpenAI/Codex + Anthropic (legacy), and Google OAuth multi-account for WebKeys/Gemini browser auth." }
                                            div { class: "setting-row",
                                                label { "Mattermost URL" }
                                                input {
                                                    class: "s-input",
                                                    r#type: "text",
                                                    value: "{cfg.mattermost_url}",
                                                    placeholder: "https://mattermost.example.com",
                                                    oninput: {
                                                        let mut settings_draft_sig = settings_draft;
                                                        move |e: Event<FormData>| {
                                                            let current_draft = settings_draft_sig.read().clone();
                                                            if let Some(mut draft) = current_draft {
                                                                draft.mattermost_url = e.value();
                                                                settings_draft_sig.set(Some(draft));
                                                            }
                                                        }
                                                    },
                                                }

                                                label { "Mattermost Channel ID" }
                                                input {
                                                    class: "s-input",
                                                    r#type: "text",
                                                    value: "{cfg.mattermost_channel_id}",
                                                    oninput: {
                                                        let mut settings_draft_sig = settings_draft;
                                                        move |e: Event<FormData>| {
                                                            let current_draft = settings_draft_sig.read().clone();
                                                            if let Some(mut draft) = current_draft {
                                                                draft.mattermost_channel_id = e.value();
                                                                settings_draft_sig.set(Some(draft));
                                                            }
                                                        }
                                                    },
                                                }
                                            }

                                            if cfg.oauth_ui_enabled {
                                                for (provider, label) in OAUTH_PROVIDERS.iter() {
                                                    div { class: "setting-row oauth-row",
                                                    strong { "{label}" }
                                                    if let Some(status) = oauth_snap.get(*provider) {
                                                        p { class: "sb-hint", "{status.message}" }
                                                        p { class: "sb-hint", {format!("Connected: {}", if status.connected { "yes" } else { "no" })} }
                                                    } else {
                                                        p { class: "sb-hint", "OAuth status loading..." }
                                                    }
                                                    div { class: "inline-actions",
                                                        button {
                                                            class: "btn btn-sm btn-sec",
                                                            onclick: {
                                                                let provider = provider.to_string();
                                                                let token = admin_token.clone();
                                                                let mut info_sig = info;
                                                                let mut error_sig = error;
                                                                move |_| {
                                                                    let provider = provider.clone();
                                                                    let token = token.clone();
                                                                    spawn(async move {
                                                                        match oauth_start_api(&provider, token.as_deref()).await {
                                                                            Ok(msg) => {
                                                                                info_sig.set(Some(msg));
                                                                                error_sig.set(None);
                                                                            }
                                                                            Err(err) => error_sig.set(Some(err)),
                                                                        }
                                                                    });
                                                                }
                                                            },
                                                            "Connect"
                                                        }
                                                        button {
                                                            class: "btn btn-sm btn-sec",
                                                            onclick: {
                                                                let provider = provider.to_string();
                                                                let token = admin_token.clone();
                                                                let mut info_sig = info;
                                                                let mut error_sig = error;
                                                                move |_| {
                                                                    let provider = provider.clone();
                                                                    let token = token.clone();
                                                                    spawn(async move {
                                                                        match oauth_refresh_api(&provider, token.as_deref()).await {
                                                                            Ok(msg) => {
                                                                                info_sig.set(Some(msg));
                                                                                error_sig.set(None);
                                                                            }
                                                                            Err(err) => error_sig.set(Some(err)),
                                                                        }
                                                                    });
                                                                }
                                                            },
                                                            "Refresh"
                                                        }
                                                        button {
                                                            class: "btn btn-sm btn-danger",
                                                            onclick: {
                                                                let provider = provider.to_string();
                                                                let token = admin_token.clone();
                                                                let mut info_sig = info;
                                                                let mut error_sig = error;
                                                                let mut oauth_sig = oauth_status;
                                                                move |_| {
                                                                    let provider = provider.clone();
                                                                    let token = token.clone();
                                                                    spawn(async move {
                                                                        match oauth_disconnect_api(&provider, token.as_deref()).await {
                                                                            Ok(()) => {
                                                                                info_sig.set(Some(format!("{} OAuth disconnected", provider)));
                                                                                error_sig.set(None);
                                                                                oauth_sig.write().remove(&provider);
                                                                            }
                                                                            Err(err) => error_sig.set(Some(err)),
                                                                        }
                                                                    });
                                                                }
                                                            },
                                                            "Disconnect"
                                                        }
                                                    }
                                                    }
                                                }
                                            }
                                        },
                                        SettingsTab::Models => rsx! {
                                            h3 { "Models and Providers" }
                                            p { class: "sb-hint", "Choose inference provider and model defaults." }
                                            div { class: "setting-row",
                                                label { "Inference Provider" }
                                                select {
                                                    class: "s-input",
                                                    value: "{cfg.inference_provider}",
                                                    onchange: {
                                                        let mut settings_draft_sig = settings_draft;
                                                        move |e: Event<FormData>| {
                                                            let current_draft = settings_draft_sig.read().clone();
                                                            if let Some(mut draft) = current_draft {
                                                                draft.inference_provider = e.value();
                                                                settings_draft_sig.set(Some(draft));
                                                            }
                                                        }
                                                    },
                                                    option { value: "anthropic", "Anthropic" }
                                                    option { value: "openai", "OpenAI" }
                                                    option { value: "gemini", "Google Gemini" }
                                                    option { value: "gemini-cli", "Gemini CLI (OAuth)" }
                                                    option { value: "nvidia", "NVIDIA" }
                                                }

                                                label { "Model" }
                                                input {
                                                    class: "s-input",
                                                    r#type: "text",
                                                    value: "{cfg.inference_model}",
                                                    oninput: {
                                                        let mut settings_draft_sig = settings_draft;
                                                        move |e: Event<FormData>| {
                                                            let current_draft = settings_draft_sig.read().clone();
                                                            if let Some(mut draft) = current_draft {
                                                                draft.inference_model = e.value();
                                                                settings_draft_sig.set(Some(draft));
                                                            }
                                                        }
                                                    },
                                                }

                                                if cfg.inference_provider == "nvidia" {
                                                    p { class: "sb-hint", "NVIDIA presets" }
                                                    div { class: "inline-actions",
                                                        button { class: "btn btn-sm btn-sec", onclick: {
                                                            let mut settings_draft_sig = settings_draft;
                                                            move |_| {
                                                                let current_draft = settings_draft_sig.read().clone();
                                                                if let Some(mut draft) = current_draft {
                                                                    draft.inference_model = "nvidia/llama-3.3-nemotron-super-49b-v1".to_string();
                                                                    settings_draft_sig.set(Some(draft));
                                                                }
                                                            }
                                                        }, "Nemotron 49B" }
                                                        button { class: "btn btn-sm btn-sec", onclick: {
                                                            let mut settings_draft_sig = settings_draft;
                                                            move |_| {
                                                                let current_draft = settings_draft_sig.read().clone();
                                                                if let Some(mut draft) = current_draft {
                                                                    draft.inference_model = "meta/llama-3.1-70b-instruct".to_string();
                                                                    settings_draft_sig.set(Some(draft));
                                                                }
                                                            }
                                                        }, "Llama 3.1 70B" }
                                                        button { class: "btn btn-sm btn-sec", onclick: {
                                                            let mut settings_draft_sig = settings_draft;
                                                            move |_| {
                                                                let current_draft = settings_draft_sig.read().clone();
                                                                if let Some(mut draft) = current_draft {
                                                                    draft.inference_model = "mistralai/mixtral-8x7b-instruct-v0.1".to_string();
                                                                    settings_draft_sig.set(Some(draft));
                                                                }
                                                            }
                                                        }, "Mixtral 8x7B" }
                                                    }
                                                }

                                                label { "Max Tokens" }
                                                input {
                                                    class: "s-input",
                                                    r#type: "number",
                                                    min: "256",
                                                    max: "32768",
                                                    value: "{cfg.inference_max_tokens}",
                                                    oninput: {
                                                        let mut settings_draft_sig = settings_draft;
                                                        move |e: Event<FormData>| {
                                                            if let Ok(v) = e.value().parse::<u32>() {
                                                                let current_draft = settings_draft_sig.read().clone();
                                                                if let Some(mut draft) = current_draft {
                                                                    draft.inference_max_tokens = v.clamp(256, 32768);
                                                                    settings_draft_sig.set(Some(draft));
                                                                }
                                                            }
                                                        }
                                                    },
                                                }

                                                label { "Temperature" }
                                                input {
                                                    class: "s-input",
                                                    r#type: "number",
                                                    step: "0.1",
                                                    min: "0",
                                                    max: "1",
                                                    value: "{cfg.inference_temperature}",
                                                    oninput: {
                                                        let mut settings_draft_sig = settings_draft;
                                                        move |e: Event<FormData>| {
                                                            if let Ok(v) = e.value().parse::<f32>() {
                                                                let current_draft = settings_draft_sig.read().clone();
                                                                if let Some(mut draft) = current_draft {
                                                                    draft.inference_temperature = v.clamp(0.0, 1.0);
                                                                    settings_draft_sig.set(Some(draft));
                                                                }
                                                            }
                                                        }
                                                    },
                                                }
                                            }
                                        },
                                        SettingsTab::Security => rsx! {
                                            h3 { "Security and Secrets" }
                                            if let Some(vault) = vault_snap.as_ref() {
                                                p { class: "sb-hint", {format!(
                                                    "Vault: {} ({})",
                                                    if vault.available { "available" } else { "unavailable" },
                                                    vault.key_source
                                                )} }
                                                p { class: "sb-hint", "Path: {vault.vault_path}" }
                                                if let Some(err) = vault.error.as_ref() {
                                                    p { class: "sb-hint", "Vault error: {err}" }
                                                }
                                            } else {
                                                p { class: "sb-hint", "Vault status loading..." }
                                            }

                                            for (provider, label) in SECRET_PROVIDERS.iter() {
                                                div { class: "setting-row secret-row",
                                                    strong { "{label}" }
                                                    if let Some(meta) = secret_meta_snap.get(*provider) {
                                                        p { class: "sb-hint", {format!(
                                                            "Configured: {} | last4: {} | updated: {}",
                                                            if meta.configured { "yes" } else { "no" },
                                                            meta.last4,
                                                            meta.last_updated
                                                        )} }
                                                    } else {
                                                        p { class: "sb-hint", "Not configured" }
                                                    }

                                                    input {
                                                        class: "s-input",
                                                        r#type: "password",
                                                        placeholder: "Enter API key or token",
                                                        value: "{secret_inputs_snap.get(*provider).cloned().unwrap_or_default()}",
                                                        oninput: {
                                                            let provider = provider.to_string();
                                                            let mut inputs_sig = secret_inputs;
                                                            move |e: Event<FormData>| {
                                                                inputs_sig.write().insert(provider.clone(), e.value());
                                                            }
                                                        },
                                                    }

                                                    if let Some(msg) = secret_feedback_snap.get(*provider) {
                                                        p { class: "sb-hint", "{msg}" }
                                                    }

                                                    div { class: "inline-actions",
                                                        button {
                                                            class: "btn btn-sm btn-sec",
                                                            onclick: {
                                                                let provider = provider.to_string();
                                                                let token = admin_token.clone();
                                                                let mut inputs_sig = secret_inputs;
                                                                let mut feedback_sig = secret_feedback;
                                                                let mut info_sig = info;
                                                                let mut error_sig = error;
                                                                let mut settings_current_sig = settings_current;
                                                                let mut settings_draft_sig = settings_draft;
                                                                let mut vault_sig = vault_status;
                                                                let mut secrets_sig = secret_meta;
                                                                let mut gh_status_sig = gh_status;
                                                                let mut gh_repos_sig = gh_repos;
                                                                let mut oauth_sig = oauth_status;
                                                                let mut google_oauth_sig = google_oauth_status;
                                                                move |_| {
                                                                    let provider = provider.clone();
                                                                    let value = inputs_sig.read().get(&provider).cloned().unwrap_or_default();
                                                                    if value.trim().is_empty() {
                                                                        error_sig.set(Some(format!("{} key is empty", provider)));
                                                                        return;
                                                                    }

                                                                    let token = token.clone();
                                                                    spawn(async move {
                                                                        match store_secret_api(&provider, &value, token.as_deref()).await {
                                                                            Ok(_) => {
                                                                                feedback_sig.write().insert(provider.clone(), "Credential saved".to_string());
                                                                                info_sig.set(Some(format!("{} credential saved", provider)));
                                                                                error_sig.set(None);
                                                                                let _ = refresh_settings_bundle(
                                                                                    token.as_deref(),
                                                                                    settings_current_sig,
                                                                                    settings_draft_sig,
                                                                                    vault_sig,
                                                                                    secrets_sig,
                                                                                    gh_status_sig,
                                                                                    gh_repos_sig,
                                                                                    oauth_sig,
                                                                                    google_oauth_sig,
                                                                                ).await;
                                                                            }
                                                                            Err(err) => error_sig.set(Some(err)),
                                                                        }
                                                                    });
                                                                }
                                                            },
                                                            "Save/Update"
                                                        }
                                                        button {
                                                            class: "btn btn-sm btn-sec",
                                                            onclick: {
                                                                let provider = provider.to_string();
                                                                let token = admin_token.clone();
                                                                let mut feedback_sig = secret_feedback;
                                                                let mut error_sig = error;
                                                                move |_| {
                                                                    let provider = provider.clone();
                                                                    let token = token.clone();
                                                                    spawn(async move {
                                                                        match test_secret_api(&provider, token.as_deref()).await {
                                                                            Ok(result) => {
                                                                                let msg = if result.test_passed {
                                                                                    "Test passed".to_string()
                                                                                } else {
                                                                                    format!("Test failed: {}", result.error.unwrap_or_else(|| "unknown error".to_string()))
                                                                                };
                                                                                feedback_sig.write().insert(provider.clone(), msg);
                                                                            }
                                                                            Err(err) => error_sig.set(Some(err)),
                                                                        }
                                                                    });
                                                                }
                                                            },
                                                            "Test"
                                                        }
                                                        button {
                                                            class: "btn btn-sm btn-danger",
                                                            onclick: {
                                                                let provider = provider.to_string();
                                                                let token = admin_token.clone();
                                                                let mut feedback_sig = secret_feedback;
                                                                let mut inputs_sig = secret_inputs;
                                                                let mut info_sig = info;
                                                                let mut error_sig = error;
                                                                let mut settings_current_sig = settings_current;
                                                                let mut settings_draft_sig = settings_draft;
                                                                let mut vault_sig = vault_status;
                                                                let mut secrets_sig = secret_meta;
                                                                let mut gh_status_sig = gh_status;
                                                                let mut gh_repos_sig = gh_repos;
                                                                let mut oauth_sig = oauth_status;
                                                                let mut google_oauth_sig = google_oauth_status;
                                                                move |_| {
                                                                    let provider = provider.clone();
                                                                    let token = token.clone();
                                                                    spawn(async move {
                                                                        match revoke_secret_api(&provider, token.as_deref()).await {
                                                                            Ok(()) => {
                                                                                feedback_sig.write().insert(provider.clone(), "Credential revoked".to_string());
                                                                                inputs_sig.write().remove(&provider);
                                                                                info_sig.set(Some(format!("{} credential revoked", provider)));
                                                                                error_sig.set(None);
                                                                                let _ = refresh_settings_bundle(
                                                                                    token.as_deref(),
                                                                                    settings_current_sig,
                                                                                    settings_draft_sig,
                                                                                    vault_sig,
                                                                                    secrets_sig,
                                                                                    gh_status_sig,
                                                                                    gh_repos_sig,
                                                                                    oauth_sig,
                                                                                    google_oauth_sig,
                                                                                ).await;
                                                                            }
                                                                            Err(err) => error_sig.set(Some(err)),
                                                                        }
                                                                    });
                                                                }
                                                            },
                                                            "Revoke"
                                                        }
                                                    }
                                                }

                                                if cfg.inference_provider == "gemini-cli" {
                                                    p { class: "sb-hint", "Gemini CLI uses local OAuth via the `gemini` executable (no API key in vault)." }
                                                    div { class: "inline-actions",
                                                        button { class: "btn btn-sm btn-sec", onclick: {
                                                            let mut settings_draft_sig = settings_draft;
                                                            move |_| {
                                                                let current_draft = settings_draft_sig.read().clone();
                                                                if let Some(mut draft) = current_draft {
                                                                    draft.inference_model = "gemini-2.5-flash".to_string();
                                                                    settings_draft_sig.set(Some(draft));
                                                                }
                                                            }
                                                        }, "2.5 Flash" }
                                                        button { class: "btn btn-sm btn-sec", onclick: {
                                                            let mut settings_draft_sig = settings_draft;
                                                            move |_| {
                                                                let current_draft = settings_draft_sig.read().clone();
                                                                if let Some(mut draft) = current_draft {
                                                                    draft.inference_model = "gemini-2.5-pro".to_string();
                                                                    settings_draft_sig.set(Some(draft));
                                                                }
                                                            }
                                                        }, "2.5 Pro" }
                                                    }
                                                }
                                            }
                                        },
                                        SettingsTab::Advanced => rsx! {
                                            h3 { "Advanced" }
                                            p { class: "sb-hint", "Security posture and gate behavior controls." }
                                            div { class: "setting-row",
                                                label { "Hard Gates Enabled" }
                                                select {
                                                    class: "s-input",
                                                    value: "{bool_to_str(cfg.hard_gates_enabled)}",
                                                    onchange: {
                                                        let mut settings_draft_sig = settings_draft;
                                                        move |e: Event<FormData>| {
                                                            let current_draft = settings_draft_sig.read().clone();
                                                            if let Some(mut draft) = current_draft {
                                                                draft.hard_gates_enabled = parse_bool(&e.value());
                                                                settings_draft_sig.set(Some(draft));
                                                            }
                                                        }
                                                    },
                                                    option { value: "true", "Enabled" }
                                                    option { value: "false", "Disabled" }
                                                }

                                                label { "Require Human Smoke Test" }
                                                select {
                                                    class: "s-input",
                                                    value: "{bool_to_str(cfg.require_human_smoke_test_before_deploy)}",
                                                    onchange: {
                                                        let mut settings_draft_sig = settings_draft;
                                                        move |e: Event<FormData>| {
                                                            let current_draft = settings_draft_sig.read().clone();
                                                            if let Some(mut draft) = current_draft {
                                                                draft.require_human_smoke_test_before_deploy = parse_bool(&e.value());
                                                                settings_draft_sig.set(Some(draft));
                                                            }
                                                        }
                                                    },
                                                    option { value: "true", "Enabled" }
                                                    option { value: "false", "Disabled" }
                                                }

                                                label { "Provider Inference Only" }
                                                select {
                                                    class: "s-input",
                                                    value: "{bool_to_str(cfg.provider_inference_only)}",
                                                    onchange: {
                                                        let mut settings_draft_sig = settings_draft;
                                                        move |e: Event<FormData>| {
                                                            let current_draft = settings_draft_sig.read().clone();
                                                            if let Some(mut draft) = current_draft {
                                                                draft.provider_inference_only = parse_bool(&e.value());
                                                                settings_draft_sig.set(Some(draft));
                                                            }
                                                        }
                                                    },
                                                    option { value: "true", "Enabled" }
                                                    option { value: "false", "Disabled" }
                                                }

                                                label { "Credentials UI Enabled" }
                                                select {
                                                    class: "s-input",
                                                    value: "{bool_to_str(cfg.credentials_ui_enabled)}",
                                                    onchange: {
                                                        let mut settings_draft_sig = settings_draft;
                                                        move |e: Event<FormData>| {
                                                            let current_draft = settings_draft_sig.read().clone();
                                                            if let Some(mut draft) = current_draft {
                                                                draft.credentials_ui_enabled = parse_bool(&e.value());
                                                                settings_draft_sig.set(Some(draft));
                                                            }
                                                        }
                                                    },
                                                    option { value: "true", "Enabled" }
                                                    option { value: "false", "Disabled" }
                                                }

                                                label { "Show Only Masked Secrets" }
                                                select {
                                                    class: "s-input",
                                                    value: "{bool_to_str(cfg.show_only_masked_secrets_in_ui)}",
                                                    onchange: {
                                                        let mut settings_draft_sig = settings_draft;
                                                        move |e: Event<FormData>| {
                                                            let current_draft = settings_draft_sig.read().clone();
                                                            if let Some(mut draft) = current_draft {
                                                                draft.show_only_masked_secrets_in_ui = parse_bool(&e.value());
                                                                settings_draft_sig.set(Some(draft));
                                                            }
                                                        }
                                                    },
                                                    option { value: "true", "Enabled" }
                                                    option { value: "false", "Disabled" }
                                                }
                                            }
                                        },
                                    }
                                } else {
                                    h3 { "Loading settings..." }
                                    p { class: "sb-hint", "Waiting for /api/settings response." }
                                }
                            }
                        }
                    }
                }
            }

            if let Some(pending) = pending_panel_action.as_ref() {
                div {
                    class: "modal-overlay",
                    onclick: {
                        let mut panel_action_confirm = panel_action_confirm;
                        move |_| {
                            panel_action_confirm.set(None);
                        }
                    },
                    div {
                        class: "confirm-modal",
                        onclick: move |e: Event<MouseData>| { e.stop_propagation(); },
                        h3 { class: "confirm-title", "Confirm {pending.action.confirm_label()}" }
                        p { class: "sb-hint", "{pending.action.confirmation_text(&pending.panel_title)}" }
                        div { class: "confirm-actions",
                            button {
                                class: "btn btn-sm btn-sec",
                                onclick: {
                                    let mut panel_action_confirm = panel_action_confirm;
                                    move |_| {
                                        panel_action_confirm.set(None);
                                    }
                                },
                                "Cancel"
                            }
                            button {
                                class: if pending.action == DestructiveAction::Remove {
                                    "btn btn-sm btn-danger"
                                } else {
                                    "btn btn-sm btn-warn"
                                },
                                onclick: {
                                    let action_template = pending.clone();
                                    let token = admin_token.clone();
                                    let mut panel_action_confirm = panel_action_confirm;
                                    let mut info_sig = info;
                                    let mut error_sig = error;
                                    let mut messages_sig = messages;
                                    let mut agents_sig = agents;
                                    let mut gates_sig = gates;
                                    let mut events_sig = events;
                                    let mut layouts_sig = panel_layouts;
                                    let mut z_sig = next_z;
                                    let mut detached_sig = detached_windows;
                                    move |_| {
                                        let action = action_template.clone();
                                        panel_action_confirm.set(None);
                                        let token = token.clone();
                                        spawn(async move {
                                            match action.action {
                                                DestructiveAction::Stop => {
                                                    match stop_agent_api(&action.agent_id, token.as_deref()).await {
                                                        Ok(_) => {
                                                            info_sig.set(Some(format!("{} stopped", action.panel_title)));
                                                            error_sig.set(None);
                                                        }
                                                        Err(e) => {
                                                            error_sig.set(Some(e));
                                                            return;
                                                        }
                                                    }

                                                    if let Err(e) = refresh_dashboard_state(
                                                        token,
                                                        agents_sig,
                                                        gates_sig,
                                                        events_sig,
                                                        messages_sig,
                                                        layouts_sig,
                                                        z_sig,
                                                        detached_sig,
                                                    )
                                                    .await
                                                    {
                                                        error_sig.set(Some(e));
                                                    }
                                                }
                                                DestructiveAction::Clear => {
                                                    messages_sig
                                                        .write()
                                                        .insert(action.panel_key.clone(), Vec::new());
                                                    match clear_agent_api(&action.agent_id, token.as_deref()).await {
                                                        Ok(()) => {
                                                            info_sig.set(Some(format!("Chat cleared for {}", action.panel_title)));
                                                            error_sig.set(None);
                                                        }
                                                        Err(e) => error_sig.set(Some(e)),
                                                    }
                                                }
                                                DestructiveAction::Remove => {
                                                    match remove_agent_api(&action.agent_id, token.as_deref()).await {
                                                        Ok(()) => {
                                                            info_sig.set(Some(format!("{} removed", action.panel_title)));
                                                            error_sig.set(None);
                                                        }
                                                        Err(e) => {
                                                            error_sig.set(Some(e));
                                                            return;
                                                        }
                                                    }

                                                    if let Err(e) = refresh_dashboard_state(
                                                        token,
                                                        agents_sig,
                                                        gates_sig,
                                                        events_sig,
                                                        messages_sig,
                                                        layouts_sig,
                                                        z_sig,
                                                        detached_sig,
                                                    )
                                                    .await
                                                    {
                                                        error_sig.set(Some(e));
                                                    }
                                                }
                                            }
                                        });
                                    }
                                },
                                "Confirm"
                            }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn DetachedWindow(props: DetachedWindowProps) -> Element {
    let mut history = use_signal(Vec::<UiMsg>::new);
    let mut draft = use_signal(String::new);
    let mut info = use_signal(|| None::<String>);
    let mut error = use_signal(|| None::<String>);
    let mut history_busy = use_signal(|| false);
    let mut confirm_action = use_signal(|| None::<DestructiveAction>);

    {
        let token = props.admin_token.clone();
        let agent_id = props.agent_id.clone();
        let panel_key = props.panel_key.clone();
        let mut history_sig = history;
        let mut error_sig = error;
        let mut busy_sig = history_busy;
        use_future(move || {
            let token = token.clone();
            let agent_id = agent_id.clone();
            let panel_key = panel_key.clone();
            async move {
                append_ui_diagnostic(&format!("detached window mounted key={panel_key}"));
                let _ = refresh_detached_history(agent_id, token, history_sig, error_sig, busy_sig)
                    .await;
            }
        });
    }

    let msg_snap = history.read().clone();
    let draft_val = draft.read().clone();
    let title = props.title.clone();
    let subtitle = props.subtitle.clone();
    let aid = props.agent_id.clone();
    let token = props.admin_token.clone();
    let panel_key = props.panel_key.clone();
    let is_history_busy = *history_busy.read();
    let can_submit = !is_history_busy && !draft_val.trim().is_empty();
    let mut composer_models = use_signal(Vec::<String>::new);
    let mut model_picker_open = use_signal(|| false);
    let mut composer_mode = use_signal(|| {
        if aid.is_some() {
            "build".to_string()
        } else {
            "orchestrator".to_string()
        }
    });
    let composer_models_snap = composer_models.read().clone();
    let selected_model_values = normalized_model_values(Some(&composer_models_snap));
    let selected_model_values_with_default =
        model_values_with_default_fallback(&selected_model_values, &props.default_chat_model);
    let selected_targets = model_targets_from_values(&selected_model_values_with_default);
    let selected_provider_model = selected_targets
        .first()
        .map(|target| (target.provider.clone(), target.model.clone()));
    let wrap_mode_enabled = selected_targets.len() > 1;
    let selected_runtime_label = if wrap_mode_enabled {
        format!("wrap x{} models", selected_targets.len())
    } else {
        selected_provider_model
            .as_ref()
            .map(|(p, m)| runtime_label(Some(p.as_str()), Some(m.as_str())))
            .unwrap_or_else(|| "runtime default".to_string())
    };
    let selected_models_payload = if wrap_mode_enabled {
        Some(selected_targets.clone())
    } else {
        None
    };
    let wrap_mode_payload = if wrap_mode_enabled { Some(true) } else { None };
    let selected_mode = composer_mode.read().clone();
    let mode_options = if aid.is_some() {
        &SUBAGENT_CHAT_MODES[..]
    } else {
        &KAIZEN_CHAT_MODES[..]
    };

    rsx! {
        style { {CSS} }
        div { class: "detached-shell",
            div { class: "detached-head",
                div { class: "detached-title-wrap",
                    h2 { class: "detached-title", "{title}" }
                    p { class: "detached-sub", "{subtitle}" }
                }
                div { class: "detached-actions",
                    button {
                        class: "btn btn-xs btn-sec",
                        disabled: is_history_busy,
                        onclick: {
                            let aid = aid.clone();
                            let token = token.clone();
                            let mut info_sig = info;
                            let mut error_sig = error;
                            let mut history_sig = history;
                            let mut busy_sig = history_busy;
                            move |_| {
                                let aid = aid.clone();
                                let token = token.clone();
                                spawn(async move {
                                    match refresh_detached_history(
                                        aid,
                                        token,
                                        history_sig,
                                        error_sig,
                                        busy_sig,
                                    )
                                    .await
                                    {
                                        Ok(()) => info_sig.set(Some("Detached view refreshed".into())),
                                        Err(_) => info_sig.set(None),
                                    }
                                });
                            }
                        },
                        if is_history_busy { "Refreshing..." } else { "Refresh" }
                    }

                    if aid.is_some() {
                        button {
                            class: "btn btn-xs btn-warn",
                            onclick: {
                                let mut confirm_action = confirm_action;
                                move |_| {
                                    confirm_action.set(Some(DestructiveAction::Stop));
                                }
                            },
                            "Stop"
                        }
                        button {
                            class: "btn btn-xs btn-sec",
                            onclick: {
                                let mut confirm_action = confirm_action;
                                move |_| {
                                    confirm_action.set(Some(DestructiveAction::Clear));
                                }
                            },
                            "Clear"
                        }
                        button {
                            class: "btn btn-xs btn-danger",
                            onclick: {
                                let mut confirm_action = confirm_action;
                                move |_| {
                                    confirm_action.set(Some(DestructiveAction::Remove));
                                }
                            },
                            "Remove"
                        }
                    }
                    button {
                        class: "btn btn-xs btn-sec",
                        onclick: {
                            let panel_key = panel_key.clone();
                            move |_| {
                                append_ui_diagnostic(&format!(
                                    "detached window reattach key={panel_key}"
                                ));
                                window().close();
                            }
                        },
                        "Reattach"
                    }
                }
            }

            if let Some(e) = error.read().as_ref() {
                div { class: "banner err", "{e}" }
            }
            if let Some(i) = info.read().as_ref() {
                div { class: "banner ok", "{i}" }
            }

            if let Some(action) = confirm_action.read().as_ref() {
                div { class: "confirm-inline",
                    strong { class: "confirm-inline-title", "Confirm {action.confirm_label()}" }
                    p { class: "sb-hint", "{action.confirmation_text(&title)}" }
                    div { class: "confirm-actions",
                        button {
                            class: "btn btn-sm btn-sec",
                            onclick: {
                                let mut confirm_action = confirm_action;
                                move |_| {
                                    confirm_action.set(None);
                                }
                            },
                            "Cancel"
                        }
                        button {
                            class: if *action == DestructiveAction::Remove {
                                "btn btn-sm btn-danger"
                            } else {
                                "btn btn-sm btn-warn"
                            },
                            onclick: {
                                let action = *action;
                                let aid = aid.clone();
                                let token = token.clone();
                                let mut confirm_action = confirm_action;
                                let mut info_sig = info;
                                let mut error_sig = error;
                                let mut history_sig = history;
                                move |_| {
                                    confirm_action.set(None);
                                    if let Some(id) = aid.clone() {
                                        let token = token.clone();
                                        spawn(async move {
                                            match action {
                                                DestructiveAction::Stop => {
                                                    match stop_agent_api(&id, token.as_deref()).await {
                                                        Ok(_) => {
                                                            info_sig.set(Some("Agent stopped".into()));
                                                            error_sig.set(None);
                                                        }
                                                        Err(e) => error_sig.set(Some(e)),
                                                    }
                                                }
                                                DestructiveAction::Clear => {
                                                    history_sig.set(Vec::new());
                                                    match clear_agent_api(&id, token.as_deref()).await {
                                                        Ok(()) => {
                                                            info_sig.set(Some("Chat cleared".into()));
                                                            error_sig.set(None);
                                                        }
                                                        Err(e) => error_sig.set(Some(e)),
                                                    }
                                                }
                                                DestructiveAction::Remove => {
                                                    match remove_agent_api(&id, token.as_deref()).await {
                                                        Ok(()) => window().close(),
                                                        Err(e) => error_sig.set(Some(e)),
                                                    }
                                                }
                                            }
                                        });
                                    }
                                }
                            },
                            "Confirm"
                        }
                    }
                }
            }

            div { class: "detached-stream",
                if msg_snap.is_empty() {
                    p { class: "stream-empty", "No messages yet." }
                }
                for line in msg_snap.iter().rev().take(80).rev() {
                    div {
                        class: if line.role == "user" { "msg msg-you" } else { "msg msg-ai" },
                        span {
                            class: "msg-role",
                            if line.role == "user" {
                                "You"
                            } else if line.role == "assistant" {
                                "{title}"
                            } else {
                                "{line.role}"
                            }
                        }
                        span { class: "msg-txt", "{line.content}" }
                    }
                }
            }

            div { class: "composer-toolbar",
                div { class: "model-picker-summary",
                    if wrap_mode_enabled {
                        "Wrap mode on ({selected_targets.len()} models)"
                    } else {
                        "Model: {selected_runtime_label}"
                    }
                }
                if *model_picker_open.read() {
                    div {
                        class: "model-picker-popover",
                        p { class: "model-picker-title", "Model selection" }
                        p { class: "model-picker-hint", "Select one for direct mode, or multiple for wrap mode." }
                        for (provider, label, model) in CHAT_MODEL_PRESETS {
                            label {
                                class: "model-picker-option",
                                input {
                                    r#type: "checkbox",
                                    checked: selected_model_values_with_default
                                        .iter()
                                        .any(|entry| entry == &model_value(provider, model)),
                                    onclick: {
                                        let provider = provider.to_string();
                                        let model = model.to_string();
                                        let mut composer_models = composer_models;
                                        let default_model_value = props.default_chat_model.clone();
                                        move |_| {
                                            let preset_value = model_value(&provider, &model);
                                            let explicit_selected = {
                                                let current = composer_models.read().clone();
                                                normalized_model_values(Some(&current))
                                            };
                                            let mut effective_selected = model_values_with_default_fallback(
                                                &explicit_selected,
                                                &default_model_value,
                                            );

                                            if let Some(idx) = effective_selected
                                                .iter()
                                                .position(|entry| entry == &preset_value)
                                            {
                                                if effective_selected.len() > 1 {
                                                    effective_selected.remove(idx);
                                                }
                                            } else {
                                                effective_selected.push(preset_value);
                                            }

                                            let updated_selected =
                                                normalized_model_values(Some(&effective_selected));
                                            if updated_selected.is_empty()
                                                || (updated_selected.len() == 1
                                                    && updated_selected[0] == default_model_value)
                                            {
                                                composer_models.set(Vec::new());
                                            } else {
                                                composer_models.set(updated_selected);
                                            }
                                        }
                                    },
                                }
                                span { "{label}" }
                            }
                        }
                    }
                }
                button {
                    class: if *model_picker_open.read() {
                        "btn btn-xs btn-sec btn-model-picker-open"
                    } else {
                        "btn btn-xs btn-sec"
                    },
                    title: "Open model picker",
                    onclick: {
                        let mut model_picker_open = model_picker_open;
                        move |_| {
                            let is_open = *model_picker_open.read();
                            model_picker_open.set(!is_open);
                        }
                    },
                    if *model_picker_open.read() { "Close picker" } else { "Models" }
                }
                div { class: "comp-mode-row",
                    for mode_name in mode_options.iter() {
                        button {
                            class: if *mode_name == selected_mode { "comp-mode-chip comp-mode-chip-active" } else { "comp-mode-chip" },
                            onclick: {
                                let mode_name = mode_name.to_string();
                                move |_| composer_mode.set(mode_name.clone())
                            },
                            "{mode_name}"
                        }
                    }
                }
            }

            div { class: "composer",
                textarea {
                    class: "comp-in",
                    rows: "1",
                    disabled: is_history_busy,
                    value: draft_val,
                    placeholder: "Message your agent...",
                    oninput: move |e: Event<FormData>| draft.set(e.value()),
                    onkeydown: {
                        let aid = aid.clone();
                        let token = token.clone();
                        let mut draft_sig = draft;
                        let mut history_sig = history;
                        let mut info_sig = info;
                        let mut error_sig = error;
                        let mut busy_sig = history_busy;
                        let title_for_line = title.clone();
                        let selected_provider_model = selected_provider_model.clone();
                        let selected_models_payload = selected_models_payload.clone();
                        let wrap_mode_payload = wrap_mode_payload;
                        let selected_mode = selected_mode.clone();
                        let selected_runtime_label = selected_runtime_label.clone();
                        move |evt: Event<KeyboardData>| {
                            if evt.key().to_string() != "Enter" || evt.modifiers().shift() {
                                return;
                            }

                            evt.prevent_default();

                            let currently_busy = *busy_sig.read();
                            if currently_busy {
                                return;
                            }

                            let text = draft_sig.read().trim().to_string();
                            if text.is_empty() {
                                return;
                            }

                            busy_sig.set(true);
                            draft_sig.set(String::new());
                            history_sig.write().push(UiMsg::new("user", text.clone()));

                            let aid = aid.clone();
                            let token = token.clone();
                            let title_for_line = title_for_line.clone();
                            let provider_model = selected_provider_model.clone();
                            let selected_models_payload = selected_models_payload.clone();
                            let wrap_mode_payload = wrap_mode_payload;
                            let mode = selected_mode.clone();
                            let selected_runtime_label = selected_runtime_label.clone();
                            spawn(async move {
                                let provider = if wrap_mode_payload.unwrap_or(false) {
                                    None
                                } else {
                                    provider_model.as_ref().map(|(p, _)| p.as_str())
                                };
                                let model = if wrap_mode_payload.unwrap_or(false) {
                                    None
                                } else {
                                    provider_model.as_ref().map(|(_, m)| m.as_str())
                                };
                                match send_chat(
                                    &text,
                                    aid.as_deref(),
                                    provider,
                                    model,
                                    Some(mode.as_str()),
                                    selected_models_payload,
                                    wrap_mode_payload,
                                    token.as_deref(),
                                )
                                .await
                                {
                                    Ok(reply) => {
                                        history_sig
                                            .write()
                                            .push(UiMsg::new("assistant", reply.reply));
                                        let runtime = if reply.provider.is_some() || reply.model.is_some() {
                                            runtime_label(
                                                reply.provider.as_deref(),
                                                reply.model.as_deref(),
                                            )
                                        } else {
                                            selected_runtime_label.clone()
                                        };
                                        info_sig.set(Some(format!("Reply via {runtime}")));
                                        error_sig.set(None);

                                        let _ = refresh_detached_history(
                                            aid.clone(),
                                            token.clone(),
                                            history_sig,
                                            error_sig,
                                            busy_sig,
                                        )
                                        .await;
                                    }
                                    Err(e) => {
                                        busy_sig.set(false);
                                        error_sig.set(Some(e.clone()));
                                        history_sig.write().push(UiMsg::new(
                                            "system",
                                            format!("Error from {title_for_line}: {e}"),
                                        ));
                                    }
                                }
                            });
                        }
                    },
                }
                button {
                    class: "btn btn-send",
                    disabled: !can_submit,
                    onclick: {
                        let aid = aid.clone();
                        let token = token.clone();
                        let mut draft_sig = draft;
                        let mut history_sig = history;
                        let mut info_sig = info;
                        let mut error_sig = error;
                        let mut busy_sig = history_busy;
                        let title_for_line = title.clone();
                        let selected_provider_model = selected_provider_model.clone();
                        let selected_models_payload = selected_models_payload.clone();
                        let wrap_mode_payload = wrap_mode_payload;
                        let selected_mode = selected_mode.clone();
                        let selected_runtime_label = selected_runtime_label.clone();
                        move |_| {
                            let currently_busy = *busy_sig.read();
                            if currently_busy {
                                return;
                            }

                            let text = draft_sig.read().trim().to_string();
                            if text.is_empty() {
                                return;
                            }

                            busy_sig.set(true);
                            draft_sig.set(String::new());
                            history_sig.write().push(UiMsg::new("user", text.clone()));

                            let aid = aid.clone();
                            let token = token.clone();
                            let title_for_line = title_for_line.clone();
                            let provider_model = selected_provider_model.clone();
                            let selected_models_payload = selected_models_payload.clone();
                            let wrap_mode_payload = wrap_mode_payload;
                            let mode = selected_mode.clone();
                            let selected_runtime_label = selected_runtime_label.clone();
                            spawn(async move {
                                let provider = if wrap_mode_payload.unwrap_or(false) {
                                    None
                                } else {
                                    provider_model.as_ref().map(|(p, _)| p.as_str())
                                };
                                let model = if wrap_mode_payload.unwrap_or(false) {
                                    None
                                } else {
                                    provider_model.as_ref().map(|(_, m)| m.as_str())
                                };
                                match send_chat(
                                    &text,
                                    aid.as_deref(),
                                    provider,
                                    model,
                                    Some(mode.as_str()),
                                    selected_models_payload,
                                    wrap_mode_payload,
                                    token.as_deref(),
                                )
                                .await
                                {
                                    Ok(reply) => {
                                        history_sig
                                            .write()
                                            .push(UiMsg::new("assistant", reply.reply));
                                        let runtime = if reply.provider.is_some() || reply.model.is_some() {
                                            runtime_label(
                                                reply.provider.as_deref(),
                                                reply.model.as_deref(),
                                            )
                                        } else {
                                            selected_runtime_label.clone()
                                        };
                                        info_sig.set(Some(format!("Reply via {runtime}")));
                                        error_sig.set(None);

                                        let _ = refresh_detached_history(
                                            aid.clone(),
                                            token.clone(),
                                            history_sig,
                                            error_sig,
                                            busy_sig,
                                        )
                                        .await;
                                    }
                                    Err(e) => {
                                        busy_sig.set(false);
                                        error_sig.set(Some(e.clone()));
                                        history_sig.write().push(UiMsg::new(
                                            "system",
                                            format!("Error from {title_for_line}: {e}"),
                                        ));
                                    }
                                }
                            });
                        }
                    },
                    if is_history_busy { "Sending..." } else { "Send" }
                }
            }
        }
    }
}

fn tab_btn(
    label: &str,
    tab: SettingsTab,
    active: SettingsTab,
    sig: &mut Signal<SettingsTab>,
) -> Element {
    let cls = if tab == active {
        "tab tab-active"
    } else {
        "tab"
    };
    let mut sig = *sig;
    rsx! {
        button { class: "{cls}", onclick: move |_| { sig.set(tab); }, "{label}" }
    }
}

fn card(
    key: &str,
    title: &str,
    subtitle: &str,
    agent: Option<&ApiAgent>,
    edge_accent: bool,
    mode: PanelMode,
    m_snap: &HashMap<String, Vec<UiMsg>>,
    sending_snap: &HashMap<String, bool>,
    composer_models_snap: &HashMap<String, Vec<String>>,
    model_picker_open_snap: &HashMap<String, bool>,
    composer_mode_snap: &HashMap<String, String>,
    d_snap: &HashMap<String, String>,
    drafts: &mut Signal<HashMap<String, String>>,
    messages: &mut Signal<HashMap<String, Vec<UiMsg>>>,
    sending: &mut Signal<HashMap<String, bool>>,
    composer_models: &mut Signal<HashMap<String, Vec<String>>>,
    model_picker_open: &mut Signal<HashMap<String, bool>>,
    composer_mode: &mut Signal<HashMap<String, String>>,
    info: &mut Signal<Option<String>>,
    error: &mut Signal<Option<String>>,
    default_chat_model: &str,
    admin_token: &Option<String>,
    agents: &mut Signal<Vec<ApiAgent>>,
    layouts: &mut Signal<HashMap<String, PanelLayout>>,
    next_z: &mut Signal<u32>,
    detached_windows: &mut Signal<HashMap<String, WeakDesktopContext>>,
    drag_state: &mut Signal<Option<DragState>>,
    panel_action_confirm: &mut Signal<Option<PendingPanelAction>>,
) -> Element {
    let msgs = m_snap.get(key).cloned().unwrap_or_default();
    let draft = d_snap.get(key).cloned().unwrap_or_default();
    let k = key.to_string();
    let t = title.to_string();
    let subtitle_owned = subtitle.to_string();
    let aid = agent.map(|a| a.id.clone());
    let can_send = key == "kaizen" || aid.is_some();
    let can_submit = can_send && !draft.trim().is_empty();
    let has_agent = agent.is_some();
    let is_kaizen = key == "kaizen";
    let is_sending = sending_snap.get(key).copied().unwrap_or(false);
    let mode_options = if is_kaizen {
        &KAIZEN_CHAT_MODES[..]
    } else {
        &SUBAGENT_CHAT_MODES[..]
    };
    let selected_mode = composer_mode_snap.get(key).cloned().unwrap_or_else(|| {
        if is_kaizen {
            "orchestrator".to_string()
        } else {
            "build".to_string()
        }
    });
    let selected_model_values = normalized_model_values(composer_models_snap.get(key));
    let selected_model_values_with_default =
        model_values_with_default_fallback(&selected_model_values, default_chat_model);
    let selected_targets = model_targets_from_values(&selected_model_values_with_default);
    let selected_provider_model = selected_targets
        .first()
        .map(|target| (target.provider.clone(), target.model.clone()));
    let wrap_mode_enabled = selected_targets.len() > 1;
    let selected_runtime_label = if wrap_mode_enabled {
        format!("wrap x{} models", selected_targets.len())
    } else {
        selected_provider_model
            .as_ref()
            .map(|(p, m)| runtime_label(Some(p.as_str()), Some(m.as_str())))
            .unwrap_or_else(|| "runtime default".to_string())
    };
    let is_model_picker_open = model_picker_open_snap.get(key).copied().unwrap_or(false);
    let detach_supported = native_detach_enabled();

    let mut d_sig = *drafts;
    let mut m_sig = *messages;
    let mut sending_sig = *sending;
    let mut composer_models_sig = *composer_models;
    let mut model_picker_open_sig = *model_picker_open;
    let mut composer_mode_sig = *composer_mode;
    let mut i_sig = *info;
    let mut e_sig = *error;
    let mut a_sig = *agents;
    let mut l_sig = *layouts;
    let mut z_sig = *next_z;
    let mut dw_sig = *detached_windows;
    let mut drag_sig = *drag_state;
    let mut panel_confirm_sig = *panel_action_confirm;
    let token = admin_token.clone();
    let default_chat_model_owned = default_chat_model.to_string();

    // Reusable submit logic for both button and Ctrl+Enter
    let submit_action = {
        let k = k.clone();
        let aid = aid.clone();
        let token = token.clone();
        let t = t.clone();
        let default_model_value = default_chat_model.to_string();
        let mut d_sig = d_sig;
        let mut m_sig = m_sig;
        let mut sending_sig = sending_sig;
        let mut composer_models_sig = composer_models_sig;
        let mut i_sig = i_sig;
        let mut e_sig = e_sig;
        let mode = selected_mode.clone();

        std::rc::Rc::new(std::cell::RefCell::new(move || {
            let text = d_sig
                .read()
                .get(&k)
                .cloned()
                .unwrap_or_default()
                .trim()
                .to_string();
            if text.is_empty() {
                return;
            }
            if sending_sig.read().get(&k).copied().unwrap_or(false) {
                return;
            }

            let selected_values = normalized_model_values(composer_models_sig.read().get(&k));
            let selected_values_with_default =
                model_values_with_default_fallback(&selected_values, &default_model_value);
            let selected_targets = model_targets_from_values(&selected_values_with_default);
            let wrap_mode = selected_targets.len() > 1;
            let primary_target = selected_targets.first().cloned();
            let primary_provider = if wrap_mode {
                None
            } else {
                primary_target
                    .as_ref()
                    .map(|target| target.provider.clone())
            };
            let primary_model = if wrap_mode {
                None
            } else {
                primary_target.as_ref().map(|target| target.model.clone())
            };
            let selected_models_payload = if wrap_mode {
                Some(selected_targets.clone())
            } else {
                None
            };
            let wrap_mode_payload = if wrap_mode { Some(true) } else { None };
            let runtime_lbl = if wrap_mode {
                format!("wrap x{} models", selected_targets.len())
            } else if let Some(target) = primary_target.as_ref() {
                runtime_label(Some(target.provider.as_str()), Some(target.model.as_str()))
            } else {
                "runtime default".to_string()
            };

            d_sig.write().insert(k.clone(), String::new());
            m_sig
                .write()
                .entry(k.clone())
                .or_default()
                .push(UiMsg::new("user", text.clone()));
            sending_sig.write().insert(k.clone(), true);
            e_sig.set(None);

            let k_hist = k.clone();
            let aid_hist = aid.clone();
            let token_hist = token.clone();
            let t_hist = t.clone();
            let primary_provider = primary_provider.clone();
            let primary_model = primary_model.clone();
            let selected_models_payload = selected_models_payload.clone();
            let wrap_mode_payload = wrap_mode_payload;
            let mode = mode.clone();
            let runtime_lbl = runtime_lbl.clone();

            spawn(async move {
                let provider = primary_provider.as_deref();
                let model = primary_model.as_deref();
                match send_chat(
                    &text,
                    aid_hist.as_deref(),
                    provider,
                    model,
                    Some(mode.as_str()),
                    selected_models_payload,
                    wrap_mode_payload,
                    token_hist.as_deref(),
                )
                .await
                {
                    Ok(resp) => {
                        m_sig
                            .write()
                            .entry(k_hist.clone())
                            .or_default()
                            .push(UiMsg::new("assistant", resp.reply));
                        let runtime = if resp.provider.is_some() || resp.model.is_some() {
                            runtime_label(resp.provider.as_deref(), resp.model.as_deref())
                        } else {
                            runtime_lbl
                        };
                        i_sig.set(Some(format!("Reply via {runtime}")));
                        sending_sig.write().insert(k_hist.clone(), false);

                        if let Ok(history) =
                            fetch_chat_history(aid_hist.as_deref(), token_hist.as_deref()).await
                        {
                            m_sig.write().insert(k_hist, history);
                        }
                    }
                    Err(e) => {
                        sending_sig.write().insert(k_hist.clone(), false);
                        e_sig.set(Some(e.clone()));
                        m_sig
                            .write()
                            .entry(k_hist)
                            .or_default()
                            .push(UiMsg::new("system", format!("Error from {t_hist}: {e}")));
                    }
                }
            });
        }))
    };

    let card_class = match (mode == PanelMode::Floating, edge_accent, is_kaizen) {
        (true, true, true) => {
            "card floating-card card-edge-accent card-kaizen-hero card-kaizen-floating"
        }
        (true, false, true) => "card floating-card card-kaizen-hero card-kaizen-floating",
        (false, true, true) => "card card-edge-accent card-kaizen-hero",
        (false, false, true) => "card card-kaizen-hero",
        (true, true, false) => "card floating-card card-edge-accent",
        (true, false, false) => "card floating-card",
        (false, true, false) => "card card-edge-accent",
        (false, false, false) => "card",
    };

    rsx! {
        div { class: "{card_class}", key: "{k}",
            onmousedown: {
                let k = k.clone();
                let mut drag_sig = drag_sig;
                let mut z_sig = z_sig;
                let mut l_sig = l_sig;
                move |evt: Event<MouseData>| {
                    if mode != PanelMode::Floating {
                        return;
                    }

                    let local = evt.data().element_coordinates();
                    let drag_zone_height = if is_kaizen { 96.0 } else { FLOAT_DRAG_ZONE_HEIGHT };
                    if local.y > drag_zone_height {
                        return;
                    }

                    let point = evt.data().client_coordinates();
                    let mut map = l_sig.read().clone();
                    let Some(layout) = map.get_mut(&k) else {
                        return;
                    };

                    layout.z = bump_z(&mut z_sig);
                    let snapshot = layout.clone();
                    l_sig.set(map);

                    drag_sig.set(Some(DragState {
                        panel_key: k.clone(),
                        mouse_start_x: point.x,
                        mouse_start_y: point.y,
                        panel_start_x: snapshot.x,
                        panel_start_y: snapshot.y,
                    }));
                }
            },
            div {
                class: if mode == PanelMode::Floating { "card-head card-head-drag" } else { "card-head" },
                h2 {
                    class: "card-title",
                    "{t}"
                }
                div {
                    class: "card-actions",
                    onmousedown: move |e: Event<MouseData>| e.stop_propagation(),
                    button {
                        class: if is_model_picker_open {
                            "btn btn-xs btn-accent btn-model-toggle"
                        } else {
                            "btn btn-xs btn-sec btn-model-toggle"
                        },
                        title: "Open model picker",
                        onclick: {
                            let k = k.clone();
                            let mut model_picker_open_sig = model_picker_open_sig;
                            move |_| {
                                let mut map = model_picker_open_sig.read().clone();
                                let current = map.get(&k).copied().unwrap_or(false);
                                map.insert(k.clone(), !current);
                                model_picker_open_sig.set(map);
                            }
                        },
                        "M"
                    }
                    button {
                        class: if mode == PanelMode::Docked { "btn btn-xs btn-accent" } else { "btn btn-xs btn-sec" },
                        title: "Dock this panel",
                        onclick: {
                            let k = k.clone();
                            let mut l_sig = l_sig;
                            let mut z_sig = z_sig;
                            let mut dw_sig = dw_sig;
                            let mut drag_sig = drag_sig;
                            move |_| {
                                drag_sig.set(None);

                                let removed = {
                                    let mut map = dw_sig.write();
                                    map.remove(&k)
                                };

                                if let Some(handle) = removed {
                                    if let Some(win) = handle.upgrade() {
                                        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                            win.close();
                                        }));
                                    }
                                }

                                let mut map = l_sig.read().clone();
                                let layout = map.entry(k.clone()).or_insert_with(|| default_panel_layout(0));
                                layout.mode = PanelMode::Docked;
                                layout.z = bump_z(&mut z_sig);
                                l_sig.set(map.clone());
                                persist_layouts(&map);
                            }
                        },
                        "D"
                    }
                    button {
                        class: if mode == PanelMode::Floating { "btn btn-xs btn-accent" } else { "btn btn-xs btn-sec" },
                        title: "Float this panel",
                        onclick: {
                            let k = k.clone();
                            let mut l_sig = l_sig;
                            let mut z_sig = z_sig;
                            let mut drag_sig = drag_sig;
                            move |_| {
                                drag_sig.set(None);
                                let mut map = l_sig.read().clone();
                                let layout = map.entry(k.clone()).or_insert_with(|| default_panel_layout(0));
                                layout.mode = PanelMode::Floating;
                                layout.width = layout.width.max(MIN_PANEL_WIDTH);
                                layout.height = layout.height.max(MIN_PANEL_HEIGHT);
                                layout.z = bump_z(&mut z_sig);
                                l_sig.set(map.clone());
                                persist_layouts(&map);
                            }
                        },
                        "F"
                    }
                    button {
                        class: if mode == PanelMode::Detached { "btn btn-xs btn-accent" } else { "btn btn-xs btn-sec" },
                        title: if detach_supported {
                            "Detach to native window"
                        } else {
                            "Native detach guarded for stability; using floating mode"
                        },
                        onclick: {
                            let k = k.clone();
                            let title = t.clone();
                            let subtitle = subtitle_owned.clone();
                            let aid = aid.clone();
                            let token = token.clone();
                            let detach_supported = detach_supported;
                            let mut l_sig = l_sig;
                            let mut z_sig = z_sig;
                            let mut dw_sig = dw_sig;
                            let mut e_sig = e_sig;
                            let mut drag_sig = drag_sig;
                            let default_chat_model_owned = default_chat_model_owned.clone();
                            move |_| {
                                drag_sig.set(None);
                                append_ui_diagnostic(&format!("detach requested key={k}"));

                                if !detach_supported {
                                    let mut map = l_sig.read().clone();
                                    let layout =
                                        map.entry(k.clone()).or_insert_with(|| default_panel_layout(0));
                                    layout.mode = PanelMode::Floating;
                                    layout.width = layout.width.max(MIN_DETACHED_WIDTH);
                                    layout.height = layout.height.max(MIN_DETACHED_HEIGHT);
                                    layout.z = bump_z(&mut z_sig);
                                    l_sig.set(map.clone());
                                    persist_layouts(&map);
                                    e_sig.set(Some(
                                        "Native detach is guarded for stability. Set KAIZEN_ENABLE_NATIVE_DETACH=1 and KAIZEN_NATIVE_DETACH_UNSAFE_ACK=1 to enable it, otherwise floating mode is used."
                                            .to_string(),
                                    ));
                                    append_ui_diagnostic(&format!(
                                        "detach fallback to floating key={k}"
                                    ));
                                    return;
                                }

                                let already_open = dw_sig
                                    .read()
                                    .get(&k)
                                    .and_then(|w| w.upgrade())
                                    .is_some();
                                if already_open {
                                    return;
                                }

                                let spawn_layout = l_sig
                                    .read()
                                    .get(&k)
                                    .cloned()
                                    .unwrap_or_else(|| default_panel_layout(0));

                                let launch_result = std::panic::catch_unwind(
                                    std::panic::AssertUnwindSafe(|| {
                                        let dom = VirtualDom::new_with_props(
                                            DetachedWindow,
                                            DetachedWindowProps {
                                                panel_key: k.clone(),
                                                title: title.clone(),
                                                subtitle: subtitle.clone(),
                                                agent_id: aid.clone(),
                                                admin_token: token.clone(),
                                                default_chat_model: default_chat_model_owned.clone(),
                                            },
                                        );

                                        let cfg = DesktopConfig::new().with_window(
                                            WindowBuilder::new()
                                                .with_title(format!("Kaizen MAX | {title}"))
                                                .with_min_inner_size(LogicalSize::new(
                                                    MIN_DETACHED_WIDTH,
                                                    MIN_DETACHED_HEIGHT,
                                                ))
                                                .with_inner_size(LogicalSize::new(
                                                    spawn_layout.width.max(MIN_DETACHED_WIDTH),
                                                    spawn_layout.height.max(MIN_DETACHED_HEIGHT),
                                                ))
                                                .with_position(LogicalPosition::new(
                                                    spawn_layout.x.max(MIN_PANEL_X),
                                                    spawn_layout.y.max(MIN_PANEL_Y),
                                                )),
                                        );

                                        window().new_window(dom, cfg)
                                    }),
                                );

                                match launch_result {
                                    Ok(weak) => {
                                        dw_sig.write().insert(k.clone(), weak);

                                        let mut map = l_sig.read().clone();
                                        let layout =
                                            map.entry(k.clone()).or_insert_with(|| default_panel_layout(0));
                                        layout.mode = PanelMode::Detached;
                                        layout.z = bump_z(&mut z_sig);
                                        l_sig.set(map.clone());
                                        persist_layouts(&map);

                                        e_sig.set(None);
                                        append_ui_diagnostic(&format!("detach opened key={k}"));
                                    }
                                    Err(_) => {
                                        e_sig.set(Some(
                                            "Detach failed due to a window initialization error".to_string(),
                                        ));
                                        append_ui_diagnostic(&format!("detach failed key={k}"));
                                    }
                                }
                            }
                        },
                        "↗"
                    }

                    if has_agent {
                        button {
                            class: "btn btn-xs btn-warn",
                            title: "Stop agent",
                            onclick: {
                                let aid = aid.clone();
                                let k = k.clone();
                                let t = t.clone();
                                let mut panel_confirm_sig = panel_confirm_sig;
                                move |_| {
                                    if let Some(id) = aid.clone() {
                                        panel_confirm_sig.set(Some(PendingPanelAction {
                                            action: DestructiveAction::Stop,
                                            panel_key: k.clone(),
                                            panel_title: t.clone(),
                                            agent_id: id,
                                        }));
                                    }
                                }
                            },
                            "Stop"
                        }
                        button {
                            class: "btn btn-xs btn-sec",
                            title: "Clear chat",
                            onclick: {
                                let aid = aid.clone();
                                let k = k.clone();
                                let t = t.clone();
                                let mut panel_confirm_sig = panel_confirm_sig;
                                move |_| {
                                    if let Some(id) = aid.clone() {
                                        panel_confirm_sig.set(Some(PendingPanelAction {
                                            action: DestructiveAction::Clear,
                                            panel_key: k.clone(),
                                            panel_title: t.clone(),
                                            agent_id: id,
                                        }));
                                    }
                                }
                            },
                            "Clear"
                        }
                        button {
                            class: "btn btn-xs btn-danger",
                            title: "Remove agent",
                            onclick: {
                                let aid = aid.clone();
                                let k = k.clone();
                                let t = t.clone();
                                let mut panel_confirm_sig = panel_confirm_sig;
                                move |_| {
                                    if let Some(id) = aid.clone() {
                                        panel_confirm_sig.set(Some(PendingPanelAction {
                                            action: DestructiveAction::Remove,
                                            panel_key: k.clone(),
                                            panel_title: t.clone(),
                                            agent_id: id,
                                        }));
                                    }
                                }
                            },
                            "Remove"
                        }
                    }
                }
                span { class: "comp-runtime", "Runtime: {selected_runtime_label}" }
            }

            p { class: "card-sub", "{subtitle}" }

            if is_kaizen {
                div { class: "kaizen-hero",
                    div { class: "kaizen-hero-copy",
                        p { class: "kaizen-hero-kicker", "Kaizen Orchestrator" }
                        h3 { class: "kaizen-hero-title", "Command Deck" }
                        p { class: "kaizen-hero-text", "Plan, supervise, and approve every agent before delivery." }
                    }
                    div { class: "kaizen-hero-stats",
                        div { class: "kaizen-stat",
                            span { class: "kaizen-stat-label", "Active" }
                            strong { class: "kaizen-stat-val", "{a_sig.read().len()}" }
                        }
                        div { class: "kaizen-stat",
                            span { class: "kaizen-stat-label", "Phase" }
                            strong { class: "kaizen-stat-val", "{mode.label()}" }
                        }
                    }
                }
            }

            div { class: "card-stream",
                if msgs.is_empty() {
                    p { class: "stream-empty", if can_send { "No messages yet." } else { "Spawn to chat." } }
                }
                for line in msgs.iter().rev().take(80).rev() {
                    div {
                        class: if line.role == "user" { "msg msg-you" } else { "msg msg-ai" },
                        span {
                            class: "msg-role",
                            if line.role == "user" {
                                "You"
                            } else if line.role == "assistant" {
                                "{t}"
                            } else {
                                "{line.role}"
                            }
                        }
                        span { class: "msg-txt", "{line.content}" }
                    }
                }
            }

            div { class: "composer-toolbar",
                div { class: "model-picker-summary",
                    if wrap_mode_enabled {
                        "Wrap mode on ({selected_targets.len()} models)"
                    } else {
                        "Model: {selected_runtime_label}"
                    }
                }
                if is_model_picker_open {
                    div {
                        class: "model-picker-popover",
                        p { class: "model-picker-title", "Model selection" }
                        p { class: "model-picker-hint", "Select one for direct mode, or multiple for wrap mode." }
                        for (provider, label, model) in CHAT_MODEL_PRESETS {
                            label {
                                class: "model-picker-option",
                                input {
                                    r#type: "checkbox",
                                    checked: selected_model_values_with_default
                                        .iter()
                                        .any(|entry| entry == &model_value(provider, model)),
                                    onclick: {
                                        let k = k.clone();
                                        let provider = provider.to_string();
                                        let model = model.to_string();
                                        let mut composer_models_sig = composer_models_sig;
                                        let default_model_value = default_chat_model.to_string();
                                        move |_| {
                                            let preset_value = model_value(&provider, &model);
                                            let mut map = composer_models_sig.read().clone();
                                            let explicit_selected =
                                                normalized_model_values(map.get(&k));
                                            let mut effective_selected = model_values_with_default_fallback(
                                                &explicit_selected,
                                                &default_model_value,
                                            );

                                            if let Some(idx) = effective_selected
                                                .iter()
                                                .position(|entry| entry == &preset_value)
                                            {
                                                if effective_selected.len() > 1 {
                                                    effective_selected.remove(idx);
                                                }
                                            } else {
                                                effective_selected.push(preset_value);
                                            }

                                            let updated_selected =
                                                normalized_model_values(Some(&effective_selected));
                                            if updated_selected.is_empty()
                                                || (updated_selected.len() == 1
                                                    && updated_selected[0] == default_model_value)
                                            {
                                                map.remove(&k);
                                            } else {
                                                map.insert(k.clone(), updated_selected);
                                            }
                                            composer_models_sig.set(map);
                                        }
                                    },
                                }
                                span { "{label}" }
                            }
                        }
                    }
                }
                div { class: "comp-mode-row",
                    for mode_name in mode_options.iter() {
                        button {
                            class: if *mode_name == selected_mode {
                                "comp-mode-chip comp-mode-chip-active"
                            } else {
                                "comp-mode-chip"
                            },
                            onclick: {
                                let k = k.clone();
                                let mode_name = mode_name.to_string();
                                let mut composer_mode_sig = composer_mode_sig;
                                move |_| {
                                    composer_mode_sig.write().insert(k.clone(), mode_name.clone());
                                }
                            },
                            "{mode_name}"
                        }
                    }
                }
            }

            div { class: "composer",
                textarea {
                    class: "comp-in",
                    rows: "1",
                    disabled: !can_send,
                    value: draft,
                    placeholder: if can_send { "Message your agent..." } else { "Not active" },
                    oninput: {
                        let k = k.clone();
                        move |e: Event<FormData>| {
                            d_sig.write().insert(k.clone(), e.value());
                        }
                    },
                    onkeydown: {
                        let k = k.clone();
                        let mut d_sig = d_sig;
                        let submit = submit_action.clone();
                        move |evt: Event<KeyboardData>| {
                            let key = evt.key().to_string();
                            if key == "Escape" {
                                d_sig.write().insert(k.clone(), String::new());
                                return;
                            }

                            let ctrl = evt.modifiers().ctrl() || evt.modifiers().meta();
                            if key == "Enter" && ctrl {
                                evt.prevent_default();
                                (submit.borrow_mut())();
                            }
                        }
                    },
                }
                button {
                    class: "btn btn-send",
                    disabled: !can_submit || is_sending,
                    onclick: {
                        let submit = submit_action.clone();
                        move |_| (submit.borrow_mut())()
                    },
                    if is_sending { "Sending..." } else { "Send" }
                }
            }
        }
    }
}

fn checklist(c: &GateConditions) -> Element {
    let items = [
        ("Plan drafted", c.plan_defined),
        ("Plan acknowledged", c.plan_acknowledged),
        ("Execution artifacts", c.execution_artifacts_present),
        ("Reasoning checks", c.passed_reasoners_test),
        ("Kaizen review", c.kaizen_review_approved),
        ("Human smoke test", c.human_smoke_test_passed),
        ("Deploy validation", c.deploy_validation_passed),
    ];
    rsx! {
        div { class: "checklist",
            for (label, done) in items {
                div { class: "ck-row",
                    span { class: if done { "ck done" } else { "ck" }, if done { "v" } else { " " } }
                    span { class: if done { "ck-label ck-done" } else { "ck-label" }, "{label}" }
                }
            }
        }
    }
}

fn default_panel_layout(index: usize) -> PanelLayout {
    let col = (index % 3) as f64;
    let row = (index / 3) as f64;
    PanelLayout {
        mode: PanelMode::Docked,
        x: 240.0 + col * 360.0,
        y: 100.0 + row * 60.0,
        width: 360.0,
        height: 390.0,
        z: (10 + index) as u32,
    }
}

fn bump_z(sig: &mut Signal<u32>) -> u32 {
    let next = (*sig.read()).saturating_add(1);
    sig.set(next);
    next
}

fn normalize_panel_layouts(
    layouts: &mut HashMap<String, PanelLayout>,
    agents: &[ApiAgent],
    next_z: &mut u32,
) -> bool {
    let (viewport_w, viewport_h) = viewport_size();
    let mut changed = false;
    let mut valid_keys = HashSet::new();
    valid_keys.insert("kaizen".to_string());
    for agent in agents {
        valid_keys.insert(agent.id.clone());
    }

    let stale: Vec<String> = layouts
        .keys()
        .filter(|key| !valid_keys.contains(*key))
        .cloned()
        .collect();
    for key in stale {
        layouts.remove(&key);
        changed = true;
    }

    let mut ordered_keys = vec!["kaizen".to_string()];
    ordered_keys.extend(agents.iter().map(|a| a.id.clone()));

    for (idx, key) in ordered_keys.iter().enumerate() {
        if !layouts.contains_key(key) {
            layouts.insert(key.clone(), default_panel_layout(idx));
            changed = true;
        }

        if let Some(layout) = layouts.get_mut(key) {
            let (min_w, min_h) = if layout.mode == PanelMode::Detached {
                (MIN_DETACHED_WIDTH, MIN_DETACHED_HEIGHT)
            } else {
                (MIN_PANEL_WIDTH, MIN_PANEL_HEIGHT)
            };

            if layout.width < min_w {
                layout.width = min_w;
                changed = true;
            }
            if layout.height < min_h {
                layout.height = min_h;
                changed = true;
            }

            if layout.mode == PanelMode::Floating || layout.mode == PanelMode::Detached {
                let previous_x = layout.x;
                let previous_y = layout.y;

                // Monitor Fallback:
                // We clamp to the main window's viewport as a proxy for "safe primary area".
                // This ensures that if a monitor is missing (or window was far off-screen),
                // it is pulled back into view.
                let max_width = viewport_w.max(min_w);
                let max_height = viewport_h.max(min_h);

                if layout.width > max_width {
                    layout.width = max_width;
                    changed = true;
                }
                if layout.height > max_height {
                    layout.height = max_height;
                    changed = true;
                }

                clamp_floating_layout(layout, viewport_w, viewport_h);
                if (layout.x - previous_x).abs() > f64::EPSILON
                    || (layout.y - previous_y).abs() > f64::EPSILON
                {
                    changed = true;
                }
            }

            // Docked panels ignore x/y/w/h, so we don't clamp them.

            if layout.z >= *next_z {
                *next_z = layout.z.saturating_add(1);
            }
        }
    }

    changed
}

fn layout_file_path() -> Option<PathBuf> {
    let appdata = std::env::var("APPDATA").ok()?;
    Some(PathBuf::from(appdata).join("KaizenMAX").join("layout.json"))
}

fn load_layouts() -> HashMap<String, PanelLayout> {
    let Some(path) = layout_file_path() else {
        return HashMap::new();
    };

    let Ok(raw) = std::fs::read_to_string(path) else {
        return HashMap::new();
    };

    let Ok(mut parsed) = serde_json::from_str::<HashMap<String, PanelLayout>>(&raw) else {
        return HashMap::new();
    };

    let allow_detach = native_detach_enabled();

    for layout in parsed.values_mut() {
        if layout.mode == PanelMode::Detached && !allow_detach {
            layout.mode = PanelMode::Floating;
        }
    }

    parsed
}

fn persist_layouts(layouts: &HashMap<String, PanelLayout>) {
    let Some(path) = layout_file_path() else {
        return;
    };

    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    if let Ok(json) = serde_json::to_vec_pretty(layouts) {
        let _ = std::fs::write(path, json);
    }
}

fn workspace_file_path() -> Option<PathBuf> {
    let appdata = std::env::var("APPDATA").ok()?;
    Some(
        PathBuf::from(appdata)
            .join("KaizenMAX")
            .join("workspaces.json"),
    )
}

fn workspace_tile_from_path(path: &str) -> WorkspaceTile {
    let trimmed = path.trim();
    let normalized = trimmed.replace('\\', "/").to_ascii_lowercase();
    let name = PathBuf::from(trimmed)
        .file_name()
        .map(|segment| segment.to_string_lossy().to_string())
        .filter(|segment| !segment.trim().is_empty())
        .unwrap_or_else(|| trimmed.to_string());

    WorkspaceTile {
        id: format!("local:{normalized}"),
        name,
        path: Some(trimmed.to_string()),
    }
}

fn load_workspace_tiles() -> Vec<WorkspaceTile> {
    let mut tiles = vec![WorkspaceTile {
        id: "kaizen-max".to_string(),
        name: "Kaizen MAX".to_string(),
        path: None,
    }];

    let Some(path) = workspace_file_path() else {
        return tiles;
    };

    let Ok(raw) = std::fs::read_to_string(path) else {
        return tiles;
    };

    let Ok(saved) = serde_json::from_str::<PersistedWorkspaces>(&raw) else {
        return tiles;
    };

    let mut seen = HashSet::new();
    for saved_path in saved.paths {
        let trimmed = saved_path.trim();
        if trimmed.is_empty() {
            continue;
        }
        let key = trimmed.replace('\\', "/").to_ascii_lowercase();
        if !seen.insert(key) {
            continue;
        }
        tiles.push(workspace_tile_from_path(trimmed));
    }

    tiles
}

fn persist_workspace_tiles(tiles: &[WorkspaceTile]) {
    let Some(path) = workspace_file_path() else {
        return;
    };

    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let mut paths = Vec::new();
    for tile in tiles {
        if let Some(path) = tile.path.as_ref() {
            let trimmed = path.trim();
            if !trimmed.is_empty() {
                paths.push(trimmed.to_string());
            }
        }
    }

    let payload = PersistedWorkspaces { paths };
    if let Ok(json) = serde_json::to_vec_pretty(&payload) {
        let _ = std::fs::write(path, json);
    }
}

fn prune_detached_windows(
    detached: &mut Signal<HashMap<String, WeakDesktopContext>>,
    valid_keys: &HashSet<String>,
) {
    let stale: Vec<String> = detached
        .read()
        .keys()
        .filter(|key| !valid_keys.contains(*key))
        .cloned()
        .collect();

    if stale.is_empty() {
        return;
    }

    let handles_to_close: Vec<WeakDesktopContext> = {
        let mut map = detached.write();
        stale
            .into_iter()
            .filter_map(|key| map.remove(&key))
            .collect()
    };

    for handle in handles_to_close {
        if let Some(win) = handle.upgrade() {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                win.close();
            }));
        }
    }
}

async fn refresh_dashboard_state(
    token: Option<String>,
    mut agents: Signal<Vec<ApiAgent>>,
    mut gates: Signal<Option<GateSnapshot>>,
    mut events: Signal<Vec<CrystalBallEvent>>,
    mut messages: Signal<HashMap<String, Vec<UiMsg>>>,
    mut layouts: Signal<HashMap<String, PanelLayout>>,
    mut next_z: Signal<u32>,
    mut detached_windows: Signal<HashMap<String, WeakDesktopContext>>,
) -> Result<(), String> {
    let dashboard = fetch_dashboard(token.as_deref()).await?;
    let transcript_map = fetch_all_histories(&dashboard.agents, token.as_deref()).await;

    let mut layout_map = layouts.read().clone();
    let mut z = *next_z.read();
    let changed = normalize_panel_layouts(&mut layout_map, &dashboard.agents, &mut z);
    next_z.set(z);

    if changed {
        layouts.set(layout_map.clone());
        persist_layouts(&layout_map);
    }

    let mut valid = HashSet::new();
    valid.insert("kaizen".to_string());
    for agent in &dashboard.agents {
        valid.insert(agent.id.clone());
    }
    prune_detached_windows(&mut detached_windows, &valid);

    agents.set(dashboard.agents);
    gates.set(Some(dashboard.gates));
    events.set(dashboard.events);
    messages.set(transcript_map);

    Ok(())
}

const CSS: &str = r#"
*{margin:0;padding:0;box-sizing:border-box}
.shell{min-height:100vh;display:flex;flex-direction:column;background:#0f1923;color:#e4ecf5;font-family:'Segoe UI','Inter',-apple-system,sans-serif;font-size:14px;position:relative}

.top-bar{display:flex;align-items:center;justify-content:space-between;padding:12px 20px;background:linear-gradient(90deg,#151f2e,#1a2840);border-bottom:1px solid #2c3f58;gap:14px;flex-shrink:0;z-index:10}
.top-left{display:flex;align-items:center;gap:12px;min-width:0}
.top-right{display:flex;align-items:center;gap:8px}
.toggle-btn{background:#233448;color:#8da5be;border:1px solid #35506e;border-radius:6px;width:26px;height:26px;cursor:pointer;font-size:13px;display:flex;align-items:center;justify-content:center}
.toggle-btn:hover{background:#2f4560;color:#c8daf0}
.brand-lockup{display:flex;align-items:center;gap:10px;min-width:0}
.brand-mark{width:28px;height:28px;flex-shrink:0;border-radius:8px;background:linear-gradient(145deg,#53a4ff 0%,#2d5ac2 46%,#5f2de2 100%);box-shadow:0 8px 18px rgba(61,104,199,.35),inset 0 0 0 1px rgba(255,255,255,.16);position:relative;overflow:hidden}
.brand-mark::before{content:"";position:absolute;inset:-6px 11px;background:linear-gradient(180deg,rgba(255,255,255,.95),rgba(255,230,110,.9));transform:rotate(32deg)}
.brand-text{display:flex;flex-direction:column;min-width:0}
.logo{font-size:20px;font-weight:700;color:#f0f6ff;line-height:1.05;letter-spacing:.2px}
.brand-sub{font-size:10px;letter-spacing:1.2px;text-transform:uppercase;color:#86a6c4;line-height:1.1}
.top-meta{font-size:12px;color:#8ca4bd}

.btn{border:none;border-radius:8px;cursor:pointer;font-size:13px;font-weight:500;padding:7px 13px;transition:background .12s,transform .06s}
.btn:hover{filter:brightness(1.12)}
.btn:active{transform:scale(.97)}
.btn:focus-visible,.comp-in:focus-visible,.s-input:focus-visible,.modal-close:focus-visible{outline:2px solid #6fb0ff;outline-offset:2px}
.btn-sec{background:#283d56;color:#d0e0f2;border:1px solid #3b5874}
.btn-accent{background:#3d8b65;color:#effff6}
.btn-send{background:linear-gradient(180deg,#57a4ff,#3d84db);color:#fff;border-left:1px solid #35506d;border-radius:0;padding:8px 14px}
.btn-send:disabled{background:#2c3f55;color:#5d7a96;cursor:default;border-left-color:#2f435b}
.btn-sm{font-size:11px;padding:5px 9px}
.btn-xs{font-size:9px;padding:2px 5px;border-radius:4px}
.btn-warn{background:#7a6520;color:#ffe8a0}
.btn-danger{background:#7a2e38;color:#ffd4d4}
.btn-danger:hover{background:#983845}

.banner{padding:7px 20px;font-size:12px}
.err{background:rgba(130,34,46,.32);color:#ffbcb6;border-bottom:1px solid #8b3a4a}
.ok{background:rgba(38,96,68,.25);color:#c4efd2;border-bottom:1px solid #3c7656}

.body{display:flex;flex:1;min-height:0;overflow:hidden}

.left{width:230px;min-width:230px;background:#131e2c;border-right:1px solid #263a52;padding:14px 12px;display:flex;flex-direction:column;gap:14px;overflow-y:auto}
.sb-head{display:flex;align-items:center;justify-content:space-between}
.sb-title{font-size:11px;text-transform:uppercase;letter-spacing:1.1px;color:#6f8da8;font-weight:600}
.sb-label{font-size:11px;text-transform:uppercase;letter-spacing:1px;color:#6f8da8;font-weight:600}
.sb-hint{font-size:12px;color:#6d8ba7;line-height:1.4}
.sb-sub{font-size:11px;color:#7b94ad;line-height:1.4}
.sb-section{display:flex;flex-direction:column;gap:7px}
.sb-spacer{flex:1}
.gear-btn{background:#233448;color:#8da5be;border:1px solid #35506e;border-radius:5px;width:24px;height:24px;cursor:pointer;font-size:11px;display:flex;align-items:center;justify-content:center}
.gear-btn:hover{background:#2f4560;color:#c8daf0}
.ws-item{display:flex;align-items:center;gap:8px;padding:7px 9px;border-radius:7px;cursor:pointer;font-size:13px;color:#b3c8de}
.ws-item:hover{background:#1d2e42}
.ws-active{background:#223650;color:#e6f0ff}
.ws-dot{width:8px;height:8px;border-radius:50%;background:#4a6580;flex-shrink:0}
.ws-dot-on{background:#4dd88a}
.workspace-taskbar{display:flex;flex-direction:column;gap:8px}
.workspace-add-row{display:flex;gap:6px;align-items:center}
.workspace-path-input{flex:1;min-width:0;font-size:12px}
.ws-pill{display:flex;align-items:center;justify-content:space-between;gap:8px;padding:9px 10px;border-radius:10px;border:1px solid #2a425e;background:#162637;color:#c4d8ed;font-size:12px;cursor:pointer;text-align:left;transition:all .12s}
.ws-pill:hover{border-color:#4a78a8;background:#1e3247}
.ws-pill-active{border-color:#62a8f7;background:linear-gradient(90deg,#20384f,#1a2f44);box-shadow:0 0 0 1px rgba(98,168,247,.25)}
.ws-pill-name{font-weight:600;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
.ws-pill-meta{font-size:10px;color:#86a7c6;flex-shrink:0}
.ws-add{justify-content:center;border-style:dashed;color:#9eb8d4}
.sb-bottom-stack{display:flex;flex-direction:column;gap:10px}
.account-hero{border:1px solid #35506d;background:linear-gradient(155deg,#1a2b3f,#182739 50%,#142232);border-radius:12px;padding:10px;display:flex;flex-direction:column;gap:9px}
.account-top{display:flex;align-items:center;gap:9px}
.account-avatar{width:34px;height:34px;border-radius:50%;display:flex;align-items:center;justify-content:center;font-weight:700;background:linear-gradient(145deg,#58a3ff,#6f4de7);color:#fff;box-shadow:0 0 0 2px rgba(255,255,255,.08)}
.account-meta{display:flex;flex-direction:column;min-width:0}
.account-name{font-size:13px;color:#e6f1ff;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
.account-sub{font-size:11px;color:#8aa8c5}
.account-chip-row{display:flex;flex-wrap:wrap;gap:6px}
.account-chip{font-size:10px;padding:3px 8px;border-radius:999px;border:1px solid #35506d;color:#9cc0df;background:#152436}
.account-open-btn{width:100%}

.center{flex:1;padding:16px;overflow-y:auto;background:#111c2a;position:relative}
.grid{display:flex;flex-wrap:wrap;gap:14px;align-items:flex-start;position:relative;z-index:1}

.card{width:380px;min-height:340px;background:#1b2a3d;border:1px solid #314a66;border-radius:12px;padding:12px;display:flex;flex-direction:column;gap:8px;transition:border-color .12s,box-shadow .12s;overflow:hidden}
.card:hover{border-color:#4a8dd4;box-shadow:0 4px 18px rgba(74,141,212,.10)}
.card-edge-accent{border-color:#3c4956;box-shadow:inset 0 0 0 1px rgba(10,12,16,.72),inset 0 0 0 2px rgba(170,180,192,.14)}
.card-edge-accent:hover{border-color:#5f6c79;box-shadow:0 4px 18px rgba(74,141,212,.08),inset 0 0 0 1px rgba(10,12,16,.8),inset 0 0 0 2px rgba(186,195,206,.18)}
.floating-card{width:100%;height:100%;min-height:0}
.card-add{border:2px dashed #384f6a;background:transparent;cursor:pointer;display:flex;align-items:center;justify-content:center;min-height:340px}
.card-add:hover{border-color:#5a8abf;background:rgba(90,138,191,.05)}
.add-inner{display:flex;flex-direction:column;align-items:center;gap:8px;color:#5a7d9e;font-size:14px}
.add-icon{font-size:34px;color:#4a7aa6}
.card-head{display:flex;align-items:center;justify-content:space-between;gap:5px;padding-top:0}
.card-head-drag{cursor:grab;user-select:none}
.card-head-drag:active{cursor:grabbing}
.is-dragging .card-head-drag{cursor:grabbing}
.card-kaizen-hero{background:radial-gradient(120% 110% at 10% -10%,rgba(98,168,247,.16),transparent 38%),radial-gradient(100% 90% at 92% -18%,rgba(123,94,250,.14),transparent 44%),#1b2a3d}
.card-kaizen-floating{box-shadow:0 18px 46px rgba(16,34,62,.5),0 0 0 1px rgba(98,168,247,.22)}
.card-title{font-size:14px;font-weight:600;color:#f2f8ff;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
.card-actions{display:flex;gap:2px;flex-wrap:nowrap;justify-content:flex-end}
.btn-model-toggle{min-width:20px;padding:2px 5px;font-weight:700}
.card-sub{font-size:10px;color:#7a99b5;margin-top:-3px}
.mode-chip{display:none}
.kaizen-hero{position:relative;display:flex;justify-content:space-between;gap:12px;padding:10px;border:1px solid #32567d;border-radius:10px;background:linear-gradient(130deg,rgba(32,55,80,.92),rgba(24,41,61,.95));overflow:hidden}
.kaizen-hero::after{content:"";position:absolute;width:110px;height:110px;right:-34px;top:-50px;border-radius:50%;background:radial-gradient(circle,rgba(106,181,239,.35),transparent 68%);pointer-events:none}
.kaizen-hero-copy{display:flex;flex-direction:column;gap:2px;z-index:1}
.kaizen-hero-kicker{font-size:10px;letter-spacing:.8px;text-transform:uppercase;color:#8eb6db}
.kaizen-hero-title{font-size:16px;line-height:1.1;color:#eef6ff}
.kaizen-hero-text{font-size:11px;color:#9bbddc;max-width:280px}
.kaizen-hero-stats{display:flex;flex-direction:column;gap:6px;z-index:1}
.kaizen-stat{padding:5px 8px;border:1px solid #365779;border-radius:8px;background:#15283b;display:flex;flex-direction:column;align-items:flex-end;min-width:86px}
.kaizen-stat-label{font-size:9px;color:#87aac8;text-transform:uppercase;letter-spacing:.7px}
.kaizen-stat-val{font-size:13px;color:#e6f3ff}
.card-stream{flex:1;background:#141f2e;border:1px solid #283e56;border-radius:8px;padding:8px;overflow-y:auto;min-height:100px;display:flex;flex-direction:column;gap:5px}
.stream-empty{color:#5b7d9a;font-size:12px}
.msg{padding:5px 9px;border-radius:8px;font-size:12px;line-height:1.4;max-width:95%}
.msg-you{background:#253d58;color:#d8e8f8;align-self:flex-end;border-bottom-right-radius:2px}
.msg-ai{background:#1e3048;color:#c8ddf0;align-self:flex-start;border-bottom-left-radius:2px}
.msg-role{font-weight:600;margin-right:5px;font-size:10px;color:#8aafcc}
.composer-toolbar{display:flex;flex-direction:column;align-items:stretch;gap:3px}
.comp-model{min-width:140px;max-width:180px;background:#152436;border:1px solid #35506d;border-radius:6px;padding:4px 7px;color:#d5e7fb;font-size:10px;outline:none}
.comp-model:focus{border-color:#62a8f7}
.model-picker-summary{font-size:10px;color:#6b8aa8}
.model-picker-popover{border:1px solid #2a4058;border-radius:6px;background:#0f1a28;padding:5px;display:flex;flex-direction:column;gap:3px;max-height:180px;overflow-y:auto}
.model-picker-title{font-size:10px;font-weight:600;color:#c8daf0}
.model-picker-hint{font-size:9px;color:#5d7a96}
.model-picker-option{display:flex;align-items:center;gap:4px;color:#a8c0d8;font-size:10px}
.model-picker-option input{accent-color:#62a8f7}
.comp-mode-row{display:flex;align-items:center;gap:4px;flex-wrap:nowrap;overflow-x:auto;overflow-y:hidden;padding-bottom:1px}
.comp-mode-chip{background:#151f2c;border:1px solid #2a3d52;color:#7a9ab8;border-radius:4px;padding:2px 5px;font-size:9px;line-height:1;cursor:pointer;text-transform:capitalize}
.comp-mode-chip:hover{border-color:#3d5a78;color:#a0c0e0}
.comp-mode-chip-active{background:#1a2a3c;border-color:#3d5a78;color:#a8d0f0}
.comp-runtime{font-size:9px;color:#5d7590;white-space:nowrap;margin-left:auto}
.composer{display:flex;gap:0;background:#101b2a;border:1px solid #35506d;border-radius:10px;overflow:hidden;transition:border-color .14s,box-shadow .14s}
.composer:focus-within{border-color:#62a8f7;box-shadow:0 0 0 3px rgba(98,168,247,.17)}
.comp-in{flex:1;background:transparent;border:none;padding:10px 12px;color:#e0ecf8;font-size:12px;outline:none;resize:none;min-height:40px;max-height:180px;font-family:inherit;line-height:1.35}
.comp-in:disabled{background:#0f1820;color:#4a6580}

.right{width:295px;min-width:295px;background:#131e2c;border-left:1px solid #263a52;padding:14px 12px;display:flex;flex-direction:column;gap:16px;overflow-y:auto}
.overview-head{display:flex;flex-direction:column;gap:5px;padding-bottom:4px;border-bottom:1px solid #223248}
.agent-row{display:flex;align-items:center;gap:8px;padding:5px 7px;border-radius:7px}
.agent-row:hover{background:#1b2d42}
.a-dot{width:9px;height:9px;border-radius:50%;flex-shrink:0}
.a-info{flex:1;display:flex;flex-direction:column}
.a-info strong{font-size:12px;color:#e0ecf8}
.a-meta{font-size:10px;color:#6f8da8}
.wf-step{font-size:13px;font-weight:600;color:#bdd4eb;padding:4px 0}
.checklist{display:flex;flex-direction:column;gap:3px}
.ck-row{display:flex;align-items:center;gap:7px;font-size:12px}
.ck{width:16px;height:16px;border-radius:3px;background:#1f3148;border:1px solid #3a5572;display:flex;align-items:center;justify-content:center;font-size:11px;color:#4a6580;flex-shrink:0}
.done{background:#2a5e44;border-color:#3d8b65;color:#9bf5c0}
.ck-label{color:#8ca4bd}
.ck-done{color:#b8e4cc;text-decoration:line-through}
.ev-row{display:flex;flex-direction:column;gap:1px;padding:3px 0;border-bottom:1px solid #1e3048}
.ev-actors{font-size:10px;color:#5b7d9a;font-weight:600}
.ev-msg{font-size:11px;color:#8ca4bd;line-height:1.3}

.floating-layer{position:fixed;inset:0;z-index:60;pointer-events:none}
.floating-frame{position:absolute;pointer-events:auto;display:flex;flex-direction:column;background:transparent}
.floating-resize-handle{position:absolute;z-index:1}
.floating-resize-n{top:0;left:8px;right:8px;height:6px;cursor:n-resize}
.floating-resize-s{bottom:0;left:8px;right:8px;height:6px;cursor:s-resize}
.floating-resize-e{right:0;top:8px;bottom:8px;width:6px;cursor:e-resize}
.floating-resize-w{left:0;top:8px;bottom:8px;width:6px;cursor:w-resize}
.floating-resize-ne{top:0;right:0;width:12px;height:12px;cursor:nesw-resize}
.floating-resize-nw{top:0;left:0;width:12px;height:12px;cursor:nwse-resize}
.floating-resize-se{bottom:0;right:0;width:14px;height:14px;cursor:nwse-resize;opacity:.8;background:linear-gradient(135deg,transparent 0%,transparent 40%,#6aa8eb 40%,#6aa8eb 50%,transparent 50%,transparent 100%)}
.floating-resize-sw{bottom:0;left:0;width:12px;height:12px;cursor:nesw-resize}
.floating-resize-handle:hover{background:rgba(106,168,235,.15)}
.floating-resize-se:hover{opacity:1;filter:brightness(1.1)}
.drag-scrim{position:fixed;inset:0;z-index:59;pointer-events:auto}
.is-dragging,.is-dragging *{user-select:none}

.detached-shell{min-height:100vh;background:#111c2a;color:#e4ecf5;display:flex;flex-direction:column;padding:14px;gap:10px}
.detached-head{display:flex;justify-content:space-between;gap:10px;align-items:flex-start}
.detached-title-wrap{display:flex;flex-direction:column;gap:3px}
.detached-title{font-size:22px;color:#f0f6ff}
.detached-sub{font-size:12px;color:#7f9bb7}
.detached-actions{display:flex;gap:5px;flex-wrap:wrap}
.detached-stream{flex:1;background:#141f2e;border:1px solid #283e56;border-radius:8px;padding:8px;overflow-y:auto;display:flex;flex-direction:column;gap:5px;min-height:200px}

.modal-overlay{position:fixed;top:0;left:0;right:0;bottom:0;background:rgba(6,12,22,.72);display:flex;align-items:center;justify-content:center;z-index:1000}
.modal{background:#1a2840;border:1px solid #3b5874;border-radius:14px;width:700px;max-height:80vh;display:flex;flex-direction:column;box-shadow:0 20px 60px rgba(0,0,0,.45)}
.confirm-modal{background:#1a2840;border:1px solid #3b5874;border-radius:12px;max-width:420px;width:calc(100% - 24px);padding:16px;display:flex;flex-direction:column;gap:10px;box-shadow:0 20px 60px rgba(0,0,0,.45)}
.confirm-title{font-size:16px;color:#f0f6ff}
.confirm-actions{display:flex;justify-content:flex-end;gap:8px}
.confirm-inline{background:#182739;border:1px solid #35506d;border-radius:10px;padding:10px;display:flex;flex-direction:column;gap:7px}
.confirm-inline-title{font-size:13px;color:#e6f1ff}
.modal-header{display:flex;align-items:center;justify-content:space-between;padding:16px 20px;border-bottom:1px solid #2c3f58}
.modal-header h2{font-size:20px;color:#f0f6ff}
.modal-close{background:#283d56;color:#b0c4da;border:1px solid #3b5874;border-radius:6px;width:28px;height:28px;cursor:pointer;font-size:14px;display:flex;align-items:center;justify-content:center}
.modal-close:hover{background:#3d556f;color:#fff}
.modal-body{display:flex;flex:1;min-height:0;overflow:hidden}
.tabs{display:flex;flex-direction:column;gap:2px;padding:14px 10px;border-right:1px solid #2c3f58;min-width:140px}
.tab{background:transparent;color:#8ca4bd;border:none;border-radius:7px;padding:8px 12px;text-align:left;cursor:pointer;font-size:13px}
.tab:hover{background:#1f3148;color:#d0e0f2}
.tab-active{background:#283d56;color:#f0f6ff;font-weight:600}
.tab-content{flex:1;padding:18px 22px;overflow-y:auto}
.tab-content h3{font-size:16px;color:#e4ecf5;margin-bottom:10px}
.setting-row{display:flex;flex-direction:column;gap:8px;margin-top:10px}
.s-input{background:#141f2e;border:1px solid #3a5572;border-radius:6px;padding:7px 10px;color:#e0ecf8;font-size:13px;outline:none}
.s-input:focus{border-color:#4a8dd4}

@media (max-width: 1150px){
  .right{display:none}
}

@media (max-width: 900px){
  .body{flex-direction:column}
  .left{width:100%;min-width:0;border-right:none;border-bottom:1px solid #263a52}
  .center{padding:12px}
  .card{width:100%}
  .top-meta{display:none}

  .composer{position:sticky;bottom:0;z-index:8;margin-top:8px;border-radius:12px}
  .comp-in{font-size:16px;min-height:44px}
  .btn-send{min-width:64px;min-height:44px}
  .composer-toolbar{gap:8px}
  .comp-model{max-width:none;min-width:0}
  .comp-runtime{margin-left:0}
  .workspace-taskbar{flex-direction:row;overflow-x:auto;padding-bottom:2px}
  .ws-pill{min-width:180px}
  .account-hero{padding:9px}

  .floating-layer{position:static;inset:auto;display:flex;flex-direction:column;gap:12px;pointer-events:auto;z-index:2;margin-top:12px}
  .floating-frame{position:relative !important;left:auto !important;top:auto !important;width:100% !important;height:auto !important;min-height:360px}
  .floating-resize-handle{display:none}
}

@media (pointer: coarse){
  .btn-xs{font-size:11px;padding:5px 8px;border-radius:6px}
  .card-actions{gap:4px}
  .toggle-btn{width:34px;height:34px}
}
"#;

fn bool_to_str(v: bool) -> &'static str {
    if v {
        "true"
    } else {
        "false"
    }
}

fn parse_bool(v: &str) -> bool {
    matches!(v, "true" | "1" | "on")
}

fn settings_to_patch(cfg: &KaizenSettings) -> SettingsPatchRequest {
    SettingsPatchRequest {
        runtime_engine: Some(cfg.runtime_engine.clone()),
        auto_spawn_subagents: Some(cfg.auto_spawn_subagents),
        max_subagents: Some(cfg.max_subagents),
        allow_direct_user_to_subagent_chat: Some(cfg.allow_direct_user_to_subagent_chat),
        hard_gates_enabled: Some(cfg.hard_gates_enabled),
        require_human_smoke_test_before_deploy: Some(cfg.require_human_smoke_test_before_deploy),
        provider_inference_only: Some(cfg.provider_inference_only),
        credentials_ui_enabled: Some(cfg.credentials_ui_enabled),
        oauth_ui_enabled: None,
        agent_name_editable_after_spawn: Some(cfg.agent_name_editable_after_spawn),
        show_only_masked_secrets_in_ui: Some(cfg.show_only_masked_secrets_in_ui),
        mattermost_url: Some(cfg.mattermost_url.clone()),
        mattermost_channel_id: Some(cfg.mattermost_channel_id.clone()),
        selected_github_repo: Some(cfg.selected_github_repo.clone()),
        inference_provider: Some(cfg.inference_provider.clone()),
        inference_model: Some(cfg.inference_model.clone()),
        inference_max_tokens: Some(cfg.inference_max_tokens),
        inference_temperature: Some(cfg.inference_temperature),
    }
}

async fn refresh_settings_bundle(
    t: Option<&str>,
    mut settings_current_sig: Signal<Option<KaizenSettings>>,
    mut settings_draft_sig: Signal<Option<KaizenSettings>>,
    mut vault_sig: Signal<Option<VaultStatus>>,
    mut secrets_sig: Signal<HashMap<String, SecretMetadata>>,
    mut gh_status_sig: Signal<Option<GitHubStatusResponse>>,
    mut gh_repos_sig: Signal<Vec<GitHubRepoSummary>>,
    mut oauth_sig: Signal<HashMap<String, OAuthStatusResponse>>,
    mut google_oauth_sig: Signal<Option<GoogleOAuthStatusResponse>>,
) -> Result<(), String> {
    let settings = fetch_settings_api(t).await?;
    settings_current_sig.set(Some(settings.clone()));
    let should_initialize_draft = settings_draft_sig.read().is_none();
    if should_initialize_draft {
        settings_draft_sig.set(Some(settings.clone()));
    }

    match fetch_vault_status_api(t).await {
        Ok(vault) => vault_sig.set(Some(vault)),
        Err(_) => vault_sig.set(None),
    }

    let secrets = fetch_secrets_api(t).await.unwrap_or_default();
    let secret_map = secrets
        .into_iter()
        .map(|meta| (meta.provider.clone(), meta))
        .collect::<HashMap<_, _>>();
    secrets_sig.set(secret_map);

    match fetch_github_status_api(t).await {
        Ok(status) => gh_status_sig.set(Some(status)),
        Err(_) => gh_status_sig.set(None),
    }

    match fetch_github_repos_api(t).await {
        Ok(response) => {
            if response.connected {
                gh_repos_sig.set(response.repos);
            } else {
                gh_repos_sig.set(Vec::new());
            }
        }
        Err(_) => gh_repos_sig.set(Vec::new()),
    }

    if settings.oauth_ui_enabled {
        let mut oauth_map = HashMap::new();
        for (provider, _) in OAUTH_PROVIDERS.iter() {
            if let Ok(status) = fetch_oauth_status_api(provider, t).await {
                oauth_map.insert(status.provider.clone(), status);
            }
        }
        oauth_sig.set(oauth_map);

        match fetch_google_oauth_status_api(t).await {
            Ok(status) => google_oauth_sig.set(Some(status)),
            Err(_) => google_oauth_sig.set(None),
        }
    } else {
        oauth_sig.set(HashMap::new());
        google_oauth_sig.set(None);
    }

    Ok(())
}

async fn refresh_detached_history(
    agent_id: Option<String>,
    token: Option<String>,
    mut history_sig: Signal<Vec<UiMsg>>,
    mut error_sig: Signal<Option<String>>,
    mut busy_sig: Signal<bool>,
) -> Result<(), String> {
    let already_busy = *busy_sig.read();
    if already_busy {
        return Ok(());
    }

    busy_sig.set(true);

    let result = fetch_chat_history(agent_id.as_deref(), token.as_deref()).await;

    busy_sig.set(false);

    match result {
        Ok(messages) => {
            history_sig.set(messages);
            error_sig.set(None);
            Ok(())
        }
        Err(e) => {
            error_sig.set(Some(e.clone()));
            Err(e)
        }
    }
}

async fn fetch_dashboard(t: Option<&str>) -> Result<Dashboard, String> {
    let client = Client::new();
    let agents = rj::<Vec<ApiAgent>>(client.get(u("/api/agents")), t).await?;
    let gates = rj::<GateSnapshot>(client.get(u("/api/gates")), t).await?;
    let events = rj::<Vec<CrystalBallEvent>>(client.get(u("/api/events?limit=150")), t)
        .await
        .unwrap_or_default();
    Ok(Dashboard {
        agents,
        gates,
        events,
    })
}

async fn fetch_settings_api(t: Option<&str>) -> Result<KaizenSettings, String> {
    rj(Client::new().get(u("/api/settings")), t).await
}

async fn patch_settings_api(
    cfg: &KaizenSettings,
    t: Option<&str>,
) -> Result<KaizenSettings, String> {
    let patch = settings_to_patch(cfg);
    rj(Client::new().patch(u("/api/settings")).json(&patch), t).await
}

async fn patch_selected_repo_api(repo: &str, t: Option<&str>) -> Result<(), String> {
    let patch = SettingsPatchRequest {
        runtime_engine: None,
        auto_spawn_subagents: None,
        max_subagents: None,
        allow_direct_user_to_subagent_chat: None,
        hard_gates_enabled: None,
        require_human_smoke_test_before_deploy: None,
        provider_inference_only: None,
        credentials_ui_enabled: None,
        oauth_ui_enabled: None,
        agent_name_editable_after_spawn: None,
        show_only_masked_secrets_in_ui: None,
        mattermost_url: None,
        mattermost_channel_id: None,
        selected_github_repo: Some(repo.to_string()),
        inference_provider: None,
        inference_model: None,
        inference_max_tokens: None,
        inference_temperature: None,
    };

    let response = ah(Client::new().patch(u("/api/settings")).json(&patch), t)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if response.status().is_success() {
        Ok(())
    } else {
        Err(format!(
            "{} {}",
            response.status(),
            response.text().await.unwrap_or_default()
        ))
    }
}

async fn fetch_vault_status_api(t: Option<&str>) -> Result<VaultStatus, String> {
    rj(Client::new().get(u("/api/vault/status")), t).await
}

async fn fetch_secrets_api(t: Option<&str>) -> Result<Vec<SecretMetadata>, String> {
    rj(Client::new().get(u("/api/secrets")), t).await
}

async fn store_secret_api(
    provider: &str,
    value: &str,
    t: Option<&str>,
) -> Result<SecretMetadata, String> {
    rj(
        Client::new()
            .put(u(&format!("/api/secrets/{provider}")))
            .json(&StoreSecretRequest {
                value,
                secret_type: "api_key",
            }),
        t,
    )
    .await
}

async fn test_secret_api(provider: &str, t: Option<&str>) -> Result<SecretTestResult, String> {
    rj(
        Client::new().post(u(&format!("/api/secrets/{provider}/test"))),
        t,
    )
    .await
}

async fn revoke_secret_api(provider: &str, t: Option<&str>) -> Result<(), String> {
    let response = ah(
        Client::new().delete(u(&format!("/api/secrets/{provider}"))),
        t,
    )
    .send()
    .await
    .map_err(|e| e.to_string())?;

    if response.status().is_success() {
        Ok(())
    } else {
        Err(format!(
            "{} {}",
            response.status(),
            response.text().await.unwrap_or_default()
        ))
    }
}

async fn fetch_github_status_api(t: Option<&str>) -> Result<GitHubStatusResponse, String> {
    rj(Client::new().get(u("/api/github/status")), t).await
}

async fn fetch_github_repos_api(t: Option<&str>) -> Result<GitHubReposResponse, String> {
    rj(Client::new().get(u("/api/github/repos?limit=100")), t).await
}

async fn fetch_oauth_status_api(
    provider: &str,
    t: Option<&str>,
) -> Result<OAuthStatusResponse, String> {
    rj(
        Client::new().get(u(&format!("/api/oauth/{provider}/status"))),
        t,
    )
    .await
}

async fn oauth_start_api(provider: &str, t: Option<&str>) -> Result<String, String> {
    let response = ah(
        Client::new().get(u(&format!("/api/oauth/{provider}/start"))),
        t,
    )
    .send()
    .await
    .map_err(|e| e.to_string())?;

    if response.status().is_success() {
        Ok(format!("OAuth start initiated for {provider}"))
    } else {
        Err(format!(
            "{} {}",
            response.status(),
            response.text().await.unwrap_or_default()
        ))
    }
}

async fn oauth_refresh_api(provider: &str, t: Option<&str>) -> Result<String, String> {
    let response = ah(
        Client::new().post(u(&format!("/api/oauth/{provider}/refresh"))),
        t,
    )
    .send()
    .await
    .map_err(|e| e.to_string())?;

    if response.status().is_success() {
        Ok(format!("OAuth refresh succeeded for {provider}"))
    } else {
        Err(format!(
            "{} {}",
            response.status(),
            response.text().await.unwrap_or_default()
        ))
    }
}

async fn oauth_disconnect_api(provider: &str, t: Option<&str>) -> Result<(), String> {
    let response = ah(
        Client::new().delete(u(&format!("/api/oauth/{provider}"))),
        t,
    )
    .send()
    .await
    .map_err(|e| e.to_string())?;

    if response.status().is_success() {
        Ok(())
    } else {
        Err(format!(
            "{} {}",
            response.status(),
            response.text().await.unwrap_or_default()
        ))
    }
}

async fn fetch_google_oauth_status_api(
    t: Option<&str>,
) -> Result<GoogleOAuthStatusResponse, String> {
    rj(Client::new().get(u("/api/webkeys/oauth/google/status")), t).await
}

async fn google_oauth_start_api(t: Option<&str>) -> Result<GoogleOAuthStartResponse, String> {
    let response = ah(Client::new().post(u("/api/webkeys/oauth/google/start")), t)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if response.status().is_success() {
        response
            .json::<GoogleOAuthStartResponse>()
            .await
            .map_err(|e| e.to_string())
    } else {
        Err(format!(
            "{} {}",
            response.status(),
            response.text().await.unwrap_or_default()
        ))
    }
}

async fn google_oauth_disconnect_account_api(
    account_id: &str,
    t: Option<&str>,
) -> Result<(), String> {
    let response = ah(
        Client::new().delete(u(&format!("/api/webkeys/oauth/google/{account_id}"))),
        t,
    )
    .send()
    .await
    .map_err(|e| e.to_string())?;

    if response.status().is_success() {
        Ok(())
    } else {
        Err(format!(
            "{} {}",
            response.status(),
            response.text().await.unwrap_or_default()
        ))
    }
}

async fn fetch_chat_history(agent_id: Option<&str>, t: Option<&str>) -> Result<Vec<UiMsg>, String> {
    let path = match agent_id {
        Some(id) => format!("/api/chat/history?agent_id={id}&limit=100"),
        None => "/api/chat/history?limit=100".to_string(),
    };
    let history = rj::<ChatHistoryResponse>(Client::new().get(u(&path)), t).await?;
    Ok(history
        .messages
        .into_iter()
        .map(|m| UiMsg::new(m.role, m.content))
        .collect())
}

async fn fetch_all_histories(agents: &[ApiAgent], t: Option<&str>) -> HashMap<String, Vec<UiMsg>> {
    let mut all = HashMap::new();
    if let Ok(history) = fetch_chat_history(None, t).await {
        all.insert("kaizen".to_string(), history);
    } else {
        all.insert("kaizen".to_string(), Vec::new());
    }

    for agent in agents {
        if let Ok(history) = fetch_chat_history(Some(&agent.id), t).await {
            all.insert(agent.id.clone(), history);
        } else {
            all.insert(agent.id.clone(), Vec::new());
        }
    }

    all
}

async fn send_chat(
    msg: &str,
    agent_id: Option<&str>,
    provider: Option<&str>,
    model: Option<&str>,
    mode: Option<&str>,
    selected_models: Option<Vec<ChatModelTarget>>,
    wrap_mode: Option<bool>,
    t: Option<&str>,
) -> Result<ChatResponse, String> {
    rj(
        Client::new().post(u("/api/chat")).json(&ChatRequest {
            message: msg,
            agent_id,
            clear_history: false,
            provider,
            model,
            mode,
            selected_models,
            wrap_mode,
        }),
        t,
    )
    .await
}

async fn spawn_agent_api(
    name: &str,
    tid: &str,
    obj: &str,
    t: Option<&str>,
) -> Result<ApiAgent, String> {
    rj(
        Client::new()
            .post(u("/api/agents"))
            .json(&SpawnAgentRequest {
                agent_name: name,
                task_id: tid,
                objective: obj,
                user_requested: true,
            }),
        t,
    )
    .await
}

async fn remove_agent_api(id: &str, t: Option<&str>) -> Result<(), String> {
    let r = ah(Client::new().delete(u(&format!("/api/agents/{id}"))), t)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if r.status().is_success() {
        Ok(())
    } else {
        Err(format!(
            "{} {}",
            r.status(),
            r.text().await.unwrap_or_default()
        ))
    }
}

async fn clear_agent_api(id: &str, t: Option<&str>) -> Result<(), String> {
    let r = ah(Client::new().post(u(&format!("/api/agents/{id}/clear"))), t)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if r.status().is_success() {
        Ok(())
    } else {
        Err(format!(
            "{} {}",
            r.status(),
            r.text().await.unwrap_or_default()
        ))
    }
}

async fn stop_agent_api(id: &str, t: Option<&str>) -> Result<ApiAgent, String> {
    rj(
        ah(Client::new().post(u(&format!("/api/agents/{id}/stop"))), t),
        t,
    )
    .await
}

async fn advance_gate(t: Option<&str>) -> Result<(), String> {
    let r = ah(Client::new().post(u("/api/gates/advance")), t)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if r.status().is_success() {
        Ok(())
    } else {
        Err(format!(
            "{} {}",
            r.status(),
            r.text().await.unwrap_or_default()
        ))
    }
}

async fn rj<T: for<'de> Deserialize<'de>>(
    req: RequestBuilder,
    t: Option<&str>,
) -> Result<T, String> {
    let r = ah(req, t).send().await.map_err(|e| e.to_string())?;
    if !r.status().is_success() {
        let s = r.status();
        let b = r.text().await.unwrap_or_default();
        return Err(format!("{s} {b}"));
    }
    r.json::<T>().await.map_err(|e| e.to_string())
}

fn ah(req: RequestBuilder, t: Option<&str>) -> RequestBuilder {
    if let Some(tok) = t {
        req.header("x-admin-token", tok)
    } else {
        req
    }
}

fn u(path: &str) -> String {
    format!("{API_BASE}{path}")
}

fn now_s() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
