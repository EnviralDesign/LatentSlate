//! Disk materialization, freeze, and cache for resolved media bindings.

use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::core::generation::{
    decode_video_reference_frame, extract_video_reference_frame_with_command,
};
use crate::core::media_binding::{frozen_origin_from_plan, MediaBindingError, MediaResolvePlan};
use crate::state::{BoundMediaType, MediaBindingSource, MediaBindingSpec, Project};

/// Bump when materializer command/filter behavior changes so cache keys miss.
pub const MEDIA_MATERIALIZER_REVISION: u32 = 1;

pub fn materialize_plan(
    project: &Project,
    plan: &MediaResolvePlan,
) -> Result<PathBuf, MediaBindingError> {
    if !plan.is_ok() {
        return Err(plan.errors.first().cloned().unwrap_or(
            MediaBindingError::MaterializationFailed {
                detail: "cannot materialize an unresolved binding.".to_string(),
            },
        ));
    }
    let root = project
        .project_path
        .as_ref()
        .ok_or_else(|| MediaBindingError::SourceMissing {
            detail: "project folder is unavailable.".to_string(),
        })?;
    if plan.uses_original_source {
        if let Some(path) = plan
            .source_path_absolute
            .clone()
            .filter(|path| path.exists())
        {
            return Ok(path);
        }
        if let Some(rel) = &plan.source_path {
            let absolute = if rel.is_absolute() {
                rel.clone()
            } else {
                root.join(rel)
            };
            if absolute.exists() {
                return Ok(absolute);
            }
        }
        return Err(MediaBindingError::SourceMissing {
            detail: "resolved source file is missing.".to_string(),
        });
    }

    let source =
        plan.source_path_absolute
            .clone()
            .ok_or_else(|| MediaBindingError::SourceMissing {
                detail: "resolved source file is missing.".to_string(),
            })?;

    match plan.media_type {
        BoundMediaType::Image => materialize_image_frame(root, plan, &source),
        BoundMediaType::Video => materialize_video_range(root, plan, &source),
        BoundMediaType::Audio => materialize_audio_range(root, plan, &source),
    }
}

pub fn freeze_binding(
    project: &Project,
    target_folder: &Path,
    field_name: &str,
    spec: &MediaBindingSpec,
    plan: &MediaResolvePlan,
) -> Result<MediaBindingSpec, MediaBindingError> {
    let materialized = materialize_plan(project, plan)?;
    let root = project
        .project_path
        .as_ref()
        .ok_or_else(|| MediaBindingError::SourceMissing {
            detail: "project folder is unavailable.".to_string(),
        })?;
    let extension = materialized
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or(match plan.media_type {
            BoundMediaType::Image => "png",
            BoundMediaType::Video => "mp4",
            BoundMediaType::Audio => "wav",
        });
    let dest_dir = target_folder
        .join("inputs")
        .join("frozen")
        .join(sanitize_name(field_name));
    fs::create_dir_all(&dest_dir).map_err(|err| MediaBindingError::MaterializationFailed {
        detail: format!("failed to create frozen input folder: {err}"),
    })?;
    let dest = dest_dir.join(format!("{}.{extension}", sanitize_name(field_name)));
    copy_atomic(&materialized, &dest)?;
    let relative = dest
        .strip_prefix(root)
        .map(Path::to_path_buf)
        .unwrap_or(dest);
    Ok(MediaBindingSpec {
        source: MediaBindingSource::FrozenArtifact {
            path: relative,
            media_type: plan.media_type,
            original_binding: Some(Box::new(spec.clone())),
            origin: Some(frozen_origin_from_plan(plan)),
        },
        sample: plan.normalized_sample.clone(),
        coverage: spec.coverage,
    })
}

pub fn unfreeze_binding(spec: &MediaBindingSpec) -> Result<MediaBindingSpec, MediaBindingError> {
    crate::core::media_binding::unfreeze_spec(spec)
}

