use gloo_timers::future::TimeoutFuture;
use leptos::ev;
use leptos::html;
use leptos::*;
use leptos_router::*;
use pulldown_cmark::{html::push_html, Event, Options, Parser};
use serde_json::{json, Map, Value};
use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::rc::Rc;
use std::time::Duration;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;

use crate::models::types::*;

const ADMIN_TOKEN_KEY: &str = "KAIZEN_ADMIN_TOKEN";

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(catch, js_namespace = ["window", "__TAURI__", "core"], js_name = invoke)]
    async fn tauri_invoke(cmd: &str, args: JsValue) -> Result<JsValue, JsValue>;
}

fn js_error(value: JsValue) -> String {
    value
        .as_string()
        .unwrap_or_else(|| "tauri invoke rejected".to_string())
}

fn browser_storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok().flatten()
}

fn load_admin_token() -> String {
    browser_storage()
        .and_then(|storage| storage.get_item(ADMIN_TOKEN_KEY).ok().flatten())
        .unwrap_or_default()
}

fn persist_admin_token(value: &str) {
    if let Some(storage) = browser_storage() {
        if value.trim().is_empty() {
            let _ = storage.remove_item(ADMIN_TOKEN_KEY);
        } else {
            let _ = storage.set_item(ADMIN_TOKEN_KEY, value);
        }
    }
}

fn token_opt(token: &str) -> Option<String> {
    let value = token.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn bool_patch(field: &str, value: bool) -> Value {
    let mut map = Map::new();
    map.insert(field.to_string(), Value::Bool(value));
    Value::Object(map)
}

fn render_markdown(content: &str) -> String {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_TASKLISTS);

    let parser = Parser::new_ext(content, options).filter_map(|event| match event {
        Event::Html(_) | Event::InlineHtml(_) => None,
        _ => Some(event),
    });

    let mut rendered = String::new();
    push_html(&mut rendered, parser);

    let rendered = wrap_code_blocks_with_copy_controls(&rendered);

    if rendered.trim().is_empty() {
        format!("<p>{}</p>", content)
    } else {
        rendered
    }
}

async fn detach_agent(agent_id: String) {
    let args = json!({ "agent_id": agent_id });
    if let Ok(js_args) = serde_wasm_bindgen::to_value(&args) {
        let _ = tauri_invoke("open_agent_window", js_args).await;
    }
}

async fn focus_agent(agent_id: String) {
    let args = json!({ "agent_id": agent_id });
    if let Ok(js_args) = serde_wasm_bindgen::to_value(&args) {
        let _ = tauri_invoke("focus_agent_window", js_args).await;
    }
}

async fn open_external_browser(url: String) -> Result<(), String> {
    let args = json!({ "url": url });
    let js_args = serde_wasm_bindgen::to_value(&args)
        .map_err(|error| format!("failed to serialize open_external_url args: {error}"))?;
    tauri_invoke("open_external_url", js_args)
        .await
        .map_err(js_error)?;
    Ok(())
}

fn status_class(status: &AgentStatus) -> &'static str {
    match status {
        AgentStatus::Idle => "idle",
        AgentStatus::Active => "active",
        AgentStatus::Blocked => "blocked",
        AgentStatus::ReviewPending => "review",
        AgentStatus::Done => "done",
    }
}

fn status_label(status: &AgentStatus) -> &'static str {
    match status {
        AgentStatus::Idle => "Idle",
        AgentStatus::Active => "Active",
        AgentStatus::Blocked => "Blocked",
        AgentStatus::ReviewPending => "Review Pending",
        AgentStatus::Done => "Done",
    }
}

fn tab_label(tab: &TabId) -> &'static str {
    match tab {
        TabId::Mission => "Mission",
        TabId::Branches => "Branches",
        TabId::Gates => "Gates",
        TabId::Activity => "Activity",
        TabId::Memory => "Memory",
        TabId::Calendar => "Calendar",
        TabId::Kanban => "Kanban",
        TabId::Workspace => "Workspace",
        TabId::Integrations => "Integrations",
        TabId::Settings => "Settings",
    }
}

fn gate_label(state: &GateState) -> &'static str {
    match state {
        GateState::Plan => "Plan",
        GateState::Execute => "Execute",
        GateState::Review => "Review",
        GateState::HumanSmokeTest => "Human Smoke Test",
        GateState::Deploy => "Deploy",
        GateState::Complete => "Complete",
    }
}

fn compact_time(raw: &str) -> String {
    if let Some((_, right)) = raw.split_once('T') {
        right.trim_end_matches('Z').chars().take(12).collect()
    } else {
        raw.chars().take(12).collect()
    }
}

fn sender_label(role: &str) -> &'static str {
    match role {
        "assistant" | "agent" => "Kaizen",
        "user" => "Operator",
        _ => "System",
    }
}

fn branch_label(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        "primary".to_string()
    } else {
        trimmed.to_string()
    }
}

fn mission_label(agent: &SubAgent) -> String {
    let mission = agent.mission_id.trim();
    if mission.is_empty() {
        let task = agent.task_id.trim();
        if task.is_empty() {
            "general".to_string()
        } else {
            task.to_string()
        }
    } else {
        mission.to_string()
    }
}

fn grouped_branches(agents: &[SubAgent]) -> BTreeMap<String, BTreeMap<String, Vec<SubAgent>>> {
    let mut map = BTreeMap::<String, BTreeMap<String, Vec<SubAgent>>>::new();
    for agent in agents {
        let branch = branch_label(&agent.branch_id);
        let mission = mission_label(agent);
        map.entry(branch)
            .or_default()
            .entry(mission)
            .or_default()
            .push(agent.clone());
    }
    map
}

#[derive(Clone, PartialEq)]
struct ActivityEventRow {
    event: CrystalBallEvent,
    branch_id: String,
    mission_id: String,
}

#[derive(Clone, PartialEq)]
struct JournalEntryRow {
    task_id: String,
    branch_id: String,
    mission_id: String,
    first_timestamp: String,
    last_timestamp: String,
    event_count: usize,
    highlights: Vec<String>,
}

#[derive(Clone, PartialEq)]
struct ScheduledTaskRow {
    slot: String,
    branch_id: String,
    mission_id: String,
    worker_name: String,
    objective: String,
    directive: String,
}

#[derive(Clone, PartialEq)]
struct MissionKanbanCard {
    branch_id: String,
    mission_id: String,
    workers: Vec<SubAgent>,
    active_count: usize,
    blocked_count: usize,
    review_count: usize,
    done_count: usize,
    lane: &'static str,
}

fn normalize_scope_key(value: &str) -> String {
    value
        .trim()
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

fn event_timestamp_seconds(raw: &str) -> Option<f64> {
    raw.trim().parse::<f64>().ok()
}

fn iso_timestamp(raw: &str) -> Option<String> {
    let seconds = event_timestamp_seconds(raw)?;
    js_sys::Date::new(&JsValue::from_f64(seconds * 1000.0))
        .to_iso_string()
        .as_string()
}

fn day_bucket_label(raw: &str) -> String {
    iso_timestamp(raw)
        .map(|iso| iso.chars().take(10).collect::<String>())
        .unwrap_or_else(|| raw.chars().take(10).collect())
}

fn time_bucket_label(raw: &str) -> String {
    if let Some(iso) = iso_timestamp(raw) {
        if let Some((_, right)) = iso.split_once('T') {
            return right.chars().take(5).collect();
        }
    }
    compact_time(raw)
}

fn infer_event_scope(event: &CrystalBallEvent, agents: &[SubAgent]) -> (String, String) {
    let find_by_id = |id: &str| {
        let trimmed = id.trim();
        if trimmed.is_empty() {
            None
        } else {
            agents.iter().find(|agent| agent.id == trimmed)
        }
    };

    if let Some(agent) =
        find_by_id(&event.source_agent_id).or_else(|| find_by_id(&event.target_agent_id))
    {
        return (branch_label(&agent.branch_id), mission_label(agent));
    }

    let source_actor = event.source_actor.trim();
    let target_actor = event.target_actor.trim();
    if let Some(agent) = agents.iter().find(|agent| {
        (!source_actor.is_empty() && agent.name.eq_ignore_ascii_case(source_actor))
            || (!target_actor.is_empty() && agent.name.eq_ignore_ascii_case(target_actor))
    }) {
        return (branch_label(&agent.branch_id), mission_label(agent));
    }

    let task_key = normalize_scope_key(&event.task_id);
    if !task_key.is_empty() {
        if let Some(agent) = agents.iter().find(|agent| {
            normalize_scope_key(&agent.mission_id) == task_key
                || normalize_scope_key(&agent.task_id) == task_key
        }) {
            return (branch_label(&agent.branch_id), mission_label(agent));
        }

        let mission = if matches!(task_key.as_str(), "chat" | "settings" | "smoke") {
            "general".to_string()
        } else {
            task_key
        };
        return ("primary".to_string(), mission);
    }

    ("primary".to_string(), "general".to_string())
}

fn mission_lane_for_workers(workers: &[SubAgent]) -> &'static str {
    if workers.is_empty() {
        "backlog"
    } else if workers
        .iter()
        .all(|worker| matches!(worker.status, AgentStatus::Done))
    {
        "done"
    } else if workers
        .iter()
        .any(|worker| matches!(worker.status, AgentStatus::ReviewPending))
    {
        "review"
    } else if workers
        .iter()
        .any(|worker| matches!(worker.status, AgentStatus::Active | AgentStatus::Blocked))
    {
        "in_progress"
    } else {
        "backlog"
    }
}

fn polar_percent(index: usize, total: usize, radius: f64) -> (f64, f64) {
    if total <= 1 {
        return (50.0, 50.0);
    }

    let angle = (index as f64 / total as f64) * std::f64::consts::TAU - std::f64::consts::FRAC_PI_2;
    (50.0 + radius * angle.cos(), 50.0 + radius * angle.sin())
}

fn mission_pod_style(index: usize, total: usize) -> String {
    let (x, y) = polar_percent(index, total.max(1), 34.0);
    format!("left: {:.1}%; top: {:.1}%;", x, y)
}

fn worker_seat_style(index: usize, total: usize) -> String {
    let radius = if total <= 1 {
        0.0
    } else if total <= 3 {
        20.0
    } else {
        25.0
    };
    let (x, y) = polar_percent(index, total.max(1), radius);
    format!("left: {:.1}%; top: {:.1}%;", x, y)
}

fn wrap_code_blocks_with_copy_controls(rendered: &str) -> String {
    const OPEN_BLOCK: &str = "<pre><code";
    const CLOSE_BLOCK: &str = "</code></pre>";

    let mut output = String::with_capacity(rendered.len() + 192);
    let mut cursor = 0usize;

    while let Some(relative_start) = rendered[cursor..].find(OPEN_BLOCK) {
        let block_start = cursor + relative_start;
        output.push_str(&rendered[cursor..block_start]);

        let Some(relative_end) = rendered[block_start..].find(CLOSE_BLOCK) else {
            output.push_str(&rendered[block_start..]);
            return output;
        };

        let block_end = block_start + relative_end + CLOSE_BLOCK.len();
        output.push_str(
            "<div class=\"code-block-shell\"><button type=\"button\" class=\"code-copy-btn\" data-copy-state=\"idle\">Copy</button>",
        );
        output.push_str(&rendered[block_start..block_end]);
        output.push_str("</div>");

        cursor = block_end;
    }

    output.push_str(&rendered[cursor..]);
    output
}

fn set_copy_button_state(button: &web_sys::Element, label: &str, state: &str) {
    button.set_text_content(Some(label));
    let _ = button.set_attribute("data-copy-state", state);
}

fn handle_markdown_copy_click(event: web_sys::MouseEvent) {
    let Some(target) = event.target() else {
        return;
    };

    let Ok(target_element) = target.dyn_into::<web_sys::Element>() else {
        return;
    };

    let button = if target_element.class_list().contains("code-copy-btn") {
        Some(target_element)
    } else {
        target_element.closest(".code-copy-btn").ok().flatten()
    };

    let Some(button) = button else {
        return;
    };

    event.prevent_default();

    let code_text = button
        .parent_element()
        .and_then(|shell| shell.query_selector("pre code").ok().flatten())
        .and_then(|code| code.text_content())
        .unwrap_or_default();

    if code_text.trim().is_empty() {
        set_copy_button_state(&button, "No code", "error");
        let button_reset = button.clone();
        wasm_bindgen_futures::spawn_local(async move {
            TimeoutFuture::new(900).await;
            set_copy_button_state(&button_reset, "Copy", "idle");
        });
        return;
    }

    let Some(window) = web_sys::window() else {
        set_copy_button_state(&button, "Clipboard off", "error");
        return;
    };

    let clipboard = window.navigator().clipboard();

    set_copy_button_state(&button, "Copying...", "busy");
    let promise = clipboard.write_text(&code_text);
    let button_reset = button.clone();
    wasm_bindgen_futures::spawn_local(async move {
        if JsFuture::from(promise).await.is_ok() {
            set_copy_button_state(&button_reset, "Copied", "ok");
            TimeoutFuture::new(1200).await;
        } else {
            set_copy_button_state(&button_reset, "Failed", "error");
            TimeoutFuture::new(1500).await;
        }
        set_copy_button_state(&button_reset, "Copy", "idle");
    });
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct CoreRequestInput {
    method: String,
    path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    body: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    admin_token: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CoreResponseOutput {
    pub status: u16,
    pub body: Value,
}

async fn core_request<T: for<'de> serde::Deserialize<'de>>(
    input: CoreRequestInput,
) -> Result<T, String> {
    let args =
        serde_wasm_bindgen::to_value(&json!({ "input": input })).map_err(|e| e.to_string())?;
    let response = tauri_invoke("core_request", args).await.map_err(js_error)?;
    let response: CoreResponseOutput =
        serde_wasm_bindgen::from_value(response).map_err(|e| e.to_string())?;

    if response.status >= 200 && response.status < 300 {
        serde_json::from_value(response.body).map_err(|e| e.to_string())
    } else {
        Err(format!("Error {}: {}", response.status, response.body))
    }
}

async fn invoke_tauri_command<T: for<'de> serde::Deserialize<'de>>(cmd: &str) -> Result<T, String> {
    let args = serde_wasm_bindgen::to_value(&json!({})).map_err(|e| e.to_string())?;
    let response = tauri_invoke(cmd, args).await.map_err(js_error)?;
    serde_wasm_bindgen::from_value(response).map_err(|e| e.to_string())
}

async fn invoke_tauri_with_args<T: for<'de> serde::Deserialize<'de>>(
    cmd: &str,
    payload: Value,
) -> Result<T, String> {
    let args = serde_wasm_bindgen::to_value(&payload).map_err(|e| e.to_string())?;
    let response = tauri_invoke(cmd, args).await.map_err(js_error)?;
    serde_wasm_bindgen::from_value(response).map_err(|e| e.to_string())
}

async fn check_release_update() -> Result<ReleaseUpdateStatus, String> {
    invoke_tauri_command("check_release_update").await
}

async fn apply_release_update() -> Result<ReleaseUpdateAction, String> {
    invoke_tauri_command("apply_release_update").await
}

async fn start_local_auth_flow(provider: &str) -> Result<LocalAuthAction, String> {
    invoke_tauri_with_args(
        "start_local_auth_flow",
        json!({ "provider": provider.to_string() }),
    )
    .await
}

#[derive(Clone)]
pub struct AppState {
    pub active_tab: RwSignal<TabId>,
    pub health: RwSignal<Option<HealthResponse>>,
    pub agents: RwSignal<Vec<SubAgent>>,
    pub events: RwSignal<Vec<CrystalBallEvent>>,
    pub admin_token: RwSignal<String>,
    pub release_update: RwSignal<Option<ReleaseUpdateStatus>>,
    pub update_busy: RwSignal<bool>,
    pub update_notice: RwSignal<String>,
}

impl AppState {
    fn new() -> Self {
        Self {
            active_tab: create_rw_signal(TabId::Mission),
            health: create_rw_signal(None),
            agents: create_rw_signal(vec![]),
            events: create_rw_signal(vec![]),
            admin_token: create_rw_signal(load_admin_token()),
            release_update: create_rw_signal(None),
            update_busy: create_rw_signal(false),
            update_notice: create_rw_signal(String::new()),
        }
    }

    fn admin_token_opt(&self) -> Option<String> {
        token_opt(&self.admin_token.get_untracked())
    }

    fn start_polling(&self) {
        let state = self.clone();

        let state_init = state.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let _ = state_init.refresh_health().await;
            let _ = state_init.refresh_agents().await;
            let _ = state_init.refresh_events().await;
            let _ = state_init.refresh_release_update().await;
        });

        if let Ok(handle) = set_interval_with_handle(
            move || {
                let state_clone = state.clone();
                wasm_bindgen_futures::spawn_local(async move {
                    let _ = state_clone.refresh_health().await;
                    let _ = state_clone.refresh_agents().await;
                    let _ = state_clone.refresh_events().await;
                });
            },
            Duration::from_secs(5),
        ) {
            on_cleanup(move || {
                handle.clear();
            });
        }

        let update_state = self.clone();
        if let Ok(handle) = set_interval_with_handle(
            move || {
                let state_clone = update_state.clone();
                wasm_bindgen_futures::spawn_local(async move {
                    let _ = state_clone.refresh_release_update().await;
                });
            },
            Duration::from_secs(900),
        ) {
            on_cleanup(move || {
                handle.clear();
            });
        }
    }

    async fn refresh_health(&self) -> Result<(), String> {
        let payload = core_request::<HealthResponse>(CoreRequestInput {
            method: "GET".to_string(),
            path: "/health".to_string(),
            body: None,
            admin_token: None,
        })
        .await?;
        self.health.set(Some(payload));
        Ok(())
    }

    async fn refresh_agents(&self) -> Result<(), String> {
        let payload = core_request::<Vec<SubAgent>>(CoreRequestInput {
            method: "GET".to_string(),
            path: "/api/agents".to_string(),
            body: None,
            admin_token: None,
        })
        .await?;
        self.agents.set(payload);
        Ok(())
    }

    async fn refresh_events(&self) -> Result<(), String> {
        let payload = core_request::<Vec<CrystalBallEvent>>(CoreRequestInput {
            method: "GET".to_string(),
            path: "/api/events?limit=200".to_string(),
            body: None,
            admin_token: None,
        })
        .await?;
        self.events.set(payload);
        Ok(())
    }

    async fn refresh_release_update(&self) -> Result<(), String> {
        let payload = check_release_update().await?;
        self.release_update.set(Some(payload));
        Ok(())
    }
}

