//! NLA AI Video Creator
//! 
//! A local-first, AI-native Non-Linear Animation editor for generative video production.

mod app;
mod constants;
mod components;
mod hotkeys;
mod state;
mod timeline;
mod core;
mod providers;

use dioxus::desktop::{Config, WindowBuilder, LogicalSize};
use crate::core::preview_store;

mod utils;

// ... (imports)

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if let Some(config) = crate::core::automation::config_from_args(&args) {
        if let Err(err) = crate::core::automation::start(config) {
            eprintln!("[AUTOMATION ERROR] {}", err);
        }
    }

    // Configure the window
    let config = Config::new()
        .with_window(
            WindowBuilder::new()
                .with_title("NLA AI Video Creator")
                .with_inner_size(LogicalSize::new(1280.0, 800.0))
                .with_resizable(true)
        )
        .with_menu(None) // Disable default menu bar
        .with_custom_head(r#"<meta http-equiv="Content-Security-Policy" content="default-src 'self' 'unsafe-inline' 'unsafe-eval' ws: http: https: nla: data: file:;">"#.to_string())
        .with_custom_protocol("nla".to_string(), |_id, request| {
            let request_path = request.uri().path();
            if request_path.starts_with("/preview/raw/") {
                let version_str = request_path.trim_start_matches("/preview/raw/");
                let version = version_str.parse::<u64>().ok();
                let bytes = match version {
                    Some(version) => preview_store::get_preview_bytes(version),
                    None => preview_store::get_latest_preview_bytes(),
                };

                return match bytes {
                    Some(bytes) => http::Response::builder()
                        .status(200)
                        .header("Content-Type", "application/octet-stream")
                        .header("Access-Control-Allow-Origin", "*")
                        .body(std::borrow::Cow::from(bytes))
                        .unwrap_or_else(|_| {
                            http::Response::builder()
                                .status(500)
                                .body(std::borrow::Cow::from(Vec::new()))
                                .unwrap()
                        }),
                    None => http::Response::builder()
                        .status(404)
                        .body(std::borrow::Cow::from(Vec::new()))
                        .unwrap(),
                };
            }

            // request.uri().path() will be like "/C:/Users/Dev/.cache/thumb.jpg"
            // We need to strip the leading slash to get the Windows path
            let raw_path = request_path.trim_start_matches('/');

            // Decode URL-encoded characters (e.g., spaces)
            let decoded = percent_encoding::percent_decode_str(raw_path).decode_utf8_lossy();
            let path = std::path::PathBuf::from(decoded.to_string());

            // NOTE: fs::read loads the entire file into memory.
            // This is efficient for small images/thumbnails but NOT for large video files.
            // For video playback, we would need to implement HTTP Range requests and streaming.
            match std::fs::read(&path) {
                Ok(bytes) => {
                    let mime = mime_guess::from_path(&path)
                        .first_or_octet_stream()
                        .as_ref()
                        .to_string();

                    http::Response::builder()
                        .status(200)
                        .header("Content-Type", mime)
                        .header("Access-Control-Allow-Origin", "*")
                        .body(std::borrow::Cow::from(bytes))
                        .unwrap_or_else(|_| {
                             http::Response::builder()
                                .status(500)
                                .body(std::borrow::Cow::from(Vec::new()))
                                .unwrap()
                        })
                },
                Err(e) => {
                    eprintln!("Failed to load asset: {:?} - {}", path, e);
                    http::Response::builder()
                        .status(404)
                        .body(std::borrow::Cow::from(Vec::new()))
                        .unwrap()
                }
            }
        });

    // Launch the Dioxus desktop application
    dioxus::LaunchBuilder::desktop()
        .with_cfg(config)
        .launch(app::App);
}
