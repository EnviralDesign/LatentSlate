#![allow(dead_code)]
// Project thumbnail generation is a preserved background service from the
// pre-egui app. The current egui timeline can display cached thumbnails, but
// the async generation service is not fully reconnected yet.

use crate::state::Asset;
use image::imageops::FilterType;
use image::{DynamicImage, GenericImageView, ImageFormat};
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use tokio::sync::Semaphore;
use uuid::Uuid;

const THUMBNAIL_INTERVAL_SECONDS: f64 = 1.0;
const THUMBNAIL_HEIGHT: u32 = 120;

/// Manages the generation of thumbnails for assets
#[derive(Debug)]
pub struct Thumbnailer {
    // Semaphore to limit the number of concurrent ffmpeg processes
    semaphore: Arc<Semaphore>,
    cache_root: PathBuf,
    project_root: PathBuf,
}

impl PartialEq for Thumbnailer {
    fn eq(&self, other: &Self) -> bool {
        self.cache_root == other.cache_root
    }
}

impl Thumbnailer {
    pub fn new(project_root: PathBuf) -> Self {
        let cache_root = project_root.join(".cache").join("thumbnails");
        // Ensure cache directory exists
        if !cache_root.exists() {
            let _ = std::fs::create_dir_all(&cache_root);
        }

        Self {
            // Limit to 2 concurrent thumbnail tasks to avoid choking the CPU
            semaphore: Arc::new(Semaphore::new(2)),
            cache_root,
            project_root,
        }
    }

    /// Queues a thumbnail generation task for an asset
    /// Returns the path to the thumbnail directory for this asset
    /// If force is true, existing thumbnails will be overwritten (directory cleared)
    pub async fn generate(&self, asset: &Asset, force: bool) -> Option<PathBuf> {
        // Visual assets only
        if !asset.is_visual() {
            return None;
        }

        let (absolute_source_path, source_kind) = match &asset.kind {
            crate::state::AssetKind::Video { path } => {
                (self.project_root.join(path), SourceKind::Video)
            }
            crate::state::AssetKind::Image { path } => {
                (self.project_root.join(path), SourceKind::Still)
            }
            crate::state::AssetKind::GenerativeImage {
                folder,
                active_version,
                ..
            } => {
                let path = resolve_generative_source(
                    &self.project_root,
                    folder,
                    active_version.as_deref(),
                    &["png", "jpg", "jpeg", "webp"],
                );
                let Some(path) = path else {
                    if force {
                        self.clear_cache_for_asset(asset.id);
                    }
                    return None;
                };
                (path, SourceKind::Still)
            }
            crate::state::AssetKind::GenerativeVideo {
                folder,
                active_version,
                ..
            } => {
                let path = resolve_generative_source(
                    &self.project_root,
                    folder,
                    active_version.as_deref(),
                    &["mp4", "mov", "mkv", "webm"],
                );
                let Some(path) = path else {
                    if force {
                        self.clear_cache_for_asset(asset.id);
                    }
                    return None;
                };
                (path, SourceKind::Video)
            }
            _ => return None,
        };

        self.generate_from_source(asset, &absolute_source_path, force, source_kind)
            .await
    }

    /// Get the path to the thumbnail for a specific time
    /// Returns None if not generated yet
    pub fn get_thumbnail_path(&self, asset_id: uuid::Uuid, time_seconds: f64) -> Option<PathBuf> {
        let dir = self.cache_root.join(asset_id.to_string());
        if !dir.exists() {
            return None;
        }

        // Map time to index (fps=1/interval)
        // thumb_0001.jpg covers 0-interval
        // thumb_0002.jpg covers interval-2*interval
        let index = (time_seconds / THUMBNAIL_INTERVAL_SECONDS).floor() as u32 + 1;

        let path = dir.join(format!("thumb_{:04}.jpg", index));
        if path.exists() {
            Some(path)
        } else {
            // Fallback to first frame if out of bounds (or handle empty)
            let fallback = dir.join("thumb_0001.jpg");
            if fallback.exists() {
                Some(fallback)
            } else {
                None
            }
        }
    }

