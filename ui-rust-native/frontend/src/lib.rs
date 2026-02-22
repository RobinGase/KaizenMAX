mod app;
pub mod models;

use app::MissionControlApp;
use js_sys::Reflect;
use leptos::*;

// Need a #[wasm_bindgen] entry point for the lib
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

fn boot_log(msg: &str) {
    if let Some(window) = web_sys::window() {
        let w: JsValue = window.into();
        if let Ok(logger) = Reflect::get(&w, &JsValue::from_str("__kaizenPushBootLog")) {
            if let Some(func) = logger.dyn_ref::<js_sys::Function>() {
                let _ = func.call1(&JsValue::NULL, &JsValue::from_str(msg));
            }
        }
    }
}

#[wasm_bindgen(start)]
pub fn run() {
    console_error_panic_hook::set_once();
    boot_log("WASM start entry reached.");

    mount_to_body(MissionControlApp);
    boot_log("Leptos mount_to_body completed.");
}
