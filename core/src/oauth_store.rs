use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::RngCore;
use reqwest::Url;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

pub const GOOGLE_PROJECT_ENV_HINTS: [&str; 3] = [
    "GOOGLE_CLOUD_PROJECT",
    "GOOGLE_PROJECT_ID",
    "GCLOUD_PROJECT",
];

const GOOGLE_AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const GOOGLE_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const GEMINI_OAUTH_STORE_ENV: &str = "KAIZEN_GEMINI_OAUTH_STORE_PATH";
const TOKEN_REFRESH_SKEW_SECS: u64 = 60;
const OAUTH_STATE_TTL_SECS: u64 = 10 * 60;
const GEMINI_SCOPES: [&str; 2] = [
    "https://www.googleapis.com/auth/cloud-platform",
    "https://www.googleapis.com/auth/generative-language.retriever",
];

#[derive(Debug, Clone)]
pub struct GeminiOAuthConfig {
    pub client_id: String,
    pub client_secret: Option<String>,
    pub project_id: String,
    pub redirect_uri: String,
}

#[derive(Debug, Clone)]
pub struct PendingGeminiOAuth {
    pub state_token: String,
    pub code_verifier: String,
    pub config: GeminiOAuthConfig,
    pub created_at_epoch_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeminiOAuthTokens {
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub expires_at_epoch_secs: Option<u64>,
    #[serde(default)]
    pub token_type: Option<String>,
    #[serde(default)]
    pub scope: Option<String>,
    pub project_id: String,
    pub updated_at_epoch_secs: u64,
}

#[derive(Debug, Clone)]
pub struct StoredGeminiOAuthStatus {
    pub present: bool,
    pub access_token_present: bool,
    pub access_token_ready: bool,
    pub refresh_token_present: bool,
    pub refresh_token_ready: bool,
    pub expires_at_epoch_secs: Option<u64>,
    pub project_id: Option<String>,
    pub message: String,
}

impl StoredGeminiOAuthStatus {
    pub fn connected(&self) -> bool {
        self.access_token_ready || self.refresh_token_ready
    }
}

#[derive(Debug, Deserialize)]
struct GoogleTokenResponse {
    access_token: String,
    #[serde(default)]
    expires_in: Option<u64>,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    scope: Option<String>,
    #[serde(default)]
    token_type: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GoogleTokenError {
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    error_description: Option<String>,
}

impl PendingGeminiOAuth {
    pub fn new(config: GeminiOAuthConfig) -> Self {
        Self {
            state_token: random_urlsafe_bytes(24),
            code_verifier: random_urlsafe_bytes(48),
            config,
            created_at_epoch_secs: now_epoch_secs(),
        }
    }

    pub fn is_stale(&self) -> bool {
        now_epoch_secs()
            > self
                .created_at_epoch_secs
                .saturating_add(OAUTH_STATE_TTL_SECS)
    }

    pub fn authorize_url(&self) -> Result<String, String> {
        let scope = GEMINI_SCOPES.join(" ");
        let code_challenge = pkce_code_challenge(&self.code_verifier);
        let url = Url::parse_with_params(
            GOOGLE_AUTH_URL,
            &[
                ("client_id", self.config.client_id.as_str()),
                ("redirect_uri", self.config.redirect_uri.as_str()),
                ("response_type", "code"),
                ("scope", scope.as_str()),
                ("access_type", "offline"),
                ("prompt", "consent"),
                ("include_granted_scopes", "true"),
                ("state", self.state_token.as_str()),
                ("code_challenge", code_challenge.as_str()),
                ("code_challenge_method", "S256"),
            ],
        )
        .map_err(|error| format!("Failed to build Gemini OAuth authorize URL: {error}"))?;

        Ok(url.to_string())
    }
}

impl GeminiOAuthTokens {
    fn access_token_present(&self) -> bool {
        !self.access_token.trim().is_empty()
    }

    fn refresh_token_present(&self) -> bool {
        self.refresh_token
            .as_deref()
            .map(|value| !value.trim().is_empty())
            .unwrap_or(false)
    }