fn materialize_image_frame(
    root: &Path,
    plan: &MediaResolvePlan,
    source: &Path,
) -> Result<PathBuf, MediaBindingError> {
    let time = plan.source_frame_time.unwrap_or(0.0);
    if plan.source_media_type == Some(BoundMediaType::Image) {
        return Ok(source.to_path_buf());
    }
    let cache_dir = root.join(".cache").join("media_inputs");
    fs::create_dir_all(&cache_dir).map_err(|err| MediaBindingError::MaterializationFailed {
        detail: format!("failed to create media input cache: {err}"),
    })?;
    let output = cache_dir.join(format!("{}.png", cache_key(plan, "png-frame")));
    if output.exists() {
        return Ok(output);
    }
    let tmp = temp_sibling(&output);
    if extract_video_reference_frame_with_command(time, source, &tmp).is_some() {
        commit_atomic(&tmp, &output)?;
        return Ok(output);
    }
    if decode_video_reference_frame(
        plan.source_asset_id.unwrap_or(uuid::Uuid::nil()),
        time,
        source,
        &tmp,
    )
    .is_some()
    {
        commit_atomic(&tmp, &output)?;
        return Ok(output);
    }
    let _ = fs::remove_file(&tmp);
    Err(MediaBindingError::FfmpegFailed {
        detail: format!(
            "could not extract a frame at {} from {}.",
            crate::core::media_binding::format_timecode(time),
            source.display()
        ),
    })
}

fn materialize_video_range(
    root: &Path,
    plan: &MediaResolvePlan,
    source: &Path,
) -> Result<PathBuf, MediaBindingError> {
    let range = plan
        .source_range
        .ok_or_else(|| MediaBindingError::UnsupportedSample {
            detail: "video materialization needs a source range.".to_string(),
        })?;
    let duration = range.duration().max(1.0 / 120.0);
    let target_duration = plan.retime_to_duration.unwrap_or(duration);
    let cache_dir = root.join(".cache").join("media_inputs");
    fs::create_dir_all(&cache_dir).map_err(|err| MediaBindingError::MaterializationFailed {
        detail: format!("failed to create media input cache: {err}"),
    })?;
    let output = cache_dir.join(format!("{}.mp4", cache_key(plan, "mp4-range")));
    if output.exists() {
        return Ok(output);
    }
    let tmp = temp_sibling(&output);
    let speed = (target_duration / duration).max(0.001);
    let filter = if (speed - 1.0).abs() > 0.001 {
        format!(
            "setpts=PTS*{:.9},scale=trunc(iw/2)*2:trunc(ih/2)*2",
            1.0 / speed
        )
    } else {
        "scale=trunc(iw/2)*2:trunc(ih/2)*2".to_string()
    };
    let status = Command::new("ffmpeg")
        .arg("-hide_banner")
        .arg("-loglevel")
        .arg("error")
        .arg("-y")
        .arg("-ss")
        .arg(format!("{:.6}", range.start_seconds.max(0.0)))
        .arg("-t")
        .arg(format!("{:.6}", duration))
        .arg("-i")
        .arg(source)
        .arg("-an")
        .arg("-vf")
        .arg(&filter)
        .arg("-pix_fmt")
        .arg("yuv420p")
        .arg("-c:v")
        .arg("libx264")
        .arg("-preset")
        .arg("veryfast")
        .arg("-crf")
        .arg("18")
        .arg(&tmp)
        .status()
        .map_err(|err| MediaBindingError::FfmpegFailed {
            detail: format!("ffmpeg failed to start: {err}"),
        })?;
    if !status.success() || !tmp.exists() {
        let _ = fs::remove_file(&tmp);
        return Err(MediaBindingError::FfmpegFailed {
            detail: "ffmpeg could not extract the aligned video range.".to_string(),
        });
    }
    commit_atomic(&tmp, &output)?;
    Ok(output)
}

fn materialize_audio_range(
    root: &Path,
    plan: &MediaResolvePlan,
    source: &Path,
) -> Result<PathBuf, MediaBindingError> {
    let range = plan
        .source_range
        .ok_or_else(|| MediaBindingError::UnsupportedSample {
            detail: "audio materialization needs a source range.".to_string(),
        })?;
    let duration = range.duration().max(1.0 / 120.0);
    let target_duration = plan.retime_to_duration.unwrap_or(duration);
    let cache_dir = root.join(".cache").join("media_inputs");
    fs::create_dir_all(&cache_dir).map_err(|err| MediaBindingError::MaterializationFailed {
        detail: format!("failed to create media input cache: {err}"),
    })?;
    let output = cache_dir.join(format!("{}.wav", cache_key(plan, "wav-range")));
    if output.exists() {
        return Ok(output);
    }
    let tmp = temp_sibling(&output);
    let tempo = duration / target_duration.max(1e-6);
    let mut command = Command::new("ffmpeg");
    command
        .arg("-hide_banner")
        .arg("-loglevel")
        .arg("error")
        .arg("-y")
        .arg("-ss")
        .arg(format!("{:.6}", range.start_seconds.max(0.0)))
        .arg("-t")
        .arg(format!("{:.6}", duration))
        .arg("-i")
        .arg(source)
        .arg("-vn");
    if (tempo - 1.0).abs() > 0.001 {
        let filter = atempo_filter(tempo)?;
        command.arg("-filter:a").arg(filter);
    }
    let status = command
        .arg("-acodec")
        .arg("pcm_s16le")
        .arg("-ar")
        .arg("48000")
        .arg(&tmp)
        .status()
        .map_err(|err| MediaBindingError::FfmpegFailed {
            detail: format!("ffmpeg failed to start: {err}"),
        })?;
    if !status.success() || !tmp.exists() {
        let _ = fs::remove_file(&tmp);
        return Err(MediaBindingError::FfmpegFailed {
            detail: "ffmpeg could not extract the aligned audio range.".to_string(),
        });
    }
    commit_atomic(&tmp, &output)?;
    Ok(output)
}

