use serde::Serialize;
use serde_json::Value;
use std::{collections::HashSet, env};
use tokio::process::Command;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenClawBridgeStatus {
    pub enabled: bool,
    pub cli_available: bool,
    pub gateway_reachable: bool,
    pub allowed_tools: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct OpenClawToolResult {
    pub tool_id: String,
    pub reply: String,
}

#[derive(Debug, Clone)]
enum OpenClawInvocation {
    Sessions,
    Health,
    BrowserStatus,
    BrowserTabs,
    BrowserOpen { url: String },
    CronStatus,
    CronList,
}

impl OpenClawInvocation {
    fn tool_id(&self) -> &'static str {
        match self {
            Self::Sessions => "sessions",
            Self::Health => "health",
            Self::BrowserStatus | Self::BrowserTabs | Self::BrowserOpen { .. } => "browser",
            Self::CronStatus | Self::CronList => "scheduler",
        }
    }

    fn args(&self) -> Vec<String> {
        match self {
            Self::Sessions => vec!["sessions".into(), "--json".into()],
            Self::Health => vec!["health".into(), "--json".into()],
            Self::BrowserStatus => vec!["browser".into(), "status".into(), "--json".into()],
            Self::BrowserTabs => vec!["browser".into(), "tabs".into(), "--json".into()],
            Self::BrowserOpen { url } => vec![
                "browser".into(),
                "open".into(),
                url.clone(),
                "--json".into(),
            ],
            Self::CronStatus => vec!["cron".into(), "status".into(), "--json".into()],
            Self::CronList => vec!["cron".into(), "list".into(), "--json".into()],
        }
    }
}

pub async fn status() -> OpenClawBridgeStatus {
    let config = BridgeConfig::load();
    let cli_available = openclaw_cli_available(&config).await;
    let gateway_reachable = if cli_available {
        command_succeeds(&config, &["health", "--json"]).await
    } else {
        false
    };

    OpenClawBridgeStatus {
        enabled: config.enabled,
        cli_available,
        gateway_reachable,
        allowed_tools: config.allowed_tools.into_iter().collect(),
    }
}

pub async fn maybe_execute_from_prompt(message: &str) -> Result<Option<OpenClawToolResult>, String> {
    let config = BridgeConfig::load();
    if !config.enabled {
        return Ok(None);
    }

    let Some(invocation) = detect_invocation(message) else {
        return Ok(None);
    };

    if !config.allowed_tools.contains(invocation.tool_id()) {
        return Ok(None);
    }

    let cli_available = openclaw_cli_available(&config).await;
    if !cli_available {
        return Ok(Some(OpenClawToolResult {
            tool_id: invocation.tool_id().to_string(),
            reply: "OpenClaw fallback is enabled, but the OpenClaw CLI is not available on this machine.".to_string(),
        }));
    }

    let output = run_invocation(&config, &invocation).await?;
    Ok(Some(OpenClawToolResult {
        tool_id: invocation.tool_id().to_string(),
        reply: format_output(&invocation, &output),
    }))
}

#[derive(Debug, Clone)]
struct BridgeConfig {
    enabled: bool,
    cli_path: String,
    allowed_tools: HashSet<String>,
}

impl BridgeConfig {
    fn load() -> Self {
        let cli_path = env::var("OPENCLAW_CLI_PATH").unwrap_or_else(|_| {
            if cfg!(windows) {
                "openclaw.cmd".to_string()
            } else {
                "openclaw".to_string()
            }
        });

        let enabled = env::var("ZEROCLAW_OPENCLAW_FALLBACK_ENABLED")
            .ok()
            .map(|value| !matches!(value.trim().to_ascii_lowercase().as_str(), "0" | "false" | "off" | "no"))
            .unwrap_or(true);

        let allowed_tools = env::var("ZEROCLAW_OPENCLAW_ALLOWED_TOOLS")
            .ok()
            .unwrap_or_else(|| "sessions,browser,scheduler,health".to_string())
            .split(',')
            .map(|item| item.trim().to_ascii_lowercase())
            .filter(|item| !item.is_empty())
            .collect();

        Self {
            enabled,
            cli_path,
            allowed_tools,
        }
    }
}