#[component]
fn MissionTabView(app_state: AppState) -> impl IntoView {
    let (messages, set_messages) = create_signal(Vec::<InferenceChatMessage>::new());
    let (input, set_input) = create_signal(String::new());
    let (is_sending, set_is_sending) = create_signal(false);
    let (is_streaming_reply, set_is_streaming_reply) = create_signal(false);
    let (chat_notice, set_chat_notice) = create_signal(String::new());

    let chat_log_ref = create_node_ref::<html::Div>();

    let refresh_main_history: Rc<dyn Fn()> = Rc::new(move || {
        if is_sending.get_untracked() || is_streaming_reply.get_untracked() {
            return;
        }
        wasm_bindgen_futures::spawn_local(async move {
            if let Ok(res) = core_request::<ChatHistoryResponse>(CoreRequestInput {
                method: "GET".to_string(),
                path: "/api/chat/history?limit=120".to_string(),
                body: None,
                admin_token: None,
            })
            .await
            {
                set_messages.set(res.messages);
            }
        });
    });

    {
        let refresh_main_history = Rc::clone(&refresh_main_history);
        create_effect(move |_| {
            (refresh_main_history)();
            let refresh_main_history = Rc::clone(&refresh_main_history);
            if let Ok(handle) =
                set_interval_with_handle(move || (refresh_main_history)(), Duration::from_secs(3))
            {
                on_cleanup(move || {
                    handle.clear();
                });
            }
        });
    }

    {
        create_effect(move |_| {
            let _ = messages.get().len();
            if let Some(log) = chat_log_ref.get() {
                log.set_scroll_top(log.scroll_height());
            }
        });
    }

    let send_main_message: Rc<dyn Fn()> = Rc::new({
        let app_state = app_state.clone();
        let refresh_main_history = Rc::clone(&refresh_main_history);

        move || {
            let text = input.get().trim().to_string();
            if text.is_empty() || is_sending.get() {
                return;
            }

            set_input.set(String::new());
            set_is_sending.set(true);
            set_is_streaming_reply.set(true);
            set_chat_notice.set(String::new());

            set_messages.update(|rows| {
                rows.push(InferenceChatMessage {
                    role: "user".to_string(),
                    content: text.clone(),
                });
                rows.push(InferenceChatMessage {
                    role: "assistant".to_string(),
                    content: String::new(),
                });
            });

            let app_state = app_state.clone();
            let refresh_main_history = Rc::clone(&refresh_main_history);
            wasm_bindgen_futures::spawn_local(async move {
                let mut streamed_text = String::new();
                let stream_result = stream_chat_reply(text.clone(), None, |token| {
                    streamed_text.push_str(&token);
                    let partial = streamed_text.clone();
                    set_messages.update(|rows| {
                        if let Some(last) = rows.last_mut() {
                            last.content = partial.clone();
                        }
                    });
                })
                .await;

                match stream_result {
                    Ok(full_response) => {
                        let final_text = if full_response.trim().is_empty() {
                            streamed_text
                        } else {
                            full_response
                        };
                        set_messages.update(|rows| {
                            if let Some(last) = rows.last_mut() {
                                last.content = final_text.clone();
                            }
                        });
                        set_chat_notice.set(String::new());
                    }
                    Err(stream_err) => {
                        if streamed_text.is_empty() {
                            match core_request::<ChatResponse>(CoreRequestInput {
                                method: "POST".to_string(),
                                path: "/api/chat".to_string(),
                                body: Some(json!({ "message": text })),
                                admin_token: None,
                            })
                            .await
                            {
                                Ok(response) => {
                                    set_messages.update(|rows| {
                                        if let Some(last) = rows.last_mut() {
                                            last.content = response.reply.clone();
                                        }
                                    });
                                    set_chat_notice.set(String::new());
                                }
                                Err(err) => {
                                    set_chat_notice.set(format!(
                                        "Chat stream failed ({}) and fallback failed: {}",
                                        stream_err, err
                                    ));
                                }
                            }
                        } else {
                            set_chat_notice.set(format!("Chat stream interrupted: {}", stream_err));
                        }
                    }
                }

                set_is_streaming_reply.set(false);

                (refresh_main_history)();
                let _ = app_state.refresh_events().await;
                set_is_sending.set(false);
            });
        }
    });

    let send_on_enter = Rc::clone(&send_main_message);
    let send_on_click = Rc::clone(&send_main_message);

    view! {
        <div class="main-mission">
            <div class="chat-panel">
                <div
                    class="chat-log"
                    node_ref=chat_log_ref
                    on:click=move |ev| handle_markdown_copy_click(ev)
                >
                    {move || {
                        if messages.get().is_empty() {
                            view! {
                                <div class="chat-empty">
                                    <div class="empty-title">"Mission console is live."</div>
                                    <div class="empty-copy">"Send an instruction to Kaizen to begin execution."</div>
                                </div>
                            }
                                .into_view()
                        } else {
                            view! {
                                <For
                                    each=move || {
                                        messages
                                            .get()
                                            .into_iter()
                                            .enumerate()
                                            .collect::<Vec<_>>()
                                    }
                                    key=|item| item.0
                                    children=move |item| {
                                        let msg = item.1;
                                        let role_class = if msg.role == "user" {
                                            "message user"
                                        } else {
                                            "message assistant"
                                        };
                                        let sender = sender_label(&msg.role);
                                        let content_view = if msg.role == "assistant" {
                                            let rendered = render_markdown(&msg.content);
                                            view! {
                                                <div class="message-body markdown-body" inner_html=rendered></div>
                                            }
                                                .into_view()
                                        } else {
                                            view! { <div class="message-body plain-message">{msg.content.clone()}</div> }
                                                .into_view()
                                        };
                                        view! {
                                            <div class=role_class>
                                                <div class="msg-sender">{sender}</div>
                                                {content_view}
                                            </div>
                                        }
                                    }
                                />
                            }
                                .into_view()
                        }
                    }}
                </div>

                <div class="composer-container">
                    <div class="composer-row">
                        <textarea
                            class="composer"
                            rows="3"
                            placeholder="Direct Kaizen..."
                            prop:value=move || input.get()
                            prop:disabled=move || is_sending.get()
                            on:input=move |ev| set_input.set(event_target_value(&ev))
                            on:keydown=move |ev| {
                                if ev.key() == "Enter" && !ev.shift_key() {
                                    ev.prevent_default();
                                    (send_on_enter)();
                                }
                            }
                        ></textarea>

                        <button
                            class="send-btn"
                            prop:disabled=move || is_sending.get()
                            on:click=move |_| (send_on_click)()
                        >
                            {move || if is_sending.get() { "Sending..." } else { "Send" }}
                        </button>
                    </div>

                    {move || {
                        if chat_notice.get().is_empty() {
                            ().into_view()
                        } else {
                            view! { <div class="notice error">{chat_notice.get()}</div> }.into_view()
                        }
                    }}
                </div>

            </div>
        </div>
    }
}

#[component]
fn BranchesTabView(app_state: AppState) -> impl IntoView {
    let branches = create_memo(move |_| grouped_branches(&app_state.agents.get()));
    let branch_rows = create_memo(move |_| {
        branches
            .get()
            .into_iter()
            .collect::<Vec<(String, BTreeMap<String, Vec<SubAgent>>)>>()
    });
    let (show_desk_view, set_show_desk_view) = create_signal(false);

    view! {
        <section class="tab-view">
            <div class="tab-head">
                <h2>"Branches"</h2>
                <p>"Company hierarchy with org chart and spatial office views."</p>
            </div>

            <div class="card toolbar-card">
                <div class="toolbar">
                    <span class="tiny-pill">"Workers " {move || app_state.agents.get().len()}</span>
                    <span class="tiny-pill">
                        "Branches "
                        {move || {
                            let rows = branch_rows.get();
                            if rows.is_empty() {
                                0usize
                            } else {
                                rows.len()
                            }
                        }}
                    </span>
                    <span class="tiny-pill">
                        "Missions "
                        {move || {
                            branch_rows
                                .get()
                                .into_iter()
                                .map(|(_, missions)| missions.len())
                                .sum::<usize>()
                        }}
                    </span>

                    <div class="view-toggle">
                        <button
                            class=move || {
                                if !show_desk_view.get() {
                                    "tiny-btn active"
                                } else {
                                    "tiny-btn"
                                }
                            }
                            on:click=move |_| set_show_desk_view.set(false)
                        >
                            "Org Chart"
                        </button>
                        <button
                            class=move || {
                                if show_desk_view.get() {
                                    "tiny-btn active"
                                } else {
                                    "tiny-btn"
                                }
                            }
                            on:click=move |_| set_show_desk_view.set(true)
                        >
                            "Office 2D"
                        </button>
                    </div>
                </div>
            </div>

            {move || {
                if branch_rows.get().is_empty() {
                    view! { <div class="card"><div class="muted">"No branch workers are active yet."</div></div> }
                        .into_view()
                } else if show_desk_view.get() {
                    view! {
                        <div class="desk-board office-board">
                            <For
                                each=move || branch_rows.get()
                                key=|item| item.0.clone()
                                children=move |item| {
                                    let branch_id = item.0;
                                    let missions = item
                                        .1
                                        .into_iter()
                                        .collect::<Vec<(String, Vec<SubAgent>)>>();
                                    let mission_total = missions.len();

                                    view! {
                                        <article class="card desk-branch-card">
                                            <div class="desk-branch-head">
                                                <h3>{format!("Branch: {}", branch_id)}</h3>
                                                <span class="tiny-pill">
                                                    {format!(
                                                        "{} workers",
                                                        missions
                                                            .iter()
                                                            .map(|(_, workers)| workers.len())
                                                            .sum::<usize>()
                                                    )}
                                                </span>
                                            </div>

                                            <div class="office-floor">
                                                <div class="office-center-hub">
                                                    <div class="hub-title">"Kaizen Hub"</div>
                                                    <div class="hub-copy">{format!("{} missions", mission_total)}</div>
                                                </div>

                                                {missions
                                                    .into_iter()
                                                    .enumerate()
                                                    .map(|(mission_index, (mission_id, workers))| {
                                                        let pod_style = mission_pod_style(mission_index, mission_total);
                                                        let worker_total = workers.len();
                                                        let lane_label = mission_lane_for_workers(&workers)
                                                            .replace('_', " ");

                                                        view! {
                                                            <section class="mission-pod" style=pod_style>
                                                                <div class="mission-pod-head">
                                                                    <span>{mission_id.clone()}</span>
                                                                    <span class="tiny-pill">{format!("{} | {}", worker_total, lane_label)}</span>
                                                                </div>

                                                                <div class="pod-workers">
                                                                    {workers
                                                                        .into_iter()
                                                                        .enumerate()
                                                                        .map(|(worker_index, worker)| {
                                                                            let status = status_class(&worker.status).to_string();
                                                                            let seat_style = worker_seat_style(worker_index, worker_total);
                                                                            view! {
                                                                                <div class="office-worker" style=seat_style>
                                                                                    <span class=format!("status-dot {}", status)></span>
                                                                                    <div class="worker-meta">
                                                                                        <div>{worker.name}</div>
                                                                                        <div class="agent-task">{worker.objective}</div>
                                                                                    </div>
                                                                                </div>
                                                                            }
                                                                        })
                                                                        .collect_view()}
                                                                </div>
                                                            </section>
                                                        }
                                                    })
                                                    .collect_view()}
                                            </div>
                                        </article>
                                    }
                                }
                            />
                        </div>
                    }
                        .into_view()
                } else {
                    view! {
                        <div class="card org-chart-card">
                            <h3>"Org Chart"</h3>
                            <div class="hierarchy">
                                <details class="tree-group" open=true>
                                    <summary class="tree-summary">
                                        <span>"Orchestrator"</span>
                                        <span class="count-pill">{move || app_state.agents.get().len()} " workers"</span>
                                    </summary>

                                    <For
                                        each=move || branch_rows.get()
                                        key=|item| item.0.clone()
                                        children=move |item| {
                                            let branch_id = item.0;
                                            let missions = item
                                                .1
                                                .into_iter()
                                                .collect::<Vec<(String, Vec<SubAgent>)>>();

                                            view! {
                                                <details class="tree-group" open=true>
                                                    <summary class="tree-summary">
                                                        <span>{format!("Branch: {}", branch_id)}</span>
                                                        <span class="count-pill">
                                                            {format!(
                                                                "{} workers",
                                                                missions
                                                                    .iter()
                                                                    .map(|(_, workers)| workers.len())
                                                                    .sum::<usize>()
                                                            )}
                                                        </span>
                                                    </summary>

                                                    {missions
                                                        .into_iter()
                                                        .map(|(mission_id, workers)| {
                                                            let active_count = workers
                                                                .iter()
                                                                .filter(|worker| matches!(worker.status, AgentStatus::Active))
                                                                .count();
                                                            view! {
                                                                <details class="tree-group mission-node" open=true>
                                                                    <summary class="tree-summary">
                                                                        <span>{format!("Mission: {}", mission_id)}</span>
                                                                        <span class="count-pill">{format!("{}/{} active", active_count, workers.len())}</span>
                                                                    </summary>

                                                                    <div class="worker-list">
                                                                        {workers
                                                                            .into_iter()
                                                                            .map(|worker| {
                                                                                let status = status_class(&worker.status).to_string();
                                                                                view! {
                                                                                    <div class="worker-row">
                                                                                        <span class=format!("status-dot {}", status)></span>
                                                                                        <span class="worker-name">{worker.name}</span>
                                                                                        <span class="worker-status">{status_label(&worker.status)}</span>
                                                                                    </div>
                                                                                }
                                                                            })
                                                                            .collect_view()}
                                                                    </div>
                                                                </details>
                                                            }
                                                        })
                                                        .collect_view()}
                                                </details>
                                            }
                                        }
                                    />
                                </details>
                            </div>
                        </div>
                    }
                        .into_view()
                }
            }}
        </section>
    }
}

