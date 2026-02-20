//! ZeroClaw Gateway - Kaizen MAX core runtime
//!
//! This is the Rust-native gateway that handles:
//! - Agent lifecycle management
//! - Orchestration state machine (hard gates)
//! - MCP tool routing
//! - Provider inference API proxying

use axum::{
    Json, Router,
    body::{Body, to_bytes},
    extract::{Path, Query, State},
    http::{HeaderMap, HeaderName, HeaderValue, StatusCode, Uri},
    middleware::{self, Next},
    response::Response,
    response::sse::{Event, KeepAlive, Sse},
    routing::{delete, get, patch, post, put},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use futures_util::Stream;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    cmp::Ordering,
    collections::{HashMap, HashSet},
    convert::Infallible,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering as AtomicOrdering},
    },
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::process::Command;
use tokio::sync::RwLock;
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use tracing_subscriber::EnvFilter;
use zeroclaw_gateway::{
    agents::{AgentRegistry, AgentStatus, SubAgent},
    crystal_ball::{
        CrystalBallClient, CrystalBallConfig, CrystalBallEvent, MattermostSmokeResult,
        MattermostValidation, redact_sensitive,
    },
    event_archive::{ArchiveIntegrityReport, EventArchive},
    gate_engine::{GateConditionPatch, GateRuntime, GateState, TransitionResult},
    inference::{
        self, AnthropicStreamEvent, ChatMessage as InferenceChatMessage, InferenceClient,
        InferenceProvider, InferenceRequest, OpenAIStreamChunk,
    },
    settings::{KaizenSettings, SettingsPatch},
    vault::{SecretMetadata, SecretVault, VaultStatus},
    webkeys::{
        VirtualKeyCreationResult, VirtualKeyPublicRecord, WebKeysService,
        WebProviderBindingPublicRecord, WebProviderType, auth::authenticate_bearer,
        browser::BrowserManager as WebkeysBrowserManager, gemini_executor::GeminiExecutor,
        runtime::WebkeysRuntime, session_health::SessionHealth,
    },
};

const LOCAL_EVENT_RETENTION_SECS: f64 = 72.0 * 3600.0;
const MAX_LOCAL_EVENTS: usize = 1000;

const GOOGLE_OAUTH_ACCOUNTS_VAULT_KEY: &str = "webkeys_google_oauth_accounts";
const GOOGLE_OAUTH_CLIENT_ID_VAULT_KEY: &str = "google_oauth_client_id";
const GOOGLE_OAUTH_CLIENT_SECRET_VAULT_KEY: &str = "google_oauth_client_secret";
const GOOGLE_OAUTH_STATE_TTL_SECS: i64 = 15 * 60;

#[derive(Debug, Clone)]
struct PendingGoogleOAuthSession {
    code_verifier: String,
    redirect_uri: String,
    created_at_unix: i64,
}

#[derive(Clone)]
struct AppState {
    settings: Arc<RwLock<KaizenSettings>>,
    admin_api_token: Arc<Option<String>>,
    agents: Arc<RwLock<AgentRegistry>>,
    gates: Arc<RwLock<GateRuntime>>,
    events: Arc<RwLock<Vec<CrystalBallEvent>>>,
    crystal_ball: Arc<RwLock<Option<CrystalBallClient>>>,
    event_archive: Arc<EventArchive>,
    vault: Arc<Option<SecretVault>>,
    vault_status: Arc<RwLock<VaultStatus>>,
    inference: InferenceClient,
    system_prompt: Arc<String>,
    /// Per-session conversation history (keyed by "kaizen" or agent_id).
    conversations: Arc<RwLock<HashMap<String, Vec<InferenceChatMessage>>>>,
    /// Monotonic generation counters for conversation keys.
    ///
    /// Any clear/remove operation bumps the generation so stale in-flight
    /// inference responses cannot repopulate cleared histories.
    conversation_versions: Arc<RwLock<HashMap<String, u64>>>,
    next_id: Arc<AtomicU64>,
    /// WebKeys service for virtual key management
    webkeys: Option<WebKeysService>,
    /// Runtime that routes verified requests to the correct browser/API executor
    webkeys_runtime: Option<std::sync::Arc<WebkeysRuntime>>,
    /// Short-lived PKCE state for Google OAuth flow (state_token -> verifier/session metadata)
    google_oauth_pending: Arc<RwLock<HashMap<String, PendingGoogleOAuthSession>>>,
}

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
    engine: String,
    version: &'static str,
}

#[derive(Serialize)]
struct CrystalBallHealthResponse {
    enabled: bool,
    mode: String,
    mattermost_configured: bool,
    mattermost_connected: bool,
    local_archive_path: String,
    local_archive_ttl_days: f64,
    local_event_count: usize,
    archive_integrity_valid: bool,
    archive_hmac_configured: bool,
    archive_signed_records: usize,
    archive_legacy_unsigned_records: usize,
    archive_mac_verified_records: usize,
    archive_mac_missing_records: usize,
    archive_mac_unverified_records: usize,
    archive_last_hash: String,
}

#[derive(Serialize)]
struct CrystalBallValidateResponse {
    enabled: bool,
    configured: bool,
    validation: Option<MattermostValidation>,
    error: Option<String>,
}

#[derive(Serialize)]
struct CrystalBallSmokeResponse {
    enabled: bool,
    configured: bool,
    success: bool,
    smoke: Option<MattermostSmokeResult>,
    error: Option<String>,
}

fn archive_integrity_fallback(reason: &str) -> ArchiveIntegrityReport {
    ArchiveIntegrityReport {
        valid: false,
        total_records: 0,
        signed_records: 0,
        legacy_unsigned_records: 0,
        hmac_configured: false,
        mac_verified_records: 0,
        mac_missing_records: 0,
        mac_unverified_records: 0,
        first_invalid_line: None,
        reason: Some(reason.to_string()),
        last_hash: "GENESIS".to_string(),
    }
}

async fn read_archive_integrity(archive: Arc<EventArchive>) -> ArchiveIntegrityReport {
    match tokio::task::spawn_blocking(move || archive.verify_integrity()).await {
        Ok(Ok(report)) => report,
        Ok(Err(err)) => archive_integrity_fallback(err.as_str()),
        Err(err) => archive_integrity_fallback(format!("Archive check join error: {err}").as_str()),
    }
}

async fn health(State(state): State<AppState>) -> Json<HealthResponse> {
    let settings = state.settings.read().await;
    Json(HealthResponse {
        status: "ok",
        engine: settings.runtime_engine.clone(),
        version: env!("CARGO_PKG_VERSION"),
    })
}

async fn crystal_ball_health(State(state): State<AppState>) -> Json<CrystalBallHealthResponse> {
    let settings = state.settings.read().await.clone();
    let local_event_count = state.events.read().await.len();
    let integrity = read_archive_integrity(state.event_archive.clone()).await;

    let client = state.crystal_ball.read().await.clone();
    let mattermost_configured = client.is_some();
    let mattermost_connected = if settings.crystal_ball_enabled {
        if let Some(client) = client {
            client.fetch_recent_events(1).await.is_ok()
        } else {
            false
        }
    } else {
        false
    };

    Json(CrystalBallHealthResponse {
        enabled: settings.crystal_ball_enabled,
        mode: if mattermost_configured {
            "mattermost+local".to_string()
        } else {
            "local".to_string()
        },
        mattermost_configured,
        mattermost_connected,
        local_archive_path: state.event_archive.path().display().to_string(),
        local_archive_ttl_days: state.event_archive.archive_ttl_days(),
        local_event_count,
        archive_integrity_valid: integrity.valid,
        archive_hmac_configured: integrity.hmac_configured,
        archive_signed_records: integrity.signed_records,
        archive_legacy_unsigned_records: integrity.legacy_unsigned_records,
        archive_mac_verified_records: integrity.mac_verified_records,
        archive_mac_missing_records: integrity.mac_missing_records,
        archive_mac_unverified_records: integrity.mac_unverified_records,
        archive_last_hash: integrity.last_hash,
    })
}

async fn crystal_ball_audit(State(state): State<AppState>) -> Json<ArchiveIntegrityReport> {
    Json(read_archive_integrity(state.event_archive.clone()).await)
}

async fn crystal_ball_validate(State(state): State<AppState>) -> Json<CrystalBallValidateResponse> {
    let settings = state.settings.read().await.clone();
    let enabled = settings.crystal_ball_enabled;
    let client = state.crystal_ball.read().await.clone();

    if !enabled {
        return Json(CrystalBallValidateResponse {
            enabled,
            configured: client.is_some(),
            validation: None,
            error: Some("Crystal Ball is disabled in settings".to_string()),
        });
    }

    let Some(client) = client else {
        return Json(CrystalBallValidateResponse {
            enabled,
            configured: false,
            validation: None,
            error: Some("Mattermost client is not configured".to_string()),
        });
    };

    let validation = client.validate_connection().await;
    let ok = validation.reachable && validation.auth_ok && validation.channel_ok;
    Json(CrystalBallValidateResponse {
        enabled,
        configured: true,
        validation: Some(validation),
        error: if ok {
            None
        } else {
            Some("Mattermost validation failed".to_string())
        },
    })
}

async fn crystal_ball_smoke(State(state): State<AppState>) -> Json<CrystalBallSmokeResponse> {
    let settings = state.settings.read().await.clone();
    let enabled = settings.crystal_ball_enabled;
    let client = state.crystal_ball.read().await.clone();

    if !enabled {
        return Json(CrystalBallSmokeResponse {
            enabled,
            configured: client.is_some(),
            success: false,
            smoke: None,
            error: Some("Crystal Ball is disabled in settings".to_string()),
        });
    }

    let Some(client) = client else {
        return Json(CrystalBallSmokeResponse {
            enabled,
            configured: false,
            success: false,
            smoke: None,
            error: Some("Mattermost client is not configured".to_string()),
        });
    };

    let smoke = client.run_smoke_test().await;
    let success = smoke.sent && smoke.fetched && smoke.detected;

    if success {
        push_event(
            &state,
            CrystalBallEvent {
                event_id: next_id(&state, "event"),
                timestamp: now_timestamp(),
                event_type: "crystal_ball.smoke".to_string(),
                source_actor: "Kaizen".to_string(),
                source_agent_id: "kaizen".to_string(),
                target_actor: "crystal_ball".to_string(),
                target_agent_id: "mattermost".to_string(),
                task_id: "smoke".to_string(),
                message: format!("Mattermost smoke succeeded ({})", smoke.marker),
                visibility: "operator".to_string(),
            },
        )
        .await;
    }

    Json(CrystalBallSmokeResponse {
        enabled,
        configured: true,
        success,
        error: if success {
            None
        } else {
            Some(
                smoke
                    .error
                    .clone()
                    .unwrap_or_else(|| "Mattermost smoke test failed".to_string()),
            )
        },
        smoke: Some(smoke),
    })
}

fn parse_timestamp_seconds(value: &str) -> Option<f64> {
    value.parse::<f64>().ok()
}

fn now_timestamp() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}.{:03}", now.as_secs(), now.subsec_millis())
}

fn next_id(state: &AppState, prefix: &str) -> String {
    let id = state.next_id.fetch_add(1, AtomicOrdering::Relaxed);
    format!("{prefix}-{id}")
}

fn should_compact_archive(event_id: &str) -> bool {
    event_id
        .rsplit('-')
        .next()
        .and_then(|value| value.parse::<u64>().ok())
        .map(|value| value % 200 == 0)
        .unwrap_or(false)
}

fn prune_events(events: &mut Vec<CrystalBallEvent>) {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64();

    events.retain(|event| {
        parse_timestamp_seconds(event.timestamp.as_str())
            .map(|ts| (now - ts) <= LOCAL_EVENT_RETENTION_SECS)
            .unwrap_or(true)
    });

    if events.len() > MAX_LOCAL_EVENTS {
        let overflow = events.len() - MAX_LOCAL_EVENTS;
        events.drain(0..overflow);
    }
}

async fn push_event(state: &AppState, mut event: CrystalBallEvent) {
    event.message = redact_sensitive(event.message.as_str());

    let should_compact = should_compact_archive(event.event_id.as_str());
    let archive = state.event_archive.clone();
    let archive_event = event.clone();
    tokio::task::spawn_blocking(move || {
        if let Err(err) = archive.append(&archive_event) {
            tracing::warn!("Failed to append Crystal Ball archive event: {}", err);
            return;
        }

        if should_compact {
            if let Err(err) = archive.compact() {
                tracing::warn!("Failed to compact Crystal Ball archive: {}", err);
            }
        }
    });

    let mut events = state.events.write().await;
    events.push(event.clone());
    prune_events(&mut events);
    drop(events);

    let crystal_ball_enabled = state.settings.read().await.crystal_ball_enabled;
    if !crystal_ball_enabled {
        return;
    }

    let crystal_ball = state.crystal_ball.read().await.clone();
    if let Some(client) = crystal_ball {
        tokio::spawn(async move {
            if let Err(err) = client.publish_event(&event).await {
                tracing::warn!("Crystal Ball Mattermost publish failed: {}", err);
            }
        });
    }
}

async fn build_crystal_ball_client(
    settings: &KaizenSettings,
    vault: Option<&SecretVault>,
) -> Option<CrystalBallClient> {
    if !settings.crystal_ball_enabled {
        return None;
    }

    let base_url = settings.mattermost_url.trim();
    let channel_id = settings.mattermost_channel_id.trim();

    if !base_url.is_empty() && !channel_id.is_empty() {
        if let Some(vault) = vault {
            match vault.decrypt("mattermost").await {
                Ok(token) => {
                    let config = CrystalBallConfig {
                        base_url: base_url.to_string(),
                        token,
                        channel_id: channel_id.to_string(),
                    };
                    if let Some(client) = CrystalBallClient::from_config(config) {
                        return Some(client);
                    }
                    tracing::warn!(
                        "Mattermost settings are present but Crystal Ball config is invalid."
                    );
                }
                Err(err) => {
                    tracing::warn!(
                        "Mattermost settings are present but no token is stored in vault provider 'mattermost': {}",
                        err
                    );
                }
            }
        } else {
            tracing::warn!(
                "Mattermost settings are present but vault is unavailable for token decryption."
            );
        }
    }

    CrystalBallClient::from_env()
}

async fn refresh_crystal_ball_client(state: &AppState) {
    let settings = state.settings.read().await.clone();
    let new_client = build_crystal_ball_client(&settings, state.vault.as_ref().as_ref()).await;
    let client_available = new_client.is_some();

    {
        let mut crystal_ball = state.crystal_ball.write().await;
        *crystal_ball = new_client;
    }

    if settings.crystal_ball_enabled && !client_available {
        tracing::warn!(
            "Crystal Ball enabled but Mattermost client is not configured. Running local feed only."
        );
    }
}