    fn access_token_ready(&self) -> bool {
        self.access_token_present()
            && !token_expiring_within(self.expires_at_epoch_secs, TOKEN_REFRESH_SKEW_SECS)
    }
}

pub fn start_gemini_oauth(
    default_redirect_uri: String,
) -> Result<(PendingGeminiOAuth, String), String> {
    let config = resolve_gemini_oauth_config(default_redirect_uri)?;
    let pending = PendingGeminiOAuth::new(config);
    let redirect_url = pending.authorize_url()?;
    Ok((pending, redirect_url))
}

pub fn resolve_gemini_oauth_config(
    default_redirect_uri: String,
) -> Result<GeminiOAuthConfig, String> {
    let client_id = first_present_env(["KAIZEN_GEMINI_OAUTH_CLIENT_ID", "GOOGLE_OAUTH_CLIENT_ID"])
        .map(|(_, value)| value)
        .ok_or_else(|| {
            "Gemini OAuth needs GOOGLE_OAUTH_CLIENT_ID (or KAIZEN_GEMINI_OAUTH_CLIENT_ID)."
                .to_string()
        })?;

    let project_id = google_project_id_from_env().ok_or_else(|| {
        "Gemini OAuth needs GOOGLE_CLOUD_PROJECT (or GOOGLE_PROJECT_ID / GCLOUD_PROJECT)."
            .to_string()
    })?;

    let client_secret = first_present_env([
        "KAIZEN_GEMINI_OAUTH_CLIENT_SECRET",
        "GOOGLE_OAUTH_CLIENT_SECRET",
    ])
    .map(|(_, value)| value);

    let redirect_uri = std::env::var("KAIZEN_GEMINI_OAUTH_REDIRECT_URI")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or(default_redirect_uri);

    Ok(GeminiOAuthConfig {
        client_id,
        client_secret,
        project_id,
        redirect_uri,
    })
}

pub fn google_project_id_from_env() -> Option<String> {
    first_present_env(GOOGLE_PROJECT_ENV_HINTS).map(|(_, value)| value)
}

pub fn gemini_oauth_store_exists() -> bool {
    gemini_oauth_store_candidates()
        .into_iter()
        .any(|candidate| candidate.exists())
}

pub fn load_gemini_tokens() -> Result<Option<GeminiOAuthTokens>, String> {
    for candidate in gemini_oauth_store_candidates() {
        if !candidate.exists() {
            continue;
        }

        let text = fs::read_to_string(&candidate).map_err(|error| {
            format!(
                "Failed to read Gemini OAuth store '{}': {error}",
                candidate.display()
            )
        })?;
        let parsed = serde_json::from_str::<GeminiOAuthTokens>(&text).map_err(|error| {
            format!(
                "Failed to parse Gemini OAuth store '{}': {error}",
                candidate.display()
            )
        })?;
        return Ok(Some(parsed));
    }

    Ok(None)
}

pub fn stored_gemini_oauth_status() -> Result<StoredGeminiOAuthStatus, String> {
    let Some(tokens) = load_gemini_tokens()? else {
        return Ok(StoredGeminiOAuthStatus {
            present: false,
            access_token_present: false,
            access_token_ready: false,
            refresh_token_present: false,
            refresh_token_ready: false,
            expires_at_epoch_secs: None,
            project_id: None,
            message: "No app-managed Gemini OAuth tokens are stored.".to_string(),
        });
    };

    let access_token_present = tokens.access_token_present();
    let access_token_ready = tokens.access_token_ready();
    let refresh_token_present = tokens.refresh_token_present();
    let refresh_token_ready = refresh_token_present && gemini_oauth_client_id().is_some();
    let project_id = Some(tokens.project_id.clone());
    let message = if access_token_ready {
        format!(
            "App-managed Gemini OAuth is connected for Google project '{}'.",
            tokens.project_id
        )
    } else if refresh_token_ready {
        format!(
            "Stored Gemini OAuth token for project '{}' has expired and will refresh automatically.",
            tokens.project_id
        )
    } else if refresh_token_present {
        "Stored Gemini OAuth refresh token exists, but GOOGLE_OAUTH_CLIENT_ID (or KAIZEN_GEMINI_OAUTH_CLIENT_ID) is missing, so automatic refresh is unavailable.".to_string()
    } else {
        "Stored Gemini OAuth access token is expired and no refresh token is available. Disconnect and reconnect Gemini OAuth.".to_string()
    };

    Ok(StoredGeminiOAuthStatus {
        present: true,
        access_token_present,
        access_token_ready,
        refresh_token_present,
        refresh_token_ready,
        expires_at_epoch_secs: tokens.expires_at_epoch_secs,
        project_id,
        message,
    })
}

pub fn save_gemini_tokens(tokens: &GeminiOAuthTokens) -> Result<PathBuf, String> {
    let path = gemini_oauth_store_write_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            format!(
                "Failed to create Gemini OAuth store directory '{}': {error}",
                parent.display()
            )
        })?;
    }

    let json = serde_json::to_string_pretty(tokens)
        .map_err(|error| format!("Failed to encode Gemini OAuth store JSON: {error}"))?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, json).map_err(|error| {
        format!(
            "Failed to write Gemini OAuth tmp file '{}': {error}",
            tmp.display()
        )
    })?;
    fs::rename(&tmp, &path).map_err(|error| {
        format!(
            "Failed to move Gemini OAuth store into place '{}': {error}",
            path.display()
        )
    })?;

    Ok(path)
}

