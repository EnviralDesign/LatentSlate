//! Timeline-aware media binding resolution, migration, and materialization.
#![allow(dead_code)]

mod materialize;

#[cfg(test)]
mod tests;

#[allow(unused_imports)]
pub use materialize::{freeze_binding, materialize_plan, MEDIA_MATERIALIZER_REVISION};

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use uuid::Uuid;

use crate::core::generation::{
    active_asset_source_path, generative_asset_source_path, semantic_reference_slot,
    source_media_fps, video_asset_source_path,
};
use crate::state::{
    Asset, AssetKind, BoundMediaType, Clip, FrozenMediaOrigin, GenerativeConfig, InputRole,
    InputValue, MediaBindingRelation, MediaBindingSource, MediaBindingSpec, MediaBindingStability,
    MediaCoveragePolicy, MediaFramePoint, MediaSample, MediaTimeRange, Project, ProviderEntry,
    ProviderInputField, ProviderInputType, ResolvedMediaInput, SourceFrameReference,
    TimelineSourceQuery, TimelineTrackScope, TrackType,
};

/// Inputs required to resolve one provider media field.
#[derive(Clone, Copy)]
pub struct MediaResolveContext<'a> {
    pub project: &'a Project,
    pub target_asset_id: Option<Uuid>,
    pub context_clip_id: Option<Uuid>,
    pub field: &'a ProviderInputField,
    pub provider: Option<&'a ProviderEntry>,
    pub config: Option<&'a GenerativeConfig>,
}

/// Pure resolution result. Never writes disk.
#[derive(Debug, Clone, PartialEq)]
pub struct MediaResolvePlan {
    pub field_name: String,
    pub field_label: String,
    pub spec: MediaBindingSpec,
    pub normalized_sample: MediaSample,
    pub media_type: BoundMediaType,
    pub source_media_type: Option<BoundMediaType>,
    pub stability: MediaBindingStability,
    pub relation: Option<MediaBindingRelation>,
    pub source_asset_id: Option<Uuid>,
    pub source_clip_id: Option<Uuid>,
    pub source_version: Option<String>,
    pub source_path: Option<PathBuf>,
    pub source_path_absolute: Option<PathBuf>,
    pub target_range: Option<MediaTimeRange>,
    pub source_range: Option<MediaTimeRange>,
    pub target_frame_time: Option<f64>,
    pub source_frame_time: Option<f64>,
    pub retime_to_duration: Option<f64>,
    pub uses_original_source: bool,
    pub candidate_count: usize,
    pub ranking_explanation: Option<String>,
    pub diagnostics: Vec<String>,
    pub errors: Vec<MediaBindingError>,
}

impl MediaResolvePlan {
    pub fn is_ok(&self) -> bool {
        self.errors.is_empty() && self.relation.is_some()
    }

    pub fn to_resolved(&self, materialized_path: PathBuf) -> Option<ResolvedMediaInput> {
        let relation = self.relation?;
        if !self.errors.is_empty() {
            return None;
        }
        Some(ResolvedMediaInput {
            media_type: self.media_type,
            source_media_type: self.source_media_type,
            stability: self.stability,
            relation,
            sample: self.normalized_sample.clone(),
            source_asset_id: self.source_asset_id,
            source_clip_id: self.source_clip_id,
            source_version: self.source_version.clone(),
            source_path: self.source_path.clone(),
            materialized_path,
            target_range: self.target_range,
            source_range: self.source_range,
            target_frame_time: self.target_frame_time,
            source_frame_time: self.source_frame_time,
        })
    }

    pub fn primary_error_message(&self) -> Option<String> {
        self.errors
            .first()
            .map(|error| error.message(&self.field_label))
    }

    pub fn error_messages(&self) -> Vec<String> {
        self.errors
            .iter()
            .map(|error| error.message(&self.field_label))
            .collect()
    }
}

/// Structured binding failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MediaBindingError {
    ContextRequired,
    InvalidContext { detail: String },
    MultiplePlacementsRequireContext { count: usize },
    SourceMissing { detail: String },
    SourceVersionMissing { version: String },
    IncompatibleMedia { expected: String, actual: String },
    SelfReferenceExcluded,
    NoTimelineCandidate { detail: String },
    FrameOutsideCoverage { detail: String },
    StrictRangeIncomplete { detail: String },
    UnsupportedCoveragePolicy(String),
    UnsupportedSample { detail: String },
    FrozenFileMissing { path: String },
    OutputSampleOutOfRange { detail: String },
    MaterializationFailed { detail: String },
    FfmpegFailed { detail: String },
    UnfreezeRequiresNewSource,
}

impl MediaBindingError {
    pub fn message(&self, field_label: &str) -> String {
        let detail = match self {
            Self::ContextRequired => {
                "timeline context required. Select a timeline clip or choose an explicit asset."
                    .to_string()
            }
            Self::InvalidContext { detail } => detail.clone(),
            Self::MultiplePlacementsRequireContext { count } => {
                format!(
                    "this asset is placed {count} times. Select a timeline clip as generation context."
                )
            }
            Self::SourceMissing { detail } => detail.clone(),
            Self::SourceVersionMissing { version } => {
                format!("source version {version} is missing or has no output file.")
            }
            Self::IncompatibleMedia { expected, actual } => {
                format!("expected {expected} media, found {actual}.")
            }
            Self::SelfReferenceExcluded => {
                "automatic timeline follow cannot use another placement of the same generative asset. Fork the asset for independent continuation.".to_string()
            }
            Self::NoTimelineCandidate { detail } => detail.clone(),
            Self::FrameOutsideCoverage { detail } => detail.clone(),
            Self::StrictRangeIncomplete { detail } => detail.clone(),
            Self::UnsupportedCoveragePolicy(policy) => {
                format!("{policy} coverage is not implemented. Strict coverage is required.")
            }
            Self::UnsupportedSample { detail } => detail.clone(),
            Self::FrozenFileMissing { path } => {
                format!("frozen input file is missing ({path}).")
            }
            Self::OutputSampleOutOfRange { detail } => detail.clone(),
            Self::MaterializationFailed { detail } => detail.clone(),
            Self::FfmpegFailed { detail } => detail.clone(),
            Self::UnfreezeRequiresNewSource => {
                "this frozen input has no original binding. Choose a new source.".to_string()
            }
        };
        format!("{field_label}: {detail}")
    }
}

#[derive(Clone, Copy)]
struct TargetWindow {
    start: f64,
    end: f64,
    fps: f64,
    frame_count: Option<u32>,
}

impl TargetWindow {
    fn first_global(self) -> f64 {
        self.start
    }

    fn last_global(self) -> f64 {
        if let Some(count) = self.frame_count.filter(|count| *count > 0) {
            self.start + (count.saturating_sub(1) as f64) / self.fps.max(1.0)
        } else {
            (self.end - 1.0 / self.fps.max(1.0)).max(self.start)
        }
    }

