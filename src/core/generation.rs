#![allow(dead_code)]
// Provider input resolution is intentionally staged while the egui migration
// rebuilds the generation/attributes surface. Do not delete this without also
// replacing provider execution and seed handling.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;
use uuid::Uuid;

use crate::core::video_decode::VideoDecodeWorker;
use crate::state::{
    Asset, AssetKind, Clip, GenerativeConfig, InputValue, Project, ProviderEntry,
    ProviderInputField, ProviderInputType, SourceFrameReference,
};

#[derive(Debug, Clone)]
pub struct ResolvedInputs {
    pub values: HashMap<String, Value>,
    pub snapshot: HashMap<String, InputValue>,
    pub missing_required: Vec<String>,
}

pub fn resolve_provider_inputs(
    project: &Project,
    context_clip_id: Option<Uuid>,
    provider: &ProviderEntry,
    config: &GenerativeConfig,
) -> ResolvedInputs {
    let mut values = HashMap::new();
    let mut snapshot = HashMap::new();
    let mut missing_required = Vec::new();

    for input in provider.inputs.iter() {
        match input.input_type {
            ProviderInputType::Image | ProviderInputType::Video | ProviderInputType::Audio => {
                let binding = asset_input_value(project, context_clip_id, provider, config, input);
                if let Some(binding) = binding {
                    if let Some(path) = asset_ref_path(project, &binding, &input.input_type) {
                        values.insert(
                            input.name.clone(),
                            Value::String(path.to_string_lossy().to_string()),
                        );
                        snapshot.insert(input.name.clone(), binding);
                    } else if input.required {
                        missing_required.push(input.name.clone());
                    }
                } else if input.required {
                    missing_required.push(input.name.clone());
                }
            }
            _ => {
                let mut value =
                    literal_input_value(config, &input.name).or_else(|| input.default.clone());
                if value.is_none()
                    && matches!(
                        input.input_type,
                        ProviderInputType::Integer | ProviderInputType::Number
                    )
                {
                    value = infer_frame_input_from_reference(project, config, input);
                }

                if let Some(value) = value {
                    values.insert(input.name.clone(), value.clone());
                    snapshot.insert(input.name.clone(), InputValue::Literal { value });
                } else if input.required {
                    missing_required.push(input.name.clone());
                }
            }
        }
    }

    ResolvedInputs {
        values,
        snapshot,
        missing_required,
    }
}

fn infer_frame_input_from_reference(
    project: &Project,
    config: &GenerativeConfig,
    input: &ProviderInputField,
) -> Option<Value> {
    let key = format!("{} {}", input.name, input.label).to_ascii_lowercase();
    if !contains_any(&key, &["frame"]) {
        return None;
    }
    let slot = if contains_any(&key, &["end", "last", "final"]) {
        "end_image"
    } else if contains_any(&key, &["start", "first", "initial", "init"]) {
        "start_image"
    } else {
        "image"
    };

    let reference = config.reference_slots.get(slot)?;
    let fps = project.settings.fps.max(1.0);
    let frame_time = reference_frame_time(project, reference, fps)?;
    let frame = (frame_time.max(0.0) * fps).round() as i64;
    if frame >= 0 {
        Some(Value::Number(serde_json::Number::from(frame)))
    } else {
        None
    }
}

fn reference_frame_time(project: &Project, reference: &InputValue, fps: f64) -> Option<f64> {
    match reference {
        InputValue::AssetRef {
            asset_id,
            source_clip_id,
            frame_reference,
            ..
        } => {
            let frame = SourceFrameReference::First;
            let frame_ref = frame_reference.unwrap_or(frame);
            let asset = project.find_asset(*asset_id)?;
            let media_fps = source_media_fps(asset, fps);
            source_clip_id
                .and_then(|clip_id| {
                    project
                        .clips
                        .iter()
                        .find(|clip| clip.id == clip_id)
                        .map(|clip| source_frame_time(clip, frame_ref, media_fps))
                })
                .or_else(|| Some(asset_level_frame_time(asset, frame_ref, media_fps)))
        }
        InputValue::GenerationRef {
            asset_id,
            frame_reference,
            ..
        } => {
            let frame = frame_reference.unwrap_or(SourceFrameReference::First);
            let asset = project.find_asset(*asset_id)?;
            Some(asset_level_frame_time(
                asset,
                frame,
                source_media_fps(asset, fps),
            ))
        }
        InputValue::Literal { .. } => None,
    }
}

