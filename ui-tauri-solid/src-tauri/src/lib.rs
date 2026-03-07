mod commands;

use commands::{
    apply_repo_update, core_request, get_repo_update_status, open_external_url, CoreClientState,
};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let core_base_url = std::env::var("KAIZEN_CORE_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:9100".to_string())
        .trim_end_matches('/')
        .to_string();

    let client = reqwest::Client::builder()
        .user_agent("kaizen-max-mission-control/0.1.0")
        .build()
        .expect("failed to build core API HTTP client");

    tauri::Builder::default()
        .manage(CoreClientState {
            core_base_url,
            client,
        })
        .invoke_handler(tauri::generate_handler![
            core_request,
            open_external_url,
            get_repo_update_status,
            apply_repo_update
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