pub fn clear_gemini_tokens() -> Result<bool, String> {
    let mut removed_any = false;
    for candidate in gemini_oauth_store_candidates() {
        if !candidate.exists() {
            continue;
        }
        fs::remove_file(&candidate).map_err(|error| {
            format!(
                "Failed to remove Gemini OAuth store '{}': {error}",
                candidate.display()
            )
        })?;
        removed_any = true;
    }

    Ok(removed_any)
}

pub async fn exchange_gemini_code(
    pending: &PendingGeminiOAuth,
    code: &str,
) -> Result<GeminiOAuthTokens, String> {
    let code = code.trim();
    if code.is_empty() {
        return Err("Gemini OAuth callback did not include an authorization code.".to_string());
    }

    let mut form: Vec<(&str, String)> = vec![
        ("client_id", pending.config.client_id.clone()),
        ("code", code.to_string()),
        ("code_verifier", pending.code_verifier.clone()),
        ("grant_type", "authorization_code".to_string()),
        ("redirect_uri", pending.config.redirect_uri.clone()),
    ];
    if let Some(client_secret) = pending.config.client_secret.clone() {
        form.push(("client_secret", client_secret));
    }

    let response = google_token_request(form).await?;
    Ok(GeminiOAuthTokens {
        access_token: response.access_token.trim().to_string(),
        refresh_token: normalize_optional(response.refresh_token),
        expires_at_epoch_secs: response
            .expires_in
            .map(|ttl| now_epoch_secs().saturating_add(ttl)),
        token_type: normalize_optional(response.token_type),
        scope: normalize_optional(response.scope),
        project_id: pending.config.project_id.clone(),
        updated_at_epoch_secs: now_epoch_secs(),
    })
}

pub async fn refresh_stored_gemini_tokens() -> Result<GeminiOAuthTokens, String> {
    let tokens = load_gemini_tokens()?
        .ok_or_else(|| "No app-managed Gemini OAuth tokens are stored.".to_string())?;
    refresh_gemini_tokens(&tokens).await
}

pub async fn load_or_refresh_gemini_tokens() -> Result<Option<GeminiOAuthTokens>, String> {
    let Some(tokens) = load_gemini_tokens()? else {
        return Ok(None);
    };

    if tokens.access_token_ready() {
        return Ok(Some(tokens));
    }

    if tokens.refresh_token_present() {
        let refreshed = refresh_gemini_tokens(&tokens).await?;
        save_gemini_tokens(&refreshed)?;
        return Ok(Some(refreshed));
    }

    Err(
        "Stored Gemini OAuth access token is expired and no refresh token is available. Disconnect and reconnect Gemini OAuth.".to_string(),
    )
}

