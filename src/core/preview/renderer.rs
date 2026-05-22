use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use image::{Rgba, RgbaImage};

use crate::core::media::probe_duration_seconds;
use crate::core::preview_store;
use crate::core::video_decode::{DecodeMode, VideoDecodeWorker};
use crate::state::{Asset, AssetKind, Project, TrackType};

use super::{
    cache::FrameCache,
    layers::{
        composite_layer, compute_layer_placement, preview_canvas_size, DecodedFrame, PendingDecode,
        PreviewLayer,
    },
    types::{
        FrameKey, PlateCache, PreviewDecodeMode, PreviewFrameInfo, PreviewLayerGpu,
        PreviewLayerPlacement, PreviewLayerStack, PreviewRgbaFrame, PreviewStats, RenderOutput,
        RenderRgbaOutput, MAX_CACHE_BUCKETS, PLATE_BORDER_COLOR, PLATE_BORDER_WIDTH,
    },
    utils::{
        clamp_time, draw_border, elapsed_ms, frame_index_to_time, resolve_asset_source,
        scale_image_to_fit, time_to_frame_index, track_lane_id,
    },
};

/// Generates composited preview frames for the current timeline time.
pub struct PreviewRenderer {
    project_root: PathBuf,
    max_width: u32,
    max_height: u32,
    video_decoder: VideoDecodeWorker,
    frame_cache: Mutex<FrameCache>,
    duration_cache: Mutex<HashMap<PathBuf, Option<f64>>>,
    #[allow(dead_code)]
    // Plate textures belong to the preserved GPU-layer preview path. The
    // current egui compositor draws the plate in UI space.
    plate_cache: Mutex<Option<PlateCache>>,
}

impl PreviewRenderer {
    /// Create a new preview renderer with explicit preview bounds.
    pub fn new_with_limits(
        project_root: PathBuf,
        max_cache_bytes: usize,
        max_width: u32,
        max_height: u32,
    ) -> Self {
        let max_width = max_width.max(1);
        let max_height = max_height.max(1);
        Self {
            project_root,
            max_width,
            max_height,
            video_decoder: VideoDecodeWorker::new(max_width, max_height),
            frame_cache: Mutex::new(FrameCache::new(max_cache_bytes)),
            duration_cache: Mutex::new(HashMap::new()),
            plate_cache: Mutex::new(None),
        }
    }

    #[allow(dead_code)]
    // Reconnected when provider output folders can invalidate preview caches.
    pub fn invalidate_folder(&self, folder: &Path) {
        if let Ok(mut cache) = self.frame_cache.lock() {
            cache.invalidate_folder(folder);
        }
    }

    fn cached_video_duration(&self, path: &Path) -> Option<f64> {
        let mut cache = self.duration_cache.lock().ok()?;
        if let Some(duration) = cache.get(path) {
            return *duration;
        }
        let duration = probe_duration_seconds(path);
        cache.insert(path.to_path_buf(), duration);
        duration
    }

    fn mapped_source_time(
        &self,
        asset: &Asset,
        path: &Path,
        source_time: f64,
        declared_duration: Option<f64>,
    ) -> (f64, Option<f64>) {
        let AssetKind::GenerativeVideo { .. } = asset.kind else {
            return (source_time, declared_duration);
        };

        let declared = declared_duration.filter(|value| *value > 0.0);
        let actual = self
            .cached_video_duration(path)
            .filter(|value| *value > 0.0);
        if let (Some(declared), Some(actual)) = (declared, actual) {
            let ratio = actual / declared;
            return (source_time * ratio, Some(actual));
        }

        (source_time, declared_duration)
    }