fn extract_presented_admin_token(headers: &HeaderMap) -> Option<String> {
    if let Some(value) = headers.get("x-admin-token") {
        if let Ok(value) = value.to_str() {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }

    let auth = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let auth = auth.trim();
    if auth.to_lowercase().starts_with("bearer ") {
        let token = auth[7..].trim();
        if !token.is_empty() {
            return Some(token.to_string());
        }
    }

    None
}

fn require_admin_access(
    state: &AppState,
    headers: &HeaderMap,
    action: &str,
) -> Result<(), (StatusCode, String)> {
    validate_admin_access(
        state.admin_api_token.as_ref().as_ref().map(|s| s.as_str()),
        headers,
        action,
    )
    .map_err(|msg| (StatusCode::UNAUTHORIZED, msg))
}

/// HTTP-ready version: takes the token directly (pre-extracted from AppState).
fn validate_admin_access_http(
    expected_token: Option<&str>,
    headers: &HeaderMap,
    action: &str,
) -> Result<(), (StatusCode, String)> {
    validate_admin_access(expected_token, headers, action)
        .map_err(|msg| (StatusCode::UNAUTHORIZED, msg))
}

fn validate_admin_access(
    expected_token: Option<&str>,
    headers: &HeaderMap,
    action: &str,
) -> Result<(), String> {
    let Some(expected_token) = expected_token else {
        // No token configured: keep local-default behavior for developer workflows.
        return Ok(());
    };

    let provided = extract_presented_admin_token(headers).ok_or_else(|| {
        format!(
            "Admin token required for {action}. Provide `Authorization: Bearer <token>` or `x-admin-token`."
        )
    })?;

    if provided != expected_token {
        return Err(format!("Invalid admin token for {action}."));
    }

    Ok(())
}

fn sanitize_uri_for_log(uri: &Uri) -> String {
    let path = uri.path();
    let query = uri.query().unwrap_or_default();

    if query.is_empty() {
        return redact_sensitive(path);
    }

    redact_sensitive(format!("{}?{}", path, query).as_str())
}

fn wipe_string(secret: &mut String) {
    // Best-effort in-memory wipe for short-lived plaintext material.
    unsafe {
        secret.as_bytes_mut().fill(0);
    }
    secret.clear();
}

fn is_loopback_host(host: &str) -> bool {
    let normalized = host.trim().to_lowercase();
    if normalized == "localhost" {
        return true;
    }

    if let Ok(ip) = normalized.parse::<std::net::IpAddr>() {
        return ip.is_loopback();
    }

    false
}

fn resolve_bind_mode() -> String {
    std::env::var("ZEROCLAW_MODE")
        .unwrap_or_else(|_| "native".to_string())
        .to_lowercase()
}

fn enforce_network_policy(mode: &str, host: &str) -> Result<(), String> {
    match mode {
        "native" | "local" => {
            if !is_loopback_host(host) {
                return Err(format!(
                    "ZEROCLAW_MODE={mode} requires loopback host. Got ZEROCLAW_HOST={host}."
                ));
            }
            Ok(())
        }
        "remote" => {
            let ack = std::env::var("ZEROCLAW_REMOTE_SECURITY_ACK").unwrap_or_default();
            if ack != "I_UNDERSTAND_REMOTE_REQUIRES_TLS_MTLS_AUTH" {
                return Err(
                    "ZEROCLAW_MODE=remote requires ZEROCLAW_REMOTE_SECURITY_ACK=I_UNDERSTAND_REMOTE_REQUIRES_TLS_MTLS_AUTH"
                        .to_string(),
                );
            }

            tracing::warn!(
                "Remote mode enabled. You must enforce TLS/mTLS/auth at the edge (reverse proxy or service mesh)."
            );
            Ok(())
        }
        other => Err(format!(
            "Unsupported ZEROCLAW_MODE={other}. Use 'native' or 'remote'."
        )),
    }
}

fn parse_cors_origins(csv: &str) -> Result<Vec<HeaderValue>, String> {
    let mut origins = Vec::new();
    for raw in csv.split(',') {
        let origin = raw.trim();
        if origin.is_empty() {
            continue;
        }

        if !(origin.starts_with("http://") || origin.starts_with("https://")) {
            return Err(format!(
                "Invalid CORS origin '{origin}'. Origins must start with http:// or https://"
            ));
        }

        origins.push(
            origin
                .parse::<HeaderValue>()
                .map_err(|e| format!("Invalid CORS origin '{origin}': {e}"))?,
        );
    }

    if origins.is_empty() {
        return Err("KAIZEN_CORS_ORIGINS must include at least one origin".to_string());
    }

    Ok(origins)
}

async fn redact_error_response_middleware(request: axum::extract::Request, next: Next) -> Response {
    let response = next.run(request).await;
    if response.status().is_success() {
        return response;
    }

    let status = response.status();
    let headers_snapshot = response.headers().clone();
    let (parts, body) = response.into_parts();

    let bytes = to_bytes(body, 1024 * 1024).await.unwrap_or_default();
    let raw_text = String::from_utf8_lossy(&bytes);
    let redacted_text = redact_sensitive(raw_text.as_ref());

    let mut rebuilt = Response::from_parts(parts, Body::from(redacted_text));
    *rebuilt.status_mut() = status;
    *rebuilt.headers_mut() = headers_snapshot;
    rebuilt
        .headers_mut()
        .remove(axum::http::header::CONTENT_LENGTH);
    rebuilt
}

async fn get_settings(State(state): State<AppState>) -> Json<KaizenSettings> {
    Json(state.settings.read().await.clone())
}

async fn patch_settings(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(patch): Json<SettingsPatch>,
) -> Result<Json<KaizenSettings>, (StatusCode, String)> {
    require_admin_access(&state, &headers, "PATCH /api/settings")?;

    {
        let mut settings = state.settings.write().await;
        settings.apply_patch(patch);
        if settings.max_subagents > 20 {
            return Err((
                StatusCode::BAD_REQUEST,
                "max_subagents must be <= 20".to_string(),
            ));
        }

        {
            let mut registry = state.agents.write().await;
            registry.set_max_subagents(settings.max_subagents as usize);
        }

        let persisted_path = settings
            .persist_to_workspace()
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
        tracing::info!("Persisted runtime settings to {}", persisted_path.display());
    };

    refresh_crystal_ball_client(&state).await;

    {
        let mut events = state.events.write().await;
        prune_events(&mut events);
    }

    push_event(
        &state,
        CrystalBallEvent {
            event_id: next_id(&state, "event"),
            timestamp: now_timestamp(),
            event_type: "orchestration.started".to_string(),
            source_actor: "Kaizen".to_string(),
            source_agent_id: "kaizen".to_string(),
            target_actor: "operator".to_string(),
            target_agent_id: "human".to_string(),
            task_id: "settings".to_string(),
            message: "Runtime settings updated via API".to_string(),
            visibility: "operator".to_string(),
        },
    )
    .await;

    Ok(Json(state.settings.read().await.clone()))
}

#[derive(Debug, Deserialize)]
struct ChatModelTarget {
    provider: String,
    model: String,
}

#[derive(Debug, Deserialize)]
struct ChatRequest {
    message: String,
    agent_id: Option<String>,
    /// If true, clear conversation history before this message.
    #[serde(default)]
    clear_history: bool,
    /// Optional one-off provider override for this message.
    #[serde(default)]
    provider: Option<String>,
    /// Optional one-off model override for this message.
    #[serde(default)]
    model: Option<String>,
    /// Optional execution mode hint for this message.
    #[serde(default)]
    mode: Option<String>,
    /// Optional multi-model fanout targets.
    #[serde(default)]
    selected_models: Option<Vec<ChatModelTarget>>,
    /// Explicit wrap/fanout mode.
    #[serde(default)]
    wrap_mode: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct ChatHistoryQuery {
    agent_id: Option<String>,
    #[serde(default = "default_chat_history_limit")]
    limit: usize,
}

fn default_chat_history_limit() -> usize {
    100
}

#[derive(Debug, Serialize)]
struct ChatHistoryResponse {
    conversation_key: String,
    messages: Vec<InferenceChatMessage>,
}

#[derive(Debug, Serialize)]
struct ChatResponse {
    reply: String,
    source: String,
    target: String,
    active_agents: usize,
    gate_state: GateState,
    model: Option<String>,
    provider: Option<String>,
    mode: Option<String>,
    input_tokens: Option<u32>,
    output_tokens: Option<u32>,
}

fn normalize_chat_mode(mode: Option<&str>) -> Result<Option<String>, (StatusCode, String)> {
    let Some(raw) = mode.map(str::trim).filter(|v| !v.is_empty()) else {
        return Ok(None);
    };

    let normalized = raw.to_ascii_lowercase();
    let allowed = matches!(
        normalized.as_str(),
        "yolo" | "build" | "plan" | "reason" | "orchestrator"
    );

    if !allowed {
        return Err((
            StatusCode::BAD_REQUEST,
            format!(
                "Unknown mode '{}'. Use 'yolo', 'build', 'plan', 'reason', or 'orchestrator'.",
                raw
            ),
        ));
    }

    Ok(Some(normalized))
}

fn normalize_chat_targets(targets: Option<&[ChatModelTarget]>) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();

    let Some(targets) = targets else {
        return out;
    };

    for target in targets {
        let provider = target.provider.trim();
        let model = target.model.trim();
        if provider.is_empty() || model.is_empty() {
            continue;
        }
        let key = format!("{}|{}", provider.to_ascii_lowercase(), model);
        if seen.insert(key) {
            out.push((provider.to_string(), model.to_string()));
        }
    }

    out
}

fn resolve_provider_override_alias<'a>(
    requested_provider: &'a str,
    configured_provider: &'a str,
) -> (&'a str, bool) {
    let zeroclaw_native = matches!(
        requested_provider.to_ascii_lowercase().as_str(),
        "zeroclaw" | "kaizen" | "kai-zen" | "native"
    );
    let provider_name = if zeroclaw_native {
        configured_provider
    } else {
        requested_provider
    };

    (provider_name, zeroclaw_native)
}

fn mode_instruction(mode: &str) -> &'static str {
    match mode {
        "yolo" => "Mode yolo: move fast, prioritize momentum, and surface risks briefly.",
        "build" => "Mode build: produce concrete implementation steps and executable outputs.",
        "plan" => "Mode plan: structure work into ordered milestones before implementation.",
        "reason" => "Mode reason: explain assumptions, alternatives, and decision rationale.",
        "orchestrator" => {
            "Mode orchestrator: coordinate multiple agents, assign responsibilities, and verify handoffs."
        }
        _ => "",
    }
}

fn apply_mode_prompt(base: &str, mode: Option<&str>) -> String {
    let Some(mode) = mode else {
        return base.to_string();
    };

    format!(
        "{base}\n\nOperator-selected mode: {mode}.\n{}",
        mode_instruction(mode)
    )
}

/// Resolve inference settings into provider + model + API key from vault.
async fn resolve_inference(
    state: &AppState,
    provider_override: Option<&str>,
    model_override: Option<&str>,
) -> Result<(InferenceProvider, String, String), (StatusCode, String)> {
    let settings = state.settings.read().await;
    let requested_provider = provider_override
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .unwrap_or(&settings.inference_provider);

    let (provider_name, zeroclaw_native) =
        resolve_provider_override_alias(requested_provider, &settings.inference_provider);

    let provider = InferenceProvider::from_str_loose(provider_name).ok_or((
        StatusCode::BAD_REQUEST,
        format!(
            "Unknown inference provider '{}'. Use 'zeroclaw', 'anthropic', 'openai', 'gemini', 'gemini-cli', or 'nvidia'.",
            requested_provider
        ),
    ))?;

    let model = if zeroclaw_native {
        if settings.inference_model.is_empty() {
            provider.default_model().to_string()
        } else {
            settings.inference_model.clone()
        }
    } else if let Some(m) = model_override.map(str::trim).filter(|v| !v.is_empty()) {
        m.to_string()
    } else if settings.inference_model.is_empty() {
        provider.default_model().to_string()
    } else {
        settings.inference_model.clone()
    };

    // Guard: web model names (e.g. "Web-Gem") must never go through the vault/API path.
    // They belong to the /v1/chat/completions route backed by virtual keys (sk-vt-*).
    if model.starts_with("Web-") {
        return Err((
            StatusCode::BAD_REQUEST,
            format!(
                "Model '{}' is a web model. Use /v1/chat/completions with a virtual key (sk-vt-*), not /api/chat.",
                model
            ),
        ));
    }

    let api_key = if let Some(vault_key) = provider.vault_key() {
        let vault = state.vault.as_ref().as_ref().ok_or((
            StatusCode::SERVICE_UNAVAILABLE,
            "Secret vault is not available. Open Settings -> Providers to check vault status."
                .to_string(),
        ))?;

        vault.decrypt(vault_key).await.map_err(|e| {
            (
                StatusCode::SERVICE_UNAVAILABLE,
                format!(
                    "No API key configured for '{}'. Store one via PUT /api/secrets/{}. Error: {}",
                    provider, vault_key, e
                ),
            )
        })?
    } else {
        // CLI-managed auth (Gemini CLI OAuth) does not use vault API keys.
        String::new()
    };

    Ok((provider, model, api_key))
}

/// Get or create conversation history for a conversation key.
async fn get_conversation(state: &AppState, key: &str) -> Vec<InferenceChatMessage> {
    let conversations = state.conversations.read().await;
    conversations.get(key).cloned().unwrap_or_default()
}

async fn conversation_version(state: &AppState, key: &str) -> u64 {
    let versions = state.conversation_versions.read().await;
    versions.get(key).copied().unwrap_or(0)
}

async fn clear_conversation(state: &AppState, key: &str) {
    {
        let mut conversations = state.conversations.write().await;
        conversations.remove(key);
    }

    bump_conversation_version(state, key).await;
}

async fn bump_conversation_version(state: &AppState, key: &str) {
    let mut versions = state.conversation_versions.write().await;
    let entry = versions.entry(key.to_string()).or_insert(0);
    *entry = entry.saturating_add(1);
}

/// Append messages to conversation history.
async fn append_to_conversation(
    state: &AppState,
    key: &str,
    user_msg: &str,
    assistant_msg: &str,
    expected_version: u64,
) {
    if key != "kaizen" {
        let agents = state.agents.read().await;
        if agents.get(key).is_none() {
            return;
        }
    }

    let mut conversations = state.conversations.write().await;
    let current_version = {
        let versions = state.conversation_versions.read().await;
        versions.get(key).copied().unwrap_or(0)
    };
    if current_version != expected_version {
        return;
    }

    let history = conversations.entry(key.to_string()).or_default();
    history.push(InferenceChatMessage {
        role: "user".to_string(),
        content: user_msg.to_string(),
    });
    history.push(InferenceChatMessage {
        role: "assistant".to_string(),
        content: assistant_msg.to_string(),
    });

    // Keep conversation history bounded (last 50 turns = 100 messages)
    if history.len() > 100 {
        let drain = history.len() - 100;
        history.drain(0..drain);
    }
}

