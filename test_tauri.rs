use tauri::{AppHandle, Manager};

fn test(app: AppHandle) {
    let _ = tauri::WebviewWindowBuilder::new(&app, "label", tauri::WebviewUrl::App("index.html".into())).build();
}
