//! Kaizen Gateway - Kaizen MAX core runtime
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
    response::sse::{Event, KeepAlive, Sse},
    response::{Html, IntoResponse, Response},
    routing::{delete, get, patch, post},
};
use kaizen_gateway::{
    agents::{AgentRegistry, AgentStatus, Branch, Mission, SubAgent},
    crystal_ball::{
        CrystalBallClient, CrystalBallEvent, MattermostSmokeResult, MattermostValidation,
        redact_sensitive,
    },
    event_archive::{ArchiveIntegrityReport, EventArchive},
    gate_engine::{GateConditionPatch, GateRuntime, GateState, TransitionResult},
    inference::{
        self, AnthropicStreamEvent, ChatMessage as InferenceChatMessage, InferenceClient,
        InferenceCredential, InferenceProvider, InferenceRequest, LiveInferenceEvent,
        OpenAIStreamChunk,
    },
    oauth_store,
    provider_auth::{self, ProviderAuthStatus},
    settings::{KaizenSettings, SettingsPatch},
};
use serde::{Deserialize, Serialize};
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

const LOCAL_EVENT_RETENTION_SECS: f64 = 72.0 * 3600.0;
const MAX_LOCAL_EVENTS: usize = 1000;

#[derive(Clone)]
struct AppState {
    settings: Arc<RwLock<KaizenSettings>>,
    admin_api_token: Arc<Option<String>>,
    agents: Arc<RwLock<AgentRegistry>>,
    gates: Arc<RwLock<GateRuntime>>,
    events: Arc<RwLock<Vec<CrystalBallEvent>>>,
    crystal_ball: Arc<RwLock<Option<CrystalBallClient>>>,
    event_archive: Arc<EventArchive>,
    inference: InferenceClient,
    system_prompt: Arc<String>,
    /// Per-session conversation history (keyed by "kaizen" or agent_id).
    conversations: Arc<RwLock<HashMap<String, Vec<InferenceChatMessage>>>>,
    /// Monotonic generation counters for conversation keys.
    ///
    /// Any clear/remove operation bumps the generation so stale in-flight
    /// inference responses cannot repopulate cleared histories.
    conversation_versions: Arc<RwLock<HashMap<String, u64>>>,
    pending_gemini_oauth: Arc<RwLock<Option<oauth_store::PendingGeminiOAuth>>>,
    next_id: Arc<AtomicU64>,
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

async fn build_crystal_ball_client(settings: &KaizenSettings) -> Option<CrystalBallClient> {
    if !settings.crystal_ball_enabled {
        return None;
    }

    // Vault disconnected - Crystal Ball uses env-only config until vault reconnects.
    CrystalBallClient::from_env()
}

async fn refresh_crystal_ball_client(state: &AppState) {
    let settings = state.settings.read().await.clone();
    let new_client = build_crystal_ball_client(&settings).await;
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

fn env_with_legacy(primary: &str, legacy: &str) -> Option<String> {
    std::env::var(primary)
        .ok()
        .or_else(|| std::env::var(legacy).ok())
}

fn resolve_bind_mode() -> String {
    env_with_legacy("KAIZEN_MODE", "ZEROCLAW_MODE")
        .unwrap_or_else(|| "native".to_string())
        .to_lowercase()
}

fn resolve_bind_host() -> String {
    env_with_legacy("KAIZEN_HOST", "ZEROCLAW_HOST").unwrap_or_else(|| "127.0.0.1".to_string())
}

fn resolve_bind_port() -> String {
    env_with_legacy("KAIZEN_PORT", "ZEROCLAW_PORT").unwrap_or_else(|| "9100".to_string())
}

fn enforce_network_policy(mode: &str, host: &str) -> Result<(), String> {
    match mode {
        "native" | "local" => {
            if !is_loopback_host(host) {
                return Err(format!(
                    "KAIZEN_MODE={mode} requires loopback host. Got KAIZEN_HOST={host}."
                ));
            }
            Ok(())
        }
        "remote" => {
            let ack = env_with_legacy("KAIZEN_REMOTE_SECURITY_ACK", "ZEROCLAW_REMOTE_SECURITY_ACK")
                .unwrap_or_default();
            if ack != "I_UNDERSTAND_REMOTE_REQUIRES_TLS_MTLS_AUTH" {
                return Err(
                    "KAIZEN_MODE=remote requires KAIZEN_REMOTE_SECURITY_ACK=I_UNDERSTAND_REMOTE_REQUIRES_TLS_MTLS_AUTH"
                        .to_string(),
                );
            }

            tracing::warn!(
                "Remote mode enabled. You must enforce TLS/mTLS/auth at the edge (reverse proxy or service mesh)."
            );
            Ok(())
        }
        other => Err(format!(
            "Unsupported KAIZEN_MODE={other}. Use 'native' or 'remote'."
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

        if !(origin.starts_with("http://")
            || origin.starts_with("https://")
            || origin.starts_with("tauri://"))
        {
            return Err(format!(
                "Invalid CORS origin '{origin}'. Origins must start with http://, https://, or tauri://"
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
    let kaizen_native = matches!(
        requested_provider.to_ascii_lowercase().as_str(),
        "zeroclaw" | "kaizen" | "kai-zen" | "native"
    );
    let provider_name = if kaizen_native {
        configured_provider
    } else {
        requested_provider
    };

    (provider_name, kaizen_native)
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

/// Resolve inference settings into provider + model + local credential material.
async fn resolve_inference(
    state: &AppState,
    provider_override: Option<&str>,
    model_override: Option<&str>,
) -> Result<(InferenceProvider, String, InferenceCredential), (StatusCode, String)> {
    let settings = state.settings.read().await;
    let configured_provider = settings.inference_provider.clone();
    let configured_model = settings.inference_model.clone();
    let requested_provider = provider_override
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .unwrap_or(configured_provider.as_str());

    let (provider_name, kaizen_native) =
        resolve_provider_override_alias(requested_provider, configured_provider.as_str());

    let provider = InferenceProvider::from_str_loose(provider_name).ok_or((
        StatusCode::BAD_REQUEST,
        if kaizen_native {
            format!(
                "Zeroclaw is mapped to '{}', but that is not a supported concrete provider. Use openai, anthropic, gemini, gemini-cli, codex-cli, or nvidia in settings.",
                configured_provider
            )
        } else {
            format!(
                "Unknown inference provider '{}'. Use 'kaizen' (legacy alias: 'zeroclaw'), 'anthropic', 'openai', 'gemini', 'gemini-cli', 'codex-cli', or 'nvidia'.",
                requested_provider
            )
        },
    ))?;

    let model = if kaizen_native {
        if configured_model.is_empty() {
            provider.default_model().to_string()
        } else {
            configured_model.clone()
        }
    } else if let Some(m) = model_override.map(str::trim).filter(|v| !v.is_empty()) {
        m.to_string()
    } else if configured_model.is_empty() {
        provider.default_model().to_string()
    } else {
        configured_model
    };
    drop(settings);

    let credential = provider_auth::resolve_credential(provider)
        .await
        .map_err(|reason| {
            (
                StatusCode::SERVICE_UNAVAILABLE,
                format!("Provider '{provider}' is not ready. {reason}"),
            )
        })?;

    Ok((provider, model, credential))
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
                Ok((provider, resolved_model, mut credential)) => {
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

                    let inference_result = state.inference.complete(&credential, &req).await;
                    credential.wipe();

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
            Ok((provider, model, mut credential)) => {
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

                let inference_result = state.inference.complete(&credential, &req).await;
                credential.wipe();

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
                // Fallback: provider auth is not configured - return helpful message
                tracing::warn!("Inference not available: {}", reason);
                (
                    format!(
                        "Kaizen is in offline mode. Open Integrations and configure \
                         OpenAI/Anthropic/NVIDIA API keys, Gemini API key or Google OAuth, or Gemini CLI local OAuth. Reason: {reason}"
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
) -> Result<Response, (StatusCode, String)> {
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

    let (provider, model, mut credential) = resolve_inference(
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

    if matches!(provider, InferenceProvider::CodexCli) {
        let mut live_events = state
            .inference
            .stream_codex_cli_live(&req)
            .map_err(|e| (StatusCode::BAD_GATEWAY, e))?;
        credential.wipe();

        let state_clone = state.clone();
        let conv_key = conversation_key.clone();
        let user_msg = message.clone();
        let provider_name = provider.to_string();

        let stream = async_stream::stream! {
            let mut final_response = String::new();
            let mut done_emitted = false;

            while let Some(event) = live_events.recv().await {
                match event {
                    Ok(LiveInferenceEvent::Token(text)) => {
                        final_response.push_str(&text);
                        let token_event = Event::default()
                            .event("token")
                            .data(serde_json::json!({ "text": text }).to_string());
                        yield Result::<Event, Infallible>::Ok(token_event);
                    }
                    Ok(LiveInferenceEvent::Done { full_response, .. }) => {
                        if !full_response.trim().is_empty() {
                            final_response = full_response;
                        }

                        append_to_conversation(
                            &state_clone,
                            &conv_key,
                            &user_msg,
                            &final_response,
                            expected_conversation_version,
                        ).await;

                        let done_event = Event::default()
                            .event("done")
                            .data(serde_json::json!({
                                "full_response": final_response,
                                "model": model,
                                "provider": provider_name,
                            }).to_string());
                        yield Result::<Event, Infallible>::Ok(done_event);
                        done_emitted = true;
                        break;
                    }
                    Err(error) => {
                        let err_event = Event::default()
                            .event("error")
                            .data(error);
                        yield Result::<Event, Infallible>::Ok(err_event);
                        break;
                    }
                }
            }

            if !done_emitted && !final_response.trim().is_empty() {
                append_to_conversation(
                    &state_clone,
                    &conv_key,
                    &user_msg,
                    &final_response,
                    expected_conversation_version,
                ).await;

                let done_event = Event::default()
                    .event("done")
                    .data(serde_json::json!({
                        "full_response": final_response,
                        "model": model,
                        "provider": provider_name,
                    }).to_string());
                yield Result::<Event, Infallible>::Ok(done_event);
            }
        };

        return Ok(Sse::new(stream).keep_alive(KeepAlive::default()).into_response());
    }

    let raw_response = state
        .inference
        .stream_raw(&credential, &req)
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, e));
    credential.wipe();
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
                    yield Result::<Event, Infallible>::Ok(err_event);
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
                    yield Result::<Event, Infallible>::Ok(done_event);
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
                                        tracing::trace!("Anthropic SSE chunk emitted: {}", text);
                                        let token_event = Event::default()
                                            .event("token")
                                            .data(serde_json::json!({ "text": text }).to_string());
                                        yield Result::<Event, Infallible>::Ok(token_event);
                                    }
                                }
                                AnthropicStreamEvent::MessageStop {} => {
                                    tracing::info!("Stream complete for agent {}", conv_key);
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
                                    yield Result::<Event, Infallible>::Ok(done_event);
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
                                    tracing::trace!("OpenAI/Nvidia SSE chunk emitted: {}", text);
                                    let token_event = Event::default()
                                        .event("token")
                                        .data(serde_json::json!({ "text": text }).to_string());
                                    yield Result::<Event, Infallible>::Ok(token_event);
                                }
                            }
                        }
                    }
                    InferenceProvider::Gemini => {
                        let tokens = parse_gemini_stream_tokens(data);
                        for text in tokens {
                            full_response.push_str(&text);
                            tracing::trace!("Gemini SSE chunk emitted: {}", text);
                            let token_event = Event::default()
                                .event("token")
                                .data(serde_json::json!({ "text": text }).to_string());
                            yield Result::<Event, Infallible>::Ok(token_event);
                        }
                    }
                    InferenceProvider::GeminiCli | InferenceProvider::CodexCli => {
                        // stream_raw currently rejects CLI-backed providers, so this
                        // branch is effectively unreachable for now.
                    }
                }
            }
        }

        if !done_emitted {
            tracing::info!("Stream complete for agent {}", conv_key);
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
            yield Result::<Event, Infallible>::Ok(done_event);
        }
    };

    Ok(Sse::new(stream).keep_alive(KeepAlive::default()).into_response())
}

#[derive(Debug, Deserialize)]
struct SpawnAgentRequest {
    agent_name: String,
    #[serde(default)]
    task_id: Option<String>,
    #[serde(default)]
    mission_id: Option<String>,
    #[serde(default)]
    branch_id: Option<String>,
    objective: String,
    #[serde(default)]
    user_requested: bool,
}

#[derive(Debug, Deserialize)]
struct CreateBranchRequest {
    id: String,
    #[serde(default)]
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CreateMissionRequest {
    id: String,
    branch_id: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    objective: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ListMissionsQuery {
    branch_id: Option<String>,
}

#[derive(Debug, Serialize)]
struct MissionTopologyNode {
    mission: Mission,
    workers: Vec<SubAgent>,
    active_workers: usize,
    blocked_workers: usize,
}

#[derive(Debug, Serialize)]
struct BranchTopologyNode {
    branch: Branch,
    missions: Vec<MissionTopologyNode>,
    total_workers: usize,
    active_workers: usize,
    blocked_workers: usize,
}

fn normalize_scope_id(raw: &str) -> String {
    raw.trim()
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

async fn list_agents(State(state): State<AppState>) -> Json<Vec<SubAgent>> {
    Json(state.agents.read().await.list().to_vec())
}

async fn list_branches(State(state): State<AppState>) -> Json<Vec<Branch>> {
    Json(state.agents.read().await.list_branches().to_vec())
}

async fn create_branch(
    State(state): State<AppState>,
    Json(request): Json<CreateBranchRequest>,
) -> Result<Json<Branch>, (StatusCode, String)> {
    let id = normalize_scope_id(&request.id);
    if id.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "Branch id cannot be empty".to_string(),
        ));
    }

    let name = request
        .name
        .unwrap_or_else(|| id.replace('-', " "))
        .trim()
        .to_string();
    if name.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "Branch name cannot be empty".to_string(),
        ));
    }