fn parse_gemini_stream_tokens(data: &str) -> Vec<String> {
    fn collect_tokens(value: &serde_json::Value, out: &mut Vec<String>) {
        if let Some(candidates) = value.get("candidates").and_then(|v| v.as_array()) {
            for candidate in candidates {
                if let Some(parts) = candidate
                    .get("content")
                    .and_then(|c| c.get("parts"))
                    .and_then(|v| v.as_array())
                {
                    for part in parts {
                        if let Some(text) = part.get("text").and_then(|v| v.as_str()) {
                            if !text.is_empty() {
                                out.push(text.to_string());
                            }
                        }
                    }
                }
            }
        }

        if let Some(items) = value.as_array() {
            for item in items {
                collect_tokens(item, out);
            }
        }
    }

    let mut tokens = Vec::new();
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(data) {
        collect_tokens(&value, &mut tokens);
    }
    tokens
}

async fn get_chat_history(
    State(state): State<AppState>,
    Query(query): Query<ChatHistoryQuery>,
) -> Result<Json<ChatHistoryResponse>, (StatusCode, String)> {
    let conversation_key = if let Some(agent_id) = query.agent_id {
        let agents = state.agents.read().await;
        agents
            .get(&agent_id)
            .ok_or((StatusCode::NOT_FOUND, "agent not found".to_string()))?;
        agent_id
    } else {
        "kaizen".to_string()
    };

    let mut messages = get_conversation(&state, &conversation_key).await;
    let limit = query.limit.clamp(1, 100);
    if messages.len() > limit {
        messages = messages.split_off(messages.len() - limit);
    }

    Ok(Json(ChatHistoryResponse {
        conversation_key,
        messages,
    }))
}

async fn chat(
    State(state): State<AppState>,
    Json(request): Json<ChatRequest>,
) -> Result<Json<ChatResponse>, (StatusCode, String)> {
    let message = request.message.trim();
    if message.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "message cannot be empty".to_string(),
        ));
    }

    let selected_mode = normalize_chat_mode(request.mode.as_deref())?;

    let source = "user".to_string();
    let conversation_key: String;
    let target: String;

    if let Some(ref agent_id) = request.agent_id {
        let allow_direct = state
            .settings
            .read()
            .await
            .allow_direct_user_to_subagent_chat;
        if !allow_direct {
            return Err((
                StatusCode::FORBIDDEN,
                "Direct user-to-subagent chat is disabled in settings".to_string(),
            ));
        }

        let agents = state.agents.read().await;
        let agent = agents
            .get(agent_id)
            .ok_or((StatusCode::NOT_FOUND, "agent not found".to_string()))?;

        target = agent.name.clone();
        conversation_key = agent_id.clone();
    } else {
        target = "Kaizen".to_string();
        conversation_key = "kaizen".to_string();
    }

    // Clear history if requested
    if request.clear_history {
        clear_conversation(&state, &conversation_key).await;
    }

    let expected_conversation_version = conversation_version(&state, &conversation_key).await;

    // Emit Crystal Ball event for the user message
    push_event(
        &state,
        CrystalBallEvent {
            event_id: next_id(&state, "event"),
            timestamp: now_timestamp(),
            event_type: "orchestration.requested".to_string(),
            source_actor: source.clone(),
            source_agent_id: "human".to_string(),
            target_actor: target.clone(),
            target_agent_id: target.to_lowercase(),
            task_id: "chat".to_string(),
            message: if let Some(mode) = selected_mode.as_deref() {
                format!("[mode:{mode}] {message}")
            } else {
                message.to_string()
            },
            visibility: "operator".to_string(),
        },
    )
    .await;

    let requested_targets = normalize_chat_targets(request.selected_models.as_deref());
    let wrap_mode_requested = request.wrap_mode.unwrap_or(false);
    if wrap_mode_requested && requested_targets.len() < 2 {
        return Err((
            StatusCode::BAD_REQUEST,
            "wrap_mode requires at least 2 valid selected_models targets".to_string(),
        ));
    }
    let use_wrap_mode = wrap_mode_requested && requested_targets.len() > 1;

    // Attempt real inference
    let (reply, model, provider_name, input_tokens, output_tokens) = if use_wrap_mode {
        let settings = state.settings.read().await;
        let max_tokens = settings.inference_max_tokens;
        let temperature = settings.inference_temperature;
        drop(settings);

        let history = get_conversation(&state, &conversation_key).await;
        let system_prompt =
            apply_mode_prompt(state.system_prompt.as_ref(), selected_mode.as_deref());

        let mut sections = Vec::new();
        let mut errors = Vec::new();

        for (provider_override, model_override) in requested_targets {
            match resolve_inference(&state, Some(&provider_override), Some(&model_override)).await {
                Ok((provider, resolved_model, mut api_key)) => {
                    let mut messages = history.clone();
                    messages.push(InferenceChatMessage {
                        role: "user".to_string(),
                        content: message.to_string(),
                    });

                    let req = InferenceRequest {
                        provider,
                        model: resolved_model.clone(),
                        system_prompt: system_prompt.clone(),
                        messages,
                        max_tokens,
                        temperature,
                    };

                    let inference_result = state.inference.complete(&api_key, &req).await;
                    wipe_string(&mut api_key);

                    match inference_result {
                        Ok(resp) => {
                            sections.push(format!(
                                "[{} / {}]\n{}",
                                resp.provider,
                                resp.model,
                                resp.content.trim()
                            ));
                        }
                        Err(err) => {
                            tracing::error!(
                                "Wrap mode inference failed for {} / {}: {}",
                                provider_override,
                                resolved_model,
                                err
                            );
                            errors.push(format!(
                                "{} / {} -> {}",
                                provider_override, resolved_model, err
                            ));
                        }
                    }
                }
                Err((_status, reason)) => {
                    errors.push(format!(
                        "{} / {} -> {}",
                        provider_override, model_override, reason
                    ));
                }
            }
        }

        let mut combined = if sections.is_empty() {
            let detail = if errors.is_empty() {
                "No model targets were available.".to_string()
            } else {
                format!("{}", errors.join(" | "))
            };
            format!("Wrap mode did not return any model output. {detail}")
        } else {
            sections.join("\n\n----------------\n\n")
        };

        if !errors.is_empty() {
            combined.push_str("\n\n[Wrap warnings]\n");
            for err in errors {
                combined.push_str("- ");
                combined.push_str(&err);
                combined.push('\n');
            }
        }

        append_to_conversation(
            &state,
            &conversation_key,
            message,
            &combined,
            expected_conversation_version,
        )
        .await;

        (combined, None, None, None, None)
    } else {
        match resolve_inference(
            &state,
            request.provider.as_deref(),
            request.model.as_deref(),
        )
        .await
        {
            Ok((provider, model, mut api_key)) => {
                let settings = state.settings.read().await;
                let max_tokens = settings.inference_max_tokens;
                let temperature = settings.inference_temperature;
                drop(settings);

                let history = get_conversation(&state, &conversation_key).await;

                let mut messages = history;
                messages.push(InferenceChatMessage {
                    role: "user".to_string(),
                    content: message.to_string(),
                });

                let req = InferenceRequest {
                    provider,
                    model: model.clone(),
                    system_prompt: apply_mode_prompt(
                        state.system_prompt.as_ref(),
                        selected_mode.as_deref(),
                    ),
                    messages,
                    max_tokens,
                    temperature,
                };

                let inference_result = state.inference.complete(&api_key, &req).await;
                wipe_string(&mut api_key);

                match inference_result {
                    Ok(resp) => {
                        // Store in conversation history
                        append_to_conversation(
                            &state,
                            &conversation_key,
                            message,
                            &resp.content,
                            expected_conversation_version,
                        )
                        .await;

                        (
                            resp.content,
                            Some(resp.model),
                            Some(resp.provider),
                            resp.input_tokens,
                            resp.output_tokens,
                        )
                    }
                    Err(e) => {
                        tracing::error!("Inference failed: {}", e);
                        (
                            format!("[Inference error] {e}"),
                            Some(model),
                            Some(provider.to_string()),
                            None,
                            None,
                        )
                    }
                }
            }
            Err((_status, reason)) => {
                // Fallback: no vault or no API key configured - return helpful message
                tracing::warn!("Inference not available: {}", reason);
                (
                    format!(
                        "Kaizen is in offline mode. Open Settings -> Providers and add an API key \
                         for Anthropic, OpenAI, Gemini, or NVIDIA (or select Gemini CLI with local OAuth). Reason: {reason}"
                    ),
                    None,
                    None,
                    None,
                    None,
                )
            }
        }
    };

    let active_agents = state.agents.read().await.active_count();
    let gate_state = state.gates.read().await.current_state;

    // Emit Crystal Ball event for the response
    push_event(
        &state,
        CrystalBallEvent {
            event_id: next_id(&state, "event"),
            timestamp: now_timestamp(),
            event_type: "orchestration.response".to_string(),
            source_actor: target.clone(),
            source_agent_id: target.to_lowercase(),
            target_actor: source.clone(),
            target_agent_id: "human".to_string(),
            task_id: "chat".to_string(),
            message: if reply.len() > 200 {
                format!("{}...", &reply[..200])
            } else {
                reply.clone()
            },
            visibility: "operator".to_string(),
        },
    )
    .await;

    Ok(Json(ChatResponse {
        reply,
        source,
        target,
        active_agents,
        gate_state,
        model,
        provider: provider_name,
        mode: selected_mode,
        input_tokens,
        output_tokens,
    }))
}

// ── Streaming Chat (SSE) ───────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct ChatStreamRequest {
    message: String,
    agent_id: Option<String>,
    #[serde(default)]
    clear_history: bool,
    #[serde(default)]
    provider: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    mode: Option<String>,
    #[serde(default)]
    selected_models: Option<Vec<ChatModelTarget>>,
    #[serde(default)]
    wrap_mode: Option<bool>,
}

async fn chat_stream(
    State(state): State<AppState>,
    Json(request): Json<ChatStreamRequest>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, (StatusCode, String)> {
    let message = request.message.trim().to_string();
    if message.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "message cannot be empty".to_string(),
        ));
    }

    let selected_mode = normalize_chat_mode(request.mode.as_deref())?;

    let selected_targets = normalize_chat_targets(request.selected_models.as_deref());
    if request.wrap_mode.unwrap_or(false) || selected_targets.len() > 1 {
        return Err((
            StatusCode::BAD_REQUEST,
            "Wrap mode is currently available on POST /api/chat only. Use non-stream chat for multi-model requests."
                .to_string(),
        ));
    }

    let conversation_key = if let Some(ref agent_id) = request.agent_id {
        let allow_direct = state
            .settings
            .read()
            .await
            .allow_direct_user_to_subagent_chat;
        if !allow_direct {
            return Err((
                StatusCode::FORBIDDEN,
                "Direct user-to-subagent chat is disabled in settings".to_string(),
            ));
        }
        let agents = state.agents.read().await;
        agents
            .get(agent_id)
            .ok_or((StatusCode::NOT_FOUND, "agent not found".to_string()))?;
        agent_id.clone()
    } else {
        "kaizen".to_string()
    };

    if request.clear_history {
        clear_conversation(&state, &conversation_key).await;
    }

    let expected_conversation_version = conversation_version(&state, &conversation_key).await;

    let (provider, model, mut api_key) = resolve_inference(
        &state,
        request.provider.as_deref(),
        request.model.as_deref(),
    )
    .await?;

    let settings = state.settings.read().await;
    let max_tokens = settings.inference_max_tokens;
    let temperature = settings.inference_temperature;
    drop(settings);

    let history = get_conversation(&state, &conversation_key).await;
    let mut messages = history;
    messages.push(InferenceChatMessage {
        role: "user".to_string(),
        content: message.clone(),
    });

    let req = InferenceRequest {
        provider,
        model: model.clone(),
        system_prompt: apply_mode_prompt(state.system_prompt.as_ref(), selected_mode.as_deref()),
        messages,
        max_tokens,
        temperature,
    };

    let raw_response = state
        .inference
        .stream_raw(&api_key, &req)
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, e));
    wipe_string(&mut api_key);
    let raw_response = raw_response?;

    // Build SSE stream that parses provider-specific SSE and re-emits normalized tokens
    let state_clone = state.clone();
    let conv_key = conversation_key.clone();
    let user_msg = message.clone();

    let stream = async_stream::stream! {
        use futures_util::StreamExt;

        let mut byte_stream = raw_response.bytes_stream();
        let mut buffer = String::new();
        let mut full_response = String::new();
        let mut done_emitted = false;

        while let Some(chunk_result) = byte_stream.next().await {
            let chunk = match chunk_result {
                Ok(bytes) => String::from_utf8_lossy(&bytes).to_string(),
                Err(e) => {
                    let err_event = Event::default()
                        .event("error")
                        .data(format!("Stream error: {e}"));
                    yield Ok(err_event);
                    break;
                }
            };

            buffer.push_str(&chunk);

            // Parse SSE lines
            while let Some(line_end) = buffer.find('\n') {
                let line = buffer[..line_end].trim_end_matches('\r').to_string();
                buffer = buffer[line_end + 1..].to_string();

                if line.is_empty() || line.starts_with(':') {
                    continue;
                }

                if !line.starts_with("data: ") {
                    continue;
                }

                let data = &line[6..];

                if data == "[DONE]" {
                    // Store conversation history
                    append_to_conversation(
                        &state_clone,
                        &conv_key,
                        &user_msg,
                        &full_response,
                        expected_conversation_version,
                    ).await;

                    let done_event = Event::default()
                        .event("done")
                        .data(serde_json::json!({
                            "full_response": full_response,
                            "model": model,
                            "provider": provider.to_string(),
                        }).to_string());
                    yield Ok(done_event);
                    done_emitted = true;
                    break;
                }

                // Parse based on provider
                match provider {
                    InferenceProvider::Anthropic => {
                        if let Ok(event) = serde_json::from_str::<AnthropicStreamEvent>(data) {
                            match event {
                                AnthropicStreamEvent::ContentBlockDelta { delta, .. } => {
                                    if let Some(text) = delta.text {
                                        full_response.push_str(&text);
                                        let token_event = Event::default()
                                            .event("token")
                                            .data(serde_json::json!({ "text": text }).to_string());
                                        yield Ok(token_event);
                                    }
                                }
                                AnthropicStreamEvent::MessageStop {} => {
                                    append_to_conversation(
                                        &state_clone,
                                        &conv_key,
                                        &user_msg,
                                        &full_response,
                                        expected_conversation_version,
                                    ).await;

                                    let done_event = Event::default()
                                        .event("done")
                                        .data(serde_json::json!({
                                            "full_response": full_response,
                                            "model": model,
                                            "provider": "anthropic",
                                        }).to_string());
                                    yield Ok(done_event);
                                    done_emitted = true;
                                }
                                _ => {}
                            }
                        }
                    }
                    InferenceProvider::OpenAI | InferenceProvider::Nvidia => {
                        if let Ok(chunk) = serde_json::from_str::<OpenAIStreamChunk>(data) {
                            for choice in &chunk.choices {
                                if let Some(ref text) = choice.delta.content {
                                    full_response.push_str(text);
                                    let token_event = Event::default()
                                        .event("token")
                                        .data(serde_json::json!({ "text": text }).to_string());
                                    yield Ok(token_event);
                                }
                            }
                        }
                    }
                    InferenceProvider::Gemini => {
                        let tokens = parse_gemini_stream_tokens(data);
                        for text in tokens {
                            full_response.push_str(&text);
                            let token_event = Event::default()
                                .event("token")
                                .data(serde_json::json!({ "text": text }).to_string());
                            yield Ok(token_event);
                        }
                    }
                    InferenceProvider::GeminiCli => {
                        // stream_raw currently rejects Gemini CLI, so this branch is
                        // effectively unreachable for now.
                    }
                }
            }
        }

        if !done_emitted {
            append_to_conversation(
                &state_clone,
                &conv_key,
                &user_msg,
                &full_response,
                expected_conversation_version,
            ).await;

            let done_event = Event::default()
                .event("done")
                .data(serde_json::json!({
                    "full_response": full_response,
                    "model": model,
                    "provider": provider.to_string(),
                }).to_string());
            yield Ok(done_event);
        }
    };

    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