    pub fn clear_cache_for_asset(&self, asset_id: Uuid) {
        let dir = self.cache_root.join(asset_id.to_string());
        if dir.exists() {
            if let Err(err) = std::fs::remove_dir_all(&dir) {
                println!("Failed to clear thumbnails for {}: {}", asset_id, err);
            }
        }
    }
}

impl Thumbnailer {
    async fn generate_from_source(
        &self,
        asset: &Asset,
        absolute_source_path: &PathBuf,
        force: bool,
        source_kind: SourceKind,
    ) -> Option<PathBuf> {
        let asset_id = asset.id.to_string();
        let output_dir = self.cache_root.join(&asset_id);

        if !force
            && output_dir.exists()
            && output_dir
                .read_dir()
                .map(|mut i| i.next().is_some())
                .unwrap_or(false)
        {
            return Some(output_dir);
        }

        let Ok(_permit) = self.semaphore.acquire().await else {
            return None;
        };

        if output_dir.exists() {
            let _ = std::fs::remove_dir_all(&output_dir);
        }
        let _ = std::fs::create_dir_all(&output_dir);

        let source = absolute_source_path.clone();
        let out = output_dir.clone();
        let _ = tokio::task::spawn_blocking(move || {
            if !source.exists() {
                println!("Thumbnailer Warning: Source file not found: {:?}", source);
                return;
            }

            match source_kind {
                SourceKind::Video => {
                    let output_pattern = out.join("thumb_%04d.jpg");
                    let status = Command::new("ffmpeg")
                        .arg("-i")
                        .arg(&source)
                        .arg("-vf")
                        .arg(format!(
                            "fps=1/{},scale=-1:{}",
                            THUMBNAIL_INTERVAL_SECONDS, THUMBNAIL_HEIGHT
                        ))
                        .arg("-q:v")
                        .arg("5")
                        .arg(output_pattern)
                        .status();

                    match status {
                        Ok(s) if s.success() => println!("Generated thumbnails for {}", asset_id),
                        _ => println!(
                            "Failed to generate thumbnails for {}. Valid path? {:?} Status: {:?}",
                            asset_id, source, status
                        ),
                    }
                }
                SourceKind::Still => {
                    if let Err(err) = generate_still_thumbnail(&source, &out) {
                        println!(
                            "Failed to generate image thumbnail for {}: {}",
                            asset_id, err
                        );
                    }
                }
            }
        })
        .await;

        Some(output_dir)
    }
}

#[derive(Clone, Copy)]
enum SourceKind {
    Video,
    Still,
}

fn generate_still_thumbnail(source: &PathBuf, out_dir: &PathBuf) -> Result<(), String> {
    let image = image::open(source).map_err(|err| err.to_string())?;
    let resized = resize_to_height(image, THUMBNAIL_HEIGHT);
    let output_path = out_dir.join("thumb_0001.jpg");
    resized
        .save_with_format(output_path, ImageFormat::Jpeg)
        .map_err(|err| err.to_string())
}

fn resize_to_height(image: DynamicImage, height: u32) -> DynamicImage {
    let (width, current_height) = image.dimensions();
    if current_height <= height {
        return image;
    }
    let scale = height as f32 / current_height as f32;
    let target_w = ((width as f32) * scale).round().max(1.0) as u32;
    image.resize_exact(target_w, height, FilterType::Triangle)
}

fn resolve_generative_source(
    project_root: &PathBuf,
    folder: &PathBuf,
    active_version: Option<&str>,
    extensions: &[&str],
) -> Option<PathBuf> {
    let active_version = active_version?;
    let folder_path = project_root.join(folder);

    for ext in extensions {
        let candidate = folder_path.join(format!("{}.{}", active_version, ext));
        if candidate.exists() {
            return Some(candidate);
        }
    }
    None
}