async fn stream_chat_reply(
    message: String,
    agent_id: Option<String>,
    mut on_token: impl FnMut(String),
) -> Result<String, String> {
    let window = web_sys::window().ok_or_else(|| "window unavailable".to_string())?;

    let init = web_sys::RequestInit::new();
    init.set_method("POST");
    init.set_mode(web_sys::RequestMode::Cors);

    let payload = json!({
        "message": message,
        "agent_id": agent_id,
    });
    let payload_text = serde_json::to_string(&payload).map_err(|e| e.to_string())?;
    init.set_body(&JsValue::from_str(&payload_text));

    let request =
        web_sys::Request::new_with_str_and_init("http://127.0.0.1:9100/api/chat/stream", &init)
            .map_err(js_error)?;
    request
        .headers()
        .set("Content-Type", "application/json")
        .map_err(js_error)?;
    request
        .headers()
        .set("Accept", "text/event-stream")
        .map_err(js_error)?;

    let response_value = JsFuture::from(window.fetch_with_request(&request))
        .await
        .map_err(js_error)?;
    let response: web_sys::Response = response_value
        .dyn_into()
        .map_err(|_| "failed to cast fetch response".to_string())?;

    if !response.ok() {
        let body_text = JsFuture::from(response.text().map_err(js_error)?)
            .await
            .ok()
            .and_then(|v| v.as_string())
            .unwrap_or_default();
        return Err(format!(
            "stream request failed ({}): {}",
            response.status(),
            body_text
        ));
    }

    let body = response
        .body()
        .ok_or_else(|| "stream response body missing".to_string())?;
    let reader = body
        .get_reader()
        .dyn_into::<web_sys::ReadableStreamDefaultReader>()
        .map_err(|_| "failed to acquire stream reader".to_string())?;

    let mut stream_buffer = String::new();
    let mut full_response = String::new();

    loop {
        let read_result = JsFuture::from(reader.read())
            .await
            .map_err(|e| format!("stream read failed: {}", js_error(e)))?;

        let done = js_sys::Reflect::get(&read_result, &JsValue::from_str("done"))
            .ok()
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        if done {
            break;
        }

        let chunk_value = js_sys::Reflect::get(&read_result, &JsValue::from_str("value"))
            .map_err(|e| format!("stream chunk missing: {}", js_error(e)))?;
        if chunk_value.is_undefined() || chunk_value.is_null() {
            continue;
        }

        let bytes = js_sys::Uint8Array::new(&chunk_value).to_vec();
        let chunk = String::from_utf8_lossy(&bytes).replace("\r\n", "\n");
        stream_buffer.push_str(&chunk);

        while let Some(frame_end) = stream_buffer.find("\n\n") {
            let frame = stream_buffer[..frame_end].to_string();
            stream_buffer = stream_buffer[(frame_end + 2)..].to_string();

            let mut event_name = "message".to_string();
            let mut data_lines: Vec<String> = Vec::new();

            for line in frame.lines() {
                if let Some(rest) = line.strip_prefix("event:") {
                    event_name = rest.trim().to_string();
                } else if let Some(rest) = line.strip_prefix("data:") {
                    data_lines.push(rest.trim_start().to_string());
                }
            }

            let data = data_lines.join("\n");
            if data.is_empty() {
                continue;
            }

            match event_name.as_str() {
                "token" => {
                    let text = serde_json::from_str::<Value>(&data)
                        .ok()
                        .and_then(|value| {
                            value
                                .get("text")
                                .and_then(|text| text.as_str())
                                .map(|text| text.to_string())
                        })
                        .unwrap_or_default();

                    if !text.is_empty() {
                        full_response.push_str(&text);
                        on_token(text);
                    }
                }
                "done" => {
                    if let Some(done_text) =
                        serde_json::from_str::<Value>(&data).ok().and_then(|value| {
                            value
                                .get("full_response")
                                .and_then(|text| text.as_str())
                                .map(|text| text.to_string())
                        })
                    {
                        full_response = done_text;
                    }
                    return Ok(full_response);
                }
                "error" => return Err(data),
                _ => {}
            }
        }
    }

    Ok(full_response)
}

#[component]
fn GatesTabView(app_state: AppState) -> impl IntoView {
    let (gate_snapshot, set_gate_snapshot) = create_signal(None::<GateSnapshot>);
    let (transition, set_transition) = create_signal(None::<GateTransitionResult>);
    let (gate_busy, set_gate_busy) = create_signal(false);
    let (gate_error, set_gate_error) = create_signal(String::new());

    let refresh_gates: Rc<dyn Fn()> = Rc::new({
        let app_state = app_state.clone();
        move || {
            let app_state = app_state.clone();
            wasm_bindgen_futures::spawn_local(async move {
                match core_request::<GateSnapshot>(CoreRequestInput {
                    method: "GET".to_string(),
                    path: "/api/gates".to_string(),
                    body: None,
                    admin_token: app_state.admin_token_opt(),
                })
                .await
                {
                    Ok(snapshot) => {
                        set_gate_snapshot.set(Some(snapshot));
                        set_gate_error.set(String::new());
                    }
                    Err(err) => set_gate_error.set(err),
                }
            });
        }
    });

    {
        let refresh_gates = Rc::clone(&refresh_gates);
        create_effect(move |_| {
            (refresh_gates)();
            let refresh_gates = Rc::clone(&refresh_gates);
            if let Ok(handle) =
                set_interval_with_handle(move || (refresh_gates)(), Duration::from_secs(12))
            {
                on_cleanup(move || handle.clear());
            }
        });
    }

    let patch_condition: Rc<dyn Fn(&'static str, bool)> = Rc::new({
        let app_state = app_state.clone();
        let refresh_gates = Rc::clone(&refresh_gates);
        move |field, value| {
            if gate_busy.get() {
                return;
            }
            set_gate_busy.set(true);
            let app_state = app_state.clone();
            let refresh_gates = Rc::clone(&refresh_gates);
            let field_name = field.to_string();
            wasm_bindgen_futures::spawn_local(async move {
                match core_request::<GateSnapshot>(CoreRequestInput {
                    method: "PATCH".to_string(),
                    path: "/api/gates/conditions".to_string(),
                    body: Some(bool_patch(&field_name, value)),
                    admin_token: app_state.admin_token_opt(),
                })
                .await
                {
                    Ok(snapshot) => {
                        set_gate_snapshot.set(Some(snapshot));
                        set_gate_error.set(String::new());
                    }
                    Err(err) => set_gate_error.set(err),
                }
                set_gate_busy.set(false);
                (refresh_gates)();
            });
        }
    });

    let advance_gate: Rc<dyn Fn()> = Rc::new({
        let app_state = app_state.clone();
        let refresh_gates = Rc::clone(&refresh_gates);
        move || {
            if gate_busy.get() {
                return;
            }
            set_gate_busy.set(true);
            let app_state = app_state.clone();
            let refresh_gates = Rc::clone(&refresh_gates);
            wasm_bindgen_futures::spawn_local(async move {
                match core_request::<GateTransitionResult>(CoreRequestInput {
                    method: "POST".to_string(),
                    path: "/api/gates/advance".to_string(),
                    body: None,
                    admin_token: app_state.admin_token_opt(),
                })
                .await
                {
                    Ok(result) => {
                        set_transition.set(Some(result));
                        set_gate_error.set(String::new());
                    }
                    Err(err) => set_gate_error.set(err),
                }
                set_gate_busy.set(false);
                (refresh_gates)();
            });
        }
    });

    view! {
        <section class="tab-view">
            <div class="tab-head">
                <h2>"Gates"</h2>
                <p>"Control the orchestration state machine and gate conditions."</p>
            </div>

            {move || {
                if gate_error.get().is_empty() {
                    ().into_view()
                } else {
                    view! { <div class="notice error">{gate_error.get()}</div> }.into_view()
                }
            }}

            <div class="card-grid two-col">
                <article class="card">
                    <h3>"Runtime"</h3>
                    <div class="inline-meta">
                        {move || {
                            if let Some(snapshot) = gate_snapshot.get() {
                                view! {
                                    <>
                                        <span class="tiny-pill">"Current: " {gate_label(&snapshot.current_state)}</span>
                                        <span class="tiny-pill">{if snapshot.hard_gates_enabled { "Hard Gates On" } else { "Hard Gates Off" }}</span>
                                    </>
                                }
                                    .into_view()
                            } else {
                                view! { <span class="tiny-pill">"Loading..."</span> }.into_view()
                            }
                        }}
                    </div>

                    <div class="toolbar-inline" style="margin-top: 10px;">
                        <button
                            class="action-btn"
                            prop:disabled=move || gate_busy.get()
                            on:click=move |_| (advance_gate)()
                        >
                            {move || if gate_busy.get() { "Advancing..." } else { "Advance Gate" }}
                        </button>
                        <button class="action-btn" on:click=move |_| (refresh_gates)()>"Refresh"</button>
                    </div>

                    {move || {
                        if let Some(result) = transition.get() {
                            let blocked = if result.blocked_by.is_empty() {
                                "none".to_string()
                            } else {
                                result.blocked_by.join(", ")
                            };
                            view! {
                                <div class="notice neutral" style="margin-top: 10px;">
                                    <strong>{if result.allowed { "Transition allowed" } else { "Transition blocked" }}</strong>
                                    <div>{format!("{} -> {}", gate_label(&result.from), gate_label(&result.to))}</div>
                                    <div>{format!("Blocked by: {}", blocked)}</div>
                                </div>
                            }
                                .into_view()
                        } else {
                            ().into_view()
                        }
                    }}
                </article>

                <article class="card">
                    <h3>"Condition Controls"</h3>
                    {move || {
                        if let Some(snapshot) = gate_snapshot.get() {
                            let patch_condition_plan_defined = Rc::clone(&patch_condition);
                            let patch_condition_plan_ack = Rc::clone(&patch_condition);
                            let patch_condition_artifacts = Rc::clone(&patch_condition);
                            let patch_condition_reasoners = Rc::clone(&patch_condition);
                            let patch_condition_approval = Rc::clone(&patch_condition);
                            let patch_condition_smoke = Rc::clone(&patch_condition);
                            let patch_condition_deploy = Rc::clone(&patch_condition);

                            view! {
                                <div class="condition-list">
                                    <div class="condition-row">
                                        <span>"plan_defined"</span>
                                        <span class="tiny-pill">{if snapshot.conditions.plan_defined { "true" } else { "false" }}</span>
                                        <button class="tiny-btn" on:click=move |_| (patch_condition_plan_defined)("plan_defined", !snapshot.conditions.plan_defined)>"Toggle"</button>
                                    </div>
                                    <div class="condition-row">
                                        <span>"plan_acknowledged"</span>
                                        <span class="tiny-pill">{if snapshot.conditions.plan_acknowledged { "true" } else { "false" }}</span>
                                        <button class="tiny-btn" on:click=move |_| (patch_condition_plan_ack)("plan_acknowledged", !snapshot.conditions.plan_acknowledged)>"Toggle"</button>
                                    </div>
                                    <div class="condition-row">
                                        <span>"execution_artifacts_present"</span>
                                        <span class="tiny-pill">{if snapshot.conditions.execution_artifacts_present { "true" } else { "false" }}</span>
                                        <button class="tiny-btn" on:click=move |_| (patch_condition_artifacts)("execution_artifacts_present", !snapshot.conditions.execution_artifacts_present)>"Toggle"</button>
                                    </div>
                                    <div class="condition-row">
                                        <span>"passed_reasoners_test"</span>
                                        <span class="tiny-pill">{if snapshot.conditions.passed_reasoners_test { "true" } else { "false" }}</span>
                                        <button class="tiny-btn" on:click=move |_| (patch_condition_reasoners)("passed_reasoners_test", !snapshot.conditions.passed_reasoners_test)>"Toggle"</button>
                                    </div>
                                    <div class="condition-row">
                                        <span>"kaizen_review_approved"</span>
                                        <span class="tiny-pill">{if snapshot.conditions.kaizen_review_approved { "true" } else { "false" }}</span>
                                        <button class="tiny-btn" on:click=move |_| (patch_condition_approval)("kaizen_review_approved", !snapshot.conditions.kaizen_review_approved)>"Toggle"</button>
                                    </div>
                                    <div class="condition-row">
                                        <span>"human_smoke_test_passed"</span>
                                        <span class="tiny-pill">{if snapshot.conditions.human_smoke_test_passed { "true" } else { "false" }}</span>
                                        <button class="tiny-btn" on:click=move |_| (patch_condition_smoke)("human_smoke_test_passed", !snapshot.conditions.human_smoke_test_passed)>"Toggle"</button>
                                    </div>
                                    <div class="condition-row">
                                        <span>"deploy_validation_passed"</span>
                                        <span class="tiny-pill">{if snapshot.conditions.deploy_validation_passed { "true" } else { "false" }}</span>
                                        <button class="tiny-btn" on:click=move |_| (patch_condition_deploy)("deploy_validation_passed", !snapshot.conditions.deploy_validation_passed)>"Toggle"</button>
                                    </div>
                                </div>
                            }
                                .into_view()
                        } else {
                            view! { <div class="muted">"No gate snapshot loaded."</div> }.into_view()
                        }
                    }}
                </article>
            </div>
        </section>
    }
}

#[component]
fn ActivityTabView(app_state: AppState) -> impl IntoView {
    let (query, set_query) = create_signal(String::new());
    let (kind, set_kind) = create_signal(String::new());
    let (branch_scope, set_branch_scope) = create_signal(String::new());
    let (mission_scope, set_mission_scope) = create_signal(String::new());

    let scoped_events = create_memo(move |_| {
        let agents = app_state.agents.get();
        app_state
            .events
            .get()
            .into_iter()
            .map(|event| {
                let (branch_id, mission_id) = infer_event_scope(&event, &agents);
                ActivityEventRow {
                    event,
                    branch_id,
                    mission_id,
                }
            })
            .collect::<Vec<_>>()
    });

    let event_types = create_memo(move |_| {
        let mut values = scoped_events
            .get()
            .into_iter()
            .map(|row| row.event.event_type)
            .collect::<Vec<_>>();
        values.sort();
        values.dedup();
        values
    });

    let branch_options = create_memo(move |_| {
        let mut values = scoped_events
            .get()
            .into_iter()
            .map(|row| row.branch_id)
            .collect::<Vec<_>>();
        values.sort();
        values.dedup();
        values
    });

    let mission_options = create_memo(move |_| {
        let selected_branch = branch_scope.get();
        let mut values = scoped_events
            .get()
            .into_iter()
            .filter(|row| selected_branch.is_empty() || row.branch_id == selected_branch)
            .map(|row| row.mission_id)
            .collect::<Vec<_>>();
        values.sort();
        values.dedup();
        values
    });

    let filtered_events = create_memo(move |_| {
        let needle = query.get().to_lowercase();
        let selected_kind = kind.get();
        let selected_branch = branch_scope.get();
        let selected_mission = mission_scope.get();

        scoped_events
            .get()
            .into_iter()
            .filter(|row| {
                let event = &row.event;
                let type_match = selected_kind.is_empty() || event.event_type == selected_kind;
                let branch_match = selected_branch.is_empty() || row.branch_id == selected_branch;
                let mission_match =
                    selected_mission.is_empty() || row.mission_id == selected_mission;
                let text_match = needle.is_empty()
                    || event.message.to_lowercase().contains(&needle)
                    || event.source_actor.to_lowercase().contains(&needle)
                    || event.target_actor.to_lowercase().contains(&needle)
                    || event.task_id.to_lowercase().contains(&needle)
                    || row.branch_id.to_lowercase().contains(&needle)
                    || row.mission_id.to_lowercase().contains(&needle);
                type_match && branch_match && mission_match && text_match
            })
            .collect::<Vec<_>>()
    });

    view! {
        <section class="tab-view">
            <div class="tab-head">
                <h2>"Activity"</h2>
                <p>"Filter and inspect orchestration events in real time."</p>
            </div>

            <div class="card toolbar-card">
                <div class="toolbar">
                    <input
                        class="text-input"
                        type="text"
                        placeholder="Search message, actor, task..."
                        prop:value=move || query.get()
                        on:input=move |ev| set_query.set(event_target_value(&ev))
                    />

                    <select
                        class="select-input"
                        prop:value=move || kind.get()
                        on:change=move |ev| set_kind.set(event_target_value(&ev))
                    >
                        <option value="">"All event types"</option>
                        <For
                            each=move || event_types.get()
                            key=|value| value.clone()
                            children=move |value| view! { <option value=value.clone()>{value}</option> }
                        />
                    </select>

                    <select
                        class="select-input"
                        prop:value=move || branch_scope.get()
                        on:change=move |ev| {
                            let selected = event_target_value(&ev);
                            set_branch_scope.set(selected);
                            set_mission_scope.set(String::new());
                        }
                    >
                        <option value="">"All branches"</option>
                        <For
                            each=move || branch_options.get()
                            key=|value| value.clone()
                            children=move |value| view! { <option value=value.clone()>{value}</option> }
                        />
                    </select>

                    <select
                        class="select-input"
                        prop:value=move || mission_scope.get()
                        on:change=move |ev| set_mission_scope.set(event_target_value(&ev))
                    >
                        <option value="">"All missions"</option>
                        <For
                            each=move || mission_options.get()
                            key=|value| value.clone()
                            children=move |value| view! { <option value=value.clone()>{value}</option> }
                        />
                    </select>

                    <span class="tiny-pill">"Rows " {move || filtered_events.get().len()}</span>
                </div>
            </div>

            <div class="card activity-table">
                <div class="table-header row">
                    <span>"Time"</span>
                    <span>"Type"</span>
                    <span>"Branch"</span>
                    <span>"Mission"</span>
                    <span>"Source"</span>
                    <span>"Target"</span>
                    <span>"Task"</span>
                    <span>"Message"</span>
                </div>

                {move || {
                    if filtered_events.get().is_empty() {
                        view! { <div class="table-empty">"No matching activity events."</div> }
                            .into_view()
                    } else {
                        view! {
                            <For
                                each=move || filtered_events.get()
                                key=|row| row.event.event_id.clone()
                                children=move |row| {
                                    let ActivityEventRow {
                                        event,
                                        branch_id,
                                        mission_id,
                                    } = row;
                                    view! {
                                        <div class="table-row row">
                                            <span>{time_bucket_label(&event.timestamp)}</span>
                                            <span>{event.event_type}</span>
                                            <span>{branch_id}</span>
                                            <span>{mission_id}</span>
                                            <span>{event.source_actor}</span>
                                            <span>{event.target_actor}</span>
                                            <span>{event.task_id}</span>
                                            <span class="truncate">{event.message}</span>
                                        </div>
                                    }
                                }
                            />
                        }
                            .into_view()
                    }
                }}
            </div>
        </section>
    }
}