// ---- WebKeys Routes ----

#[derive(Debug, Deserialize)]
struct CreateVirtualKeyHttpRequest {
    name: String,
    provider_binding_ids: Vec<String>,
    default_binding_id: Option<String>,
    model_allowlist: Option<Vec<String>>,
    #[serde(default)]
    metadata: Option<std::collections::HashMap<String, String>>,
    rate_limit_rpm: Option<u32>,
    rate_limit_tpm: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct UpdateVirtualKeyHttpRequest {
    name: Option<String>,
    enabled: Option<bool>,
    provider_binding_ids: Option<Vec<String>>,
    default_binding_id: Option<String>,
    model_allowlist: Option<Vec<String>>,
    metadata: Option<std::collections::HashMap<String, String>>,
    rate_limit_rpm: Option<u32>,
    rate_limit_tpm: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct CreateProviderBindingHttpRequest {
    provider_type: WebProviderType,
    account_id: String,
    display_name: String,
    profile_path: String,
}

#[derive(Debug, Deserialize)]
struct UpdateProviderBindingHttpRequest {
    display_name: Option<String>,
    enabled: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct VerifyVirtualKeyRequest {
    raw_key: String,
    preferred_binding_id: Option<String>,
}

#[derive(Debug, Serialize)]
struct VerifyVirtualKeyResponse {
    valid: bool,
    key: Option<VirtualKeyPublicRecord>,
    selected_binding_id: Option<String>,
    error: Option<String>,
}

// Admin: Virtual Key Routes
// All /api/webkeys/* routes require admin-token authentication.

async fn webkeys_list_keys(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<VirtualKeyPublicRecord>>, (StatusCode, String)> {
    validate_admin_access_http(
        state.admin_api_token.as_deref(),
        &headers,
        "webkeys:list_keys",
    )?;
    let webkeys = state.webkeys.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "WebKeys not available".to_string(),
    ))?;
    let keys = webkeys.list_virtual_keys().await;
    Ok(Json(keys))
}

async fn webkeys_create_key(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<CreateVirtualKeyHttpRequest>,
) -> Result<Json<VirtualKeyCreationResult>, (StatusCode, String)> {
    validate_admin_access_http(
        state.admin_api_token.as_deref(),
        &headers,
        "webkeys:create_key",
    )?;
    let webkeys = state.webkeys.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "WebKeys not available".to_string(),
    ))?;

    let internal_request = zeroclaw_gateway::webkeys::CreateVirtualKeyRequest {
        name: request.name,
        provider_binding_ids: request.provider_binding_ids,
        default_binding_id: request.default_binding_id,
        model_allowlist: request.model_allowlist,
        metadata: request.metadata,
        rate_limit_rpm: request.rate_limit_rpm,
        rate_limit_tpm: request.rate_limit_tpm,
    };

    webkeys
        .create_virtual_key(internal_request)
        .await
        .map(Json)
        .map_err(|e| (StatusCode::BAD_REQUEST, e))
}

async fn webkeys_get_key(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<VirtualKeyPublicRecord>, (StatusCode, String)> {
    validate_admin_access_http(
        state.admin_api_token.as_deref(),
        &headers,
        "webkeys:get_key",
    )?;
    let webkeys = state.webkeys.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "WebKeys not available".to_string(),
    ))?;

    webkeys
        .get_virtual_key(&id)
        .await
        .map(Json)
        .ok_or((StatusCode::NOT_FOUND, "Key not found".to_string()))
}

async fn webkeys_update_key(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(request): Json<UpdateVirtualKeyHttpRequest>,
) -> Result<Json<VirtualKeyPublicRecord>, (StatusCode, String)> {
    validate_admin_access_http(
        state.admin_api_token.as_deref(),
        &headers,
        "webkeys:update_key",
    )?;
    let webkeys = state.webkeys.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "WebKeys not available".to_string(),
    ))?;

    let internal_request = zeroclaw_gateway::webkeys::UpdateVirtualKeyRequest {
        name: request.name,
        enabled: request.enabled,
        provider_binding_ids: request.provider_binding_ids,
        default_binding_id: request.default_binding_id,
        model_allowlist: request.model_allowlist,
        metadata: request.metadata,
        rate_limit_rpm: request.rate_limit_rpm,
        rate_limit_tpm: request.rate_limit_tpm,
    };

    webkeys
        .update_virtual_key(&id, internal_request)
        .await
        .map(Json)
        .map_err(|e| (StatusCode::BAD_REQUEST, e))
}

async fn webkeys_delete_key(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<StatusCode, (StatusCode, String)> {
    validate_admin_access_http(
        state.admin_api_token.as_deref(),
        &headers,
        "webkeys:delete_key",
    )?;
    let webkeys = state.webkeys.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "WebKeys not available".to_string(),
    ))?;

    webkeys
        .delete_virtual_key(&id)
        .await
        .map(|found| {
            if found {
                StatusCode::NO_CONTENT
            } else {
                StatusCode::NOT_FOUND
            }
        })
        .map_err(|e| (StatusCode::BAD_REQUEST, e))
}

async fn webkeys_rotate_key(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<VirtualKeyCreationResult>, (StatusCode, String)> {
    validate_admin_access_http(
        state.admin_api_token.as_deref(),
        &headers,
        "webkeys:rotate_key",
    )?;
    let webkeys = state.webkeys.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "WebKeys not available".to_string(),
    ))?;

    webkeys
        .rotate_virtual_key(&id)
        .await
        .map(Json)
        .map_err(|e| (StatusCode::BAD_REQUEST, e))
}

async fn webkeys_verify_key(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<VerifyVirtualKeyRequest>,
) -> Result<Json<VerifyVirtualKeyResponse>, (StatusCode, String)> {
    validate_admin_access_http(
        state.admin_api_token.as_deref(),
        &headers,
        "webkeys:verify_key",
    )?;
    let webkeys = match state.webkeys.as_ref() {
        Some(w) => w,
        None => {
            return Ok(Json(VerifyVirtualKeyResponse {
                valid: false,
                key: None,
                selected_binding_id: None,
                error: Some("WebKeys not available".to_string()),
            }));
        }
    };

    match webkeys
        .verify_virtual_key(&request.raw_key, request.preferred_binding_id.as_deref())
        .await
    {
        Ok((key, binding_id)) => Ok(Json(VerifyVirtualKeyResponse {
            valid: true,
            key: Some(key),
            selected_binding_id: Some(binding_id),
            error: None,
        })),
        Err(e) => Ok(Json(VerifyVirtualKeyResponse {
            valid: false,
            key: None,
            selected_binding_id: None,
            error: Some(e),
        })),
    }
}

// Admin: Provider Binding Routes

async fn webkeys_list_bindings(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<WebProviderBindingPublicRecord>>, (StatusCode, String)> {
    validate_admin_access_http(
        state.admin_api_token.as_deref(),
        &headers,
        "webkeys:list_bindings",
    )?;
    let webkeys = state.webkeys.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "WebKeys not available".to_string(),
    ))?;
    let bindings = webkeys.list_provider_bindings().await;
    Ok(Json(bindings))
}

async fn webkeys_create_binding(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<CreateProviderBindingHttpRequest>,
) -> Result<Json<WebProviderBindingPublicRecord>, (StatusCode, String)> {
    validate_admin_access_http(
        state.admin_api_token.as_deref(),
        &headers,
        "webkeys:create_binding",
    )?;
    let webkeys = state.webkeys.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "WebKeys not available".to_string(),
    ))?;

    let internal_request = zeroclaw_gateway::webkeys::CreateProviderBindingRequest {
        provider_type: request.provider_type,
        account_id: request.account_id,
        display_name: request.display_name,
        profile_path: request.profile_path,
    };

    webkeys
        .create_provider_binding(internal_request)
        .await
        .map(Json)
        .map_err(|e| (StatusCode::BAD_REQUEST, e))
}

async fn webkeys_get_binding(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<WebProviderBindingPublicRecord>, (StatusCode, String)> {
    validate_admin_access_http(
        state.admin_api_token.as_deref(),
        &headers,
        "webkeys:get_binding",
    )?;
    let webkeys = state.webkeys.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "WebKeys not available".to_string(),
    ))?;

    webkeys
        .get_provider_binding(&id)
        .await
        .map(Json)
        .ok_or((StatusCode::NOT_FOUND, "Binding not found".to_string()))
}

async fn webkeys_update_binding(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(request): Json<UpdateProviderBindingHttpRequest>,
) -> Result<Json<WebProviderBindingPublicRecord>, (StatusCode, String)> {
    validate_admin_access_http(
        state.admin_api_token.as_deref(),
        &headers,
        "webkeys:update_binding",
    )?;
    let webkeys = state.webkeys.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "WebKeys not available".to_string(),
    ))?;

    let internal_request = zeroclaw_gateway::webkeys::UpdateProviderBindingRequest {
        display_name: request.display_name,
        enabled: request.enabled,
    };

    webkeys
        .update_provider_binding(&id, internal_request)
        .await
        .map(Json)
        .map_err(|e| (StatusCode::BAD_REQUEST, e))
}

async fn webkeys_delete_binding(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<StatusCode, (StatusCode, String)> {
    validate_admin_access_http(
        state.admin_api_token.as_deref(),
        &headers,
        "webkeys:delete_binding",
    )?;
    let webkeys = state.webkeys.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "WebKeys not available".to_string(),
    ))?;

    webkeys
        .delete_provider_binding(&id)
        .await
        .map(|found| {
            if found {
                StatusCode::NO_CONTENT
            } else {
                StatusCode::NOT_FOUND
            }
        })
        .map_err(|e| (StatusCode::BAD_REQUEST, e))
}

// Runtime: OpenAI-Compatible Routes

#[derive(Debug, Deserialize)]
struct ChatCompletionRequest {
    model: String,
    messages: Vec<ChatCompletionMessage>,
    #[serde(default)]
    stream: bool,
    #[serde(default)]
    #[allow(dead_code)]
    max_tokens: Option<u32>,
    #[serde(default)]
    #[allow(dead_code)]
    temperature: Option<f32>,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionMessage {
    role: String,
    content: String,
}

#[derive(Debug, Serialize)]
struct ChatCompletionResponse {
    id: String,
    object: String,
    created: i64,
    model: String,
    choices: Vec<ChatCompletionChoice>,
    usage: ChatCompletionUsage,
}

#[derive(Debug, Serialize)]
struct ChatCompletionChoice {
    index: u32,
    message: ChatCompletionResponseMessage,
    finish_reason: String,
}

#[derive(Debug, Serialize)]
struct ChatCompletionResponseMessage {
    role: String,
    content: String,
}

#[derive(Debug, Serialize)]
struct ChatCompletionUsage {
    prompt_tokens: u32,
    completion_tokens: u32,
    total_tokens: u32,
}

#[derive(Debug, Serialize)]
struct ModelListResponse {
    object: String,
    data: Vec<ModelInfo>,
}

#[derive(Debug, Serialize)]
struct ModelInfo {
    id: String,
    object: String,
    created: i64,
    owned_by: String,
}

async fn webkeys_chat_completions(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<ChatCompletionRequest>,
) -> Result<Json<ChatCompletionResponse>, (StatusCode, Json<serde_json::Value>)> {
    let webkeys = state.webkeys.as_ref().ok_or_else(|| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "WebKeys not available"})),
        )
    })?;

    let runtime = state.webkeys_runtime.as_ref().ok_or_else(|| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "WebKeys runtime not available"})),
        )
    })?;

    // 1. Authenticate via sk-vt-* bearer token (optional in native mode)
    let is_native = resolve_bind_mode() == "native";
    let key_record = match authenticate_bearer(&headers, webkeys).await {
        Ok(record) => Some(record),
        Err(e) if is_native => {
            tracing::debug!("Native mode: skipping virtual key auth for /v1/chat/completions");
            None
        }
        Err(e) => {
            let env = e.as_openai_error();
            return Err((
                StatusCode::UNAUTHORIZED,
                Json(serde_json::to_value(&env).unwrap_or_default()),
            ));
        }
    };

    // 2. Build runtime request from the OpenAI-compatible payload
    let runtime_request = zeroclaw_gateway::webkeys::types::ChatCompletionRequest {
        model: Some(request.model.clone()),
        messages: request
            .messages
            .iter()
            .map(|m| zeroclaw_gateway::webkeys::types::ChatMessage {
                role: m.role.clone(),
                content: m.content.clone(),
            })
            .collect(),
        stream: Some(request.stream),
    };

    // 3. Execute through runtime (auth-checked, health-checked)
    let result = runtime.execute_chat(runtime_request).await.map_err(|e| {
        let status = match e {
            zeroclaw_gateway::webkeys::runtime_contract::RuntimeError::AuthRequired => {
                StatusCode::UNAUTHORIZED
            }
            zeroclaw_gateway::webkeys::runtime_contract::RuntimeError::Unavailable(_) => {
                StatusCode::SERVICE_UNAVAILABLE
            }
            zeroclaw_gateway::webkeys::runtime_contract::RuntimeError::InvalidRequest(_) => {
                StatusCode::BAD_REQUEST
            }
        };
        (
            status,
            Json(serde_json::json!({"error": {"message": e.to_string(), "type": "runtime_error"}})),
        )
    })?;

    // 4. Record usage (best-effort, non-fatal, only when key was provided)
    if let Some(ref record) = key_record {
        let _ = webkeys.record_usage(&record.id, 0, 0).await;
    }

    // 5. Return OpenAI-compatible response
    Ok(Json(ChatCompletionResponse {
        id: format!("chatcmpl-{}", uuid::Uuid::new_v4()),
        object: "chat.completion".to_string(),
        created: chrono::Utc::now().timestamp(),
        model: result.model,
        choices: vec![ChatCompletionChoice {
            index: 0,
            message: ChatCompletionResponseMessage {
                role: "assistant".to_string(),
                content: result.content,
            },
            finish_reason: result.finish_reason,
        }],
        usage: ChatCompletionUsage {
            prompt_tokens: 0,
            completion_tokens: 0,
            total_tokens: 0,
        },
    }))
}