    /// Render a preview frame for the given time as raw RGBA bytes.
    pub fn render_frame_rgba(
        &self,
        project: &Project,
        time_seconds: f64,
        decode_mode: PreviewDecodeMode,
        allow_hw_decode: bool,
    ) -> RenderRgbaOutput {
        let render_start = Instant::now();
        let mut stats = PreviewStats::default();
        let project_root = project.project_path.as_ref().unwrap_or(&self.project_root);

        let (canvas_w, canvas_h, preview_scale) = preview_canvas_size(
            project.settings.width,
            project.settings.height,
            self.max_width,
            self.max_height,
        );

        let fps = project.settings.fps.max(1.0);
        let collect_start = Instant::now();
        let layers = self.collect_layers(
            project,
            project_root,
            time_seconds,
            fps,
            decode_mode,
            allow_hw_decode,
            &mut stats,
        );
        stats.collect_ms = elapsed_ms(collect_start);
        stats.layers = layers.len();

        let has_visual_assets = project.clips.iter().any(|clip| {
            project
                .find_asset(clip.asset_id)
                .map(|asset| asset.is_visual())
                .unwrap_or(false)
        });

        if layers.is_empty() && !has_visual_assets {
            stats.total_ms = elapsed_ms(render_start);
            return RenderRgbaOutput { frame: None, stats };
        }

        let mut canvas = RgbaImage::from_pixel(canvas_w, canvas_h, Rgba([0, 0, 0, 255]));

        let composite_start = Instant::now();
        for layer in layers {
            composite_layer(
                &mut canvas,
                &layer.image,
                layer.source_width,
                layer.source_height,
                layer.transform,
                preview_scale,
            );
        }
        stats.composite_ms = elapsed_ms(composite_start);

        let encode_start = Instant::now();
        let bytes = canvas.into_raw();
        stats.encode_ms = elapsed_ms(encode_start);
        stats.total_ms = elapsed_ms(render_start);

        RenderRgbaOutput {
            frame: Some(PreviewRgbaFrame {
                width: canvas_w,
                height: canvas_h,
                bytes,
            }),
            stats,
        }
    }

    /// Render a preview frame for the given time and store it in the live preview store.
    pub fn render_frame(
        &self,
        project: &Project,
        time_seconds: f64,
        decode_mode: PreviewDecodeMode,
        allow_hw_decode: bool,
    ) -> RenderOutput {
        let output = self.render_frame_rgba(project, time_seconds, decode_mode, allow_hw_decode);
        let frame = output.frame.and_then(|rgba| {
            preview_store::store_preview_frame(rgba.width, rgba.height, rgba.bytes).map(|version| {
                PreviewFrameInfo {
                    version,
                    width: rgba.width,
                    height: rgba.height,
                }
            })
        });
        RenderOutput {
            frame,
            layers: None,
            stats: output.stats,
        }
    }

