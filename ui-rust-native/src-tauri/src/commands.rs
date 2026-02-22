use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Manager, State, WebviewUrl, WebviewWindowBuilder as WindowBuilder};

pub struct CoreClientState {
    pub core_base_url: String,
    pub client: reqwest::Client,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoreRequestInput {
    pub method: String,
    pub path: String,
    #[serde(default)]
    pub body: Option<Value>,
    #[serde(default)]
    pub admin_token: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct CoreResponseOutput {
    pub status: u16,
    pub body: Value,
}

#[tauri::command]
pub async fn core_request(
    input: CoreRequestInput,
    state: State<'_, CoreClientState>,
) -> Result<CoreResponseOutput, String> {
    let method = input
        .method
        .parse::<Method>()
        .map_err(|error| format!("Invalid method '{}': {error}", input.method))?;

    let path = if input.path.starts_with('/') {
        input.path
    } else {
        format!("/{}", input.path)
    };

    let url = format!("{}{}", state.core_base_url, path);
    let mut request = state.client.request(method, url);

    if let Some(token) = input.admin_token {
        let trimmed = token.trim();
        if !trimmed.is_empty() {
            request = request.bearer_auth(trimmed);
        }
    }

    if let Some(body) = input.body {
        request = request.json(&body);
    }

    let response = request
        .send()
        .await
        .map_err(|error| format!("Core request failed: {error}"))?;

    let status = response.status().as_u16();
    let text = response
        .text()
        .await
        .map_err(|error| format!("Failed to read response body: {error}"))?;

    let body = if text.trim().is_empty() {
        Value::Null
    } else {
        serde_json::from_str(&text).unwrap_or(Value::String(text))
    };

    Ok(CoreResponseOutput { status, body })
}

#[tauri::command]
pub async fn open_agent_window(app: AppHandle, agent_id: String) -> Result<(), String> {
    let label = format!("agent-{}", agent_id);
    
    if let Some(window) = app.get_webview_window(&label) {
        window.set_focus().map_err(|e| e.to_string())?;
        return Ok(());
    }

    WindowBuilder::new(
        &app,
        label,
        WebviewUrl::App(format!("/#/chat/{}", agent_id).into()),
    )
    .title(format!("Agent {}", agent_id))
    .inner_size(800.0, 600.0)
    .build()
    .map_err(|e| e.to_string())?;

    Ok(())
}

#[tauri::command]
pub async fn focus_agent_window(app: AppHandle, agent_id: String) -> Result<(), String> {
    let label = format!("agent-{}", agent_id);
    if let Some(window) = app.get_webview_window(&label) {
        window.set_focus().map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub async fn close_agent_window(app: AppHandle, agent_id: String) -> Result<(), String> {
    let label = format!("agent-{}", agent_id);
    if let Some(window) = app.get_webview_window(&label) {
        window.close().map_err(|e| e.to_string())?;
    }
    Ok(())
}