async fn webkeys_list_models(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<ModelListResponse>, (StatusCode, Json<serde_json::Value>)> {
    // /v1/models is protected — require a valid sk-vt-* bearer token
    let webkeys = state.webkeys.as_ref().ok_or_else(|| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "WebKeys not available"})),
        )
    })?;

    // In native mode, skip auth — allow unauthenticated model listing
    let is_native = resolve_bind_mode() == "native";
    if !is_native {
        authenticate_bearer(&headers, webkeys).await.map_err(|e| {
            let env = e.as_openai_error();
            (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::to_value(&env).unwrap_or_default()),
            )
        })?;
    }

    let model_ids = state
        .webkeys_runtime
        .as_ref()
        .map(|rt| rt.models())
        .unwrap_or_else(|| vec!["Web-Gem".to_string()]);

    let ts = chrono::Utc::now().timestamp();
    let models = model_ids
        .into_iter()
        .map(|id| ModelInfo {
            object: "model".to_string(),
            created: ts,
            owned_by: "kaizenmax".to_string(),
            id,
        })
        .collect();

    Ok(Json(ModelListResponse {
        object: "list".to_string(),
        data: models,
    }))
}

#[derive(Debug, Deserialize)]
struct SpawnAgentRequest {
    agent_name: String,
    task_id: String,
    objective: String,
    #[serde(default)]
    user_requested: bool,
}

async fn list_agents(State(state): State<AppState>) -> Json<Vec<SubAgent>> {
    Json(state.agents.read().await.list().to_vec())
}

async fn spawn_agent(
    State(state): State<AppState>,
    Json(request): Json<SpawnAgentRequest>,
) -> Result<Json<SubAgent>, (StatusCode, String)> {
    let settings = state.settings.read().await.clone();
    if !settings.auto_spawn_subagents && !request.user_requested {
        return Err((
            StatusCode::FORBIDDEN,
            "Sub-agent spawn denied: explicit user request required".to_string(),
        ));
    }

    let agent_id = next_id(&state, "agent");
    let created = {
        let mut registry = state.agents.write().await;
        let created = registry
            .spawn(
                agent_id,
                request.agent_name,
                request.task_id,
                request.objective,
            )
            .map_err(|err| (StatusCode::BAD_REQUEST, err))?;
        created.clone()
    };

    push_event(
        &state,
        CrystalBallEvent {
            event_id: next_id(&state, "event"),
            timestamp: now_timestamp(),
            event_type: "subagent.spawned".to_string(),
            source_actor: "Kaizen".to_string(),
            source_agent_id: "kaizen".to_string(),
            target_actor: created.name.clone(),
            target_agent_id: created.id.clone(),
            task_id: created.task_id.clone(),
            message: format!("Spawned '{}' for task {}", created.name, created.task_id),
            visibility: "operator".to_string(),
        },
    )
    .await;

    Ok(Json(created))
}

#[derive(Debug, Deserialize)]
struct AgentStatusPatch {
    status: AgentStatus,
    #[serde(default)]
    kaizen_review_approved: bool,
}

async fn update_agent_status(
    State(state): State<AppState>,
    Path(agent_id): Path<String>,
    Json(request): Json<AgentStatusPatch>,
) -> Result<Json<SubAgent>, (StatusCode, String)> {
    if request.status == AgentStatus::Done {
        let gates = state.gates.read().await;
        if !gates.conditions.passed_reasoners_test || !gates.conditions.kaizen_review_approved {
            return Err((
                StatusCode::FORBIDDEN,
                "Cannot mark agent done until gate conditions include passed_reasoners_test and kaizen_review_approved".to_string(),
            ));
        }
    }

    let updated = {
        let mut registry = state.agents.write().await;
        registry
            .set_status(&agent_id, request.status, request.kaizen_review_approved)
            .map_err(|err| (StatusCode::BAD_REQUEST, err))?;
        registry
            .get(&agent_id)
            .ok_or((StatusCode::NOT_FOUND, "agent not found".to_string()))?
            .clone()
    };

    push_event(
        &state,
        CrystalBallEvent {
            event_id: next_id(&state, "event"),
            timestamp: now_timestamp(),
            event_type: "subagent.status".to_string(),
            source_actor: updated.name.clone(),
            source_agent_id: updated.id.clone(),
            target_actor: "Kaizen".to_string(),
            target_agent_id: "kaizen".to_string(),
            task_id: updated.task_id.clone(),
            message: format!("Status changed to {:?}", updated.status),
            visibility: "operator".to_string(),
        },
    )
    .await;

    Ok(Json(updated))
}

#[derive(Debug, Serialize)]
struct GateSnapshot {
    current_state: GateState,
    conditions: zeroclaw_gateway::gate_engine::GateConditions,
    hard_gates_enabled: bool,
}

async fn get_gates(State(state): State<AppState>) -> Json<GateSnapshot> {
    let gate_runtime = state.gates.read().await.clone();
    let settings = state.settings.read().await.clone();
    Json(GateSnapshot {
        current_state: gate_runtime.current_state,
        conditions: gate_runtime.conditions,
        hard_gates_enabled: settings.hard_gates_enabled,
    })
}

async fn patch_gate_conditions(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(patch): Json<GateConditionPatch>,
) -> Result<Json<GateSnapshot>, (StatusCode, String)> {
    require_admin_access(&state, &headers, "PATCH /api/gates/conditions")?;

    {
        let mut gates = state.gates.write().await;
        gates.update_conditions(patch);
    }
    Ok(get_gates(State(state)).await)
}

