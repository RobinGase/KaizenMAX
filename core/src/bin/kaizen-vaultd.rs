//! Kaizen Vault Daemon
//!
//! Standalone secret vault service for multi-application local usage.
//! Secrets are app-scoped and encrypted at rest with the same SecretVault backend.

use axum::{
    body::Body,
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, HeaderName, HeaderValue, Request, StatusCode, header},
    middleware::{self, Next},
    response::Response,
    routing::{get, post, put},
};
use constant_time_eq::constant_time_eq;
use kaizen_gateway::vault::{SecretMetadata, SecretVault, VaultStatus};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::Mutex;
use tracing_subscriber::EnvFilter;

#[derive(Clone)]
struct VaultdState {
    vault: Arc<SecretVault>,
    status: VaultStatus,
    auth: Arc<AuthConfig>,
    auth_failures: Arc<Mutex<HashMap<String, AuthFailureWindow>>>,
}

#[derive(Clone, Debug)]
struct AuthConfig {
    admin_token: Option<String>,
    admin_cross_app_bypass: bool,
    app_tokens: HashMap<String, String>,
}

#[derive(Debug)]
struct AuthContext {
    app_id: String,
}

#[derive(Debug, Clone)]
struct AuthFailureWindow {
    started_at: Instant,
    attempts: u32,
    blocked_until: Option<Instant>,
}

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
    service: &'static str,
    vault_available: bool,
    key_source: String,
    key_path: Option<String>,
    app_token_count: usize,
    admin_token_enabled: bool,
    admin_cross_app_bypass: bool,
}

const AUTH_FAIL_WINDOW_SECS: u64 = 60;
const AUTH_FAIL_MAX_ATTEMPTS: u32 = 20;
const AUTH_BLOCK_SECS: u64 = 60;

#[derive(Debug, Deserialize)]
struct StoreSecretRequest {
    value: String,
    #[serde(default = "default_secret_type")]
    secret_type: String,
}

fn default_secret_type() -> String {
    "api_key".to_string()
}

#[derive(Debug, Serialize, Deserialize)]
struct SecretTestResult {
    provider: String,
    configured: bool,
    test_passed: bool,
    error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct SecretUseResponse {
    provider: String,
    key: String,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("info".parse().unwrap()))
        .init();

    let auth = match load_auth_config() {
        Ok(cfg) => cfg,
        Err(err) => {
            tracing::error!("Vault daemon auth configuration error: {}", err);
            std::process::exit(1);
        }
    };

    let (vault, vault_status) = match SecretVault::from_env_or_bootstrap() {
        Ok((vault, status)) => {
            tracing::info!(
                "Vault daemon initialized (source={}, path={})",
                status.key_source,
                status.vault_path
            );
            (vault, status)
        }
        Err(err) => {
            tracing::error!("Vault daemon failed to initialize vault: {}", err);
            std::process::exit(1);
        }
    };

    let host = std::env::var("KAIZEN_VAULTD_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
    let port = std::env::var("KAIZEN_VAULTD_PORT")
        .ok()
        .and_then(|v| v.parse::<u16>().ok())
        .unwrap_or(9210);

    let addr = format!("{}:{}", host, port);
    let listener = match tokio::net::TcpListener::bind(&addr).await {
        Ok(listener) => listener,
        Err(err) => {
            tracing::error!("Failed to bind Vault daemon at {}: {}", addr, err);
            std::process::exit(1);
        }
    };

    let state = VaultdState {
        vault: Arc::new(vault),
        status: vault_status,
        auth: Arc::new(auth),
        auth_failures: Arc::new(Mutex::new(HashMap::new())),
    };

    let app = Router::new()
        .route("/health", get(health))
        .route("/v1/secrets", get(list_secrets))
        .route(
            "/v1/secrets/{provider}",
            put(store_secret).delete(revoke_secret),
        )
        .route("/v1/secrets/{provider}/test", post(test_secret))
        .route("/v1/secrets/{provider}/use", get(use_secret))
        .layer(middleware::from_fn(security_headers))
        .with_state(state);

    tracing::info!("Kaizen Vault daemon listening on http://{}", addr);
    if let Err(err) = axum::serve(listener, app).await {
        tracing::error!("Vault daemon server error: {}", err);
    }
}

async fn health(State(state): State<VaultdState>) -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        service: "kaizen-vaultd",
        vault_available: state.status.available,
        key_source: state.status.key_source.clone(),
        key_path: state.status.key_path.clone(),
        app_token_count: state.auth.app_tokens.len(),
        admin_token_enabled: state.auth.admin_token.is_some(),
        admin_cross_app_bypass: state.auth.admin_cross_app_bypass,
    })
}