pub fn semantic_reference_slot(input: &ProviderInputField) -> Option<&'static str> {
    match input.input_type {
        ProviderInputType::Image => {
            let key = format!("{} {}", input.name, input.label).to_ascii_lowercase();
            if contains_any(&key, &["end", "last", "final"]) {
                Some("end_image")
            } else if contains_any(&key, &["start", "first", "initial", "init"]) {
                Some("start_image")
            } else {
                Some("image")
            }
        }
        ProviderInputType::Video => Some("video"),
        ProviderInputType::Audio => Some("audio"),
        _ => None,
    }
}

pub fn compatible_asset_for_provider_input(asset: &Asset, input_type: &ProviderInputType) -> bool {
    match input_type {
        ProviderInputType::Image => asset.is_image(),
        ProviderInputType::Video => asset.is_video(),
        ProviderInputType::Audio => asset.is_audio(),
        _ => false,
    }
}

pub fn asset_source_available_for_provider_input(
    project: &Project,
    asset: &Asset,
    input_type: &ProviderInputType,
) -> bool {
    let Some(project_root) = project.project_path.as_ref() else {
        return false;
    };
    let source = if matches!(input_type, ProviderInputType::Image) && asset.is_video() {
        video_asset_source_path(project_root, asset)
    } else if compatible_asset_for_provider_input(asset, input_type) {
        active_asset_source_path(project_root, asset)
    } else {
        None
    };
    source.is_some_and(|path| path.exists())
}

fn asset_input_value(
    project: &Project,
    context_clip_id: Option<Uuid>,
    provider: &ProviderEntry,
    config: &GenerativeConfig,
    input: &ProviderInputField,
) -> Option<InputValue> {
    if let Some(value) = config.inputs.get(&input.name) {
        return resolve_unpinned_asset_ref(project, context_clip_id, input, value.clone());
    }

    if let Some(slot) = semantic_reference_slot(input) {
        if let Some(value) = config.reference_slots.get(slot) {
            return resolve_unpinned_asset_ref(project, context_clip_id, input, value.clone());
        }
    }

    best_timeline_asset_ref(project, context_clip_id, input, provider)
}

fn resolve_unpinned_asset_ref(
    project: &Project,
    context_clip_id: Option<Uuid>,
    input: &ProviderInputField,
    value: InputValue,
) -> Option<InputValue> {
    match value {
        InputValue::AssetRef {
            asset_id,
            source_clip_id,
            pinned,
            frame_reference,
        } => {
            if pinned {
                Some(InputValue::AssetRef {
                    asset_id,
                    source_clip_id,
                    pinned,
                    frame_reference,
                })
            } else {
                best_timeline_asset_ref_for_input(project, context_clip_id, input).or(Some(
                    InputValue::AssetRef {
                        asset_id,
                        source_clip_id,
                        pinned,
                        frame_reference,
                    },
                ))
            }
        }
        InputValue::GenerationRef { .. } => Some(value),
        InputValue::Literal { .. } => None,
    }
}

fn best_timeline_asset_ref(
    project: &Project,
    context_clip_id: Option<Uuid>,
    input: &ProviderInputField,
    _provider: &ProviderEntry,
) -> Option<InputValue> {
    best_timeline_asset_ref_for_input(project, context_clip_id, input)
}

fn best_timeline_asset_ref_for_input(
    project: &Project,
    context_clip_id: Option<Uuid>,
    input: &ProviderInputField,
) -> Option<InputValue> {
    let context = context_clip_id.and_then(|id| project.clips.iter().find(|clip| clip.id == id))?;
    let slot = semantic_reference_slot(input).unwrap_or("image");
    let target_time = if slot.starts_with("end") {
        context.end_time()
    } else {
        context.start_time
    };
    let context_track_index = project
        .tracks
        .iter()
        .position(|track| track.id == context.track_id);

    project
        .clips
        .iter()
        .filter(|clip| clip.id != context.id)
        .filter_map(|clip| {
            let asset = project.find_asset(clip.asset_id)?;
            if !asset_source_available_for_provider_input(project, asset, &input.input_type) {
                return None;
            }
            let (time_distance, frame_reference) =
                timeline_asset_candidate(asset, clip, &input.input_type, slot, target_time)?;
            let track_penalty = match (
                context_track_index,
                project
                    .tracks
                    .iter()
                    .position(|track| track.id == clip.track_id),
            ) {
                (Some(context_index), Some(index)) if index == context_index + 1 => 0.0,
                (Some(context_index), Some(index)) if index == context_index => 0.05,
                (Some(context_index), Some(index)) if index > context_index => {
                    0.1 + (index - context_index - 1) as f64 * 0.15
                }
                (Some(context_index), Some(index)) => (context_index - index) as f64 * 0.5,
                _ => 1.0,
            };
            Some((
                time_distance + track_penalty,
                clip.asset_id,
                clip.id,
                frame_reference,
            ))
        })
        .min_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal))
        .map(
            |(_, asset_id, source_clip_id, frame_reference)| InputValue::AssetRef {
                asset_id,
                source_clip_id: Some(source_clip_id),
                pinned: false,
                frame_reference,
            },
        )
}