async fn advance_gates(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<TransitionResult>, (StatusCode, String)> {
    require_admin_access(&state, &headers, "POST /api/gates/advance")?;

    let hard_gates_enabled = state.settings.read().await.hard_gates_enabled;

    let result = {
        let mut gates = state.gates.write().await;
        if hard_gates_enabled {
            gates.advance()
        } else {
            let from = gates.current_state;
            let to = match from {
                GateState::Plan => GateState::Execute,
                GateState::Execute => GateState::Review,
                GateState::Review => GateState::HumanSmokeTest,
                GateState::HumanSmokeTest => GateState::Deploy,
                GateState::Deploy => GateState::Complete,
                GateState::Complete => GateState::Complete,
            };
            gates.current_state = to;
            TransitionResult {
                allowed: true,
                from,
                to,
                blocked_by: Vec::new(),
            }
        }
    };

    let event_type = if result.allowed {
        "gate.transition"
    } else {
        "gate.blocked"
    };

    push_event(
        &state,
        CrystalBallEvent {
            event_id: next_id(&state, "event"),
            timestamp: now_timestamp(),
            event_type: event_type.to_string(),
            source_actor: "gate_engine".to_string(),
            source_agent_id: "system".to_string(),
            target_actor: "Kaizen".to_string(),
            target_agent_id: "kaizen".to_string(),
            task_id: "gates".to_string(),
            message: if result.allowed {
                format!("Transition {:?} -> {:?}", result.from, result.to)
            } else {
                format!("Blocked at {:?}: {:?}", result.from, result.blocked_by)
            },
            visibility: "operator".to_string(),
        },
    )
    .await;

    Ok(Json(result))
}

// ---- Agent Rename ----

#[derive(Debug, Deserialize)]
struct AgentRenamePatch {
    name: String,
}

async fn rename_agent(
    State(state): State<AppState>,
    Path(agent_id): Path<String>,
    Json(request): Json<AgentRenamePatch>,
) -> Result<Json<SubAgent>, (StatusCode, String)> {
    let settings = state.settings.read().await;
    if !settings.agent_name_editable_after_spawn {
        return Err((
            StatusCode::FORBIDDEN,
            "Agent renaming is disabled in settings".to_string(),
        ));
    }
    drop(settings);

    let old_name = {
        let registry = state.agents.read().await;
        registry
            .get(&agent_id)
            .map(|a| a.name.clone())
            .ok_or((StatusCode::NOT_FOUND, "agent not found".to_string()))?
    };

    {
        let mut registry = state.agents.write().await;
        registry
            .rename(&agent_id, &request.name)
            .map_err(|err| (StatusCode::BAD_REQUEST, err))?;
    }

    let updated = {
        let registry = state.agents.read().await;
        registry
            .get(&agent_id)
            .ok_or((StatusCode::NOT_FOUND, "agent not found".to_string()))?
            .clone()
    };

    push_event(
        &state,
        CrystalBallEvent {
            event_id: next_id(&state, "event"),
            timestamp: now_timestamp(),
            event_type: "subagent.renamed".to_string(),
            source_actor: updated.name.clone(),
            source_agent_id: updated.id.clone(),
            target_actor: "Kaizen".to_string(),
            target_agent_id: "kaizen".to_string(),
            task_id: updated.task_id.clone(),
            message: format!("Renamed from '{}' to '{}'", old_name, updated.name),
            visibility: "operator".to_string(),
        },
    )
    .await;

    Ok(Json(updated))
}

// ---- Agent Remove / Clear / Stop ----

async fn remove_agent(
    State(state): State<AppState>,
    Path(agent_id): Path<String>,
) -> Result<StatusCode, (StatusCode, String)> {
    let removed = {
        let mut registry = state.agents.write().await;
        registry
            .remove(&agent_id)
            .map_err(|err| (StatusCode::NOT_FOUND, err))?
    };

    // Clear conversation history for this agent
    clear_conversation(&state, &agent_id).await;

    push_event(
        &state,
        CrystalBallEvent {
            event_id: next_id(&state, "event"),
            timestamp: now_timestamp(),
            event_type: "subagent.removed".to_string(),
            source_actor: "operator".to_string(),
            source_agent_id: "human".to_string(),
            target_actor: removed.name.clone(),
            target_agent_id: removed.id.clone(),
            task_id: removed.task_id.clone(),
            message: format!("Agent '{}' removed from active board", removed.name),
            visibility: "operator".to_string(),
        },
    )
    .await;

    Ok(StatusCode::NO_CONTENT)
}

async fn clear_agent_chat(
    State(state): State<AppState>,
    Path(agent_id): Path<String>,
) -> Result<StatusCode, (StatusCode, String)> {
    // Verify agent exists
    {
        let registry = state.agents.read().await;
        registry
            .get(&agent_id)
            .ok_or((StatusCode::NOT_FOUND, "agent not found".to_string()))?;
    }

    // Clear conversation history
    clear_conversation(&state, &agent_id).await;

    push_event(
        &state,
        CrystalBallEvent {
            event_id: next_id(&state, "event"),
            timestamp: now_timestamp(),
            event_type: "orchestration.started".to_string(),
            source_actor: "operator".to_string(),
            source_agent_id: "human".to_string(),
            target_actor: agent_id.clone(),
            target_agent_id: agent_id,
            task_id: "chat".to_string(),
            message: "Agent chat history cleared".to_string(),
            visibility: "operator".to_string(),
        },
    )
    .await;

    Ok(StatusCode::NO_CONTENT)
}

async fn stop_agent(
    State(state): State<AppState>,
    Path(agent_id): Path<String>,
) -> Result<Json<SubAgent>, (StatusCode, String)> {
    let (agent_name, agent_task, updated) = {
        let mut registry = state.agents.write().await;
        let agent = registry
            .get(&agent_id)
            .ok_or((StatusCode::NOT_FOUND, "agent not found".to_string()))?
            .clone();

        if agent.status != AgentStatus::Done {
            registry
                .set_status(&agent_id, AgentStatus::Blocked, false)
                .map_err(|err| (StatusCode::CONFLICT, err))?;
        }

        let updated = registry
            .get(&agent_id)
            .ok_or((StatusCode::NOT_FOUND, "agent not found".to_string()))?
            .clone();

        (agent.name, agent.task_id, updated)
    };

    bump_conversation_version(&state, &agent_id).await;

    push_event(
        &state,
        CrystalBallEvent {
            event_id: next_id(&state, "event"),
            timestamp: now_timestamp(),
            event_type: "subagent.stopped".to_string(),
            source_actor: "operator".to_string(),
            source_agent_id: "human".to_string(),
            target_actor: agent_name,
            target_agent_id: agent_id,
            task_id: agent_task,
            message: format!("Agent stopped by operator. Status: {:?}", updated.status),
            visibility: "operator".to_string(),
        },
    )
    .await;

    Ok(Json(updated))
}

// ---- GitHub Integration Endpoints ----

#[derive(Debug, Deserialize)]
struct GitHubReposQuery {
    limit: Option<u16>,
}

#[derive(Debug, Serialize)]
struct GitHubStatusResponse {
    authenticated: bool,
    host: String,
    login: Option<String>,
    token_source: Option<String>,
    scopes: Vec<String>,
    git_protocol: Option<String>,
    error: Option<String>,
}

#[derive(Debug, Serialize)]
struct GitHubRepoSummary {
    name_with_owner: String,
    is_private: bool,
    updated_at: String,
    url: String,
    viewer_permission: String,
}

#[derive(Debug, Serialize)]
struct GitHubReposResponse {
    connected: bool,
    repos: Vec<GitHubRepoSummary>,
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GhAuthStatusPayload {
    hosts: HashMap<String, Vec<GhAuthHostStatus>>,
}

#[derive(Debug, Deserialize)]
struct GhAuthHostStatus {
    state: String,
    active: bool,
    host: String,
    login: Option<String>,
    #[serde(rename = "tokenSource")]
    token_source: Option<String>,
    scopes: Option<String>,
    #[serde(rename = "gitProtocol")]
    git_protocol: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GhRepoPayload {
    #[serde(rename = "nameWithOwner")]
    name_with_owner: String,
    #[serde(rename = "isPrivate")]
    is_private: bool,
    #[serde(rename = "updatedAt")]
    updated_at: String,
    url: String,
    #[serde(rename = "viewerPermission")]
    viewer_permission: Option<String>,
}

async fn run_gh_command(args: &[String]) -> Result<String, String> {
    let mut command = Command::new("gh");
    for arg in args {
        command.arg(arg);
    }

    let output = command
        .output()
        .await
        .map_err(|e| format!("Failed to run gh command: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let details = if !stderr.is_empty() {
            stderr
        } else if !stdout.is_empty() {
            stdout
        } else {
            format!("gh exited with status {}", output.status)
        };
        return Err(details);
    }

    String::from_utf8(output.stdout).map_err(|e| format!("gh output was not valid UTF-8: {e}"))
}

async fn github_status(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<GitHubStatusResponse>, (StatusCode, String)> {
    require_admin_access(&state, &headers, "GET /api/github/status")?;

    let args = vec![
        "auth".to_string(),
        "status".to_string(),
        "--json".to_string(),
        "hosts".to_string(),
    ];

    let output = match run_gh_command(&args).await {
        Ok(out) => out,
        Err(err) => {
            return Ok(Json(GitHubStatusResponse {
                authenticated: false,
                host: "github.com".to_string(),
                login: None,
                token_source: None,
                scopes: Vec::new(),
                git_protocol: None,
                error: Some(err),
            }));
        }
    };

    let parsed = match serde_json::from_str::<GhAuthStatusPayload>(&output) {
        Ok(v) => v,
        Err(err) => {
            return Ok(Json(GitHubStatusResponse {
                authenticated: false,
                host: "github.com".to_string(),
                login: None,
                token_source: None,
                scopes: Vec::new(),
                git_protocol: None,
                error: Some(format!("Failed to parse gh auth status JSON: {err}")),
            }));
        }
    };

    let primary = parsed
        .hosts
        .values()
        .flat_map(|items| items.iter())
        .find(|item| item.active)
        .or_else(|| {
            parsed
                .hosts
                .values()
                .flat_map(|items| items.iter())
                .find(|item| item.state.eq_ignore_ascii_case("success"))
        })
        .or_else(|| parsed.hosts.values().flat_map(|items| items.iter()).next());

    if let Some(host) = primary {
        let scopes = host
            .scopes
            .as_deref()
            .unwrap_or_default()
            .split(',')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .collect::<Vec<_>>();

        return Ok(Json(GitHubStatusResponse {
            authenticated: host.state.eq_ignore_ascii_case("success"),
            host: host.host.clone(),
            login: host.login.clone(),
            token_source: host.token_source.clone(),
            scopes,
            git_protocol: host.git_protocol.clone(),
            error: None,
        }));
    }

    Ok(Json(GitHubStatusResponse {
        authenticated: false,
        host: "github.com".to_string(),
        login: None,
        token_source: None,
        scopes: Vec::new(),
        git_protocol: None,
        error: Some("gh auth status returned no host entries".to_string()),
    }))
}

async fn github_repos(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<GitHubReposQuery>,
) -> Result<Json<GitHubReposResponse>, (StatusCode, String)> {
    require_admin_access(&state, &headers, "GET /api/github/repos")?;

    let limit = query.limit.unwrap_or(80).clamp(1, 200);
    let args = vec![
        "repo".to_string(),
        "list".to_string(),
        "--json".to_string(),
        "nameWithOwner,isPrivate,updatedAt,url,viewerPermission".to_string(),
        "--limit".to_string(),
        limit.to_string(),
    ];

    let output = match run_gh_command(&args).await {
        Ok(out) => out,
        Err(err) => {
            return Ok(Json(GitHubReposResponse {
                connected: false,
                repos: Vec::new(),
                error: Some(err),
            }));
        }
    };

    let parsed = match serde_json::from_str::<Vec<GhRepoPayload>>(&output) {
        Ok(items) => items,
        Err(err) => {
            return Ok(Json(GitHubReposResponse {
                connected: false,
                repos: Vec::new(),
                error: Some(format!("Failed to parse gh repo list JSON: {err}")),
            }));
        }
    };

    let repos = parsed
        .into_iter()
        .map(|repo| GitHubRepoSummary {
            name_with_owner: repo.name_with_owner,
            is_private: repo.is_private,
            updated_at: repo.updated_at,
            url: repo.url,
            viewer_permission: repo
                .viewer_permission
                .unwrap_or_else(|| "UNKNOWN".to_string()),
        })
        .collect();

    Ok(Json(GitHubReposResponse {
        connected: true,
        repos,
        error: None,
    }))
}

// ---- Secret Vault Endpoints ----

async fn get_vault_status(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<VaultStatus>, (StatusCode, String)> {
    require_admin_access(&state, &headers, "GET /api/vault/status")?;
    Ok(Json(state.vault_status.read().await.clone()))
}

fn require_vault(state: &AppState) -> Result<&SecretVault, (StatusCode, String)> {
    state.vault.as_ref().as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "Secret vault is not available. Check /api/vault/status for diagnostics.".to_string(),
    ))
}

async fn list_secrets(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<SecretMetadata>>, (StatusCode, String)> {
    require_admin_access(&state, &headers, "GET /api/secrets")?;
    let vault = require_vault(&state)?;
    Ok(Json(vault.list().await))
}

#[derive(Debug, Deserialize)]
struct StoreSecretRequest {
    value: String,
    #[serde(default = "default_api_key_type")]
    secret_type: String,
}

fn default_api_key_type() -> String {
    "api_key".to_string()
}

async fn store_secret(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(provider): Path<String>,
    Json(request): Json<StoreSecretRequest>,
) -> Result<Json<SecretMetadata>, (StatusCode, String)> {
    require_admin_access(&state, &headers, "PUT /api/secrets/{provider}")?;
    let vault = require_vault(&state)?;
    let meta = vault
        .store(&provider, &request.value, &request.secret_type)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;

    push_event(
        &state,
        CrystalBallEvent {
            event_id: next_id(&state, "event"),
            timestamp: now_timestamp(),
            event_type: "orchestration.started".to_string(),
            source_actor: "operator".to_string(),
            source_agent_id: "human".to_string(),
            target_actor: "vault".to_string(),
            target_agent_id: "system".to_string(),
            task_id: "credentials".to_string(),
            message: format!("Credential stored for provider '{}'", provider),
            visibility: "admin".to_string(),
        },
    )
    .await;

    if provider.eq_ignore_ascii_case("mattermost") {
        refresh_crystal_ball_client(&state).await;
    }

    Ok(Json(meta))
}

async fn revoke_secret(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(provider): Path<String>,
) -> Result<StatusCode, (StatusCode, String)> {
    require_admin_access(&state, &headers, "DELETE /api/secrets/{provider}")?;
    let vault = require_vault(&state)?;
    vault
        .revoke(&provider)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;

    push_event(
        &state,
        CrystalBallEvent {
            event_id: next_id(&state, "event"),
            timestamp: now_timestamp(),
            event_type: "orchestration.started".to_string(),
            source_actor: "operator".to_string(),
            source_agent_id: "human".to_string(),
            target_actor: "vault".to_string(),
            target_agent_id: "system".to_string(),
            task_id: "credentials".to_string(),
            message: format!("Credential revoked for provider '{}'", provider),
            visibility: "admin".to_string(),
        },
    )
    .await;

    if provider.eq_ignore_ascii_case("mattermost") {
        refresh_crystal_ball_client(&state).await;
    }

    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize)]
struct SecretTestResult {
    provider: String,
    configured: bool,
    test_passed: bool,
    error: Option<String>,
}

async fn test_secret(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(provider): Path<String>,
) -> Result<Json<SecretTestResult>, (StatusCode, String)> {
    require_admin_access(&state, &headers, "POST /api/secrets/{provider}/test")?;
    let vault = require_vault(&state)?;

    if !vault.has(&provider).await {
        return Ok(Json(SecretTestResult {
            provider,
            configured: false,
            test_passed: false,
            error: Some("No credential stored for this provider".to_string()),
        }));
    }

    // Decrypt to verify the key is valid ciphertext (integrity check).
    // Actual provider API validation would go here in production.
    match vault.decrypt(&provider).await {
        Ok(_) => Ok(Json(SecretTestResult {
            provider,
            configured: true,
            test_passed: true,
            error: None,
        })),
        Err(e) => Ok(Json(SecretTestResult {
            provider,
            configured: true,
            test_passed: false,
            error: Some(e),
        })),
    }
}

/// Response for the secure key-use endpoint.
/// Returns the decrypted key for internal localhost-only use.
#[derive(Serialize)]
struct SecretUseResponse {
    provider: String,
    key: String,
}

/// Secure endpoint to retrieve decrypted key for internal use.
/// Requires admin token and localhost origin for security.
async fn use_secret(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(provider): Path<String>,
) -> Result<Json<SecretUseResponse>, (StatusCode, String)> {
    require_admin_access(&state, &headers, "GET /api/secrets/{provider}/use")?;
    let vault = require_vault(&state)?;

    if !vault.has(&provider).await {
        return Err((
            StatusCode::NOT_FOUND,
            format!("No credential stored for provider: {}", provider),
        ));
    }

    // Decrypt the key
    let key = vault
        .decrypt(&provider)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;

    // Log access to Crystal Ball audit trail
    push_event(
        &state,
        CrystalBallEvent {
            event_id: next_id(&state, "event"),
            timestamp: now_timestamp(),
            event_type: "vault.key_used".to_string(),
            source_actor: "operator".to_string(),
            source_agent_id: "human".to_string(),
            target_actor: "vault".to_string(),
            target_agent_id: "system".to_string(),
            task_id: "credentials".to_string(),
            message: format!(
                "Decrypted key retrieved for provider '{}' via /use endpoint",
                provider
            ),
            visibility: "admin".to_string(),
        },
    )
    .await;

    Ok(Json(SecretUseResponse { provider, key }))
}

// ---- OAuth Framework Endpoints ----

#[derive(Serialize)]
struct OAuthStartResponse {
    provider: String,
    redirect_url: String,
    state_token: String,
}

#[derive(Serialize)]
struct OAuthStatusResponse {
    provider: String,
    supported: bool,
    connected: bool,
    access_token_configured: bool,
    refresh_token_configured: bool,
    message: String,
}

fn canonical_oauth_provider(provider: &str) -> String {
    match provider.trim().to_lowercase().as_str() {
        "openai" | "gpt" | "codex" => "openai".to_string(),
        "anthropic" | "claude" => "anthropic".to_string(),
        "gemini" | "google" | "googleai" => "gemini".to_string(),
        "nvidia" | "nim" => "nvidia".to_string(),
        "opencode" => "opencode".to_string(),
        other => other.to_string(),
    }
}

fn oauth_supported(provider: &str) -> bool {
    matches!(provider, "openai" | "anthropic")
}

async fn oauth_status(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(provider): Path<String>,
) -> Result<Json<OAuthStatusResponse>, (StatusCode, String)> {
    require_admin_access(&state, &headers, "GET /api/oauth/{provider}/status")?;

    let provider = canonical_oauth_provider(&provider);
    let supported = oauth_supported(&provider);
    let vault = require_vault(&state)?;

    let access_key = format!("{provider}_oauth_access");
    let refresh_key = format!("{provider}_oauth_refresh");
    let access_token_configured = vault.has(&access_key).await;
    let refresh_token_configured = vault.has(&refresh_key).await;
    let connected = access_token_configured || refresh_token_configured;

    let message = if connected {
        "OAuth tokens are stored in encrypted vault".to_string()
    } else if supported {
        "Provider supports OAuth but no tokens are connected".to_string()
    } else {
        "OAuth is not supported for this provider in current release".to_string()
    };

    Ok(Json(OAuthStatusResponse {
        provider,
        supported,
        connected,
        access_token_configured,
        refresh_token_configured,
        message,
    }))
}

async fn oauth_start(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(provider): Path<String>,
) -> Result<Json<OAuthStartResponse>, (StatusCode, String)> {
    require_admin_access(&state, &headers, "GET /api/oauth/{provider}/start")?;
    let provider = canonical_oauth_provider(&provider);

    if !oauth_supported(&provider) {
        return Err((
            StatusCode::BAD_REQUEST,
            format!(
                "OAuth is not available for provider '{}'. Use API key credentials in Settings.",
                provider
            ),
        ));
    }

    Err((
        StatusCode::NOT_IMPLEMENTED,
        format!(
            "OAuth start for '{}' is scaffolded but not configured. Set provider OAuth client env vars before enabling.",
            provider
        ),
    ))
}

async fn oauth_callback(
    State(_state): State<AppState>,
    Path(provider): Path<String>,
    Query(_params): Query<HashMap<String, String>>,
) -> Result<StatusCode, (StatusCode, String)> {
    let provider = canonical_oauth_provider(&provider);

    if !oauth_supported(&provider) {
        return Err((
            StatusCode::BAD_REQUEST,
            format!(
                "OAuth callback is not supported for provider '{}'.",
                provider
            ),
        ));
    }

    Err((
        StatusCode::NOT_IMPLEMENTED,
        format!("OAuth callback for '{}' is not yet configured.", provider),
    ))
}

async fn oauth_refresh(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(provider): Path<String>,
) -> Result<StatusCode, (StatusCode, String)> {
    require_admin_access(&state, &headers, "POST /api/oauth/{provider}/refresh")?;
    let provider = canonical_oauth_provider(&provider);

    if !oauth_supported(&provider) {
        return Err((
            StatusCode::BAD_REQUEST,
            format!(
                "OAuth refresh is not supported for provider '{}'.",
                provider
            ),
        ));
    }

    Err((
        StatusCode::NOT_IMPLEMENTED,
        format!("OAuth refresh for '{}' is not yet configured.", provider),
    ))
}

async fn oauth_disconnect(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(provider): Path<String>,
) -> Result<StatusCode, (StatusCode, String)> {
    require_admin_access(&state, &headers, "DELETE /api/oauth/{provider}")?;
    let provider = canonical_oauth_provider(&provider);

    let vault = require_vault(&state)?;
    let access_key = format!("{provider}_oauth_access");
    let refresh_key = format!("{provider}_oauth_refresh");
    vault.revoke(&access_key).await.ok();
    vault.revoke(&refresh_key).await.ok();

    push_event(
        &state,
        CrystalBallEvent {
            event_id: next_id(&state, "event"),
            timestamp: now_timestamp(),
            event_type: "orchestration.started".to_string(),
            source_actor: "operator".to_string(),
            source_agent_id: "human".to_string(),
            target_actor: "vault".to_string(),
            target_agent_id: "system".to_string(),
            task_id: "credentials".to_string(),
            message: format!("OAuth disconnected for provider '{}'", provider),
            visibility: "admin".to_string(),
        },
    )
    .await;

    Ok(StatusCode::NO_CONTENT)
}

// ---- WebKeys Google OAuth (multi-account, vault-backed) ----

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GoogleOAuthAccountRecord {
    account_id: String,
    email: Option<String>,
    access_token: String,
    refresh_token: Option<String>,
    scope: Option<String>,
    token_type: Option<String>,
    expires_at: Option<String>,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GoogleOAuthAccountPublic {
    account_id: String,
    email: Option<String>,
    scope: Option<String>,
    expires_at: Option<String>,
    updated_at: String,
    has_refresh_token: bool,
}

impl From<&GoogleOAuthAccountRecord> for GoogleOAuthAccountPublic {
    fn from(record: &GoogleOAuthAccountRecord) -> Self {
        Self {
            account_id: record.account_id.clone(),
            email: record.email.clone(),
            scope: record.scope.clone(),
            expires_at: record.expires_at.clone(),
            updated_at: record.updated_at.clone(),
            has_refresh_token: record.refresh_token.is_some(),
        }
    }
}

#[derive(Debug, Serialize)]
struct GoogleOAuthStartResponse {
    provider: String,
    redirect_url: String,
    state_token: String,
    redirect_uri: String,
}

#[derive(Debug, Serialize)]
struct GoogleOAuthStatusResponse {
    provider: String,
    connected: bool,
    account_count: usize,
    accounts: Vec<GoogleOAuthAccountPublic>,
}

#[derive(Debug, Serialize)]
struct GoogleOAuthCallbackResponse {
    provider: String,
    connected: bool,
    account: GoogleOAuthAccountPublic,
    message: String,
}

#[derive(Debug, Deserialize)]
struct GoogleOAuthCallbackQuery {
    code: Option<String>,
    state: Option<String>,
    scope: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GoogleTokenExchangeResponse {
    access_token: String,
    refresh_token: Option<String>,
    scope: Option<String>,
    token_type: Option<String>,
    expires_in: Option<i64>,
    id_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GoogleTokenErrorResponse {
    error: Option<String>,
    error_description: Option<String>,
}

fn random_urlsafe_string(bytes_len: usize) -> String {
    let mut bytes = vec![0u8; bytes_len];
    rand::thread_rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

fn pkce_s256_challenge(code_verifier: &str) -> String {
    let digest = Sha256::digest(code_verifier.as_bytes());
    URL_SAFE_NO_PAD.encode(digest)
}

fn google_oauth_redirect_uri() -> String {
    if let Ok(uri) = std::env::var("KAIZEN_GOOGLE_OAUTH_REDIRECT_URI") {
        let trimmed = uri.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }

    if let Ok(base) = std::env::var("KAIZEN_PUBLIC_BASE_URL") {
        let trimmed = base.trim().trim_end_matches('/');
        if !trimmed.is_empty() {
            return format!("{trimmed}/api/webkeys/oauth/google/callback");
        }
    }

    let port = std::env::var("ZEROCLAW_PORT").unwrap_or_else(|_| "9100".to_string());
    format!("http://127.0.0.1:{port}/api/webkeys/oauth/google/callback")
}

fn decode_google_id_token_claims(id_token: &str) -> Option<serde_json::Value> {
    let payload = id_token.split('.').nth(1)?;
    let decoded = URL_SAFE_NO_PAD.decode(payload).ok()?;
    serde_json::from_slice(&decoded).ok()
}

async fn load_google_oauth_client_id(state: &AppState) -> Result<String, (StatusCode, String)> {
    if let Ok(value) = std::env::var("KAIZEN_GOOGLE_OAUTH_CLIENT_ID") {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return Ok(trimmed.to_string());
        }
    }

    let vault = require_vault(state)?;
    if vault.has(GOOGLE_OAUTH_CLIENT_ID_VAULT_KEY).await {
        let client_id = vault
            .decrypt(GOOGLE_OAUTH_CLIENT_ID_VAULT_KEY)
            .await
            .map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!(
                        "Failed to decrypt '{}' from vault: {e}",
                        GOOGLE_OAUTH_CLIENT_ID_VAULT_KEY
                    ),
                )
            })?;
        let trimmed = client_id.trim().to_string();
        if !trimmed.is_empty() {
            return Ok(trimmed);
        }
    }

    Err((
        StatusCode::BAD_REQUEST,
        format!(
            "Google OAuth client ID is not configured. Set KAIZEN_GOOGLE_OAUTH_CLIENT_ID or store '{}' in vault.",
            GOOGLE_OAUTH_CLIENT_ID_VAULT_KEY
        ),
    ))
}

async fn load_google_oauth_client_secret(
    state: &AppState,
) -> Result<Option<String>, (StatusCode, String)> {
    if let Ok(value) = std::env::var("KAIZEN_GOOGLE_OAUTH_CLIENT_SECRET") {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return Ok(Some(trimmed.to_string()));
        }
    }

    let Some(vault) = state.vault.as_ref().as_ref() else {
        return Ok(None);
    };

    if !vault.has(GOOGLE_OAUTH_CLIENT_SECRET_VAULT_KEY).await {
        return Ok(None);
    }

    let secret = vault
        .decrypt(GOOGLE_OAUTH_CLIENT_SECRET_VAULT_KEY)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!(
                    "Failed to decrypt '{}' from vault: {e}",
                    GOOGLE_OAUTH_CLIENT_SECRET_VAULT_KEY
                ),
            )
        })?;
    Ok(Some(secret))
}