    /// Render the per-layer stack for GPU compositing.
    #[allow(dead_code)]
    // Preserved for the future GPU compositor; the egui path currently consumes
    // encoded preview frames.
    pub fn render_layers(
        &self,
        project: &Project,
        time_seconds: f64,
        decode_mode: PreviewDecodeMode,
        allow_hw_decode: bool,
    ) -> RenderOutput {
        let render_start = Instant::now();
        let mut stats = PreviewStats::default();
        let project_root = project.project_path.as_ref().unwrap_or(&self.project_root);

        let (canvas_w, canvas_h, preview_scale) = preview_canvas_size(
            project.settings.width,
            project.settings.height,
            self.max_width,
            self.max_height,
        );

        let fps = project.settings.fps.max(1.0);
        let collect_start = Instant::now();
        let layers = self.collect_layers(
            project,
            project_root,
            time_seconds,
            fps,
            decode_mode,
            allow_hw_decode,
            &mut stats,
        );
        stats.collect_ms = elapsed_ms(collect_start);
        stats.layers = layers.len();

        let has_visual_assets = project.clips.iter().any(|clip| {
            project
                .find_asset(clip.asset_id)
                .map(|asset| asset.is_visual())
                .unwrap_or(false)
        });

        if layers.is_empty() && !has_visual_assets {
            stats.total_ms = elapsed_ms(render_start);
            return RenderOutput {
                frame: None,
                layers: None,
                stats,
            };
        }

        let mut gpu_layers = Vec::new();
        // Add the black fill plate as the first layer (canvas background)
        if let Some((plate_fill, _border)) = self.plate_images(canvas_w, canvas_h) {
            let placement = PreviewLayerPlacement {
                offset_x: 0.0,
                offset_y: 0.0,
                scaled_w: canvas_w as f32,
                scaled_h: canvas_h as f32,
                opacity: 1.0,
                rotation_deg: 0.0,
            };
            gpu_layers.push(PreviewLayerGpu {
                image: plate_fill,
                placement,
            });
            // NOTE: The UI draws the preview border in screen space, not as a texture layer.
            // This ensures the border is always exactly 1 pixel wide regardless of canvas scale.
        }
        let canvas_w_f = canvas_w as f32;
        let canvas_h_f = canvas_h as f32;
        for layer in layers {
            if let Some(placement) = compute_layer_placement(
                &layer.image,
                layer.source_width,
                layer.source_height,
                layer.transform,
                preview_scale,
                canvas_w_f,
                canvas_h_f,
            ) {
                gpu_layers.push(PreviewLayerGpu {
                    image: layer.image,
                    placement,
                });
            }
        }

        stats.total_ms = elapsed_ms(render_start);
        RenderOutput {
            frame: None,
            layers: Some(PreviewLayerStack {
                canvas_width: canvas_w,
                canvas_height: canvas_h,
                layers: gpu_layers,
            }),
            stats,
        }
    }

