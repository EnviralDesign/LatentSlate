//! Selection state shared across views.

use uuid::Uuid;

/// Tracks the current selection across timeline and assets.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SelectionState {
    /// Selected clip IDs in the timeline.
    pub clip_ids: Vec<Uuid>,
    /// Selected asset IDs in the assets panel.
    pub asset_ids: Vec<Uuid>,
    /// Selected track IDs in the timeline.
    pub track_ids: Vec<Uuid>,
    /// Selected marker IDs in the timeline.
    pub marker_ids: Vec<Uuid>,
}

impl SelectionState {
    /// Clear all selections.
    pub fn clear(&mut self) {
        self.clip_ids.clear();
        self.asset_ids.clear();
        self.track_ids.clear();
        self.marker_ids.clear();
    }

    /// Replace the selection with a single clip.
    pub fn select_clip(&mut self, clip_id: Uuid) {
        self.clip_ids.clear();
        self.asset_ids.clear();
        self.track_ids.clear();
        self.marker_ids.clear();
        self.clip_ids.push(clip_id);
    }

    /// Remove a clip from selection, if present.
    #[allow(dead_code)]
    // Kept for multi-select editing actions; current egui interactions replace
    // the primary selection directly.
    pub fn remove_clip(&mut self, clip_id: Uuid) {
        self.clip_ids.retain(|id| *id != clip_id);
    }

    /// Return the primary selected clip, if any.
    pub fn primary_clip(&self) -> Option<Uuid> {
        self.clip_ids.first().copied()
    }

    /// Replace the selection with a single track.
    pub fn select_track(&mut self, track_id: Uuid) {
        self.clip_ids.clear();
        self.asset_ids.clear();
        self.track_ids.clear();
        self.marker_ids.clear();
        self.track_ids.push(track_id);
    }

    /// Return the primary selected track, if any.
    pub fn primary_track(&self) -> Option<Uuid> {
        self.track_ids.first().copied()
    }

    /// Replace the selection with a single marker.
    pub fn select_marker(&mut self, marker_id: Uuid) {
        self.clip_ids.clear();
        self.asset_ids.clear();
        self.track_ids.clear();
        self.marker_ids.clear();
        self.marker_ids.push(marker_id);
    }

    /// Remove a marker from selection, if present.
    #[allow(dead_code)]
    // Kept for multi-select editing actions; current egui interactions replace
    // the primary selection directly.
    pub fn remove_marker(&mut self, marker_id: Uuid) {
        self.marker_ids.retain(|id| *id != marker_id);
    }

    /// Return the primary selected marker, if any.
    pub fn primary_marker(&self) -> Option<Uuid> {
        self.marker_ids.first().copied()
    }
}