async fn load_google_oauth_accounts(
    vault: &SecretVault,
) -> Result<HashMap<String, GoogleOAuthAccountRecord>, (StatusCode, String)> {
    if !vault.has(GOOGLE_OAUTH_ACCOUNTS_VAULT_KEY).await {
        return Ok(HashMap::new());
    }

    let raw = vault
        .decrypt(GOOGLE_OAUTH_ACCOUNTS_VAULT_KEY)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!(
                    "Failed to decrypt '{}' from vault: {e}",
                    GOOGLE_OAUTH_ACCOUNTS_VAULT_KEY
                ),
            )
        })?;

    if raw.trim().is_empty() {
        return Ok(HashMap::new());
    }

    serde_json::from_str::<HashMap<String, GoogleOAuthAccountRecord>>(&raw).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!(
                "Failed to parse '{}' payload from vault: {e}",
                GOOGLE_OAUTH_ACCOUNTS_VAULT_KEY
            ),
        )
    })
}

async fn save_google_oauth_accounts(
    vault: &SecretVault,
    accounts: &HashMap<String, GoogleOAuthAccountRecord>,
) -> Result<(), (StatusCode, String)> {
    if accounts.is_empty() {
        let _ = vault.revoke(GOOGLE_OAUTH_ACCOUNTS_VAULT_KEY).await;
        return Ok(());
    }

    let payload = serde_json::to_string(accounts).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to serialize Google OAuth account map: {e}"),
        )
    })?;

    vault
        .store(
            GOOGLE_OAUTH_ACCOUNTS_VAULT_KEY,
            &payload,
            "oauth_google_accounts",
        )
        .await
        .map(|_| ())
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!(
                    "Failed to persist '{}' to vault: {e}",
                    GOOGLE_OAUTH_ACCOUNTS_VAULT_KEY
                ),
            )
        })
}

fn prune_expired_google_oauth_state(map: &mut HashMap<String, PendingGoogleOAuthSession>) {
    let now = chrono::Utc::now().timestamp();
    map.retain(|_, session| (now - session.created_at_unix) <= GOOGLE_OAUTH_STATE_TTL_SECS);
}

async fn webkeys_google_oauth_start(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<GoogleOAuthStartResponse>, (StatusCode, String)> {
    require_admin_access(&state, &headers, "POST /api/webkeys/oauth/google/start")?;

    let client_id = load_google_oauth_client_id(&state).await?;
    let redirect_uri = google_oauth_redirect_uri();

    let state_token = random_urlsafe_string(24);
    let code_verifier = random_urlsafe_string(64);
    let code_challenge = pkce_s256_challenge(&code_verifier);

    {
        let mut pending = state.google_oauth_pending.write().await;
        prune_expired_google_oauth_state(&mut pending);
        pending.insert(
            state_token.clone(),
            PendingGoogleOAuthSession {
                code_verifier,
                redirect_uri: redirect_uri.clone(),
                created_at_unix: chrono::Utc::now().timestamp(),
            },
        );
    }

    let mut authorize_url = reqwest::Url::parse("https://accounts.google.com/o/oauth2/v2/auth")
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to construct Google authorize URL: {e}"),
            )
        })?;

    authorize_url
        .query_pairs_mut()
        .append_pair("client_id", &client_id)
        .append_pair("redirect_uri", &redirect_uri)
        .append_pair("response_type", "code")
        .append_pair("scope", "openid email profile")
        .append_pair("access_type", "offline")
        .append_pair("include_granted_scopes", "true")
        .append_pair("prompt", "consent")
        .append_pair("state", &state_token)
        .append_pair("code_challenge", &code_challenge)
        .append_pair("code_challenge_method", "S256");

    Ok(Json(GoogleOAuthStartResponse {
        provider: "google".to_string(),
        redirect_url: authorize_url.to_string(),
        state_token,
        redirect_uri,
    }))
}

