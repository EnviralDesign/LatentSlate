//! Canonical timeline-aware media input bindings.
//!
//! Persistent source selection, sampling, and coverage for provider media fields.
//! Resolution and materialization live in `crate::core::media_binding`.
#![allow(dead_code)]

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

fn default_true() -> bool {
    true
}

/// Provider-facing media kind for a bound field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BoundMediaType {
    Image,
    Video,
    Audio,
}

impl BoundMediaType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Image => "image",
            Self::Video => "video",
            Self::Audio => "audio",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Image => "Image",
            Self::Video => "Video",
            Self::Audio => "Audio",
        }
    }
}

/// User-facing stability derived from source selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaBindingStability {
    Follow,
    LockSource,
    FreezeInput,
}

impl MediaBindingStability {
    pub fn label(self) -> &'static str {
        match self {
            Self::Follow => "Follow",
            Self::LockSource => "Locked Source",
            Self::FreezeInput => "Frozen Input",
        }
    }
}

/// Why the resolver chose a particular source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaBindingRelation {
    ExplicitAsset,
    ExplicitClip,
    Frozen,
    ExactKeyframe,
    TouchingPrevious,
    TouchingNext,
    CoveringFrame,
    CoveringRange,
}

impl MediaBindingRelation {
    pub fn label(self) -> &'static str {
        match self {
            Self::ExplicitAsset => "explicit project asset",
            Self::ExplicitClip => "explicit timeline clip",
            Self::Frozen => "frozen input",
            Self::ExactKeyframe => "exact keyframe",
            Self::TouchingPrevious => "touching previous clip",
            Self::TouchingNext => "touching next clip",
            Self::CoveringFrame => "covering frame",
            Self::CoveringRange => "covering range",
        }
    }
}

/// Half-open media time range in seconds.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct MediaTimeRange {
    pub start_seconds: f64,
    pub end_seconds: f64,
}

impl MediaTimeRange {
    pub fn new(start_seconds: f64, end_seconds: f64) -> Self {
        Self {
            start_seconds,
            end_seconds,
        }
    }

    pub fn duration(self) -> f64 {
        (self.end_seconds - self.start_seconds).max(0.0)
    }
}

/// Track filter for Follow Timeline queries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TimelineTrackScope {
    Auto,
    SameTrack,
    Below,
    SpecificTrack { track_id: Uuid },
}

impl Default for TimelineTrackScope {
    fn default() -> Self {
        Self::Auto
    }
}

impl TimelineTrackScope {
    pub fn label(&self) -> String {
        match self {
            Self::Auto => "Auto".to_string(),
            Self::SameTrack => "Same Track".to_string(),
            Self::Below => "Tracks Below".to_string(),
            Self::SpecificTrack { .. } => "Specific Track".to_string(),
        }
    }
}

/// Persisted Follow Timeline query.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TimelineSourceQuery {
    #[serde(default)]
    pub scope: TimelineTrackScope,
    #[serde(default = "default_true")]
    pub prefer_touching: bool,
}

impl Default for TimelineSourceQuery {
    fn default() -> Self {
        Self {
            scope: TimelineTrackScope::Auto,
            prefer_touching: true,
        }
    }
}

/// A sample point relative to output or source media.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MediaFramePoint {
    OutputStart,
    OutputEnd,
    OutputOffset { seconds: f64 },
    OutputFrame { frame: u32 },
    SourceStart,
    SourceEnd,
    SourceTime { seconds: f64 },
}

impl MediaFramePoint {
    pub fn label(&self) -> String {
        match self {
            Self::OutputStart => "Output first frame".to_string(),
            Self::OutputEnd => "Output last frame".to_string(),
            Self::OutputOffset { seconds } => format!("Output +{seconds:.3}s"),
            Self::OutputFrame { frame } => format!("Output frame {frame}"),
            Self::SourceStart => "Source first frame".to_string(),
            Self::SourceEnd => "Source last frame".to_string(),
            Self::SourceTime { seconds } => format!("Source {seconds:.3}s"),
        }
    }

    pub fn is_output_relative(&self) -> bool {
        matches!(
            self,
            Self::OutputStart
                | Self::OutputEnd
                | Self::OutputOffset { .. }
                | Self::OutputFrame { .. }
        )
    }

    pub fn is_source_relative(&self) -> bool {
        matches!(
            self,
            Self::SourceStart | Self::SourceEnd | Self::SourceTime { .. }
        )
    }
}

/// Sampling rule for one provider media field.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MediaSample {
    Auto,
    Whole,
    Frame {
        at: MediaFramePoint,
    },
    AlignedRange,
    SourceRange {
        start_seconds: f64,
        duration_seconds: f64,
    },
}

impl Default for MediaSample {
    fn default() -> Self {
        Self::Auto
    }
}

impl MediaSample {
    pub fn label(&self) -> String {
        match self {
            Self::Auto => "Auto".to_string(),
            Self::Whole => "Whole source".to_string(),
            Self::Frame { at } => at.label(),
            Self::AlignedRange => "Aligned with output".to_string(),
            Self::SourceRange {
                start_seconds,
                duration_seconds,
            } => format!("Source {start_seconds:.3}s + {duration_seconds:.3}s"),
        }
    }
}

/// Coverage policy. Only Strict is implemented initially.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaCoveragePolicy {
    Strict,
    TrimToOverlap,
    PadSilence,
    HoldEdges,
    Loop,
}

impl Default for MediaCoveragePolicy {
    fn default() -> Self {
        Self::Strict
    }
}

