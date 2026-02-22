use leptos::*;
use leptos::ev;
use leptos_router::*;
use serde_json::{json, Value};
use std::collections::BTreeMap;
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
    let args = serde_wasm_bindgen::to_value(&json!({ "input": input })).map_err(|e| e.to_string())?;
    let response = tauri_invoke("core_request", args)
        .await
        .map_err(js_error)?;
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

        // Initial fetch
        let state_init = state.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let _ = state_init.refresh_health().await;
            let _ = state_init.refresh_agents().await;
            let _ = state_init.refresh_events().await;
        });

        // Polling every 5 seconds
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

    let left_width = create_rw_signal(260);
    let right_width = create_rw_signal(320);
    let dragging = create_rw_signal(None::<&'static str>);

    window_event_listener(ev::mousemove, move |ev| {
        if let Some(side) = dragging.get_untracked() {
            let x = ev.client_x();
            if side == "left" {
                left_width.set(x.max(150).min(500));
            } else {
                if let Some(win) = web_sys::window() {
                    let window_width = win.inner_width().unwrap().as_f64().unwrap() as i32;
                    right_width.set((window_width - x).max(200).min(600));
                }
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
                style=move || format!("grid-template-columns: {}px 4px 1fr 4px {}px", left_width.get(), right_width.get())
            >
                <nav class="nav-rail">
                    <div class="brand">
                        <div class="brand-title">"Kaizen MAX"</div>
                    </div>
                    <div class="panel-title">"Agents"</div>
                    {move || {
                        app_state
                            .agents
                            .get()
                            .into_iter()
                            .map(|agent| {
                                let agent_id = agent.id.clone();
                                view! {
                                    <div class="agent-item" style="display: flex; justify-content: space-between; align-items: center;">
                                        <span>{agent.name}</span>
                                        <button
                                            style="background: transparent; border: 1px solid #444; color: #aaa; border-radius: 4px; padding: 2px 6px; cursor: pointer; font-size: 10px;"
                                            on:click=move |_| {
                                                let id = agent_id.clone();
                                                wasm_bindgen_futures::spawn_local(async move {
                                                    detach_agent(id).await;
                                                });
                                            }
                                        >
                                            "Detach"
                                        </button>
                                    </div>
                                }
                            })
                            .collect_view()
                    }}
                </nav>

                <div
                    class="resizer-h"
                    class:active=move || dragging.get() == Some("left")
                    on:mousedown=move |_| dragging.set(Some("left"))
                />

                <main class="main-shell">
                    <header class="top-bar">
                        <span class="status-chip ok">
                            "Health: "
                            {move || health.get().map_or("-".to_string(), |h| h.status)}
                        </span>
                    </header>
                    <div class="chat-panel">
                        <div class="chat-log">
                            <div class="message agent">
                                <div class="msg-sender">"Codex"</div>
                                "Welcome to Kaizen MAX. How can we build today?"
                            </div>
                            <div class="message user">
                                <div class="msg-sender">"User"</div>
                                "Implement the initial tri-pane layout."
                            </div>
                            <div class="message agent">
                                <div class="msg-sender">"Frontend Agent"</div>
                                "I have implemented the layout and applied Codex-style CSS."
                            </div>
                        </div>
                        <div class="composer-container">
                            <textarea class="composer" rows="3" placeholder="Message the agents..."></textarea>
                        </div>
                    </div>
                </main>

                <div
                    class="resizer-h"
                    class:active=move || dragging.get() == Some("right")
                    on:mousedown=move |_| dragging.set(Some("right"))
                />

                <div class="branches-panel">
                    <div class="panel-title">"Company Branches"</div>
                    <div class="hierarchy" style="font-family: var(--font-mono); font-size: 12px;">
                        <div class="orchestrator-node" style="margin-bottom: 12px;">
                            <div style="color: var(--mc-text); font-weight: bold; margin-bottom: 4px;">"▼ Orchestrator"</div>
                            <div class="branch-nodes" style="margin-left: 8px; border-left: 1px solid var(--mc-border); padding-left: 12px;">
                                <div class="branch-node">
                                    <div style="color: var(--mc-text); margin-bottom: 4px;">"▼ Branch: Primary"</div>
                                    <div class="mission-nodes" style="margin-left: 8px; border-left: 1px solid var(--mc-border); padding-left: 12px;">
                                        {move || {
                                            let agents = app_state.agents.get();
                                            let mut missions: BTreeMap<String, Vec<SubAgent>> = BTreeMap::new();
                                            for agent in agents {
                                                let tid = agent.task_id.clone().unwrap_or_else(|| "general".to_string());
                                                missions.entry(tid).or_default().push(agent);
                                            }

                                            missions.into_iter().map(|(tid, workers)| {
                                                let active_count = workers.iter().filter(|w| matches!(w.status, AgentStatus::Active)).count();
                                                let total_count = workers.len();

                                                view! {
                                                    <div class="mission-node" style="margin-bottom: 8px;">
                                                        <div style="color: var(--mc-muted); display: flex; justify-content: space-between; margin-bottom: 2px;">
                                                            <span>"▼ Mission: " {tid}</span>
                                                            <span style="font-size: 10px; opacity: 0.6;">{format!("{}/{}", active_count, total_count)}</span>
                                                        </div>
                                                        <div class="worker-nodes" style="margin-left: 8px; border-left: 1px solid var(--mc-border); padding-left: 12px;">
                                                            {workers.into_iter().map(|worker| {
                                                                let status_color = match worker.status {
                                                                    AgentStatus::Active => "#4caf50",
                                                                    AgentStatus::Idle => "#888888",
                                                                    AgentStatus::Blocked => "#f44336",
                                                                    AgentStatus::Done => "#2196f3",
                                                                };
                                                                view! {
                                                                    <div class="worker-node" style="margin-top: 2px; display: flex; align-items: center; gap: 8px;">
                                                                        <span style=format!("width: 6px; height: 6px; border-radius: 50%; background: {};", status_color)></span>
                                                                        <span style="color: var(--mc-text); opacity: 0.8;">{worker.name}</span>
                                                                    </div>
                                                                }
                                                            }).collect_view()}
                                                        </div>
                                                    </div>
                                                }
                                            }).collect_view()
                                        }}
                                    </div>
                                </div>
                            </div>
                        </div>
                    </div>
                </div>
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

    let refresh_history = move || {
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
    };

    create_effect(move |_| {
        refresh_history();
        let handle = set_interval_with_handle(move || refresh_history(), Duration::from_secs(3))
            .expect("Failed to set interval");

        on_cleanup(move || {
            handle.clear();
        });
    });

    let send_message = move || {
        let id = agent_id();
        let text = input.get();
        if text.is_empty() || id.is_empty() {
            return;
        }

        set_input.set(String::new());

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
            refresh_history();
        });
    };

    view! {
        <div class="app-shell" style="grid-template-columns: 1fr;">
            <main class="main-shell">
                <header class="top-bar">
                    <span class="status-chip neutral">
                        "Detached Agent Chat: " {agent_id}
                    </span>
                </header>
                <div class="chat-panel">
                    <div class="chat-log" style="overflow-y: auto; flex: 1;">
                        <For
                            each=move || messages.get()
                            key=|msg| format!("{}-{}", msg.role, msg.content.len())
                            children=move |msg| {
                                let role_class = if msg.role == "user" { "message user" } else { "message agent" };
                                let sender = if msg.role == "user" { "User" } else { "Agent" };
                                view! {
                                    <div class=role_class>
                                        <div class="msg-sender">{sender}</div>
                                        {msg.content}
                                    </div>
                                }
                            }
                        />
                    </div>
                    <div class="composer-container" style="display: flex; gap: 8px; align-items: flex-end;">
                        <textarea
                            class="composer"
                            style="flex: 1;"
                            rows="3"
                            placeholder="Message agent..."
                            prop:value=move || input.get()
                            on:input=move |ev| set_input.set(event_target_value(&ev))
                            on:keydown=move |ev| {
                                if ev.key() == "Enter" && !ev.shift_key() {
                                    ev.prevent_default();
                                    send_message();
                                }
                            }
                        ></textarea>
                        <button
                            style="background: #444; color: white; border: none; border-radius: 4px; padding: 8px 16px; cursor: pointer; margin-bottom: 8px;"
                            on:click=move |_| send_message()
                        >
                            "Send"
                        </button>
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
