use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use uuid::Uuid;

use super::{Clip, ClipImageMode, ClipTransform, Marker, ProjectSettings, Track, TrackType};
use crate::state::{generative_video_duration_seconds, Asset, AssetKind, GenerativeConfig};

/// The main project container
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    /// Schema version for future compatibility
    pub version: String,
    /// Project name
    pub name: String,
    /// Project settings (resolution, fps, etc.)
    pub settings: ProjectSettings,
    /// All tracks in the project (ordered top to bottom)
    pub tracks: Vec<Track>,
    /// All assets in the project
    pub assets: Vec<Asset>,
    /// All clips placed on tracks
    pub clips: Vec<Clip>,
    /// All markers
    pub markers: Vec<Marker>,

    /// Path to the project folder (not serialized - set on load)
    #[serde(skip)]
    pub project_path: Option<PathBuf>,
    /// In-memory generative configs keyed by asset id.
    #[serde(skip)]
    pub generative_configs: HashMap<Uuid, GenerativeConfig>,
}

impl Default for Project {
    fn default() -> Self {
        Self {
            version: "1.0".to_string(),
            name: "Untitled Project".to_string(),
            settings: ProjectSettings::default(),
            tracks: vec![
                Track::default_video(),
                Track::default_audio(),
                Track::markers(),
            ],
            assets: Vec::new(),
            clips: Vec::new(),
            markers: Vec::new(),
            project_path: None,
            generative_configs: HashMap::new(),
        }
    }
}