async fn security_headers(req: Request<Body>, next: Next) -> Response {
    let mut response = next.run(req).await;
    let headers = response.headers_mut();
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(header::PRAGMA, HeaderValue::from_static("no-cache"));
    headers.insert(
        HeaderName::from_static("x-content-type-options"),
        HeaderValue::from_static("nosniff"),
    );
    response
}

async fn list_secrets(
    State(state): State<VaultdState>,
    headers: HeaderMap,
) -> Result<Json<Vec<SecretMetadata>>, (StatusCode, String)> {
    let auth = authorize(&state, &headers).await?;
    let prefix = app_prefix(&auth.app_id);

    let mut scoped = Vec::new();
    for mut item in state.vault.list().await {
        if let Some(provider) = item.provider.strip_prefix(&prefix) {
            item.provider = provider.to_string();
            scoped.push(item);
        }
    }

    Ok(Json(scoped))
}

async fn store_secret(
    State(state): State<VaultdState>,
    headers: HeaderMap,
    Path(provider): Path<String>,
    Json(payload): Json<StoreSecretRequest>,
) -> Result<Json<SecretMetadata>, (StatusCode, String)> {
    let auth = authorize(&state, &headers).await?;
    let provider = normalize_provider(&provider)?;
    let scoped_provider = scoped_provider(&auth.app_id, &provider);

    let mut metadata = state
        .vault
        .store(&scoped_provider, &payload.value, &payload.secret_type)
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err))?;

    metadata.provider = provider;
    Ok(Json(metadata))
}

async fn revoke_secret(
    State(state): State<VaultdState>,
    headers: HeaderMap,
    Path(provider): Path<String>,
) -> Result<StatusCode, (StatusCode, String)> {
    let auth = authorize(&state, &headers).await?;
    let provider = normalize_provider(&provider)?;
    let scoped_provider = scoped_provider(&auth.app_id, &provider);

    state
        .vault
        .revoke(&scoped_provider)
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err))?;

    Ok(StatusCode::NO_CONTENT)
}

async fn test_secret(
    State(state): State<VaultdState>,
    headers: HeaderMap,
    Path(provider): Path<String>,
) -> Result<Json<SecretTestResult>, (StatusCode, String)> {
    let auth = authorize(&state, &headers).await?;
    let provider = normalize_provider(&provider)?;
    let scoped_provider = scoped_provider(&auth.app_id, &provider);

    if !state.vault.has(&scoped_provider).await {
        return Ok(Json(SecretTestResult {
            provider,
            configured: false,
            test_passed: false,
            error: Some("No credential stored for this provider".to_string()),
        }));
    }

    match state.vault.decrypt(&scoped_provider).await {
        Ok(_) => Ok(Json(SecretTestResult {
            provider,
            configured: true,
            test_passed: true,
            error: None,
        })),
        Err(err) => Ok(Json(SecretTestResult {
            provider,
            configured: true,
            test_passed: false,
            error: Some(err),
        })),
    }
}