#[component]
fn WorkspaceTabView(app_state: AppState) -> impl IntoView {
    let (spawn_name, set_spawn_name) = create_signal(String::new());
    let (spawn_branch_id, set_spawn_branch_id) = create_signal("primary".to_string());
    let (spawn_mission_id, set_spawn_mission_id) = create_signal(String::new());
    let (spawn_objective, set_spawn_objective) = create_signal(String::new());
    let (workspace_busy, set_workspace_busy) = create_signal(false);
    let (workspace_notice, set_workspace_notice) = create_signal(String::new());
    let (control_branch_scope, set_control_branch_scope) = create_signal(String::new());
    let (control_mission_scope, set_control_mission_scope) = create_signal(String::new());

    let app_state_for_branch_options = app_state.clone();
    let control_branch_options = create_memo(move |_| {
        let mut values = app_state_for_branch_options
            .agents
            .get()
            .into_iter()
            .map(|agent| branch_label(&agent.branch_id))
            .collect::<Vec<_>>();
        values.sort();
        values.dedup();
        values
    });

    let app_state_for_mission_options = app_state.clone();
    let control_mission_options = create_memo(move |_| {
        let selected_branch = control_branch_scope.get();
        let mut values = app_state_for_mission_options
            .agents
            .get()
            .into_iter()
            .filter(|agent| {
                selected_branch.is_empty() || branch_label(&agent.branch_id) == selected_branch
            })
            .map(|agent| mission_label(&agent))
            .collect::<Vec<_>>();
        values.sort();
        values.dedup();
        values
    });

    let app_state_for_filters = app_state.clone();
    let filtered_control_agents = create_memo(move |_| {
        let selected_branch = control_branch_scope.get();
        let selected_mission = control_mission_scope.get();

        app_state_for_filters
            .agents
            .get()
            .into_iter()
            .filter(|agent| {
                let branch = branch_label(&agent.branch_id);
                let mission = mission_label(agent);
                (selected_branch.is_empty() || branch == selected_branch)
                    && (selected_mission.is_empty() || mission == selected_mission)
            })
            .collect::<Vec<_>>()
    });

    let run_request: Rc<dyn Fn(String, String, Option<Value>)> = Rc::new({
        let app_state = app_state.clone();
        move |method, path, body| {
            if workspace_busy.get() {
                return;
            }
            set_workspace_busy.set(true);
            let app_state = app_state.clone();
            wasm_bindgen_futures::spawn_local(async move {
                match core_request::<Value>(CoreRequestInput {
                    method,
                    path,
                    body,
                    admin_token: None,
                })
                .await
                {
                    Ok(_) => set_workspace_notice.set("Action completed.".to_string()),
                    Err(err) => set_workspace_notice.set(err),
                }
                let _ = app_state.refresh_agents().await;
                let _ = app_state.refresh_events().await;
                set_workspace_busy.set(false);
            });
        }
    });

    let spawn_agent = {
        let app_state = app_state.clone();
        move |_| {
            if workspace_busy.get() {
                return;
            }
            let name = spawn_name.get().trim().to_string();
            let branch_id = spawn_branch_id.get().trim().to_string();
            let mission_id = spawn_mission_id.get().trim().to_string();
            let legacy_task_id = mission_id.clone();
            let objective = spawn_objective.get().trim().to_string();

            if name.is_empty()
                || branch_id.is_empty()
                || mission_id.is_empty()
                || objective.is_empty()
            {
                set_workspace_notice.set(
                    "Provide agent name, branch id, mission id, and objective before spawning."
                        .to_string(),
                );
                return;
            }

            set_workspace_busy.set(true);
            let app_state = app_state.clone();
            wasm_bindgen_futures::spawn_local(async move {
                match core_request::<SubAgent>(CoreRequestInput {
                    method: "POST".to_string(),
                    path: "/api/agents".to_string(),
                    body: Some(json!({
                        "agent_name": name,
                        "branch_id": branch_id,
                        "mission_id": mission_id,
                        "task_id": legacy_task_id,
                        "objective": objective,
                        "user_requested": true,
                    })),
                    admin_token: None,
                })
                .await
                {
                    Ok(agent) => {
                        set_workspace_notice.set(format!(
                            "Spawned '{}' in branch '{}' / mission '{}'",
                            agent.name, agent.branch_id, agent.mission_id
                        ));
                        set_spawn_name.set(String::new());
                        set_spawn_mission_id.set(String::new());
                        set_spawn_objective.set(String::new());
                    }
                    Err(err) => set_workspace_notice.set(err),
                }

                let _ = app_state.refresh_agents().await;
                let _ = app_state.refresh_events().await;
                set_workspace_busy.set(false);
            });
        }
    };

    let run_request_for_view = Rc::clone(&run_request);

    view! {
        <section class="tab-view">
            <div class="tab-head">
                <h2>"Workspace"</h2>
                <p>"Create and manage sub-agent execution from one place."</p>
            </div>

            <div class="card">
                <h3>"Spawn Sub-Agent"</h3>
                <div class="form-grid">
                    <input
                        class="text-input"
                        type="text"
                        placeholder="Agent name"
                        prop:value=move || spawn_name.get()
                        on:input=move |ev| set_spawn_name.set(event_target_value(&ev))
                    />
                    <input
                        class="text-input"
                        type="text"
                        placeholder="Branch id"
                        prop:value=move || spawn_branch_id.get()
                        on:input=move |ev| set_spawn_branch_id.set(event_target_value(&ev))
                    />
                    <input
                        class="text-input"
                        type="text"
                        placeholder="Mission id"
                        prop:value=move || spawn_mission_id.get()
                        on:input=move |ev| set_spawn_mission_id.set(event_target_value(&ev))
                    />
                    <input
                        class="text-input"
                        type="text"
                        placeholder="Objective"
                        prop:value=move || spawn_objective.get()
                        on:input=move |ev| set_spawn_objective.set(event_target_value(&ev))
                    />
                    <button class="action-btn" prop:disabled=move || workspace_busy.get() on:click=spawn_agent>
                        {move || if workspace_busy.get() { "Working..." } else { "Spawn" }}
                    </button>
                </div>
            </div>

            {move || {
                if workspace_notice.get().is_empty() {
                    ().into_view()
                } else {
                    view! { <div class="notice neutral">{workspace_notice.get()}</div> }.into_view()
                }
            }}

            <div class="card">
                <h3>"Agent Control"</h3>
                <div class="toolbar-inline" style="margin-bottom: 10px;">
                    <select
                        class="select-input"
                        prop:value=move || control_branch_scope.get()
                        on:change=move |ev| {
                            let selected = event_target_value(&ev);
                            set_control_branch_scope.set(selected);
                            set_control_mission_scope.set(String::new());
                        }
                    >
                        <option value="">"All branches"</option>
                        <For
                            each=move || control_branch_options.get()
                            key=|value| value.clone()
                            children=move |value| view! { <option value=value.clone()>{value}</option> }
                        />
                    </select>

                    <select
                        class="select-input"
                        prop:value=move || control_mission_scope.get()
                        on:change=move |ev| set_control_mission_scope.set(event_target_value(&ev))
                    >
                        <option value="">"All missions"</option>
                        <For
                            each=move || control_mission_options.get()
                            key=|value| value.clone()
                            children=move |value| view! { <option value=value.clone()>{value}</option> }
                        />
                    </select>

                    <span class="tiny-pill">"Visible " {move || filtered_control_agents.get().len()}</span>
                </div>

                {move || {
                    let run_request = Rc::clone(&run_request_for_view);
                    if app_state.agents.get().is_empty() {
                        view! { <div class="muted">"No active sub-agents."</div> }.into_view()
                    } else if filtered_control_agents.get().is_empty() {
                        view! { <div class="muted">"No agents match the selected branch/mission scope."</div> }
                            .into_view()
                    } else {
                        view! {
                            <For
                                each=move || filtered_control_agents.get()
                                key=|agent| agent.id.clone()
                                children=move |agent| {
                                    let status = status_class(&agent.status).to_string();

                                    let can_activate = matches!(
                                        agent.status,
                                        AgentStatus::Idle
                                            | AgentStatus::Blocked
                                            | AgentStatus::ReviewPending
                                    );
                                    let can_review = matches!(agent.status, AgentStatus::Active);
                                    let can_done = matches!(agent.status, AgentStatus::ReviewPending);

                                    let id_activate = agent.id.clone();
                                    let id_review = agent.id.clone();
                                    let id_done = agent.id.clone();
                                    let id_stop = agent.id.clone();
                                    let id_clear = agent.id.clone();
                                    let id_remove = agent.id.clone();
                                    let agent_name = agent.name.clone();
                                    let agent_scope =
                                        format!("{} / {}", branch_label(&agent.branch_id), mission_label(&agent));

                                    let run_request_activate = Rc::clone(&run_request);
                                    let run_request_review = Rc::clone(&run_request);
                                    let run_request_done = Rc::clone(&run_request);
                                    let run_request_stop = Rc::clone(&run_request);
                                    let run_request_clear = Rc::clone(&run_request);
                                    let run_request_remove = Rc::clone(&run_request);

                                    view! {
                                        <div class="agent-control-row">
                                            <div class="agent-control-head">
                                                <span class=format!("status-dot {}", status)></span>
                                                <div>
                                                    <div class="agent-name">{agent_name}</div>
                                                    <div class="agent-task">{agent_scope}</div>
                                                </div>
                                            </div>

                                            <div class="control-actions">
                                                <button
                                                    class="tiny-btn"
                                                    prop:disabled=move || workspace_busy.get() || !can_activate
                                                    on:click=move |_| {
                                                        (run_request_activate)(
                                                            "PATCH".to_string(),
                                                            format!("/api/agents/{}/status", id_activate),
                                                            Some(json!({ "status": "active" })),
                                                        );
                                                    }
                                                >
                                                    "Activate"
                                                </button>

                                                <button
                                                    class="tiny-btn"
                                                    prop:disabled=move || workspace_busy.get() || !can_review
                                                    on:click=move |_| {
                                                        (run_request_review)(
                                                            "PATCH".to_string(),
                                                            format!("/api/agents/{}/status", id_review),
                                                            Some(json!({ "status": "review_pending" })),
                                                        );
                                                    }
                                                >
                                                    "Review"
                                                </button>

                                                <button
                                                    class="tiny-btn"
                                                    prop:disabled=move || workspace_busy.get() || !can_done
                                                    on:click=move |_| {
                                                        (run_request_done)(
                                                            "PATCH".to_string(),
                                                            format!("/api/agents/{}/status", id_done),
                                                            Some(json!({
                                                                "status": "done",
                                                                "kaizen_review_approved": true,
                                                            })),
                                                        );
                                                    }
                                                >
                                                    "Done"
                                                </button>

                                                <button
                                                    class="tiny-btn"
                                                    on:click=move |_| {
                                                        (run_request_stop)(
                                                            "POST".to_string(),
                                                            format!("/api/agents/{}/stop", id_stop),
                                                            None,
                                                        );
                                                    }
                                                >
                                                    "Stop"
                                                </button>

                                                <button
                                                    class="tiny-btn"
                                                    on:click=move |_| {
                                                        (run_request_clear)(
                                                            "POST".to_string(),
                                                            format!("/api/agents/{}/clear", id_clear),
                                                            None,
                                                        );
                                                    }
                                                >
                                                    "Clear Chat"
                                                </button>

                                                <button
                                                    class="tiny-btn danger"
                                                    on:click=move |_| {
                                                        (run_request_remove)(
                                                            "DELETE".to_string(),
                                                            format!("/api/agents/{}", id_remove),
                                                            None,
                                                        );
                                                    }
                                                >
                                                    "Remove"
                                                </button>
                                            </div>
                                        </div>
                                    }
                                }
                            />
                        }
                            .into_view()
                    }
                }}
            </div>
        </section>
    }
}