#[allow(dead_code)]
impl Project {
    /// Create a new project with default settings
    #[allow(dead_code)]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ..Default::default()
        }
    }

    /// Get the project duration (end of last clip or marker)
    pub fn duration(&self) -> f64 {
        let clip_end = self
            .clips
            .iter()
            .map(|clip| self.effective_clip_end_time(clip))
            .fold(0.0, f64::max);
        let marker_end = self.markers.iter().map(|m| m.time).fold(0.0, f64::max);
        let configured = self.settings.duration_seconds.max(0.0);
        clip_end.max(marker_end).max(configured)
    }

    /// True when an image clip is displayed as a point-in-time keyframe reference.
    pub fn is_keyframe_reference_clip(&self, clip: &Clip) -> bool {
        clip.image_mode == ClipImageMode::Keyframe
            && self
                .find_asset(clip.asset_id)
                .is_some_and(|asset| asset.is_image())
    }

    /// Timeline end used by duration and navigation. Keyframe references are points.
    pub fn effective_clip_end_time(&self, clip: &Clip) -> f64 {
        if self.is_keyframe_reference_clip(clip) {
            clip.start_time
        } else {
            clip.end_time()
        }
    }

    /// Find a track by ID
    pub fn find_track(&self, id: Uuid) -> Option<&Track> {
        self.tracks.iter().find(|t| t.id == id)
    }

    /// Find an asset by ID
    pub fn find_asset(&self, id: Uuid) -> Option<&Asset> {
        self.assets.iter().find(|a| a.id == id)
    }

    /// Return true when an asset can be placed on the given track.
    pub fn asset_compatible_with_track(&self, asset_id: Uuid, track_id: Uuid) -> bool {
        let Some(asset) = self.find_asset(asset_id) else {
            return false;
        };
        let Some(track) = self.find_track(track_id) else {
            return false;
        };
        asset_matches_track_type(asset, track.track_type)
    }

    /// Get the in-memory generative config for an asset.
    pub fn generative_config(&self, asset_id: Uuid) -> Option<&GenerativeConfig> {
        self.generative_configs.get(&asset_id)
    }

    /// Get the in-memory generative config for an asset, mutably.
    pub fn generative_config_mut(&mut self, asset_id: Uuid) -> Option<&mut GenerativeConfig> {
        self.generative_configs.get_mut(&asset_id)
    }

    /// Set the cached duration (in seconds) for an asset
    pub fn set_asset_duration(&mut self, id: Uuid, duration_seconds: Option<f64>) -> bool {
        if let Some(asset) = self.assets.iter_mut().find(|a| a.id == id) {
            asset.set_duration_seconds(duration_seconds);
            return true;
        }
        false
    }

    /// Get the cached duration (in seconds) for an asset
    pub fn asset_duration_seconds(&self, id: Uuid) -> Option<f64> {
        self.find_asset(id).and_then(|asset| asset.duration_seconds)
    }

    /// Get a clip duration for an asset, falling back to a default value
    pub fn asset_clip_duration(&self, id: Uuid, default_duration: f64) -> f64 {
        self.asset_duration_seconds(id).unwrap_or(default_duration)
    }

    pub fn ensure_generative_video_durations(&mut self) {
        for asset in self.assets.iter_mut() {
            let AssetKind::GenerativeVideo {
                fps, frame_count, ..
            } = &asset.kind
            else {
                continue;
            };
            if asset.duration_seconds.is_none() {
                asset.duration_seconds = generative_video_duration_seconds(*fps, *frame_count);
            }
        }
    }

    /// Get all clips on a specific track
    pub fn clips_on_track(&self, track_id: Uuid) -> Vec<&Clip> {
        self.clips
            .iter()
            .filter(|c| c.track_id == track_id)
            .collect()
    }

    /// Get all clips that overlap a time range
    pub fn clips_in_range(&self, start: f64, end: f64) -> Vec<&Clip> {
        self.clips
            .iter()
            .filter(|clip| {
                if self.is_keyframe_reference_clip(clip) {
                    clip.start_time >= start && clip.start_time < end
                } else {
                    clip.overlaps(start, end)
                }
            })
            .collect()
    }

    /// Get assets that have clips overlapping a time range
    pub fn assets_in_range(&self, start: f64, end: f64) -> Vec<&Asset> {
        let clip_asset_ids: Vec<Uuid> = self
            .clips_in_range(start, end)
            .iter()
            .map(|c| c.asset_id)
            .collect();

        self.assets
            .iter()
            .filter(|a| clip_asset_ids.contains(&a.id))
            .collect()
    }

    /// Add a new video track
    pub fn add_video_track(&mut self) -> Uuid {
        let count = self
            .tracks
            .iter()
            .filter(|t| t.track_type == TrackType::Video)
            .count();
        let track = Track::new(format!("Video {}", count + 1), TrackType::Video);
        let id = track.id;
        self.tracks.push(track);
        id
    }

    /// Add a new audio track
    pub fn add_audio_track(&mut self) -> Uuid {
        let count = self
            .tracks
            .iter()
            .filter(|t| t.track_type == TrackType::Audio)
            .count();
        let track = Track::new(format!("Audio {}", count + 1), TrackType::Audio);
        let id = track.id;
        self.tracks.push(track);
        id
    }

    /// Add a new marker track.
    pub fn add_marker_track(&mut self) -> Uuid {
        let count = self
            .tracks
            .iter()
            .filter(|t| t.track_type == TrackType::Marker)
            .count();
        let name = if count == 0 {
            "Markers".to_string()
        } else {
            format!("Markers {}", count + 1)
        };
        let track = Track::new(name, TrackType::Marker);
        let id = track.id;
        self.tracks.push(track);
        id
    }

    /// Return the first marker track, used for legacy unassigned markers.
    pub fn first_marker_track_id(&self) -> Option<Uuid> {
        self.tracks
            .iter()
            .find(|track| track.track_type == TrackType::Marker)
            .map(|track| track.id)
    }

    /// Return the first marker track, creating it if the project has none.
    pub fn ensure_marker_track(&mut self) -> Uuid {
        self.first_marker_track_id()
            .unwrap_or_else(|| self.add_marker_track())
    }

    /// True if a marker should render on the given marker track.
    pub fn marker_belongs_to_track(&self, marker: &Marker, track_id: Uuid) -> bool {
        marker_belongs_to_track_id(marker, track_id, self.first_marker_track_id())
    }

    /// Count timeline clips and markers that would be removed with a track.
    pub fn track_delete_counts(&self, id: Uuid) -> (usize, usize) {
        let clip_count = self.clips.iter().filter(|clip| clip.track_id == id).count();
        let marker_count = self
            .tracks
            .iter()
            .find(|track| track.id == id)
            .filter(|track| track.track_type == TrackType::Marker)
            .map(|_| {
                let first_marker_track_id = self.first_marker_track_id();
                self.markers
                    .iter()
                    .filter(|marker| marker_belongs_to_track_id(marker, id, first_marker_track_id))
                    .count()
            })
            .unwrap_or(0);
        (clip_count, marker_count)
    }

    /// Remove a track by ID, including timeline items owned by that track.
    pub fn remove_track(&mut self, id: Uuid) -> bool {
        let Some(track_type) = self
            .tracks
            .iter()
            .find(|track| track.id == id)
            .map(|track| track.track_type)
        else {
            return false;
        };

        // Remove any clips on this track
        self.clips.retain(|c| c.track_id != id);
        if track_type == TrackType::Marker {
            let first_marker_track_id = self.first_marker_track_id();
            self.markers
                .retain(|marker| !marker_belongs_to_track_id(marker, id, first_marker_track_id));
        }

        // Remove the track
        let len = self.tracks.len();
        self.tracks.retain(|t| t.id != id);
        self.tracks.len() < len
    }

    /// Add an asset to the project
    pub fn add_asset(&mut self, asset: Asset) -> Uuid {
        let id = asset.id;
        let is_generative = asset.is_generative();
        self.assets.push(asset);
        if is_generative {
            self.generative_configs
                .entry(id)
                .or_insert_with(GenerativeConfig::default);
        }
        id
    }

    /// Import a file into the project
    /// Copies the file to the appropriate project subdirectory and returns a new Asset ID
    pub fn import_file(&mut self, source_path: &Path) -> io::Result<Uuid> {
        let project_root = self.project_path.as_ref().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::Other,
                "Project must be saved before importing files",
            )
        })?;

        // 1. Determine asset type and target subfolder
        let ext = source_path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();

        let (subfolder, is_video, is_audio, _is_image) = match ext.as_str() {
            "mp4" | "mov" | "avi" | "mkv" | "webm" => ("video", true, false, false),
            "mp3" | "wav" | "ogg" | "flac" => ("audio", false, true, false),
            "png" | "jpg" | "jpeg" | "gif" | "webp" => ("images", false, false, true),
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "Unsupported file type",
                ))
            }
        };

        // 2. Determine target filename with collision handling
        let file_stem = source_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("import");

        let target_dir = project_root.join(subfolder);
        // Ensure directory exists (it should, but safety first)
        if !target_dir.exists() {
            fs::create_dir_all(&target_dir)?;
        }

        let mut target_filename = format!("{}.{}", file_stem, ext);
        let mut target_path = target_dir.join(&target_filename);
        let mut counter = 1;

        while target_path.exists() {
            target_filename = format!("{}_{}.{}", file_stem, counter, ext);
            target_path = target_dir.join(&target_filename);
            counter += 1;
        }

        // 3. Copy the file
        fs::copy(source_path, &target_path)?;

        // 4. Create Asset with relative path
        let relative_path = PathBuf::from(subfolder).join(&target_filename);
        let name = file_stem.to_string(); // Use original filename as display name

        let asset = if is_video {
            Asset::new_video(name, relative_path)
        } else if is_audio {
            Asset::new_audio(name, relative_path)
        } else {
            Asset::new_image(name, relative_path)
        };

        Ok(self.add_asset(asset))
    }

    /// Remove an asset by ID (also removes any clips using this asset)
    pub fn remove_asset(&mut self, id: Uuid) -> bool {
        // Remove any clips that reference this asset
        self.clips.retain(|c| c.asset_id != id);

        // Remove the asset
        let len = self.assets.len();
        self.assets.retain(|a| a.id != id);
        self.generative_configs.remove(&id);
        self.assets.len() < len
    }

    /// Rename an asset by ID.
    pub fn rename_asset(&mut self, id: Uuid, name: impl Into<String>) -> bool {
        let name = name.into();
        if let Some(asset) = self.assets.iter_mut().find(|asset| asset.id == id) {
            asset.name = name;
            return true;
        }
        false
    }

    /// Add a clip to the project
    pub fn add_clip(&mut self, clip: Clip) -> Uuid {
        let id = clip.id;
        self.clips.push(clip);
        id
    }

    /// Create and add a clip from an asset at the specified time
    /// Places on first compatible track (Video track for video/image, Audio for audio)
    pub fn add_clip_from_asset(
        &mut self,
        asset_id: Uuid,
        start_time: f64,
        duration: f64,
    ) -> Option<Uuid> {
        // Find the asset to determine what track type to use
        let asset = self.assets.iter().find(|a| a.id == asset_id)?;

        let target_track_type = if asset.is_visual() {
            TrackType::Video
        } else if asset.is_audio() {
            TrackType::Audio
        } else {
            return None; // Can't place this asset type
        };

        // Find first matching track
        let track = self
            .tracks
            .iter()
            .find(|t| t.track_type == target_track_type)?;
        let track_id = track.id;

        // Create the clip
        let clip = Clip::new(asset_id, track_id, start_time, duration);
        Some(self.add_clip(clip))
    }

    /// Create and add a clip from an asset on a specific compatible track.
    pub fn add_clip_from_asset_to_track(
        &mut self,
        asset_id: Uuid,
        track_id: Uuid,
        start_time: f64,
        duration: f64,
    ) -> Option<Uuid> {
        if !self.asset_compatible_with_track(asset_id, track_id) {
            return None;
        }

        let clip = Clip::new(asset_id, track_id, start_time, duration);
        Some(self.add_clip(clip))
    }

    /// Update a clip label by ID (per-instance display name).
    pub fn set_clip_label(&mut self, id: Uuid, label: Option<String>) -> bool {
        if let Some(clip) = self.clips.iter_mut().find(|clip| clip.id == id) {
            clip.label = label;
            return true;
        }
        false
    }

    /// Update image timeline display mode for a clip instance.
    pub fn set_clip_image_mode(&mut self, id: Uuid, mode: ClipImageMode) -> bool {
        if let Some(clip) = self.clips.iter_mut().find(|clip| clip.id == id) {
            clip.image_mode = mode;
            return true;
        }
        false
    }

    /// Add a marker to the project
    pub fn add_marker(&mut self, mut marker: Marker) -> Uuid {
        let id = marker.id;
        if marker.track_id.is_none() {
            marker.track_id = Some(self.ensure_marker_track());
        }
        self.markers.push(marker);
        // Keep markers sorted by time
        self.markers
            .sort_by(|a, b| a.time.partial_cmp(&b.time).unwrap());
        id
    }

    /// Move a marker to a new time (seconds), keeping the list sorted.
    pub fn move_marker(&mut self, id: Uuid, new_time: f64) -> bool {
        if let Some(marker) = self.markers.iter_mut().find(|marker| marker.id == id) {
            marker.time = new_time.max(0.0);
            self.markers
                .sort_by(|a, b| a.time.partial_cmp(&b.time).unwrap());
            return true;
        }
        false
    }

    /// Update a marker label (empty string clears it).
    pub fn set_marker_label(&mut self, id: Uuid, label: Option<String>) -> bool {
        if let Some(marker) = self.markers.iter_mut().find(|marker| marker.id == id) {
            marker.label = label.filter(|value| !value.trim().is_empty());
            return true;
        }
        false
    }

    /// Update a marker description (empty string clears it).
    pub fn set_marker_description(&mut self, id: Uuid, description: Option<String>) -> bool {
        if let Some(marker) = self.markers.iter_mut().find(|marker| marker.id == id) {
            marker.description = description.filter(|value| !value.trim().is_empty());
            return true;
        }
        false
    }

    /// Update a marker color (hex string) or clear it.
    pub fn set_marker_color(&mut self, id: Uuid, color: Option<String>) -> bool {
        if let Some(marker) = self.markers.iter_mut().find(|marker| marker.id == id) {
            marker.color = color.filter(|value| !value.trim().is_empty());
            return true;
        }
        false
    }

    /// Remove a clip by ID
    pub fn remove_clip(&mut self, id: Uuid) -> bool {
        let len = self.clips.len();
        self.clips.retain(|c| c.id != id);
        self.clips.len() < len
    }

    /// Move a clip to a new start time
    pub fn move_clip(&mut self, id: Uuid, new_start_time: f64) -> bool {
        if let Some(clip) = self.clips.iter_mut().find(|c| c.id == id) {
            clip.start_time = new_start_time.max(0.0);
            return true;
        }
        false
    }

    /// Resize a clip (change start and/or duration)
    pub fn resize_clip(&mut self, id: Uuid, new_start: f64, new_duration: f64) -> bool {
        if let Some(clip) = self.clips.iter_mut().find(|c| c.id == id) {
            let old_start = clip.start_time;
            let start_time = new_start.max(0.0);
            let mut duration = new_duration.max(0.1); // Minimum 0.1 second

            let asset = self.assets.iter().find(|a| a.id == clip.asset_id);
            let max_duration = asset.and_then(|a| a.duration_seconds).filter(|d| *d > 0.0);

            if let Some(max_duration) = max_duration {
                duration = duration.min(max_duration);
            }

            if let Some(asset) = asset {
                if (asset.is_video() || asset.is_audio())
                    && (start_time - old_start).abs() > f64::EPSILON
                {
                    let delta = start_time - old_start;
                    clip.trim_in_seconds = (clip.trim_in_seconds + delta).max(0.0);

                    if let Some(max_duration) = max_duration {
                        let max_trim_in = (max_duration - duration).max(0.0);
                        if clip.trim_in_seconds > max_trim_in {
                            clip.trim_in_seconds = max_trim_in;
                        }
                    }
                }
            }

            clip.start_time = start_time;
            clip.duration = duration;
            return true;
        }
        false
    }

    /// Update the transform for a clip.
    pub fn set_clip_transform(&mut self, id: Uuid, transform: ClipTransform) -> bool {
        if let Some(clip) = self.clips.iter_mut().find(|c| c.id == id) {
            clip.transform = transform;
            return true;
        }
        false
    }

    /// Move a clip to the nearest compatible track above or below.
    pub fn move_clip_to_adjacent_track(&mut self, id: Uuid, direction: i32) -> bool {
        if direction == 0 {
            return false;
        }

        let clip_index = match self.clips.iter().position(|clip| clip.id == id) {
            Some(index) => index,
            None => return false,
        };

        let asset_id = self.clips[clip_index].asset_id;
        let asset = match self.find_asset(asset_id) {
            Some(asset) => asset,
            None => return false,
        };

        let target_track_type = if asset.is_visual() {
            TrackType::Video
        } else if asset.is_audio() {
            TrackType::Audio
        } else {
            return false;
        };

        let current_track_id = self.clips[clip_index].track_id;
        let current_track_index = match self
            .tracks
            .iter()
            .position(|track| track.id == current_track_id)
        {
            Some(index) => index,
            None => return false,
        };

        let mut index = current_track_index as i32 + direction.signum();
        while index >= 0 && (index as usize) < self.tracks.len() {
            let track = &self.tracks[index as usize];
            if track.track_type == target_track_type {
                self.clips[clip_index].track_id = track.id;
                return true;
            }
            index += direction.signum();
        }

        false
    }

    /// Remove a marker by ID
    pub fn remove_marker(&mut self, id: Uuid) -> bool {
        let len = self.markers.len();
        self.markers.retain(|m| m.id != id);
        self.markers.len() < len
    }

    /// Move a track up in the list (visually higher)
    pub fn move_track_up(&mut self, id: Uuid) -> bool {
        if let Some(index) = self.tracks.iter().position(|t| t.id == id) {
            if index > 0 {
                self.tracks.swap(index, index - 1);
                return true;
            }
        }
        false
    }

    /// Move a track down in the list (visually lower)
    pub fn move_track_down(&mut self, id: Uuid) -> bool {
        if let Some(index) = self.tracks.iter().position(|t| t.id == id) {
            if index < self.tracks.len() - 1 {
                self.tracks.swap(index, index + 1);
                return true;
            }
        }
        false
    }
}

