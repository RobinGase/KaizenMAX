mod app;
pub mod models;

use app::MissionControlApp;
use leptos::*;

// Need a #[wasm_bindgen] entry point for the lib
use wasm_bindgen::prelude::*;

#[wasm_bindgen(start)]
pub fn run() {
    mount_to_body(MissionControlApp);
}