#[component]
fn MemoryTabView(app_state: AppState) -> impl IntoView {
    let (query, set_query) = create_signal(String::new());
    let (branch_scope, set_branch_scope) = create_signal(String::new());

    let journal_entries = create_memo(move |_| {
        let agents = app_state.agents.get();
        let mut by_task = HashMap::<String, Vec<CrystalBallEvent>>::new();

        for event in app_state.events.get() {
            let task_key = normalize_scope_key(&event.task_id);
            let key = if task_key.is_empty() {
                "general".to_string()
            } else {
                task_key
            };
            by_task.entry(key).or_default().push(event);
        }

        let mut rows = by_task
            .into_iter()
            .map(|(task_id, mut events)| {
                events.sort_by(|left, right| {
                    event_timestamp_seconds(&left.timestamp)
                        .partial_cmp(&event_timestamp_seconds(&right.timestamp))
                        .unwrap_or(Ordering::Equal)
                });

                let first_timestamp = events
                    .first()
                    .map(|event| event.timestamp.clone())
                    .unwrap_or_default();
                let last_timestamp = events
                    .last()
                    .map(|event| event.timestamp.clone())
                    .unwrap_or_default();

                let (branch_id, mission_id) = events
                    .last()
                    .map(|event| infer_event_scope(event, &agents))
                    .unwrap_or_else(|| ("primary".to_string(), "general".to_string()));

                let mut highlights = Vec::new();
                let mut seen = HashSet::<String>::new();
                for event in events.iter().rev() {
                    let line = event.message.trim();
                    if line.is_empty() {
                        continue;
                    }

                    if seen.insert(line.to_string()) {
                        highlights.push(line.to_string());
                    }

                    if highlights.len() == 3 {
                        break;
                    }
                }

                JournalEntryRow {
                    task_id,
                    branch_id,
                    mission_id,
                    first_timestamp,
                    last_timestamp,
                    event_count: events.len(),
                    highlights,
                }
            })
            .collect::<Vec<_>>();

        rows.sort_by(|left, right| {
            event_timestamp_seconds(&right.last_timestamp)
                .partial_cmp(&event_timestamp_seconds(&left.last_timestamp))
                .unwrap_or(Ordering::Equal)
        });
        rows
    });

    let branch_options = create_memo(move |_| {
        let mut values = journal_entries
            .get()
            .into_iter()
            .map(|entry| entry.branch_id)
            .collect::<Vec<_>>();
        values.sort();
        values.dedup();
        values
    });

    let filtered_entries = create_memo(move |_| {
        let needle = query.get().to_lowercase();
        let selected_branch = branch_scope.get();

        journal_entries
            .get()
            .into_iter()
            .filter(|entry| {
                let branch_match = selected_branch.is_empty() || entry.branch_id == selected_branch;
                let text_match = needle.is_empty()
                    || entry.task_id.to_lowercase().contains(&needle)
                    || entry.branch_id.to_lowercase().contains(&needle)
                    || entry.mission_id.to_lowercase().contains(&needle)
                    || entry
                        .highlights
                        .iter()
                        .any(|line| line.to_lowercase().contains(&needle));
                branch_match && text_match
            })
            .collect::<Vec<_>>()
    });

    view! {
        <section class="tab-view">
            <div class="tab-head">
                <h2>"Memory"</h2>
                <p>"Journalized Crystal Ball history grouped by task and scope."</p>
            </div>

            <div class="card toolbar-card">
                <div class="toolbar">
                    <input
                        class="text-input"
                        type="text"
                        placeholder="Search task, branch, mission, memory line..."
                        prop:value=move || query.get()
                        on:input=move |ev| set_query.set(event_target_value(&ev))
                    />

                    <select
                        class="select-input"
                        prop:value=move || branch_scope.get()
                        on:change=move |ev| set_branch_scope.set(event_target_value(&ev))
                    >
                        <option value="">"All branches"</option>
                        <For
                            each=move || branch_options.get()
                            key=|value| value.clone()
                            children=move |value| view! { <option value=value.clone()>{value}</option> }
                        />
                    </select>

                    <span class="tiny-pill">"Entries " {move || filtered_entries.get().len()}</span>
                </div>
            </div>

            {move || {
                if filtered_entries.get().is_empty() {
                    view! { <div class="card"><div class="table-empty">"No memory entries match the current filters."</div></div> }
                        .into_view()
                } else {
                    view! {
                        <div class="journal-grid">
                            <For
                                each=move || filtered_entries.get()
                                key=|entry| entry.task_id.clone()
                                children=move |entry| {
                                    view! {
                                        <article class="card journal-card">
                                            <div class="journal-head">
                                                <div>
                                                    <h3>{format!("Task: {}", entry.task_id)}</h3>
                                                    <div class="journal-meta">
                                                        <span class="tiny-pill">{format!("{} / {}", entry.branch_id, entry.mission_id)}</span>
                                                        <span class="tiny-pill">{format!("{} events", entry.event_count)}</span>
                                                        <span class="tiny-pill">
                                                            {format!(
                                                                "{} -> {}",
                                                                day_bucket_label(&entry.first_timestamp),
                                                                day_bucket_label(&entry.last_timestamp)
                                                            )}
                                                        </span>
                                                    </div>
                                                </div>
                                            </div>

                                            <div class="journal-highlights">
                                                {entry
                                                    .highlights
                                                    .into_iter()
                                                    .map(|line| {
                                                        view! { <div class="journal-line">{line}</div> }
                                                    })
                                                    .collect_view()}
                                            </div>
                                        </article>
                                    }
                                }
                            />
                        </div>
                    }
                        .into_view()
                }
            }}
        </section>
    }
}

#[component]
fn CalendarTabView(app_state: AppState) -> impl IntoView {
    let app_state_for_schedule = app_state.clone();
    let scheduled_rows = create_memo(move |_| {
        let mut workers = app_state_for_schedule.agents.get();
        let status_priority = |status: &AgentStatus| -> u8 {
            match status {
                AgentStatus::Blocked => 0,
                AgentStatus::ReviewPending => 1,
                AgentStatus::Active => 2,
                AgentStatus::Idle => 3,
                AgentStatus::Done => 4,
            }
        };

        workers.sort_by(|left, right| {
            status_priority(&left.status)
                .cmp(&status_priority(&right.status))
                .then_with(|| left.name.cmp(&right.name))
        });

        workers
            .into_iter()
            .enumerate()
            .map(|(index, worker)| {
                let hour = 9 + (index / 2) as i32;
                let minute = if index % 2 == 0 { 0 } else { 30 };
                let directive = match worker.status {
                    AgentStatus::Blocked => "Escalate blocker and unblock execution",
                    AgentStatus::ReviewPending => "Review handoff and approve closure",
                    AgentStatus::Active => "Continue execution sprint",
                    AgentStatus::Idle => "Start mission kickoff",
                    AgentStatus::Done => "Archive output and report",
                }
                .to_string();

                ScheduledTaskRow {
                    slot: format!("{:02}:{:02}", hour, minute),
                    branch_id: branch_label(&worker.branch_id),
                    mission_id: mission_label(&worker),
                    worker_name: worker.name,
                    objective: worker.objective,
                    directive,
                }
            })
            .collect::<Vec<_>>()
    });

    let app_state_for_days = app_state.clone();
    let day_buckets = create_memo(move |_| {
        let agents = app_state_for_days.agents.get();
        let mut grouped = BTreeMap::<String, Vec<ActivityEventRow>>::new();

        for event in app_state_for_days.events.get() {
            let (branch_id, mission_id) = infer_event_scope(&event, &agents);
            grouped
                .entry(day_bucket_label(&event.timestamp))
                .or_default()
                .push(ActivityEventRow {
                    event,
                    branch_id,
                    mission_id,
                });
        }

        let mut days = grouped.into_iter().collect::<Vec<_>>();
        days.sort_by(|left, right| right.0.cmp(&left.0));
        days.truncate(5);

        for (_, rows) in &mut days {
            rows.sort_by(|left, right| {
                event_timestamp_seconds(&right.event.timestamp)
                    .partial_cmp(&event_timestamp_seconds(&left.event.timestamp))
                    .unwrap_or(Ordering::Equal)
            });
            rows.truncate(8);
        }

        days
    });

    view! {
        <section class="tab-view">
            <div class="tab-head">
                <h2>"Calendar"</h2>
                <p>"Auto-scheduled worker queue plus recent multi-day execution timeline."</p>
            </div>

            <div class="calendar-grid">
                <article class="card">
                    <h3>"Scheduled Queue"</h3>
                    {move || {
                        if scheduled_rows.get().is_empty() {
                            view! { <div class="table-empty">"No active workers to schedule."</div> }
                                .into_view()
                        } else {
                            view! {
                                <div class="schedule-list">
                                    <For
                                        each=move || scheduled_rows.get()
                                        key=|row| format!("{}:{}", row.worker_name, row.slot)
                                        children=move |row| {
                                            view! {
                                                <div class="schedule-row">
                                                    <div class="schedule-slot">{row.slot}</div>
                                                    <div class="schedule-body">
                                                        <div class="schedule-title">
                                                            {format!("{} ({}/{})", row.worker_name, row.branch_id, row.mission_id)}
                                                        </div>
                                                        <div class="schedule-copy">{row.directive}</div>
                                                        <div class="schedule-meta">{row.objective}</div>
                                                    </div>
                                                </div>
                                            }
                                        }
                                    />
                                </div>
                            }
                                .into_view()
                        }
                    }}
                </article>

                <article class="card">
                    <h3>"Recent Calendar"</h3>
                    {move || {
                        if day_buckets.get().is_empty() {
                            view! { <div class="table-empty">"No timeline events available yet."</div> }
                                .into_view()
                        } else {
                            view! {
                                <div class="calendar-days">
                                    <For
                                        each=move || day_buckets.get()
                                        key=|day| day.0.clone()
                                        children=move |day| {
                                            let day_label = day.0;
                                            let rows = day.1;
                                            view! {
                                                <div class="calendar-day">
                                                    <div class="calendar-day-head">{day_label}</div>
                                                    <div class="calendar-day-body">
                                                        {rows
                                                            .into_iter()
                                                            .map(|row| {
                                                                view! {
                                                                    <div class="calendar-event">
                                                                        <span class="event-time">{time_bucket_label(&row.event.timestamp)}</span>
                                                                        <span class="event-type">{row.event.event_type}</span>
                                                                        <span class="event-msg">{format!("[{}/{}] {}", row.branch_id, row.mission_id, row.event.message)}</span>
                                                                    </div>
                                                                }
                                                            })
                                                            .collect_view()}
                                                    </div>
                                                </div>
                                            }
                                        }
                                    />
                                </div>
                            }
                                .into_view()
                        }
                    }}
                </article>
            </div>
        </section>
    }
}

#[component]
fn KanbanTabView(app_state: AppState) -> impl IntoView {
    let mission_cards = create_memo(move |_| {
        let mut cards = grouped_branches(&app_state.agents.get())
            .into_iter()
            .flat_map(|(branch_id, missions)| {
                missions.into_iter().map(move |(mission_id, workers)| {
                    let active_count = workers
                        .iter()
                        .filter(|worker| matches!(worker.status, AgentStatus::Active))
                        .count();
                    let blocked_count = workers
                        .iter()
                        .filter(|worker| matches!(worker.status, AgentStatus::Blocked))
                        .count();
                    let review_count = workers
                        .iter()
                        .filter(|worker| matches!(worker.status, AgentStatus::ReviewPending))
                        .count();
                    let done_count = workers
                        .iter()
                        .filter(|worker| matches!(worker.status, AgentStatus::Done))
                        .count();

                    MissionKanbanCard {
                        branch_id: branch_id.clone(),
                        mission_id,
                        workers: workers.clone(),
                        active_count,
                        blocked_count,
                        review_count,
                        done_count,
                        lane: mission_lane_for_workers(&workers),
                    }
                })
            })
            .collect::<Vec<_>>();

        cards.sort_by(|left, right| {
            left.branch_id
                .cmp(&right.branch_id)
                .then_with(|| left.mission_id.cmp(&right.mission_id))
        });
        cards
    });

    view! {
        <section class="tab-view">
            <div class="tab-head">
                <h2>"Kanban"</h2>
                <p>"Mission flow board from backlog to done using live worker state."</p>
            </div>

            {move || {
                if mission_cards.get().is_empty() {
                    view! { <div class="card"><div class="table-empty">"No missions available for Kanban yet."</div></div> }
                        .into_view()
                } else {
                    view! {
                        <div class="kanban-board">
                            {[
                                ("backlog", "Backlog"),
                                ("in_progress", "In Progress"),
                                ("review", "Review"),
                                ("done", "Done"),
                            ]
                                .into_iter()
                                .map(|(lane, title)| {
                                    let cards = mission_cards
                                        .get()
                                        .into_iter()
                                        .filter(|card| card.lane == lane)
                                        .collect::<Vec<_>>();

                                    view! {
                                        <section class=format!("kanban-column lane-{}", lane)>
                                            <div class="kanban-column-head">
                                                <span>{title}</span>
                                                <span class="tiny-pill">{cards.len()}</span>
                                            </div>

                                            <div class="kanban-cards">
                                                {if cards.is_empty() {
                                                    view! { <div class="kanban-empty">"No missions"</div> }.into_view()
                                                } else {
                                                    cards
                                                        .into_iter()
                                                        .map(|card| {
                                                            let MissionKanbanCard {
                                                                branch_id,
                                                                mission_id,
                                                                workers,
                                                                active_count,
                                                                blocked_count,
                                                                review_count,
                                                                done_count,
                                                                ..
                                                            } = card;

                                                            view! {
                                                                <article class="kanban-card">
                                                                    <div class="kanban-title">{mission_id}</div>
                                                                    <div class="kanban-meta">{branch_id}</div>
                                                                    <div class="kanban-stats">
                                                                        <span class="tiny-pill">{format!("active {}", active_count)}</span>
                                                                        <span class="tiny-pill">{format!("blocked {}", blocked_count)}</span>
                                                                        <span class="tiny-pill">{format!("review {}", review_count)}</span>
                                                                        <span class="tiny-pill">{format!("done {}", done_count)}</span>
                                                                    </div>
                                                                    <div class="kanban-workers">
                                                                        {workers
                                                                            .into_iter()
                                                                            .map(|worker| {
                                                                                let status = status_class(&worker.status).to_string();
                                                                                view! {
                                                                                    <div class="kanban-worker">
                                                                                        <span class=format!("status-dot {}", status)></span>
                                                                                        <span>{worker.name}</span>
                                                                                    </div>
                                                                                }
                                                                            })
                                                                            .collect_view()}
                                                                    </div>
                                                                </article>
                                                            }
                                                        })
                                                        .collect_view()
                                                        .into_view()
                                                }}
                                            </div>
                                        </section>
                                    }
                                })
                                .collect_view()}
                        </div>
                    }
                        .into_view()
                }
            }}
        </section>
    }
}

