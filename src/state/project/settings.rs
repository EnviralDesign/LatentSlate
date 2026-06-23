use serde::{Deserialize, Serialize};

/// Project-level settings
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectSettings {
    /// Video width in pixels
    pub width: u32,
    /// Video height in pixels
    pub height: u32,
    /// Frame rate (frames per second)
    pub fps: f64,
    /// Project timeline duration in seconds
    #[serde(default = "default_project_duration_seconds")]
    pub duration_seconds: f64,
    /// Preview downsample width in pixels
    #[serde(default = "default_preview_max_width")]
    pub preview_max_width: u32,
    /// Preview downsample height in pixels
    #[serde(default = "default_preview_max_height")]
    pub preview_max_height: u32,
}

fn default_project_duration_seconds() -> f64 {
    60.0
}

fn default_preview_max_width() -> u32 {
    960
}

fn default_preview_max_height() -> u32 {
    540
}

impl Default for ProjectSettings {
    fn default() -> Self {
        Self {
            width: 1280,
            height: 720,
            fps: 30.0,
            duration_seconds: default_project_duration_seconds(),
            preview_max_width: default_preview_max_width(),
            preview_max_height: default_preview_max_height(),
        }
    }
}