    let branch = {
        let mut registry = state.agents.write().await;
        registry
            .create_branch(id, name)
            .map_err(|err| (StatusCode::BAD_REQUEST, err))?
    };

    Ok(Json(branch))
}

async fn list_missions(
    State(state): State<AppState>,
    Query(query): Query<ListMissionsQuery>,
) -> Json<Vec<Mission>> {
    let registry = state.agents.read().await;
    let missions = if let Some(branch_id) = query.branch_id {
        let normalized = normalize_scope_id(&branch_id);
        registry.list_missions_for_branch(normalized.as_str())
    } else {
        registry.list_missions().to_vec()
    };

    Json(missions)
}

async fn create_mission(
    State(state): State<AppState>,
    Json(request): Json<CreateMissionRequest>,
) -> Result<Json<Mission>, (StatusCode, String)> {
    let id = normalize_scope_id(&request.id);
    let branch_id = normalize_scope_id(&request.branch_id);

    if id.is_empty() || branch_id.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "Mission id and branch id are required".to_string(),
        ));
    }

    let name = request.name.unwrap_or_else(|| id.clone());
    let objective = request
        .objective
        .unwrap_or_else(|| "Mission objective pending".to_string());

    let mission = {
        let mut registry = state.agents.write().await;
        registry
            .create_mission(id, branch_id, name, objective)
            .map_err(|err| (StatusCode::BAD_REQUEST, err))?
    };

    Ok(Json(mission))
}

