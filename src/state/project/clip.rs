use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Transform controls for a visual clip.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ClipTransform {
    /// Horizontal translation in project pixels.
    pub position_x: f32,
    /// Vertical translation in project pixels.
    pub position_y: f32,
    /// Horizontal scale factor.
    pub scale_x: f32,
    /// Vertical scale factor.
    pub scale_y: f32,
    /// Rotation in degrees.
    pub rotation_deg: f32,
    /// Opacity from 0.0 (transparent) to 1.0 (opaque).
    pub opacity: f32,
}

impl Default for ClipTransform {
    fn default() -> Self {
        Self {
            position_x: 0.0,
            position_y: 0.0,
            scale_x: 1.0,
            scale_y: 1.0,
            rotation_deg: 0.0,
            opacity: 1.0,
        }
    }
}

/// Timeline display mode for image clip instances.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClipImageMode {
    /// Draw the image as a normal temporal still clip.
    Still,
    /// Draw the image as a keyframe/reference pin anchored at clip start.
    Keyframe,
}

impl Default for ClipImageMode {
    fn default() -> Self {
        Self::Still
    }
}

/// How a time-based source maps into a timeline clip.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClipTimeMode {
    /// Timeline length is a crop/trim window into the source media.
    Crop,
    /// Timeline length stretches the remaining source media to fit the clip.
    Stretch,
}

impl Default for ClipTimeMode {
    fn default() -> Self {
        Self::Crop
    }
}

/// Timeline linkage for a generated clip that bridges two source video clips.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClipBridgeLink {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub left_clip_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub right_clip_id: Option<Uuid>,
}

impl ClipBridgeLink {
    pub fn new(left_clip_id: Option<Uuid>, right_clip_id: Option<Uuid>) -> Self {
        Self {
            left_clip_id,
            right_clip_id,
        }
    }
}

/// A clip placed on a track
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Clip {
    /// Unique identifier
    pub id: Uuid,
    /// Reference to the asset this clip uses
    pub asset_id: Uuid,
    /// The track this clip is on
    pub track_id: Uuid,
    /// Start time in seconds
    pub start_time: f64,
    /// Duration in seconds
    pub duration: f64,
    /// Trim-in time in seconds (offset into source media)
    #[serde(default)]
    pub trim_in_seconds: f64,
    /// Volume multiplier for this clip.
    #[serde(default = "default_volume")]
    pub volume: f32,
    /// Optional user-facing label for this clip instance.
    #[serde(default)]
    pub label: Option<String>,
    /// Image-specific timeline display mode.
    #[serde(default)]
    pub image_mode: ClipImageMode,
    /// Time mapping used for video/audio source playback.
    #[serde(default)]
    pub time_mode: ClipTimeMode,
    /// Transform applied when compositing this clip.
    #[serde(default)]
    pub transform: ClipTransform,
    /// Optional source linkage for purpose-built timeline bridge clips.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bridge: Option<ClipBridgeLink>,
}

impl Clip {
    /// Create a new clip
    #[allow(dead_code)]
    pub fn new(asset_id: Uuid, track_id: Uuid, start_time: f64, duration: f64) -> Self {
        Self {
            id: Uuid::new_v4(),
            asset_id,
            track_id,
            start_time,
            duration,
            trim_in_seconds: 0.0,
            volume: 1.0,
            label: None,
            image_mode: ClipImageMode::Still,
            time_mode: ClipTimeMode::Crop,
            transform: ClipTransform::default(),
            bridge: None,
        }
    }

    /// Get the end time of this clip
    pub fn end_time(&self) -> f64 {
        self.start_time + self.duration
    }

    /// Map a project timeline time to a source-media time for this clip.
    pub fn source_time_at(&self, timeline_time: f64, source_duration: Option<f64>) -> f64 {
        let local_time = (timeline_time - self.start_time).max(0.0);
        self.source_time_for_local(local_time, source_duration)
    }

    /// Map a local clip time to a source-media time for this clip.
    pub fn source_time_for_local(&self, local_time: f64, source_duration: Option<f64>) -> f64 {
        let trim = self.trim_in_seconds.max(0.0);
        let local_time = local_time.max(0.0);
        match self.time_mode {
            ClipTimeMode::Crop => trim + local_time,
            ClipTimeMode::Stretch => {
                let Some(source_duration) = source_duration.filter(|duration| *duration > 0.0)
                else {
                    return trim + local_time;
                };
                let available = (source_duration - trim).max(0.0);
                if self.duration <= f64::EPSILON || available <= f64::EPSILON {
                    return trim;
                }
                let fraction = (local_time / self.duration).clamp(0.0, 1.0);
                trim + available * fraction
            }
        }
    }

    /// Check if this clip overlaps with a time range
    #[allow(dead_code)]
    pub fn overlaps(&self, start: f64, end: f64) -> bool {
        self.start_time < end && self.end_time() > start
    }
}

fn default_volume() -> f32 {
    1.0
}