    fn collect_layers(
        &self,
        project: &Project,
        project_root: &Path,
        time_seconds: f64,
        fps: f64,
        decode_mode: PreviewDecodeMode,
        allow_hw_decode: bool,
        stats: &mut PreviewStats,
    ) -> Vec<PreviewLayer> {
        let mut track_order: HashMap<uuid::Uuid, usize> = HashMap::new();
        let mut video_tracks = 0;
        for track in project.tracks.iter() {
            if track.track_type == TrackType::Video {
                track_order.insert(track.id, video_tracks);
                video_tracks += 1;
            }
        }

        let decode_mode = match decode_mode {
            PreviewDecodeMode::Seek => DecodeMode::Seek,
            PreviewDecodeMode::Sequential => DecodeMode::Sequential,
        };

        let mut layers = Vec::new();
        let mut pending = Vec::new();
        for clip in project.clips.iter() {
            let track_index = match track_order.get(&clip.track_id) {
                Some(index) => *index,
                None => continue,
            };

            if time_seconds < clip.start_time || time_seconds >= clip.end_time() {
                continue;
            }

            let asset = match project.find_asset(clip.asset_id) {
                Some(asset) if asset.is_visual() => asset,
                _ => continue,
            };

            let source_time = (time_seconds - clip.start_time + clip.trim_in_seconds).max(0.0);
            let Some((path, is_video, duration)) = resolve_asset_source(
                project_root,
                asset,
                &["png", "jpg", "jpeg", "webp"],
                &["mp4", "mov", "mkv", "webm"],
            ) else {
                continue;
            };

            let (frame_index, frame_time) = if is_video {
                let (mapped_time, clamp_duration) =
                    self.mapped_source_time(asset, &path, source_time, duration);
                let time = clamp_time(mapped_time, clamp_duration);
                let index = time_to_frame_index(time, fps);
                let frame_time = frame_index_to_time(index, fps);
                (index, frame_time)
            } else {
                (0, 0.0)
            };

            let cache_key = FrameKey {
                path: path.clone(),
                frame_index,
            };

            if let Ok(mut cache) = self.frame_cache.lock() {
                if let Some(cached) = cache.get(&cache_key) {
                    stats.cache_hits += 1;
                    layers.push(PreviewLayer {
                        track_index,
                        start_time: clip.start_time,
                        image: cached.image,
                        transform: clip.transform,
                        source_width: cached.source_width,
                        source_height: cached.source_height,
                    });
                    continue;
                }
            }

            stats.cache_misses += 1;

            if !is_video {
                let decode_start = Instant::now();
                let decoded = self.load_still(&path);
                let decode_ms = elapsed_ms(decode_start);
                stats.still_load_ms += decode_ms;
                if let Some(decoded) = decoded {
                    let image = Arc::new(decoded.image);
                    if let Ok(mut cache) = self.frame_cache.lock() {
                        cache.insert(
                            cache_key,
                            Arc::clone(&image),
                            decoded.source_width,
                            decoded.source_height,
                        );
                    }
                    layers.push(PreviewLayer {
                        track_index,
                        start_time: clip.start_time,
                        image,
                        transform: clip.transform,
                        source_width: decoded.source_width,
                        source_height: decoded.source_height,
                    });
                }
                continue;
            }

            pending.push(PendingDecode {
                track_index,
                start_time: clip.start_time,
                path,
                frame_time,
                cache_key,
                transform: clip.transform,
                lane_id: track_lane_id(clip.track_id),
            });
        }

        if !pending.is_empty() {
            let mut requests = Vec::with_capacity(pending.len());
            for item in pending {
                if let Some(receiver) = self.video_decoder.decode_async(
                    &item.path,
                    item.frame_time,
                    decode_mode,
                    item.lane_id,
                    allow_hw_decode,
                ) {
                    requests.push((item, receiver));
                }
            }

            for (item, receiver) in requests {
                if let Ok(response) = receiver.recv() {
                    let timings = response.timings;
                    stats.video_decode_ms += timings.total_ms();
                    stats.video_decode_seek_ms += timings.seek_ms;
                    stats.video_decode_packet_ms += timings.packet_ms;
                    stats.video_decode_transfer_ms += timings.transfer_ms;
                    stats.video_decode_scale_ms += timings.scale_ms;
                    stats.video_decode_copy_ms += timings.copy_ms;
                    if let Some(image) = response.image {
                        let image = Arc::new(image);
                        if let Ok(mut cache) = self.frame_cache.lock() {
                            cache.insert(
                                item.cache_key,
                                Arc::clone(&image),
                                response.source_width,
                                response.source_height,
                            );
                        }
                        if response.used_hw {
                            stats.hw_decode_frames += 1;
                        } else {
                            stats.sw_decode_frames += 1;
                        }
                        layers.push(PreviewLayer {
                            track_index: item.track_index,
                            start_time: item.start_time,
                            image,
                            transform: item.transform,
                            source_width: response.source_width,
                            source_height: response.source_height,
                        });
                    }
                }
            }
        }

        layers.sort_by(|a, b| {
            b.track_index.cmp(&a.track_index).then_with(|| {
                a.start_time
                    .partial_cmp(&b.start_time)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
        });

        layers
    }

    #[allow(dead_code)]
    // Preview prefetch remains staged until playback/scrub scheduling is
    // promoted out of the immediate UI pass.
    pub fn prefetch_frames(
        &self,
        project: &Project,
        time_seconds: f64,
        direction: i32,
        window_frames: u32,
        decode_mode: PreviewDecodeMode,
        allow_hw_decode: bool,
    ) {
        if window_frames == 0 || direction == 0 {
            return;
        }

        let fps = project.settings.fps.max(1.0);
        let project_root = project.project_path.as_ref().unwrap_or(&self.project_root);
        let start_frame = time_to_frame_index(time_seconds, fps);
        let step = direction.signum() as i64;

        for offset in 1..=window_frames {
            let frame_index = start_frame + step * offset as i64;
            if frame_index < 0 {
                break;
            }
            let frame_time = frame_index_to_time(frame_index, fps);
            for clip in project.clips.iter() {
                if frame_time < clip.start_time || frame_time >= clip.end_time() {
                    continue;
                }

                let asset = match project.find_asset(clip.asset_id) {
                    Some(asset) if asset.is_visual() => asset,
                    _ => continue,
                };

                let source_time = (frame_time - clip.start_time + clip.trim_in_seconds).max(0.0);
                let _ = self.load_clip_frame(
                    project_root,
                    asset,
                    source_time,
                    fps,
                    decode_mode,
                    track_lane_id(clip.track_id),
                    allow_hw_decode,
                    None,
                );
            }
        }
    }

    #[allow(dead_code)]
    // Cache bucket visualization is retained for future timeline diagnostics.
    pub fn cached_buckets_for_project(
        &self,
        project: &Project,
        bucket_hint_seconds: f64,
    ) -> HashMap<uuid::Uuid, Vec<bool>> {
        let project_root = project.project_path.as_ref().unwrap_or(&self.project_root);
        let fps = project.settings.fps.max(1.0);
        let min_bucket_seconds = (1.0 / fps).max(0.001);
        let bucket_hint_seconds = bucket_hint_seconds.max(min_bucket_seconds);
        let mut result = HashMap::new();

        let Ok(cache) = self.frame_cache.lock() else {
            return result;
        };

        for clip in project.clips.iter() {
            let Some(asset) = project.find_asset(clip.asset_id) else {
                continue;
            };
            if !asset.is_visual() {
                continue;
            }

            let Some((path, is_video, _duration)) = resolve_asset_source(
                project_root,
                asset,
                &["png", "jpg", "jpeg", "webp"],
                &["mp4", "mov", "mkv", "webm"],
            ) else {
                continue;
            };

            let clip_duration = clip.duration.max(0.0);
            if clip_duration <= 0.0 {
                continue;
            }

            let mut bucket_seconds =
                bucket_hint_seconds.max(clip_duration / MAX_CACHE_BUCKETS as f64);
            bucket_seconds = bucket_seconds.max(min_bucket_seconds);
            let bucket_count = (clip_duration / bucket_seconds).ceil().max(1.0) as usize;

            let mut buckets = vec![false; bucket_count];
            let Some(asset_frames) = cache.asset_index.get(&path) else {
                result.insert(clip.id, buckets);
                continue;
            };

            if !is_video {
                let cached = asset_frames.contains(&0);
                for bucket in buckets.iter_mut() {
                    *bucket = cached;
                }
                result.insert(clip.id, buckets);
                continue;
            }

            let clip_start = clip.trim_in_seconds.max(0.0);
            let clip_end = clip_start + clip_duration;
            for frame_index in asset_frames.iter() {
                let frame_time = frame_index_to_time(*frame_index, fps);
                if frame_time < clip_start || frame_time > clip_end {
                    continue;
                }
                let time_in_clip = (frame_time - clip_start).max(0.0);
                let bucket_index = (time_in_clip / bucket_seconds).floor() as usize;
                if let Some(bucket) = buckets.get_mut(bucket_index) {
                    *bucket = true;
                }
            }

            result.insert(clip.id, buckets);
        }

        result
    }

    #[allow(dead_code)]
    // Used by the staged GPU layer renderer path.
    fn load_clip_frame(
        &self,
        project_root: &Path,
        asset: &Asset,
        time_seconds: f64,
        fps: f64,
        decode_mode: PreviewDecodeMode,
        lane_id: u64,
        allow_hw_decode: bool,
        mut stats: Option<&mut PreviewStats>,
    ) -> Option<Arc<RgbaImage>> {
        let (path, is_video, duration) = resolve_asset_source(
            project_root,
            asset,
            &["png", "jpg", "jpeg", "webp"],
            &["mp4", "mov", "mkv", "webm"],
        )?;

        let (frame_index, frame_time) = if is_video {
            let (mapped_time, clamp_duration) =
                self.mapped_source_time(asset, &path, time_seconds, duration);
            let time = clamp_time(mapped_time, clamp_duration);
            let index = time_to_frame_index(time, fps);
            let frame_time = frame_index_to_time(index, fps);
            (index, frame_time)
        } else {
            (0, 0.0)
        };

        let cache_key = FrameKey {
            path: path.clone(),
            frame_index,
        };

        if let Ok(mut cache) = self.frame_cache.lock() {
            if let Some(cached) = cache.get(&cache_key) {
                if let Some(stats) = stats.as_deref_mut() {
                    stats.cache_hits += 1;
                }
                return Some(cached.image);
            }
        }

        if let Some(stats) = stats.as_deref_mut() {
            stats.cache_misses += 1;
        }

        let decoded = if is_video {
            let mode = match decode_mode {
                PreviewDecodeMode::Seek => DecodeMode::Seek,
                PreviewDecodeMode::Sequential => DecodeMode::Sequential,
            };
            let response = match mode {
                DecodeMode::Seek => {
                    self.video_decoder
                        .decode(&path, frame_time, lane_id, allow_hw_decode)?
                }
                DecodeMode::Sequential => self.video_decoder.decode_sequential(
                    &path,
                    frame_time,
                    lane_id,
                    allow_hw_decode,
                )?,
            };
            if let Some(stats) = stats.as_deref_mut() {
                let timings = response.timings;
                stats.video_decode_ms += timings.total_ms();
                stats.video_decode_seek_ms += timings.seek_ms;
                stats.video_decode_packet_ms += timings.packet_ms;
                stats.video_decode_transfer_ms += timings.transfer_ms;
                stats.video_decode_scale_ms += timings.scale_ms;
                stats.video_decode_copy_ms += timings.copy_ms;
            }
            if let Some(image) = response.image {
                if let Some(stats) = stats.as_deref_mut() {
                    if response.used_hw {
                        stats.hw_decode_frames += 1;
                    } else {
                        stats.sw_decode_frames += 1;
                    }
                }
                DecodedFrame {
                    image,
                    source_width: response.source_width,
                    source_height: response.source_height,
                }
            } else {
                return None;
            }
        } else {
            let decode_start = Instant::now();
            let image = self.load_still(&path)?;
            let decode_ms = elapsed_ms(decode_start);
            if let Some(stats) = stats.as_deref_mut() {
                stats.still_load_ms += decode_ms;
            }
            image
        };

        let image = Arc::new(decoded.image);
        if let Ok(mut cache) = self.frame_cache.lock() {
            cache.insert(
                cache_key,
                Arc::clone(&image),
                decoded.source_width,
                decoded.source_height,
            );
        }

        Some(image)
    }

    fn load_still(&self, path: &Path) -> Option<DecodedFrame> {
        let image = image::open(path).ok()?.into_rgba8();
        let source_width = image.width().max(1);
        let source_height = image.height().max(1);
        let image = scale_image_to_fit(image, self.max_width, self.max_height);
        Some(DecodedFrame {
            image,
            source_width,
            source_height,
        })
    }

    // Video decoding handled by the in-process decoder worker.
}

impl PreviewRenderer {
    #[allow(dead_code)]
    // Used by the staged GPU layer renderer path.
    fn plate_images(&self, width: u32, height: u32) -> Option<(Arc<RgbaImage>, Arc<RgbaImage>)> {
        if width == 0 || height == 0 {
            return None;
        }

        if let Ok(mut cache) = self.plate_cache.lock() {
            if let Some(entry) = cache.as_ref() {
                if entry.width == width && entry.height == height {
                    return Some((Arc::clone(&entry.fill), Arc::clone(&entry.border)));
                }
            }

            let fill = Arc::new(RgbaImage::from_pixel(width, height, Rgba([0, 0, 0, 255])));
            let mut border = RgbaImage::from_pixel(width, height, Rgba([0, 0, 0, 0]));
            draw_border(&mut border, PLATE_BORDER_COLOR, PLATE_BORDER_WIDTH);

            let border = Arc::new(border);
            *cache = Some(PlateCache {
                width,
                height,
                fill: Arc::clone(&fill),
                border: Arc::clone(&border),
            });
            return Some((fill, border));
        }

        None
    }
}