fn timeline_asset_candidate(
    asset: &Asset,
    clip: &Clip,
    input_type: &ProviderInputType,
    slot: &str,
    target_time: f64,
) -> Option<(f64, Option<SourceFrameReference>)> {
    match input_type {
        ProviderInputType::Image => {
            if asset.is_image() {
                return Some(((clip.start_time - target_time).abs(), None));
            }
            if asset.is_video() {
                let start_distance = (clip.start_time - target_time).abs();
                let end_distance = (clip.end_time() - target_time).abs();
                let prefer_last_for_start =
                    slot.starts_with("start") && end_distance <= start_distance;
                let prefer_first_for_end =
                    slot.starts_with("end") && start_distance <= end_distance;
                let frame = if prefer_last_for_start {
                    SourceFrameReference::Last
                } else if prefer_first_for_end || start_distance <= end_distance {
                    SourceFrameReference::First
                } else {
                    SourceFrameReference::Last
                };
                let distance = match frame {
                    SourceFrameReference::First => start_distance,
                    SourceFrameReference::Last => end_distance,
                };
                return Some((distance, Some(frame)));
            }
            None
        }
        ProviderInputType::Video | ProviderInputType::Audio => {
            if compatible_asset_for_provider_input(asset, input_type) {
                Some(((clip.start_time - target_time).abs(), None))
            } else {
                None
            }
        }
        _ => None,
    }
}

fn asset_ref_path(
    project: &Project,
    value: &InputValue,
    input_type: &ProviderInputType,
) -> Option<std::path::PathBuf> {
    match value {
        InputValue::AssetRef {
            asset_id,
            source_clip_id,
            frame_reference,
            ..
        } => {
            let root = project.project_path.as_ref()?;
            let asset = project.find_asset(*asset_id)?;
            if matches!(input_type, ProviderInputType::Image) && asset.is_video() {
                let source_path = video_asset_source_path(root, asset)?;
                let frame = (*frame_reference).unwrap_or(SourceFrameReference::First);
                let source_clip = (*source_clip_id)
                    .and_then(|clip_id| project.clips.iter().find(|clip| clip.id == clip_id));
                let media_fps = source_media_fps(asset, project.settings.fps);
                let time_seconds = source_clip
                    .map(|clip| source_frame_time(clip, frame, media_fps))
                    .unwrap_or_else(|| asset_level_frame_time(asset, frame, media_fps));
                return extract_video_reference_frame(
                    root,
                    *asset_id,
                    *source_clip_id,
                    None,
                    frame,
                    time_seconds,
                    &source_path,
                );
            }
            if !compatible_asset_for_provider_input(asset, input_type) {
                return None;
            }
            active_asset_source_path(root, asset)
        }
        InputValue::GenerationRef {
            asset_id,
            version,
            frame_reference,
        } => {
            let root = project.project_path.as_ref()?;
            let asset = project.find_asset(*asset_id)?;
            if matches!(input_type, ProviderInputType::Image) && asset.is_video() {
                let source_path = generative_asset_source_path(root, asset, Some(version))?;
                let frame = (*frame_reference).unwrap_or(SourceFrameReference::First);
                let media_fps = source_media_fps(asset, project.settings.fps);
                let time_seconds = asset_level_frame_time(asset, frame, media_fps);
                return extract_video_reference_frame(
                    root,
                    *asset_id,
                    None,
                    Some(version),
                    frame,
                    time_seconds,
                    &source_path,
                );
            }
            if !compatible_asset_for_provider_input(asset, input_type) {
                return None;
            }
            generative_asset_source_path(root, asset, Some(version))
        }
        InputValue::Literal { .. } => None,
    }
}