async fn openclaw_cli_available(config: &BridgeConfig) -> bool {
    command_succeeds(config, &["--version"]).await
}

async fn command_succeeds(config: &BridgeConfig, args: &[&str]) -> bool {
    Command::new(&config.cli_path)
        .args(args)
        .output()
        .await
        .map(|output| output.status.success())
        .unwrap_or(false)
}

async fn run_invocation(config: &BridgeConfig, invocation: &OpenClawInvocation) -> Result<String, String> {
    let output = Command::new(&config.cli_path)
        .args(invocation.args())
        .output()
        .await
        .map_err(|error| format!("Failed to start OpenClaw: {error}"))?;

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();

    if !output.status.success() {
        let detail = if !stderr.is_empty() { stderr } else { stdout };
        return Err(format!("OpenClaw {} failed: {}", invocation.tool_id(), detail));
    }

    if !stdout.is_empty() {
        Ok(stdout)
    } else if !stderr.is_empty() {
        Ok(stderr)
    } else {
        Ok(String::new())
    }
}

fn detect_invocation(message: &str) -> Option<OpenClawInvocation> {
    let trimmed = message.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Some(explicit) = parse_explicit_command(trimmed) {
        return Some(explicit);
    }

    let lower = trimmed.to_ascii_lowercase();
    if lower.contains("openclaw sessions") || lower.contains("show openclaw sessions") {
        return Some(OpenClawInvocation::Sessions);
    }
    if lower.contains("openclaw health") || lower == "health" || lower == "openclaw status" {
        return Some(OpenClawInvocation::Health);
    }
    if lower.contains("browser status") && lower.contains("openclaw") {
        return Some(OpenClawInvocation::BrowserStatus);
    }
    if lower.contains("browser tabs") && lower.contains("openclaw") {
        return Some(OpenClawInvocation::BrowserTabs);
    }
    if (lower.contains("openclaw") || lower.contains("browser"))
        && (lower.contains("open ") || lower.contains("browse ") || lower.contains("visit "))
    {
        if let Some(url) = first_url(trimmed) {
            return Some(OpenClawInvocation::BrowserOpen { url });
        }
    }
    if lower.contains("openclaw cron status") || lower.contains("scheduler status") {
        return Some(OpenClawInvocation::CronStatus);
    }
    if lower.contains("openclaw cron") || lower.contains("list cron jobs") {
        return Some(OpenClawInvocation::CronList);
    }

    None
}

fn parse_explicit_command(message: &str) -> Option<OpenClawInvocation> {
    let raw = message
        .strip_prefix("/openclaw ")
        .or_else(|| message.strip_prefix("openclaw:"))
        .or_else(|| message.strip_prefix("openclaw "))
        .map(str::trim)?;

    let tokens: Vec<&str> = raw.split_whitespace().collect();
    match tokens.as_slice() {
        ["sessions"] => Some(OpenClawInvocation::Sessions),
        ["health"] | ["status"] => Some(OpenClawInvocation::Health),
        ["browser", "status"] => Some(OpenClawInvocation::BrowserStatus),
        ["browser", "tabs"] => Some(OpenClawInvocation::BrowserTabs),
        ["browser", "open", url] | ["browser", "navigate", url] => Some(OpenClawInvocation::BrowserOpen {
            url: (*url).to_string(),
        }),
        ["cron", "status"] => Some(OpenClawInvocation::CronStatus),
        ["cron", "list"] => Some(OpenClawInvocation::CronList),
        _ => None,
    }
}

fn first_url(message: &str) -> Option<String> {
    message
        .split_whitespace()
        .find(|part| part.starts_with("http://") || part.starts_with("https://"))
        .map(|part| part.trim_matches(|ch: char| ch == '"' || ch == '\'' || ch == ')' || ch == ']' || ch == '>'))
        .map(str::to_string)
}