async fn use_secret(
    State(state): State<VaultdState>,
    headers: HeaderMap,
    Path(provider): Path<String>,
) -> Result<Json<SecretUseResponse>, (StatusCode, String)> {
    let auth = authorize(&state, &headers).await?;
    let provider = normalize_provider(&provider)?;
    let scoped_provider = scoped_provider(&auth.app_id, &provider);

    if !state.vault.has(&scoped_provider).await {
        return Err((
            StatusCode::NOT_FOUND,
            format!("No credential stored for provider: {}", provider),
        ));
    }

    let key = state
        .vault
        .decrypt(&scoped_provider)
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err))?;

    Ok(Json(SecretUseResponse { provider, key }))
}

fn load_auth_config() -> Result<AuthConfig, String> {
    let admin_token = read_env_or_file("KAIZEN_VAULTD_ADMIN_TOKEN", "KAIZEN_VAULTD_ADMIN_TOKEN_FILE")?
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty());
    let admin_cross_app_bypass = std::env::var("KAIZEN_VAULTD_ADMIN_CROSS_APP_BYPASS")
        .ok()
        .map(|v| parse_bool(&v))
        .unwrap_or(false);

    let mut app_tokens = HashMap::new();
    if let Some(raw) =
        read_env_or_file("KAIZEN_VAULTD_APP_TOKENS", "KAIZEN_VAULTD_APP_TOKENS_FILE")?
    {
        for item in raw.split(',') {
            let pair = item.trim();
            if pair.is_empty() {
                continue;
            }

            let mut parts = pair.splitn(2, '=');
            let app = parts
                .next()
                .ok_or_else(|| "Invalid KAIZEN_VAULTD_APP_TOKENS format".to_string())?;
            let token = parts
                .next()
                .ok_or_else(|| "Invalid KAIZEN_VAULTD_APP_TOKENS format".to_string())?
                .trim();

            if token.is_empty() {
                return Err(format!(
                    "App token for '{}' is empty in KAIZEN_VAULTD_APP_TOKENS",
                    app
                ));
            }

            let app_id = normalize_app_id(app)?;
            app_tokens.insert(app_id, token.to_string());
        }
    }

    if app_tokens.is_empty() && !(admin_cross_app_bypass && admin_token.is_some()) {
        return Err(
            "Vault daemon requires app tokens. Set KAIZEN_VAULTD_APP_TOKENS (or *_FILE). Admin bypass-only mode requires KAIZEN_VAULTD_ADMIN_CROSS_APP_BYPASS=true and KAIZEN_VAULTD_ADMIN_TOKEN"
                .to_string(),
        );
    }

    Ok(AuthConfig {
        admin_token,
        admin_cross_app_bypass,
        app_tokens,
    })
}

async fn authorize(state: &VaultdState, headers: &HeaderMap) -> Result<AuthContext, (StatusCode, String)> {
    let app_id = extract_app_id(headers)?;
    let token = extract_token(headers)
        .ok_or((StatusCode::UNAUTHORIZED, "Missing vault auth token".to_string()))?;

    if auth_currently_blocked(state, &app_id).await {
        return Err((
            StatusCode::TOO_MANY_REQUESTS,
            "Too many failed auth attempts; try again later".to_string(),
        ));
    }

    if let Some(expected) = state.auth.app_tokens.get(&app_id) {
        if constant_time_eq(token.as_bytes(), expected.as_bytes()) {
            clear_auth_failures(state, &app_id).await;
            return Ok(AuthContext { app_id });
        }

        register_auth_failure(state, &app_id).await;
        return Err((StatusCode::FORBIDDEN, "Invalid vault token".to_string()));
    }

    if state.auth.admin_cross_app_bypass {
        if let Some(admin_token) = state.auth.admin_token.as_ref() {
            if constant_time_eq(token.as_bytes(), admin_token.as_bytes()) {
                clear_auth_failures(state, &app_id).await;
                return Ok(AuthContext { app_id });
            }
        }
    }

    register_auth_failure(state, &app_id).await;
    Err((
        StatusCode::FORBIDDEN,
        format!("App '{}' is not allowed", app_id),
    ))
}

