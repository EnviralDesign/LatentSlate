//! LatentSlate
//!
//! A local-first, AI-native timeline editor for generative video production.

mod constants;
mod core;
mod editor;
mod egui_app;
mod providers;
mod state;
mod ui_kit;

fn main() {
    if let Err(err) = crate::core::automation::reset_agent_capture_dir() {
        eprintln!("[AGENT API WARN] {err}");
    }

    let args: Vec<String> = std::env::args().collect();
    if let Some(config) = crate::core::automation::config_from_args(&args) {
        if let Err(err) = crate::core::automation::start(config) {
            eprintln!("[AUTOMATION ERROR] {err}");
        }
    }

    if let Err(err) = egui_app::run() {
        eprintln!("[APP ERROR] {err}");
    }
}
