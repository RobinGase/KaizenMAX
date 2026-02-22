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

    let (messages, set_messages) = create_signal(Vec::<InferenceChatMessage>::new());
    let (input, set_input) = create_signal(String::new());

    let refresh_history = move || {
        wasm_bindgen_futures::spawn_local(async move {
            if let Ok(res) = core_request::<ChatHistoryResponse>(CoreRequestInput {
                method: "GET".to_string(),
                path: "/api/chat/history".to_string(),
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
        let text = input.get();
        if text.is_empty() {
            return;
        }

        set_input.set(String::new());

        wasm_bindgen_futures::spawn_local(async move {
            let _ = core_request::<Value>(CoreRequestInput {
                method: "POST".to_string(),
                path: "/api/chat".to_string(),
                body: Some(json!({
                    "message": text,
                    "agent_id": None::<String>,
                })),
                admin_token: None,
            })
            .await;
            refresh_history();
        });
    };


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

                    <div class="nav-tabs">
                        <div class="nav-tab active">"Mission"</div>
                        <div class="nav-tab">"Branches"</div>
                        <div class="nav-tab">"Gates"</div>
                        <div class="nav-tab">"Activity"</div>
                        <div class="nav-tab">"Workspace"</div>
                        <div class="nav-tab">"Integrations"</div>
                        <div class="nav-tab">"Settings"</div>
                    </div>

                    <div class="panel-title">"Agents"</div>
                    {move || {
                        app_state
                            .agents
                            .get()
                            .into_iter()
                            .map(|agent| {
                                let agent_id = agent