#[component]
fn IntegrationsTabView(app_state: AppState) -> impl IntoView {
    let (gh_status, set_gh_status) = create_signal(None::<GitHubStatusResponse>);
    let (gh_repos, set_gh_repos) = create_signal(Vec::<GitHubRepoSummary>::new());
    let (provider_statuses, set_provider_statuses) =
        create_signal(Vec::<ProviderAuthStatusResponse>::new());
    let (runtime_status, set_runtime_status) = create_signal(None::<ZeroclawRuntimeStatusResponse>);
    let (_current_settings, set_current_settings) = create_signal(None::<KaizenSettings>);
    let (oauth_statuses, set_oauth_statuses) =
        create_signal(HashMap::<String, OAuthStatusResponse>::new());
    let (selected_provider, set_selected_provider) = create_signal("codex-cli".to_string());
    let (selected_model, set_selected_model) = create_signal("gpt-5.4".to_string());
    let (integration_error, set_integration_error) = create_signal(String::new());
    let (integration_notice, set_integration_notice) = create_signal(String::new());
    let (integration_busy, set_integration_busy) = create_signal(false);

    let refresh_integrations: Rc<dyn Fn()> = Rc::new({
        let app_state = app_state.clone();
        move || {
            if integration_busy.get() {
                return;
            }
            set_integration_busy.set(true);
            let app_state = app_state.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let token = app_state.admin_token_opt();
                let mut issues = Vec::<String>::new();
                let mut next_oauth_statuses = HashMap::<String, OAuthStatusResponse>::new();

                match core_request::<KaizenSettings>(CoreRequestInput {
                    method: "GET".to_string(),
                    path: "/api/settings".to_string(),
                    body: None,
                    admin_token: token.clone(),
                })
                .await
                {
                    Ok(settings) => {
                        set_selected_provider.set(settings.inference_provider.clone());
                        set_selected_model.set(settings.inference_model.clone());
                        set_current_settings.set(Some(settings));
                    }
                    Err(err) => issues.push(format!("Settings: {}", err)),
                }

                match core_request::<GitHubStatusResponse>(CoreRequestInput {
                    method: "GET".to_string(),
                    path: "/api/github/status".to_string(),
                    body: None,
                    admin_token: token.clone(),
                })
                .await
                {
                    Ok(status) => set_gh_status.set(Some(status)),
                    Err(err) => issues.push(format!("GitHub status: {}", err)),
                }

                match core_request::<GitHubReposResponse>(CoreRequestInput {
                    method: "GET".to_string(),
                    path: "/api/github/repos?limit=20".to_string(),
                    body: None,
                    admin_token: token.clone(),
                })
                .await
                {
                    Ok(repos) => set_gh_repos.set(repos.repos),
                    Err(err) => issues.push(format!("GitHub repos: {}", err)),
                }

                match core_request::<Vec<ProviderAuthStatusResponse>>(CoreRequestInput {
                    method: "GET".to_string(),
                    path: "/api/providers/status".to_string(),
                    body: None,
                    admin_token: token.clone(),
                })
                .await
                {
                    Ok(rows) => set_provider_statuses.set(rows),
                    Err(err) => issues.push(format!("Provider auth: {}", err)),
                }

                match core_request::<ZeroclawRuntimeStatusResponse>(CoreRequestInput {
                    method: "GET".to_string(),
                    path: "/api/zeroclaw/status".to_string(),
                    body: None,
                    admin_token: token.clone(),
                })
                .await
                {
                    Ok(status) => {
                        set_selected_provider.set(status.active_provider.clone());
                        set_selected_model.set(status.active_model.clone());
                        set_runtime_status.set(Some(status));
                    }
                    Err(err) => issues.push(format!("Zeroclaw runtime: {}", err)),
                }

                match core_request::<OAuthStatusResponse>(CoreRequestInput {
                    method: "GET".to_string(),
                    path: "/api/oauth/gemini/status".to_string(),
                    body: None,
                    admin_token: token,
                })
                .await
                {
                    Ok(status) => {
                        next_oauth_statuses.insert("gemini".to_string(), status);
                    }
                    Err(err) => issues.push(format!("Gemini OAuth: {}", err)),
                }

                set_oauth_statuses.set(next_oauth_statuses);
                set_integration_error.set(issues.join(" | "));
                set_integration_busy.set(false);
            });
        }
    });

    let schedule_oauth_poll: Rc<dyn Fn()> = Rc::new({
        let refresh_integrations = Rc::clone(&refresh_integrations);
        move || {
            let refresh_integrations = Rc::clone(&refresh_integrations);
            wasm_bindgen_futures::spawn_local(async move {
                for _ in 0..12 {
                    TimeoutFuture::new(2500).await;
                    (refresh_integrations)();
                }
            });
        }
    });

    let start_gemini_oauth: Rc<dyn Fn()> = Rc::new({
        let app_state = app_state.clone();
        let schedule_oauth_poll = Rc::clone(&schedule_oauth_poll);
        let refresh_integrations = Rc::clone(&refresh_integrations);
        move || {
            if integration_busy.get() {
                return;
            }

            set_integration_busy.set(true);
            let app_state = app_state.clone();
            let schedule_oauth_poll = Rc::clone(&schedule_oauth_poll);
            let refresh_integrations = Rc::clone(&refresh_integrations);
            wasm_bindgen_futures::spawn_local(async move {
                match core_request::<OAuthStartResponse>(CoreRequestInput {
                    method: "GET".to_string(),
                    path: "/api/oauth/gemini/start".to_string(),
                    body: None,
                    admin_token: app_state.admin_token_opt(),
                })
                .await
                {
                    Ok(start) => {
                        let open_result = open_external_browser(start.redirect_url.clone()).await;
                        if open_result.is_err() {
                            if let Some(window) = web_sys::window() {
                                let _ = window.open_with_url(&start.redirect_url);
                            }
                        }
                        set_integration_error.set(String::new());
                        set_integration_notice
                            .set("Gemini OAuth opened in your browser.".to_string());
                        (schedule_oauth_poll)();
                    }
                    Err(err) => {
                        set_integration_notice.set(String::new());
                        set_integration_error.set(format!("Gemini OAuth start failed: {}", err));
                    }
                }
                set_integration_busy.set(false);
                (refresh_integrations)();
            });
        }
    });

    let refresh_gemini_oauth: Rc<dyn Fn()> = Rc::new({
        let app_state = app_state.clone();
        let refresh_integrations = Rc::clone(&refresh_integrations);
        move || {
            if integration_busy.get() {
                return;
            }

            set_integration_busy.set(true);
            let app_state = app_state.clone();
            let refresh_integrations = Rc::clone(&refresh_integrations);
            wasm_bindgen_futures::spawn_local(async move {
                match core_request::<Value>(CoreRequestInput {
                    method: "POST".to_string(),
                    path: "/api/oauth/gemini/refresh".to_string(),
                    body: None,
                    admin_token: app_state.admin_token_opt(),
                })
                .await
                {
                    Ok(_) => {
                        set_integration_error.set(String::new());
                        set_integration_notice.set("Gemini OAuth token refreshed.".to_string());
                    }
                    Err(err) => {
                        set_integration_notice.set(String::new());
                        set_integration_error.set(format!("Gemini OAuth refresh failed: {}", err));
                    }
                }
                set_integration_busy.set(false);
                (refresh_integrations)();
            });
        }
    });

    let disconnect_gemini_oauth: Rc<dyn Fn()> = Rc::new({
        let app_state = app_state.clone();
        let refresh_integrations = Rc::clone(&refresh_integrations);
        move || {
            if integration_busy.get() {
                return;
            }

            set_integration_busy.set(true);
            let app_state = app_state.clone();
            let refresh_integrations = Rc::clone(&refresh_integrations);
            wasm_bindgen_futures::spawn_local(async move {
                match core_request::<Value>(CoreRequestInput {
                    method: "DELETE".to_string(),
                    path: "/api/oauth/gemini".to_string(),
                    body: None,
                    admin_token: app_state.admin_token_opt(),
                })
                .await
                {
                    Ok(_) => {
                        set_integration_error.set(String::new());
                        set_integration_notice.set("Gemini OAuth disconnected.".to_string());
                    }
                    Err(err) => {
                        set_integration_notice.set(String::new());
                        set_integration_error
                            .set(format!("Gemini OAuth disconnect failed: {}", err));
                    }
                }
                set_integration_busy.set(false);
                (refresh_integrations)();
            });
        }
    });

    {
        let refresh_integrations = Rc::clone(&refresh_integrations);
        create_effect(move |_| {
            untrack({
                let refresh_integrations = Rc::clone(&refresh_integrations);
                move || (refresh_integrations)()
            });
            let refresh_integrations = Rc::clone(&refresh_integrations);
            if let Ok(handle) =
                set_interval_with_handle(move || (refresh_integrations)(), Duration::from_secs(20))
            {
                on_cleanup(move || handle.clear());
            }
        });
    }

    let refresh_integrations_top = Rc::clone(&refresh_integrations);
    let gemini_oauth = create_memo(move |_| oauth_statuses.get().get("gemini").cloned());
    let codex_status = create_memo(move |_| {
        provider_statuses
            .get()
            .into_iter()
            .find(|row| row.provider == "codex-cli")
    });
    let gemini_status = create_memo(move |_| {
        provider_statuses
            .get()
            .into_iter()
            .find(|row| row.provider == "gemini")
    });
    let gemini_cli_status = create_memo(move |_| {
        provider_statuses
            .get()
            .into_iter()
            .find(|row| row.provider == "gemini-cli")
    });
    let runtime_provider_options = create_memo(move |_| {
        runtime_status
            .get()
            .map(|status| status.providers)
            .unwrap_or_default()
    });
    let selected_provider_models = create_memo(move |_| {
        runtime_provider_options
            .get()
            .into_iter()
            .find(|provider| provider.id == selected_provider.get())
            .map(|provider| provider.models)
            .unwrap_or_default()
    });

    let save_route: Rc<dyn Fn(String, String)> = Rc::new({
        let app_state = app_state.clone();
        let refresh_integrations = Rc::clone(&refresh_integrations);
        move |provider: String, model: String| {
            if integration_busy.get() {
                return;
            }

            set_integration_busy.set(true);
            set_selected_provider.set(provider.clone());
            set_selected_model.set(model.clone());
            let app_state = app_state.clone();
            let refresh_integrations = Rc::clone(&refresh_integrations);
            wasm_bindgen_futures::spawn_local(async move {
                match core_request::<KaizenSettings>(CoreRequestInput {
                    method: "PATCH".to_string(),
                    path: "/api/settings".to_string(),
                    body: Some(json!({
                        "inference_provider": provider,
                        "inference_model": model,
                    })),
                    admin_token: app_state.admin_token_opt(),
                })
                .await
                {
                    Ok(settings) => {
                        set_current_settings.set(Some(settings));
                        set_integration_error.set(String::new());
                        set_integration_notice.set("Zeroclaw updated.".to_string());
                    }
                    Err(err) => {
                        set_integration_notice.set(String::new());
                        set_integration_error.set(format!("Could not save Zeroclaw route: {}", err));
                    }
                }
                set_integration_busy.set(false);
                (refresh_integrations)();
            });
        }
    });

    let start_codex_auth: Rc<dyn Fn()> = Rc::new({
        let schedule_oauth_poll = Rc::clone(&schedule_oauth_poll);
        let refresh_integrations = Rc::clone(&refresh_integrations);
        move || {
            if integration_busy.get() {
                return;
            }

            set_integration_busy.set(true);
            let schedule_oauth_poll = Rc::clone(&schedule_oauth_poll);
            let refresh_integrations = Rc::clone(&refresh_integrations);
            wasm_bindgen_futures::spawn_local(async move {
                match start_local_auth_flow("codex-cli").await {
                    Ok(action) => {
                        set_integration_error.set(String::new());
                        set_integration_notice.set(action.message);
                        (schedule_oauth_poll)();
                    }
                    Err(err) => {
                        set_integration_notice.set(String::new());
                        set_integration_error.set(format!("Codex sign-in failed: {}", err));
                    }
                }
                set_integration_busy.set(false);
                (refresh_integrations)();
            });
        }
    });

    let start_gemini_cli_auth: Rc<dyn Fn()> = Rc::new({
        let schedule_oauth_poll = Rc::clone(&schedule_oauth_poll);
        let refresh_integrations = Rc::clone(&refresh_integrations);
        move || {
            if integration_busy.get() {
                return;
            }

            set_integration_busy.set(true);
            let schedule_oauth_poll = Rc::clone(&schedule_oauth_poll);
            let refresh_integrations = Rc::clone(&refresh_integrations);
            wasm_bindgen_futures::spawn_local(async move {
                match start_local_auth_flow("gemini-cli").await {
                    Ok(action) => {
                        set_integration_error.set(String::new());
                        set_integration_notice.set(action.message);
                        (schedule_oauth_poll)();
                    }
                    Err(err) => {
                        set_integration_notice.set(String::new());
                        set_integration_error.set(format!("Gemini CLI sign-in failed: {}", err));
                    }
                }
                set_integration_busy.set(false);
                (refresh_integrations)();
            });
        }
    });

    let save_route_for_selector = Rc::clone(&save_route);
    let start_codex_auth_card = Rc::clone(&start_codex_auth);
    let start_gemini_oauth_card = Rc::clone(&start_gemini_oauth);
    let refresh_gemini_oauth_card = Rc::clone(&refresh_gemini_oauth);
    let disconnect_gemini_oauth_card = Rc::clone(&disconnect_gemini_oauth);
    let start_gemini_cli_auth_card = Rc::clone(&start_gemini_cli_auth);

    view! {
        <section class="tab-view">
            <div class="tab-head">
                <h2>"Integrations"</h2>
                <p>"Set up Zeroclaw once, then let it handle the runtime."</p>
            </div>

            <div class="toolbar-inline" style="margin-bottom: 12px;">
                <button
                    class="action-btn"
                    prop:disabled=move || integration_busy.get()
                    on:click=move |_| (refresh_integrations_top)()
                >
                    {move || if integration_busy.get() { "Refreshing..." } else { "Refresh Integrations" }}
                </button>
            </div>

            {move || {
                if integration_notice.get().is_empty() {
                    ().into_view()
                } else {
                    view! { <div class="notice neutral">{integration_notice.get()}</div> }.into_view()
                }
            }}

            {move || {
                if integration_error.get().is_empty() {
                    ().into_view()
                } else {
                    view! { <div class="notice error">{integration_error.get()}</div> }.into_view()
                }
            }}

            <div class="card">
                <h3>"Zeroclaw"</h3>
                <div class="list-stack compact" style="margin-bottom: 12px;">
                    <div>{move || {
                        runtime_status
                            .get()
                            .map(|status| status.message)
                            .unwrap_or_else(|| "Zeroclaw runtime is loading.".to_string())
                    }}</div>
                    <div class="muted">{move || {
                        runtime_status
                            .get()
                            .map(|status| format!("{} connected account(s).", status.connected_accounts))
                            .unwrap_or_else(|| "Checking connected accounts.".to_string())
                    }}</div>
                </div>
                <div class="form-grid two-col" style="align-items: end;">
                    <label>
                        <span>"Provider"</span>
                        <select
                            class="select-input"
                            prop:value=move || selected_provider.get()
                            prop:disabled=move || integration_busy.get()
                            on:change=move |ev| set_selected_provider.set(event_target_value(&ev))
                        >
                            <For
                                each=move || runtime_provider_options.get()
                                key=|provider| provider.id.clone()
                                children=move |provider| {
                                    view! {
                                        <option value={provider.id.clone()}>{provider.label}</option>
                                    }
                                }
                            />
                        </select>
                    </label>
                    <label>
                        <span>"Model"</span>
                        <select
                            class="select-input"
                            prop:value=move || selected_model.get()
                            prop:disabled=move || integration_busy.get() || selected_provider_models.get().is_empty()
                            on:change=move |ev| set_selected_model.set(event_target_value(&ev))
                        >
                            <For
                                each=move || selected_provider_models.get()
                                key=|model| model.clone()
                                children=move |model| {
                                    view! {
                                        <option value={model.clone()}>{model}</option>
                                    }
                                }
                            />
                        </select>
                    </label>
                </div>
                <div class="toolbar-inline" style="margin-top: 12px;">
                    <button
                        class="action-btn"
                        prop:disabled=move || integration_busy.get()
                        on:click=move |_| (save_route_for_selector)(selected_provider.get(), selected_model.get())
                    >
                        "Save Zeroclaw"
                    </button>
                </div>
            </div>

            <div class="card">
                <h3>"Tools"</h3>
                <div class="list-stack compact">
                    {move || {
                        if let Some(status) = runtime_status.get() {
                            view! {
                                <For
                                    each=move || status.tools.clone()
                                    key=|tool| tool.id.clone()
                                    children=move |tool| {
                                        let state = if tool.available && tool.connected {
                                            "Ready"
                                        } else if tool.available {
                                            "Available"
                                        } else {
                                            "Planned"
                                        };
                                        view! {
                                            <div class="repo-row">
                                                <span>{format!("{} · {}", tool.label, state)}</span>
                                                <span>{tool.message}</span>
                                            </div>
                                        }
                                    }
                                />
                            }.into_view()
                        } else {
                            view! { <div class="muted">"Tool status is loading."</div> }.into_view()
                        }
                    }}
                </div>
            </div>

            <div class="card-grid two-col">
                <article class="card">
                    <h3>"Codex"</h3>
                    {move || {
                        if let Some(status) = codex_status.get() {
                            let start_codex_auth_click = Rc::clone(&start_codex_auth_card);
                            view! {
                                <div class="list-stack">
                                    <div>{if status.can_chat { "Signed in." } else { "Not connected yet." }}</div>
                                    <div>{status.message}</div>
                                    <div class="toolbar-inline">
                                        <button
                                            class="action-btn"
                                            prop:disabled=move || integration_busy.get()
                                            on:click=move |_| (start_codex_auth_click)()
                                        >
                                            "Add Account"
                                        </button>
                                    </div>
                                </div>
                            }
                                .into_view()
                        } else {
                            view! { <div class="muted">"Codex status is loading."</div> }.into_view()
                        }
                    }}
                </article>

                <article class="card">
                    <h3>"Gemini"</h3>
                    {move || {
                        if let Some(status) = gemini_status.get() {
                            let oauth = gemini_oauth.get();
                            let start_gemini_oauth_click = Rc::clone(&start_gemini_oauth_card);
                            let refresh_gemini_oauth_click = Rc::clone(&refresh_gemini_oauth_card);
                            let disconnect_gemini_oauth_click = Rc::clone(&disconnect_gemini_oauth_card);
                            view! {
                                <div class="list-stack">
                                    <div>{status.message}</div>
                                    <div class="toolbar-inline">
                                        <button
                                            class="action-btn"
                                            prop:disabled=move || integration_busy.get()
                                            on:click=move |_| (start_gemini_oauth_click)()
                                        >
                                            "Connect"
                                        </button>
                                    </div>
                                    {move || {
                                        if let Some(oauth) = oauth.clone() {
                                            if oauth.connected {
                                                let refresh_gemini_oauth_button = Rc::clone(&refresh_gemini_oauth_click);
                                                let disconnect_gemini_oauth_button = Rc::clone(&disconnect_gemini_oauth_click);
                                                view! {
                                                    <div class="toolbar-inline">
                                                        <button
                                                            class="action-btn subtle"
                                                            prop:disabled=move || integration_busy.get()
                                                            on:click=move |_| (refresh_gemini_oauth_button)()
                                                        >
                                                            "Refresh"
                                                        </button>
                                                        <button
                                                            class="action-btn danger"
                                                            prop:disabled=move || integration_busy.get()
                                                            on:click=move |_| (disconnect_gemini_oauth_button)()
                                                        >
                                                            "Disconnect"
                                                        </button>
                                                    </div>
                                                }.into_view()
                                            } else {
                                                ().into_view()
                                            }
                                        } else {
                                            ().into_view()
                                        }
                                    }}
                                </div>
                            }
                                .into_view()
                        } else {
                            view! { <div class="muted">"Gemini status is loading."</div> }.into_view()
                        }
                    }}
                </article>
            </div>

            <details class="card details-card">
                <summary class="details-summary">"Advanced"</summary>
                <div class="card-grid two-col" style="margin-top: 12px;">
                    <article class="card inner-card">
                        <h3>"Gemini CLI"</h3>
                        {move || {
                            let start_gemini_cli_auth_click = Rc::clone(&start_gemini_cli_auth_card);
                            view! {
                                <div class="list-stack compact">
                                    <div>{move || {
                                        gemini_cli_status
                                            .get()
                                            .map(|row| row.message)
                                            .unwrap_or_else(|| "Gemini CLI status is loading.".to_string())
                                    }}</div>
                                    <div class="toolbar-inline">
                                        <button
                                            class="action-btn subtle"
                                            prop:disabled=move || integration_busy.get()
                                            on:click=move |_| (start_gemini_cli_auth_click)()
                                        >
                                            "Add Account"
                                        </button>
                                    </div>
                                </div>
                            }
                                .into_view()
                        }}
                    </article>

                    <article class="card inner-card">
                        <h3>"Other Providers"</h3>
                        <div class="list-stack compact">
                            <div>"OpenAI, Anthropic, and NVIDIA use API keys today."</div>
                            <div>"Multiple OpenAI accounts are not supported yet."</div>
                            <div>"Kilocode is not integrated yet."</div>
                        </div>
                    </article>

                    <article class="card inner-card">
                        <h3>"GitHub"</h3>
                        {move || {
                            if let Some(status) = gh_status.get() {
                                view! {
                                    <div class="list-stack compact">
                                        <div>{if status.authenticated { "Connected." } else { "Not connected." }}</div>
                                        <div>{status.login.unwrap_or_else(|| status.host)}</div>
                                    </div>
                                }.into_view()
                            } else {
                                view! { <div class="muted">"GitHub status is loading."</div> }.into_view()
                            }
                        }}
                    </article>

                    <article class="card inner-card">
                        <h3>"Repositories"</h3>
                        {move || {
                            if gh_repos.get().is_empty() {
                                view! { <div class="muted">"No repositories loaded."</div> }.into_view()
                            } else {
                                view! {
                                    <For
                                        each=move || gh_repos.get()
                                        key=|repo| repo.name_with_owner.clone()
                                        children=move |repo| {
                                            view! {
                                                <div class="repo-row">
                                                    <span>{repo.name_with_owner}</span>
                                                    <span>{repo.viewer_permission}</span>
                                                </div>
                                            }
                                        }
                                    />
                                }.into_view()
                            }
                        }}
                    </article>
                </div>
            </details>
        </section>
    }
}

