use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// The type of track
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TrackType {
    /// Video track - holds video clips, image clips, generative visual content
    Video,
    /// Audio track - holds audio clips, generative audio content
    Audio,
    /// Marker track - holds point-in-time markers
    Marker,
}

/// A track in the timeline
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Track {
    /// Unique identifier
    pub id: Uuid,
    /// Display name (e.g., "Video 1", "Audio 1", "Markers")
    pub name: String,
    /// Type of track
    pub track_type: TrackType,
    /// Track volume (applies to audio playback for audio/video clips).
    #[serde(default = "default_volume")]
    pub volume: f32,
    /// Whether this track is muted. Video tracks hide visual clips and silence embedded audio.
    #[serde(default)]
    pub muted: bool,
}

impl Track {
    /// Create a new track
    pub fn new(name: impl Into<String>, track_type: TrackType) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            track_type,
            volume: 1.0,
            muted: false,
        }
    }

    /// Create the default video track
    pub fn default_video() -> Self {
        Self::new("Video 1", TrackType::Video)
    }

    /// Create the default audio track
    pub fn default_audio() -> Self {
        Self::new("Audio 1", TrackType::Audio)
    }

    /// Create the markers track
    pub fn markers() -> Self {
        Self::new("Markers", TrackType::Marker)
    }
}

fn default_volume() -> f32 {
    1.0
}