async fn auth_currently_blocked(state: &VaultdState, app_id: &str) -> bool {
    let now = Instant::now();
    let mut map = state.auth_failures.lock().await;

    if let Some(window) = map.get_mut(app_id) {
        if let Some(until) = window.blocked_until {
            if until > now {
                return true;
            }

            window.blocked_until = None;
            window.started_at = now;
            window.attempts = 0;
        }
    }

    false
}

async fn register_auth_failure(state: &VaultdState, app_id: &str) {
    let now = Instant::now();
    let mut map = state.auth_failures.lock().await;
    let window = map
        .entry(app_id.to_string())
        .or_insert_with(|| AuthFailureWindow {
            started_at: now,
            attempts: 0,
            blocked_until: None,
        });

    if now.duration_since(window.started_at) > Duration::from_secs(AUTH_FAIL_WINDOW_SECS) {
        window.started_at = now;
        window.attempts = 0;
        window.blocked_until = None;
    }

    window.attempts = window.attempts.saturating_add(1);
    if window.attempts >= AUTH_FAIL_MAX_ATTEMPTS {
        window.blocked_until = Some(now + Duration::from_secs(AUTH_BLOCK_SECS));
    }
}

async fn clear_auth_failures(state: &VaultdState, app_id: &str) {
    let mut map = state.auth_failures.lock().await;
    map.remove(app_id);
}

fn read_env_or_file(env_key: &str, file_key: &str) -> Result<Option<String>, String> {
    if let Ok(path) = std::env::var(file_key) {
        let trimmed_path = path.trim();
        if !trimmed_path.is_empty() {
            let content = fs::read_to_string(trimmed_path).map_err(|err| {
                format!("Failed to read {} from '{}': {}", file_key, trimmed_path, err)
            })?;
            let trimmed = content.trim().to_string();
            if !trimmed.is_empty() {
                return Ok(Some(trimmed));
            }
        }
    }

    Ok(std::env::var(env_key)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty()))
}

fn parse_bool(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

fn extract_app_id(headers: &HeaderMap) -> Result<String, (StatusCode, String)> {
    let value = headers
        .get("x-vault-app")
        .ok_or((StatusCode::BAD_REQUEST, "Missing x-vault-app header".to_string()))?
        .to_str()
        .map_err(|_| {
            (
                StatusCode::BAD_REQUEST,
                "x-vault-app header is not valid UTF-8".to_string(),
            )
        })?;

    normalize_app_id(value).map_err(|err| (StatusCode::BAD_REQUEST, err))
}

fn extract_token(headers: &HeaderMap) -> Option<String> {
    if let Some(value) = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
    {
        let trimmed = value.trim();
        if let Some(rest) = trimmed.strip_prefix("Bearer ") {
            let token = rest.trim();
            if !token.is_empty() {
                return Some(token.to_string());
            }
        }
    }

    headers
        .get("x-vault-token")
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(ToString::to_string)
}

fn normalize_app_id(value: &str) -> Result<String, String> {
    let app = value.trim().to_ascii_lowercase();
    if app.is_empty() {
        return Err("App id cannot be empty".to_string());
    }

    if !app
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
    {
        return Err(
            "App id contains invalid characters; allowed: a-z 0-9 - _ .".to_string(),
        );
    }

    Ok(app)
}

fn normalize_provider(value: &str) -> Result<String, (StatusCode, String)> {
    let provider = value.trim().to_ascii_lowercase();
    if provider.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "Provider cannot be empty".to_string()));
    }

    if provider.contains('/') || provider.contains('\\') || provider.contains(':') {
        return Err((
            StatusCode::BAD_REQUEST,
            "Provider contains invalid characters".to_string(),
        ));
    }

    let canonical = match provider.as_str() {
        "zeroclaw" | "kai-zen" | "kaizen" => "kaizen".to_string(),
        _ => provider,
    };

    Ok(canonical)
}

fn app_prefix(app_id: &str) -> String {
    format!("app/{}/", app_id)
}

fn scoped_provider(app_id: &str, provider: &str) -> String {
    format!("{}{}", app_prefix(app_id), provider)
}
