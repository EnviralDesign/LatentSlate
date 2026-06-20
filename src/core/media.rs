use serde_json::Value;
use std::path::Path;
use std::process::Command;

/// Probe media duration in seconds using ffprobe.
pub fn probe_duration_seconds(path: &Path) -> Option<f64> {
    let output = Command::new("ffprobe")
        .arg("-v")
        .arg("error")
        .arg("-show_entries")
        .arg("format=duration")
        .arg("-of")
        .arg("default=noprint_wrappers=1:nokey=1")
        .arg(path)
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let duration_str = stdout.trim();
    if duration_str.is_empty() {
        return None;
    }

    duration_str.parse::<f64>().ok()
}

/// Probe image or video frame dimensions in pixels, if available.
pub fn probe_media_dimensions(path: &Path) -> Option<(u32, u32)> {
    if let Ok((width, height)) = image::image_dimensions(path) {
        return Some((width, height));
    }

    let output = Command::new("ffprobe")
        .arg("-v")
        .arg("error")
        .arg("-select_streams")
        .arg("v:0")
        .arg("-show_entries")
        .arg("stream=width,height")
        .arg("-of")
        .arg("json")
        .arg(path)
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    parse_ffprobe_dimensions(&output.stdout)
}

fn parse_ffprobe_dimensions(raw: &[u8]) -> Option<(u32, u32)> {
    let parsed: Value = serde_json::from_slice(raw).ok()?;
    let streams = parsed.get("streams")?.as_array()?;
    let stream = streams.first()?;
    let width = stream.get("width").and_then(|value| value.as_u64())?;
    let height = stream.get("height").and_then(|value| value.as_u64())?;
    Some((u32::try_from(width).ok()?, u32::try_from(height).ok()?))
}

pub fn probe_asset_duration(
    project: &mut crate::state::Project,
    asset_id: uuid::Uuid,
) -> Option<f64> {
    let (project_root, asset_path, needs_probe) = {
        let project_root = project.project_path.clone();
        let asset = project.find_asset(asset_id);
        let needs_probe = asset
            .map(|asset| asset.duration_seconds.is_none() && (asset.is_video() || asset.is_audio()))
            .unwrap_or(false);
        let asset_path = asset.and_then(|asset| match &asset.kind {
            crate::state::AssetKind::Video { path } => Some(path.clone()),
            crate::state::AssetKind::Audio { path } => Some(path.clone()),
            _ => None,
        });
        (project_root, asset_path, needs_probe)
    };

    let Some(project_root) = project_root else {
        return None;
    };
    let Some(asset_path) = asset_path else {
        return None;
    };
    if !needs_probe {
        return project.asset_duration_seconds(asset_id);
    }

    let duration = probe_duration_seconds(&project_root.join(asset_path));
    if let Some(duration) = duration {
        project.set_asset_duration(asset_id, Some(duration));
    }
    duration
}

pub fn probe_missing_duration(project: &mut crate::state::Project) {
    let asset_ids: Vec<uuid::Uuid> = project
        .assets
        .iter()
        .filter(|asset| asset.duration_seconds.is_none() && (asset.is_video() || asset.is_audio()))
        .map(|asset| asset.id)
        .collect();

    for asset_id in asset_ids {
        let _ = probe_asset_duration(project, asset_id);
    }
}

pub fn resolve_asset_duration_seconds(
    project: &mut crate::state::Project,
    asset_id: uuid::Uuid,
) -> Option<f64> {
    let (project_root, asset_path, cached_duration, should_probe) = {
        let project_root = project.project_path.clone();
        let asset = project.find_asset(asset_id);
        let cached_duration = asset.and_then(|asset| asset.duration_seconds);
        let should_probe = asset
            .map(|asset| asset.is_video() || asset.is_audio())
            .unwrap_or(false);
        let asset_path = asset.and_then(|asset| match &asset.kind {
            crate::state::AssetKind::Video { path } => Some(path.clone()),
            crate::state::AssetKind::Audio { path } => Some(path.clone()),
            _ => None,
        });
        (project_root, asset_path, cached_duration, should_probe)
    };

    if let Some(duration) = cached_duration {
        return Some(duration);
    }

    if !should_probe {
        return None;
    }

    let Some(project_root) = project_root else {
        return None;
    };
    let Some(asset_path) = asset_path else {
        return None;
    };

    let absolute_path = project_root.join(asset_path);
    let duration = probe_duration_seconds(&absolute_path);
    if let Some(duration) = duration {
        project.set_asset_duration(asset_id, Some(duration));
        return Some(duration);
    }

    None
}
