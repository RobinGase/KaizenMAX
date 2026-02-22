use leptos::*;
use leptos_router::*;
use serde_json::{json, Value};
use web_sys::window;
use std::collections::HashMap;

use crate::models::types::*; // Import all types from the new types.rs file

// ... (Simulated Core Request - Keep it simple for the initial build slice)
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
struct CoreResponse {
    status: u16,
    body: Value,
}

// In a real Leptos app using Tauri, you would use tauri_sys::invoke.
// For this first slice, we will just simulate responses to prove the UI compiles and renders.
async fn core_request<T: for<'de> serde::Deserialize<'de>>(input: CoreRequestInput) -> Result<T, String> {
    let response_json = match input.path.as_str() {
        "/health" => json!({"status": "ok", "engine": "Kaizen", "version": "0.1.0"}),
        "/api/agents" => json!([]),
        _ => json!({"error": "Not Found"}),
    };
    serde_json::from_value(response_json).map_err(|e| e.to_string())
}

#[derive(Clone)]
pub struct AppState {
    active_tab: RwSignal<TabId>,
    health: RwSignal<Option<HealthResponse>>,
}

impl AppState {
    fn new() -> Self {
        Self {
            active_tab: create_rw_signal(TabId::Mission),
            health: create_rw_signal(None),
        }
    }
    async fn refresh_health(&self) {
        if let Ok(payload) = core_request::<HealthResponse>(CoreRequestInput {
            method: "GET".to_string(), path: "/health".to_string(), body: None, admin_token: None
        }).await {
            self.health.set(Some(payload));
        }
    }
}

#[component]
pub fn MissionControlApp() -> impl IntoView {
    let app_state = AppState::new();
    provide_context(app_state.clone());

    create_effect(move |_| {
        let state = expect_context::<AppState>();
        wasm_bindgen_futures::spawn_local(async move {
            state.refresh_health().await;
        });
    });

    let health = app_state.health;

    view! {
        <div class="app-shell">
            <div class="mission-layout">
                <nav class="nav-rail">
                    <div class="brand">
                        <div class="brand-title">"Kaizen MAX"</div>
                    </div>
                    <div class="panel-title">"Agents"</div>
                    <div class="agent-item">"Codex"</div>
                    <div class="agent-item">"Frontend Agent"</div>
                    <div class="agent-item">"Backend Agent"</div>
                </nav>
                <main class="main-shell">
                    <header class="top-bar">
                        <span class="status-chip ok">
                            "Health: " {move || health.get().map_or("-".to_string(), |h| h.status)}
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
                <div class="branches-panel">
                    <div class="panel-title">"Branches & Context"</div>
                    <div class="branch-item">"main"</div>
                    <div class="branch-item">"feature/ui-layout"</div>
                </div>
            </div>
        </div>
    }
}

// Implement Display for TabId
impl std::fmt::Display for TabId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}
