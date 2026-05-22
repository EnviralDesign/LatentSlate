#![allow(dead_code)]
// Provider input resolution is intentionally staged while the egui migration
// rebuilds the generation/attributes surface. Do not delete this without also
// replacing provider execution and seed handling.

use std::collections::HashMap;

use serde_json::Value;
use uuid::Uuid;

use crate::state::{
    Asset, AssetKind, GenerativeConfig, InputValue, Project, ProviderEntry, ProviderInputField,
    ProviderInputType,
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
                    if let Some(path) = asset_ref_path(project, &binding) {
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
                let value =
                    literal_input_value(config, &input.name).or_else(|| input.default.clone());

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
        } => {
            if pinned {
                Some(InputValue::AssetRef {
                    asset_id,
                    source_clip_id,
                    pinned,
                })
            } else {
                best_timeline_asset_ref_for_input(project, context_clip_id, input).or(Some(
                    InputValue::AssetRef {
                        asset_id,
                        source_clip_id,
                        pinned,
                    },
                ))
            }
        }
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
            if !compatible_asset_for_provider_input(asset, &input.input_type) {
                return None;
            }
            let clip_anchor = if slot.starts_with("end") {
                clip.start_time
            } else {
                clip.start_time
            };
            let time_distance = (clip_anchor - target_time).abs();
            let track_penalty = match (
                context_track_index,
                project
                    .tracks
                    .iter()
                    .position(|track| track.id == clip.track_id),
            ) {
                (Some(context_index), Some(index)) if index > context_index => 0.0,
                (Some(context_index), Some(index)) => {
                    (context_index as f64 - index as f64).abs() * 0.25
                }
                _ => 1.0,
            };
            Some((time_distance + track_penalty, clip.asset_id, clip.id))
        })
        .min_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(_, asset_id, source_clip_id)| InputValue::AssetRef {
            asset_id,
            source_clip_id: Some(source_clip_id),
            pinned: false,
        })
}

fn asset_ref_path(project: &Project, value: &InputValue) -> Option<std::path::PathBuf> {
    let InputValue::AssetRef { asset_id, .. } = value else {
        return None;
    };
    let root = project.project_path.as_ref()?;
    let asset = project.find_asset(*asset_id)?;
    match &asset.kind {
        AssetKind::Image { path } | AssetKind::Video { path } | AssetKind::Audio { path } => {
            Some(root.join(path))
        }
        AssetKind::GenerativeImage {
            folder,
            active_version,
        } => resolve_generative_source(
            root,
            folder,
            active_version.as_deref(),
            &["png", "jpg", "jpeg", "webp"],
        ),
        AssetKind::GenerativeVideo {
            folder,
            active_version,
            ..
        } => resolve_generative_source(
            root,
            folder,
            active_version.as_deref(),
            &["mp4", "mov", "mkv", "webm"],
        ),
        AssetKind::GenerativeAudio {
            folder,
            active_version,
        } => resolve_generative_source(
            root,
            folder,
            active_version.as_deref(),
            &["wav", "mp3", "ogg", "flac", "m4a"],
        ),
    }
}

fn resolve_generative_source(
    project_root: &std::path::Path,
    folder: &std::path::Path,
    active_version: Option<&str>,
    extensions: &[&str],
) -> Option<std::path::PathBuf> {
    let folder_path = project_root.join(folder);
    if let Some(version) = active_version {
        for extension in extensions {
            let candidate = folder_path.join(format!("{version}.{extension}"));
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }

    let mut entries: Vec<std::path::PathBuf> = std::fs::read_dir(folder_path)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file()
                && path
                    .extension()
                    .and_then(|extension| extension.to_str())
                    .is_some_and(|extension| {
                        extensions
                            .iter()
                            .any(|allowed| allowed.eq_ignore_ascii_case(extension))
                    })
        })
        .collect();
    entries.sort();
    entries.into_iter().next()
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