impl MediaCoveragePolicy {
    pub fn label(self) -> &'static str {
        match self {
            Self::Strict => "Strict",
            Self::TrimToOverlap => "Trim to overlap",
            Self::PadSilence => "Pad silence",
            Self::HoldEdges => "Hold edges",
            Self::Loop => "Loop",
        }
    }

    pub fn is_supported(self) -> bool {
        matches!(self, Self::Strict)
    }
}

/// Explanatory provenance recorded when an input is frozen.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct FrozenMediaOrigin {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_asset_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_clip_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_frame_time: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_frame_time: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_range: Option<MediaTimeRange>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_range: Option<MediaTimeRange>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relation: Option<MediaBindingRelation>,
}

/// Where a media field obtains its source.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MediaBindingSource {
    FollowTimeline {
        #[serde(default)]
        query: TimelineSourceQuery,
    },
    TimelineClip {
        clip_id: Uuid,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        version: Option<String>,
    },
    ProjectAsset {
        asset_id: Uuid,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        version: Option<String>,
    },
    FrozenArtifact {
        path: PathBuf,
        media_type: BoundMediaType,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        original_binding: Option<Box<MediaBindingSpec>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        origin: Option<FrozenMediaOrigin>,
    },
}

impl MediaBindingSource {
    pub fn follow_auto() -> Self {
        Self::FollowTimeline {
            query: TimelineSourceQuery::default(),
        }
    }

    pub fn stability(&self) -> MediaBindingStability {
        match self {
            Self::FollowTimeline { .. } => MediaBindingStability::Follow,
            Self::TimelineClip { .. } | Self::ProjectAsset { .. } => {
                MediaBindingStability::LockSource
            }
            Self::FrozenArtifact { .. } => MediaBindingStability::FreezeInput,
        }
    }
}

/// Persisted intent for one provider media field.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MediaBindingSpec {
    pub source: MediaBindingSource,
    #[serde(default)]
    pub sample: MediaSample,
    #[serde(default)]
    pub coverage: MediaCoveragePolicy,
}

impl Default for MediaBindingSpec {
    fn default() -> Self {
        Self {
            source: MediaBindingSource::follow_auto(),
            sample: MediaSample::Auto,
            coverage: MediaCoveragePolicy::Strict,
        }
    }
}

impl MediaBindingSpec {
    pub fn follow_auto() -> Self {
        Self::default()
    }

    pub fn stability(&self) -> MediaBindingStability {
        self.source.stability()
    }
}

/// Concrete queue-time resolution of one media field.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResolvedMediaInput {
    pub media_type: BoundMediaType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_media_type: Option<BoundMediaType>,
    pub stability: MediaBindingStability,
    pub relation: MediaBindingRelation,
    pub sample: MediaSample,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_asset_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_clip_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_path: Option<PathBuf>,
    pub materialized_path: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_range: Option<MediaTimeRange>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_range: Option<MediaTimeRange>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_frame_time: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_frame_time: Option<f64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binding_spec_round_trips_follow_auto() {
        let spec = MediaBindingSpec::follow_auto();
        let json = serde_json::to_value(&spec).unwrap();
        let restored: MediaBindingSpec = serde_json::from_value(json).unwrap();
        assert_eq!(spec, restored);
        assert_eq!(restored.stability(), MediaBindingStability::Follow);
    }

    #[test]
    fn missing_fields_default_safely() {
        let json = serde_json::json!({
            "source": { "type": "follow_timeline" }
        });
        let spec: MediaBindingSpec = serde_json::from_value(json).unwrap();
        assert!(matches!(
            spec.source,
            MediaBindingSource::FollowTimeline { query } if query.prefer_touching && matches!(query.scope, TimelineTrackScope::Auto)
        ));
        assert!(matches!(spec.sample, MediaSample::Auto));
        assert_eq!(spec.coverage, MediaCoveragePolicy::Strict);
    }

    #[test]
    fn frozen_artifact_round_trips_nested_original() {
        let spec = MediaBindingSpec {
            source: MediaBindingSource::FrozenArtifact {
                path: PathBuf::from("generated/video/x/inputs/frozen/start_image/start_image.png"),
                media_type: BoundMediaType::Image,
                original_binding: Some(Box::new(MediaBindingSpec::follow_auto())),
                origin: Some(FrozenMediaOrigin {
                    source_asset_id: Some(Uuid::nil()),
                    source_clip_id: None,
                    source_version: Some("v4".into()),
                    target_frame_time: Some(5.0),
                    source_frame_time: Some(4.958),
                    target_range: None,
                    source_range: None,
                    relation: Some(MediaBindingRelation::TouchingPrevious),
                }),
            },
            sample: MediaSample::Frame {
                at: MediaFramePoint::OutputStart,
            },
            coverage: MediaCoveragePolicy::Strict,
        };
        let json = serde_json::to_string(&spec).unwrap();
        let restored: MediaBindingSpec = serde_json::from_str(&json).unwrap();
        assert_eq!(spec, restored);
        assert_eq!(restored.stability(), MediaBindingStability::FreezeInput);
    }

    #[test]
    fn reserved_coverage_policies_deserialize() {
        for name in ["trim_to_overlap", "pad_silence", "hold_edges", "loop"] {
            let json = serde_json::json!(name);
            let policy: MediaCoveragePolicy = serde_json::from_value(json).unwrap();
            assert!(!policy.is_supported());
        }
    }
}
