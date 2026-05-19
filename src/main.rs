//! NLA AI Video Creator
//!
//! A local-first, AI-native Non-Linear Animation editor for generative video production.

mod constants;
mod core;
mod editor;
mod egui_app;
mod providers;
mod state;

fn main() {
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
