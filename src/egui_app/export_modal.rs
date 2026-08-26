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
    ValidationIssue,
    Running,
    Canceling,
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
        if self.output_path.trim().is_empty() {
            return Err("Choose an output file before exporting.".to_string());
        }
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

pub(super) fn export_operation_presentation(
    state: &ExportModalState,
) -> crate::ui_kit::OperationPresentation {
    use crate::ui_kit::{
        OperationPhase as Phase, OperationPresentation, OperationSeverity as Severity,
    };

    match state.status {
        ExportRunStatus::Idle => {
            OperationPresentation::new(Phase::Idle, Severity::Neutral, "Ready to export.")
                .detail("Choose settings, then start the export.")
        }
        ExportRunStatus::ValidationIssue => {
            let mut presentation = OperationPresentation::new(
                Phase::Blocked,
                Severity::Warning,
                "Export settings need attention.",
            );
            if let Some(error) = state.error.as_deref() {
                presentation = presentation.detail(error);
            }
            presentation
        }
        ExportRunStatus::Running => {
            let title = match state.stage.as_str() {
                "frames" => "Rendering video.",
                "audio" => "Preparing audio.",
                "encode" => "Encoding and writing MP4.",
                _ => "Preparing export.",
            };
            OperationPresentation::new(Phase::Running, Severity::Informational, title)
                .detail(&state.message)
                .progress(state.progress, export_progress_label(state))
        }
        ExportRunStatus::Canceling => OperationPresentation::new(
            Phase::Canceling,
            Severity::Informational,
            "Canceling export.",
        )
        .detail("The current export step is being stopped safely.")
        .progress(state.progress, export_progress_label(state)),
        ExportRunStatus::Finished => {
            let severity = if state.warnings.is_empty() {
                Severity::Success
            } else {
                Severity::Warning
            };
            let mut presentation = OperationPresentation::new(
                Phase::Succeeded,
                severity,
                if state.warnings.is_empty() {
                    "Export complete."
                } else {
                    "Export complete with warnings."
                },
            );
            if let Some(summary) = state.summary.as_ref() {
                presentation = presentation.detail(summary.output_path.display().to_string());
            }
            if !state.warnings.is_empty() {
                presentation = presentation.technical_detail(state.warnings.join("\n"));
            }
            presentation
        }
        ExportRunStatus::Failed => {
            let mut presentation =
                OperationPresentation::new(Phase::Failed, Severity::Error, "Export failed.")
                    .detail(format!(
                        "The output could not be completed during {}.",
                        export_stage_label(&state.stage)
                    ));
            if let Some(error) = state.error.as_deref() {
                presentation = presentation.technical_detail(error);
            }
            presentation
        }
        ExportRunStatus::Cancelled => {
            OperationPresentation::new(Phase::Canceled, Severity::Neutral, "Export canceled.")
                .detail("No completed output was reported.")
        }
    }
}

fn export_progress_label(state: &ExportModalState) -> String {
    if state.frame_label.is_empty() {
        export_stage_label(&state.stage).to_string()
    } else {
        state.frame_label.clone()
    }
}

fn export_stage_label(stage: &str) -> &'static str {
    match stage {
        "frames" => "video rendering",
        "audio" => "audio preparation",
        "encode" => "MP4 encoding",
        "preparing" => "preparation",
        _ => "export processing",
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

#[cfg(test)]
mod operation_mapping_tests {
    use super::*;
    use crate::ui_kit::{OperationPhase as Phase, OperationSeverity as Severity};

    fn state(status: ExportRunStatus) -> ExportModalState {
        let mut state = ExportModalState::for_project(&Project::default());
        state.status = status;
        state
    }

    #[test]
    fn export_validation_failure_and_success_have_distinct_meaning() {
        let mut validation = state(ExportRunStatus::ValidationIssue);
        validation.error = Some("Width must be a whole number.".to_string());
        let validation = export_operation_presentation(&validation);
        assert_eq!(validation.phase, Phase::Blocked);
        assert_eq!(validation.severity, Severity::Warning);
        assert!(validation.technical_detail.is_none());

        let failed = export_operation_presentation(&state(ExportRunStatus::Failed));
        assert_eq!(failed.phase, Phase::Failed);
        assert_eq!(failed.severity, Severity::Error);

        let succeeded = export_operation_presentation(&state(ExportRunStatus::Finished));
        assert_eq!(succeeded.phase, Phase::Succeeded);
        assert_eq!(succeeded.severity, Severity::Success);
    }

    #[test]
    fn export_canceling_and_warning_success_are_not_errors() {
        let canceling = export_operation_presentation(&state(ExportRunStatus::Canceling));
        assert_eq!(canceling.phase, Phase::Canceling);
        assert_eq!(canceling.severity, Severity::Informational);

        let mut warned = state(ExportRunStatus::Finished);
        warned
            .warnings
            .push("Audio source was skipped.".to_string());
        let warned = export_operation_presentation(&warned);
        assert_eq!(warned.phase, Phase::Succeeded);
        assert_eq!(warned.severity, Severity::Warning);
        assert_eq!(
            warned.technical_detail.as_deref(),
            Some("Audio source was skipped.")
        );
    }

    #[test]
    fn empty_output_path_stays_a_local_validation_issue() {
        let mut state = ExportModalState::for_project(&Project::default());
        state.output_path = "  ".to_string();
        assert_eq!(
            state.to_settings().unwrap_err(),
            "Choose an output file before exporting."
        );
    }
}
