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
    /// Whether visual clips on this track appear in preview and export output.
    #[serde(default = "default_visual_enabled")]
    pub visual_enabled: bool,
    /// Whether audio carried by clips on this track is silenced in playback and export.
    #[serde(default)]
    pub audio_muted: bool,
    /// Legacy combined mute state. It is read from older project files and migrated on load.
    #[serde(default, rename = "muted", skip_serializing)]
    legacy_muted: bool,
}

impl Track {
    /// Create a new track
    pub fn new(name: impl Into<String>, track_type: TrackType) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            track_type,
            volume: 1.0,
            visual_enabled: true,
            audio_muted: false,
            legacy_muted: false,
        }
    }

    /// Whether this track contributes visual output.
    pub fn visual_output_enabled(&self) -> bool {
        self.visual_enabled
    }

    /// Whether this track's audio is silenced.
    pub fn is_audio_muted(&self) -> bool {
        self.audio_muted
    }

    /// Enable or disable this track's visual output.
    pub fn set_visual_output_enabled(&mut self, enabled: bool) {
        self.visual_enabled = enabled;
    }

    /// Mute or unmute this track's audio output.
    pub fn set_audio_muted(&mut self, muted: bool) {
        self.audio_muted = muted;
    }

    /// Enable or disable all output from this track.
    pub fn set_track_disabled(&mut self, disabled: bool) {
        self.visual_enabled = !disabled;
        self.audio_muted = disabled;
    }

    /// Apply the pre-output-split `muted` state from an older project file.
    pub(crate) fn migrate_legacy_mute(&mut self) -> bool {
        if !self.legacy_muted {
            return false;
        }

        self.set_track_disabled(true);
        self.legacy_muted = false;
        true
    }

    /// Create a numbered video track.
    pub fn video(number: usize) -> Self {
        Self::new(format!("Video {number}"), TrackType::Video)
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

fn default_visual_enabled() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_muted_track_disables_both_outputs() {
        let mut track: Track = serde_json::from_str(
            r#"{
                "id": "00000000-0000-0000-0000-000000000000",
                "name": "Video 1",
                "track_type": "Video",
                "muted": true
            }"#,
        )
        .expect("legacy track parses");

        assert!(track.migrate_legacy_mute());
        assert!(!track.visual_output_enabled());
        assert!(track.is_audio_muted());
        assert!(!serde_json::to_string(&track)
            .expect("track serializes")
            .contains("\"muted\""));
    }
}
