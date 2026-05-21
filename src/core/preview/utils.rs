use std::path::Path;
use std::time::Instant;

use image::imageops::{resize, FilterType};
use image::{Rgba, RgbaImage};

use crate::state::{Asset, AssetKind};

use super::types::FFMPEG_TIME_EPSILON;

pub(crate) fn clamp_time(time_seconds: f64, duration: Option<f64>) -> f64 {
    let mut time = time_seconds.max(0.0);
    if let Some(duration) = duration {
        let limit = (duration - FFMPEG_TIME_EPSILON).max(0.0);
        time = time.min(limit);
    }
    time
}

pub(crate) fn elapsed_ms(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1000.0
}

pub(crate) fn time_to_frame_index(time_seconds: f64, fps: f64) -> i64 {
    let fps = fps.max(1.0);
    (time_seconds.max(0.0) * fps).floor() as i64
}

pub(crate) fn frame_index_to_time(frame_index: i64, fps: f64) -> f64 {
    let fps = fps.max(1.0);
    let frame_index = frame_index.max(0) as f64;
    frame_index / fps
}

pub(crate) fn track_lane_id(track_id: uuid::Uuid) -> u64 {
    let raw = track_id.as_u128();
    (raw as u64) ^ ((raw >> 64) as u64)
}

pub(crate) fn image_size_bytes(image: &RgbaImage) -> usize {
    let width = image.width() as usize;
    let height = image.height() as usize;
    width.saturating_mul(height).saturating_mul(4)
}

pub(crate) fn scale_image_to_fit(image: RgbaImage, max_width: u32, max_height: u32) -> RgbaImage {
    let max_width = max_width.max(1);
    let max_height = max_height.max(1);
    let width = image.width();
    let height = image.height();
    if width <= max_width && height <= max_height {
        return image;
    }

    let scale_w = max_width as f32 / width as f32;
    let scale_h = max_height as f32 / height as f32;
    let scale = scale_w.min(scale_h).max(0.01);
    let target_w = (width as f32 * scale).round().max(1.0) as u32;
    let target_h = (height as f32 * scale).round().max(1.0) as u32;

    resize(&image, target_w, target_h, FilterType::Triangle)
}

pub(crate) fn draw_border(image: &mut RgbaImage, color: Rgba<u8>, border_width: u32) {
    let width = image.width();
    let height = image.height();
    if width == 0 || height == 0 || border_width == 0 {
        return;
    }
    let border_width = border_width.min(width).min(height);

    for y in 0..border_width {
        let top = y;
        let bottom = height - 1 - y;
        for x in 0..width {
            image.put_pixel(x, top, color);
            image.put_pixel(x, bottom, color);
        }
    }

    for x in 0..border_width {
        let left = x;
        let right = width - 1 - x;
        for y in 0..height {
            image.put_pixel(left, y, color);
            image.put_pixel(right, y, color);
        }
    }
}

pub(crate) fn resolve_generative_path(
    project_root: &Path,
    folder: &Path,
    active_version: Option<&str>,
    extensions: &[&str],
) -> Option<std::path::PathBuf> {
    let folder_path = project_root.join(folder);

    if let Some(version) = active_version {
        for ext in extensions {
            let candidate = folder_path.join(format!("{}.{}", version, ext));
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }

    let entries = std::fs::read_dir(&folder_path).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() {
            if let Some(ext) = path.extension().and_then(|ext| ext.to_str()) {
                if extensions
                    .iter()
                    .any(|allowed| allowed.eq_ignore_ascii_case(ext))
                {
                    return Some(path);
                }
            }
        }
    }

    None
}

pub(crate) fn resolve_asset_source(
    project_root: &Path,
    asset: &Asset,
    image_extensions: &[&str],
    video_extensions: &[&str],
) -> Option<(std::path::PathBuf, bool, Option<f64>)> {
    match &asset.kind {
        AssetKind::Image { path } => Some((project_root.join(path), false, asset.duration_seconds)),
        AssetKind::Video { path } => Some((project_root.join(path), true, asset.duration_seconds)),
        AssetKind::GenerativeImage {
            folder,
            active_version,
            ..
        } => {
            let path = resolve_generative_path(
                project_root,
                folder,
                active_version.as_deref(),
                image_extensions,
            )?;
            Some((path, false, asset.duration_seconds))
        }
        AssetKind::GenerativeVideo {
            folder,
            active_version,
            ..
        } => {
            let path = resolve_generative_path(
                project_root,
                folder,
                active_version.as_deref(),
                video_extensions,
            )?;
            Some((path, true, asset.duration_seconds))
        }
        _ => None,
    }
}