fn video_asset_source_path(project_root: &Path, asset: &Asset) -> Option<PathBuf> {
    match &asset.kind {
        AssetKind::Video { path } => Some(project_root.join(path)),
        AssetKind::GenerativeVideo { active_version, .. } => {
            generative_asset_source_path(project_root, asset, active_version.as_deref())
        }
        _ => None,
    }
}

fn active_asset_source_path(project_root: &Path, asset: &Asset) -> Option<PathBuf> {
    match &asset.kind {
        AssetKind::Image { path } | AssetKind::Video { path } | AssetKind::Audio { path } => {
            Some(project_root.join(path))
        }
        AssetKind::GenerativeImage { active_version, .. }
        | AssetKind::GenerativeVideo { active_version, .. }
        | AssetKind::GenerativeAudio { active_version, .. } => {
            generative_asset_source_path(project_root, asset, active_version.as_deref())
        }
    }
}

fn generative_asset_source_path(
    project_root: &Path,
    asset: &Asset,
    version: Option<&str>,
) -> Option<PathBuf> {
    match &asset.kind {
        AssetKind::GenerativeImage { folder, .. } => resolve_generative_source(
            project_root,
            folder,
            version,
            &["png", "jpg", "jpeg", "webp"],
        ),
        AssetKind::GenerativeVideo { folder, .. } => resolve_generative_source(
            project_root,
            folder,
            version,
            &["mp4", "mov", "mkv", "webm"],
        ),
        AssetKind::GenerativeAudio { folder, .. } => resolve_generative_source(
            project_root,
            folder,
            version,
            &["wav", "mp3", "ogg", "flac", "m4a"],
        ),
        _ => None,
    }
}

fn resolve_generative_source(
    project_root: &std::path::Path,
    folder: &std::path::Path,
    active_version: Option<&str>,
    extensions: &[&str],
) -> Option<std::path::PathBuf> {
    let active_version = active_version?;
    let folder_path = project_root.join(folder);
    for extension in extensions {
        let candidate = folder_path.join(format!("{active_version}.{extension}"));
        if candidate.exists() {
            return Some(candidate);
        }
    }
    None
}

fn source_frame_time(clip: &Clip, frame: SourceFrameReference, fps: f64) -> f64 {
    let frame_epsilon = 1.0 / fps.max(1.0);
    match frame {
        SourceFrameReference::First => clip.trim_in_seconds.max(0.0),
        SourceFrameReference::Last => (clip.trim_in_seconds + clip.duration - frame_epsilon)
            .max(clip.trim_in_seconds)
            .max(0.0),
    }
}

fn asset_level_frame_time(asset: &Asset, frame: SourceFrameReference, fps: f64) -> f64 {
    match frame {
        SourceFrameReference::First => 0.0,
        SourceFrameReference::Last => {
            let frame_epsilon = 1.0 / fps.max(1.0);
            asset
                .duration_seconds
                .map(|duration| (duration - frame_epsilon).max(0.0))
                .unwrap_or(0.0)
        }
    }
}

fn source_media_fps(asset: &Asset, fallback_fps: f64) -> f64 {
    match asset.kind {
        AssetKind::GenerativeVideo { fps, .. } if fps > 0.0 => fps,
        _ => fallback_fps.max(1.0),
    }
}

fn extract_video_reference_frame(
    project_root: &Path,
    asset_id: Uuid,
    source_clip_id: Option<Uuid>,
    source_version: Option<&str>,
    frame: SourceFrameReference,
    time_seconds: f64,
    source_path: &Path,
) -> Option<PathBuf> {
    let cache_dir = project_root.join(".cache").join("reference_frames");
    std::fs::create_dir_all(&cache_dir).ok()?;
    let time_millis = (time_seconds.max(0.0) * 1000.0).round() as u64;
    let clip_part = source_clip_id
        .map(|id| id.to_string())
        .unwrap_or_else(|| "asset".to_string());
    let version_part = source_version
        .map(|version| format!("_{}", sanitize_cache_key(version)))
        .unwrap_or_default();
    let output_path = cache_dir.join(format!(
        "{}_{}{}_{}_{}.png",
        asset_id,
        clip_part,
        version_part,
        frame.as_str(),
        time_millis
    ));
    if output_path.exists() {
        return Some(output_path);
    }

    if let Some(path) =
        extract_video_reference_frame_with_command(time_seconds, source_path, &output_path)
    {
        return Some(path);
    }
    decode_video_reference_frame(asset_id, time_seconds, source_path, &output_path)
}

