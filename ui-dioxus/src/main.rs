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

fn native_detach_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| match std::env::var("KAIZEN_ENABLE_NATIVE_DETACH") {
        Ok(raw) => {
            let value = raw.trim().to_ascii_lowercase();
            matches!(value.as_str(), "1" | "true" | "yes" | "on")
        }
        Err(_) => false,
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
}

#[derive(Clone, Debug, Serialize)]
struct ChatRequest<'a> {
    message: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    agent_id: Option<&'a str>,
    clear_history: bool,
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
    mouse_start_x: f64,
    mouse_start_y: f64,
    panel_start_w: f64,
    panel_start_h: f64,
}

#[derive(Clone, Debug)]
struct PanelCardData {
    key: String,
    title: String,
    subtitle: String,
    agent: Option<ApiAgent>,
}

#[derive(Props, Clone, PartialEq)]
struct DetachedWindowProps {
    panel_key: String,
    title: String,
    subtitle: String,
    agent_id: Option<String>,
    admin_token: Option<String>,
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

    let mut left_open = use_signal(|| true);
    let mut settings_open = use_signal(|| false);
    let mut settings_tab = use_signal(|| SettingsTab::General);

    let mut panel_layouts = use_signal(load_layouts);
    let mut next_z = use_signal(|| 100u32);
    let mut detached_windows = use_signal(HashMap::<String, WeakDesktopContext>::new);
    let mut drag_state = use_signal(|| None::<DragState>);
    let mut resize_state = use_signal(|| None::<ResizeState>);

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
                                layout.mode = PanelMode::Docked;
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
    let selected_repo = settings_snap
        .as_ref()
        .map(|s| s.selected_github_repo.clone())
        .unwrap_or_default();

    let gate_label = g_snap
        .as_ref()
        .map(|g| g.current_state.label())
        .unwrap_or("Planning");
    let sidebar_open = *left_open.read();
    let show_settings = *settings_open.read();
    let active_tab = *settings_tab.read();

