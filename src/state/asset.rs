//! Asset types
//!
//! Assets represent content in the project - both imported files and generative assets.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

/// The kind of asset - either a simple file reference or a generative asset
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
#[allow(dead_code)]
pub enum AssetKind {
    /// A standard video file
    Video {
        /// Path relative to project root
        path: PathBuf,
    },
    /// A standard image file
    Image {
        /// Path relative to project root
        path: PathBuf,
    },
    /// A standard audio file
    Audio {
        /// Path relative to project root
        path: PathBuf,
    },
    /// A generative video asset with version history
    GenerativeVideo {
        /// Folder path relative to project root (e.g., `generated/video/<asset-id>`)
        folder: PathBuf,
        /// Currently active version (e.g., "v1")
        active_version: Option<String>,
        /// Target frames per second for generated output
        #[serde(default = "default_generative_video_fps")]
        fps: f64,
        /// Target frame count for generated output
        #[serde(default = "default_generative_video_frame_count")]
        frame_count: u32,
    },
    /// A generative image asset with version history
    GenerativeImage {
        /// Folder path relative to project root
        folder: PathBuf,
        /// Currently active version
        active_version: Option<String>,
    },
    /// A generative audio asset with version history
    GenerativeAudio {
        /// Folder path relative to project root
        folder: PathBuf,
        /// Currently active version
        active_version: Option<String>,
    },
}

#[allow(dead_code)]
impl AssetKind {
    /// Returns true if this is a generative asset
    pub fn is_generative(&self) -> bool {
        matches!(
            self,
            AssetKind::GenerativeVideo { .. }
                | AssetKind::GenerativeImage { .. }
                | AssetKind::GenerativeAudio { .. }
        )
    }

    /// Returns true if this asset produces video/image content (placeable on video tracks)
    pub fn is_visual(&self) -> bool {
        matches!(
            self,
            AssetKind::Video { .. }
                | AssetKind::Image { .. }
                | AssetKind::GenerativeVideo { .. }
                | AssetKind::GenerativeImage { .. }
        )
    }

    /// Returns true if this asset produces audio content (placeable on audio tracks)
    pub fn is_audio(&self) -> bool {
        matches!(
            self,
            AssetKind::Audio { .. } | AssetKind::GenerativeAudio { .. }
        )
    }
}

/// An asset in the project
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Asset {
    /// Unique identifier
    pub id: Uuid,
    /// User-facing display name
    pub name: String,
    /// Optional duration in seconds for time-based media
    #[serde(default)]
    pub duration_seconds: Option<f64>,
    /// The type and location of this asset
    pub kind: AssetKind,
}

#[allow(dead_code)]
impl Asset {
    /// Create a new video asset from an imported file
    pub fn new_video(name: impl Into<String>, path: PathBuf) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            duration_seconds: None,
            kind: AssetKind::Video { path },
        }
    }

    /// Create a new image asset from an imported file
    pub fn new_image(name: impl Into<String>, path: PathBuf) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            duration_seconds: None,
            kind: AssetKind::Image { path },
        }
    }

    /// Create a new audio asset from an imported file
    pub fn new_audio(name: impl Into<String>, path: PathBuf) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            duration_seconds: None,
            kind: AssetKind::Audio { path },
        }
    }

    /// Create a new generative video asset (starts hollow)
    pub fn new_generative_video(
        name: impl Into<String>,
        folder: PathBuf,
        fps: f64,
        frame_count: u32,
    ) -> Self {
        let duration_seconds = generative_video_duration_seconds(fps, frame_count);
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            duration_seconds,
            kind: AssetKind::GenerativeVideo {
                folder,
                active_version: None,
                fps,
                frame_count,
            },
        }
    }

    /// Create a new generative image asset (starts hollow)
    pub fn new_generative_image(name: impl Into<String>, folder: PathBuf) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            duration_seconds: None,
            kind: AssetKind::GenerativeImage {
                folder,
                active_version: None,
            },
        }
    }

    /// Create a new generative audio asset (starts hollow)
    pub fn new_generative_audio(name: impl Into<String>, folder: PathBuf) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            duration_seconds: None,
            kind: AssetKind::GenerativeAudio {
                folder,
                active_version: None,
            },
        }
    }

    /// Check if this is a generative asset
    pub fn is_generative(&self) -> bool {
        self.kind.is_generative()
    }

    /// Check if this is a video asset (including generative video)
    pub fn is_video(&self) -> bool {
        matches!(
            self.kind,
            AssetKind::Video { .. } | AssetKind::GenerativeVideo { .. }
        )
    }

    /// Check if this is an image asset (including generative image)  
    pub fn is_image(&self) -> bool {
        matches!(
            self.kind,
            AssetKind::Image { .. } | AssetKind::GenerativeImage { .. }
        )
    }

    /// Check if this asset can be placed on a video track
    pub fn is_visual(&self) -> bool {
        self.kind.is_visual()
    }

    /// Check if this asset can be placed on an audio track
    pub fn is_audio(&self) -> bool {
        self.kind.is_audio()
    }

    /// Return the active version label for generative assets, if any.
    pub fn active_version(&self) -> Option<&str> {
        match &self.kind {
            AssetKind::GenerativeVideo { active_version, .. }
            | AssetKind::GenerativeImage { active_version, .. }
            | AssetKind::GenerativeAudio { active_version, .. } => active_version.as_deref(),
            _ => None,
        }
    }

    /// Update the cached duration in seconds for this asset
    pub fn set_duration_seconds(&mut self, duration_seconds: Option<f64>) {
        self.duration_seconds = duration_seconds;
    }
}

pub const DEFAULT_GENERATIVE_VIDEO_FPS: f64 = 16.0;
pub const DEFAULT_GENERATIVE_VIDEO_FRAME_COUNT: u32 = 81;

fn default_generative_video_fps() -> f64 {
    DEFAULT_GENERATIVE_VIDEO_FPS
}

fn default_generative_video_frame_count() -> u32 {
    DEFAULT_GENERATIVE_VIDEO_FRAME_COUNT
}

pub fn generative_video_duration_seconds(fps: f64, frame_count: u32) -> Option<f64> {
    if fps > 0.0 && frame_count > 0 {
        Some(frame_count as f64 / fps)
    } else {
        None
    }
}

pub fn asset_display_name(asset: &Asset) -> String {
    if asset.is_generative() {
        if let Some(version) = asset.active_version() {
            return format!("{} ({})", asset.name, version);
        }
    }
    asset.name.clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_asset_creation() {
        let video = Asset::new_video("My Video", PathBuf::from("video/clip.mp4"));
        assert!(video.is_visual());
        assert!(!video.is_audio());
        assert!(!video.is_generative());

        let gen_video = Asset::new_generative_video(
            "Gen Video",
            PathBuf::from("generated/video/550e8400-e29b-41d4-a716-446655440000"),
            DEFAULT_GENERATIVE_VIDEO_FPS,
            DEFAULT_GENERATIVE_VIDEO_FRAME_COUNT,
        );
        assert!(gen_video.is_visual());
        assert!(!gen_video.is_audio());
        assert!(gen_video.is_generative());
    }

    #[test]
    fn test_asset_serialization() {
        let asset = Asset::new_image("Test Image", PathBuf::from("images/test.png"));
        let json = serde_json::to_string_pretty(&asset).unwrap();
        let parsed: Asset = serde_json::from_str(&json).unwrap();
        assert_eq!(asset.id, parsed.id);
        assert_eq!(asset.name, parsed.name);
    }
}
