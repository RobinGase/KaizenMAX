//! ZeroClaw Gateway - Kaizen MAX core runtime
//!
//! This is the Rust-native gateway that handles:
//! - Agent lifecycle management
//! - Orchestration state machine (hard gates)
//! - MCP tool routing
//! - Provider inference API proxying

use axum::{routing::get, Json, Router};
use serde::Serialize;
use tower_http::cors::CorsLayer;
use tracing_subscriber::EnvFilter;

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
    engine: &'static str,
    version: &'static str,
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        engine: "zeroclaw",
        version: env!("CARGO_PKG_VERSION"),
    })
}

#[derive(Serialize)]
struct SettingsResponse {
    runtime_engine: &'static str,
    hard_gates_enabled: bool,
    max_subagents: u32,
    provider_inference_only: bool,
}

async fn settings() -> Json<SettingsResponse> {
    // TODO: Load from config/defaults.json and .env overrides
    Json(SettingsResponse {
        runtime_engine: "zeroclaw",
        hard_gates_enabled: true,
        max_subagents: 5,
        provider_inference_only: true,
    })
}

#[tokio::main]
async fn main() {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("info".parse().unwrap()))
        .init();

    let host = std::env::var("ZEROCLAW_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
    let port = std::env::var("ZEROCLAW_PORT").unwrap_or_else(|_| "9100".to_string());
    let addr = format!("{host}:{port}");

    let app = Router::new()
        .route("/health", get(health))
        .route("/api/settings", get(settings))
        .layer(CorsLayer::permissive());

    tracing::info!("ZeroClaw gateway starting on {}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
