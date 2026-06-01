//! Shared FFmpeg process-level initialization.

use std::sync::OnceLock;

use ffmpeg_next as ffmpeg;

pub fn init_ffmpeg() -> Result<(), String> {
    static INIT: OnceLock<Result<(), String>> = OnceLock::new();
    let result = INIT.get_or_init(|| {
        ffmpeg::init().map_err(|err| err.to_string())?;
        ffmpeg::util::log::set_level(ffmpeg_log_level());
        Ok(())
    });
    result.clone()
}

fn ffmpeg_log_level() -> ffmpeg::util::log::Level {
    std::env::var("LATENTSLATE_FFMPEG_LOG")
        .ok()
        .and_then(|value| parse_ffmpeg_log_level(&value))
        .unwrap_or(ffmpeg::util::log::Level::Error)
}

fn parse_ffmpeg_log_level(value: &str) -> Option<ffmpeg::util::log::Level> {
    match value.trim().to_ascii_lowercase().as_str() {
        "quiet" => Some(ffmpeg::util::log::Level::Quiet),
        "panic" => Some(ffmpeg::util::log::Level::Panic),
        "fatal" => Some(ffmpeg::util::log::Level::Fatal),
        "error" => Some(ffmpeg::util::log::Level::Error),
        "warning" | "warn" => Some(ffmpeg::util::log::Level::Warning),
        "info" => Some(ffmpeg::util::log::Level::Info),
        "verbose" => Some(ffmpeg::util::log::Level::Verbose),
        "debug" => Some(ffmpeg::util::log::Level::Debug),
        "trace" => Some(ffmpeg::util::log::Level::Trace),
        _ => None,
    }
}
