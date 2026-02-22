mod app;
mod models;

use app::MissionControlApp;

fn main() {
    leptos::mount_to_body(MissionControlApp);
}