async fn get_topology(State(state): State<AppState>) -> Json<Vec<BranchTopologyNode>> {
    let registry = state.agents.read().await;
    let mut workers_by_scope: HashMap<(String, String), Vec<SubAgent>> = HashMap::new();

    for worker in registry.list() {
        workers_by_scope
            .entry((worker.branch_id.clone(), worker.mission_id.clone()))
            .or_default()
            .push(worker.clone());
    }

    let mut branches = Vec::new();
    for branch in registry.list_branches() {
        let mut mission_nodes = Vec::new();
        for mission in registry.list_missions_for_branch(&branch.id) {
            let workers = workers_by_scope
                .remove(&(branch.id.clone(), mission.id.clone()))
                .unwrap_or_default();

            let active_workers = workers
                .iter()
                .filter(|worker| matches!(worker.status, AgentStatus::Active))
                .count();
            let blocked_workers = workers
                .iter()
                .filter(|worker| matches!(worker.status, AgentStatus::Blocked))
                .count();

            mission_nodes.push(MissionTopologyNode {
                mission,
                workers,
                active_workers,
                blocked_workers,
            });
        }

        let total_workers = mission_nodes.iter().map(|node| node.workers.len()).sum();
        let active_workers = mission_nodes.iter().map(|node| node.active_workers).sum();
        let blocked_workers = mission_nodes.iter().map(|node| node.blocked_workers).sum();

        branches.push(BranchTopologyNode {
            branch: branch.clone(),
            missions: mission_nodes,
            total_workers,
            active_workers,
            blocked_workers,
        });
    }

    Json(branches)
}