async fn refresh_gemini_tokens(existing: &GeminiOAuthTokens) -> Result<GeminiOAuthTokens, String> {
    let client_id = gemini_oauth_client_id().ok_or_else(|| {
        "Stored Gemini OAuth refresh token exists, but GOOGLE_OAUTH_CLIENT_ID (or KAIZEN_GEMINI_OAUTH_CLIENT_ID) is missing.".to_string()
    })?;

    let refresh_token = existing
        .refresh_token
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "Stored Gemini OAuth tokens do not include a refresh token.".to_string())?;

    let mut form: Vec<(&str, String)> = vec![
        ("client_id", client_id),
        ("grant_type", "refresh_token".to_string()),
        ("refresh_token", refresh_token.to_string()),
    ];
    if let Some(client_secret) = gemini_oauth_client_secret() {
        form.push(("client_secret", client_secret));
    }

    let response = google_token_request(form).await?;
    Ok(GeminiOAuthTokens {
        access_token: response.access_token.trim().to_string(),
        refresh_token: normalize_optional(response.refresh_token)
            .or_else(|| existing.refresh_token.clone()),
        expires_at_epoch_secs: response
            .expires_in
            .map(|ttl| now_epoch_secs().saturating_add(ttl)),
        token_type: normalize_optional(response.token_type).or_else(|| existing.token_type.clone()),
        scope: normalize_optional(response.scope).or_else(|| existing.scope.clone()),
        project_id: existing.project_id.clone(),
        updated_at_epoch_secs: now_epoch_secs(),
    })
}

async fn google_token_request(form: Vec<(&str, String)>) -> Result<GoogleTokenResponse, String> {
    let client = reqwest::Client::builder()
        .user_agent("kaizen-gateway/0.1.0")
        .build()
        .map_err(|error| format!("Failed to build Google OAuth HTTP client: {error}"))?;

    let response = client
        .post(GOOGLE_TOKEN_URL)
        .form(&form)
        .send()
        .await
        .map_err(|error| format!("Google OAuth token request failed: {error}"))?;

    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|error| format!("Failed to read Google OAuth token response: {error}"))?;

    if !status.is_success() {
        let trimmed = body.trim();
        if let Ok(parsed) = serde_json::from_str::<GoogleTokenError>(trimmed) {
            let code = parsed.error.unwrap_or_else(|| "unknown_error".to_string());
            let description = parsed
                .error_description
                .unwrap_or_else(|| "no error_description returned".to_string());
            return Err(format!(
                "Google OAuth token request failed ({}): {} - {}",
                status, code, description
            ));
        }

        return Err(format!(
            "Google OAuth token request failed ({}): {}",
            status,
            if trimmed.is_empty() {
                "empty response body".to_string()
            } else {
                trimmed.to_string()
            }
        ));
    }

    serde_json::from_str::<GoogleTokenResponse>(&body)
        .map_err(|error| format!("Failed to parse Google OAuth token response JSON: {error}"))
}

fn gemini_oauth_store_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    if let Ok(path) = std::env::var(GEMINI_OAUTH_STORE_ENV) {
        let trimmed = path.trim();
        if !trimmed.is_empty() {
            candidates.push(PathBuf::from(trimmed));
        }
    }

    candidates.push(PathBuf::from("../data/oauth/gemini_tokens.json"));
    candidates.push(PathBuf::from("data/oauth/gemini_tokens.json"));

    candidates
}

fn gemini_oauth_store_write_path() -> PathBuf {
    if let Ok(path) = std::env::var(GEMINI_OAUTH_STORE_ENV) {
        let trimmed = path.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }

    let workspace_path = PathBuf::from("../data/oauth/gemini_tokens.json");
    if workspace_path.exists() || path_parent_exists(workspace_path.as_path()) {
        return workspace_path;
    }

    let local_path = PathBuf::from("data/oauth/gemini_tokens.json");
    if local_path.exists() || path_parent_exists(local_path.as_path()) {
        return local_path;
    }

    workspace_path
}

fn path_parent_exists(path: &Path) -> bool {
    path.parent().map(|parent| parent.exists()).unwrap_or(false)
}