fn decode_video_reference_frame(
    asset_id: Uuid,
    time_seconds: f64,
    source_path: &Path,
    output_path: &Path,
) -> Option<PathBuf> {
    let worker = VideoDecodeWorker::new(4096, 4096);
    let lane_id = asset_id.as_u128() as u64;
    let response = worker.decode(source_path, time_seconds.max(0.0), lane_id, false)?;
    let image = response.image?;
    image.save(output_path).ok()?;
    output_path.exists().then(|| output_path.to_path_buf())
}

fn extract_video_reference_frame_with_command(
    time_seconds: f64,
    source_path: &Path,
    output_path: &Path,
) -> Option<PathBuf> {
    let status = Command::new("ffmpeg")
        .arg("-hide_banner")
        .arg("-loglevel")
        .arg("error")
        .arg("-y")
        .arg("-ss")
        .arg(format!("{:.6}", time_seconds.max(0.0)))
        .arg("-i")
        .arg(source_path)
        .arg("-frames:v")
        .arg("1")
        .arg(output_path)
        .status()
        .ok()?;

    if status.success() && output_path.exists() {
        Some(output_path.to_path_buf())
    } else {
        None
    }
}

fn sanitize_cache_key(value: &str) -> String {
    value
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
        .collect()
}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| haystack.contains(needle))
}

pub fn next_version_label(config: &GenerativeConfig) -> String {
    let mut max_version = 0u32;
    for record in config.versions.iter() {
        if let Some(number) = parse_version_number(&record.version) {
            max_version = max_version.max(number);
        }
    }
    if let Some(active) = config.active_version.as_ref() {
        if let Some(number) = parse_version_number(active) {
            max_version = max_version.max(number);
        }
    }
    format!("v{}", max_version + 1)
}

fn literal_input_value(config: &GenerativeConfig, name: &str) -> Option<Value> {
    config.inputs.get(name).and_then(|input| match input {
        InputValue::Literal { value } => Some(value.clone()),
        _ => None,
    })
}

fn parse_version_number(version: &str) -> Option<u32> {
    let trimmed = version.trim();
    let numeric = trimmed
        .strip_prefix('v')
        .or_else(|| trimmed.strip_prefix('V'))?;
    numeric.parse::<u32>().ok()
}

/// Resolve which provider input should be treated as the seed for batching.
pub fn resolve_seed_field(provider: &ProviderEntry, preferred: Option<&str>) -> Option<String> {
    if let Some(preferred) = preferred {
        if provider
            .inputs
            .iter()
            .any(|input| input.name == preferred && is_seed_candidate(input))
        {
            return Some(preferred.to_string());
        }
    }

    provider
        .inputs
        .iter()
        .find(|input| is_seed_candidate(input) && seed_like(&input.name, &input.label))
        .map(|input| input.name.clone())
}

/// Clone inputs and snapshot, overriding the seed field with a new value.
pub fn update_seed_inputs(
    values: &HashMap<String, Value>,
    snapshot: &HashMap<String, InputValue>,
    seed_field: &str,
    seed: i64,
) -> (HashMap<String, Value>, HashMap<String, InputValue>) {
    let mut values = values.clone();
    let mut snapshot = snapshot.clone();
    let seed_value = Value::Number(seed.into());
    values.insert(seed_field.to_string(), seed_value.clone());
    snapshot.insert(
        seed_field.to_string(),
        InputValue::Literal { value: seed_value },
    );
    (values, snapshot)
}

/// Generate a random seed suitable for numeric seed inputs.
pub fn random_seed_i64() -> i64 {
    let raw = Uuid::new_v4().as_u128();
    (raw % i64::MAX as u128) as i64
}

fn seed_like(name: &str, label: &str) -> bool {
    name.to_ascii_lowercase().contains("seed") || label.to_ascii_lowercase().contains("seed")
}

fn is_seed_candidate(input: &ProviderInputField) -> bool {
    matches!(
        input.input_type,
        ProviderInputType::Integer | ProviderInputType::Number
    )
}