fn format_output(invocation: &OpenClawInvocation, output: &str) -> String {
    match invocation {
        OpenClawInvocation::Sessions => summarize_sessions(output),
        OpenClawInvocation::Health => summarize_health(output),
        OpenClawInvocation::BrowserStatus => summarize_browser_status(output),
        OpenClawInvocation::BrowserTabs => summarize_browser_tabs(output),
        OpenClawInvocation::BrowserOpen { url } => format!("OpenClaw opened {} in its browser.", url),
        OpenClawInvocation::CronStatus => summarize_cron_status(output),
        OpenClawInvocation::CronList => summarize_cron_list(output),
    }
}

fn summarize_sessions(output: &str) -> String {
    if let Some(value) = parse_json_fragment(output) {
        let count = value.get("count").and_then(|v| v.as_u64()).unwrap_or(0);
        let mut lines = vec![format!("OpenClaw sessions: {} total.", count)];
        if let Some(sessions) = value.get("sessions").and_then(|v| v.as_array()) {
            for session in sessions.iter().take(3) {
                let key = session.get("key").and_then(|v| v.as_str()).unwrap_or("unknown");
                let model = session.get("model").and_then(|v| v.as_str()).unwrap_or("unknown");
                lines.push(format!("- {} ({})", key, model));
            }
        }
        return lines.join("\n");
    }
    format!("OpenClaw sessions:\n{}", output.trim())
}

fn summarize_health(output: &str) -> String {
    if let Some(value) = parse_json_fragment(output) {
        return format!(
            "OpenClaw gateway health: {}",
            serde_json::to_string_pretty(&value).unwrap_or_else(|_| output.trim().to_string())
        );
    }
    format!("OpenClaw gateway health:\n{}", output.trim())
}

fn summarize_browser_status(output: &str) -> String {
    if let Some(value) = parse_json_fragment(output) {
        return format!(
            "OpenClaw browser status:\n{}",
            serde_json::to_string_pretty(&value).unwrap_or_else(|_| output.trim().to_string())
        );
    }
    format!("OpenClaw browser status:\n{}", output.trim())
}

fn summarize_browser_tabs(output: &str) -> String {
    if let Some(value) = parse_json_fragment(output) {
        return format!(
            "OpenClaw browser tabs:\n{}",
            serde_json::to_string_pretty(&value).unwrap_or_else(|_| output.trim().to_string())
        );
    }
    format!("OpenClaw browser tabs:\n{}", output.trim())
}

fn summarize_cron_status(output: &str) -> String {
    if let Some(value) = parse_json_fragment(output) {
        return format!(
            "OpenClaw scheduler status:\n{}",
            serde_json::to_string_pretty(&value).unwrap_or_else(|_| output.trim().to_string())
        );
    }
    format!("OpenClaw scheduler status:\n{}", output.trim())
}

fn summarize_cron_list(output: &str) -> String {
    if let Some(value) = parse_json_fragment(output) {
        return format!(
            "OpenClaw cron jobs:\n{}",
            serde_json::to_string_pretty(&value).unwrap_or_else(|_| output.trim().to_string())
        );
    }
    format!("OpenClaw cron jobs:\n{}", output.trim())
}

fn parse_json_fragment(output: &str) -> Option<Value> {
    serde_json::from_str::<Value>(output).ok().or_else(|| {
        output
            .lines()
            .find_map(|line| serde_json::from_str::<Value>(line).ok())
    }).or_else(|| {
        let trimmed = output.trim();
        let start = trimmed.find(|ch| ch == '{' || ch == '[')?;
        let end = trimmed.rfind(|ch| ch == '}' || ch == ']')?;
        if end <= start {
            return None;
        }
        serde_json::from_str::<Value>(&trimmed[start..=end]).ok()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_explicit_sessions_command() {
        let invocation = detect_invocation("/openclaw sessions").expect("should detect");
        assert_eq!(invocation.tool_id(), "sessions");
    }

    #[test]
    fn detects_browser_open_from_natural_prompt() {
        let invocation = detect_invocation("OpenClaw browser open https://example.com").expect("should detect");
        assert_eq!(invocation.tool_id(), "browser");
    }

    #[test]
    fn parses_json_fragment_with_extra_logs() {
        let parsed = parse_json_fragment("{\"count\":1}\nwarning").expect("json fragment");
        assert_eq!(parsed.get("count").and_then(|v| v.as_u64()), Some(1));
    }
}