#[component]
fn SettingsTabView(app_state: AppState) -> impl IntoView {
    let (settings_notice, set_settings_notice) = create_signal(String::new());
    let (settings_busy, set_settings_busy) = create_signal(false);

    let (runtime_engine, set_runtime_engine) = create_signal(String::new());
    let (inference_provider, set_inference_provider) = create_signal(String::new());
    let (inference_model, set_inference_model) = create_signal(String::new());
    let (max_subagents, set_max_subagents) = create_signal(String::new());
    let (inference_max_tokens, set_inference_max_tokens) = create_signal(String::new());
    let (inference_temperature, set_inference_temperature) = create_signal(String::new());
    let (selected_repo, set_selected_repo) = create_signal(String::new());

    let (auto_spawn_subagents, set_auto_spawn_subagents) = create_signal(false);
    let (orchestrator_full_control, set_orchestrator_full_control) = create_signal(true);
    let (allow_direct_chat, set_allow_direct_chat) = create_signal(true);
    let (hard_gates_enabled, set_hard_gates_enabled) = create_signal(true);
    let (human_smoke_required, set_human_smoke_required) = create_signal(true);

    let refresh_settings: Rc<dyn Fn()> = Rc::new({
        let app_state = app_state.clone();
        move || {
            if settings_busy.get() {
                return;
            }
            set_settings_busy.set(true);
            let app_state = app_state.clone();
            wasm_bindgen_futures::spawn_local(async move {
                match core_request::<KaizenSettings>(CoreRequestInput {
                    method: "GET".to_string(),
                    path: "/api/settings".to_string(),
                    body: None,
                    admin_token: app_state.admin_token_opt(),
                })
                .await
                {
                    Ok(settings) => {
                        set_runtime_engine.set(settings.runtime_engine);
                        set_inference_provider.set(settings.inference_provider);
                        set_inference_model.set(settings.inference_model);
                        set_max_subagents.set(settings.max_subagents.to_string());
                        set_inference_max_tokens.set(settings.inference_max_tokens.to_string());
                        set_inference_temperature.set(settings.inference_temperature.to_string());
                        set_selected_repo.set(settings.selected_github_repo);
                        set_auto_spawn_subagents.set(settings.auto_spawn_subagents);
                        set_orchestrator_full_control.set(settings.orchestrator_full_control);
                        set_allow_direct_chat.set(settings.allow_direct_user_to_subagent_chat);
                        set_hard_gates_enabled.set(settings.hard_gates_enabled);
                        set_human_smoke_required
                            .set(settings.require_human_smoke_test_before_deploy);
                        set_settings_notice.set("Settings loaded.".to_string());
                    }
                    Err(err) => set_settings_notice.set(format!(
                        "Failed to load settings. Check admin token. {}",
                        err
                    )),
                }
                set_settings_busy.set(false);
            });
        }
    });

    {
        let refresh_settings = Rc::clone(&refresh_settings);
        create_effect(move |_| {
            (refresh_settings)();
        });
    }

    let save_settings = {
        let app_state = app_state.clone();
        move |_| {
            if settings_busy.get() {
                return;
            }

            let max_subagents_value = max_subagents.get().parse::<u32>().unwrap_or(5);
            let max_tokens_value = inference_max_tokens.get().parse::<u32>().unwrap_or(4096);
            let temperature_value = inference_temperature.get().parse::<f32>().unwrap_or(0.7);

            set_settings_busy.set(true);
            let app_state = app_state.clone();
            wasm_bindgen_futures::spawn_local(async move {
                match core_request::<KaizenSettings>(CoreRequestInput {
                    method: "PATCH".to_string(),
                    path: "/api/settings".to_string(),
                    body: Some(json!({
                        "runtime_engine": runtime_engine.get(),
                        "inference_provider": inference_provider.get(),
                        "inference_model": inference_model.get(),
                        "max_subagents": max_subagents_value,
                        "inference_max_tokens": max_tokens_value,
                        "inference_temperature": temperature_value,
                        "selected_github_repo": selected_repo.get(),
                        "auto_spawn_subagents": auto_spawn_subagents.get(),
                        "orchestrator_full_control": orchestrator_full_control.get(),
                        "allow_direct_user_to_subagent_chat": allow_direct_chat.get(),
                        "hard_gates_enabled": hard_gates_enabled.get(),
                        "require_human_smoke_test_before_deploy": human_smoke_required.get(),
                    })),
                    admin_token: app_state.admin_token_opt(),
                })
                .await
                {
                    Ok(_) => set_settings_notice.set("Settings saved.".to_string()),
                    Err(err) => set_settings_notice.set(format!("Save failed: {}", err)),
                }
                set_settings_busy.set(false);
            });
        }
    };

    view! {
        <section class="tab-view">
            <div class="tab-head">
                <h2>"Settings"</h2>
                <p>"Configure runtime, inference, and governance settings."</p>
            </div>

            <div class="card">
                <h3>"Admin Access"</h3>
                <div class="form-grid single-col">
                    <label>
                        <span>"Admin API Token (stored locally)"</span>
                        <input
                            class="text-input"
                            type="password"
                            prop:value=move || app_state.admin_token.get()
                            on:input=move |ev| {
                                let value = event_target_value(&ev);
                                app_state.admin_token.set(value.clone());
                                persist_admin_token(&value);
                            }
                        />
                    </label>
                </div>
            </div>

            <div class="card-grid two-col">
                <article class="card">
                    <h3>"Runtime"</h3>
                    <div class="form-grid">
                        <label>
                            <span>"Runtime Engine"</span>
                            <input class="text-input" prop:value=move || runtime_engine.get() on:input=move |ev| set_runtime_engine.set(event_target_value(&ev)) />
                        </label>
                        <label>
                            <span>"Max Subagents"</span>
                            <input class="text-input" type="number" prop:value=move || max_subagents.get() on:input=move |ev| set_max_subagents.set(event_target_value(&ev)) />
                        </label>
                        <label class="check-row">
                            <input type="checkbox" checked=move || auto_spawn_subagents.get() on:change=move |ev| set_auto_spawn_subagents.set(event_target_checked(&ev)) />
                            <span>"Auto spawn sub-agents"</span>
                        </label>
                        <label class="check-row">
                            <input type="checkbox" checked=move || orchestrator_full_control.get() on:change=move |ev| set_orchestrator_full_control.set(event_target_checked(&ev)) />
                            <span>"Orchestrator full control (autonomous staffing)"</span>
                        </label>
                        <label class="check-row">
                            <input type="checkbox" checked=move || allow_direct_chat.get() on:change=move |ev| set_allow_direct_chat.set(event_target_checked(&ev)) />
                            <span>"Allow direct user-to-subagent chat"</span>
                        </label>
                    </div>
                </article>

                <article class="card">
                    <h3>"Inference"</h3>
                    <div class="form-grid">
                        <label>
                            <span>"Provider"</span>
                            <input class="text-input" prop:value=move || inference_provider.get() on:input=move |ev| set_inference_provider.set(event_target_value(&ev)) />
                        </label>
                        <label>
                            <span>"Model"</span>
                            <input class="text-input" prop:value=move || inference_model.get() on:input=move |ev| set_inference_model.set(event_target_value(&ev)) />
                        </label>
                        <label>
                            <span>"Max Tokens"</span>
                            <input class="text-input" type="number" prop:value=move || inference_max_tokens.get() on:input=move |ev| set_inference_max_tokens.set(event_target_value(&ev)) />
                        </label>
                        <label>
                            <span>"Temperature"</span>
                            <input class="text-input" type="number" step="0.1" prop:value=move || inference_temperature.get() on:input=move |ev| set_inference_temperature.set(event_target_value(&ev)) />
                        </label>
                    </div>
                </article>
            </div>

            <div class="card">
                <h3>"Governance + Repo"</h3>
                <div class="form-grid">
                    <label>
                        <span>"Selected GitHub Repo"</span>
                        <input class="text-input" prop:value=move || selected_repo.get() on:input=move |ev| set_selected_repo.set(event_target_value(&ev)) />
                    </label>
                    <label class="check-row">
                        <input type="checkbox" checked=move || hard_gates_enabled.get() on:change=move |ev| set_hard_gates_enabled.set(event_target_checked(&ev)) />
                        <span>"Hard gates enabled"</span>
                    </label>
                    <label class="check-row">
                        <input type="checkbox" checked=move || human_smoke_required.get() on:change=move |ev| set_human_smoke_required.set(event_target_checked(&ev)) />
                        <span>"Require human smoke test before deploy"</span>
                    </label>
                </div>

                <div class="toolbar-inline" style="margin-top: 10px;">
                    <button class="action-btn" prop:disabled=move || settings_busy.get() on:click=move |_| (refresh_settings)()>
                        {move || if settings_busy.get() { "Working..." } else { "Refresh" }}
                    </button>
                    <button class="action-btn" prop:disabled=move || settings_busy.get() on:click=save_settings>
                        {move || if settings_busy.get() { "Saving..." } else { "Save Settings" }}
                    </button>
                </div>
            </div>

            {move || {
                if settings_notice.get().is_empty() {
                    ().into_view()
                } else {
                    view! { <div class="notice neutral">{settings_notice.get()}</div> }.into_view()
                }
            }}
        </section>
    }
}

