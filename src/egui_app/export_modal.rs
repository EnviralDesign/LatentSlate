use std::path::PathBuf;

use crate::core::export::{
    TimestampOverlayPosition, TimestampOverlaySettings, VideoExportCodec, VideoExportFrameFormat,
    VideoExportQuality, VideoExportSettings, VideoExportSummary,
};
use crate::editor::default_projects_dir;
use crate::state::Project;
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum ExportRunStatus {
    Idle,
    Running,
    Finished,
    Cancelled,
    Failed,
}

#[derive(Clone, Debug)]
pub(super) struct ExportModalState {
    pub(super) output_path: String,
    pub(super) codec: VideoExportCodec,
    pub(super) width: String,
    pub(super) height: String,
    pub(super) fps: String,
    pub(super) start_seconds: String,
    pub(super) duration_seconds: String,
    pub(super) include_audio: bool,
    pub(super) quality: VideoExportQuality,
    pub(super) frame_format: VideoExportFrameFormat,
    pub(super) timestamp_overlay_enabled: bool,
    pub(super) timestamp_overlay_position: TimestampOverlayPosition,
    pub(super) status: ExportRunStatus,
    pub(super) progress: f32,
    pub(super) stage: String,
    pub(super) message: String,
    pub(super) frame_label: String,
    pub(super) error: Option<String>,
    pub(super) summary: Option<VideoExportSummary>,
    pub(super) warnings: Vec<String>,
}

impl ExportModalState {
    pub(super) fn for_project(project: &Project) -> Self {
        let settings = &project.settings;
        let duration = settings.duration_seconds.max(1.0);
        Self {
            output_path: export_default_output_path(project).display().to_string(),
            codec: VideoExportCodec::H264,
            width: settings.width.to_string(),
            height: settings.height.to_string(),
            fps: format_export_number(settings.fps),
            start_seconds: "0".to_string(),
            duration_seconds: format_export_number(duration),
            include_audio: true,
            quality: VideoExportQuality::Balanced,
            frame_format: VideoExportFrameFormat::Png,
            timestamp_overlay_enabled: false,
            timestamp_overlay_position: TimestampOverlayPosition::BottomCenter,
            status: ExportRunStatus::Idle,
            progress: 0.0,
            stage: "ready".to_string(),
            message: "Ready to export".to_string(),
            frame_label: String::new(),
            error: None,
            summary: None,
            warnings: Vec::new(),
        }
    }

    pub(super) fn to_settings(&self) -> Result<VideoExportSettings, String> {
        let output_path = ensure_mp4_extension(PathBuf::from(self.output_path.trim()));
        let width = parse_export_u32("Width", &self.width)?;
        let height = parse_export_u32("Height", &self.height)?;
        let fps = parse_export_f64("FPS", &self.fps)?;
        let start_seconds = parse_export_f64("Start Seconds", &self.start_seconds)?;
        let duration_seconds = parse_export_f64("Duration Seconds", &self.duration_seconds)?;
        Ok(VideoExportSettings {
            output_path,
            codec: self.codec,
            width,
            height,
            fps,
            start_seconds,
            duration_seconds,
            include_audio: self.include_audio,
            quality: self.quality,
            frame_format: self.frame_format,
            timestamp_overlay: TimestampOverlaySettings {
                enabled: self.timestamp_overlay_enabled,
                position: self.timestamp_overlay_position,
            },
        })
    }
}

fn export_default_output_path(project: &Project) -> PathBuf {
    let file_name = format!("{}.mp4", sanitize_export_stem(&project.name));
    project
        .project_path
        .as_ref()
        .map(|root| root.join("exports").join(&file_name))
        .unwrap_or_else(|| default_projects_dir().join("exports").join(file_name))
}

fn sanitize_export_stem(value: &str) -> String {
    let stem = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim_matches('_')
        .to_string();
    if stem.is_empty() {
        "export".to_string()
    } else {
        stem
    }
}

pub(super) fn ensure_mp4_extension(mut path: PathBuf) -> PathBuf {
    let needs_extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| !extension.eq_ignore_ascii_case("mp4"))
        .unwrap_or(true);
    if needs_extension {
        path.set_extension("mp4");
    }
    path
}

fn parse_export_u32(label: &str, value: &str) -> Result<u32, String> {
    value
        .trim()
        .parse::<u32>()
        .map_err(|_| format!("{label} must be a whole number."))
}

fn parse_export_f64(label: &str, value: &str) -> Result<f64, String> {
    value
        .trim()
        .parse::<f64>()
        .map_err(|_| format!("{label} must be a number."))
}

fn format_export_number(value: f64) -> String {
    if (value - value.round()).abs() < 0.0001 {
        format!("{value:.0}")
    } else {
        format!("{value:.3}")
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string()
    }
}