    let mut all_panels: Vec<PanelCardData> = vec![PanelCardData {
        key: "kaizen".to_string(),
        title: "Kaizen".to_string(),
        subtitle: format!("Primary planner | {gate_label}"),
        agent: None,
    }];
    for agent in &a_snap {
        all_panels.push(PanelCardData {
            key: agent.id.clone(),
            title: agent.name.clone(),
            subtitle: format!("{} | {}", agent.task_id, agent.status.label()),
            agent: Some(agent.clone()),
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
            class: "shell",
            onmousemove: {
                let mut drag_sig = drag_state;
                let mut resize_sig = resize_state;
                let mut layouts_sig = panel_layouts;
                move |evt: Event<MouseData>| {
                    let point = evt.data().client_coordinates();

                    if let Some(dragging) = drag_sig.read().clone() {
                        let dx = point.x - dragging.mouse_start_x;
                        let dy = point.y - dragging.mouse_start_y;
                        let mut layouts_map = layouts_sig.read().clone();
                        if let Some(layout) = layouts_map.get_mut(&dragging.panel_key) {
                            layout.x = (dragging.panel_start_x + dx).max(8.0);
                            layout.y = (dragging.panel_start_y + dy).max(52.0);
                        }
                        layouts_sig.set(layouts_map);
                    } else if let Some(resizing) = resize_sig.read().clone() {
                        let dx = point.x - resizing.mouse_start_x;
                        let dy = point.y - resizing.mouse_start_y;
                        let mut layouts_map = layouts_sig.read().clone();
                        if let Some(layout) = layouts_map.get_mut(&resizing.panel_key) {
                            layout.width = (resizing.panel_start_w + dx).max(320.0);
                            layout.height = (resizing.panel_start_h + dy).max(250.0);
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
                    h1 { class: "logo", "Kaizen MAX" }
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
                            div { class: "ws-item ws-active",
                                div { class: "ws-dot ws-dot-on" }
                                span { "Kaizen MAX" }
                            }
                            div { class: "ws-item",
                                div { class: "ws-dot" }
                                span { "Add workspace..." }
                            }
                        }
                        div { class: "sb-spacer" }
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
                                            ).await;
                                        });
                                    }
                                },
                                "Settings"
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
                                layout.mode,
                                &m_snap,
                                &d_snap,
                                &mut drafts,
                                &mut messages,
                                &mut info,
                                &mut error,
                                &admin_token,
                                &mut agents,
                                &mut gates,
                                &mut events,
                                &mut panel_layouts,
                                &mut next_z,
                                &mut detached_windows,
                                &mut drag_state,
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
                for (panel, layout) in floating_panels.iter() {
                    div {
                        class: "floating-frame",
                        key: "float-{panel.key}",
                        style: "left:{layout.x}px;top:{layout.y}px;width:{layout.width}px;height:{layout.height}px;z-index:{layout.z};",
                        {card(
                            &panel.key,
                            &panel.title,
                            &panel.subtitle,
                            panel.agent.as_ref(),
                            PanelMode::Floating,
                            &m_snap,
                            &d_snap,
                            &mut drafts,
                            &mut messages,
                            &mut info,
                            &mut error,
                            &admin_token,
                            &mut agents,
                            &mut gates,
                            &mut events,
                            &mut panel_layouts,
                            &mut next_z,
                            &mut detached_windows,
                            &mut drag_state,
                        )}
                        div {
                            class: "floating-resize",
                            onmousedown: {
                                let k = panel.key.clone();
                                let l = layout.clone();
                                let mut resize_sig = resize_state;
                                let mut z_sig = next_z;
                                let mut layouts_sig = panel_layouts;
                                move |evt: Event<MouseData>| {
                                    let point = evt.data().client_coordinates();
                                    let mut map = layouts_sig.read().clone();
                                    if let Some(existing) = map.get_mut(&k) {
                                        existing.z = bump_z(&mut z_sig);
                                    }
                                    layouts_sig.set(map);
                                    resize_sig.set(Some(ResizeState {
                                        panel_key: k.clone(),
                                        mouse_start_x: point.x,
                                        mouse_start_y: point.y,
                                        panel_start_w: l.width,
                                        panel_start_h: l.height,
                                    }));
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
                                            p { class: "sb-hint", "OAuth is enabled for OpenAI/Codex and Anthropic in this release." }
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

                                                label { "OAuth UI Enabled" }
                                                select {
                                                    class: "s-input",
                                                    value: "{bool_to_str(cfg.oauth_ui_enabled)}",
                                                    onchange: {
                                                        let mut settings_draft_sig = settings_draft;
                                                        move |e: Event<FormData>| {
                                                            let current_draft = settings_draft_sig.read().clone();
                                                            if let Some(mut draft) = current_draft {
                                                                draft.oauth_ui_enabled = parse_bool(&e.value());
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
                                let aid = aid.clone();
                                let token = token.clone();
                                let mut info_sig = info;
                                let mut error_sig = error;
                                move |_| {
                                    if let Some(id) = aid.clone() {
                                        let token = token.clone();
                                        spawn(async move {
                                            match stop_agent_api(&id, token.as_deref()).await {
                                                Ok(_) => {
                                                    info_sig.set(Some("Agent stopped".into()));
                                                    error_sig.set(None);
                                                }
                                                Err(e) => error_sig.set(Some(e)),
                                            }
                                        });
                                    }
                                }
                            },
                            "Stop"
                        }
                        button {
                            class: "btn btn-xs btn-sec",
                            onclick: {
                                let aid = aid.clone();
                                let token = token.clone();
                                let mut info_sig = info;
                                let mut error_sig = error;
                                let mut history_sig = history;
                                move |_| {
                                    history_sig.set(Vec::new());
                                    if let Some(id) = aid.clone() {
                                        let token = token.clone();
                                        spawn(async move {
                                            match clear_agent_api(&id, token.as_deref()).await {
                                                Ok(()) => {
                                                    info_sig.set(Some("Chat cleared".into()));
                                                    error_sig.set(None);
                                                }
                                                Err(e) => error_sig.set(Some(e)),
                                            }
                                        });
                                    }
                                }
                            },
                            "Clear"
                        }
                        button {
                            class: "btn btn-xs btn-danger",
                            onclick: {
                                let aid = aid.clone();
                                let token = token.clone();
                                let mut error_sig = error;
                                move |_| {
                                    if let Some(id) = aid.clone() {
                                        let token = token.clone();
                                        spawn(async move {
                                            match remove_agent_api(&id, token.as_deref()).await {
                                                Ok(()) => window().close(),
                                                Err(e) => error_sig.set(Some(e)),
                                            }
                                        });
                                    }
                                }
                            },
                            "X"
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

            div { class: "detached-stream",
                if msg_snap.is_empty() {
                    p { class: "stream-empty", "No messages yet." }
                }
                for line in msg_snap.iter().rev().take(20).rev() {
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

            div { class: "composer",
                input {
                    r#type: "text",
                    class: "comp-in",
                    value: draft_val,
                    placeholder: "Message...",
                    oninput: move |e: Event<FormData>| draft.set(e.value()),
                }
                button {
                    class: "btn btn-send",
                    disabled: is_history_busy,
                    onclick: {
                        let aid = aid.clone();
                        let token = token.clone();
                        let mut draft_sig = draft;
                        let mut history_sig = history;
                        let mut info_sig = info;
                        let mut error_sig = error;
                        let mut busy_sig = history_busy;
                        let title_for_line = title.clone();
                        move |_| {
                            let currently_busy = *busy_sig.read();
                            if currently_busy {
                                return;
                            }

                            let text = draft_sig.read().trim().to_string();
                            if text.is_empty() {
                                return;
                            }

                            draft_sig.set(String::new());
                            history_sig.write().push(UiMsg::new("user", text.clone()));

                            let aid = aid.clone();
                            let token = token.clone();
                            let title_for_line = title_for_line.clone();
                            spawn(async move {
                                match send_chat(&text, aid.as_deref(), token.as_deref()).await {
                                    Ok(reply) => {
                                        history_sig
                                            .write()
                                            .push(UiMsg::new("assistant", reply.reply));
                                        info_sig.set(Some("Reply received".into()));
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
                    "Send"
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
    mode: PanelMode,
    m_snap: &HashMap<String, Vec<UiMsg>>,
    d_snap: &HashMap<String, String>,
    drafts: &mut Signal<HashMap<String, String>>,
    messages: &mut Signal<HashMap<String, Vec<UiMsg>>>,
    info: &mut Signal<Option<String>>,
    error: &mut Signal<Option<String>>,
    admin_token: &Option<String>,
    agents: &mut Signal<Vec<ApiAgent>>,
    gates: &mut Signal<Option<GateSnapshot>>,
    events: &mut Signal<Vec<CrystalBallEvent>>,
    layouts: &mut Signal<HashMap<String, PanelLayout>>,
    next_z: &mut Signal<u32>,
    detached_windows: &mut Signal<HashMap<String, WeakDesktopContext>>,
    drag_state: &mut Signal<Option<DragState>>,
) -> Element {
    let msgs = m_snap.get(key).cloned().unwrap_or_default();
    let draft = d_snap.get(key).cloned().unwrap_or_default();
    let k = key.to_string();
    let t = title.to_string();
    let subtitle_owned = subtitle.to_string();
    let aid = agent.map(|a| a.id.clone());
    let can_send = key == "kaizen" || aid.is_some();
    let has_agent = agent.is_some();
    let detach_supported = native_detach_enabled();

    let mut d_sig = *drafts;
    let mut m_sig = *messages;
    let mut i_sig = *info;
    let mut e_sig = *error;
    let mut a_sig = *agents;
    let mut g_sig = *gates;
    let mut ev_sig = *events;
    let mut l_sig = *layouts;
    let mut z_sig = *next_z;
    let mut dw_sig = *detached_windows;
    let mut drag_sig = *drag_state;
    let token = admin_token.clone();

    let card_class = if mode == PanelMode::Floating {
        "card floating-card"
    } else {
        "card"
    };

    rsx! {
        div { class: "{card_class}", key: "{k}",
            div {
                class: if mode == PanelMode::Floating { "card-head card-head-drag" } else { "card-head" },
                h2 {
                    class: "card-title",
                    onmousedown: {
                        let k = k.clone();
                        let mut drag_sig = drag_sig;
                        let mut z_sig = z_sig;
                        let mut l_sig = l_sig;
                        move |evt: Event<MouseData>| {
                            if mode != PanelMode::Floating {
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
                    "{t}"
                }
                div { class: "card-actions",
                    button {
                        class: if mode == PanelMode::Docked { "btn btn-xs btn-accent" } else { "btn btn-xs btn-sec" },
                        title: "Dock this panel",
                        onclick: {
                            let k = k.clone();
                            let mut l_sig = l_sig;
                            let mut z_sig = z_sig;
                            let mut dw_sig = dw_sig;
                            move |_| {
                                if let Some(handle) = dw_sig.write().remove(&k) {
                                    if let Some(win) = handle.upgrade() {
                                        win.close();
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
                        "Dock"
                    }
                    button {
                        class: if mode == PanelMode::Floating { "btn btn-xs btn-accent" } else { "btn btn-xs btn-sec" },
                        title: "Float this panel",
                        onclick: {
                            let k = k.clone();
                            let mut l_sig = l_sig;
                            let mut z_sig = z_sig;
                            move |_| {
                                let mut map = l_sig.read().clone();
                                let layout = map.entry(k.clone()).or_insert_with(|| default_panel_layout(0));
                                layout.mode = PanelMode::Floating;
                                layout.width = layout.width.max(360.0);
                                layout.height = layout.height.max(280.0);
                                layout.z = bump_z(&mut z_sig);
                                l_sig.set(map.clone());
                                persist_layouts(&map);
                            }
                        },
                        "Float"
                    }
                    button {
                        class: if mode == PanelMode::Detached { "btn btn-xs btn-accent" } else { "btn btn-xs btn-sec" },
                        title: if detach_supported { "Detach to native window" } else { "Native detach disabled, switching to floating mode" },
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
                            move |_| {
                                append_ui_diagnostic(&format!("detach requested key={k}"));

                                if !detach_supported {
                                    let mut map = l_sig.read().clone();
                                    let layout =
                                        map.entry(k.clone()).or_insert_with(|| default_panel_layout(0));
                                    layout.mode = PanelMode::Floating;
                                    layout.width = layout.width.max(420.0);
                                    layout.height = layout.height.max(320.0);
                                    layout.z = bump_z(&mut z_sig);
                                    l_sig.set(map.clone());
                                    persist_layouts(&map);
                                    e_sig.set(Some(
                                        "Native detach is disabled for stability. Switched to floating mode."
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
                                            },
                                        );

                                        let cfg = DesktopConfig::new().with_window(
                                            WindowBuilder::new()
                                                .with_title(format!("Kaizen MAX | {title}"))
                                                .with_inner_size(LogicalSize::new(
                                                    spawn_layout.width.max(420.0),
                                                    spawn_layout.height.max(320.0),
                                                ))
                                                .with_position(LogicalPosition::new(
                                                    spawn_layout.x.max(20.0),
                                                    spawn_layout.y.max(20.0),
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
                        "Detach"
                    }

                    if has_agent {
                        button {
                            class: "btn btn-xs btn-warn",
                            title: "Stop agent",
                            onclick: {
                                let aid = aid.clone();
                                let token = token.clone();
                                let mut i_sig = i_sig;
                                let mut e_sig = e_sig;
                                let mut a_sig = a_sig;
                                let mut g_sig = g_sig;
                                let mut ev_sig = ev_sig;
                                let mut m_sig = m_sig;
                                let mut l_sig = l_sig;
                                let mut z_sig = z_sig;
                                let mut dw_sig = dw_sig;
                                move |_| {
                                    if let Some(id) = aid.clone() {
                                        let token = token.clone();
                                        spawn(async move {
                                            match stop_agent_api(&id, token.as_deref()).await {
                                                Ok(_) => {
                                                    i_sig.set(Some("Agent stopped".into()));
                                                    e_sig.set(None);
                                                }
                                                Err(e) => {
                                                    e_sig.set(Some(e));
                                                    return;
                                                }
                                            }

                                            if let Err(e) = refresh_dashboard_state(
                                                token,
                                                a_sig,
                                                g_sig,
                                                ev_sig,
                                                m_sig,
                                                l_sig,
                                                z_sig,
                                                dw_sig,
                                            )
                                            .await
                                            {
                                                e_sig.set(Some(e));
                                            }
                                        });
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
                                let token = token.clone();
                                let k2 = k.clone();
                                let mut i_sig = i_sig;
                                let mut e_sig = e_sig;
                                let mut m_sig = m_sig;
                                move |_| {
                                    m_sig.write().insert(k2.clone(), Vec::new());

                                    if let Some(id) = aid.clone() {
                                        let token = token.clone();
                                        spawn(async move {
                                            match clear_agent_api(&id, token.as_deref()).await {
                                                Ok(()) => {
                                                    i_sig.set(Some("Chat cleared".into()));
                                                    e_sig.set(None);
                                                }
                                                Err(e) => e_sig.set(Some(e)),
                                            }
                                        });
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
                                let token = token.clone();
                                let mut i_sig = i_sig;
                                let mut e_sig = e_sig;
                                let mut a_sig = a_sig;
                                let mut g_sig = g_sig;
                                let mut ev_sig = ev_sig;
                                let mut m_sig = m_sig;
                                let mut l_sig = l_sig;
                                let mut z_sig = z_sig;
                                let mut dw_sig = dw_sig;
                                move |_| {
                                    if let Some(id) = aid.clone() {
                                        let token = token.clone();
                                        spawn(async move {
                                            match remove_agent_api(&id, token.as_deref()).await {
                                                Ok(()) => {
                                                    i_sig.set(Some("Agent removed".into()));
                                                    e_sig.set(None);
                                                }
                                                Err(e) => {
                                                    e_sig.set(Some(e));
                                                    return;
                                                }
                                            }

                                            if let Err(e) = refresh_dashboard_state(
                                                token,
                                                a_sig,
                                                g_sig,
                                                ev_sig,
                                                m_sig,
                                                l_sig,
                                                z_sig,
                                                dw_sig,
                                            )
                                            .await
                                            {
                                                e_sig.set(Some(e));
                                            }
                                        });
                                    }
                                }
                            },
                            "X"
                        }
                    }
                }
            }

            p { class: "card-sub", "{subtitle}" }
            p { class: "mode-chip", "Mode: {mode.label()}" }

            div { class: "card-stream",
                if msgs.is_empty() {
                    p { class: "stream-empty", if can_send { "No messages yet." } else { "Spawn to chat." } }
                }
                for line in msgs.iter().rev().take(12).rev() {
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

            div { class: "composer",
                input {
                    r#type: "text",
                    class: "comp-in",
                    disabled: !can_send,
                    value: draft,
                    placeholder: if can_send { "Message..." } else { "Not active" },
                    oninput: {
                        let k = k.clone();
                        move |e: Event<FormData>| {
                            d_sig.write().insert(k.clone(), e.value());
                        }
                    },
                }
                button {
                    class: "btn btn-send",
                    disabled: !can_send,
                    onclick: {
                        let k = k.clone();
                        let aid = aid.clone();
                        let token = token.clone();
                        let t = t.clone();
                        let mut d_sig = d_sig;
                        let mut m_sig = m_sig;
                        let mut i_sig = i_sig;
                        let mut e_sig = e_sig;
                        move |_| {
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

                            d_sig.write().insert(k.clone(), String::new());
                            m_sig.write().entry(k.clone()).or_default().push(UiMsg::new("user", text.clone()));
                            e_sig.set(None);

                            let aid = aid.clone();
                            let token = token.clone();
                            let k_hist = k.clone();
                            let t = t.clone();
                            spawn(async move {
                                match send_chat(&text, aid.as_deref(), token.as_deref()).await {
                                    Ok(resp) => {
                                        m_sig
                                            .write()
                                            .entry(k_hist.clone())
                                            .or_default()
                                            .push(UiMsg::new("assistant", resp.reply));
                                        i_sig.set(Some("Reply received".into()));

                                        if let Ok(history) = fetch_chat_history(aid.as_deref(), token.as_deref()).await {
                                            m_sig.write().insert(k_hist.clone(), history);
                                        }
                                    }
                                    Err(e) => {
                                        e_sig.set(Some(e.clone()));
                                        m_sig.write().entry(k_hist).or_default().push(UiMsg::new(
                                            "system",
                                            format!("Error from {t}: {e}"),
                                        ));
                                    }
                                }
                            });
                        }
                    },
                    "Send"
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
            if layout.width < 320.0 {
                layout.width = 320.0;
                changed = true;
            }
            if layout.height < 250.0 {
                layout.height = 250.0;
                changed = true;
            }
            if layout.x < 8.0 {
                layout.x = 8.0;
                changed = true;
            }
            if layout.y < 52.0 {
                layout.y = 52.0;
                changed = true;
            }

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

    for layout in parsed.values_mut() {
        if layout.mode == PanelMode::Detached {
            layout.mode = PanelMode::Docked;
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

    let mut map = detached.write();
    for key in stale {
        if let Some(handle) = map.remove(&key) {
            if let Some(win) = handle.upgrade() {
                win.close();
            }
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
.top-left{display:flex;align-items:center;gap:12px}
.top-right{display:flex;align-items:center;gap:8px}
.toggle-btn{background:#233448;color:#8da5be;border:1px solid #35506e;border-radius:6px;width:26px;height:26px;cursor:pointer;font-size:13px;display:flex;align-items:center;justify-content:center}
.toggle-btn:hover{background:#2f4560;color:#c8daf0}
.logo{font-size:24px;font-weight:700;color:#f0f6ff}
.top-meta{font-size:12px;color:#8ca4bd}

.btn{border:none;border-radius:8px;cursor:pointer;font-size:13px;font-weight:500;padding:7px 13px;transition:background .12s,transform .06s}
.btn:hover{filter:brightness(1.12)}
.btn:active{transform:scale(.97)}
.btn-sec{background:#283d56;color:#d0e0f2;border:1px solid #3b5874}
.btn-accent{background:#3d8b65;color:#effff6}
.btn-send{background:#4389e0;color:#fff;border-radius:0 8px 8px 0;padding:7px 14px}
.btn-send:disabled{background:#2c3f55;color:#5d7a96;cursor:default}
.btn-sm{font-size:11px;padding:5px 9px}
.btn-xs{font-size:10px;padding:3px 7px;border-radius:5px}
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

.center{flex:1;padding:16px;overflow-y:auto;background:#111c2a;position:relative}
.grid{display:flex;flex-wrap:wrap;gap:14px;align-items:flex-start;position:relative;z-index:1}

.card{width:320px;min-height:340px;background:#1b2a3d;border:1px solid #314a66;border-radius:12px;padding:12px;display:flex;flex-direction:column;gap:8px;transition:border-color .12s,box-shadow .12s;overflow:hidden}
.card:hover{border-color:#4a8dd4;box-shadow:0 4px 18px rgba(74,141,212,.10)}
.floating-card{width:100%;height:100%;min-height:0}
.card-add{border:2px dashed #384f6a;background:transparent;cursor:pointer;display:flex;align-items:center;justify-content:center;min-height:340px}
.card-add:hover{border-color:#5a8abf;background:rgba(90,138,191,.05)}
.add-inner{display:flex;flex-direction:column;align-items:center;gap:8px;color:#5a7d9e;font-size:14px}
.add-icon{font-size:34px;color:#4a7aa6}
.card-head{display:flex;align-items:center;justify-content:space-between;gap:8px}
.card-head-drag{cursor:move}
.card-title{font-size:18px;font-weight:700;color:#f2f8ff;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
.card-actions{display:flex;gap:4px;flex-wrap:wrap;justify-content:flex-end}
.card-sub{font-size:11px;color:#7a99b5;margin-top:-2px}
.mode-chip{font-size:10px;color:#9dc3e5;background:#203247;border:1px solid #35506d;border-radius:999px;padding:2px 8px;display:inline-flex;align-self:flex-start}
.card-stream{flex:1;background:#141f2e;border:1px solid #283e56;border-radius:8px;padding:8px;overflow-y:auto;min-height:100px;max-height:220px;display:flex;flex-direction:column;gap:5px}
.stream-empty{color:#5b7d9a;font-size:12px}
.msg{padding:5px 9px;border-radius:8px;font-size:12px;line-height:1.4;max-width:95%}
.msg-you{background:#253d58;color:#d8e8f8;align-self:flex-end;border-bottom-right-radius:2px}
.msg-ai{background:#1e3048;color:#c8ddf0;align-self:flex-start;border-bottom-left-radius:2px}
.msg-role{font-weight:600;margin-right:5px;font-size:10px;color:#8aafcc}
.composer{display:flex;gap:0}
.comp-in{flex:1;background:#141f2e;border:1px solid #3a5572;border-right:none;border-radius:8px 0 0 8px;padding:8px 10px;color:#e0ecf8;font-size:12px;outline:none}
.comp-in:focus{border-color:#4a8dd4}
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
.floating-resize{position:absolute;right:3px;bottom:3px;width:14px;height:14px;cursor:nwse-resize;background:linear-gradient(135deg,transparent 0%,transparent 45%,#4a8dd4 45%,#4a8dd4 55%,transparent 55%,transparent 100%)}

.detached-shell{min-height:100vh;background:#111c2a;color:#e4ecf5;display:flex;flex-direction:column;padding:14px;gap:10px}
.detached-head{display:flex;justify-content:space-between;gap:10px;align-items:flex-start}
.detached-title-wrap{display:flex;flex-direction:column;gap:3px}
.detached-title{font-size:22px;color:#f0f6ff}
.detached-sub{font-size:12px;color:#7f9bb7}
.detached-actions{display:flex;gap:5px;flex-wrap:wrap}
.detached-stream{flex:1;background:#141f2e;border:1px solid #283e56;border-radius:8px;padding:8px;overflow-y:auto;display:flex;flex-direction:column;gap:5px;min-height:200px}

.modal-overlay{position:fixed;top:0;left:0;right:0;bottom:0;background:rgba(6,12,22,.72);display:flex;align-items:center;justify-content:center;z-index:1000}
.modal{background:#1a2840;border:1px solid #3b5874;border-radius:14px;width:700px;max-height:80vh;display:flex;flex-direction:column;box-shadow:0 20px 60px rgba(0,0,0,.45)}
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
        oauth_ui_enabled: Some(cfg.oauth_ui_enabled),
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
) -> Result<(), String> {
    let settings = fetch_settings_api(t).await?;
    settings_current_sig.set(Some(settings.clone()));
    let should_initialize_draft = settings_draft_sig.read().is_none();
    if should_initialize_draft {
        settings_draft_sig.set(Some(settings));
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

    let mut oauth_map = HashMap::new();
    for (provider, _) in OAUTH_PROVIDERS.iter() {
        if let Ok(status) = fetch_oauth_status_api(provider, t).await {
            oauth_map.insert(status.provider.clone(), status);
        }
    }
    oauth_sig.set(oauth_map);

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
    t: Option<&str>,
) -> Result<ChatResponse, String> {
    rj(
        Client::new().post(u("/api/chat")).json(&ChatRequest {
            message: msg,
            agent_id,
            clear_history: false,
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
