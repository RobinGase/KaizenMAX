pub mod commands;

use commands::{
    close_agent_window, core_request, focus_agent_window, open_agent_window, open_external_url,
    CoreClientState,
};
use std::sync::Once;

fn init_logging() {
    static INIT: Once = Once::new();

    INIT.call_once(|| {
        let fallback = "info,kaizen_mission_control=debug";
        let filter = tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(fallback));

        let _ = tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_target(true)
            .compact()
            .try_init();
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    init_logging();

    let core_base_url = std::env::var("KAIZEN_CORE_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:9100".to_string())
        .trim_end_matches('/')
        .to_string();

    let client = reqwest::Client::builder()
        .user_agent("kaizen-max-mission-control/0.1.0")
        .build()
        .expect("failed to build core API HTTP client");

    tracing::info!(%core_base_url, "Mission Control desktop starting");

    tauri::Builder::default()
        .manage(CoreClientState {
            core_base_url,
            client,
        })
        .invoke_handler(tauri::generate_handler![
            core_request,
            open_external_url,
            open_agent_window,
            focus_agent_window,
            close_agent_window
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