async fn spawn_agent(
    State(state): State<AppState>,
    Json(request): Json<SpawnAgentRequest>,
) -> Result<Json<SubAgent>, (StatusCode, String)> {
    let settings = state.settings.read().await.clone();
    if !request.user_requested
        && !(settings.auto_spawn_subagents || settings.orchestrator_full_control)
    {
        return Err((
            StatusCode::FORBIDDEN,
            "Sub-agent spawn denied: explicit user request required or enable orchestrator control"
                .to_string(),
        ));
    }

    let branch_id = request
        .branch_id
        .as_deref()
        .map(normalize_scope_id)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "primary".to_string());

    let mission_id = request
        .mission_id
        .as_deref()
        .map(normalize_scope_id)
        .filter(|value| !value.is_empty())
        .or_else(|| {
            request
                .task_id
                .as_deref()
                .map(normalize_scope_id)
                .filter(|value| !value.is_empty())
        })
        .ok_or((
            StatusCode::BAD_REQUEST,
            "Mission id (or legacy task_id) is required".to_string(),
        ))?;

    let task_id = request
        .task_id
        .clone()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| mission_id.clone());

    let agent_id = next_id(&state, "agent");
    let created = {
        let mut registry = state.agents.write().await;
        let created = registry
            .spawn_scoped(
                agent_id,
                request.agent_name,
                branch_id,
                mission_id,
                task_id,
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
            message: format!(
                "Spawned '{}' in branch '{}' for mission '{}'",
                created.name, created.branch_id, created.mission_id
            ),
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
    conditions: kaizen_gateway::gate_engine::GateConditions,
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

// ---- Provider Auth Endpoints ----

async fn list_provider_statuses(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<ProviderAuthStatus>>, (StatusCode, String)> {
    require_admin_access(&state, &headers, "GET /api/providers/status")?;
    let settings = state.settings.read().await.clone();
    Ok(Json(
        provider_auth::collect_provider_auth_statuses(&settings).await,
    ))
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

#[derive(Debug, Deserialize)]
struct OAuthCallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

fn canonical_oauth_provider(provider: &str) -> String {
    match provider.trim().to_lowercase().as_str() {
        "openai" | "gpt" | "codex" => "openai".to_string(),
        "anthropic" | "claude" => "anthropic".to_string(),
        "gemini" | "google" | "googleai" => "gemini".to_string(),
        "nvidia" | "nim" => "nvidia".to_string(),
        other => other.to_string(),
    }
}

fn oauth_supported(provider: &str) -> bool {
    matches!(provider, "gemini")
}

async fn oauth_status(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(provider): Path<String>,
) -> Result<Json<OAuthStatusResponse>, (StatusCode, String)> {
    require_admin_access(&state, &headers, "GET /api/oauth/{provider}/status")?;

    let provider = canonical_oauth_provider(&provider);
    if provider == "gemini" {
        match oauth_store::stored_gemini_oauth_status() {
            Ok(stored) if stored.present => {
                return Ok(Json(OAuthStatusResponse {
                    provider,
                    supported: true,
                    connected: stored.connected(),
                    access_token_configured: stored.access_token_present,
                    refresh_token_configured: stored.refresh_token_present,
                    message: stored.message,
                }));
            }
            Ok(_) => {}
            Err(error) => {
                return Ok(Json(OAuthStatusResponse {
                    provider,
                    supported: true,
                    connected: false,
                    access_token_configured: false,
                    refresh_token_configured: false,
                    message: format!(
                        "Stored Gemini OAuth session could not be read. Disconnect and reconnect Gemini OAuth. Details: {error}"
                    ),
                }));
            }
        }
    }

    let settings = state.settings.read().await.clone();
    let provider_status = provider_auth::provider_auth_status(&provider, &settings).await;
    let supported = oauth_supported(&provider)
        || provider_status.auth_method == "oauth_access_token_env"
        || provider_status.auth_method == "oauth_adc";
    let (connected, access_token_configured, refresh_token_configured, message) = if provider
        == "gemini"
    {
        match provider_status.auth_method.as_str() {
            "oauth_access_token_env" => (
                provider_status.can_chat,
                true,
                false,
                format!(
                    "{} This OAuth session is managed from the environment, not by the app.",
                    provider_status.message
                ),
            ),
            "oauth_adc" => (
                provider_status.can_chat,
                true,
                true,
                format!(
                    "{} This OAuth session is managed by Google ADC, not by the app.",
                    provider_status.message
                ),
            ),
            "api_key_env" => (
                false,
                false,
                false,
                format!(
                    "{} Gemini is currently using API key auth. Disconnect local OAuth or remove the API key env var if you want app-managed OAuth to take over.",
                    provider_status.message
                ),
            ),
            _ => (
                false,
                false,
                false,
                "Gemini OAuth is available. Set GOOGLE_OAUTH_CLIENT_ID (or KAIZEN_GEMINI_OAUTH_CLIENT_ID) and GOOGLE_CLOUD_PROJECT, then click Connect OAuth.".to_string(),
            ),
        }
    } else {
        (
            supported && provider_status.can_chat,
            provider_status.auth_method == "oauth_access_token_env"
                || provider_status.auth_method == "oauth_adc",
            provider_status.auth_method == "oauth_adc",
            if supported {
                provider_status.message
            } else {
                format!(
                    "Provider '{}' does not expose app-managed OAuth here. {}",
                    provider, provider_status.message
                )
            },
        )
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
                "OAuth is not available for provider '{}'. Use the provider's supported local auth method in Integrations.",
                provider
            ),
        ));
    }

    let (pending, redirect_url) =
        oauth_store::start_gemini_oauth(default_gemini_oauth_redirect_uri())
            .map_err(|error| (StatusCode::BAD_REQUEST, error))?;
    let state_token = pending.state_token.clone();
    *state.pending_gemini_oauth.write().await = Some(pending);

    Ok(Json(OAuthStartResponse {
        provider,
        redirect_url,
        state_token,
    }))
}

async fn oauth_callback(
    State(state): State<AppState>,
    Path(provider): Path<String>,
    Query(params): Query<OAuthCallbackQuery>,
) -> impl IntoResponse {
    let provider = canonical_oauth_provider(&provider);

    if !oauth_supported(&provider) {
        return (
            StatusCode::BAD_REQUEST,
            Html(oauth_callback_page(
                "Gemini OAuth not available",
                &format!(
                    "OAuth callback is not supported for provider '{}'.",
                    provider
                ),
                false,
            )),
        )
            .into_response();
    }

    if let Some(error_code) = params.error.as_deref() {
        let description = params
            .error_description
            .as_deref()
            .unwrap_or("Google did not provide an error description.");
        return (
            StatusCode::BAD_REQUEST,
            Html(oauth_callback_page(
                "Gemini OAuth failed",
                &format!("Google returned '{}': {}.", error_code, description),
                false,
            )),
        )
            .into_response();
    }

    let state_token = match params
        .state
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        Some(value) => value.to_string(),
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Html(oauth_callback_page(
                    "Gemini OAuth failed",
                    "Google callback did not include the OAuth state token.",
                    false,
                )),
            )
                .into_response();
        }
    };

    let code = match params
        .code
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        Some(value) => value.to_string(),
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Html(oauth_callback_page(
                    "Gemini OAuth failed",
                    "Google callback did not include an authorization code.",
                    false,
                )),
            )
                .into_response();
        }
    };

    let pending = {
        let mut slot = state.pending_gemini_oauth.write().await;
        let Some(existing) = slot.as_ref() else {
            return (
                StatusCode::BAD_REQUEST,
                Html(oauth_callback_page(
                    "Gemini OAuth expired",
                    "No pending Gemini OAuth login was found. Start the login flow again from Mission Control.",
                    false,
                )),
            )
                .into_response();
        };

        if existing.is_stale() {
            *slot = None;
            return (
                StatusCode::BAD_REQUEST,
                Html(oauth_callback_page(
                    "Gemini OAuth expired",
                    "The pending Gemini OAuth state expired. Start the login flow again from Mission Control.",
                    false,
                )),
            )
                .into_response();
        }

        if existing.state_token != state_token {
            return (
                StatusCode::BAD_REQUEST,
                Html(oauth_callback_page(
                    "Gemini OAuth rejected",
                    "The Gemini OAuth state token did not match the pending login request.",
                    false,
                )),
            )
                .into_response();
        }

        slot.take().expect("pending Gemini OAuth state disappeared")
    };

    match oauth_store::exchange_gemini_code(&pending, code.as_str()).await {
        Ok(tokens) => match oauth_store::save_gemini_tokens(&tokens) {
            Ok(path) => (
                StatusCode::OK,
                Html(oauth_callback_page(
                    "Gemini OAuth connected",
                    &format!(
                        "Gemini OAuth completed successfully for Google project '{}'. Tokens were stored at '{}'. You can close this window and return to Mission Control.",
                        tokens.project_id,
                        path.display()
                    ),
                    true,
                )),
            )
                .into_response(),
            Err(error) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Html(oauth_callback_page(
                    "Gemini OAuth save failed",
                    &format!("Token exchange succeeded, but storing the Gemini OAuth tokens failed: {}.", error),
                    false,
                )),
            )
                .into_response(),
        },
        Err(error) => (
            StatusCode::BAD_REQUEST,
            Html(oauth_callback_page(
                "Gemini OAuth exchange failed",
                &format!("Google authorization completed, but exchanging the code failed: {}.", error),
                false,
            )),
        )
            .into_response(),
    }
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

    let tokens = oauth_store::refresh_stored_gemini_tokens()
        .await
        .map_err(|error| (StatusCode::BAD_REQUEST, error))?;
    oauth_store::save_gemini_tokens(&tokens)
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error))?;

    Ok(StatusCode::NO_CONTENT)
}