async fn webkeys_google_oauth_callback(
    State(state): State<AppState>,
    Query(params): Query<GoogleOAuthCallbackQuery>,
) -> Result<Json<GoogleOAuthCallbackResponse>, (StatusCode, String)> {
    if let Some(error) = params.error {
        return Err((
            StatusCode::BAD_REQUEST,
            format!(
                "Google OAuth callback error: {}{}",
                error,
                params
                    .error_description
                    .as_ref()
                    .map(|d| format!(" ({d})"))
                    .unwrap_or_default()
            ),
        ));
    }

    let code = params.code.ok_or((
        StatusCode::BAD_REQUEST,
        "Missing 'code' in Google OAuth callback".to_string(),
    ))?;
    let state_token = params.state.ok_or((
        StatusCode::BAD_REQUEST,
        "Missing 'state' in Google OAuth callback".to_string(),
    ))?;

    let pending = {
        let mut map = state.google_oauth_pending.write().await;
        prune_expired_google_oauth_state(&mut map);
        map.remove(&state_token).ok_or((
            StatusCode::BAD_REQUEST,
            "Invalid or expired OAuth state token".to_string(),
        ))?
    };

    let client_id = load_google_oauth_client_id(&state).await?;
    let client_secret = load_google_oauth_client_secret(&state).await?;

    let mut form_params = vec![
        ("code", code.as_str()),
        ("client_id", client_id.as_str()),
        ("redirect_uri", pending.redirect_uri.as_str()),
        ("grant_type", "authorization_code"),
        ("code_verifier", pending.code_verifier.as_str()),
    ];

    if let Some(secret) = client_secret.as_ref() {
        form_params.push(("client_secret", secret.as_str()));
    }

    let token_resp = reqwest::Client::new()
        .post("https://oauth2.googleapis.com/token")
        .form(&form_params)
        .send()
        .await
        .map_err(|e| {
            (
                StatusCode::BAD_GATEWAY,
                format!("Google OAuth token exchange request failed: {e}"),
            )
        })?;

    if !token_resp.status().is_success() {
        let status = token_resp.status();
        let body = token_resp.text().await.unwrap_or_default();
        if let Ok(parsed) = serde_json::from_str::<GoogleTokenErrorResponse>(&body) {
            let err = parsed.error.unwrap_or_else(|| "unknown_error".to_string());
            let desc = parsed.error_description.unwrap_or_default();
            return Err((
                StatusCode::BAD_GATEWAY,
                format!("Google OAuth token exchange failed ({status}): {err} {desc}"),
            ));
        }
        return Err((
            StatusCode::BAD_GATEWAY,
            format!("Google OAuth token exchange failed ({status}): {body}"),
        ));
    }

    let token_payload = token_resp
        .json::<GoogleTokenExchangeResponse>()
        .await
        .map_err(|e| {
            (
                StatusCode::BAD_GATEWAY,
                format!("Failed to parse Google OAuth token response: {e}"),
            )
        })?;

    let claims = token_payload
        .id_token
        .as_deref()
        .and_then(decode_google_id_token_claims);
    let account_id = claims
        .as_ref()
        .and_then(|v| v.get("sub"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("google-{}", &state_token[..state_token.len().min(12)]));
    let email = claims
        .as_ref()
        .and_then(|v| v.get("email"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let now = chrono::Utc::now();
    let expires_at = token_payload
        .expires_in
        .map(|secs| (now + chrono::Duration::seconds(secs)).to_rfc3339());

    let vault = require_vault(&state)?;
    let mut accounts = load_google_oauth_accounts(vault).await?;
    let existing_created_at = accounts.get(&account_id).map(|v| v.created_at.clone());

    let record = GoogleOAuthAccountRecord {
        account_id: account_id.clone(),
        email,
        access_token: token_payload.access_token,
        refresh_token: token_payload.refresh_token,
        scope: token_payload.scope.or(params.scope),
        token_type: token_payload.token_type,
        expires_at,
        created_at: existing_created_at.unwrap_or_else(|| now.to_rfc3339()),
        updated_at: now.to_rfc3339(),
    };

    accounts.insert(account_id.clone(), record.clone());
    save_google_oauth_accounts(vault, &accounts).await?;

    push_event(
        &state,
        CrystalBallEvent {
            event_id: next_id(&state, "event"),
            timestamp: now_timestamp(),
            event_type: "oauth.connected".to_string(),
            source_actor: "operator".to_string(),
            source_agent_id: "human".to_string(),
            target_actor: "vault".to_string(),
            target_agent_id: "system".to_string(),
            task_id: "credentials".to_string(),
            message: format!("Google OAuth connected account '{}'", account_id),
            visibility: "admin".to_string(),
        },
    )
    .await;

    Ok(Json(GoogleOAuthCallbackResponse {
        provider: "google".to_string(),
        connected: true,
        account: GoogleOAuthAccountPublic::from(&record),
        message: "Google OAuth account connected and stored in encrypted vault".to_string(),
    }))
}

async fn webkeys_google_oauth_status(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<GoogleOAuthStatusResponse>, (StatusCode, String)> {
    require_admin_access(&state, &headers, "GET /api/webkeys/oauth/google/status")?;

    let vault = require_vault(&state)?;
    let accounts = load_google_oauth_accounts(vault).await?;

    let mut public_accounts = accounts
        .values()
        .map(GoogleOAuthAccountPublic::from)
        .collect::<Vec<_>>();
    public_accounts.sort_by(|a, b| {
        let ka = a
            .email
            .as_deref()
            .unwrap_or(a.account_id.as_str())
            .to_ascii_lowercase();
        let kb = b
            .email
            .as_deref()
            .unwrap_or(b.account_id.as_str())
            .to_ascii_lowercase();
        ka.cmp(&kb)
    });

    Ok(Json(GoogleOAuthStatusResponse {
        provider: "google".to_string(),
        connected: !public_accounts.is_empty(),
        account_count: public_accounts.len(),
        accounts: public_accounts,
    }))
}

async fn webkeys_google_oauth_disconnect(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(account_id): Path<String>,
) -> Result<StatusCode, (StatusCode, String)> {
    require_admin_access(
        &state,
        &headers,
        "DELETE /api/webkeys/oauth/google/{account_id}",
    )?;

    let account_id = account_id.trim();
    if account_id.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "account_id cannot be empty".to_string(),
        ));
    }

    let vault = require_vault(&state)?;
    let mut accounts = load_google_oauth_accounts(vault).await?;
    if accounts.remove(account_id).is_none() {
        return Err((
            StatusCode::NOT_FOUND,
            format!("Google OAuth account '{account_id}' was not found"),
        ));
    }

    save_google_oauth_accounts(vault, &accounts).await?;

    push_event(
        &state,
        CrystalBallEvent {
            event_id: next_id(&state, "event"),
            timestamp: now_timestamp(),
            event_type: "oauth.disconnected".to_string(),
            source_actor: "operator".to_string(),
            source_agent_id: "human".to_string(),
            target_actor: "vault".to_string(),
            target_agent_id: "system".to_string(),
            task_id: "credentials".to_string(),
            message: format!("Google OAuth disconnected account '{}'", account_id),
            visibility: "admin".to_string(),
        },
    )
    .await;

    Ok(StatusCode::NO_CONTENT)
}

// ---- Events ----

#[derive(Debug, Deserialize)]
struct EventsQuery {
    limit: Option<usize>,
}

async fn list_events(
    State(state): State<AppState>,
    Query(query): Query<EventsQuery>,
) -> Json<Vec<CrystalBallEvent>> {
    let crystal_ball_enabled = state.settings.read().await.crystal_ball_enabled;
    if !crystal_ball_enabled {
        return Json(Vec::new());
    }

    let limit = query.limit.unwrap_or(100).clamp(1, 500);

    let mut merged = {
        let events = state.events.read().await;
        events.clone()
    };

    let crystal_ball = state.crystal_ball.read().await.clone();
    if let Some(client) = crystal_ball {
        match client.fetch_recent_events(limit).await {
            Ok(remote_events) => {
                let mut known = merged
                    .iter()
                    .map(|event| event.event_id.clone())
                    .collect::<HashSet<_>>();

                for event in remote_events {
                    if known.insert(event.event_id.clone()) {
                        merged.push(event);
                    }
                }
            }
            Err(err) => {
                tracing::warn!("Failed to fetch Crystal Ball Mattermost events: {}", err);
            }
        }
    }

    merged.sort_by(|a, b| {
        let ta = parse_timestamp_seconds(a.timestamp.as_str()).unwrap_or(0.0);
        let tb = parse_timestamp_seconds(b.timestamp.as_str()).unwrap_or(0.0);
        ta.partial_cmp(&tb).unwrap_or(Ordering::Equal)
    });

    let len = merged.len();
    let start = len.saturating_sub(limit);
    Json(merged[start..].to_vec())
}

#[tokio::main]
async fn main() {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("info".parse().unwrap()))
        .init();

    let settings = KaizenSettings::load_from_workspace();
    let event_archive = EventArchive::from_env();
    if let Err(err) = event_archive.compact() {
        tracing::warn!("Crystal Ball archive compaction failed at startup: {}", err);
    }
    match event_archive.verify_integrity() {
        Ok(report) => {
            if !report.valid {
                tracing::warn!(
                    "Crystal Ball archive integrity check failed at line {:?}: {:?}",
                    report.first_invalid_line,
                    report.reason
                );
            }
        }
        Err(err) => {
            tracing::warn!("Crystal Ball archive integrity check errored: {}", err);
        }
    }
    let archived_events =
        match event_archive.load_recent(LOCAL_EVENT_RETENTION_SECS, MAX_LOCAL_EVENTS) {
            Ok(events) => events,
            Err(err) => {
                tracing::warn!("Failed to load archived Crystal Ball events: {}", err);
                Vec::new()
            }
        };

    let (vault, vault_status) = match SecretVault::from_env_or_bootstrap() {
        Ok((v, status)) => {
            tracing::info!(
                "Secret vault initialized (source={}, path={})",
                status.key_source,
                status.vault_path
            );
            if status.bootstrap_created {
                tracing::info!(
                    "Generated new managed vault key at {}",
                    status.key_path.as_deref().unwrap_or("<unknown key path>")
                );
            }
            (Some(v), status)
        }
        Err(e) => {
            tracing::warn!(
                "Secret vault initialization failed: {}. Credential endpoints will be unavailable.",
                e
            );
            (
                None,
                VaultStatus {
                    available: false,
                    key_source: "unavailable".to_string(),
                    vault_path: std::env::var("KAIZEN_VAULT_PATH")
                        .unwrap_or_else(|_| "../data/vault.json".to_string()),
                    key_path: Some(
                        std::env::var("KAIZEN_VAULT_KEY_PATH")
                            .unwrap_or_else(|_| "../data/vault.key".to_string()),
                    ),
                    bootstrap_created: false,
                    error: Some(e),
                },
            )
        }
    };

    let initial_crystal_ball = build_crystal_ball_client(&settings, vault.as_ref()).await;
    if settings.crystal_ball_enabled && initial_crystal_ball.is_none() {
        tracing::warn!(
            "Crystal Ball enabled but Mattermost client is not configured. Running local feed only."
        );
    }

    // Initialize WebKeys service
    let webkeys = match WebKeysService::new().await {
        Ok(service) => {
            tracing::info!("WebKeys service initialized");
            Some(service)
        }
        Err(e) => {
            tracing::warn!(
                "WebKeys service initialization failed: {}. Virtual key endpoints will be unavailable.",
                e
            );
            None
        }
    };

    // Initialize WebkeysRuntime (chromiumoxide-backed Gemini executor)
    let webkeys_runtime = if webkeys.is_some() && settings.webkeys.enabled {
        let profile_dir = {
            let base = settings.webkeys.profile_dir.clone();
            std::path::PathBuf::from(base).join("gemini")
        };
        let max_restarts = settings.webkeys.max_restarts;
        let default_provider = settings.webkeys.default_provider.clone();
        let default_model = settings.webkeys.default_model.clone();
        let browser = WebkeysBrowserManager::new(profile_dir);
        let health = SessionHealth::new(browser.clone(), max_restarts);
        let executor = GeminiExecutor::new(browser, health);
        let runtime = WebkeysRuntime::new(
            std::sync::Arc::new(executor),
            default_provider,
            default_model,
        );
        tracing::info!("WebkeysRuntime initialized (Gemini executor, chromiumoxide)");
        Some(std::sync::Arc::new(runtime))
    } else {
        None
    };

    let system_prompt = inference::load_system_prompt();
    tracing::info!(
        "Loaded Kaizen system prompt ({} chars)",
        system_prompt.len()
    );

    let admin_api_token = std::env::var("ADMIN_API_TOKEN")
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty());
    if admin_api_token.is_some() {
        tracing::info!("Admin token protection enabled for sensitive API endpoints.");
    }

    let state = AppState {
        settings: Arc::new(RwLock::new(settings.clone())),
        admin_api_token: Arc::new(admin_api_token),
        agents: Arc::new(RwLock::new(AgentRegistry::new(
            settings.max_subagents as usize,
        ))),
        gates: Arc::new(RwLock::new(GateRuntime::default())),
        events: Arc::new(RwLock::new(archived_events)),
        crystal_ball: Arc::new(RwLock::new(initial_crystal_ball)),
        event_archive: Arc::new(event_archive),
        vault: Arc::new(vault),
        vault_status: Arc::new(RwLock::new(vault_status)),
        inference: InferenceClient::new(),
        system_prompt: Arc::new(system_prompt),
        conversations: Arc::new(RwLock::new(HashMap::new())),
        conversation_versions: Arc::new(RwLock::new(HashMap::new())),
        next_id: Arc::new(AtomicU64::new(1)),
        webkeys,
        webkeys_runtime,
        google_oauth_pending: Arc::new(RwLock::new(HashMap::new())),
    };

    let mode = resolve_bind_mode();
    let host = std::env::var("ZEROCLAW_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
    if let Err(err) = enforce_network_policy(&mode, &host) {
        panic!("Network policy validation failed: {err}");
    }

    let port = std::env::var("ZEROCLAW_PORT").unwrap_or_else(|_| "9100".to_string());
    let addr = format!("{host}:{port}");

    // CORS: explicit allowlist only.
    let allowed_origins = std::env::var("KAIZEN_CORS_ORIGINS")
        .unwrap_or_else(|_| "http://localhost:3000,http://127.0.0.1:3000".to_string());
    let origins = parse_cors_origins(&allowed_origins)
        .unwrap_or_else(|err| panic!("CORS configuration invalid: {err}"));

    let cors = CorsLayer::new()
        .allow_origin(origins)
        .allow_methods([
            axum::http::Method::GET,
            axum::http::Method::POST,
            axum::http::Method::PUT,
            axum::http::Method::PATCH,
            axum::http::Method::DELETE,
            axum::http::Method::OPTIONS,
        ])
        .allow_headers([
            axum::http::header::CONTENT_TYPE,
            axum::http::header::AUTHORIZATION,
            HeaderName::from_static("x-admin-token"),
        ]);

    let trace_layer = TraceLayer::new_for_http()
        .make_span_with(|request: &axum::http::Request<Body>| {
            tracing::info_span!(
                "http",
                method = %request.method(),
                uri = %sanitize_uri_for_log(request.uri())
            )
        })
        .on_request(|_request: &axum::http::Request<_>, _span: &tracing::Span| {})
        .on_response(
            |response: &axum::http::Response<_>,
             latency: std::time::Duration,
             span: &tracing::Span| {
                tracing::info!(
                    parent: span,
                    status = %response.status(),
                    latency_ms = latency.as_millis(),
                    "request complete"
                );
            },
        );

    let app = Router::new()
        .route("/health", get(health))
        .route("/api/crystal-ball/health", get(crystal_ball_health))
        .route("/api/crystal-ball/audit", get(crystal_ball_audit))
        .route("/api/crystal-ball/validate", get(crystal_ball_validate))
        .route("/api/crystal-ball/smoke", post(crystal_ball_smoke))
        .route("/api/settings", get(get_settings).patch(patch_settings))
        .route("/api/chat", post(chat))
        .route("/api/chat/history", get(get_chat_history))
        .route("/api/chat/stream", post(chat_stream))
        .route("/api/agents", get(list_agents).post(spawn_agent))
        .route("/api/agents/{agent_id}/status", patch(update_agent_status))
        .route(
            "/api/agents/{agent_id}",
            patch(rename_agent).delete(remove_agent),
        )
        .route("/api/agents/{agent_id}/clear", post(clear_agent_chat))
        .route("/api/agents/{agent_id}/stop", post(stop_agent))
        .route("/api/gates", get(get_gates))
        .route("/api/gates/conditions", patch(patch_gate_conditions))
        .route("/api/gates/advance", post(advance_gates))
        .route("/api/events", get(list_events))
        // GitHub integration endpoints
        .route("/api/github/status", get(github_status))
        .route("/api/github/repos", get(github_repos))
        // Secret vault endpoints
        .route("/api/vault/status", get(get_vault_status))
        .route("/api/secrets", get(list_secrets))
        .route(
            "/api/secrets/{provider}",
            put(store_secret).delete(revoke_secret),
        )
        .route("/api/secrets/{provider}/test", post(test_secret))
        .route("/api/secrets/{provider}/use", get(use_secret))
        // OAuth endpoints
        .route("/api/oauth/{provider}/start", get(oauth_start))
        .route("/api/oauth/{provider}/status", get(oauth_status))
        .route("/api/oauth/{provider}/callback", get(oauth_callback))
        .route("/api/oauth/{provider}/refresh", post(oauth_refresh))
        .route("/api/oauth/{provider}", delete(oauth_disconnect))
        // WebKeys Google OAuth endpoints (multi-account)
        .route(
            "/api/webkeys/oauth/google/start",
            post(webkeys_google_oauth_start),
        )
        .route(
            "/api/webkeys/oauth/google/callback",
            get(webkeys_google_oauth_callback),
        )
        .route(
            "/api/webkeys/oauth/google/status",
            get(webkeys_google_oauth_status),
        )
        .route(
            "/api/webkeys/oauth/google/{account_id}",
            delete(webkeys_google_oauth_disconnect),
        )
        // WebKeys admin routes
        .route(
            "/api/webkeys/keys",
            get(webkeys_list_keys).post(webkeys_create_key),
        )
        .route(
            "/api/webkeys/keys/{id}",
            get(webkeys_get_key)
                .patch(webkeys_update_key)
                .delete(webkeys_delete_key),
        )
        .route("/api/webkeys/keys/{id}/rotate", post(webkeys_rotate_key))
        .route("/api/webkeys/keys/verify", post(webkeys_verify_key))
        .route(
            "/api/webkeys/providers",
            get(webkeys_list_bindings).post(webkeys_create_binding),
        )
        .route(
            "/api/webkeys/providers/{id}",
            get(webkeys_get_binding)
                .patch(webkeys_update_binding)
                .delete(webkeys_delete_binding),
        )
        // WebKeys runtime routes (OpenAI-compatible)
        .route("/v1/chat/completions", post(webkeys_chat_completions))
        .route("/v1/models", get(webkeys_list_models))
        .with_state(state)
        .layer(middleware::from_fn(redact_error_response_middleware))
        .layer(trace_layer)
        .layer(cors);

    tracing::info!("ZeroClaw gateway starting on {} (mode={})", addr, mode);

    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

#[cfg(test)]
mod security_tests {
    use super::*;

    #[test]
    fn normalize_chat_mode_accepts_supported_values() {
        for mode in ["yolo", "build", "plan", "reason", "orchestrator"] {
            let normalized = normalize_chat_mode(Some(mode)).unwrap();
            assert_eq!(normalized.as_deref(), Some(mode));
        }
    }

    #[test]
    fn normalize_chat_mode_rejects_invalid_values() {
        let err = normalize_chat_mode(Some("invalid-mode")).unwrap_err();
        assert_eq!(err.0, StatusCode::BAD_REQUEST);
        assert!(err.1.contains("Unknown mode 'invalid-mode'"));
    }

    #[test]
    fn resolve_provider_override_alias_maps_zeroclaw_names_to_configured_provider() {
        for alias in ["kai-zen", "zeroclaw", "native"] {
            let (provider, is_native_alias) = resolve_provider_override_alias(alias, "openai");
            assert_eq!(provider, "openai");
            assert!(is_native_alias);
        }

        let (provider, is_native_alias) = resolve_provider_override_alias("anthropic", "openai");
        assert_eq!(provider, "anthropic");
        assert!(!is_native_alias);
    }

    #[test]
    fn normalize_chat_targets_filters_invalid_and_dedupes() {
        let targets = vec![
            ChatModelTarget {
                provider: "openai".to_string(),
                model: "gpt-5.3-codex".to_string(),
            },
            ChatModelTarget {
                provider: "openai".to_string(),
                model: "gpt-5.3-codex".to_string(),
            },
            ChatModelTarget {
                provider: " ".to_string(),
                model: "invalid".to_string(),
            },
        ];

        let normalized = normalize_chat_targets(Some(&targets));
        assert_eq!(normalized.len(), 1);
        assert_eq!(normalized[0].0, "openai");
        assert_eq!(normalized[0].1, "gpt-5.3-codex");
    }

    #[test]
    fn parse_cors_origins_rejects_invalid_scheme() {
        let result = parse_cors_origins("ws://localhost:3000");
        assert!(result.is_err());
    }

    #[test]
    fn parse_cors_origins_accepts_valid_list() {
        let result = parse_cors_origins("http://localhost:3000,https://example.com");
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 2);
    }

    #[test]
    fn loopback_policy_blocks_non_loopback_in_native_mode() {
        let result = enforce_network_policy("native", "0.0.0.0");
        assert!(result.is_err());
    }

    #[test]
    fn extract_admin_token_supports_bearer_and_custom_header() {
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::AUTHORIZATION,
            "Bearer token-1".parse().unwrap(),
        );
        assert_eq!(
            extract_presented_admin_token(&headers).as_deref(),
            Some("token-1")
        );

        let mut headers2 = HeaderMap::new();
        headers2.insert("x-admin-token", "token-2".parse().unwrap());
        assert_eq!(
            extract_presented_admin_token(&headers2).as_deref(),
            Some("token-2")
        );
    }

    #[test]
    fn validate_admin_access_rejects_missing_or_bad_tokens() {
        let headers = HeaderMap::new();
        assert!(validate_admin_access(Some("expected"), &headers, "test").is_err());

        let mut headers2 = HeaderMap::new();
        headers2.insert("x-admin-token", "wrong".parse().unwrap());
        assert!(validate_admin_access(Some("expected"), &headers2, "test").is_err());

        headers2.insert("x-admin-token", "expected".parse().unwrap());
        assert!(validate_admin_access(Some("expected"), &headers2, "test").is_ok());
    }

    #[test]
    fn log_uri_sanitizer_redacts_secret_like_content() {
        let uri: Uri = "/api/foo?api_key=sk-test-secret-1234567890"
            .parse()
            .unwrap();
        let sanitized = sanitize_uri_for_log(&uri);
        assert!(!sanitized.contains("sk-test-secret-1234567890"));
    }
}