fn atempo_filter(mut tempo: f64) -> Result<String, MediaBindingError> {
    if !tempo.is_finite() || tempo <= 0.0 {
        return Err(MediaBindingError::MaterializationFailed {
            detail: format!("cannot represent audio stretch factor {tempo}."),
        });
    }
    let mut stages = Vec::new();
    while tempo > 2.0 + 1e-6 {
        stages.push(2.0);
        tempo /= 2.0;
        if stages.len() > 8 {
            return Err(MediaBindingError::MaterializationFailed {
                detail: "audio stretch factor is too extreme for atempo.".to_string(),
            });
        }
    }
    while tempo < 0.5 - 1e-6 {
        stages.push(0.5);
        tempo /= 0.5;
        if stages.len() > 8 {
            return Err(MediaBindingError::MaterializationFailed {
                detail: "audio stretch factor is too extreme for atempo.".to_string(),
            });
        }
    }
    stages.push(tempo);
    Ok(stages
        .into_iter()
        .map(|value| format!("atempo={value:.6}"))
        .collect::<Vec<_>>()
        .join(","))
}

fn cache_key(plan: &MediaResolvePlan, kind: &str) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    kind.hash(&mut hasher);
    MEDIA_MATERIALIZER_REVISION.hash(&mut hasher);
    plan.field_name.hash(&mut hasher);
    plan.source_asset_id.hash(&mut hasher);
    plan.source_clip_id.hash(&mut hasher);
    plan.source_version.hash(&mut hasher);
    plan.source_path.hash(&mut hasher);
    hash_f64(&mut hasher, plan.source_frame_time);
    hash_f64(&mut hasher, plan.target_frame_time);
    if let Some(range) = plan.source_range {
        hash_f64(&mut hasher, Some(range.start_seconds));
        hash_f64(&mut hasher, Some(range.end_seconds));
    }
    if let Some(range) = plan.target_range {
        hash_f64(&mut hasher, Some(range.start_seconds));
        hash_f64(&mut hasher, Some(range.end_seconds));
    }
    hash_f64(&mut hasher, plan.retime_to_duration);
    format!("{:016x}", hasher.finish())
}

fn hash_f64(hasher: &mut std::collections::hash_map::DefaultHasher, value: Option<f64>) {
    match value {
        Some(value) if value.is_finite() => ((value * 1_000_000.0).round() as i64).hash(hasher),
        _ => 0u8.hash(hasher),
    }
}

fn sanitize_name(value: &str) -> String {
    let mut out = String::new();
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() {
        "input".to_string()
    } else {
        out
    }
}

fn temp_sibling(path: &Path) -> PathBuf {
    let mut tmp = path.as_os_str().to_os_string();
    tmp.push(".tmp");
    PathBuf::from(tmp)
}

fn copy_atomic(source: &Path, dest: &Path) -> Result<(), MediaBindingError> {
    if source == dest {
        return Ok(());
    }
    let tmp = temp_sibling(dest);
    fs::copy(source, &tmp).map_err(|err| MediaBindingError::MaterializationFailed {
        detail: format!("failed to copy frozen input: {err}"),
    })?;
    commit_atomic(&tmp, dest)
}

fn commit_atomic(tmp: &Path, dest: &Path) -> Result<(), MediaBindingError> {
    if dest.exists() {
        let _ = fs::remove_file(dest);
    }
    fs::rename(tmp, dest).map_err(|err| {
        let _ = fs::remove_file(tmp);
        MediaBindingError::MaterializationFailed {
            detail: format!("failed to finalize media file: {err}"),
        }
    })
}