async fn oauth_disconnect(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(provider): Path<String>,
) -> Result<StatusCode, (StatusCode, String)> {
    require_admin_access(&state, &headers, "DELETE /api/oauth/{provider}")?;
    let provider = canonical_oauth_provider(&provider);

    if !oauth_supported(&provider) {
        return Err((
            StatusCode::BAD_REQUEST,
            format!(
                "OAuth disconnect is not supported for provider '{}'.",
                provider
            ),
        ));
    }

    *state.pending_gemini_oauth.write().await = None;
    oauth_store::clear_gemini_tokens()
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error))?;

    Ok(StatusCode::NO_CONTENT)
}

fn default_gemini_oauth_redirect_uri() -> String {
    format!(
        "http://127.0.0.1:{}/api/oauth/gemini/callback",
        resolve_bind_port()
    )
}

fn oauth_callback_page(title: &str, body: &str, success: bool) -> String {
    let accent = if success { "#2d7f5e" } else { "#a43b31" };
    format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><title>{}</title><style>body{{margin:0;font-family:Segoe UI,Arial,sans-serif;background:#f4f1ea;color:#1f2523;display:flex;min-height:100vh;align-items:center;justify-content:center;padding:24px;}}main{{max-width:680px;background:#fffaf0;border:1px solid #d8d0c4;border-radius:16px;padding:28px;box-shadow:0 18px 42px rgba(31,37,35,0.12);}}h1{{margin:0 0 12px;font-size:28px;line-height:1.1;}}p{{margin:0 0 16px;font-size:16px;line-height:1.6;}}.status{{display:inline-block;margin-bottom:16px;padding:6px 12px;border-radius:999px;background:{};color:#fff;font-weight:600;letter-spacing:0.02em;}}</style></head><body><main><div class=\"status\">{}</div><h1>{}</h1><p>{}</p><p>You can close this window.</p><script>window.setTimeout(function(){{window.close();}}, 2500);</script></main></body></html>",
        html_escape(title),
        accent,
        if success {
            "Connected"
        } else {
            "Action needed"
        },
        html_escape(title),
        html_escape(body),
    )
}

fn html_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
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

    // Vault disconnected - now lives in standalone Kai-Vault repo.
    // API keys are temporarily sourced from env vars.
    tracing::info!("Vault integration disconnected. API keys sourced from env vars.");

    let initial_crystal_ball = build_crystal_ball_client(&settings).await;
    if settings.crystal_ball_enabled && initial_crystal_ball.is_none() {
        tracing::warn!(
            "Crystal Ball enabled but Mattermost client is not configured. Running local feed only."
        );
    }

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
        inference: InferenceClient::new(),
        system_prompt: Arc::new(system_prompt),
        conversations: Arc::new(RwLock::new(HashMap::new())),
        conversation_versions: Arc::new(RwLock::new(HashMap::new())),
        pending_gemini_oauth: Arc::new(RwLock::new(None)),
        next_id: Arc::new(AtomicU64::new(1)),
    };

    let mode = resolve_bind_mode();
    let host = resolve_bind_host();
    if let Err(err) = enforce_network_policy(&mode, &host) {
        panic!("Network policy validation failed: {err}");
    }

    let port = resolve_bind_port();
    let addr = format!("{host}:{port}");

    // CORS: explicit allowlist only.
    let allowed_origins = std::env::var("KAIZEN_CORS_ORIGINS")
        .unwrap_or_else(|_| "http://localhost:3000,http://127.0.0.1:3000,tauri://localhost,http://tauri.localhost,https://tauri.localhost".to_string());
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
        .route("/api/topology", get(get_topology))
        .route("/api/branches", get(list_branches).post(create_branch))
        .route("/api/missions", get(list_missions).post(create_mission))
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
        // No-vault provider auth endpoints
        .route("/api/providers/status", get(list_provider_statuses))
        // OAuth endpoints
        .route("/api/oauth/{provider}/start", get(oauth_start))
        .route("/api/oauth/{provider}/status", get(oauth_status))
        .route("/api/oauth/{provider}/callback", get(oauth_callback))
        .route("/api/oauth/{provider}/refresh", post(oauth_refresh))
        .route("/api/oauth/{provider}", delete(oauth_disconnect))
        .with_state(state)
        .layer(middleware::from_fn(redact_error_response_middleware))
        .layer(trace_layer)
        .layer(cors);

    tracing::info!("Kaizen gateway starting on {} (mode={})", addr, mode);

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
    fn resolve_provider_override_alias_maps_native_aliases_to_configured_provider() {
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
        let result =
            parse_cors_origins("http://localhost:3000,https://example.com,tauri://localhost");
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 3);
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