fn gemini_oauth_client_id() -> Option<String> {
    first_present_env(["KAIZEN_GEMINI_OAUTH_CLIENT_ID", "GOOGLE_OAUTH_CLIENT_ID"])
        .map(|(_, value)| value)
}

fn gemini_oauth_client_secret() -> Option<String> {
    first_present_env([
        "KAIZEN_GEMINI_OAUTH_CLIENT_SECRET",
        "GOOGLE_OAUTH_CLIENT_SECRET",
    ])
    .map(|(_, value)| value)
}

fn first_present_env<const N: usize>(keys: [&str; N]) -> Option<(String, String)> {
    for key in keys {
        if let Ok(value) = std::env::var(key) {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                return Some((key.to_string(), trimmed.to_string()));
            }
        }
    }
    None
}

fn token_expiring_within(expires_at_epoch_secs: Option<u64>, skew_secs: u64) -> bool {
    match expires_at_epoch_secs {
        Some(expires_at) => expires_at <= now_epoch_secs().saturating_add(skew_secs),
        None => false,
    }
}

fn normalize_optional(value: Option<String>) -> Option<String> {
    value
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
}

fn pkce_code_challenge(code_verifier: &str) -> String {
    let digest = Sha256::digest(code_verifier.as_bytes());
    URL_SAFE_NO_PAD.encode(digest)
}

fn random_urlsafe_bytes(size: usize) -> String {
    let mut bytes = vec![0_u8; size];
    rand::thread_rng().fill_bytes(bytes.as_mut_slice());
    URL_SAFE_NO_PAD.encode(bytes)
}

fn now_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use tempfile::TempDir;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn stored_status_is_empty_when_file_missing() {
        let _guard = ENV_LOCK.lock().expect("env lock");
        let previous = std::env::var(GEMINI_OAUTH_STORE_ENV).ok();
        unsafe {
            std::env::remove_var(GEMINI_OAUTH_STORE_ENV);
        }
        let status = stored_gemini_oauth_status().expect("status should load");
        assert!(!status.present);
        assert!(!status.connected());
        restore_env(previous);
    }

    #[test]
    fn save_and_load_round_trip() {
        let _guard = ENV_LOCK.lock().expect("env lock");
        let previous = std::env::var(GEMINI_OAUTH_STORE_ENV).ok();
        let temp = TempDir::new().expect("temp dir");
        let path = temp.path().join("gemini_tokens.json");
        unsafe {
            std::env::set_var(GEMINI_OAUTH_STORE_ENV, path.to_string_lossy().to_string());
        }

        let tokens = GeminiOAuthTokens {
            access_token: "access-token".to_string(),
            refresh_token: Some("refresh-token".to_string()),
            expires_at_epoch_secs: Some(now_epoch_secs().saturating_add(3600)),
            token_type: Some("Bearer".to_string()),
            scope: Some("scope-a scope-b".to_string()),
            project_id: "demo-project".to_string(),
            updated_at_epoch_secs: now_epoch_secs(),
        };

        save_gemini_tokens(&tokens).expect("save tokens");
        let loaded = load_gemini_tokens()
            .expect("load tokens")
            .expect("stored tokens");
        assert_eq!(loaded.project_id, "demo-project");
        assert_eq!(loaded.refresh_token.as_deref(), Some("refresh-token"));

        restore_env(previous);
    }

    #[test]
    fn pending_authorize_url_uses_pkce() {
        let pending = PendingGeminiOAuth::new(GeminiOAuthConfig {
            client_id: "client-id".to_string(),
            client_secret: None,
            project_id: "project".to_string(),
            redirect_uri: "http://127.0.0.1:9100/api/oauth/gemini/callback".to_string(),
        });

        let url = pending.authorize_url().expect("authorize url");
        assert!(url.contains("code_challenge="));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains("access_type=offline"));
    }

    fn restore_env(previous: Option<String>) {
        match previous {
            Some(value) => unsafe {
                std::env::set_var(GEMINI_OAUTH_STORE_ENV, value);
            },
            None => unsafe {
                std::env::remove_var(GEMINI_OAUTH_STORE_ENV);
            },
        }
    }
}
