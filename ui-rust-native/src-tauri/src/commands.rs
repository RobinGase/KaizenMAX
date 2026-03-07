use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    env,
    path::{Path, PathBuf},
    process::Command as ProcessCommand,
    time::Duration,
};
use tauri::{AppHandle, Manager, State, WebviewUrl, WebviewWindowBuilder as WindowBuilder};
use tracing::{debug, info};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

fn agent_window_label(agent_id: &str) -> String {
    let safe_id: String = agent_id
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect();
    format!("agent-{}", safe_id)
}

const OFFICE_WINDOW_LABEL: &str = "office-board";

pub struct CoreClientState {
    pub core_base_url: String,
    pub client: reqwest::Client,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseUpdateStatus {
    pub supported: bool,
    pub repo_root: Option<String>,
    pub current_branch: Option<String>,
    pub current_commit: Option<String>,
    pub remote_commit: Option<String>,
    pub worktree_clean: bool,
    pub update_available: bool,
    pub can_apply: bool,
    pub behind_count: u32,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseUpdateAction {
    pub started: bool,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DetachedWindowStatus {
    pub agent_ids: Vec<String>,
    pub office_open: bool,
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

fn derive_repo_root_from_exe() -> Option<PathBuf> {
    let exe = env::current_exe().ok()?;
    let mut current = exe.parent()?.to_path_buf();
    for _ in 0..4 {
        current = current.parent()?.to_path_buf();
    }
    if current.join(".git").exists() {
        Some(current)
    } else {
        None
    }
}

fn resolve_repo_root() -> Option<PathBuf> {
    if let Ok(value) = env::var("KAIZEN_REPO_ROOT") {
        let candidate = PathBuf::from(value);
        if candidate.join(".git").exists() {
            return Some(candidate);
        }
    }
    derive_repo_root_from_exe()
}

fn run_git(repo_root: &Path, args: &[&str]) -> Result<String, String> {
    let output = ProcessCommand::new("git")
        .args(args)
        .current_dir(repo_root)
        .output()
        .map_err(|error| format!("Failed to run git {}: {error}", args.join(" ")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let detail = if !stderr.is_empty() { stderr } else { stdout };
        return Err(format!("git {} failed: {}", args.join(" "), detail));
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn release_update_status() -> ReleaseUpdateStatus {
    let Some(repo_root) = resolve_repo_root() else {
        return ReleaseUpdateStatus {
            supported: false,
            repo_root: None,
            current_branch: None,
            current_commit: None,
            remote_commit: None,
            worktree_clean: false,
            update_available: false,
            can_apply: false,
            behind_count: 0,
            reason: "This install is not running from a repo checkout, so repo-based updates are unavailable.".to_string(),
        };
    };

    let repo_root_text = repo_root.display().to_string();

    let current_branch = match run_git(&repo_root, &["rev-parse", "--abbrev-ref", "HEAD"]) {
        Ok(value) => Some(value),
        Err(error) => {
            return ReleaseUpdateStatus {
                supported: false,
                repo_root: Some(repo_root_text),
                current_branch: None,
                current_commit: None,
                remote_commit: None,
                worktree_clean: false,
                update_available: false,
                can_apply: false,
                behind_count: 0,
                reason: error,
            };
        }
    };

    let current_commit = run_git(&repo_root, &["rev-parse", "HEAD"]).ok();
    let worktree_clean = run_git(&repo_root, &["status", "--porcelain"])
        .map(|value| value.trim().is_empty())
        .unwrap_or(false);

    if let Err(error) = run_git(&repo_root, &["fetch", "origin", "main"]) {
        return ReleaseUpdateStatus {
            supported: false,
            repo_root: Some(repo_root_text),
            current_branch,
            current_commit,
            remote_commit: None,
            worktree_clean,
            update_available: false,
            can_apply: false,
            behind_count: 0,
            reason: error,
        };
    }

    let remote_commit = match run_git(&repo_root, &["rev-parse", "origin/main"]) {
        Ok(value) => Some(value),
        Err(error) => {
            return ReleaseUpdateStatus {
                supported: false,
                repo_root: Some(repo_root_text),
                current_branch,
                current_commit,
                remote_commit: None,
                worktree_clean,
                update_available: false,
                can_apply: false,
                behind_count: 0,
                reason: error,
            };
        }
    };

    let behind_count = run_git(&repo_root, &["rev-list", "--count", "HEAD..origin/main"])
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(0);

    let on_main = current_branch.as_deref() == Some("main");
    let update_available = behind_count > 0;
    let can_apply = on_main && worktree_clean && update_available;

    let reason = if !on_main {
        "Release updates are only available when this checkout is on main.".to_string()
    } else if !worktree_clean {
        "Local changes are present. Commit or discard them before applying a release update.".to_string()
    } else if update_available {
        "A newer main branch release is available.".to_string()
    } else {
        "This install is already on the latest main branch release.".to_string()
    };

    ReleaseUpdateStatus {
        supported: true,
        repo_root: Some(repo_root_text),
        current_branch,
        current_commit,
        remote_commit,
        worktree_clean,
        update_available,
        can_apply,
        behind_count,
        reason,
    }
}

fn spawn_release_update_process(repo_root: &Path) -> Result<(), String> {
    let script_path = repo_root.join("scripts").join("update-kaizen-max.ps1");
    if !script_path.exists() {
        return Err(format!(
            "Updater script not found at {}",
            script_path.display()
        ));
    }

    let mut command = ProcessCommand::new("powershell.exe");
    command
        .arg("-NoLogo")
        .arg("-NoProfile")
        .arg("-ExecutionPolicy")
        .arg("Bypass")
        .arg("-File")
        .arg(&script_path)
        .arg("-RepoRoot")
        .arg(repo_root)
        .current_dir(repo_root);

    #[cfg(windows)]
    {
        command.creation_flags(CREATE_NO_WINDOW);
    }

    command
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("Failed to start updater: {error}"))
}

#[tauri::command]
pub async fn open_external_url(url: String) -> Result<(), String> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return Err("URL is required.".to_string());
    }
    if !trimmed.starts_with("http://") && !trimmed.starts_with("https://") {
        return Err("Only http:// and https:// URLs can be opened externally.".to_string());
    }

    webbrowser::open(trimmed)
        .map(|_| ())
        .map_err(|error| format!("Failed to open external URL: {error}"))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalAuthAction {
    pub started: bool,
    pub message: String,
}

fn provider_auth_command(provider: &str) -> Result<ProcessCommand, String> {
    match provider.trim().to_ascii_lowercase().as_str() {
        "codex" | "codex-cli" => {
            let mut command = if cfg!(windows) {
                let mut command = ProcessCommand::new("cmd");
                command.arg("/C").arg("codex");
                command
            } else {
                ProcessCommand::new("codex")
            };
            command.arg("login");
            Ok(command)
        }
        "gemini" | "gemini-cli" => {
            let mut command = ProcessCommand::new("gemini");
            command.arg("login");
            Ok(command)
        }
        other => Err(format!(
            "Local sign-in is not available for provider '{}'.",
            other
        )),
    }
}

#[tauri::command]
pub async fn start_local_auth_flow(provider: String) -> Result<LocalAuthAction, String> {
    let mut command = provider_auth_command(&provider)?;

    #[cfg(windows)]
    {
        command.creation_flags(CREATE_NO_WINDOW);
    }

    command
        .spawn()
        .map_err(|error| format!("Failed to start sign-in for {}: {error}", provider))?;

    let message = match provider.trim().to_ascii_lowercase().as_str() {
        "codex" | "codex-cli" => {
            "Codex sign-in started. Finish the login in your browser, then return here."
        }
        "gemini" | "gemini-cli" => {
            "Gemini sign-in started. Finish the login flow, then return here."
        }
        _ => "Sign-in started.",
    };

    Ok(LocalAuthAction {
        started: true,
        message: message.to_string(),
    })
}

#[tauri::command]
pub async fn core_request(
    input: CoreRequestInput,
    state: State<'_, CoreClientState>,
) -> Result<CoreResponseOutput, String> {
    let method_text = input.method.clone();
    let method = input
        .method
        .to_ascii_uppercase()
        .parse::<Method>()
        .map_err(|error| format!("Invalid method '{}': {error}", input.method))?;

    let path = if input.path.starts_with('/') {
        input.path
    } else {
        format!("/{}", input.path)
    };

    let url = format!("{}{}", state.core_base_url, path);
    let mut request = state.client.request(method, url);

    debug!(
        method = %method_text,
        path = %path,
        has_body = input.body.is_some(),
        has_admin_token = input.admin_token.is_some(),
        "core_request start"
    );

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

    debug!(status = status, path = %path, "core_request complete");

    Ok(CoreResponseOutput { status, body })
}

#[tauri::command]
pub async fn check_release_update() -> Result<ReleaseUpdateStatus, String> {
    Ok(release_update_status())
}

#[tauri::command]
pub async fn apply_release_update(app: AppHandle) -> Result<ReleaseUpdateAction, String> {
    let status = release_update_status();
    if !status.supported {
        return Err(status.reason);
    }
    if !status.can_apply {
        return Err(status.reason);
    }

    let repo_root = status
        .repo_root
        .clone()
        .ok_or_else(|| "Updater repo root is unavailable.".to_string())?;
    spawn_release_update_process(Path::new(&repo_root))?;

    let app_to_close = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(400));
        app_to_close.exit(0);
    });

    Ok(ReleaseUpdateAction {
        started: true,
        message: "Applying update from origin/main. Mission Control will restart when the build is ready.".to_string(),
    })
}

#[tauri::command(rename_all = "snake_case")]
pub async fn open_agent_window(app: AppHandle, agent_id: String) -> Result<(), String> {
    let label = agent_window_label(&agent_id);

    info!(label = %label, "open_agent_window requested");

    if let Some(window) = app.get_webview_window(&label) {
        window.set_focus().map_err(|e| e.to_string())?;
        info!(label = %label, "agent window already existed; focused");
        return Ok(());
    }

    WindowBuilder::new(
        &app,
        label.clone(),
        WebviewUrl::App(format!("/chat/{}", agent_id).into()),
    )
    .title(format!("Agent {}", agent_id))
    .inner_size(800.0, 600.0)
    .build()
    .map_err(|e| e.to_string())?;

    info!(label = %label, "agent window created");

    Ok(())
}

#[tauri::command(rename_all = "snake_case")]
pub async fn focus_agent_window(app: AppHandle, agent_id: String) -> Result<(), String> {
    let label = agent_window_label(&agent_id);
    info!(label = %label, "focus_agent_window requested");
    if let Some(window) = app.get_webview_window(&label) {
        window.set_focus().map_err(|e| e.to_string())?;
        info!(label = %label, "agent window focused");
    }
    Ok(())
}

#[tauri::command(rename_all = "snake_case")]
pub async fn close_agent_window(app: AppHandle, agent_id: String) -> Result<(), String> {
    let label = agent_window_label(&agent_id);
    info!(label = %label, "close_agent_window requested");
    if let Some(window) = app.get_webview_window(&label) {
        window.close().map_err(|e| e.to_string())?;
        info!(label = %label, "agent window closed");
    }
    Ok(())
}

#[tauri::command(rename_all = "snake_case")]
pub async fn open_office_window(app: AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(OFFICE_WINDOW_LABEL) {
        window.set_focus().map_err(|e| e.to_string())?;
        info!("office window already existed; focused");
        return Ok(());
    }

    WindowBuilder::new(
        &app,
        OFFICE_WINDOW_LABEL,
        WebviewUrl::App("/office".into()),
    )
    .title("Kaizen Office")
    .inner_size(1400.0, 920.0)
    .build()
    .map_err(|e| e.to_string())?;

    info!("office window created");
    Ok(())
}

#[tauri::command(rename_all = "snake_case")]
pub async fn focus_office_window(app: AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(OFFICE_WINDOW_LABEL) {
        window.set_focus().map_err(|e| e.to_string())?;
        info!("office window focused");
    }
    Ok(())
}

#[tauri::command(rename_all = "snake_case")]
pub async fn list_detached_windows(app: AppHandle) -> Result<DetachedWindowStatus, String> {
    let mut agent_ids = Vec::new();
    for label in app.webview_windows().keys() {
        if let Some(agent_id) = label.strip_prefix("agent-") {
            agent_ids.push(agent_id.to_string());
        }
    }
    agent_ids.sort();

    Ok(DetachedWindowStatus {
        agent_ids,
        office_open: app.get_webview_window(OFFICE_WINDOW_LABEL).is_some(),
    })
}
