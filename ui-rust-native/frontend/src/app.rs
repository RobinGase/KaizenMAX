use leptos::ev;
use leptos::html;
use leptos::*;
use leptos_router::*;
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, HashSet};
use std::rc::Rc;
use std::time::Duration;
use wasm_bindgen::prelude::*;

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

fn normalize_provider(value: &str) -> String {
    value.trim().to_ascii_lowercase()
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

fn grouped_missions(agents: &[SubAgent]) -> BTreeMap<String, Vec<SubAgent>> {
    let mut map = BTreeMap::<String, Vec<SubAgent>>::new();
    for agent in agents {
        map.entry(agent.task_id.clone())
            .or_default()
            .push(agent.clone());
    }
    map
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
    pub admin_token: RwSignal<String>,
}

impl AppState {
    fn new() -> Self {
        Self {
            active_tab: create_rw_signal(TabId::Mission),
            health: create_rw_signal(None),
            agents: create_rw_signal(vec![]),
            events: create_rw_signal(vec![]),
            admin_token: create_rw_signal(load_admin_token()),
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
}

#[component]
fn MissionTabView(app_state: AppState) -> impl IntoView {
    let (messages, set_messages) = create_signal(Vec::<InferenceChatMessage>::new());
    let (input, set_input) = create_signal(String::new());
    let (is_sending, set_is_sending) = create_signal(false);
    let (chat_notice, set_chat_notice) = create_signal(String::new());

    let chat_log_ref = create_node_ref::<html::Div>();
    let telemetry_log_ref = create_node_ref::<html::Div>();

    let refresh_main_history: Rc<dyn Fn()> = Rc::new(move || {
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
                match core_request::<ChatResponse>(CoreRequestInput {
                    method: "POST".to_string(),
                    path: "/api/chat".to_string(),
                    body: Some(json!({ "message": text })),
                    admin_token: None,
                })
                .await
                {
                    Ok(_) => set_chat_notice.set(String::new()),
                    Err(err) => set_chat_notice.set(format!("Chat request failed: {}", err)),
                }

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
                <div class="chat-log" node_ref=chat_log_ref>
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
                                    key=|item| format!("{}-{}-{}", item.0, item.1.role, item.1.content.len())
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

                <section class="telemetry-panel">
                    <div class="telemetry-head">"Live Telemetry"</div>
                    <div class="telemetry-feed" node_ref=telemetry_log_ref>
                        {move || {
                            if app_state.events.get().is_empty() {
                                view! { <div class="telemetry-empty">"Waiting for crystal-ball events..."</div> }
                                    .into_view()
                            } else {
                                view! {
                                    <For
                                        each=move || {
                                            let mut rows = app_state.events.get();
                                            if rows.len() > 160 {
                                                rows.split_off(rows.len() - 160)
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
                                }
                                    .into_view()
                            }
                        }}
                    </div>
                </section>
            </div>
        </div>
    }
}

#[component]
fn BranchesTabView(app_state: AppState) -> impl IntoView {
    let missions = create_memo(move |_| grouped_missions(&app_state.agents.get()));
    let mission_rows = create_memo(move |_| {
        missions
            .get()
            .into_iter()
            .collect::<Vec<(String, Vec<SubAgent>)>>()
    });

    view! {
        <section class="tab-view">
            <div class="tab-head">
                <h2>"Branches"</h2>
                <p>"Live branch and mission structure generated from active sub-agents."</p>
            </div>

            <div class="card-grid two-col">
                <article class="card">
                    <h3>"Branch Summary"</h3>
                    <div class="stats-grid">
                        <div class="stat-tile">
                            <span class="stat-label">"Workers"</span>
                            <span class="stat-value">{move || app_state.agents.get().len()}</span>
                        </div>
                        <div class="stat-tile">
                            <span class="stat-label">"Active"</span>
                            <span class="stat-value">
                                {move || {
                                    app_state
                                        .agents
                                        .get()
                                        .iter()
                                        .filter(|a| matches!(a.status, AgentStatus::Active))
                                        .count()
                                }}
                            </span>
                        </div>
                        <div class="stat-tile">
                            <span class="stat-label">"Blocked"</span>
                            <span class="stat-value">
                                {move || {
                                    app_state
                                        .agents
                                        .get()
                                        .iter()
                                        .filter(|a| matches!(a.status, AgentStatus::Blocked))
                                        .count()
                                }}
                            </span>
                        </div>
                        <div class="stat-tile">
                            <span class="stat-label">"Missions"</span>
                            <span class="stat-value">{move || missions.get().len()}</span>
                        </div>
                    </div>
                </article>

                <article class="card">
                    <h3>"Mission Tree"</h3>
                    <div class="list-stack">
                        {move || {
                            if missions.get().is_empty() {
                                view! { <div class="muted">"No missions are active."</div> }.into_view()
                            } else {
                                view! {
                                    <For
                                        each=move || mission_rows.get()
                                        key=|item| item.0.clone()
                                        children=move |item| {
                                            let mission_name = item.0;
                                            let workers = item.1;
                                            let active_count = workers
                                                .iter()
                                                .filter(|w| matches!(w.status, AgentStatus::Active))
                                                .count();
                                            view! {
                                                <details class="mission-card" open=true>
                                                    <summary>
                                                        <span>{mission_name.clone()}</span>
                                                        <span class="tiny-pill">{format!("{}/{} active", active_count, workers.len())}</span>
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
                                        }
                                    />
                                }
                                    .into_view()
                            }
                        }}
                    </div>
                </article>
            </div>
        </section>
    }
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

    let event_types = create_memo(move |_| {
        let mut values = app_state
            .events
            .get()
            .into_iter()
            .map(|event| event.event_type)
            .collect::<Vec<_>>();
        values.sort();
        values.dedup();
        values
    });

    let filtered_events = create_memo(move |_| {
        let needle = query.get().to_lowercase();
        let selected_kind = kind.get();
        app_state
            .events
            .get()
            .into_iter()
            .filter(|event| {
                let type_match = selected_kind.is_empty() || event.event_type == selected_kind;
                let text_match = needle.is_empty()
                    || event.message.to_lowercase().contains(&needle)
                    || event.source_actor.to_lowercase().contains(&needle)
                    || event.target_actor.to_lowercase().contains(&needle)
                    || event.task_id.to_lowercase().contains(&needle);
                type_match && text_match
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

                    <span class="tiny-pill">"Rows " {move || filtered_events.get().len()}</span>
                </div>
            </div>

            <div class="card activity-table">
                <div class="table-header row">
                    <span>"Time"</span>
                    <span>"Type"</span>
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
                                key=|event| event.event_id.clone()
                                children=move |event| {
                                    view! {
                                        <div class="table-row row">
                                            <span>{compact_time(&event.timestamp)}</span>
                                            <span>{event.event_type}</span>
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
    let (spawn_task_id, set_spawn_task_id) = create_signal(String::new());
    let (spawn_objective, set_spawn_objective) = create_signal(String::new());
    let (workspace_busy, set_workspace_busy) = create_signal(false);
    let (workspace_notice, set_workspace_notice) = create_signal(String::new());

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
            let task_id = spawn_task_id.get().trim().to_string();
            let objective = spawn_objective.get().trim().to_string();

            if name.is_empty() || task_id.is_empty() || objective.is_empty() {
                set_workspace_notice
                    .set("Provide agent name, task id, and objective before spawning.".to_string());
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
                        "task_id": task_id,
                        "objective": objective,
                        "user_requested": true,
                    })),
                    admin_token: None,
                })
                .await
                {
                    Ok(agent) => {
                        set_workspace_notice
                            .set(format!("Spawned '{}' ({})", agent.name, agent.task_id));
                        set_spawn_name.set(String::new());
                        set_spawn_task_id.set(String::new());
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
                        placeholder="Task id"
                        prop:value=move || spawn_task_id.get()
                        on:input=move |ev| set_spawn_task_id.set(event_target_value(&ev))
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
                {move || {
                    let run_request = Rc::clone(&run_request_for_view);
                    if app_state.agents.get().is_empty() {
                        view! { <div class="muted">"No active sub-agents."</div> }.into_view()
                    } else {
                        view! {
                            <For
                                each=move || app_state.agents.get()
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
                                                    <div class="agent-name">{agent.name}</div>
                                                    <div class="agent-task">{agent.task_id}</div>
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
fn IntegrationsTabView(app_state: AppState) -> impl IntoView {
    let (gh_status, set_gh_status) = create_signal(None::<GitHubStatusResponse>);
    let (gh_repos, set_gh_repos) = create_signal(Vec::<GitHubRepoSummary>::new());
    let (vault_status, set_vault_status) = create_signal(None::<VaultStatusResponse>);
    let (secrets, set_secrets) = create_signal(Vec::<SecretMetadata>::new());
    let (oauth_statuses, set_oauth_statuses) = create_signal(Vec::<OAuthStatusResponse>::new());
    let (integration_error, set_integration_error) = create_signal(String::new());
    let (integration_notice, set_integration_notice) = create_signal(String::new());
    let (integration_busy, set_integration_busy) = create_signal(false);

    let (secret_provider, set_secret_provider) = create_signal("openai".to_string());
    let (secret_type, set_secret_type) = create_signal("api_key".to_string());
    let (secret_value, set_secret_value) = create_signal(String::new());
    let (oauth_provider, set_oauth_provider) = create_signal("openai".to_string());

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

                match core_request::<VaultStatusResponse>(CoreRequestInput {
                    method: "GET".to_string(),
                    path: "/api/vault/status".to_string(),
                    body: None,
                    admin_token: token.clone(),
                })
                .await
                {
                    Ok(status) => set_vault_status.set(Some(status)),
                    Err(err) => issues.push(format!("Vault status: {}", err)),
                }

                match core_request::<Vec<SecretMetadata>>(CoreRequestInput {
                    method: "GET".to_string(),
                    path: "/api/secrets".to_string(),
                    body: None,
                    admin_token: token.clone(),
                })
                .await
                {
                    Ok(rows) => set_secrets.set(rows),
                    Err(err) => issues.push(format!("Secrets: {}", err)),
                }

                let mut oauth_rows = Vec::<OAuthStatusResponse>::new();
                for provider in ["openai", "anthropic", "gemini", "nvidia"] {
                    match core_request::<OAuthStatusResponse>(CoreRequestInput {
                        method: "GET".to_string(),
                        path: format!("/api/oauth/{}/status", provider),
                        body: None,
                        admin_token: token.clone(),
                    })
                    .await
                    {
                        Ok(status) => oauth_rows.push(status),
                        Err(err) => issues.push(format!("OAuth {}: {}", provider, err)),
                    }
                }
                set_oauth_statuses.set(oauth_rows);

                set_integration_error.set(issues.join(" | "));
                set_integration_busy.set(false);
            });
        }
    });

    let save_secret: Rc<dyn Fn()> = Rc::new({
        let app_state = app_state.clone();
        let refresh_integrations = Rc::clone(&refresh_integrations);
        move || {
            if integration_busy.get() {
                return;
            }

            let provider = normalize_provider(&secret_provider.get());
            let secret_kind = secret_type.get().trim().to_string();
            let value = secret_value.get().trim().to_string();

            if provider.is_empty() || value.is_empty() {
                set_integration_error
                    .set("Provider and secret value are required before saving.".to_string());
                return;
            }

            set_integration_busy.set(true);
            let app_state = app_state.clone();
            let refresh_integrations = Rc::clone(&refresh_integrations);
            wasm_bindgen_futures::spawn_local(async move {
                match core_request::<SecretMetadata>(CoreRequestInput {
                    method: "PUT".to_string(),
                    path: format!("/api/secrets/{}", provider),
                    body: Some(json!({
                        "value": value,
                        "secret_type": secret_kind,
                    })),
                    admin_token: app_state.admin_token_opt(),
                })
                .await
                {
                    Ok(meta) => {
                        set_secret_value.set(String::new());
                        set_integration_error.set(String::new());
                        set_integration_notice
                            .set(format!("Stored {} credential for {}.", meta.secret_type, meta.provider));
                    }
                    Err(err) => {
                        set_integration_notice.set(String::new());
                        set_integration_error.set(format!("Store secret failed: {}", err));
                    }
                }
                set_integration_busy.set(false);
                (refresh_integrations)();
            });
        }
    });

    let test_secret: Rc<dyn Fn()> = Rc::new({
        let app_state = app_state.clone();
        let refresh_integrations = Rc::clone(&refresh_integrations);
        move || {
            if integration_busy.get() {
                return;
            }

            let provider = normalize_provider(&secret_provider.get());
            if provider.is_empty() {
                set_integration_error.set("Provider is required before testing.".to_string());
                return;
            }

            set_integration_busy.set(true);
            let app_state = app_state.clone();
            let refresh_integrations = Rc::clone(&refresh_integrations);
            wasm_bindgen_futures::spawn_local(async move {
                match core_request::<SecretTestResponse>(CoreRequestInput {
                    method: "POST".to_string(),
                    path: format!("/api/secrets/{}/test", provider),
                    body: None,
                    admin_token: app_state.admin_token_opt(),
                })
                .await
                {
                    Ok(result) => {
                        if result.test_passed {
                            set_integration_error.set(String::new());
                            set_integration_notice
                                .set(format!("Credential test passed for {}.", result.provider));
                        } else {
                            set_integration_notice.set(String::new());
                            set_integration_error.set(format!(
                                "Credential test failed for {}: {}",
                                result.provider,
                                result.error.unwrap_or_else(|| "unknown error".to_string())
                            ));
                        }
                    }
                    Err(err) => {
                        set_integration_notice.set(String::new());
                        set_integration_error.set(format!("Test secret failed: {}", err));
                    }
                }
                set_integration_busy.set(false);
                (refresh_integrations)();
            });
        }
    });

    let revoke_secret: Rc<dyn Fn()> = Rc::new({
        let app_state = app_state.clone();
        let refresh_integrations = Rc::clone(&refresh_integrations);
        move || {
            if integration_busy.get() {
                return;
            }

            let provider = normalize_provider(&secret_provider.get());
            if provider.is_empty() {
                set_integration_error.set("Provider is required before revoking.".to_string());
                return;
            }

            set_integration_busy.set(true);
            let app_state = app_state.clone();
            let refresh_integrations = Rc::clone(&refresh_integrations);
            wasm_bindgen_futures::spawn_local(async move {
                match core_request::<Value>(CoreRequestInput {
                    method: "DELETE".to_string(),
                    path: format!("/api/secrets/{}", provider),
                    body: None,
                    admin_token: app_state.admin_token_opt(),
                })
                .await
                {
                    Ok(_) => {
                        set_integration_error.set(String::new());
                        set_integration_notice.set("Credential revoked.".to_string());
                    }
                    Err(err) => {
                        set_integration_notice.set(String::new());
                        set_integration_error.set(format!("Revoke secret failed: {}", err));
                    }
                }
                set_integration_busy.set(false);
                (refresh_integrations)();
            });
        }
    });

    let disconnect_oauth: Rc<dyn Fn()> = Rc::new({
        let app_state = app_state.clone();
        let refresh_integrations = Rc::clone(&refresh_integrations);
        move || {
            if integration_busy.get() {
                return;
            }

            let provider = normalize_provider(&oauth_provider.get());
            if provider.is_empty() {
                set_integration_error.set("OAuth provider is required.".to_string());
                return;
            }

            set_integration_busy.set(true);
            let app_state = app_state.clone();
            let refresh_integrations = Rc::clone(&refresh_integrations);
            wasm_bindgen_futures::spawn_local(async move {
                match core_request::<Value>(CoreRequestInput {
                    method: "DELETE".to_string(),
                    path: format!("/api/oauth/{}", provider),
                    body: None,
                    admin_token: app_state.admin_token_opt(),
                })
                .await
                {
                    Ok(_) => {
                        set_integration_error.set(String::new());
                        set_integration_notice.set("OAuth tokens disconnected.".to_string());
                    }
                    Err(err) => {
                        set_integration_notice.set(String::new());
                        set_integration_error.set(format!("OAuth disconnect failed: {}", err));
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
            (refresh_integrations)();
            let refresh_integrations = Rc::clone(&refresh_integrations);
            if let Ok(handle) = set_interval_with_handle(
                move || (refresh_integrations)(),
                Duration::from_secs(20),
            ) {
                on_cleanup(move || handle.clear());
            }
        });
    }

    let refresh_integrations_top = Rc::clone(&refresh_integrations);
    let refresh_integrations_oauth = Rc::clone(&refresh_integrations);

    view! {
        <section class="tab-view">
            <div class="tab-head">
                <h2>"Integrations"</h2>
                <p>"GitHub, Vault, and OAuth integration status."</p>
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

            <div class="card-grid three-col">
                <article class="card">
                    <h3>"GitHub"</h3>
                    {move || {
                        if let Some(status) = gh_status.get() {
                            view! {
                                <div class="list-stack">
                                    <div><strong>"Authenticated: "</strong>{if status.authenticated { "Yes" } else { "No" }}</div>
                                    <div><strong>"Host: "</strong>{status.host}</div>
                                    <div><strong>"Login: "</strong>{status.login.unwrap_or_else(|| "-".to_string())}</div>
                                    <div><strong>"Token Source: "</strong>{status.token_source.unwrap_or_else(|| "-".to_string())}</div>
                                </div>
                            }
                                .into_view()
                        } else {
                            view! { <div class="muted">"No GitHub status yet."</div> }.into_view()
                        }
                    }}
                </article>

                <article class="card">
                    <h3>"Vault"</h3>
                    {move || {
                        if let Some(status) = vault_status.get() {
                            view! {
                                <div class="list-stack">
                                    <div><strong>"Available: "</strong>{if status.available { "Yes" } else { "No" }}</div>
                                    <div><strong>"Key Source: "</strong>{status.key_source}</div>
                                    <div><strong>"Vault Path: "</strong>{status.vault_path}</div>
                                    <div><strong>"Bootstrap: "</strong>{if status.bootstrap_created { "Created" } else { "Existing" }}</div>
                                </div>
                            }
                                .into_view()
                        } else {
                            view! { <div class="muted">"No vault status yet."</div> }.into_view()
                        }
                    }}
                </article>

                <article class="card">
                    <h3>"OAuth Providers"</h3>
                    {move || {
                        if oauth_statuses.get().is_empty() {
                            view! { <div class="muted">"No OAuth status rows loaded."</div> }.into_view()
                        } else {
                            view! {
                                <For
                                    each=move || oauth_statuses.get()
                                    key=|row| row.provider.clone()
                                    children=move |row| {
                                        view! {
                                            <div class="oauth-row">
                                                <span>{row.provider}</span>
                                                <span>{if row.connected { "connected" } else { "not connected" }}</span>
                                            </div>
                                        }
                                    }
                                />
                            }
                                .into_view()
                        }
                    }}
                </article>
            </div>

            <div class="card-grid two-col">
                <article class="card">
                    <h3>"Credential Actions"</h3>
                    <div class="form-grid">
                        <label>
                            <span>"Provider"</span>
                            <input
                                class="text-input"
                                type="text"
                                prop:value=move || secret_provider.get()
                                on:input=move |ev| set_secret_provider.set(event_target_value(&ev))
                            />
                        </label>
                        <label>
                            <span>"Secret Type"</span>
                            <select
                                class="select-input"
                                prop:value=move || secret_type.get()
                                on:change=move |ev| set_secret_type.set(event_target_value(&ev))
                            >
                                <option value="api_key">"api_key"</option>
                                <option value="oauth_access">"oauth_access"</option>
                                <option value="oauth_refresh">"oauth_refresh"</option>
                                <option value="oauth_client_secret">"oauth_client_secret"</option>
                            </select>
                        </label>
                        <label>
                            <span>"Secret Value"</span>
                            <input
                                class="text-input"
                                type="password"
                                prop:value=move || secret_value.get()
                                on:input=move |ev| set_secret_value.set(event_target_value(&ev))
                            />
                        </label>
                    </div>

                    <div class="toolbar-inline" style="margin-top: 10px;">
                        <button class="action-btn" prop:disabled=move || integration_busy.get() on:click=move |_| (save_secret)()>
                            {move || if integration_busy.get() { "Working..." } else { "Store Secret" }}
                        </button>
                        <button class="action-btn" prop:disabled=move || integration_busy.get() on:click=move |_| (test_secret)()>
                            "Test Secret"
                        </button>
                        <button class="action-btn danger" prop:disabled=move || integration_busy.get() on:click=move |_| (revoke_secret)()>
                            "Revoke Secret"
                        </button>
                    </div>
                </article>

                <article class="card">
                    <h3>"OAuth Actions"</h3>
                    <div class="form-grid single-col">
                        <label>
                            <span>"Provider"</span>
                            <select
                                class="select-input"
                                prop:value=move || oauth_provider.get()
                                on:change=move |ev| set_oauth_provider.set(event_target_value(&ev))
                            >
                                <option value="openai">"openai"</option>
                                <option value="anthropic">"anthropic"</option>
                                <option value="gemini">"gemini"</option>
                                <option value="nvidia">"nvidia"</option>
                            </select>
                        </label>
                    </div>

                    <div class="toolbar-inline" style="margin-top: 10px;">
                        <button class="action-btn" prop:disabled=move || integration_busy.get() on:click=move |_| (refresh_integrations_oauth)()>
                            "Reload OAuth Status"
                        </button>
                        <button class="action-btn danger" prop:disabled=move || integration_busy.get() on:click=move |_| (disconnect_oauth)()>
                            "Disconnect OAuth"
                        </button>
                    </div>
                </article>
            </div>

            <div class="card-grid two-col">
                <article class="card">
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
                            }
                                .into_view()
                        }
                    }}
                </article>

                <article class="card">
                    <h3>"Stored Secrets"</h3>
                    {move || {
                        if secrets.get().is_empty() {
                            view! { <div class="muted">"No secrets currently stored."</div> }.into_view()
                        } else {
                            view! {
                                <For
                                    each=move || secrets.get()
                                    key=|secret| secret.provider.clone()
                                    children=move |secret| {
                                        view! {
                                            <div class="repo-row">
                                                <span>{secret.provider}</span>
                                                <span>{format!("{} • {}", secret.secret_type, secret.last4)}</span>
                                            </div>
                                        }
                                    }
                                />
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

                                                view! {
                                                    <div class="agent-item">
                                                        <div class="agent-meta">
                                                            <span class=format!("status-dot {}", status)></span>
                                                            <div>
                                                                <div class="agent-name">{agent.name}</div>
                                                                <div class="agent-task">{agent.task_id}</div>
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
                        match app_state_for_tabs.active_tab.get() {
                            TabId::Mission => view! { <MissionTabView app_state=app_state_for_tabs.clone()/> }.into_view(),
                            TabId::Branches => view! { <BranchesTabView app_state=app_state_for_tabs.clone()/> }.into_view(),
                            TabId::Gates => view! { <GatesTabView app_state=app_state_for_tabs.clone()/> }.into_view(),
                            TabId::Activity => view! { <ActivityTabView app_state=app_state_for_tabs.clone()/> }.into_view(),
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
                                        let missions = grouped_missions(&app_state.agents.get());
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
    let (chat_error, set_chat_error) = create_signal(String::new());
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

            let refresh_history = Rc::clone(&refresh_history);
            wasm_bindgen_futures::spawn_local(async move {
                match core_request::<Value>(CoreRequestInput {
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
                    Ok(_) => set_chat_error.set(String::new()),
                    Err(err) => set_chat_error.set(format!("Send failed: {}", err)),
                }
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
                            each=move || {
                                messages
                                    .get()
                                    .into_iter()
                                    .enumerate()
                                    .collect::<Vec<_>>()
                            }
                            key=|item| format!("{}-{}-{}", item.0, item.1.role, item.1.content.len())
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