    fn range(self) -> MediaTimeRange {
        MediaTimeRange::new(self.start, self.end)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FrameOrientation {
    Start,
    End,
    Interior,
    Source,
}

#[derive(Clone)]
struct RankedCandidate {
    clip_id: Uuid,
    asset_id: Uuid,
    relation: MediaBindingRelation,
    relation_rank: u8,
    track_rank: (u8, i64),
    track_distance: i64,
    start_time: f64,
    temporal_distance: f64,
}

/// Convert seconds to a normalized boundary frame index.
pub fn boundary_frame(time_seconds: f64, fps: f64) -> i64 {
    (time_seconds * fps.max(1.0)).round() as i64
}

/// Convert a boundary frame index back to seconds.
pub fn frame_time(frame: i64, fps: f64) -> f64 {
    frame as f64 / fps.max(1.0)
}

pub fn boundaries_touch(left: f64, right: f64, fps: f64) -> bool {
    boundary_frame(left, fps) == boundary_frame(right, fps)
}

pub fn format_timecode(seconds: f64) -> String {
    let ms = (seconds.max(0.0) * 1000.0).round() as u64;
    let minutes = ms / 60_000;
    let secs = (ms % 60_000) / 1000;
    let millis = ms % 1000;
    format!("{minutes:02}:{secs:02}.{millis:03}")
}

pub fn bound_media_type_for_input(input: &ProviderInputField) -> Option<BoundMediaType> {
    match input.input_type {
        ProviderInputType::Image => Some(BoundMediaType::Image),
        ProviderInputType::Video => Some(BoundMediaType::Video),
        ProviderInputType::Audio => Some(BoundMediaType::Audio),
        _ => None,
    }
}

pub fn bound_media_type_for_asset(asset: &Asset) -> Option<BoundMediaType> {
    if asset.is_image() {
        Some(BoundMediaType::Image)
    } else if asset.is_video() {
        Some(BoundMediaType::Video)
    } else if asset.is_audio() {
        Some(BoundMediaType::Audio)
    } else {
        None
    }
}

pub fn source_compatible_with_field(asset: &Asset, media_type: BoundMediaType) -> bool {
    match media_type {
        BoundMediaType::Image => asset.is_image() || asset.is_video(),
        BoundMediaType::Video => asset.is_video(),
        BoundMediaType::Audio => asset.is_audio(),
    }
}

pub fn default_sample_for_field(input: &ProviderInputField) -> MediaSample {
    match input.input_type {
        ProviderInputType::Image => {
            let at = match input.role {
                Some(InputRole::EndImage) => MediaFramePoint::OutputEnd,
                _ => MediaFramePoint::OutputStart,
            };
            MediaSample::Frame { at }
        }
        ProviderInputType::Video | ProviderInputType::Audio => MediaSample::AlignedRange,
        _ => MediaSample::Auto,
    }
}

pub fn normalize_sample(sample: &MediaSample, input: &ProviderInputField) -> MediaSample {
    match sample {
        MediaSample::Auto => default_sample_for_field(input),
        other => other.clone(),
    }
}

pub fn lookup_media_binding(
    config: &GenerativeConfig,
    input: &ProviderInputField,
    project: &Project,
) -> Option<MediaBindingSpec> {
    if let Some(spec) = config.media_bindings.get(&input.name) {
        return Some(spec.clone());
    }
    if let Some(slot) = semantic_reference_slot(input) {
        if slot != input.name {
            if let Some(spec) = config.media_bindings.get(slot) {
                return Some(spec.clone());
            }
        }
    }
    if let Some(value) = config.inputs.get(&input.name) {
        if let Some(spec) = legacy_input_to_binding(value, input, project) {
            return Some(spec);
        }
    }
    if let Some(value) = config.reference_slots.get(&input.name) {
        if let Some(spec) = legacy_input_to_binding(value, input, project) {
            return Some(spec);
        }
    }
    if let Some(slot) = semantic_reference_slot(input) {
        if let Some(value) = config.reference_slots.get(slot) {
            if let Some(spec) = legacy_input_to_binding(value, input, project) {
                return Some(spec);
            }
        }
        if slot != input.name {
            if let Some(value) = config.inputs.get(slot) {
                if let Some(spec) = legacy_input_to_binding(value, input, project) {
                    return Some(spec);
                }
            }
        }
    }
    None
}

pub fn legacy_input_to_binding(
    value: &InputValue,
    input: &ProviderInputField,
    project: &Project,
) -> Option<MediaBindingSpec> {
    let sample = sample_from_legacy_frame(value, input);
    match value {
        InputValue::Literal { .. } => None,
        InputValue::AssetRef {
            asset_id,
            source_clip_id,
            pinned,
            ..
        } => {
            if !pinned {
                Some(MediaBindingSpec {
                    source: MediaBindingSource::follow_auto(),
                    sample,
                    coverage: MediaCoveragePolicy::Strict,
                })
            } else if let Some(clip_id) = source_clip_id {
                let version = project.find_asset(*asset_id).and_then(|asset| {
                    asset
                        .is_generative()
                        .then(|| asset.active_version().map(str::to_string))
                        .flatten()
                });
                Some(MediaBindingSpec {
                    source: MediaBindingSource::TimelineClip {
                        clip_id: *clip_id,
                        version,
                    },
                    sample,
                    coverage: MediaCoveragePolicy::Strict,
                })
            } else {
                let version = project.find_asset(*asset_id).and_then(|asset| {
                    asset
                        .is_generative()
                        .then(|| asset.active_version().map(str::to_string))
                        .flatten()
                });
                Some(MediaBindingSpec {
                    source: MediaBindingSource::ProjectAsset {
                        asset_id: *asset_id,
                        version,
                    },
                    sample,
                    coverage: MediaCoveragePolicy::Strict,
                })
            }
        }
        InputValue::GenerationRef {
            asset_id, version, ..
        } => Some(MediaBindingSpec {
            source: MediaBindingSource::ProjectAsset {
                asset_id: *asset_id,
                version: Some(version.clone()),
            },
            sample,
            coverage: MediaCoveragePolicy::Strict,
        }),
    }
}

fn sample_from_legacy_frame(value: &InputValue, _input: &ProviderInputField) -> MediaSample {
    let frame = match value {
        InputValue::AssetRef {
            frame_reference, ..
        }
        | InputValue::GenerationRef {
            frame_reference, ..
        } => *frame_reference,
        InputValue::Literal { .. } => None,
    };
    match frame {
        Some(SourceFrameReference::First) => MediaSample::Frame {
            at: MediaFramePoint::SourceStart,
        },
        Some(SourceFrameReference::Last) => MediaSample::Frame {
            at: MediaFramePoint::SourceEnd,
        },
        None => MediaSample::Auto,
    }
}

/// Persist canonical bindings from legacy InputValue maps without deleting aliases.
pub fn canonicalize_media_bindings(
    config: &mut GenerativeConfig,
    provider: Option<&ProviderEntry>,
    project: &Project,
) {
    if let Some(provider) = provider {
        for input in provider.inputs.iter() {
            if bound_media_type_for_input(input).is_none() {
                continue;
            }
            if config.media_bindings.contains_key(&input.name) {
                continue;
            }
            if let Some(spec) = lookup_media_binding(config, input, project) {
                config.media_bindings.insert(input.name.clone(), spec);
            }
        }
    }
    for (key, value) in config.inputs.iter().chain(config.reference_slots.iter()) {
        if config.media_bindings.contains_key(key) {
            continue;
        }
        if let Some(provider) = provider {
            if !provider.inputs.iter().any(|input| input.name == *key) {
                continue;
            }
        }
        if let Some(spec) = legacy_input_to_binding(value, &placeholder_input(key), project) {
            config.media_bindings.insert(key.clone(), spec);
        }
    }
}

fn placeholder_input(name: &str) -> ProviderInputField {
    let (input_type, role) = match name {
        "end_image" => (ProviderInputType::Image, Some(InputRole::EndImage)),
        "start_image" | "source_image" | "image" => {
            (ProviderInputType::Image, Some(InputRole::StartImage))
        }
        "left_video" => (ProviderInputType::Video, Some(InputRole::LeftVideo)),
        "right_video" => (ProviderInputType::Video, Some(InputRole::RightVideo)),
        "video" => (ProviderInputType::Video, None),
        "audio" => (ProviderInputType::Audio, None),
        _ => (ProviderInputType::Image, None),
    };
    ProviderInputField {
        name: name.to_string(),
        label: name.to_string(),
        description: None,
        input_type,
        required: false,
        default: None,
        role,
        ui: None,
    }
}

pub fn resolve_field(ctx: MediaResolveContext<'_>) -> Option<MediaResolvePlan> {
    let config = ctx.config?;
    let spec = lookup_media_binding(config, ctx.field, ctx.project)?;
    Some(resolve_media_binding(ctx, &spec))
}

pub fn resolve_media_binding(
    ctx: MediaResolveContext<'_>,
    binding: &MediaBindingSpec,
) -> MediaResolvePlan {
    let mut plan = empty_plan(ctx, binding);
    let Some(media_type) = bound_media_type_for_input(ctx.field) else {
        plan.errors.push(MediaBindingError::UnsupportedSample {
            detail: "this field is not a media input.".to_string(),
        });
        return plan;
    };
    plan.media_type = media_type;
    plan.normalized_sample = normalize_sample(&binding.sample, ctx.field);
    plan.stability = binding.stability();

    if !binding.coverage.is_supported() {
        plan.errors
            .push(MediaBindingError::UnsupportedCoveragePolicy(
                binding.coverage.label().to_string(),
            ));
        return plan;
    }

    if let Err(error) =
        validate_sample_for_field(&plan.normalized_sample, media_type, &binding.source)
    {
        plan.errors.push(error);
        return plan;
    }

    match &binding.source {
        MediaBindingSource::FrozenArtifact {
            path,
            media_type: frozen_type,
            ..
        } => resolve_frozen(ctx, &mut plan, path, *frozen_type),
        MediaBindingSource::ProjectAsset { asset_id, version } => {
            resolve_project_asset(ctx, &mut plan, *asset_id, version.as_deref());
        }
        MediaBindingSource::TimelineClip { clip_id, version } => {
            resolve_timeline_clip(ctx, &mut plan, *clip_id, version.as_deref());
        }
        MediaBindingSource::FollowTimeline { query } => resolve_follow(ctx, &mut plan, query),
    }
    plan
}

fn empty_plan(ctx: MediaResolveContext<'_>, binding: &MediaBindingSpec) -> MediaResolvePlan {
    MediaResolvePlan {
        field_name: ctx.field.name.clone(),
        field_label: ctx.field.label.clone(),
        spec: binding.clone(),
        normalized_sample: MediaSample::Auto,
        media_type: BoundMediaType::Image,
        source_media_type: None,
        stability: binding.stability(),
        relation: None,
        source_asset_id: None,
        source_clip_id: None,
        source_version: None,
        source_path: None,
        source_path_absolute: None,
        target_range: None,
        source_range: None,
        target_frame_time: None,
        source_frame_time: None,
        retime_to_duration: None,
        uses_original_source: false,
        candidate_count: 0,
        ranking_explanation: None,
        diagnostics: Vec::new(),
        errors: Vec::new(),
    }
}

fn validate_sample_for_field(
    sample: &MediaSample,
    media_type: BoundMediaType,
    source: &MediaBindingSource,
) -> Result<(), MediaBindingError> {
    match (media_type, sample) {
        (BoundMediaType::Image, MediaSample::Frame { .. } | MediaSample::Whole) => Ok(()),
        (BoundMediaType::Image, MediaSample::AlignedRange | MediaSample::SourceRange { .. }) => {
            Err(MediaBindingError::UnsupportedSample {
                detail: "image fields sample a frame or a static image, not a range.".to_string(),
            })
        }
        (
            BoundMediaType::Video | BoundMediaType::Audio,
            MediaSample::AlignedRange | MediaSample::SourceRange { .. },
        ) => Ok(()),
        (BoundMediaType::Video | BoundMediaType::Audio, MediaSample::Whole) => {
            if matches!(
                source,
                MediaBindingSource::ProjectAsset { .. } | MediaBindingSource::TimelineClip { .. }
            ) {
                Ok(())
            } else {
                Err(MediaBindingError::UnsupportedSample {
                    detail: "whole-source sampling requires an explicit asset or clip. Use aligned range to follow the timeline.".to_string(),
                })
            }
        }
        (BoundMediaType::Video | BoundMediaType::Audio, MediaSample::Frame { .. }) => {
            Err(MediaBindingError::UnsupportedSample {
                detail: "video and audio fields use a range, not a still frame.".to_string(),
            })
        }
        (_, MediaSample::Auto) => Ok(()),
    }
}

fn resolve_frozen(
    ctx: MediaResolveContext<'_>,
    plan: &mut MediaResolvePlan,
    path: &Path,
    frozen_type: BoundMediaType,
) {
    if frozen_type != plan.media_type {
        plan.errors.push(MediaBindingError::IncompatibleMedia {
            expected: plan.media_type.label().to_string(),
            actual: frozen_type.label().to_string(),
        });
        return;
    }
    let absolute = absolute_project_path(ctx.project, path);
    if !absolute.exists() {
        plan.errors.push(MediaBindingError::FrozenFileMissing {
            path: path.display().to_string(),
        });
        return;
    }
    plan.relation = Some(MediaBindingRelation::Frozen);
    plan.source_media_type = Some(frozen_type);
    plan.source_path = Some(path.to_path_buf());
    plan.source_path_absolute = Some(absolute);
    plan.uses_original_source = true;
    if let MediaBindingSource::FrozenArtifact { origin, .. } = &plan.spec.source {
        if let Some(origin) = origin {
            plan.source_asset_id = origin.source_asset_id;
            plan.source_clip_id = origin.source_clip_id;
            plan.source_version = origin.source_version.clone();
            plan.target_frame_time = origin.target_frame_time;
            plan.source_frame_time = origin.source_frame_time;
            plan.target_range = origin.target_range;
            plan.source_range = origin.source_range;
        }
    }
}

fn resolve_project_asset(
    ctx: MediaResolveContext<'_>,
    plan: &mut MediaResolvePlan,
    asset_id: Uuid,
    version: Option<&str>,
) {
    let Some(asset) = ctx.project.find_asset(asset_id).cloned() else {
        plan.errors.push(MediaBindingError::SourceMissing {
            detail: format!("project asset {asset_id} was not found."),
        });
        return;
    };
    if !source_compatible_with_field(&asset, plan.media_type) {
        plan.errors.push(MediaBindingError::IncompatibleMedia {
            expected: plan.media_type.label().to_string(),
            actual: bound_media_type_for_asset(&asset)
                .map(|kind| kind.label().to_string())
                .unwrap_or_else(|| "unknown".to_string()),
        });
        return;
    }
    match fill_source_identity(ctx.project, plan, &asset, None, version, true) {
        Ok(()) => {}
        Err(error) => {
            plan.errors.push(error);
            return;
        }
    }
    plan.relation = Some(MediaBindingRelation::ExplicitAsset);
    if let Err(error) = apply_sample(ctx, plan, None, &asset) {
        plan.errors.push(error);
    }
}

fn resolve_timeline_clip(
    ctx: MediaResolveContext<'_>,
    plan: &mut MediaResolvePlan,
    clip_id: Uuid,
    version: Option<&str>,
) {
    let Some(clip) = ctx
        .project
        .clips
        .iter()
        .find(|clip| clip.id == clip_id)
        .cloned()
    else {
        plan.errors.push(MediaBindingError::SourceMissing {
            detail: format!("timeline clip {clip_id} was not found."),
        });
        return;
    };
    let Some(asset) = ctx.project.find_asset(clip.asset_id).cloned() else {
        plan.errors.push(MediaBindingError::SourceMissing {
            detail: format!("source asset {} was not found.", clip.asset_id),
        });
        return;
    };
    if !source_compatible_with_field(&asset, plan.media_type) {
        plan.errors.push(MediaBindingError::IncompatibleMedia {
            expected: plan.media_type.label().to_string(),
            actual: bound_media_type_for_asset(&asset)
                .map(|kind| kind.label().to_string())
                .unwrap_or_else(|| "unknown".to_string()),
        });
        return;
    }
    match fill_source_identity(ctx.project, plan, &asset, Some(clip.id), version, true) {
        Ok(()) => {}
        Err(error) => {
            plan.errors.push(error);
            return;
        }
    }
    plan.relation = Some(MediaBindingRelation::ExplicitClip);
    if let Err(error) = apply_sample(ctx, plan, Some(&clip), &asset) {
        plan.errors.push(error);
    }
}

fn resolve_follow(
    ctx: MediaResolveContext<'_>,
    plan: &mut MediaResolvePlan,
    query: &TimelineSourceQuery,
) {
    if matches!(
        &plan.normalized_sample,
        MediaSample::SourceRange { .. }
            | MediaSample::Frame {
                at: MediaFramePoint::SourceTime { .. }
            }
    ) {
        plan.errors.push(MediaBindingError::UnsupportedSample {
            detail: "explicit source time/range requires a locked source.".to_string(),
        });
        return;
    }

    let Some(context) = context_clip(ctx) else {
        plan.errors.push(match context_clip_error(ctx) {
            Some(error) => error,
            None => MediaBindingError::ContextRequired,
        });
        return;
    };
    if let Some(error) = context_clip_error(ctx) {
        plan.errors.push(error);
        return;
    }

    let Some(window) = target_window(ctx, &context) else {
        plan.errors.push(MediaBindingError::ContextRequired);
        return;
    };

    match &plan.normalized_sample {
        MediaSample::Frame { .. } | MediaSample::Whole => {
            resolve_follow_frame(ctx, plan, query, &context, window);
        }
        MediaSample::AlignedRange => {
            resolve_follow_range(ctx, plan, query, &context, window);
        }
        _ => {
            plan.errors.push(MediaBindingError::UnsupportedSample {
                detail: "this sample mode cannot follow the timeline.".to_string(),
            });
        }
    }
}

fn resolve_follow_frame(
    ctx: MediaResolveContext<'_>,
    plan: &mut MediaResolvePlan,
    query: &TimelineSourceQuery,
    context: &Clip,
    window: TargetWindow,
) {
    let requested = match requested_global_time(ctx, plan, Some(window), None) {
        Ok(Some(time)) => time,
        Ok(None) => window.first_global(),
        Err(error) => {
            plan.errors.push(error);
            return;
        }
    };
    let orientation = frame_orientation(&plan.normalized_sample, window);
    let mut ranked = Vec::new();
    let mut considered = 0usize;

    for clip in eligible_source_clips(ctx, query, context, plan.media_type) {
        considered += 1;
        let Some(asset) = ctx.project.find_asset(clip.asset_id) else {
            continue;
        };
        if let Some(relation) = classify_frame_candidate(
            ctx.project,
            context,
            &clip,
            asset,
            requested,
            orientation,
            window.fps,
        ) {
            ranked.push(make_ranked(
                ctx.project,
                context,
                &clip,
                relation,
                query.prefer_touching,
                requested,
                window.fps,
            ));
        }
    }
    plan.candidate_count = ranked.len();
    if considered > 0 && ranked.is_empty() {
        plan.diagnostics.push(format!(
            "Looked at {considered} compatible clips; none covered output frame {}.",
            boundary_frame(requested, window.fps)
        ));
    }
    let Some(winner) = pick_winner(&mut ranked) else {
        let track = track_name(ctx.project, context.track_id);
        plan.errors.push(MediaBindingError::NoTimelineCandidate {
            detail: format!(
                "no compatible source exists at output frame {} on {}.",
                boundary_frame(requested, window.fps),
                track
            ),
        });
        return;
    };
    plan.ranking_explanation = ranking_explanation(&ranked, &winner, query);
    apply_ranked_winner(ctx, plan, winner, Some(window), Some(requested));
}

fn resolve_follow_range(
    ctx: MediaResolveContext<'_>,
    plan: &mut MediaResolvePlan,
    query: &TimelineSourceQuery,
    context: &Clip,
    window: TargetWindow,
) {
    let mut ranked = Vec::new();
    let mut best_partial: Option<(Clip, f64, f64)> = None;
    for clip in eligible_source_clips(ctx, query, context, plan.media_type) {
        let Some(asset) = ctx.project.find_asset(clip.asset_id) else {
            continue;
        };
        if ctx.project.is_keyframe_reference_clip(&clip) {
            continue;
        }
        if !source_compatible_with_field(asset, plan.media_type) || asset.is_image() {
            continue;
        }
        let clip_start = clip.start_time;
        let clip_end = clip.end_time();
        if range_covers(clip_start, clip_end, window.start, window.end, window.fps) {
            ranked.push(make_ranked(
                ctx.project,
                context,
                &clip,
                MediaBindingRelation::CoveringRange,
                true,
                window.start,
                window.fps,
            ));
        } else if ranges_overlap(clip_start, clip_end, window.start, window.end) {
            let lead = (window.start - clip_start).max(0.0);
            let tail = (window.end - clip_end).max(0.0);
            let missing = if clip_start > window.start {
                clip_start - window.start
            } else {
                0.0
            } + if clip_end < window.end {
                window.end - clip_end
            } else {
                0.0
            };
            if best_partial
                .as_ref()
                .is_none_or(|(_, _, current)| missing < *current)
            {
                best_partial = Some((clip, lead, missing.max(tail)));
            }
        }
    }
    plan.candidate_count = ranked.len();
    let Some(winner) = pick_winner(&mut ranked) else {
        if let Some((clip, _, _)) = best_partial {
            plan.errors
                .push(strict_range_error(window, &clip, ctx.project));
        } else {
            plan.errors.push(MediaBindingError::NoTimelineCandidate {
                detail: format!(
                    "no compatible source covers {}–{}.",
                    format_timecode(window.start),
                    format_timecode(window.end)
                ),
            });
        }
        return;
    };
    plan.ranking_explanation = ranking_explanation(&ranked, &winner, query);
    apply_ranked_winner(ctx, plan, winner, Some(window), None);
}

fn apply_ranked_winner(
    ctx: MediaResolveContext<'_>,
    plan: &mut MediaResolvePlan,
    winner: RankedCandidate,
    window: Option<TargetWindow>,
    requested: Option<f64>,
) {
    let Some(clip) = ctx
        .project
        .clips
        .iter()
        .find(|clip| clip.id == winner.clip_id)
        .cloned()
    else {
        plan.errors.push(MediaBindingError::SourceMissing {
            detail: "winning timeline clip disappeared.".to_string(),
        });
        return;
    };
    let Some(asset) = ctx.project.find_asset(clip.asset_id).cloned() else {
        plan.errors.push(MediaBindingError::SourceMissing {
            detail: "winning source asset disappeared.".to_string(),
        });
        return;
    };
    if let Err(error) = fill_source_identity(
        ctx.project,
        plan,
        &asset,
        Some(clip.id),
        asset.active_version(),
        false,
    ) {
        plan.errors.push(error);
        return;
    }
    plan.relation = Some(winner.relation);
    if let Err(error) = apply_sample_with_relation(
        ctx,
        plan,
        Some(&clip),
        &asset,
        winner.relation,
        window,
        requested,
    ) {
        plan.errors.push(error);
    }
}

fn apply_sample(
    ctx: MediaResolveContext<'_>,
    plan: &mut MediaResolvePlan,
    source_clip: Option<&Clip>,
    asset: &Asset,
) -> Result<(), MediaBindingError> {
    let window = source_clip
        .and_then(|_| target_window_from_context(ctx))
        .or_else(|| target_window_from_context(ctx));
    apply_sample_with_relation(
        ctx,
        plan,
        source_clip,
        asset,
        plan.relation.unwrap_or(MediaBindingRelation::ExplicitAsset),
        window,
        None,
    )
}

fn apply_sample_with_relation(
    ctx: MediaResolveContext<'_>,
    plan: &mut MediaResolvePlan,
    source_clip: Option<&Clip>,
    asset: &Asset,
    relation: MediaBindingRelation,
    window: Option<TargetWindow>,
    requested: Option<f64>,
) -> Result<(), MediaBindingError> {
    plan.source_media_type = bound_media_type_for_asset(asset);
    let sample = plan.normalized_sample.clone();
    match &sample {
        MediaSample::Whole => apply_whole(plan, source_clip, asset),
        MediaSample::Frame { at } => apply_frame(
            ctx,
            plan,
            source_clip,
            asset,
            at,
            relation,
            window,
            requested,
        ),
        MediaSample::AlignedRange => apply_aligned_range(ctx, plan, source_clip, asset, window),
        MediaSample::SourceRange {
            start_seconds,
            duration_seconds,
        } => apply_source_range(
            plan,
            source_clip,
            asset,
            *start_seconds,
            *duration_seconds,
            ctx.project.settings.fps,
        ),
        MediaSample::Auto => Ok(()),
    }
}

fn apply_whole(
    plan: &mut MediaResolvePlan,
    source_clip: Option<&Clip>,
    asset: &Asset,
) -> Result<(), MediaBindingError> {
    if asset.is_image() {
        plan.uses_original_source = true;
        plan.source_frame_time = Some(0.0);
        return Ok(());
    }
    if let Some(clip) = source_clip {
        let fps = source_media_fps(asset, 30.0).max(1.0);
        let start = clip.source_time_for_local(0.0, asset.duration_seconds);
        let end = clip
            .source_time_for_local((clip.duration - 1.0 / fps).max(0.0), asset.duration_seconds);
        plan.source_range = Some(MediaTimeRange::new(start, end.max(start)));
        plan.uses_original_source = false;
    } else {
        plan.uses_original_source = true;
        if let Some(duration) = asset.duration_seconds {
            plan.source_range = Some(MediaTimeRange::new(0.0, duration));
        }
    }
    Ok(())
}

fn apply_frame(
    ctx: MediaResolveContext<'_>,
    plan: &mut MediaResolvePlan,
    source_clip: Option<&Clip>,
    asset: &Asset,
    at: &MediaFramePoint,
    relation: MediaBindingRelation,
    window: Option<TargetWindow>,
    requested: Option<f64>,
) -> Result<(), MediaBindingError> {
    if asset.is_image() {
        plan.uses_original_source = true;
        plan.source_frame_time = Some(0.0);
        if let Some(time) = requested.or_else(|| {
            requested_global_time(ctx, plan, window, source_clip)
                .ok()
                .flatten()
        }) {
            plan.target_frame_time = Some(time);
        }
        return Ok(());
    }

    let timeline_fps = ctx.project.settings.fps.max(1.0);
    let source_fps = source_media_fps(asset, timeline_fps).max(1.0);
    let (target_time, source_time) = match relation {
        MediaBindingRelation::TouchingPrevious => {
            let clip = source_clip.ok_or_else(|| MediaBindingError::SourceMissing {
                detail: "touching previous clip is missing.".to_string(),
            })?;
            let target = window.map(|window| window.first_global()).unwrap_or(0.0);
            (target, last_visible_source_time(clip, asset, source_fps))
        }
        MediaBindingRelation::TouchingNext => {
            let clip = source_clip.ok_or_else(|| MediaBindingError::SourceMissing {
                detail: "touching next clip is missing.".to_string(),
            })?;
            let target = window.map(|window| window.end).unwrap_or(clip.start_time);
            (
                target,
                clip.source_time_for_local(0.0, asset.duration_seconds),
            )
        }
        _ => {
            let target = match requested_global_time(ctx, plan, window, source_clip)? {
                Some(time) => time,
                None if at.is_source_relative() => 0.0,
                None => {
                    return Err(MediaBindingError::ContextRequired);
                }
            };
            let source_time = match at {
                MediaFramePoint::SourceStart => source_clip
                    .map(|clip| clip.source_time_for_local(0.0, asset.duration_seconds))
                    .unwrap_or(0.0),
                MediaFramePoint::SourceEnd => source_clip
                    .map(|clip| last_visible_source_time(clip, asset, source_fps))
                    .unwrap_or_else(|| asset_level_last_time(asset, source_fps)),
                MediaFramePoint::SourceTime { seconds } => {
                    validate_source_time(source_clip, asset, *seconds)?;
                    *seconds
                }
                _ => {
                    if let Some(clip) = source_clip {
                        if !global_frame_in_clip(ctx.project, clip, target, timeline_fps)
                            && !matches!(
                                relation,
                                MediaBindingRelation::TouchingPrevious
                                    | MediaBindingRelation::TouchingNext
                            )
                        {
                            return Err(MediaBindingError::FrameOutsideCoverage {
                                detail: format!(
                                    "output time {} is outside source clip {}–{}.",
                                    format_timecode(target),
                                    format_timecode(clip.start_time),
                                    format_timecode(effective_clip_end(ctx.project, clip))
                                ),
                            });
                        }
                        clip.source_time_at(target, asset.duration_seconds)
                    } else {
                        match at {
                            MediaFramePoint::SourceStart => 0.0,
                            MediaFramePoint::SourceEnd => asset_level_last_time(asset, source_fps),
                            _ => {
                                return Err(MediaBindingError::UnsupportedSample {
                                    detail:
                                        "output-relative sampling needs a timeline clip source."
                                            .to_string(),
                                });
                            }
                        }
                    }
                }
            };
            (target, source_time)
        }
    };
    plan.target_frame_time = Some(target_time);
    plan.source_frame_time = Some(source_time.max(0.0));
    plan.uses_original_source = false;
    Ok(())
}

fn apply_aligned_range(
    ctx: MediaResolveContext<'_>,
    plan: &mut MediaResolvePlan,
    source_clip: Option<&Clip>,
    asset: &Asset,
    window: Option<TargetWindow>,
) -> Result<(), MediaBindingError> {
    let window = window
        .or_else(|| target_window_from_context(ctx))
        .ok_or(MediaBindingError::ContextRequired)?;
    let clip = source_clip.ok_or_else(|| MediaBindingError::UnsupportedSample {
        detail: "aligned range requires a timeline clip source.".to_string(),
    })?;
    if !range_covers(
        clip.start_time,
        clip.end_time(),
        window.start,
        window.end,
        window.fps,
    ) {
        return Err(strict_range_error(window, clip, ctx.project));
    }
    let source_start = clip.source_time_at(window.start, asset.duration_seconds);
    let source_end = clip.source_time_at(window.end, asset.duration_seconds);
    let source_range =
        MediaTimeRange::new(source_start.min(source_end), source_start.max(source_end));
    plan.target_range = Some(window.range());
    plan.source_range = Some(source_range);
    let target_duration = window.range().duration();
    if (source_range.duration() - target_duration).abs() > 1.0 / window.fps.max(1.0) {
        plan.retime_to_duration = Some(target_duration);
    }
    plan.uses_original_source = false;
    Ok(())
}

fn apply_source_range(
    plan: &mut MediaResolvePlan,
    source_clip: Option<&Clip>,
    asset: &Asset,
    start_seconds: f64,
    duration_seconds: f64,
    timeline_fps: f64,
) -> Result<(), MediaBindingError> {
    if duration_seconds <= 0.0 {
        return Err(MediaBindingError::UnsupportedSample {
            detail: "source range duration must be greater than zero.".to_string(),
        });
    }
    let end = start_seconds + duration_seconds;
    if let Some(clip) = source_clip {
        let fps = source_media_fps(asset, timeline_fps).max(1.0);
        let visible_start = clip.source_time_for_local(0.0, asset.duration_seconds);
        let visible_end = last_visible_source_time(clip, asset, fps).max(visible_start);
        if start_seconds + 1e-6 < visible_start || end - 1e-6 > visible_end + 1.0 / fps {
            return Err(MediaBindingError::FrameOutsideCoverage {
                detail: format!(
                    "source range {}–{} is outside the clip's visible source span {}–{}.",
                    format_timecode(start_seconds),
                    format_timecode(end),
                    format_timecode(visible_start),
                    format_timecode(visible_end)
                ),
            });
        }
    } else if let Some(duration) = asset.duration_seconds {
        if start_seconds < -1e-6 || end - 1e-6 > duration {
            return Err(MediaBindingError::FrameOutsideCoverage {
                detail: format!(
                    "source range {}–{} is outside asset duration {}.",
                    format_timecode(start_seconds),
                    format_timecode(end),
                    format_timecode(duration)
                ),
            });
        }
    }
    plan.source_range = Some(MediaTimeRange::new(start_seconds, end));
    plan.uses_original_source = false;
    Ok(())
}

fn fill_source_identity(
    project: &Project,
    plan: &mut MediaResolvePlan,
    asset: &Asset,
    clip_id: Option<Uuid>,
    version: Option<&str>,
    version_is_required_for_generative: bool,
) -> Result<(), MediaBindingError> {
    plan.source_asset_id = Some(asset.id);
    plan.source_clip_id = clip_id;
    let requested_version = if asset.is_generative() {
        if let Some(version) = version.filter(|version| !version.trim().is_empty()) {
            Some(version.to_string())
        } else if version_is_required_for_generative {
            asset.active_version().map(str::to_string)
        } else {
            asset.active_version().map(str::to_string)
        }
    } else {
        None
    };
    if asset.is_generative() && requested_version.is_none() {
        return Err(MediaBindingError::SourceMissing {
            detail: format!("{} has no generated version yet.", asset.name),
        });
    }
    plan.source_version = requested_version.clone();
    let Some(root) = project.project_path.as_ref() else {
        return Err(MediaBindingError::SourceMissing {
            detail: "project folder is unavailable.".to_string(),
        });
    };
    let absolute = if asset.is_generative() {
        generative_asset_source_path(root, asset, requested_version.as_deref())
    } else if asset.is_video() {
        video_asset_source_path(root, asset)
    } else {
        active_asset_source_path(root, asset)
    };
    let Some(absolute) = absolute.filter(|path| path.exists()) else {
        if let Some(version) = requested_version {
            return Err(MediaBindingError::SourceVersionMissing { version });
        }
        return Err(MediaBindingError::SourceMissing {
            detail: format!("{} has no readable source file.", asset.name),
        });
    };
    plan.source_path_absolute = Some(absolute.clone());
    plan.source_path = Some(project_relative(root, &absolute));
    Ok(())
}

fn requested_global_time(
    _ctx: MediaResolveContext<'_>,
    plan: &MediaResolvePlan,
    window: Option<TargetWindow>,
    _source_clip: Option<&Clip>,
) -> Result<Option<f64>, MediaBindingError> {
    let MediaSample::Frame { at } = &plan.normalized_sample else {
        return Ok(None);
    };
    if at.is_source_relative() {
        return Ok(window.map(|window| window.first_global()));
    }
    let window = window.ok_or(MediaBindingError::ContextRequired)?;
    match at {
        MediaFramePoint::OutputStart => Ok(Some(window.first_global())),
        MediaFramePoint::OutputEnd => Ok(Some(window.last_global())),
        MediaFramePoint::OutputOffset { seconds } => {
            let time = window.start + *seconds;
            if time + 1e-6 < window.first_global() || time - 1e-6 > window.last_global() {
                return Err(MediaBindingError::OutputSampleOutOfRange {
                    detail: format!("output offset {seconds:.3}s is outside the generated clip."),
                });
            }
            Ok(Some(time))
        }
        MediaFramePoint::OutputFrame { frame } => {
            if let Some(count) = window.frame_count {
                if *frame >= count {
                    return Err(MediaBindingError::OutputSampleOutOfRange {
                        detail: format!("output frame {frame} is outside {count} frames."),
                    });
                }
            }
            Ok(Some(window.start + (*frame as f64) / window.fps.max(1.0)))
        }
        _ => Ok(None),
    }
}

fn frame_orientation(sample: &MediaSample, window: TargetWindow) -> FrameOrientation {
    match sample {
        MediaSample::Frame {
            at: MediaFramePoint::OutputStart,
        } => FrameOrientation::Start,
        MediaSample::Frame {
            at: MediaFramePoint::OutputEnd,
        } => FrameOrientation::End,
        MediaSample::Frame {
            at: MediaFramePoint::OutputFrame { frame },
        } => {
            if *frame == 0 {
                FrameOrientation::Start
            } else if window
                .frame_count
                .is_some_and(|count| count > 0 && *frame == count - 1)
            {
                FrameOrientation::End
            } else {
                FrameOrientation::Interior
            }
        }
        MediaSample::Frame {
            at: MediaFramePoint::OutputOffset { seconds },
        } => {
            if *seconds <= 1e-6 {
                FrameOrientation::Start
            } else if (*seconds - (window.last_global() - window.start)).abs() <= 1.0 / window.fps {
                FrameOrientation::End
            } else {
                FrameOrientation::Interior
            }
        }
        MediaSample::Frame { at } if at.is_source_relative() => FrameOrientation::Source,
        _ => FrameOrientation::Start,
    }
}

fn classify_frame_candidate(
    project: &Project,
    context: &Clip,
    clip: &Clip,
    _asset: &Asset,
    requested: f64,
    orientation: FrameOrientation,
    fps: f64,
) -> Option<MediaBindingRelation> {
    if project.is_keyframe_reference_clip(clip) {
        if boundary_frame(clip.start_time, fps) == boundary_frame(requested, fps) {
            return Some(MediaBindingRelation::ExactKeyframe);
        }
        return None;
    }
    if clip.track_id == context.track_id
        && orientation == FrameOrientation::Start
        && boundaries_touch(clip.end_time(), context.start_time, fps)
    {
        return Some(MediaBindingRelation::TouchingPrevious);
    }
    if clip.track_id == context.track_id
        && orientation == FrameOrientation::End
        && boundaries_touch(clip.start_time, context.end_time(), fps)
    {
        return Some(MediaBindingRelation::TouchingNext);
    }
    if global_frame_in_clip(project, clip, requested, fps) {
        return Some(MediaBindingRelation::CoveringFrame);
    }
    None
}

fn eligible_source_clips(
    ctx: MediaResolveContext<'_>,
    query: &TimelineSourceQuery,
    context: &Clip,
    media_type: BoundMediaType,
) -> Vec<Clip> {
    let context_track_index = track_index(ctx.project, context.track_id);
    ctx.project
        .clips
        .iter()
        .filter(|clip| clip.id != context.id)
        .filter(|clip| {
            ctx.target_asset_id
                .is_none_or(|target| clip.asset_id != target)
        })
        .filter(|clip| {
            ctx.project
                .find_asset(clip.asset_id)
                .is_some_and(|asset| source_compatible_with_field(asset, media_type))
        })
        .filter(|clip| track_in_scope(ctx.project, query, context, context_track_index, clip))
        .filter(|clip| {
            ctx.project
                .find_asset(clip.asset_id)
                .is_some_and(|asset| source_file_ready(ctx.project, asset, None))
        })
        .cloned()
        .collect()
}

fn track_in_scope(
    project: &Project,
    query: &TimelineSourceQuery,
    context: &Clip,
    context_track_index: Option<usize>,
    clip: &Clip,
) -> bool {
    let Some(track) = project.find_track(clip.track_id) else {
        return false;
    };
    if track.track_type == TrackType::Marker {
        return false;
    }
    match &query.scope {
        TimelineTrackScope::Auto => true,
        TimelineTrackScope::SameTrack => clip.track_id == context.track_id,
        TimelineTrackScope::Below => {
            match (context_track_index, track_index(project, clip.track_id)) {
                (Some(context_idx), Some(clip_idx)) => clip_idx > context_idx,
                _ => false,
            }
        }
        TimelineTrackScope::SpecificTrack { track_id } => clip.track_id == *track_id,
    }
}

fn make_ranked(
    project: &Project,
    context: &Clip,
    clip: &Clip,
    relation: MediaBindingRelation,
    prefer_touching: bool,
    requested: f64,
    _fps: f64,
) -> RankedCandidate {
    let context_idx = track_index(project, context.track_id).unwrap_or(0) as i64;
    let clip_idx = track_index(project, clip.track_id).unwrap_or(0) as i64;
    RankedCandidate {
        clip_id: clip.id,
        asset_id: clip.asset_id,
        relation,
        relation_rank: relation_rank(relation, prefer_touching),
        track_rank: track_priority_rank(clip_idx, context_idx),
        track_distance: (clip_idx - context_idx).abs(),
        start_time: clip.start_time,
        temporal_distance: (clip.start_time - requested).abs(),
    }
}

fn relation_rank(relation: MediaBindingRelation, prefer_touching: bool) -> u8 {
    match relation {
        MediaBindingRelation::ExactKeyframe => 0,
        MediaBindingRelation::TouchingPrevious | MediaBindingRelation::TouchingNext => {
            if prefer_touching {
                1
            } else {
                2
            }
        }
        MediaBindingRelation::CoveringFrame | MediaBindingRelation::CoveringRange => {
            if prefer_touching {
                2
            } else {
                1
            }
        }
        _ => 3,
    }
}

fn track_priority_rank(source_idx: i64, target_idx: i64) -> (u8, i64) {
    if source_idx == target_idx {
        (0, 0)
    } else if source_idx > target_idx {
        (1, source_idx - target_idx)
    } else {
        (2, target_idx - source_idx)
    }
}

fn pick_winner(ranked: &mut [RankedCandidate]) -> Option<RankedCandidate> {
    ranked.sort_by(|left, right| {
        left.relation_rank
            .cmp(&right.relation_rank)
            .then(left.track_rank.cmp(&right.track_rank))
            .then(
                left.temporal_distance
                    .partial_cmp(&right.temporal_distance)
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
            .then(left.track_distance.cmp(&right.track_distance))
            .then(
                left.start_time
                    .partial_cmp(&right.start_time)
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
            .then(left.clip_id.cmp(&right.clip_id))
    });
    ranked.first().cloned()
}

fn ranking_explanation(
    ranked: &[RankedCandidate],
    winner: &RankedCandidate,
    query: &TimelineSourceQuery,
) -> Option<String> {
    let same_relation = ranked
        .iter()
        .filter(|candidate| candidate.relation == winner.relation)
        .count();
    if same_relation <= 1 && ranked.len() <= 1 {
        return Some(format!(
            "Selected {} ({}) with {} scope.",
            winner.relation.label(),
            if query.prefer_touching {
                "prefer touching"
            } else {
                "prefer covering"
            },
            query.scope.label().to_ascii_lowercase()
        ));
    }
    Some(format!(
        "Resolved from {same_relation} {} candidates; chose {} via track priority ({}).",
        winner.relation.label(),
        winner.relation.label(),
        query.scope.label().to_ascii_lowercase()
    ))
}

fn context_clip(ctx: MediaResolveContext<'_>) -> Option<Clip> {
    ctx.context_clip_id
        .and_then(|id| ctx.project.clips.iter().find(|clip| clip.id == id).cloned())
}

fn context_clip_error(ctx: MediaResolveContext<'_>) -> Option<MediaBindingError> {
    let Some(clip_id) = ctx.context_clip_id else {
        return None;
    };
    let Some(clip) = ctx.project.clips.iter().find(|clip| clip.id == clip_id) else {
        return Some(MediaBindingError::InvalidContext {
            detail: "the selected generation context clip was not found.".to_string(),
        });
    };
    if let Some(target_id) = ctx.target_asset_id {
        if clip.asset_id != target_id {
            return Some(MediaBindingError::InvalidContext {
                detail: "the generation context clip does not belong to this asset.".to_string(),
            });
        }
    }
    None
}

fn target_window_from_context(ctx: MediaResolveContext<'_>) -> Option<TargetWindow> {
    target_window(ctx, &context_clip(ctx)?)
}

fn target_window(ctx: MediaResolveContext<'_>, context: &Clip) -> Option<TargetWindow> {
    let fps = target_output_fps(ctx);
    let frame_count = ctx
        .target_asset_id
        .and_then(|id| ctx.project.find_asset(id))
        .and_then(|asset| match asset.kind {
            AssetKind::GenerativeVideo { frame_count, .. } if frame_count > 0 => Some(frame_count),
            _ => None,
        });
    Some(TargetWindow {
        start: context.start_time,
        end: context.end_time(),
        fps,
        frame_count,
    })
}

fn target_output_fps(ctx: MediaResolveContext<'_>) -> f64 {
    if let Some(asset) = ctx
        .target_asset_id
        .and_then(|id| ctx.project.find_asset(id))
    {
        if let AssetKind::GenerativeVideo { fps, .. } = asset.kind {
            if fps > 0.0 {
                return fps;
            }
        }
    }
    if let (Some(provider), Some(config)) = (ctx.provider, ctx.config) {
        if let Some(field) = provider
            .inputs
            .iter()
            .find(|input| input.role == Some(InputRole::Fps))
        {
            if let Some(InputValue::Literal { value }) = config.inputs.get(&field.name) {
                if let Some(fps) = json_f64(value) {
                    if fps > 0.0 {
                        return fps;
                    }
                }
            }
        }
    }
    ctx.project.settings.fps.max(1.0)
}

fn json_f64(value: &serde_json::Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_i64().map(|value| value as f64))
        .or_else(|| {
            value
                .as_str()
                .and_then(|value| value.trim().parse::<f64>().ok())
        })
}

fn source_file_ready(project: &Project, asset: &Asset, version: Option<&str>) -> bool {
    let Some(root) = project.project_path.as_ref() else {
        return false;
    };
    if asset.is_generative() {
        let version = version.or_else(|| asset.active_version());
        generative_asset_source_path(root, asset, version).is_some_and(|path| path.exists())
    } else {
        active_asset_source_path(root, asset).is_some_and(|path| path.exists())
    }
}

fn global_frame_in_clip(project: &Project, clip: &Clip, time: f64, fps: f64) -> bool {
    if project.is_keyframe_reference_clip(clip) {
        return boundary_frame(clip.start_time, fps) == boundary_frame(time, fps);
    }
    let start = boundary_frame(clip.start_time, fps);
    let end = boundary_frame(clip.end_time(), fps);
    let frame = boundary_frame(time, fps);
    frame >= start && frame < end
}

fn range_covers(
    source_start: f64,
    source_end: f64,
    target_start: f64,
    target_end: f64,
    fps: f64,
) -> bool {
    boundary_frame(source_start, fps) <= boundary_frame(target_start, fps)
        && boundary_frame(source_end, fps) >= boundary_frame(target_end, fps)
}

fn ranges_overlap(a0: f64, a1: f64, b0: f64, b1: f64) -> bool {
    a0 < b1 && b0 < a1
}

fn effective_clip_end(project: &Project, clip: &Clip) -> f64 {
    project.effective_clip_end_time(clip)
}

fn last_visible_source_time(clip: &Clip, asset: &Asset, fps: f64) -> f64 {
    clip.source_time_for_local(
        (clip.duration - 1.0 / fps.max(1.0)).max(0.0),
        asset.duration_seconds,
    )
}

fn asset_level_last_time(asset: &Asset, fps: f64) -> f64 {
    asset
        .duration_seconds
        .map(|duration| (duration - 1.0 / fps.max(1.0)).max(0.0))
        .unwrap_or(0.0)
}

fn validate_source_time(
    source_clip: Option<&Clip>,
    asset: &Asset,
    seconds: f64,
) -> Result<(), MediaBindingError> {
    if seconds < 0.0 {
        return Err(MediaBindingError::FrameOutsideCoverage {
            detail: "source time cannot be negative.".to_string(),
        });
    }
    if let Some(clip) = source_clip {
        let fps = source_media_fps(asset, 30.0).max(1.0);
        let start = clip.source_time_for_local(0.0, asset.duration_seconds);
        let end = last_visible_source_time(clip, asset, fps);
        if seconds + 1e-6 < start || seconds - 1e-6 > end {
            return Err(MediaBindingError::FrameOutsideCoverage {
                detail: format!(
                    "source time {} is outside the clip's visible source span {}–{}.",
                    format_timecode(seconds),
                    format_timecode(start),
                    format_timecode(end)
                ),
            });
        }
    } else if let Some(duration) = asset.duration_seconds {
        if seconds - 1e-6 > duration {
            return Err(MediaBindingError::FrameOutsideCoverage {
                detail: format!(
                    "source time {} is outside asset duration {}.",
                    format_timecode(seconds),
                    format_timecode(duration)
                ),
            });
        }
    }
    Ok(())
}

fn strict_range_error(window: TargetWindow, clip: &Clip, project: &Project) -> MediaBindingError {
    let missing_head = (clip.start_time - window.start).max(0.0);
    let missing_tail = (window.end - clip.end_time()).max(0.0);
    let mut missing = String::new();
    if missing_head > 1e-4 {
        missing.push_str(&format!(
            " {} missing at the start",
            format_duration(missing_head)
        ));
    }
    if missing_tail > 1e-4 {
        if !missing.is_empty() {
            missing.push(';');
        }
        missing.push_str(&format!(
            " {} missing at the end",
            format_duration(missing_tail)
        ));
    }
    let name = project
        .find_asset(clip.asset_id)
        .map(|asset| asset.name.clone())
        .unwrap_or_else(|| "Source".to_string());
    MediaBindingError::StrictRangeIncomplete {
        detail: format!(
            "{name} covers {}–{}, but this input requires {}–{}. Strict coverage is enabled;{missing}.",
            format_timecode(clip.start_time),
            format_timecode(clip.end_time()),
            format_timecode(window.start),
            format_timecode(window.end)
        ),
    }
}

fn format_duration(seconds: f64) -> String {
    format!("{:.3} s", seconds)
}

fn track_index(project: &Project, track_id: Uuid) -> Option<usize> {
    project.tracks.iter().position(|track| track.id == track_id)
}

fn track_name(project: &Project, track_id: Uuid) -> String {
    project
        .find_track(track_id)
        .map(|track| track.name.clone())
        .unwrap_or_else(|| "the selected track".to_string())
}

fn absolute_project_path(project: &Project, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else if let Some(root) = project.project_path.as_ref() {
        root.join(path)
    } else {
        path.to_path_buf()
    }
}

fn project_relative(root: &Path, path: &Path) -> PathBuf {
    path.strip_prefix(root)
        .map(Path::to_path_buf)
        .unwrap_or_else(|_| path.to_path_buf())
}

pub fn generation_context_placements(project: &Project, asset_id: Uuid) -> Vec<Uuid> {
    let mut ids: Vec<Uuid> = project
        .clips
        .iter()
        .filter(|clip| clip.asset_id == asset_id)
        .map(|clip| clip.id)
        .collect();
    ids.sort();
    ids
}

pub fn resolve_generation_context(
    project: &Project,
    asset_id: Uuid,
    selected_clip_id: Option<Uuid>,
    stored_context_clip_id: Option<Uuid>,
) -> Result<Option<Uuid>, MediaBindingError> {
    let placements = generation_context_placements(project, asset_id);
    if let Some(selected) = selected_clip_id {
        if placements.contains(&selected) {
            return Ok(Some(selected));
        }
    }
    if placements.is_empty() {
        return Ok(None);
    }
    if placements.len() == 1 {
        return Ok(Some(placements[0]));
    }
    if let Some(stored) = stored_context_clip_id {
        if placements.contains(&stored) {
            return Ok(Some(stored));
        }
    }
    Err(MediaBindingError::MultiplePlacementsRequireContext {
        count: placements.len(),
    })
}

pub fn input_value_from_plan(plan: &MediaResolvePlan) -> Option<InputValue> {
    if !plan.is_ok() {
        return None;
    }
    let frame_reference = match &plan.normalized_sample {
        MediaSample::Frame {
            at: MediaFramePoint::SourceStart | MediaFramePoint::OutputStart,
        } => Some(SourceFrameReference::First),
        MediaSample::Frame {
            at: MediaFramePoint::SourceEnd | MediaFramePoint::OutputEnd,
        } => Some(SourceFrameReference::Last),
        _ => None,
    };
    if let (Some(asset_id), Some(version)) = (plan.source_asset_id, plan.source_version.clone()) {
        return Some(InputValue::GenerationRef {
            asset_id,
            version,
            frame_reference,
        });
    }
    let asset_id = plan.source_asset_id?;
    Some(InputValue::AssetRef {
        asset_id,
        source_clip_id: plan.source_clip_id,
        pinned: !matches!(plan.stability, MediaBindingStability::Follow),
        frame_reference,
    })
}

pub fn lock_source_spec(
    plan: &MediaResolvePlan,
    spec: &MediaBindingSpec,
) -> Result<MediaBindingSpec, MediaBindingError> {
    if !plan.is_ok() {
        return Err(plan.errors.first().cloned().unwrap_or(
            MediaBindingError::NoTimelineCandidate {
                detail: "cannot lock an unresolved source.".to_string(),
            },
        ));
    }
    if matches!(plan.stability, MediaBindingStability::FreezeInput) {
        return Ok(spec.clone());
    }
    let source = if let Some(clip_id) = plan.source_clip_id {
        MediaBindingSource::TimelineClip {
            clip_id,
            version: plan.source_version.clone(),
        }
    } else if let Some(asset_id) = plan.source_asset_id {
        MediaBindingSource::ProjectAsset {
            asset_id,
            version: plan.source_version.clone(),
        }
    } else {
        return Err(MediaBindingError::SourceMissing {
            detail: "resolved source has no asset identity to lock.".to_string(),
        });
    };
    Ok(MediaBindingSpec {
        source,
        sample: plan.normalized_sample.clone(),
        coverage: spec.coverage,
    })
}

pub fn return_to_follow_spec(spec: &MediaBindingSpec) -> MediaBindingSpec {
    if let MediaBindingSource::FrozenArtifact {
        original_binding: Some(original),
        ..
    } = &spec.source
    {
        return original.as_ref().clone();
    }
    MediaBindingSpec {
        source: MediaBindingSource::FollowTimeline {
            query: TimelineSourceQuery::default(),
        },
        sample: spec.sample.clone(),
        coverage: spec.coverage,
    }
}

pub fn unfreeze_spec(spec: &MediaBindingSpec) -> Result<MediaBindingSpec, MediaBindingError> {
    match &spec.source {
        MediaBindingSource::FrozenArtifact {
            original_binding: Some(original),
            ..
        } => Ok(original.as_ref().clone()),
        MediaBindingSource::FrozenArtifact {
            original_binding: None,
            ..
        } => Err(MediaBindingError::UnfreezeRequiresNewSource),
        _ => Ok(spec.clone()),
    }
}

pub fn frozen_origin_from_plan(plan: &MediaResolvePlan) -> FrozenMediaOrigin {
    FrozenMediaOrigin {
        source_asset_id: plan.source_asset_id,
        source_clip_id: plan.source_clip_id,
        source_version: plan.source_version.clone(),
        target_frame_time: plan.target_frame_time,
        source_frame_time: plan.source_frame_time,
        target_range: plan.target_range,
        source_range: plan.source_range,
        relation: plan.relation,
    }
}

pub fn resolved_now_summary(project: &Project, plan: &MediaResolvePlan) -> String {
    if let Some(message) = plan.primary_error_message() {
        return format!("Unresolved\n{message}");
    }
    let mut lines = Vec::new();
    lines.push("Resolved now".to_string());
    let mut identity = String::new();
    if let Some(asset_id) = plan.source_asset_id {
        if let Some(asset) = project.find_asset(asset_id) {
            identity.push_str(&crate::state::asset_display_name(asset));
        } else {
            identity.push_str(&asset_id.to_string()[..8]);
        }
    }
    if let Some(version) = &plan.source_version {
        if !identity.is_empty() {
            identity.push(' ');
        }
        identity.push_str(&format!("({version})"));
    }
    if let Some(relation) = plan.relation {
        if !identity.is_empty() {
            identity.push_str(" · ");
        }
        identity.push_str(relation.label());
    }
    if !identity.is_empty() {
        lines.push(identity);
    }
    if let (Some(target), Some(source)) = (plan.target_frame_time, plan.source_frame_time) {
        lines.push(format!(
            "Global {} → source {}",
            format_timecode(target),
            format_timecode(source)
        ));
    } else if let (Some(target), Some(source)) = (plan.target_range, plan.source_range) {
        lines.push(format!(
            "Global {}–{}",
            format_timecode(target.start_seconds),
            format_timecode(target.end_seconds)
        ));
        lines.push(format!(
            "Source {}–{} · {:.3} s",
            format_timecode(source.start_seconds),
            format_timecode(source.end_seconds),
            source.duration()
        ));
    }
    if let Some(source_kind) = plan.source_media_type {
        if source_kind != plan.media_type {
            lines.push(format!(
                "{} → {}",
                source_kind.label(),
                plan.media_type.label()
            ));
        }
    }
    lines.join("\n")
}

/// Hover copy for timeline connectors and inspector ranking details.
pub fn binding_hover_text(project: &Project, plan: &MediaResolvePlan) -> String {
    let mut lines = vec![
        plan.field_label.clone(),
        plan.spec.stability().label().to_string(),
    ];
    if let Some(asset_id) = plan.source_asset_id {
        if let Some(asset) = project.find_asset(asset_id) {
            let mut line = crate::state::asset_display_name(asset);
            if let Some(version) = &plan.source_version {
                line.push_str(&format!(" ({version})"));
            }
            lines.push(line);
        }
    }
    if let Some(clip_id) = plan.source_clip_id {
        if let Some(clip) = project.clips.iter().find(|clip| clip.id == clip_id) {
            let track = project
                .find_track(clip.track_id)
                .map(|track| track.name.as_str())
                .unwrap_or("Track");
            lines.push(format!(
                "{track} · {}–{}",
                format_timecode(clip.start_time),
                format_timecode(clip.end_time())
            ));
        }
    }
    if let Some(relation) = plan.relation {
        lines.push(relation.label().to_string());
    }
    if let (Some(target), Some(source)) = (plan.target_frame_time, plan.source_frame_time) {
        lines.push(format!(
            "Global {} → source {}",
            format_timecode(target),
            format_timecode(source)
        ));
    } else if let (Some(target), Some(source)) = (plan.target_range, plan.source_range) {
        lines.push(format!(
            "Global {}–{} · source {}–{}",
            format_timecode(target.start_seconds),
            format_timecode(target.end_seconds),
            format_timecode(source.start_seconds),
            format_timecode(source.end_seconds)
        ));
    }
    if let Some(explanation) = &plan.ranking_explanation {
        lines.push(explanation.clone());
    }
    for diagnostic in &plan.diagnostics {
        lines.push(diagnostic.clone());
    }
    lines.join("\n")
}

pub fn spec_references_locked_version(
    spec: &MediaBindingSpec,
    project: &Project,
    asset_id: Uuid,
    version: &str,
) -> bool {
    match &spec.source {
        MediaBindingSource::ProjectAsset {
            asset_id: bound_id,
            version: Some(bound_version),
        } => *bound_id == asset_id && bound_version == version,
        MediaBindingSource::TimelineClip {
            clip_id,
            version: Some(bound_version),
        } => {
            bound_version == version
                && project
                    .clips
                    .iter()
                    .any(|clip| clip.id == *clip_id && clip.asset_id == asset_id)
        }
        MediaBindingSource::FrozenArtifact {
            original_binding: Some(original),
            ..
        } => spec_references_locked_version(original, project, asset_id, version),
        _ => false,
    }
}

pub fn config_references_locked_version(
    config: &GenerativeConfig,
    project: &Project,
    asset_id: Uuid,
    version: &str,
) -> bool {
    config
        .media_bindings
        .values()
        .any(|spec| spec_references_locked_version(spec, project, asset_id, version))
        || config.lab_graph.nodes.iter().any(|node| {
            node.media_bindings
                .values()
                .any(|spec| spec_references_locked_version(spec, project, asset_id, version))
        })
}

pub fn rebase_spec_for_duplicate(
    spec: &mut MediaBindingSpec,
    source_asset_id: Uuid,
    new_asset_id: Uuid,
    source_folder: &Path,
    new_folder: &Path,
) {
    match &mut spec.source {
        MediaBindingSource::ProjectAsset { asset_id, .. } if *asset_id == source_asset_id => {
            *asset_id = new_asset_id;
        }
        MediaBindingSource::FrozenArtifact {
            path,
            original_binding,
            origin,
            ..
        } => {
            *path = rebase_frozen_path(path, source_folder, new_folder);
            if let Some(origin) = origin {
                if origin.source_asset_id == Some(source_asset_id) {
                    origin.source_asset_id = Some(new_asset_id);
                }
            }
            if let Some(original) = original_binding {
                rebase_spec_for_duplicate(
                    original,
                    source_asset_id,
                    new_asset_id,
                    source_folder,
                    new_folder,
                );
            }
        }
        _ => {}
    }
}

pub fn rebase_config_media_bindings(
    config: &mut GenerativeConfig,
    source_asset_id: Uuid,
    new_asset_id: Uuid,
    source_folder: &Path,
    new_folder: &Path,
) {
    for spec in config.media_bindings.values_mut() {
        rebase_spec_for_duplicate(
            spec,
            source_asset_id,
            new_asset_id,
            source_folder,
            new_folder,
        );
    }
    for node in config.lab_graph.nodes.iter_mut() {
        for spec in node.media_bindings.values_mut() {
            rebase_spec_for_duplicate(
                spec,
                source_asset_id,
                new_asset_id,
                source_folder,
                new_folder,
            );
        }
    }
    for record in config.versions.iter_mut() {
        for spec in record.media_bindings_snapshot.values_mut() {
            rebase_spec_for_duplicate(
                spec,
                source_asset_id,
                new_asset_id,
                source_folder,
                new_folder,
            );
        }
        for resolved in record.resolved_media_inputs.values_mut() {
            if resolved.source_asset_id == Some(source_asset_id) {
                resolved.source_asset_id = Some(new_asset_id);
            }
            resolved.materialized_path =
                rebase_frozen_path(&resolved.materialized_path, source_folder, new_folder);
            if let Some(path) = resolved.source_path.as_mut() {
                *path = rebase_frozen_path(path, source_folder, new_folder);
            }
        }
    }
}

fn rebase_frozen_path(path: &Path, source_folder: &Path, new_folder: &Path) -> PathBuf {
    if let Ok(rest) = path.strip_prefix(source_folder) {
        return new_folder.join(rest);
    }
    let source = source_folder.to_string_lossy().replace('\\', "/");
    let dest = new_folder.to_string_lossy().replace('\\', "/");
    let normalized = path.to_string_lossy().replace('\\', "/");
    if source.is_empty() {
        return path.to_path_buf();
    }
    PathBuf::from(normalized.replace(&source, &dest))
}

pub fn default_follow_spec() -> MediaBindingSpec {
    MediaBindingSpec::follow_auto()
}

/// Collect Follow/Lock/Freeze labels for inspector menus.
pub fn source_menu_label(spec: &MediaBindingSpec, project: &Project) -> String {
    match &spec.source {
        MediaBindingSource::FollowTimeline { query } => {
            format!("Follow Timeline / {}", query.scope.label())
        }
        MediaBindingSource::TimelineClip { clip_id, version } => {
            let clip = project.clips.iter().find(|clip| clip.id == *clip_id);
            let name = clip
                .and_then(|clip| project.find_asset(clip.asset_id))
                .map(crate::state::asset_display_name)
                .unwrap_or_else(|| "Missing clip".to_string());
            let span = clip
                .map(|clip| {
                    format!(
                        " · {}–{}",
                        format_timecode(clip.start_time),
                        format_timecode(clip.end_time())
                    )
                })
                .unwrap_or_default();
            let version = version
                .as_deref()
                .map(|version| format!(" ({version})"))
                .unwrap_or_default();
            format!("Clip · {name}{version}{span}")
        }
        MediaBindingSource::ProjectAsset {
            asset_id,
            version: Some(version),
        } => {
            let name = project
                .find_asset(*asset_id)
                .map(crate::state::asset_display_name)
                .unwrap_or_else(|| "Missing asset".to_string());
            format!("Version · {name} ({version})")
        }
        MediaBindingSource::ProjectAsset { asset_id, .. } => {
            let name = project
                .find_asset(*asset_id)
                .map(crate::state::asset_display_name)
                .unwrap_or_else(|| "Missing asset".to_string());
            format!("Asset · {name}")
        }
        MediaBindingSource::FrozenArtifact { path, .. } => {
            let file = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("frozen");
            format!("Frozen Input · {file}")
        }
    }
}

pub fn sample_options_for_field(input: &ProviderInputField) -> Vec<MediaSample> {
    match input.input_type {
        ProviderInputType::Image => vec![
            MediaSample::Frame {
                at: MediaFramePoint::OutputStart,
            },
            MediaSample::Frame {
                at: MediaFramePoint::OutputEnd,
            },
            MediaSample::Frame {
                at: MediaFramePoint::SourceStart,
            },
            MediaSample::Frame {
                at: MediaFramePoint::SourceEnd,
            },
        ],
        ProviderInputType::Video | ProviderInputType::Audio => vec![
            MediaSample::AlignedRange,
            MediaSample::SourceRange {
                start_seconds: 0.0,
                duration_seconds: 1.0,
            },
            MediaSample::Whole,
        ],
        _ => Vec::new(),
    }
}

pub fn sample_matches_option(sample: &MediaSample, option: &MediaSample) -> bool {
    match (sample, option) {
        (MediaSample::Frame { at: left }, MediaSample::Frame { at: right }) => {
            std::mem::discriminant(left) == std::mem::discriminant(right)
        }
        (MediaSample::Auto, _) => false,
        (left, right) => std::mem::discriminant(left) == std::mem::discriminant(right),
    }
}

/// Build a compact inspector map for automation/state.
pub fn binding_inspection_json(project: &Project, plan: &MediaResolvePlan) -> serde_json::Value {
    serde_json::json!({
        "field": plan.field_name,
        "label": plan.field_label,
        "binding": plan.spec,
        "stability": plan.stability,
        "relation": plan.relation,
        "normalized_sample": plan.normalized_sample,
        "ok": plan.is_ok(),
        "summary": resolved_now_summary(project, plan),
        "ranking": plan.ranking_explanation,
        "errors": plan.error_messages(),
        "diagnostics": plan.diagnostics,
        "source_asset_id": plan.source_asset_id,
        "source_clip_id": plan.source_clip_id,
        "source_version": plan.source_version,
        "target_frame_time": plan.target_frame_time,
        "source_frame_time": plan.source_frame_time,
        "target_range": plan.target_range,
        "source_range": plan.source_range,
    })
}

pub fn inspect_config_media_bindings(
    project: &Project,
    asset_id: Uuid,
    context_clip_id: Option<Uuid>,
    provider: &ProviderEntry,
    config: &GenerativeConfig,
) -> HashMap<String, serde_json::Value> {
    let mut map = HashMap::new();
    for input in provider.inputs.iter() {
        if bound_media_type_for_input(input).is_none() {
            continue;
        }
        let ctx = MediaResolveContext {
            project,
            target_asset_id: Some(asset_id),
            context_clip_id,
            field: input,
            provider: Some(provider),
            config: Some(config),
        };
        if let Some(plan) = resolve_field(ctx) {
            map.insert(input.name.clone(), binding_inspection_json(project, &plan));
        } else if input.required {
            map.insert(
                input.name.clone(),
                serde_json::json!({
                    "field": input.name,
                    "label": input.label,
                    "ok": false,
                    "summary": format!("Unresolved\n{}: choose a source.", input.label),
                    "errors": [format!("{}: choose a source.", input.label)],
                }),
            );
        }
    }
    map
}
