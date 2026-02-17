//! Crystal Ball transport and redaction utilities.

use regex::Regex;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

const REDACTION_KEYS: [&str; 3] = [
    "OPENAI_API_KEY",
    "ANTHROPIC_API_KEY",
    "AWS_SECRET_ACCESS_KEY",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrystalBallEvent {
    pub event_id: String,
    pub timestamp: String,
    #[serde(rename = "type")]
    pub event_type: String,
    pub source_actor: String,
    pub source_agent_id: String,
    pub target_actor: String,
    pub target_agent_id: String,
    pub task_id: String,
    pub message: String,
    pub visibility: String,
}

#[derive(Debug, Clone)]
pub struct CrystalBallClient {
    base_url: String,
    token: String,
    channel_id: String,
    http: reqwest::Client,
}

#[derive(Debug, Deserialize)]
struct MattermostPostList {
    order: Vec<String>,
    posts: HashMap<String, MattermostPost>,
}

#[derive(Debug, Deserialize)]
struct MattermostPost {
    id: String,
    create_at: i64,
    user_id: String,
    message: String,
    #[serde(default)]
    props: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct MattermostUser {
    id: String,
    username: String,
}

#[derive(Debug, Deserialize)]
struct MattermostChannel {
    id: String,
    name: String,
    display_name: String,
}

#[derive(Debug, Deserialize)]
struct MattermostCreatedPost {
    id: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct MattermostValidation {
    pub reachable: bool,
    pub auth_ok: bool,
    pub channel_ok: bool,
    pub user_id: Option<String>,
    pub username: Option<String>,
    pub channel_id: String,
    pub channel_name: Option<String>,
    pub channel_display_name: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MattermostSmokeResult {
    pub sent: bool,
    pub fetched: bool,
    pub detected: bool,
    pub post_id: Option<String>,
    pub marker: String,
    pub error: Option<String>,
}

impl CrystalBallClient {
    pub fn from_env() -> Option<Self> {
        let base_url = std::env::var("MATTERMOST_URL").ok()?.trim().to_string();
        let token = std::env::var("MATTERMOST_TOKEN").ok()?.trim().to_string();
        let channel_id = std::env::var("MATTERMOST_CHANNEL_ID")
            .or_else(|_| std::env::var("CRYSTAL_BALL_CHANNEL"))
            .ok()?
            .trim()
            .to_string();

        if base_url.is_empty() || token.is_empty() || channel_id.is_empty() {
            return None;
        }

        Some(Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            token,
            channel_id,
            http: reqwest::Client::new(),
        })
    }

    pub async fn publish_event(&self, event: &CrystalBallEvent) -> Result<(), String> {
        let endpoint = format!("{}/api/v4/posts", self.base_url);
        let payload = json!({
            "channel_id": self.channel_id,
            "message": format!(
                "[{}] {} -> {} | {}",
                event.event_type, event.source_actor, event.target_actor, event.message
            ),
            "props": {
                "kaizen_event": true,
                "kaizen_event_payload": event,
            }
        });

        let response = self
            .http
            .post(endpoint)
            .header(AUTHORIZATION, format!("Bearer {}", self.token))
            .header(CONTENT_TYPE, "application/json")
            .json(&payload)
            .send()
            .await
            .map_err(|err| format!("Mattermost publish request failed: {err}"))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(format!("Mattermost publish failed ({status}): {body}"));
        }

        Ok(())
    }

    pub async fn fetch_recent_events(&self, limit: usize) -> Result<Vec<CrystalBallEvent>, String> {
        let per_page = limit.clamp(1, 200);
        let endpoint = format!(
            "{}/api/v4/channels/{}/posts?per_page={}",
            self.base_url, self.channel_id, per_page
        );

        let response = self
            .http
            .get(endpoint)
            .header(AUTHORIZATION, format!("Bearer {}", self.token))
            .send()
            .await
            .map_err(|err| format!("Mattermost fetch request failed: {err}"))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(format!("Mattermost fetch failed ({status}): {body}"));
        }

        let posts = response
            .json::<MattermostPostList>()
            .await
            .map_err(|err| format!("Failed to parse Mattermost posts: {err}"))?;

        let mut events = Vec::new();
        for post_id in posts.order {
            let Some(post) = posts.posts.get(&post_id) else {
                continue;
            };

            if let Some(payload) = post
                .props
                .as_ref()
                .and_then(|props| props.get("kaizen_event_payload"))
                .cloned()
            {
                if let Ok(event) = serde_json::from_value::<CrystalBallEvent>(payload) {
                    events.push(event);
                    continue;
                }
            }

            let flagged = post
                .props
                .as_ref()
                .and_then(|props| props.get("kaizen_event"))
                .and_then(|value| value.as_bool())
                .unwrap_or(false);

            if flagged {
                events.push(CrystalBallEvent {
                    event_id: post.id.clone(),
                    timestamp: format!("{:.3}", post.create_at as f64 / 1000.0),
                    event_type: "external".to_string(),
                    source_actor: post.user_id.clone(),
                    source_agent_id: post.user_id.clone(),
                    target_actor: "crystal_ball".to_string(),
                    target_agent_id: self.channel_id.clone(),
                    task_id: "mattermost".to_string(),
                    message: redact_sensitive(post.message.as_str()),
                    visibility: "operator".to_string(),
                });
            }
        }

        Ok(events)
    }

    pub async fn validate_connection(&self) -> MattermostValidation {
        let mut report = MattermostValidation {
            reachable: false,
            auth_ok: false,
            channel_ok: false,
            user_id: None,
            username: None,
            channel_id: self.channel_id.clone(),
            channel_name: None,
            channel_display_name: None,
            error: None,
        };

        let ping_endpoint = format!("{}/api/v4/system/ping", self.base_url);
        match self.http.get(ping_endpoint).send().await {
            Ok(response) if response.status().is_success() => {
                report.reachable = true;
            }
            Ok(response) => {
                report.error = Some(format!("Mattermost ping failed ({})", response.status()));
                return report;
            }
            Err(err) => {
                report.error = Some(format!("Mattermost ping request failed: {err}"));
                return report;
            }
        }

        let me_endpoint = format!("{}/api/v4/users/me", self.base_url);
        match self
            .http
            .get(me_endpoint)
            .header(AUTHORIZATION, format!("Bearer {}", self.token))
            .send()
            .await
        {
            Ok(response) if response.status().is_success() => {
                match response.json::<MattermostUser>().await {
                    Ok(user) => {
                        report.auth_ok = true;
                        report.user_id = Some(user.id);
                        report.username = Some(user.username);
                    }
                    Err(err) => {
                        report.error =
                            Some(format!("Failed to parse Mattermost user response: {err}"));
                        return report;
                    }
                }
            }
            Ok(response) => {
                report.error = Some(format!("Mattermost auth failed ({})", response.status()));
                return report;
            }
            Err(err) => {
                report.error = Some(format!("Mattermost auth request failed: {err}"));
                return report;
            }
        }

        let channel_endpoint = format!("{}/api/v4/channels/{}", self.base_url, self.channel_id);
        match self
            .http
            .get(channel_endpoint)
            .header(AUTHORIZATION, format!("Bearer {}", self.token))
            .send()
            .await
        {
            Ok(response) if response.status().is_success() => {
                match response.json::<MattermostChannel>().await {
                    Ok(channel) => {
                        report.channel_ok = true;
                        report.channel_name = Some(channel.name);
                        report.channel_display_name = Some(channel.display_name);
                        if channel.id != self.channel_id {
                            report.error =
                                Some("Mattermost channel ID mismatch in response".to_string());
                        }
                    }
                    Err(err) => {
                        report.error = Some(format!(
                            "Failed to parse Mattermost channel response: {err}"
                        ));
                    }
                }
            }
            Ok(response) => {
                report.error = Some(format!(
                    "Mattermost channel check failed ({})",
                    response.status()
                ));
            }
            Err(err) => {
                report.error = Some(format!("Mattermost channel request failed: {err}"));
            }
        }

        report
    }

    pub async fn run_smoke_test(&self) -> MattermostSmokeResult {
        let marker = format!(
            "kmax-smoke-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
        );

        let mut result = MattermostSmokeResult {
            sent: false,
            fetched: false,
            detected: false,
            post_id: None,
            marker: marker.clone(),
            error: None,
        };

        let endpoint = format!("{}/api/v4/posts", self.base_url);
        let payload = json!({
            "channel_id": self.channel_id,
            "message": format!("[KaizenMAX Smoke] marker={}", marker),
            "props": {
                "kaizen_smoke": true,
                "kaizen_smoke_marker": marker,
            }
        });

        let post_response = match self
            .http
            .post(endpoint)
            .header(AUTHORIZATION, format!("Bearer {}", self.token))
            .header(CONTENT_TYPE, "application/json")
            .json(&payload)
            .send()
            .await
        {
            Ok(response) => response,
            Err(err) => {
                result.error = Some(format!("Smoke publish request failed: {err}"));
                return result;
            }
        };

        if !post_response.status().is_success() {
            let status = post_response.status();
            let body = post_response.text().await.unwrap_or_default();
            result.error = Some(format!("Smoke publish failed ({status}): {body}"));
            return result;
        }

        result.sent = true;
        match post_response.json::<MattermostCreatedPost>().await {
            Ok(created) => {
                result.post_id = Some(created.id.clone());
            }
            Err(err) => {
                result.error = Some(format!("Smoke publish response parse failed: {err}"));
                return result;
            }
        }

        let fetch_endpoint = format!(
            "{}/api/v4/channels/{}/posts?per_page={}",
            self.base_url, self.channel_id, 60
        );
        let fetch_response = match self
            .http
            .get(fetch_endpoint)
            .header(AUTHORIZATION, format!("Bearer {}", self.token))
            .send()
            .await
        {
            Ok(response) => response,
            Err(err) => {
                result.error = Some(format!("Smoke fetch request failed: {err}"));
                return result;
            }
        };

        if !fetch_response.status().is_success() {
            let status = fetch_response.status();
            let body = fetch_response.text().await.unwrap_or_default();
            result.error = Some(format!("Smoke fetch failed ({status}): {body}"));
            return result;
        }

        result.fetched = true;
        let posts = match fetch_response.json::<MattermostPostList>().await {
            Ok(posts) => posts,
            Err(err) => {
                result.error = Some(format!("Smoke fetch parse failed: {err}"));
                return result;
            }
        };

        let found = posts.posts.values().any(|post| {
            if let Some(expected_id) = result.post_id.as_ref() {
                if post.id == *expected_id {
                    return true;
                }
            }

            post.props
                .as_ref()
                .and_then(|props| props.get("kaizen_smoke_marker"))
                .and_then(|value| value.as_str())
                .map(|value| value == marker)
                .unwrap_or(false)
        });

        result.detected = found;
        if !found {
            result.error =
                Some("Smoke post was created but not detected in recent posts".to_string());
        }

        result
    }
}

pub fn redact_sensitive(message: &str) -> String {
    let mut masked = message.to_string();

    for key in REDACTION_KEYS {
        masked = masked.replace(key, "[REDACTED]");
    }

    let admin_regex = Regex::new(r"ADMIN_[A-Z0-9_]+").expect("valid admin regex");
    admin_regex
        .replace_all(masked.as_str(), "[REDACTED_ADMIN]")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_known_keys() {
        let input = "token OPENAI_API_KEY=abc and ANTHROPIC_API_KEY=def";
        let output = redact_sensitive(input);
        assert!(!output.contains("OPENAI_API_KEY"));
        assert!(!output.contains("ANTHROPIC_API_KEY"));
        assert!(output.contains("[REDACTED]"));
    }

    #[test]
    fn redacts_admin_prefix_values() {
        let input = "ADMIN_HARD_GATES_ENABLED=true";
        let output = redact_sensitive(input);
        assert!(output.contains("[REDACTED_ADMIN]"));
        assert!(!output.contains("ADMIN_HARD_GATES_ENABLED"));
    }
}