#[component]
pub fn MainMissionView() -> impl IntoView {
    let app_state = use_context::<AppState>().unwrap_or_else(AppState::new);
    let health = app_state.health;

    let left_width = create_rw_signal(278);
    let right_width = create_rw_signal(320);
    let dragging = create_rw_signal(None::<&'static str>);
    let detached_windows = create_rw_signal(HashSet::<String>::new());

    window_event_listener(ev::mousemove, move |ev| {
        if let Some(side) = dragging.get_untracked() {
            let x = ev.client_x();
            if side == "left" {
                left_width.set(x.max(170).min(520));
            } else if let Some(win) = web_sys::window() {
                let window_width = win
                    .inner_width()
                    .ok()
                    .and_then(|value| value.as_f64())
                    .unwrap_or(1280.0) as i32;
                right_width.set((window_width - x).max(240).min(620));
            }
        }
    });

    window_event_listener(ev::mouseup, move |_| {
        dragging.set(None);
    });

    let app_state_for_tabs = app_state.clone();

    view! {
        <div class="app-shell">
            <div
                class="mission-layout"
                style=move || {
                    format!(
                        "grid-template-columns: {}px 4px 1fr 4px {}px",
                        left_width.get(),
                        right_width.get()
                    )
                }
            >
                <nav class="nav-rail">
                    <div class="brand">
                        <img class="brand-lockup-image" src="/headerlogo.png" alt="Kaizen Innovations" />
                        <div class="brand-subtitle">"Mission Control"</div>
                    </div>

                    <div class="section-block">
                        <div class="panel-title">"Views"</div>
                        <div class="nav-tabs">
                            <For
                                each=move || {
                                    vec![
                                        TabId::Mission,
                                        TabId::Branches,
                                        TabId::Kanban,
                                        TabId::Gates,
                                        TabId::Activity,
                                        TabId::Memory,
                                        TabId::Calendar,
                                        TabId::Workspace,
                                        TabId::Integrations,
                                        TabId::Settings,
                                    ]
                                }
                                key=|tab| tab_label(tab).to_string()
                                children=move |tab| {
                                    let tab_for_click = tab.clone();
                                    let tab_for_class = tab.clone();
                                    let tab_text = tab_label(&tab).to_string();
                                    view! {
                                        <button
                                            class=move || {
                                                if app_state.active_tab.get() == tab_for_class.clone() {
                                                    "nav-item active"
                                                } else {
                                                    "nav-item"
                                                }
                                            }
                                            on:click=move |_| app_state.active_tab.set(tab_for_click.clone())
                                        >
                                            {tab_text}
                                        </button>
                                    }
                                }
                            />
                        </div>
                    </div>

                    <div class="section-block grow">
                        <div class="panel-title">"Agents"</div>
                        <div class="agent-list">
                            {move || {
                                if app_state.agents.get().is_empty() {
                                    view! { <div class="agent-empty">"No active sub-agents."</div> }.into_view()
                                } else {
                                    view! {
                                        <For
                                            each=move || app_state.agents.get()
                                            key=|agent| agent.id.clone()
                                            children=move |agent| {
                                                let id_for_click = agent.id.clone();
                                                let id_for_class = agent.id.clone();
                                                let id_for_label = agent.id.clone();
                                                let status = status_class(&agent.status).to_string();
                                                let agent_name = agent.name.clone();
                                                let agent_scope = format!(
                                                    "{} / {}",
                                                    branch_label(&agent.branch_id),
                                                    mission_label(&agent)
                                                );

                                                view! {
                                                    <div class="agent-item">
                                                        <div class="agent-meta">
                                                            <span class=format!("status-dot {}", status)></span>
                                                            <div>
                                                                <div class="agent-name">{agent_name}</div>
                                                                <div class="agent-task">{agent_scope}</div>
                                                            </div>
                                                        </div>

                                                        <button
                                                            class=move || {
                                                                if detached_windows.with(|set| set.contains(&id_for_class)) {
                                                                    "agent-action detached"
                                                                } else {
                                                                    "agent-action"
                                                                }
                                                            }
                                                            on:click=move |_| {
                                                                let id = id_for_click.clone();
                                                                let focused = detached_windows.with(|set| set.contains(&id));
                                                                wasm_bindgen_futures::spawn_local(async move {
                                                                    if focused {
                                                                        focus_agent(id.clone()).await;
                                                                    } else {
                                                                        detach_agent(id.clone()).await;
                                                                    }
                                                                    detached_windows.update(|set| {
                                                                        set.insert(id);
                                                                    });
                                                                });
                                                            }
                                                        >
                                                            {move || {
                                                                if detached_windows.with(|set| set.contains(&id_for_label)) {
                                                                    "Focus"
                                                                } else {
                                                                    "Detach"
                                                                }
                                                            }}
                                                        </button>
                                                    </div>
                                                }
                                            }
                                        />
                                    }
                                        .into_view()
                                }
                            }}
                        </div>
                    </div>

                </nav>

                <div
                    class="resizer-h"
                    class:active=move || dragging.get() == Some("left")
                    on:mousedown=move |_| dragging.set(Some("left"))
                />

                <main class="main-shell">
                    <header class="top-bar">
                        <div class="status-block">
                            <span
                                class=move || {
                                    if health
                                        .get()
                                        .map(|h| h.status.eq_ignore_ascii_case("ok"))
                                        .unwrap_or(false)
                                    {
                                        "status-chip ok"
                                    } else {
                                        "status-chip"
                                    }
                                }
                            >
                                "Gateway "
                                {move || {
                                    health
                                        .get()
                                        .map(|h| h.status.to_uppercase())
                                        .unwrap_or_else(|| "OFFLINE".to_string())
                                }}
                            </span>
                            <span class="status-meta">
                                {move || {
                                    health
                                        .get()
                                        .map(|h| format!("{} v{}", h.engine, h.version))
                                        .unwrap_or_else(|| "engine unavailable".to_string())
                                }}
                            </span>
                        </div>

                        <div class="top-metrics">
                            {let app_state_for_banner = app_state.clone();
                            move || {
                                let update_status = app_state_for_banner.release_update.get();
                                if let Some(status) = update_status {
                                    if status.update_available {
                                        let app_state_for_update = app_state_for_banner.clone();
                                        let app_state_for_click = app_state_for_update.clone();
                                        view! {
                                            <div class="update-cta">
                                                <span class="count-pill update-pill">
                                                    {format!("Update ready +{}", status.behind_count)}
                                                </span>
                                                <button
                                                    class="tiny-btn active"
                                                    prop:disabled=move || app_state_for_update.update_busy.get()
                                                    on:click=move |_| {
                                                        let app_state = app_state_for_click.clone();
                                                        app_state.update_busy.set(true);
                                                        wasm_bindgen_futures::spawn_local(async move {
                                                            match apply_release_update().await {
                                                                Ok(action) => {
                                                                    app_state.update_notice.set(action.message);
                                                                }
                                                                Err(error) => {
                                                                    app_state.update_busy.set(false);
                                                                    app_state.update_notice.set(error);
                                                                    let _ = app_state.refresh_release_update().await;
                                                                }
                                                            }
                                                        });
                                                    }
                                                >
                                                    {move || {
                                                        if app_state_for_update.update_busy.get() {
                                                            "Updating..."
                                                        } else {
                                                            "Apply Update"
                                                        }
                                                    }}
                                                </button>
                                            </div>
                                        }
                                        .into_view()
                                    } else if !app_state_for_banner.update_notice.get().is_empty() {
                                        view! {
                                            <span class="count-pill">
                                                {app_state_for_banner.update_notice.get()}
                                            </span>
                                        }
                                        .into_view()
                                    } else {
                                        ().into_view()
                                    }
                                } else {
                                    ().into_view()
                                }
                            }}
                            <span class="count-pill">"Agents " {move || app_state.agents.get().len()}</span>
                            <span class="count-pill">"Events " {move || app_state.events.get().len()}</span>
                        </div>
                    </header>

                    {move || {
                        match app_state_for_tabs.active_tab.get() {
                            TabId::Mission => view! { <MissionTabView app_state=app_state_for_tabs.clone()/> }.into_view(),
                            TabId::Branches => view! { <BranchesTabView app_state=app_state_for_tabs.clone()/> }.into_view(),
                            TabId::Kanban => view! { <KanbanTabView app_state=app_state_for_tabs.clone()/> }.into_view(),
                            TabId::Gates => view! { <GatesTabView app_state=app_state_for_tabs.clone()/> }.into_view(),
                            TabId::Activity => view! { <ActivityTabView app_state=app_state_for_tabs.clone()/> }.into_view(),
                            TabId::Memory => view! { <MemoryTabView app_state=app_state_for_tabs.clone()/> }.into_view(),
                            TabId::Calendar => view! { <CalendarTabView app_state=app_state_for_tabs.clone()/> }.into_view(),
                            TabId::Workspace => view! { <WorkspaceTabView app_state=app_state_for_tabs.clone()/> }.into_view(),
                            TabId::Integrations => view! { <IntegrationsTabView app_state=app_state_for_tabs.clone()/> }.into_view(),
                            TabId::Settings => view! { <SettingsTabView app_state=app_state_for_tabs.clone()/> }.into_view(),
                        }
                    }}
                </main>

                <div
                    class="resizer-h"
                    class:active=move || dragging.get() == Some("right")
                    on:mousedown=move |_| dragging.set(Some("right"))
                />

                <aside class="branches-panel">
                    <div class="panel-title">"Company Branches"</div>
                    <div class="hierarchy">
                        <details class="tree-group" open=true>
                            <summary class="tree-summary">
                                <span>"Orchestrator"</span>
                                <span class="count-pill">{move || app_state.agents.get().len()} " workers"</span>
                            </summary>

                            {move || {
                                let branches = grouped_branches(&app_state.agents.get())
                                    .into_iter()
                                    .collect::<Vec<(String, BTreeMap<String, Vec<SubAgent>>)>>();

                                if branches.is_empty() {
                                    view! { <div class="muted">"No active branch workers."</div> }.into_view()
                                } else {
                                    branches
                                        .into_iter()
                                        .map(|(branch_id, missions)| {
                                            let workers_total = missions
                                                .values()
                                                .map(|workers| workers.len())
                                                .sum::<usize>();
                                            let active_total = missions
                                                .values()
                                                .map(|workers| {
                                                    workers
                                                        .iter()
                                                        .filter(|worker| matches!(worker.status, AgentStatus::Active))
                                                        .count()
                                                })
                                                .sum::<usize>();

                                            let mission_rows = missions
                                                .into_iter()
                                                .collect::<Vec<(String, Vec<SubAgent>)>>();

                                            view! {
                                                <details class="tree-group" open=true>
                                                    <summary class="tree-summary">
                                                        <span>{format!("Branch: {}", branch_id)}</span>
                                                        <span class="count-pill">{format!("{}/{} active", active_total, workers_total)}</span>
                                                    </summary>

                                                    <div class="mission-nodes">
                                                        {mission_rows
                                                            .into_iter()
                                                            .map(|(mission_id, workers)| {
                                                                let active_count = workers
                                                                    .iter()
                                                                    .filter(|worker| matches!(worker.status, AgentStatus::Active))
                                                                    .count();
                                                                let total_count = workers.len();

                                                                view! {
                                                                    <details class="tree-group mission-node" open=true>
                                                                        <summary class="tree-summary">
                                                                            <span>{format!("Mission: {}", mission_id)}</span>
                                                                            <span class="count-pill">{format!("{}/{}", active_count, total_count)}</span>
                                                                        </summary>

                                                                        <div class="worker-nodes">
                                                                            {workers
                                                                                .into_iter()
                                                                                .map(|worker| {
                                                                                    let status = status_class(&worker.status).to_string();
                                                                                    view! {
                                                                                        <div class="worker-node">
                                                                                            <span class=format!("status-dot {}", status)></span>
                                                                                            <div class="worker-meta">
                                                                                                <div>{worker.name}</div>
                                                                                                <div class="agent-task">{worker.objective}</div>
                                                                                            </div>
                                                                                        </div>
                                                                                    }
                                                                                })
                                                                                .collect_view()}
                                                                        </div>
                                                                    </details>
                                                                }
                                                            })
                                                            .collect_view()}
                                                    </div>
                                                </details>
                                            }
                                        })
                                        .collect_view()
                                        .into_view()
                                }
                            }}
                        </details>
                    </div>
                </aside>
            </div>
        </div>
    }
}

#[component]
pub fn DetachedChatView() -> impl IntoView {
    let params = use_params_map();
    let agent_id = move || params.with(|p| p.get("id").cloned().unwrap_or_default());

    let (messages, set_messages) = create_signal(Vec::<InferenceChatMessage>::new());
    let (input, set_input) = create_signal(String::new());
    let (is_sending, set_is_sending) = create_signal(false);
    let (is_streaming_reply, set_is_streaming_reply) = create_signal(false);
    let (chat_error, set_chat_error) = create_signal(String::new());
    let chat_log_ref = create_node_ref::<html::Div>();

    let refresh_history: Rc<dyn Fn()> = Rc::new(move || {
        let id = agent_id();
        if id.is_empty() {
            return;
        }
        if is_sending.get_untracked() || is_streaming_reply.get_untracked() {
            return;
        }
        wasm_bindgen_futures::spawn_local(async move {
            if let Ok(res) = core_request::<ChatHistoryResponse>(CoreRequestInput {
                method: "GET".to_string(),
                path: format!("/api/chat/history?agent_id={}", id),
                body: None,
                admin_token: None,
            })
            .await
            {
                set_messages.set(res.messages);
            }
        });
    });

    {
        create_effect(move |_| {
            let _ = messages.get().len();
            if let Some(log) = chat_log_ref.get() {
                log.set_scroll_top(log.scroll_height());
            }
        });
    }

    {
        let refresh_history = Rc::clone(&refresh_history);
        create_effect(move |_| {
            (refresh_history)();
            let refresh_history = Rc::clone(&refresh_history);
            if let Ok(handle) =
                set_interval_with_handle(move || (refresh_history)(), Duration::from_secs(3))
            {
                on_cleanup(move || {
                    handle.clear();
                });
            }
        });
    }

    let send_message: Rc<dyn Fn()> = Rc::new({
        let refresh_history = Rc::clone(&refresh_history);
        move || {
            let id = agent_id();
            let text = input.get().trim().to_string();
            if text.is_empty() || id.is_empty() || is_sending.get() {
                return;
            }

            set_input.set(String::new());
            set_is_sending.set(true);
            set_is_streaming_reply.set(true);
            set_chat_error.set(String::new());

            set_messages.update(|rows| {
                rows.push(InferenceChatMessage {
                    role: "user".to_string(),
                    content: text.clone(),
                });
                rows.push(InferenceChatMessage {
                    role: "assistant".to_string(),
                    content: String::new(),
                });
            });

            let refresh_history = Rc::clone(&refresh_history);
            wasm_bindgen_futures::spawn_local(async move {
                let mut streamed_text = String::new();
                let stream_result = stream_chat_reply(text.clone(), Some(id.clone()), |token| {
                    streamed_text.push_str(&token);
                    let partial = streamed_text.clone();
                    set_messages.update(|rows| {
                        if let Some(last) = rows.last_mut() {
                            last.content = partial.clone();
                        }
                    });
                })
                .await;

                match stream_result {
                    Ok(full_response) => {
                        let final_text = if full_response.trim().is_empty() {
                            streamed_text
                        } else {
                            full_response
                        };
                        set_messages.update(|rows| {
                            if let Some(last) = rows.last_mut() {
                                last.content = final_text.clone();
                            }
                        });
                        set_chat_error.set(String::new());
                    }
                    Err(stream_err) => {
                        if streamed_text.is_empty() {
                            match core_request::<ChatResponse>(CoreRequestInput {
                                method: "POST".to_string(),
                                path: "/api/chat".to_string(),
                                body: Some(json!({
                                    "message": text,
                                    "agent_id": Some(id),
                                })),
                                admin_token: None,
                            })
                            .await
                            {
                                Ok(response) => {
                                    set_messages.update(|rows| {
                                        if let Some(last) = rows.last_mut() {
                                            last.content = response.reply.clone();
                                        }
                                    });
                                    set_chat_error.set(String::new());
                                }
                                Err(err) => {
                                    set_chat_error.set(format!(
                                        "Stream failed ({}) and fallback failed: {}",
                                        stream_err, err
                                    ));
                                }
                            }
                        } else {
                            set_chat_error.set(format!("Stream interrupted: {}", stream_err));
                        }
                    }
                }

                set_is_streaming_reply.set(false);
                (refresh_history)();
                set_is_sending.set(false);
            });
        }
    });

    let send_on_enter = Rc::clone(&send_message);
    let send_on_click = Rc::clone(&send_message);

    view! {
        <div class="app-shell" style="grid-template-columns: 1fr;">
            <main class="main-shell">
                <header class="top-bar">
                    <span class="status-chip">"Detached Agent Chat: " {agent_id}</span>
                </header>

                <div class="chat-panel">
                    <div
                        class="chat-log"
                        node_ref=chat_log_ref
                        on:click=move |ev| handle_markdown_copy_click(ev)
                    >
                        <For
                            each=move || {
                                messages
                                    .get()
                                    .into_iter()
                                    .enumerate()
                                    .collect::<Vec<_>>()
                            }
                            key=|item| item.0
                            children=move |item| {
                                let msg = item.1;
                                let role_class = if msg.role == "user" {
                                    "message user"
                                } else {
                                    "message assistant"
                                };
                                let sender = sender_label(&msg.role);
                                let content_view = if msg.role == "assistant" {
                                    let rendered = render_markdown(&msg.content);
                                    view! {
                                        <div class="message-body markdown-body" inner_html=rendered></div>
                                    }
                                        .into_view()
                                } else {
                                    view! { <div class="message-body plain-message">{msg.content.clone()}</div> }
                                        .into_view()
                                };
                                view! {
                                    <div class=role_class>
                                        <div class="msg-sender">{sender}</div>
                                        {content_view}
                                    </div>
                                }
                            }
                        />
                    </div>

                    <div class="composer-container">
                        <div class="composer-row">
                            <textarea
                                class="composer"
                                rows="3"
                                placeholder="Message agent..."
                                prop:value=move || input.get()
                                prop:disabled=move || is_sending.get()
                                on:input=move |ev| set_input.set(event_target_value(&ev))
                                on:keydown=move |ev| {
                                    if ev.key() == "Enter" && !ev.shift_key() {
                                        ev.prevent_default();
                                        (send_on_enter)();
                                    }
                                }
                            ></textarea>

                            <button
                                class="send-btn"
                                prop:disabled=move || is_sending.get()
                                on:click=move |_| (send_on_click)()
                            >
                                {move || if is_sending.get() { "Sending..." } else { "Send" }}
                            </button>
                        </div>

                        {move || {
                            if chat_error.get().is_empty() {
                                ().into_view()
                            } else {
                                view! { <div class="notice error">{chat_error.get()}</div> }.into_view()
                            }
                        }}
                    </div>
                </div>
            </main>
        </div>
    }
}

#[component]
pub fn MissionControlApp() -> impl IntoView {
    let app_state = AppState::new();
    app_state.start_polling();
    provide_context(app_state.clone());

    view! {
        <Router>
            <main>
                <Routes>
                    <Route path="/" view=MainMissionView/>
                    <Route path="/chat/:id" view=DetachedChatView/>
                </Routes>
            </main>
        </Router>
    }
}
