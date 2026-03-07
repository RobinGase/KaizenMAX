use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;
use tauri::State;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub struct CoreClientState {
    pub core_base_url: String,
    pub client: reqwest::Client,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoUpdateStatus {
    pub enabled: bool,
    pub git_available: bool,
    pub release_branch: String,
    pub current_branch: Option<String>,
    pub repo_root: Option<String>,
    pub local_commit: Option<String>,
    pub local_subject: Option<String>,
    pub remote_commit: Option<String>,
    pub remote_subject: Option<String>,
    pub behind_count: u32,
    pub update_available: bool,
    pub local_dirty: bool,
    pub can_apply_update: bool,
    pub message: String,
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

fn resolve_repo_root() -> Option<PathBuf> {
    if let Ok(repo_root) = std::env::var("KAIZEN_REPO_ROOT") {
        let path = PathBuf::from(repo_root);
        if path.exists() {
            return Some(path);
        }
    }

    let mut candidates = Vec::new();
    if let Ok(current_exe) = std::env::current_exe() {
        candidates.push(current_exe);
    }
    if let Ok(current_dir) = std::env::current_dir() {
        candidates.push(current_dir);
    }

    for candidate in candidates {
        for ancestor in candidate.ancestors() {
            if ancestor.join(".git").exists()
                && ancestor
                    .join("scripts")
                    .join("launch-kaizen-max.ps1")
                    .exists()
            {
                return Some(ancestor.to_path_buf());
            }
        }
    }

    None
}

fn run_git(repo_root: &Path, args: &[&str]) -> Result<String, String> {
    let mut command = Command::new("git");
    command.arg("-C").arg(repo_root).args(args);

    let output = command
        .output()
        .map_err(|error| format!("Failed to run git {}: {error}", args.join(" ")))?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if stderr.is_empty() {
            Err(format!(
                "git {} failed with status {}",
                args.join(" "),
                output.status
            ))
        } else {
            Err(stderr)
        }
    }
}

fn read_update_status() -> RepoUpdateStatus {
    let release_branch = "main".to_string();
    let Some(repo_root) = resolve_repo_root() else {
        return RepoUpdateStatus {
            enabled: false,
            git_available: false,
            release_branch,
            current_branch: None,
            repo_root: None,
            local_commit: None,
            local_subject: None,
            remote_commit: None,
            remote_subject: None,
            behind_count: 0,
            update_available: false,
            local_dirty: false,
            can_apply_update: false,
            message:
                "Repo install not detected. Launch from the Kaizen MAX launcher to enable updates."
                    .to_string(),
        };
    };

    let current_branch = match run_git(&repo_root, &["rev-parse", "--abbrev-ref", "HEAD"]) {
        Ok(branch) => Some(branch),
        Err(error) => {
            return RepoUpdateStatus {
                enabled: true,
                git_available: false,
                release_branch,
                current_branch: None,
                repo_root: Some(repo_root.display().to_string()),
                local_commit: None,
                local_subject: None,
                remote_commit: None,
                remote_subject: None,
                behind_count: 0,
                update_available: false,
                local_dirty: false,
                can_apply_update: false,
                message: format!("Git is required for updates but could not be used: {error}"),
            };
        }
    };

    let local_dirty = run_git(&repo_root, &["status", "--porcelain"])
        .map(|output| !output.is_empty())
        .unwrap_or(false);

    let local_commit = run_git(&repo_root, &["rev-parse", "HEAD"]).ok();
    let local_subject = run_git(&repo_root, &["log", "-1", "--pretty=%s", "HEAD"]).ok();

    let fetch_error = run_git(&repo_root, &["fetch", "--quiet", "origin", "main"]).err();
    let remote_commit = run_git(&repo_root, &["rev-parse", "origin/main"]).ok();
    let remote_subject = run_git(&repo_root, &["log", "-1", "--pretty=%s", "origin/main"]).ok();

    let behind_count = run_git(&repo_root, &["rev-list", "--count", "HEAD..origin/main"])
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(0);

    let update_available = behind_count > 0;
    let on_release_branch = current_branch.as_deref() == Some("main");
    let can_apply_update = update_available
        && on_release_branch
        && !local_dirty
        && repo_root
            .join("scripts")
            .join("update-kaizen-max.ps1")
            .exists();

    let message = if let Some(error) = fetch_error {
        if remote_commit.is_none() {
            format!("Could not reach origin/main to check for updates: {error}")
        } else if update_available {
            format!("{behind_count} release update(s) are available from origin/main.")
        } else {
            "You are on the latest fetched release. Remote refresh failed, so status may be stale."
                .to_string()
        }
    } else if !on_release_branch {
        format!(
            "Release updates track main. Current branch is {}.",
            current_branch
                .clone()
                .unwrap_or_else(|| "unknown".to_string())
        )
    } else if local_dirty {
        "Local changes are present. Commit or discard them before applying a release update."
            .to_string()
    } else if update_available {
        format!("{behind_count} release update(s) are ready from origin/main.")
    } else {
        "This install is current with origin/main.".to_string()
    };

    RepoUpdateStatus {
        enabled: true,
        git_available: true,
        release_branch,
        current_branch,
        repo_root: Some(repo_root.display().to_string()),
        local_commit,
        local_subject,
        remote_commit,
        remote_subject,
        behind_count,
        update_available,
        local_dirty,
        can_apply_update,
        message,
    }
}

fn powershell_path() -> String {
    if let Ok(system_root) = std::env::var("SystemRoot") {
        let candidate = PathBuf::from(system_root)
            .join("System32")
            .join("WindowsPowerShell")
            .join("v1.0")
            .join("powershell.exe");
        if candidate.exists() {
            return candidate.display().to_string();
        }
    }
    "powershell.exe".to_string()
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
pub async fn get_repo_update_status() -> Result<RepoUpdateStatus, String> {
    Ok(read_update_status())
}

#[tauri::command]
pub async fn apply_repo_update() -> Result<(), String> {
    let status = read_update_status();
    if !status.enabled {
        return Err(status.message);
    }
    if !status.git_available {
        return Err(status.message);
    }
    if !status.update_available {
        return Err("No release update is currently available.".to_string());
    }
    if !status.can_apply_update {
        return Err(status.message);
    }

    let repo_root = PathBuf::from(
        status
            .repo_root
            .clone()
            .ok_or_else(|| "Repo root could not be determined.".to_string())?,
    );
    let script_path = repo_root.join("scripts").join("update-kaizen-max.ps1");
    if !script_path.exists() {
        return Err("Update script is missing.".to_string());
    }

    let mut command = Command::new(powershell_path());
    command
        .arg("-NoProfile")
        .arg("-ExecutionPolicy")
        .arg("Bypass")
        .arg("-File")
        .arg(&script_path)
        .current_dir(&repo_root);

    #[cfg(windows)]
    {
        command.creation_flags(CREATE_NO_WINDOW);
    }

    command
        .spawn()
        .map_err(|error| format!("Failed to start repo update: {error}"))?;

    Ok(())
}