fn asset_matches_track_type(asset: &Asset, track_type: TrackType) -> bool {
    match track_type {
        TrackType::Video => asset.is_visual(),
        TrackType::Audio => asset.is_audio(),
        TrackType::Marker => false,
    }
}

fn marker_belongs_to_track_id(
    marker: &Marker,
    track_id: Uuid,
    first_marker_track_id: Option<Uuid>,
) -> bool {
    marker.track_id == Some(track_id)
        || (marker.track_id.is_none() && first_marker_track_id == Some(track_id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_project() {
        let project = Project::default();
        assert_eq!(project.tracks.len(), 3);
        assert_eq!(project.tracks[0].track_type, TrackType::Video);
        assert_eq!(project.tracks[1].track_type, TrackType::Audio);
        assert_eq!(project.tracks[2].track_type, TrackType::Marker);
    }

    #[test]
    fn test_clip_overlap() {
        let clip = Clip::new(Uuid::new_v4(), Uuid::new_v4(), 5.0, 10.0);
        assert!(clip.overlaps(0.0, 10.0)); // Overlaps start
        assert!(clip.overlaps(10.0, 20.0)); // Overlaps end
        assert!(clip.overlaps(7.0, 12.0)); // Overlaps middle
        assert!(!clip.overlaps(0.0, 5.0)); // Just before
        assert!(!clip.overlaps(15.0, 20.0)); // Just after
    }

    #[test]
    fn test_project_serialization() {
        let project = Project::new("Test Project");
        let json = serde_json::to_string_pretty(&project).unwrap();
        let parsed: Project = serde_json::from_str(&json).unwrap();
        assert_eq!(project.name, parsed.name);
        assert_eq!(project.tracks.len(), parsed.tracks.len());
    }

    #[test]
    fn test_add_tracks() {
        let mut project = Project::default();
        let initial_count = project.tracks.len();

        project.add_video_track();
        assert_eq!(project.tracks.len(), initial_count + 1);
        assert_eq!(project.tracks.last().unwrap().name, "Video 2");

        project.add_audio_track();
        assert_eq!(project.tracks.len(), initial_count + 2);
        assert_eq!(project.tracks.last().unwrap().name, "Audio 2");
    }
}
