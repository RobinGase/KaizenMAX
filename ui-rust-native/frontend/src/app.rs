use leptos::ev;
use leptos::html;
use leptos::*;
use leptos_router::*;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashSet};
use std::rc::Rc;
use std::time::Duration;
use wasm_bindgen::prelude::*;

use crate::models::types::*;

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

fn status_class(status: &AgentStatus) -> &'static str {
    match status {
        AgentStatus::Idle => "idle",
        AgentStatus::Active => "active",
        AgentStatus::Blocked => "blocked",
        AgentStatus::Done => "done",
    }
}

fn tab_label(tab: &TabId) -> &'static str {
    match tab {
        TabId::Mission => "Mission",
        TabId::Branches => "Branches",
        TabId::Gates => "Gates",
        TabId::Activity => "Activity",
        TabId::Workspace => "Workspace",
        TabId::Integrations => "Integrations",
        TabId::Settings => "Settings",
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

#[derive(Clone)]
pub struct AppState {
    pub active_tab: RwSignal<TabId>,
    pub health: RwSignal<Option<HealthResponse>>,
    pub agents: RwSignal<Vec<SubAgent>>,
    pub events: RwSignal<Vec<CrystalBallEvent>>,
}

impl AppState {
    fn new() -> Self {
        Self {
            active_tab: create_rw_signal(TabId::Mission),
            health: create_rw_signal(None),
            agents: create_rw_signal(vec![]),
            events: create_rw_signal(vec![]),
        }
    }

    fn start_polling(&self) {
        let state = self.clone();

        let init_state = state.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let _ = init_state.refresh_health().await;
            let _ = init_state.refresh_agents().await;
            let _ = init_state.refresh_events().await;
        });

        let handle = set_interval_with_handle(
            move || {
                let state_clone = state.clone();
                wasm_bindgen_futures::spawn_local(async move {
                    let _ = state_clone.refresh_health().await;
                    let _ = state_clone.refresh_agents().await;
                    let _ = state_clone.refresh_events().await;
                });
            },
            Duration::from_secs(5),
        )
        .expect("Failed to create interval");

        on_cleanup(move || {
            handle.clear();
        });
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
            path: "/api/events".to_string(),
            body: None,
            admin_token: None,
        })
        .await?;
        self.events.set(payload);
        Ok(())
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

    let (messages, set_messages) = create_signal(Vec::<InferenceChatMessage>::new());
    let (input, set_input) = create_signal(String::new());
    let (is_sending, set_is_sending) = create_signal(false);

    let chat_log_ref = create_node_ref::<html::Div>();
    let telemetry_log_ref = create_node_ref::<html::Div>();

    let refresh_main_history: Rc<dyn Fn()> = Rc::new(move || {
        wasm_bindgen_futures::spawn_local(async move {
            if let Ok(res) = core_request::<ChatHistoryResponse>(CoreRequestInput {
                method: "GET".to_string(),
                path: "/api/chat/history?limit=80".to_string(),
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
            let handle =
                set_interval_with_handle(move || (refresh_main_history)(), Duration::from_secs(3))
                    .expect("Failed to set mission refresh interval");

            on_cleanup(move || {
                handle.clear();
            });
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

    {
        create_effect(move |_| {
            let _ = app_state.events.get().len();
            if let Some(log) = telemetry_log_ref.get() {
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

            let app_state = app_state.clone();
            let refresh_main_history = Rc::clone(&refresh_main_history);
            wasm_bindgen_futures::spawn_local(async move {
                let _ = core_request::<ChatResponse>(CoreRequestInput {
                    method: "POST".to_string(),
                    path: "/api/chat".to_string(),
                    body: Some(json!({
                        "message": text,
                    })),
                    admin_token: None,
                })
                .await;

                (refresh_main_history)();
                let _ = app_state.refresh_events().await;
                set_is_sending.set(false);
            });
        }
    });

    window_event_listener(ev::mousemove, move |ev| {
        if let Some(side) = dragging.get_untracked() {
            let x = ev.client_x();
            if side == "left" {
                left_width.set(x.max(170).min(520));
            } else if let Some(win) = web_sys::window() {
                let window_width = win.inner_width().unwrap().as_f64().unwrap() as i32;
                right_width.set((window_width - x).max(240).min(620));
            }
        }
    });

    window_event_listener(ev::mouseup, move |_| {
        dragging.set(None);
    });

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
                        <div class="brand-title">"Kaizen MAX"</div>
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
                                        TabId::Gates,
                                        TabId::Activity,
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
                            <Show
                                when=move || !app_state.agents.get().is_empty()
                                fallback=move || {
                                    view! { <div class="agent-empty">"No active sub-agents."</div> }
                                }
                            >
                                <For
                                    each=move || app_state.agents.get()
                                    key=|agent| agent.id.clone()
                                    children=move |agent| {
                                        let id_for_click = agent.id.clone();
                                        let id_for_class = agent.id.clone();
                                        let id_for_label = agent.id.clone();
                                        let status = status_class(&agent.status).to_string();
                                        let task = agent
                                            .task_id
                                            .clone()
                                            .unwrap_or_else(|| "general".to_string());

                                        view! {
                                            <div class="agent-item">
                                                <div class="agent-meta">
                                                    <span class=format!("status-dot {}", status)></span>
                                                    <div>
                                                        <div class="agent-name">{agent.name}</div>
                                                        <div class="agent-task">{task}</div>
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
                            </Show>
                        </div>
                    </div>

                    <div class="nav-note">"Rust-native Tauri + Leptos"</div>
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
                            <span class="count-pill">"Agents " {move || app_state.agents.get().len()}</span>
                            <span class="count-pill">"Events " {move || app_state.events.get().len()}</span>
                        </div>
                    </header>

                    {move || {
                        if app_state.active_tab.get() == TabId::Mission {
                            let send_on_enter = Rc::clone(&send_main_message);
                            let send_on_click = Rc::clone(&send_main_message);
                            view! {
                                <div class="main-mission">
                                    <div class="chat-panel">
                                        <div class="chat-log" node_ref=chat_log_ref>
                                            <Show
                                                when=move || !messages.get().is_empty()
                                                fallback=move || {
                                                    view! {
                                                        <div class="chat-empty">
                                                            <div class="empty-title">"Mission console is live."</div>
                                                            <div class="empty-copy">
                                                                "Send an instruction to Kaizen to begin execution."
                                                            </div>
                                                        </div>
                                                    }
                                                }
                                            >
                                                <For
                                                    each=move || {
                                                        messages
                                                            .get()
                                                            .into_iter()
                                                            .enumerate()
                                                            .collect::<Vec<(usize, InferenceChatMessage)>>()
                                                    }
                                                    key=|item| {
                                                        format!(
                                                            "{}-{}-{}",
                                                            item.0,
                                                            item.1.role,
                                                            item.1.content.len()
                                                        )
                                                    }
                                                    children=move |item| {
                                                        let msg = item.1;
                                                        let role_class = if msg.role == "user" {
                                                            "message user"
                                                        } else {
                                                            "message assistant"
                                                        };
                                                        let sender = sender_label(&msg.role);
                                                        view! {
                                                            <div class=role_class>
                                                                <div class="msg-sender">{sender}</div>
                                                                {msg.content}
                                                            </div>
                                                        }
                                                    }
                                                />
                                            </Show>
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
                                        </div>

                                        <section class="telemetry-panel">
                                            <div class="telemetry-head">"Live Telemetry"</div>
                                            <div class="telemetry-feed" node_ref=telemetry_log_ref>
                                                <Show
                                                    when=move || !app_state.events.get().is_empty()
                                                    fallback=move || {
                                                        view! {
                                                            <div class="telemetry-empty">
                                                                "Waiting for crystal-ball events..."
                                                            </div>
                                                        }
                                                    }
                                                >
                                                    <For
                                                        each=move || {
                                                            let mut rows = app_state.events.get();
                                                            if rows.len() > 120 {
                                                                rows.split_off(rows.len() - 120)
                                                            } else {
                                                                rows
                                                            }
                                                        }
                                                        key=|event| event.event_id.clone()
                                                        children=move |event| {
                                                            view! {
                                                                <div class="event-row">
                                                                    <span class="event-time">{compact_time(&event.timestamp)}</span>
                                                                    <span class="event-type">{event.event_type}</span>
                                                                    <span class="event-msg">{event.message}</span>
                                                                </div>
                                                            }
                                                        }
                                                    />
                                                </Show>
                                            </div>
                                        </section>
                                    </div>
                                </div>
                            }
                            .into_view()
                        } else {
                            let tab_name = tab_label(&app_state.active_tab.get()).to_string();
                            view! {
                                <section class="tab-placeholder">
                                    <h2>{tab_name}</h2>
                                    <p>
                                        "This workspace is staged and ready. Activate mission mode for live command and telemetry."
                                    </p>
                                </section>
                            }
                            .into_view()
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

                            <details class="tree-group" open=true>
                                <summary class="tree-summary">
                                    <span>"Branch: Primary"</span>
                                    <span class="count-pill">
                                        {move || {
                                            app_state
                                                .agents
                                                .get()
                                                .iter()
                                                .filter(|agent| matches!(agent.status, AgentStatus::Active))
                                                .count()
                                        }}
                                        " active"
                                    </span>
                                </summary>

                                <div class="mission-nodes">
                                    {move || {
                                        let mut missions: BTreeMap<String, Vec<SubAgent>> = BTreeMap::new();
                                        for agent in app_state.agents.get() {
                                            let task = agent
                                                .task_id
                                                .clone()
                                                .unwrap_or_else(|| "general".to_string());
                                            missions.entry(task).or_default().push(agent);
                                        }

                                        missions
                                            .into_iter()
                                            .map(|(task_id, workers)| {
                                                let active_count = workers
                                                    .iter()
                                                    .filter(|w| matches!(w.status, AgentStatus::Active))
                                                    .count();
                                                let total_count = workers.len();

                                                view! {
                                                    <details class="tree-group mission-node" open=true>
                                                        <summary class="tree-summary">
                                                            <span>{format!("Mission: {}", task_id)}</span>
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
                                            .collect_view()
                                    }}
                                </div>
                            </details>
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
    let chat_log_ref = create_node_ref::<html::Div>();

    let refresh_history: Rc<dyn Fn()> = Rc::new(move || {
        let id = agent_id();
        if id.is_empty() {
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
            let handle =
                set_interval_with_handle(move || (refresh_history)(), Duration::from_secs(3))
                    .expect("Failed to set detached chat interval");

            on_cleanup(move || {
                handle.clear();
            });
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

            let refresh_history = Rc::clone(&refresh_history);
            wasm_bindgen_futures::spawn_local(async move {
                let _ = core_request::<Value>(CoreRequestInput {
                    method: "POST".to_string(),
                    path: "/api/chat".to_string(),
                    body: Some(json!({
                        "message": text,
                        "agent_id": Some(id),
                    })),
                    admin_token: None,
                })
                .await;
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
                    <div class="chat-log" node_ref=chat_log_ref>
                        <For
                            each=move || messages.get()
                            key=|msg| format!("{}-{}", msg.role, msg.content.len())
                            children=move |msg| {
                                let role_class = if msg.role == "user" {
                                    "message user"
                                } else {
                                    "message assistant"
                                };
                                let sender = sender_label(&msg.role);
                                view! {
                                    <div class=role_class>
                                        <div class="msg-sender">{sender}</div>
                                        {msg.content}
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
