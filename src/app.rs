//! Root application component
//! 
//! This defines the main App component and the overall layout structure.

use dioxus::desktop::{use_window, use_wry_event_handler};
use dioxus::desktop::tao::event::{Event as TaoEvent, WindowEvent as TaoWindowEvent};
use dioxus::prelude::*;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use crate::core::automation::{AutomationCommand, AutomationResponse};
use crate::core::generation::next_version_label;
use crate::core::audio::decode::{decode_audio_to_f32, AudioDecodeConfig};
use crate::core::audio::cache::{cache_matches_source, load_peak_cache, peak_cache_path};
use crate::core::audio::playback::{AudioPlaybackEngine, PlaybackItem};
use crate::core::audio::waveform::{
    build_and_store_peak_cache, resolve_audio_or_video_source, resolve_audio_source, PeakBuildConfig,
};
use crate::core::media::{resolve_asset_duration_seconds, spawn_asset_duration_probe, spawn_missing_duration_probes};
use crate::core::preview_gpu::{PreviewBounds, PreviewGpuSurface};
use crate::core::provider_store::{
    list_global_provider_files,
    load_global_provider_entries_or_empty,
};
use crate::core::timeline_snap::{
    best_snap_delta_frames,
    frames_from_seconds,
    seconds_from_frames,
    snap_time_to_frame,
    SnapTarget,
    SnapTargetKind,
};
use crate::state::{
    GenerationJob, GenerationJobStatus, ProviderConnection, ProviderEntry, ProviderOutputType,
};
use crate::state::TrackType;
use crate::providers::comfyui;
use crate::timeline::{timeline_zoom_bounds, TimelinePanel};
use crate::hotkeys::{handle_hotkey, HotkeyAction, HotkeyContext, HotkeyResult};
use crate::constants::*;
use crate::components::{
    GenerationQueuePanel, NewProjectModal, PreviewPanel,
    ProviderBuilderModalV2, ProviderJsonEditorModal, ProvidersModalV2,
    SidePanel, StartupModal, StatusBar, StartupModalMode, TitleBar, TrackContextMenu,
};
use crate::components::assets::AssetsPanelContent;
use crate::components::attributes::AttributesPanelContent;


#[derive(Clone, Copy, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum PreviewCanvasMessage {
    Frame { version: u64, width: u32, height: u32 },
    Clear,
}

#[derive(Clone, Copy, Deserialize)]
struct TimelineViewportState {
    width: f64,
    scroll_left: f64,
}

enum GenerationFailure {
    Offline(String),
    Error(String),
}

fn automation_state_json(
    project: &crate::state::Project,
    current_time: f64,
    startup_done: bool,
    selection: &crate::state::SelectionState,
    providers_open: bool,
) -> serde_json::Value {
    serde_json::json!({
        "project": {
            "name": project.name.clone(),
            "path": project.project_path.clone(),
            "settings": project.settings.clone(),
            "tracks": project.tracks.clone(),
            "assets": project.assets.clone(),
            "clips": project.clips.clone(),
            "markers": project.markers.clone(),
        },
        "current_time": current_time,
        "startup_done": startup_done,
        "providers_open": providers_open,
        "selection": {
            "clips": selection.clip_ids.clone(),
            "assets": selection.asset_ids.clone(),
            "tracks": selection.track_ids.clone(),
            "markers": selection.marker_ids.clone(),
        }
    })
}

pub(crate) fn build_audio_playback_items(
    project: &crate::state::Project,
    project_root: &std::path::Path,
    engine: &AudioPlaybackEngine,
    sample_cache: &Arc<Mutex<HashMap<uuid::Uuid, Arc<Vec<f32>>>>>,
    allow_decode: bool,
) -> (Vec<PlaybackItem>, Vec<uuid::Uuid>) {
    let mut track_types = HashMap::new();
    let mut track_volumes = HashMap::new();
    for track in project.tracks.iter() {
        track_types.insert(track.id, track.track_type.clone());
        track_volumes.insert(track.id, track.volume);
    }

    let sample_rate = engine.sample_rate() as f64;
    let channels = engine.channels();
    let mut items = Vec::new();
    let mut missing = Vec::new();

    for clip in project.clips.iter() {
        let Some(track_type) = track_types.get(&clip.track_id) else {
            continue;
        };
        if *track_type != TrackType::Audio && *track_type != TrackType::Video {
            continue;
        }
        let Some(asset) = project.find_asset(clip.asset_id) else {
            continue;
        };
        if !asset.is_audio() && !asset.is_video() {
            continue;
        }
        let Some(source_path) = resolve_audio_or_video_source(project_root, asset) else {
            continue;
        };

        let cached = sample_cache
            .lock()
            .ok()
            .and_then(|cache| cache.get(&asset.id).cloned());
        let samples = if let Some(samples) = cached {
            samples
        } else if !allow_decode {
            missing.push(asset.id);
            continue;
        } else {
            let decode_config = AudioDecodeConfig {
                target_rate: engine.sample_rate(),
                target_channels: engine.channels(),
            };
            let decoded = match decode_audio_to_f32(&source_path, decode_config) {
                Ok(decoded) => decoded,
                Err(err) => {
                    eprintln!(
                        "[AUDIO ERROR] Playback decode failed asset_id={} err={}",
                        asset.id, err
                    );
                    continue;
                }
            };
            let samples = Arc::new(decoded.samples);
            if let Ok(mut cache) = sample_cache.lock() {
                cache.insert(asset.id, Arc::clone(&samples));
            }
            samples
        };

        let total_frames = (samples.len() / channels.max(1) as usize) as u64;
        let trim_frames = (clip.trim_in_seconds.max(0.0) * sample_rate).round() as u64;
        if trim_frames >= total_frames {
            continue;
        }
        let clip_frames = (clip.duration.max(0.0) * sample_rate).round() as u64;
        let available_frames = total_frames.saturating_sub(trim_frames);
        let frame_count = clip_frames.min(available_frames);
        if frame_count == 0 {
            continue;
        }
        let start_frame = (clip.start_time.max(0.0) * sample_rate).round() as u64;
        let track_volume = track_volumes.get(&clip.track_id).copied().unwrap_or(1.0);
        let clip_volume = clip.volume;
        let gain = (track_volume * clip_volume).max(0.0);

        items.push(PlaybackItem {
            samples,
            start_frame,
            sample_offset_frames: trim_frames,
            frame_count,
            channels,
            gain,
        });
    }

    (items, missing)
}

fn audio_decode_targets_for_project(
    project: &crate::state::Project,
    project_root: &std::path::Path,
) -> Vec<(uuid::Uuid, std::path::PathBuf)> {
    let mut track_types = HashMap::new();
    for track in project.tracks.iter() {
        track_types.insert(track.id, track.track_type.clone());
    }

    let mut seen = HashSet::new();
    let mut targets = Vec::new();
    for clip in project.clips.iter() {
        let Some(track_type) = track_types.get(&clip.track_id) else {
            continue;
        };
        if *track_type != TrackType::Audio && *track_type != TrackType::Video {
            continue;
        }
        let Some(asset) = project.find_asset(clip.asset_id) else {
            continue;
        };
        if !asset.is_audio() && !asset.is_video() {
            continue;
        }
        if !seen.insert(asset.id) {
            continue;
        }
        if let Some(source_path) = resolve_audio_or_video_source(project_root, asset) {
            targets.push((asset.id, source_path));
        }
    }
    targets
}

fn schedule_audio_decode_targets(
    targets: Vec<(uuid::Uuid, std::path::PathBuf)>,
    decode_config: AudioDecodeConfig,
    sample_cache: Arc<Mutex<HashMap<uuid::Uuid, Arc<Vec<f32>>>>>,
    in_flight: Arc<Mutex<HashSet<uuid::Uuid>>>,
    project_snapshot: crate::state::Project,
    project_root: std::path::PathBuf,
    audio_engine: Arc<AudioPlaybackEngine>,
) {
    for (asset_id, source_path) in targets {
        let cache_hit = sample_cache
            .lock()
            .ok()
            .map(|cache| cache.contains_key(&asset_id))
            .unwrap_or(false);
        if cache_hit {
            continue;
        }
        let mut inflight_guard = match in_flight.lock() {
            Ok(guard) => guard,
            Err(_) => continue,
        };
        if inflight_guard.contains(&asset_id) {
            continue;
        }
        inflight_guard.insert(asset_id);
        drop(inflight_guard);

        let sample_cache = Arc::clone(&sample_cache);
        let in_flight = Arc::clone(&in_flight);
        let decode_config = decode_config;
        let project_snapshot = project_snapshot.clone();
        let project_root = project_root.clone();
        let audio_engine = audio_engine.clone();
        spawn(async move {
            let result = tokio::task::spawn_blocking(move || {
                decode_audio_to_f32(&source_path, decode_config)
            })
            .await
            .ok()
            .and_then(|res| res.ok());

            if let Some(decoded) = result {
                let samples = Arc::new(decoded.samples);
                if let Ok(mut cache) = sample_cache.lock() {
                    cache.insert(asset_id, Arc::clone(&samples));
                }
                // Cache ready; no logging to reduce console noise.
                let (items, _) = build_audio_playback_items(
                    &project_snapshot,
                    &project_root,
                    &audio_engine,
                    &sample_cache,
                    false,
                );
                audio_engine.set_items(items);
            } else {
                eprintln!(
                    "[AUDIO WARN] Playback decode failed (background) asset_id={}",
                    asset_id
                );
            }

            if let Ok(mut inflight) = in_flight.lock() {
                inflight.remove(&asset_id);
            }
        });
    }
}

fn toggle_playback(
    audio_engine: &Option<Arc<AudioPlaybackEngine>>,
    audio_sample_cache: &Arc<Mutex<HashMap<uuid::Uuid, Arc<Vec<f32>>>>>,
    audio_decode_in_flight: &Arc<Mutex<HashSet<uuid::Uuid>>>,
    project: Signal<crate::state::Project>,
    current_time: Signal<f64>,
    mut is_playing: Signal<bool>,
) {
    let next_playing = !is_playing();
    if let Some(engine) = audio_engine.as_ref() {
        let engine = Arc::clone(engine);
        if next_playing {
            if let Some(project_root) = project.read().project_path.clone() {
                let project_snapshot = project.read().clone();
                let (items, missing) = build_audio_playback_items(
                    &project_snapshot,
                    &project_root,
                    &engine,
                    audio_sample_cache,
                    false,
                );
                engine.set_items(items);
                if !missing.is_empty() {
                    let mut missing_set = HashSet::<uuid::Uuid>::new();
                    for id in missing {
                        missing_set.insert(id);
                    }
                    let mut targets =
                        audio_decode_targets_for_project(&project_snapshot, &project_root);
                    targets.retain(|(id, _)| missing_set.contains(id));
                    let decode_config = AudioDecodeConfig {
                        target_rate: engine.sample_rate(),
                        target_channels: engine.channels(),
                    };
                    schedule_audio_decode_targets(
                        targets,
                        decode_config,
                        Arc::clone(audio_sample_cache),
                        Arc::clone(audio_decode_in_flight),
                        project_snapshot,
                        project_root,
                        Arc::clone(&engine),
                    );
                }
            }
            engine.seek_seconds(current_time());
            engine.play();
        } else {
            engine.pause();
        }
    }
    is_playing.set(next_playing);
}

fn set_timeline_zoom_anchored(
    new_zoom: f64,
    duration: f64,
    viewport_width: Option<f64>,
    current_time: f64,
    mut zoom: Signal<f64>,
    mut scroll_offset: Signal<f64>,
) {
    let old_zoom = zoom();
    if (new_zoom - old_zoom).abs() < f64::EPSILON {
        return;
    }

    let anchor_x = (current_time * old_zoom) - scroll_offset();
    let mut next_scroll = (current_time * new_zoom) - anchor_x;
    if !next_scroll.is_finite() {
        next_scroll = 0.0;
    }
    if next_scroll < 0.0 {
        next_scroll = 0.0;
    }
    if let Some(width) = viewport_width {
        let max_scroll = (duration * new_zoom - width).max(0.0);
        if next_scroll > max_scroll {
            next_scroll = max_scroll;
        }
    }

    zoom.set(new_zoom);
    scroll_offset.set(next_scroll);
}

async fn execute_generation_job(
    job: GenerationJob,
    mut project: Signal<crate::state::Project>,
    previewer: Signal<std::sync::Arc<crate::core::preview::PreviewRenderer>>,
    mut preview_dirty: Signal<bool>,
    thumbnailer: std::sync::Arc<crate::core::thumbnailer::Thumbnailer>,
    thumbnail_cache_buster: Signal<u64>,
    progress_tx: Option<tokio::sync::mpsc::UnboundedSender<comfyui::ComfyUiProgress>>,
) -> Result<String, GenerationFailure> {
    if job.output_type == ProviderOutputType::Audio {
        return Err(GenerationFailure::Error(
            "Audio outputs are not supported in the queue yet.".to_string(),
        ));
    }

    let folder_path = job.folder_path.clone();
    let config_snapshot = project
        .read()
        .generative_config(job.asset_id)
        .cloned()
        .unwrap_or_default();
    let version = next_version_label(&config_snapshot);

    let output = match job.provider.connection.clone() {
        ProviderConnection::ComfyUi {
            base_url,
            workflow_path,
            manifest_path,
            ..
        } => {
            let workflow_path = comfyui::resolve_workflow_path(workflow_path.as_deref());
            let manifest_path = comfyui::resolve_manifest_path(manifest_path.as_deref());
            if let Err(err) = comfyui::check_health(&base_url).await {
                return Err(GenerationFailure::Offline(err));
            }
            comfyui::generate_output(
                &base_url,
                &workflow_path,
                &job.inputs,
                manifest_path.as_deref(),
                job.output_type,
                progress_tx.clone(),
            )
            .await
            .map_err(|err| GenerationFailure::Error(err))
        }
        _ => Err(GenerationFailure::Error(
            "Provider connection not supported yet.".to_string(),
        )),
    };

    let output = match output {
        Ok(output) => output,
        Err(GenerationFailure::Error(err)) => {
            if let ProviderConnection::ComfyUi { base_url, .. } = job.provider.connection.clone()
            {
                if let Err(health_err) = comfyui::check_health(&base_url).await {
                    return Err(GenerationFailure::Offline(health_err));
                }
            }
            return Err(GenerationFailure::Error(err));
        }
        Err(other) => return Err(other),
    };

    std::fs::create_dir_all(&folder_path)
        .map_err(|err| {
            GenerationFailure::Error(format!("Failed to create output folder: {}", err))
        })?;
    let output_path = folder_path.join(format!("{}.{}", version, output.extension));
    std::fs::write(&output_path, &output.bytes)
        .map_err(|err| GenerationFailure::Error(format!("Failed to save output: {}", err)))?;
    previewer.read().invalidate_folder(&folder_path);

    {
        let mut project_write = project.write();
        project_write.update_generative_config(job.asset_id, |config| {
            config.provider_id = Some(job.provider.id);
            config.active_version = Some(version.clone());
            config.inputs = job.inputs_snapshot.clone();
            config.versions.push(crate::state::GenerationRecord {
                version: version.clone(),
                timestamp: chrono::Utc::now(),
                provider_id: job.provider.id,
                inputs_snapshot: job.inputs_snapshot.clone(),
            });
        });
        project_write
            .save_generative_config(job.asset_id)
            .map_err(|err| GenerationFailure::Error(format!("Failed to save config: {}", err)))?;
    }
    preview_dirty.set(true);

    if let Some(asset) = project.read().find_asset(job.asset_id).cloned() {
        let thumbs = thumbnailer.clone();
        let mut thumbnail_cache_buster = thumbnail_cache_buster.clone();
        spawn(async move {
            thumbs.generate(&asset, true).await;
            thumbnail_cache_buster.set(thumbnail_cache_buster() + 1);
        });
    }

    Ok(version)
}

/// Main application component
#[component]
pub fn App() -> Element {
    // Project state - the core data model
    let mut project = use_signal(|| crate::state::Project::default());
    let mut provider_entries = use_signal(|| Vec::<ProviderEntry>::new());
    let default_settings = crate::state::ProjectSettings::default();
    let default_preview_width = default_settings.preview_max_width;
    let default_preview_height = default_settings.preview_max_height;
    let default_cache_root = crate::core::paths::app_cache_root().join("scratch");
    let default_cache_root_for_thumbs = default_cache_root.clone();
    let default_cache_root_for_preview = default_cache_root.clone();
    let audio_engine = use_hook(|| {
        match AudioPlaybackEngine::new() {
            Ok(engine) => Some(Arc::new(engine)),
            Err(err) => {
                eprintln!("[AUDIO ERROR] Audio engine init failed: {}", err);
                None
            }
        }
    });
    let audio_sample_cache = use_hook(|| {
        Arc::new(Mutex::new(HashMap::<uuid::Uuid, Arc<Vec<f32>>>::new()))
    });
    let audio_decode_in_flight = use_hook(|| {
        Arc::new(Mutex::new(HashSet::<uuid::Uuid>::new()))
    });
    
    // Core services
    let mut thumbnailer = use_signal(move || {
        std::sync::Arc::new(crate::core::thumbnailer::Thumbnailer::new(
            default_cache_root_for_thumbs,
        ))
    });
    let thumbnail_refresh_tick = use_signal(|| 0_u64);
    let thumbnail_cache_buster = use_signal(|| 0_u64);
    let mut audio_waveform_cache_buster = use_signal(|| 0_u64);
    let mut previewer = use_signal(move || {
        std::sync::Arc::new(crate::core::preview::PreviewRenderer::new_with_limits(
            default_cache_root_for_preview,
            PREVIEW_CACHE_BUDGET_BYTES,
            default_preview_width,
            default_preview_height,
        ))
    });
    let preview_frame = use_signal(|| None::<crate::core::preview::PreviewFrameInfo>);
    let preview_stats = use_signal(|| None::<crate::core::preview::PreviewStats>);
    let mut preview_eval = use_signal(|| None::<document::Eval>);
    let mut preview_host_eval = use_signal(|| None::<document::Eval>);
    let preview_native_bounds = use_signal(|| None::<PreviewBounds>);
    let mut preview_native_active = use_signal(|| false);
    let mut preview_native_enabled = use_signal(|| false);
    let preview_native_attempted = use_signal(|| false);
    let mut preview_native_uploaded = use_signal(|| None::<u64>);
    let mut preview_gpu_upload_ms = use_signal(|| None::<f64>);
    let mut preview_layers =
        use_signal(|| None::<(u64, crate::core::preview::PreviewLayerStack)>);
    let mut preview_native_ready = use_signal(|| false);
    let mut preview_native_suspended = use_signal(|| false);
    let preview_gpu = use_hook(|| Rc::new(RefCell::new(None::<PreviewGpuSurface>)));
    let mut show_preview_stats = use_signal(|| false);
    let mut use_hw_decode = use_signal(|| true);
    let timeline_viewport_width = use_signal(|| None::<f64>);
    let mut timeline_viewport_eval = use_signal(|| None::<document::Eval>);
    let mut timeline_zoom_initialized = use_signal(|| false);
    let mut last_project_path = use_signal(|| None::<std::path::PathBuf>);
    let mut clip_cache_buckets =
        use_signal(|| std::sync::Arc::new(HashMap::<uuid::Uuid, Vec<bool>>::new()));
    let preview_cache_tick = use_signal(|| 0_u64);
    let desktop = use_window();
    let desktop_for_bounds = desktop.clone();
    let desktop_for_events = desktop.clone();
    let desktop_for_redraw = desktop.clone();
    let mut preview_dirty = use_signal(|| true);
    let generation_queue = use_signal(|| Vec::<GenerationJob>::new());
    let generation_active = use_signal(|| None::<uuid::Uuid>);
    let generation_tick = use_signal(|| 0_u64);
    let generation_retry_tick = use_signal(|| 0_u64);
    let generation_paused = use_signal(|| false);
    let generation_pause_reason = use_signal(|| None::<String>);
    let mut queue_open = use_signal(|| false);
    let gen_video_modal_open = use_signal(|| false);

    // Startup Modal state - check if we have a valid project path on load
    // For MVP, we start with a dummy project, so we check if project_path is None
    let mut startup_done = use_signal(|| false);
    
    // Panel state
    let mut left_width = use_signal(|| PANEL_DEFAULT_WIDTH);
    let mut left_collapsed = use_signal(|| false);
    let mut right_width = use_signal(|| PANEL_DEFAULT_WIDTH);
    let mut right_collapsed = use_signal(|| false);
    let mut timeline_height = use_signal(|| TIMELINE_DEFAULT_HEIGHT);
    let mut timeline_collapsed = use_signal(|| false);
    
    // Timeline playback state
    let mut current_time = use_signal(|| 0.0_f64);        // Current time in seconds
    let mut zoom = use_signal(|| 100.0_f64);              // Pixels per second
    let mut is_playing = use_signal(|| false);            // Playback state
    let mut scroll_offset = use_signal(|| 0.0_f64);       // Horizontal scroll position
    let mut scrub_was_playing = use_signal(|| false);
    let mut is_scrubbing = use_signal(|| false);
    let mut timeline_focused = use_signal(|| false);
    
    // Derive duration/snap targets from project
    let (duration, timeline_fps, timeline_snap_targets) = {
        let project_read = project.read();
        let duration = project_read.duration();
        let fps = project_read.settings.fps.max(1.0);
        let zoom_value = zoom();
        let visible_range = timeline_viewport_width().and_then(|width| {
            if zoom_value <= 0.0 {
                return None;
            }
            let pad = TIMELINE_SNAP_THRESHOLD_PX / zoom_value;
            let start = (scroll_offset() / zoom_value) - pad;
            let end = ((scroll_offset() + width) / zoom_value) + pad;
            Some((start.max(0.0), end.max(0.0)))
        });
        let in_view = |time: f64| -> bool {
            if let Some((start, end)) = visible_range {
                time >= start && time <= end
            } else {
                true
            }
        };
        let mut targets = Vec::new();

        for clip in project_read.clips.iter() {
            if in_view(clip.start_time) {
                let start_frame = frames_from_seconds(clip.start_time, fps).round();
                targets.push(SnapTarget::clip_edge(start_frame, clip.id));
            }
            let clip_end = clip.end_time();
            if in_view(clip_end) {
                let end_frame = frames_from_seconds(clip_end, fps).round();
                targets.push(SnapTarget::clip_edge(end_frame, clip.id));
            }
        }

        let playhead_time = current_time();
        if in_view(playhead_time) {
            let playhead_frame = frames_from_seconds(playhead_time, fps).round();
            targets.push(SnapTarget::playhead(playhead_frame));
        }

        for marker in project_read.markers.iter() {
            if in_view(marker.time) {
                let marker_frame = frames_from_seconds(marker.time, fps).round();
                targets.push(SnapTarget::marker(marker_frame, marker.id));
            }
        }

        (duration, fps, Arc::new(targets))
    };

    use_effect(move || {
        let current_path = project.read().project_path.clone();
        if current_path != last_project_path() {
            last_project_path.set(current_path);
            timeline_zoom_initialized.set(false);
        }
    });

    use_effect(move || {
        if timeline_zoom_initialized() {
            return;
        }
        let Some(_width) = timeline_viewport_width() else {
            return;
        };
        if project.read().project_path.is_none() {
            return;
        }
        let (min_zoom, _max_zoom) = timeline_zoom_bounds(
            project.read().duration(),
            timeline_viewport_width(),
            project.read().settings.fps,
        );
        zoom.set(min_zoom);
        timeline_zoom_initialized.set(true);
    });
    
    // Drag state
    let mut dragging = use_signal(|| None::<&'static str>);
    let mut drag_start_pos = use_signal(|| 0.0);
    let mut drag_start_size = use_signal(|| 0.0);
    
    // Asset Drag & Drop state
    let mut dragged_asset = use_signal(|| None::<uuid::Uuid>);
    let mut mouse_pos = use_signal(|| (0.0, 0.0));
    let mut selection = use_signal(|| crate::state::SelectionState::default());
    let selection_state = selection.read();
    let attributes_key = selection_state
        .primary_clip()
        .map(|id| format!("clip-{id}"))
        .or_else(|| selection_state.primary_marker().map(|id| format!("marker-{id}")))
        .or_else(|| selection_state.primary_track().map(|id| format!("track-{id}")))
        .unwrap_or_else(|| "none".to_string());
    drop(selection_state);
    
    // Context menu state: (x, y, track_id) - None means no menu shown
    let mut context_menu = use_signal(|| None::<(f64, f64, uuid::Uuid)>);

    use_future(move || {
        let mut thumbnail_refresh_tick = thumbnail_refresh_tick.clone();
        async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(2));
            loop {
                interval.tick().await;
                thumbnail_refresh_tick.set(thumbnail_refresh_tick() + 1);
            }
        }
    });

    use_effect(move || {
        let _queue_snapshot = generation_queue();
        let _retry_tick = generation_retry_tick();
        if generation_paused() {
            return;
        }
        let mut generation_queue = generation_queue.clone();
        let mut generation_active = generation_active.clone();
        if generation_active().is_some() {
            return;
        }

        let now = Utc::now();
        let next_job = {
            let mut queue = generation_queue.write();
            let next_index = queue
                .iter()
                .position(|job| job.status == GenerationJobStatus::Queued);
            match next_index {
                Some(index) => {
                    let job = &mut queue[index];
                    if let Some(next_at) = job.next_attempt_at {
                        if next_at > now {
                            None
                        } else {
                            job.status = GenerationJobStatus::Running;
                            job.progress_overall = Some(0.0);
                            job.progress_node = Some(0.0);
                            job.next_attempt_at = None;
                            Some(job.clone())
                        }
                    } else {
                        job.status = GenerationJobStatus::Running;
                        job.progress_overall = Some(0.0);
                        job.progress_node = Some(0.0);
                        job.next_attempt_at = None;
                        Some(job.clone())
                    }
                }
                None => None,
            }
        };

        let Some(job) = next_job else {
            return;
        };

        generation_active.set(Some(job.id));

        let mut generation_queue = generation_queue.clone();
        let mut generation_active = generation_active.clone();
        let mut generation_tick = generation_tick.clone();
        let generation_retry_tick = generation_retry_tick.clone();
        let mut generation_paused = generation_paused.clone();
        let mut generation_pause_reason = generation_pause_reason.clone();
        let project = project.clone();
        let previewer = previewer.clone();
        let preview_dirty = preview_dirty.clone();
        let thumbnailer = thumbnailer.read().clone();
        let thumbnail_cache_buster = thumbnail_cache_buster.clone();
        let (progress_tx, mut progress_rx) =
            tokio::sync::mpsc::unbounded_channel::<comfyui::ComfyUiProgress>();
        let progress_job_id = job.id;
        let mut progress_queue = generation_queue.clone();

        spawn(async move {
            spawn(async move {
                while let Some(progress) = progress_rx.recv().await {
                    let mut queue = progress_queue.write();
                    if let Some(entry) = queue.iter_mut().find(|entry| entry.id == progress_job_id) {
                        if entry.status == GenerationJobStatus::Running {
                            if let Some(overall) = progress.overall {
                                entry.progress_overall = Some(overall.clamp(0.0, 1.0));
                            }
                            if let Some(node) = progress.node {
                                entry.progress_node = Some(node.clamp(0.0, 1.0));
                            }
                        }
                    }
                }
            });

            let result = execute_generation_job(
                job.clone(),
                project,
                previewer,
                preview_dirty,
                thumbnailer,
                thumbnail_cache_buster,
                Some(progress_tx),
            )
            .await;

            let mut queue = generation_queue.write();
            if let Some(entry) = queue.iter_mut().find(|entry| entry.id == job.id) {
                match &result {
                    Ok(version) => {
                        entry.status = GenerationJobStatus::Succeeded;
                        entry.version = Some(version.clone());
                        entry.progress_overall = Some(1.0);
                        entry.progress_node = Some(1.0);
                        entry.error = None;
                        entry.attempts = 0;
                        entry.next_attempt_at = None;
                    }
                    Err(GenerationFailure::Offline(err)) => {
                        if entry.attempts == 0 {
                            entry.attempts = 1;
                            entry.status = GenerationJobStatus::Queued;
                            entry.next_attempt_at = Some(Utc::now() + chrono::Duration::seconds(5));
                            entry.error = Some("Provider offline, retrying in 5s".to_string());
                            let mut generation_retry_tick = generation_retry_tick.clone();
                            spawn(async move {
                                tokio::time::sleep(Duration::from_secs(5)).await;
                                generation_retry_tick.set(generation_retry_tick() + 1);
                            });
                        } else {
                            entry.status = GenerationJobStatus::Queued;
                            entry.next_attempt_at = None;
                            entry.error = Some("Provider offline, queue paused.".to_string());
                            generation_paused.set(true);
                            generation_pause_reason.set(Some(format!(
                                "Provider offline: {}",
                                err
                            )));
                        }
                    }
                    Err(GenerationFailure::Error(err)) => {
                        entry.status = GenerationJobStatus::Failed;
                        entry.error = Some(err.clone());
                        entry.progress_overall = None;
                        entry.progress_node = None;
                    }
                }
            }

            if result.is_ok() {
                generation_tick.set(generation_tick() + 1);
            }

            generation_active.set(None);
        });
    });

    let audio_engine_for_timer = audio_engine.clone();
    use_future(move || {
        let mut current_time = current_time.clone();
        let mut is_playing = is_playing.clone();
        let project = project.clone();
        let audio_engine = audio_engine_for_timer.clone();
        async move {
            let mut last_tick = Instant::now();
            loop {
                tokio::time::sleep(Duration::from_millis(16)).await;
                if !is_playing() {
                    last_tick = Instant::now();
                    continue;
                }

                let duration = project.read().duration();
                let fps = project.read().settings.fps.max(1.0);
                if let Some(engine) = audio_engine.as_ref() {
                    let time = engine.playhead_seconds();
                    let snapped = snap_time_to_frame(time.min(duration), fps);
                    current_time.set(snapped);
                    if time >= duration {
                        engine.pause();
                        is_playing.set(false);
                    }
                    continue;
                }

                let now = Instant::now();
                let delta = now.saturating_duration_since(last_tick);
                last_tick = now;
                let next_time = (current_time() + delta.as_secs_f64()).min(duration);
                let snapped = snap_time_to_frame(next_time, fps);
                current_time.set(snapped);

                if next_time >= duration {
                    is_playing.set(false);
                }
            }
        }
    });

    use_future(move || {
        let project = project.clone();
        let current_time = current_time.clone();
        let is_playing = is_playing.clone();
        let previewer = previewer.clone();
        let mut preview_frame = preview_frame.clone();
        let mut preview_layers = preview_layers.clone();
        let mut preview_stats = preview_stats.clone();
        let mut preview_dirty = preview_dirty.clone();
        let mut preview_cache_tick = preview_cache_tick.clone();
        let preview_native_ready = preview_native_ready.clone();
        let use_hw_decode = use_hw_decode.clone();
        async move {
            let render_request_id = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
            let render_gate = std::sync::Arc::new(tokio::sync::Semaphore::new(1));
            let prefetch_gate = std::sync::Arc::new(tokio::sync::Semaphore::new(1));
            let mut last_time = -1.0_f64;
            let mut last_interaction = Instant::now();
            loop {
                tokio::time::sleep(Duration::from_millis(PREVIEW_FRAME_INTERVAL_MS)).await;

                let time = current_time();
                let dirty = preview_dirty();
                let time_changed = (time - last_time).abs() >= 0.0001;

                if !is_playing() && (time_changed || dirty) {
                    last_interaction = Instant::now();
                }

                if !is_playing()
                    && !dirty
                    && last_interaction.elapsed()
                        >= Duration::from_millis(PREVIEW_IDLE_PREFETCH_DELAY_MS)
                {
                    if let Ok(prefetch_permit) = prefetch_gate.clone().try_acquire_owned() {
                        let project_snapshot = project.read().clone();
                        let renderer = previewer.read().clone();
                        let allow_hw_decode = use_hw_decode();
                        let fps = project_snapshot.settings.fps.max(1.0);
                        let ahead_frames =
                            (fps * PREVIEW_IDLE_PREFETCH_AHEAD_SECONDS).round() as u32;
                        let behind_frames =
                            (fps * PREVIEW_IDLE_PREFETCH_BEHIND_SECONDS).round() as u32;
                        tokio::task::spawn_blocking(move || {
                            if ahead_frames > 0 {
                                renderer.prefetch_frames(
                                    &project_snapshot,
                                    time,
                                    1,
                                    ahead_frames,
                                    crate::core::preview::PreviewDecodeMode::Sequential,
                                    allow_hw_decode,
                                );
                            }
                            if behind_frames > 0 {
                                renderer.prefetch_frames(
                                    &project_snapshot,
                                    time,
                                    -1,
                                    behind_frames,
                                    crate::core::preview::PreviewDecodeMode::Sequential,
                                    allow_hw_decode,
                                );
                            }
                            drop(prefetch_permit);
                        });
                    }
                }

                if !dirty && !time_changed {
                    continue;
                }

                let permit = match render_gate.clone().try_acquire_owned() {
                    Ok(permit) => permit,
                    Err(_) => continue,
                };
                let request_id = render_request_id
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                    + 1;

                let project_snapshot = project.read().clone();
                let renderer = previewer.read().clone();
                let use_gpu = preview_native_ready();
                let decode_mode = if is_playing() {
                    crate::core::preview::PreviewDecodeMode::Sequential
                } else {
                    crate::core::preview::PreviewDecodeMode::Seek
                };
                let allow_hw_decode = use_hw_decode();
                let render_task = tokio::task::spawn_blocking(move || {
                    let result = if use_gpu {
                        renderer.render_layers(&project_snapshot, time, decode_mode, allow_hw_decode)
                    } else {
                        renderer.render_frame(&project_snapshot, time, decode_mode, allow_hw_decode)
                    };
                    drop(permit);
                    (result, project_snapshot, use_gpu, decode_mode, allow_hw_decode)
                })
                .await
                .ok();

                let Some((render_output, project_snapshot, use_gpu, decode_mode, allow_hw_decode)) = render_task else {
                    continue;
                };

                if render_request_id.load(std::sync::atomic::Ordering::Relaxed) != request_id {
                    continue;
                }

                let crate::core::preview::RenderOutput { frame, layers, stats } = render_output;
                preview_stats.set(Some(stats));
                if SHOW_CACHE_TICKS {
                    preview_cache_tick.set(preview_cache_tick() + 1);
                }

                let rendered = if use_gpu {
                    if let Some(layers) = layers {
                        preview_layers.set(Some((request_id, layers)));
                        preview_frame.set(None);
                        true
                    } else {
                        preview_layers.set(None);
                        preview_frame.set(None);
                        false
                    }
                } else {
                    preview_layers.set(None);
                    preview_frame.set(frame);
                    frame.is_some()
                };

                preview_dirty.set(false);
                let direction = if last_time < 0.0 {
                    0
                } else if time > last_time {
                    1
                } else if time < last_time {
                    -1
                } else {
                    0
                };
                last_time = time;

                if rendered && direction != 0 {
                    let fps = project_snapshot.settings.fps.max(1.0);
                    let prefetch_seconds = if is_playing() {
                        PREVIEW_PREFETCH_PLAYBACK_SECONDS
                    } else {
                        PREVIEW_PREFETCH_SCRUB_SECONDS
                    };
                    let prefetch_frames = (fps * prefetch_seconds).round() as u32;
                    if prefetch_frames > 0 {
                        if let Ok(prefetch_permit) = prefetch_gate.clone().try_acquire_owned() {
                            let renderer = previewer.read().clone();
                            tokio::task::spawn_blocking(move || {
                                renderer.prefetch_frames(
                                    &project_snapshot,
                                    time,
                                    direction,
                                    prefetch_frames,
                                    decode_mode,
                                    allow_hw_decode,
                                );
                                drop(prefetch_permit);
                            });
                        }
                    }
                }
            }
        }
    });


    use_effect(move || {
        let _tick = preview_cache_tick();
        if !SHOW_CACHE_TICKS {
            if !clip_cache_buckets().is_empty() {
                clip_cache_buckets.set(std::sync::Arc::new(HashMap::new()));
            }
            return;
        }
        let zoom_value = zoom().max(1.0);
        let project_snapshot = project.read().clone();
        let renderer = previewer.read().clone();
        let fps = project_snapshot.settings.fps.max(1.0);
        let bucket_hint_seconds = (6.0 / zoom_value).max(1.0 / fps);
        let cache_map = renderer.cached_buckets_for_project(&project_snapshot, bucket_hint_seconds);
        clip_cache_buckets.set(std::sync::Arc::new(cache_map));
    });

    use_effect(move || {
        if preview_eval().is_some() {
            return;
        }
        let eval = document::eval(PREVIEW_CANVAS_SCRIPT);
        preview_eval.set(Some(eval));
    });

    use_effect(move || {
        let frame = preview_frame();
        let Some(eval) = preview_eval() else {
            return;
        };
        if preview_native_active() {
            return;
        }
        let _ = match frame {
            Some(frame) => eval.send(PreviewCanvasMessage::Frame {
                version: frame.version,
                width: frame.width,
                height: frame.height,
            }),
            None => eval.send(PreviewCanvasMessage::Clear),
        };
    });

    use_effect(move || {
        if preview_host_eval().is_some() {
            return;
        }
        let eval = document::eval(PREVIEW_NATIVE_HOST_SCRIPT);
        preview_host_eval.set(Some(eval));
    });

    use_effect(move || {
        if timeline_viewport_eval().is_some() {
            return;
        }
        let eval = document::eval(TIMELINE_VIEWPORT_SCRIPT);
        timeline_viewport_eval.set(Some(eval));
    });

    use_future(move || {
        let mut preview_native_bounds = preview_native_bounds.clone();
        let preview_host_eval = preview_host_eval.clone();
        let desktop = desktop_for_bounds.clone();
        async move {
            loop {
                let Some(eval) = preview_host_eval() else {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    continue;
                };
                let mut eval = eval;
                loop {
                    match eval.recv::<PreviewBounds>().await {
                        Ok(bounds) => {
                            if preview_native_bounds() != Some(bounds) {
                                preview_native_bounds.set(Some(bounds));
                                desktop.window.request_redraw();
                            }
                        }
                        Err(_) => break,
                    }
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
    });

    use_future(move || {
        let mut timeline_viewport_width = timeline_viewport_width.clone();
        let mut scroll_offset = scroll_offset.clone();
        let timeline_viewport_eval = timeline_viewport_eval.clone();
        async move {
            loop {
                let Some(eval) = timeline_viewport_eval() else {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    continue;
                };
                let mut eval = eval;
                loop {
                    match eval.recv::<TimelineViewportState>().await {
                        Ok(state) => {
                            let width = state.width.max(0.0);
                            let scroll_left = state.scroll_left.max(0.0);
                            if timeline_viewport_width() != Some(width) {
                                timeline_viewport_width.set(Some(width));
                            }
                            if (scroll_offset() - scroll_left).abs() > 0.5 {
                                scroll_offset.set(scroll_left);
                            }
                        }
                        Err(_) => break,
                    }
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
    });

    use_effect(move || {
        let enabled = project.read().project_path.is_some() && startup_done();
        preview_native_enabled.set(enabled);
        if !enabled {
            preview_native_active.set(false);
            preview_native_uploaded.set(None);
            preview_gpu_upload_ms.set(None);
            preview_layers.set(None);
            preview_native_ready.set(false);
        }
    });

    let preview_layers_for_redraw = preview_layers.clone();
    use_effect(move || {
        if !preview_native_enabled() {
            return;
        }
        if preview_frame().is_some() || preview_layers_for_redraw().is_some() {
            desktop_for_redraw.window.request_redraw();
        }
    });

    use_wry_event_handler({
        let preview_gpu = preview_gpu.clone();
        let preview_native_bounds = preview_native_bounds.clone();
        let mut preview_native_active = preview_native_active.clone();
        let preview_native_enabled = preview_native_enabled.clone();
        let mut preview_native_attempted = preview_native_attempted.clone();
        let mut preview_native_uploaded = preview_native_uploaded.clone();
        let mut preview_gpu_upload_ms = preview_gpu_upload_ms.clone();
        let preview_layers = preview_layers.clone();
        let mut preview_native_ready = preview_native_ready.clone();
        let mut preview_dirty = preview_dirty.clone();
        let preview_native_suspended = preview_native_suspended.clone();
        let desktop = desktop_for_events.clone();
        move |event, target| {
            if !preview_native_enabled() {
                return;
            }
            let Some(bounds) = preview_native_bounds() else {
                return;
            };

            let is_main_window = match event {
                TaoEvent::RedrawRequested(window_id) => *window_id == desktop.window.id(),
                TaoEvent::WindowEvent { window_id, .. } => *window_id == desktop.window.id(),
                _ => false,
            };
            if !is_main_window {
                return;
            }

            if preview_native_suspended() {
                if let Some(gpu) = preview_gpu.borrow_mut().as_mut() {
                    gpu.clear_layers();
                }
                if preview_native_active() {
                    preview_native_active.set(false);
                }
                if preview_native_uploaded().is_some() {
                    preview_native_uploaded.set(None);
                }
                return;
            }

            let should_render = matches!(
                event,
                TaoEvent::RedrawRequested(window_id) if *window_id == desktop.window.id()
            );
            let should_update = should_render
                || matches!(
                    event,
                    TaoEvent::WindowEvent {
                        event: TaoWindowEvent::Resized(_) | TaoWindowEvent::Moved(_),
                        ..
                    }
                );
            if !should_update {
                return;
            }

            let mut gpu_state = preview_gpu.borrow_mut();
            if gpu_state.is_none() {
                if preview_native_attempted() {
                    return;
                }
                preview_native_attempted.set(true);
                if let Some(gpu) = PreviewGpuSurface::new(&desktop.window, target) {
                    *gpu_state = Some(gpu);
                    preview_native_ready.set(true);
                    preview_dirty.set(true);
                } else {
                    return;
                }
            }

            if let Some(gpu) = gpu_state.as_mut() {
                let mut uploaded = false;
                let upload_ms = if let Some((stack_id, stack)) = preview_layers() {
                    if stack.layers.is_empty() {
                        if preview_native_active() {
                            preview_native_active.set(false);
                        }
                        preview_native_uploaded.set(None);
                        gpu.clear_layers();
                        uploaded = true;
                        Some(0.0)
                    } else {
                        if !preview_native_active() {
                            preview_native_active.set(true);
                        }
                        if preview_native_uploaded() != Some(stack_id) {
                            let upload_start = Instant::now();
                            uploaded = gpu.upload_layers(&stack);
                            let elapsed_ms = upload_start.elapsed().as_secs_f64() * 1000.0;
                            if uploaded {
                                preview_native_uploaded.set(Some(stack_id));
                            }
                            Some(elapsed_ms)
                        } else {
                            Some(0.0)
                        }
                    }
                } else {
                    if preview_native_active() {
                        preview_native_active.set(false);
                    }
                    preview_native_uploaded.set(None);
                    gpu.clear_layers();
                    uploaded = true;
                    Some(0.0)
                };
                if let Some(ms) = upload_ms {
                    preview_gpu_upload_ms.set(Some(ms));
                }

                let changed = gpu.apply_bounds(bounds);
                if gpu.over_limit() {
                    if preview_native_active() {
                        preview_native_active.set(false);
                    }
                    preview_native_uploaded.set(None);
                    gpu.clear_layers();
                    preview_dirty.set(true);
                    return;
                }
                if should_render || changed || uploaded {
                    gpu.render_layers();
                }
            }
        }
    });

    //  Dialog state
    let mut show_new_project_dialog = use_signal(|| false); // Kept for "File > New" inside app
    let mut show_project_settings_dialog = use_signal(|| false);
    
    // V2 Provider modals
    let show_providers_v2 = use_signal(|| false);
    let mut show_json_editor = use_signal(|| false);
    let mut show_builder_v2 = use_signal(|| false);
    let mut edit_provider_path = use_signal(|| None::<std::path::PathBuf>);
    let mut provider_files_v2 = use_signal(Vec::<std::path::PathBuf>::new);
    
    let mut menu_open = use_signal(|| false); // Track if any dropdown menu is open

    // Simple handler to open V2 providers modal
    let mut open_providers_dialog = {
        let mut show_providers_v2 = show_providers_v2.clone();
        let mut provider_files_v2 = provider_files_v2.clone();
        move || {
            provider_files_v2.set(list_global_provider_files());
            show_providers_v2.set(true);
        }
    };

    // V2 Provider modal effects
    let desktop_for_modal_redraw = desktop.clone();
    let preview_gpu_for_modal = preview_gpu.clone();
    use_effect(move || {
        let suspended = show_providers_v2()
            || show_json_editor()
            || show_builder_v2()
            || show_new_project_dialog()
            || show_project_settings_dialog()
            || menu_open()
            || queue_open()
            || gen_video_modal_open();
        if preview_native_suspended() == suspended {
            return;
        }
        preview_native_suspended.set(suspended);
        if suspended {
            if let Some(gpu) = preview_gpu_for_modal.borrow_mut().as_mut() {
                gpu.clear_layers();
            }
            if preview_native_active() {
                preview_native_active.set(false);
            }
            if preview_native_uploaded().is_some() {
                preview_native_uploaded.set(None);
            }
        } else {
            desktop_for_modal_redraw.window.request_redraw();
        }
    });

    let audio_engine_for_automation = audio_engine.clone();
    use_future(move || {
        let audio_engine = audio_engine_for_automation.clone();
        let mut project = project.clone();
        let mut provider_entries = provider_entries.clone();
        let mut thumbnailer = thumbnailer.clone();
        let mut previewer = previewer.clone();
        let mut preview_dirty = preview_dirty.clone();
        let thumbnail_cache_buster = thumbnail_cache_buster.clone();
        let mut audio_waveform_cache_buster = audio_waveform_cache_buster.clone();
        let mut startup_done = startup_done.clone();
        let mut current_time = current_time.clone();
        let mut selection = selection.clone();
        let mut show_providers_v2 = show_providers_v2.clone();
        let mut provider_files_v2 = provider_files_v2.clone();
        async move {
            if !crate::core::automation::is_enabled() {
                return;
            }

            loop {
                while let Some(envelope) = crate::core::automation::try_recv_command() {
                    let response = match &envelope.command {
                        AutomationCommand::GetState => {
                            let project_read = project.read();
                            let selection_read = selection.read();
                            AutomationResponse::ok(automation_state_json(
                                &project_read,
                                current_time(),
                                startup_done(),
                                &selection_read,
                                show_providers_v2(),
                            ))
                        }
                        AutomationCommand::CreateProject {
                            parent_dir,
                            name,
                            settings,
                        } => {
                            let settings = settings.clone().unwrap_or_default();
                            let project_dir = parent_dir.join(name);
                            let preview_limits =
                                (settings.preview_max_width, settings.preview_max_height);
                            match crate::state::Project::create_in_with_settings(
                                &project_dir,
                                name.clone(),
                                settings,
                            ) {
                                Ok(new_project) => match new_project.project_path.clone() {
                                    Some(project_root) => {
                                        thumbnailer.set(Arc::new(
                                            crate::core::thumbnailer::Thumbnailer::new(
                                                project_root.clone(),
                                            ),
                                        ));
                                        previewer.set(Arc::new(
                                            crate::core::preview::PreviewRenderer::new_with_limits(
                                                project_root.clone(),
                                                PREVIEW_CACHE_BUDGET_BYTES,
                                                preview_limits.0,
                                                preview_limits.1,
                                            ),
                                        ));
                                        provider_entries
                                            .set(load_global_provider_entries_or_empty());
                                        project.set(new_project);
                                        current_time.set(0.0);
                                        selection.write().clear();
                                        preview_dirty.set(true);
                                        audio_waveform_cache_buster
                                            .set(audio_waveform_cache_buster() + 1);
                                        startup_done.set(true);
                                        spawn_missing_duration_probes(project);
                                        AutomationResponse::ok(serde_json::json!({
                                            "project_path": project_root,
                                        }))
                                    }
                                    None => AutomationResponse::error(
                                        "Created project has no project path.",
                                    ),
                                },
                                Err(err) => AutomationResponse::error(format!(
                                    "Failed to create project: {}",
                                    err
                                )),
                            }
                        }
                        AutomationCommand::OpenProject { folder } => {
                            match crate::state::Project::load(&folder) {
                                Ok(loaded_project) => match loaded_project.project_path.clone() {
                                    Some(project_root) => {
                                        let preview_limits = (
                                            loaded_project.settings.preview_max_width,
                                            loaded_project.settings.preview_max_height,
                                        );
                                        thumbnailer.set(Arc::new(
                                            crate::core::thumbnailer::Thumbnailer::new(
                                                project_root.clone(),
                                            ),
                                        ));
                                        previewer.set(Arc::new(
                                            crate::core::preview::PreviewRenderer::new_with_limits(
                                                project_root.clone(),
                                                PREVIEW_CACHE_BUDGET_BYTES,
                                                preview_limits.0,
                                                preview_limits.1,
                                            ),
                                        ));
                                        provider_entries
                                            .set(load_global_provider_entries_or_empty());
                                        project.set(loaded_project);
                                        current_time.set(0.0);
                                        selection.write().clear();
                                        preview_dirty.set(true);
                                        audio_waveform_cache_buster
                                            .set(audio_waveform_cache_buster() + 1);
                                        startup_done.set(true);
                                        spawn_missing_duration_probes(project);
                                        AutomationResponse::ok(serde_json::json!({
                                            "project_path": project_root,
                                        }))
                                    }
                                    None => AutomationResponse::error(
                                        "Loaded project has no project path.",
                                    ),
                                },
                                Err(err) => AutomationResponse::error(format!(
                                    "Failed to open project: {}",
                                    err
                                )),
                            }
                        }
                        AutomationCommand::ImportAsset { path } => {
                            let import_result = {
                                let mut project_write = project.write();
                                project_write.import_file(path)
                            };
                            match import_result {
                                Ok(asset_id) => {
                                    preview_dirty.set(true);
                                    let asset = project.read().find_asset(asset_id).cloned();
                                    if let Some(asset) = asset {
                                        let thumbs = thumbnailer.read().clone();
                                        let mut thumbnail_cache_buster =
                                            thumbnail_cache_buster.clone();
                                        spawn(async move {
                                            thumbs.generate(&asset, false).await;
                                            thumbnail_cache_buster
                                                .set(thumbnail_cache_buster() + 1);
                                        });
                                    }
                                    spawn_asset_duration_probe(project, asset_id);
                                    AutomationResponse::ok(serde_json::json!({
                                        "asset_id": asset_id,
                                    }))
                                }
                                Err(err) => AutomationResponse::error(format!(
                                    "Failed to import asset: {}",
                                    err
                                )),
                            }
                        }
                        AutomationCommand::AddAssetToTimeline {
                            asset_id,
                            asset_name,
                            time,
                        } => {
                            let selected_asset_id = {
                                let project_read = project.read();
                                (*asset_id)
                                    .or_else(|| {
                                        asset_name.as_ref().and_then(|name| {
                                            project_read
                                                .assets
                                                .iter()
                                                .find(|asset| asset.name == *name)
                                                .map(|asset| asset.id)
                                        })
                                    })
                                    .or_else(|| project_read.assets.first().map(|asset| asset.id))
                            };
                            match selected_asset_id {
                                Some(asset_id) => {
                                    let time = (*time).unwrap_or_else(|| current_time());
                                    let duration = resolve_asset_duration_seconds(project, asset_id)
                                        .unwrap_or(DEFAULT_CLIP_DURATION_SECONDS);
                                    let clip_id = project
                                        .write()
                                        .add_clip_from_asset(asset_id, time, duration);
                                    match clip_id {
                                        Some(clip_id) => {
                                            preview_dirty.set(true);
                                            AutomationResponse::ok(serde_json::json!({
                                                "clip_id": clip_id,
                                                "asset_id": asset_id,
                                                "time": time,
                                                "duration": duration,
                                            }))
                                        }
                                        None => AutomationResponse::error(
                                            "Asset could not be placed on a compatible timeline track.",
                                        ),
                                    }
                                }
                                None => AutomationResponse::error(
                                    "No matching asset found for add_asset_to_timeline.",
                                ),
                            }
                        }
                        AutomationCommand::Seek { time } => {
                            let project_read = project.read();
                            let duration = project_read.duration();
                            let fps = project_read.settings.fps.max(1.0);
                            drop(project_read);
                            let snapped = snap_time_to_frame(*time, fps).clamp(0.0, duration);
                            current_time.set(snapped);
                            if let Some(engine) = audio_engine.as_ref() {
                                engine.seek_seconds(snapped);
                            }
                            AutomationResponse::ok(serde_json::json!({
                                "current_time": snapped,
                            }))
                        }
                        AutomationCommand::SelectClip { clip_id, index } => {
                            let selected_clip_id = {
                                let project_read = project.read();
                                (*clip_id).or_else(|| {
                                    let index = (*index).unwrap_or(0);
                                    project_read.clips.get(index).map(|clip| clip.id)
                                })
                            };
                            match selected_clip_id {
                                Some(clip_id) => {
                                    selection.write().select_clip(clip_id);
                                    AutomationResponse::ok(serde_json::json!({
                                        "clip_id": clip_id,
                                    }))
                                }
                                None => AutomationResponse::error(
                                    "No matching clip found for select_clip.",
                                ),
                            }
                        }
                        AutomationCommand::AddMarker { time } => {
                            let project_read = project.read();
                            let duration = project_read.duration();
                            let fps = project_read.settings.fps.max(1.0);
                            drop(project_read);
                            let time = (*time).unwrap_or_else(|| current_time());
                            let snapped = snap_time_to_frame(time, fps).clamp(0.0, duration);
                            let marker = crate::state::Marker::new(snapped);
                            let marker_id = project.write().add_marker(marker);
                            selection.write().select_marker(marker_id);
                            preview_dirty.set(true);
                            AutomationResponse::ok(serde_json::json!({
                                "marker_id": marker_id,
                                "time": snapped,
                            }))
                        }
                        AutomationCommand::SaveProject => match project.read().save() {
                            Ok(()) => AutomationResponse::empty_ok(),
                            Err(err) => AutomationResponse::error(format!(
                                "Failed to save project: {}",
                                err
                            )),
                        },
                        AutomationCommand::OpenProviders => {
                            provider_files_v2.set(list_global_provider_files());
                            show_providers_v2.set(true);
                            AutomationResponse::empty_ok()
                        }
                        AutomationCommand::CloseProviders => {
                            show_providers_v2.set(false);
                            AutomationResponse::empty_ok()
                        }
                    };
                    envelope.respond(response);
                }

                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        }
    });
    
    // On first load, if project has no path effectively, treat as "No Project Loaded"
    // But since we initialize with default(), we need a flag to block interaction until New/Open
    // We'll use specific "show_startup_modal" derived state
    
    let show_startup = project.read().project_path.is_none() && !startup_done();

    // Read current values
    let left_w = if left_collapsed() { PANEL_COLLAPSED_WIDTH } else { left_width() };
    let right_w = if right_collapsed() { PANEL_COLLAPSED_WIDTH } else { right_width() };
    let timeline_h = if timeline_collapsed() { TIMELINE_COLLAPSED_HEIGHT } else { timeline_height() };
    
    // Is currently dragging? (for cursor and user-select styling)

    // Always disable text selection for UI elements to feel like a native app
    // Specific text areas that need selection (e.g. inputs, logs) must override this
    let user_select_style = "none";
    let drag_cursor = match dragging() {
        Some("left") | Some("right") => "ew-resize",
        Some("timeline") => "ns-resize",
        Some("playhead") => "ew-resize",
        _ => "default",
    };
    
    // Ghost asset for drag and drop
    let drag_ghost_asset = if let Some(id) = dragged_asset() {
        project.read().assets.iter().find(|a| a.id == id).cloned()
    } else {
        None
    };
    
    // Which panel is currently being resized? (to disable transitions during drag)
    let left_resizing = dragging() == Some("left");
    let right_resizing = dragging() == Some("right");
    let timeline_resizing = dragging() == Some("timeline");
    let queue_count = generation_queue()
        .iter()
        .filter(|job| matches!(job.status, GenerationJobStatus::Queued | GenerationJobStatus::Running))
        .count();
    let queue_running = generation_active().is_some();
    let queue_paused = generation_paused();
    let on_enqueue_generation = {
        let mut generation_queue = generation_queue.clone();
        move |job: GenerationJob| {
            generation_queue.write().push(job);
        }
    };
    let on_delete_generation_job = {
        let mut generation_queue = generation_queue.clone();
        move |job_id: uuid::Uuid| {
            let mut queue = generation_queue.write();
            if let Some(index) = queue.iter().position(|job| job.id == job_id) {
                if queue[index].status == GenerationJobStatus::Running {
                    return;
                }
                queue.remove(index);
            }
        }
    };
    let on_clear_generation_queue = {
        let mut generation_queue = generation_queue.clone();
        let mut generation_paused = generation_paused.clone();
        let mut generation_pause_reason = generation_pause_reason.clone();
        move |_| {
            let mut queue = generation_queue.write();
            queue.retain(|job| job.status == GenerationJobStatus::Running);
            generation_paused.set(false);
            generation_pause_reason.set(None);
        }
    };
    let on_resume_generation_queue = {
        let mut generation_paused = generation_paused.clone();
        let mut generation_pause_reason = generation_pause_reason.clone();
        let mut generation_queue = generation_queue.clone();
        move |_| {
            generation_paused.set(false);
            generation_pause_reason.set(None);
            let mut queue = generation_queue.write();
            for job in queue.iter_mut() {
                if job.status == GenerationJobStatus::Queued {
                    job.attempts = 0;
                    job.next_attempt_at = None;
                    if let Some(error) = job.error.as_ref() {
                        if error.contains("Provider offline") {
                            job.error = None;
                        }
                    }
                }
            }
        }
    };
    let audio_engine_for_hotkeys = audio_engine.clone();
    let audio_sample_cache_for_hotkeys = audio_sample_cache.clone();
    let audio_decode_in_flight_for_hotkeys = audio_decode_in_flight.clone();
    let project_for_hotkeys = project.clone();
    let current_time_for_hotkeys = current_time.clone();
    let is_playing_for_hotkeys = is_playing.clone();
    let zoom_for_hotkeys = zoom.clone();
    let scroll_offset_for_hotkeys = scroll_offset.clone();
    let timeline_viewport_width_for_hotkeys = timeline_viewport_width.clone();

    rsx! {
        // Global CSS with drag state handling
        style {
            r#"
            *, *::before, *::after {{ box-sizing: border-box; }}
            html, body {{ margin: 0; padding: 0; overflow: hidden; background-color: {BG_BASE}; }}
            body {{ -webkit-font-smoothing: antialiased; }}
            ::-webkit-scrollbar {{ width: 6px; height: 6px; }}
            ::-webkit-scrollbar-track {{ background: transparent; }}
            ::-webkit-scrollbar-thumb {{ background: {BORDER_DEFAULT}; border-radius: 3px; }}
            ::-webkit-scrollbar-thumb:hover {{ background: {BORDER_STRONG}; }}
            .collapse-btn {{ opacity: 0.6; transition: opacity 0.15s ease, background-color 0.15s ease; }}
            .collapse-btn:hover {{ opacity: 1; background-color: {BG_HOVER} !important; }}
            .resize-handle {{ transition: background-color 0.15s ease; }}
            .resize-handle:hover {{ background-color: {BORDER_ACCENT} !important; }}
            .resize-handle:active {{ background-color: {BORDER_ACCENT} !important; }}
            .collapsed-rail {{ transition: background-color 0.15s ease; }}
            .collapsed-rail:hover {{ background-color: {BG_HOVER} !important; }}
            .resize-handle-left:hover > div, .resize-handle-right:hover > div {{ opacity: 1 !important; }}
            .menu-button {{ transition: background-color 0.1s ease; }}
            .menu-button:hover {{ background-color: {BG_HOVER} !important; }}
            .menu-item {{ transition: background-color 0.1s ease; }}
            .menu-item:hover:not(:disabled) {{ background-color: {BG_HOVER}; }}
            .queue-running {{ animation: queuePulse 1.6s ease-in-out infinite; }}
            @keyframes queuePulse {{
                0% {{ box-shadow: 0 0 0 0 rgba(249, 115, 22, 0.0); }}
                45% {{ box-shadow: 0 0 0 2px rgba(249, 115, 22, 0.35); }}
                75% {{ box-shadow: 0 0 0 4px rgba(249, 115, 22, 0.0); }}
                100% {{ box-shadow: 0 0 0 0 rgba(249, 115, 22, 0.0); }}
            }}
            .info-tooltip:hover .tooltip-content {{ opacity: 1; }}
            "#
        }

        // Main app container
        div {
            class: "app-container",
            style: "
                display: flex; flex-direction: column;
                width: 100vw; height: 100vh;
                background-color: {BG_BASE}; color: {TEXT_PRIMARY};
                font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, Oxygen, Ubuntu, sans-serif;
                overflow: hidden; position: fixed; top: 0; left: 0;
                user-select: {user_select_style};
                cursor: {drag_cursor};
            ",
            onmousedown: move |_| {
                timeline_focused.set(false);
            },
            
            onmousemove: {
                let audio_engine = audio_engine.clone();
                move |e| {
                if dragged_asset().is_some() {
                    mouse_pos.set((e.client_coordinates().x, e.client_coordinates().y));
                }
                
                if let Some(target) = dragging() {
                    e.prevent_default();
                    match target {
                        "left" => {
                            let delta = e.client_coordinates().x - drag_start_pos();
                            let new_w = (drag_start_size() + delta).clamp(PANEL_MIN_WIDTH, PANEL_MAX_WIDTH);
                            left_width.set(new_w);
                        }
                        "right" => {
                            let delta = drag_start_pos() - e.client_coordinates().x;
                            let new_w = (drag_start_size() + delta).clamp(PANEL_MIN_WIDTH, PANEL_MAX_WIDTH);
                            right_width.set(new_w);
                        }
                        "timeline" => {
                            let delta = drag_start_pos() - e.client_coordinates().y;
                            let new_h = (drag_start_size() + delta).clamp(TIMELINE_MIN_HEIGHT, TIMELINE_MAX_HEIGHT);
                            timeline_height.set(new_h);
                        }
                        "playhead" => {
                            // Convert mouse x delta to time delta using zoom factor
                            let delta_px = e.client_coordinates().x - drag_start_pos();
                            let delta_frames = (delta_px / zoom()) * timeline_fps;
                            let start_frames =
                                frames_from_seconds(drag_start_size(), timeline_fps).round();
                            let mut new_frames = start_frames + delta_frames;
                            let snap_enabled = !e.modifiers().alt();
                            let snap_threshold_frames = if zoom() > 0.0 {
                                (TIMELINE_SNAP_THRESHOLD_PX / zoom()) * timeline_fps
                            } else {
                                0.0
                            };
                            if snap_enabled {
                                let snap_targets: Vec<SnapTarget> = timeline_snap_targets
                                    .iter()
                                    .copied()
                                    .filter(|target| target.kind != SnapTargetKind::Playhead)
                                    .collect();
                                if let Some(hit) = best_snap_delta_frames(
                                    &[new_frames],
                                    &snap_targets,
                                    snap_threshold_frames,
                                ) {
                                    new_frames += hit.delta_frames;
                                }
                            }
                            let max_frames = frames_from_seconds(duration, timeline_fps).round();
                            let snapped_frames = new_frames.round().clamp(0.0, max_frames);
                            let snapped_time = seconds_from_frames(snapped_frames, timeline_fps);
                            current_time.set(snapped_time);
                            if let Some(engine) = audio_engine.as_ref() {
                                engine.seek_seconds(snapped_time);
                                engine.trigger_scrub_preview(
                                    ((engine.sample_rate() as f64) * 0.03).round() as u64,
                                );
                            }
                        }
                        _ => {}
                    }
                }
            }
            },
            onmouseup: {
                let audio_engine = audio_engine.clone();
                move |_| {
                    dragging.set(None);
                    dragged_asset.set(None);
                    if is_scrubbing() {
                        is_scrubbing.set(false);
                        if let Some(engine) = audio_engine.as_ref() {
                            engine.set_scrub_hold(false);
                            if scrub_was_playing() {
                                engine.seek_seconds(current_time());
                                engine.play();
                                is_playing.set(true);
                            } else {
                                engine.pause();
                            }
                        }
                    }
                }
            },
            // Suppress the browser's default context menu - we'll use custom menus
            oncontextmenu: move |e| e.prevent_default(),
            // Enable keyboard focus on this container for hotkeys
            tabindex: "0",
            // Hotkey handler
            onkeydown: move |e: KeyboardEvent| {
                // Build context for hotkey dispatch
                let selection_state = selection.read();
                let has_selection =
                    !selection_state.clip_ids.is_empty() || !selection_state.marker_ids.is_empty();
                drop(selection_state);
                let hotkey_context = HotkeyContext {
                    timeline_visible: !timeline_collapsed(),
                    has_selection,
                    input_focused: false, // TODO: track when input fields have focus
                };

                // Get modifier states
                let modifiers = e.modifiers();
                let shift = modifiers.shift();
                let ctrl = modifiers.ctrl();
                let alt = modifiers.alt();
                let meta = modifiers.meta();

                // Dispatch the hotkey
                match handle_hotkey(&e.key(), shift, ctrl, alt, meta, &hotkey_context) {
                    HotkeyResult::Action(action) => {
                        if matches!(action, HotkeyAction::PlayPause) && !timeline_focused() {
                            return;
                        }
                        e.prevent_default();
                        match action {
                            HotkeyAction::TimelineZoomIn => {
                                let (min_zoom, max_zoom) = timeline_zoom_bounds(
                                    duration,
                                    timeline_viewport_width(),
                                    timeline_fps,
                                );
                                let new_zoom =
                                    (zoom_for_hotkeys() * 1.25).clamp(min_zoom, max_zoom);
                                set_timeline_zoom_anchored(
                                    new_zoom,
                                    duration,
                                    timeline_viewport_width_for_hotkeys(),
                                    current_time_for_hotkeys(),
                                    zoom_for_hotkeys.clone(),
                                    scroll_offset_for_hotkeys.clone(),
                                );
                            }
                            HotkeyAction::TimelineZoomOut => {
                                let (min_zoom, max_zoom) = timeline_zoom_bounds(
                                    duration,
                                    timeline_viewport_width(),
                                    timeline_fps,
                                );
                                let new_zoom =
                                    (zoom_for_hotkeys() * 0.8).clamp(min_zoom, max_zoom);
                                set_timeline_zoom_anchored(
                                    new_zoom,
                                    duration,
                                    timeline_viewport_width_for_hotkeys(),
                                    current_time_for_hotkeys(),
                                    zoom_for_hotkeys.clone(),
                                    scroll_offset_for_hotkeys.clone(),
                                );
                            }
                            HotkeyAction::PlayPause => {
                                timeline_focused.set(true);
                                toggle_playback(
                                    &audio_engine_for_hotkeys,
                                    &audio_sample_cache_for_hotkeys,
                                    &audio_decode_in_flight_for_hotkeys,
                                    project_for_hotkeys.clone(),
                                    current_time_for_hotkeys.clone(),
                                    is_playing_for_hotkeys.clone(),
                                );
                            }
                            HotkeyAction::SaveProject => {
                                if let Err(err) = project.read().save() {
                                    println!("[PROJECT SAVE] Failed: {}", err);
                                } else {
                                    println!("[PROJECT SAVE] Saved.");
                                }
                            }
                        }
                    }
                    HotkeyResult::NoMatch | HotkeyResult::Suppressed => {}
                }
            },
            // Note: We intentionally don't clear drag on mouseleave so drag continues
            // if the user moves outside the window and back in while still holding mouse button

            // Drag Ghost
            if let Some(asset) = drag_ghost_asset {
                div {
                    style: "
                        position: fixed; left: {mouse_pos().0 + 15.0}px; top: {mouse_pos().1 + 15.0}px;
                        background-color: {BG_ELEVATED}; border: 1px solid {ACCENT_VIDEO};
                        border-radius: 4px; padding: 6px 10px; font-size: 12px; pointer-events: none;
                        z-index: 10000; box-shadow: 0 4px 12px rgba(0,0,0,0.3); opacity: 0.9;
                        color: {TEXT_PRIMARY}; display: flex; align-items: center; gap: 6px;
                    ",
                    span { "📄" } // Generic icon for now
                    "{asset.name}"
                }
            }

                TitleBar { 
                    project_name: project.read().name.clone(),
                    on_new_project: move |_| {
                        show_new_project_dialog.set(true);
                    },
                    on_save: move |_| {
                        // Since project knows its own path (if loaded/saved once), we can just save
                        // If it's effectively unsaved (default path), we might want a "Save As" flow eventually
                        // For now, MVP assumes we have a path from startup or just saves to current effective path
                        let _ = project.read().save(); 
                    },
                    on_project_settings: move |_| {
                        if project.read().project_path.is_some() && startup_done() {
                            show_project_settings_dialog.set(true);
                        }
                    },
                    on_open_providers: move |_| {
                        open_providers_dialog();
                    },
                    show_preview_stats: show_preview_stats(),
                    on_toggle_preview_stats: move |_| {
                        show_preview_stats.set(!show_preview_stats());
                    },
                    use_hw_decode: use_hw_decode(),
                    on_toggle_hw_decode: move |_| {
                        use_hw_decode.set(!use_hw_decode());
                        preview_dirty.set(true);
                    },
                    queue_count: queue_count,
                    queue_open: queue_open(),
                    queue_running: queue_running,
                    queue_paused: queue_paused,
                    project_loaded: project.read().project_path.is_some() && startup_done(),
                    on_toggle_queue: move |_| {
                        queue_open.set(!queue_open());
                    },
                    on_menu_open: move |is_open| {
                        menu_open.set(is_open);
                    },
                }

            // Main content
            div {
                class: "main-content",
                style: "display: flex; flex: 1; overflow: hidden;",

                // Left panel - Assets
                SidePanel {
                    title: "Assets",
                    width: left_w,
                    collapsed: left_collapsed(),
                    side: "left",
                    is_resizing: left_resizing,
                    on_toggle: move |_| left_collapsed.set(!left_collapsed()),
                    on_resize_start: move |e: MouseEvent| {
                        e.prevent_default();
                        dragging.set(Some("left"));
                        drag_start_pos.set(e.client_coordinates().x);
                        drag_start_size.set(left_width());
                    },
                    
                    // Assets panel content
                    AssetsPanelContent {
                        assets: project.read().assets.clone(),
                        thumbnailer: thumbnailer.read().clone(),
                        thumbnail_cache_buster: thumbnail_cache_buster(),
                        thumbnail_refresh_tick: thumbnail_refresh_tick(),
                        panel_width: left_w,
                        gen_video_modal_open: gen_video_modal_open,
                        on_import: move |asset: crate::state::Asset| {
                            let mut project_write = project.write();
                            project_write.add_asset(asset.clone());
                            let _ = project_write.save_generative_config(asset.id);
                            preview_dirty.set(true);
                            let thumbs = thumbnailer.read().clone();
                            let mut thumbnail_cache_buster = thumbnail_cache_buster.clone();
                            // Spawn background task for thumbnail generation
                            spawn(async move {
                                thumbs.generate(&asset, false).await;
                                thumbnail_cache_buster.set(thumbnail_cache_buster() + 1);
                            });
                        },
                        on_import_file: move |path: std::path::PathBuf| {
                            // Implicit Copy: Always import directly designated by the strict folder policy
                            // We assume a project exists because the startup modal blocks everything else
                            let import_result = project.write().import_file(&path);
                            match import_result {
                                Ok(asset_id) => {
                                    preview_dirty.set(true);
                                    // Trigger thumbnail generation for the new asset
                                    if let Some(asset) = project.read().find_asset(asset_id).cloned() {
                                        let thumbs = thumbnailer.read().clone();
                                        let mut thumbnail_cache_buster = thumbnail_cache_buster.clone();
                                        spawn(async move {
                                            thumbs.generate(&asset, false).await;
                                            thumbnail_cache_buster.set(thumbnail_cache_buster() + 1);
                                        });
                                    }
                                    spawn_asset_duration_probe(project, asset_id);
                                },
                                Err(e) => println!("Failed to import file {:?}: {}", path, e),
                            }
                        },
                        on_rename: move |(asset_id, name): (uuid::Uuid, String)| {
                            let trimmed = name.trim();
                            if trimmed.is_empty() {
                                return;
                            }
                            project.write().rename_asset(asset_id, trimmed.to_string());
                        },
                        on_regenerate_thumbnails: move |asset_id: uuid::Uuid| {
                            if let Some(asset) = project.read().find_asset(asset_id).cloned() {
                                let thumbs = thumbnailer.read().clone();
                                let mut thumbnail_cache_buster = thumbnail_cache_buster.clone();
                                let mut audio_waveform_cache_buster = audio_waveform_cache_buster.clone();
                                let project_root = project.read().project_path.clone();
                                spawn(async move {
                                     // Force regeneration logic could require a flag in future,
                                     // but our `generate` function currently checks existence.
                                     // To force it, we should probably delete the cache dir first or add a force flag.
                                     // For now, let's just assume the user deleted the cache or we want to try again if it failed.
                                     // To firmly force it, we'll manually nuke the cache dir for this asset here before calling generate.
                                    
                                     // We need to resolve the cache path to delete it.
                                     // Thumbnailer encapsulates the path logic.
                                     // Let's just trust `generate` or update `Thumbnailer` later to support `force`.
                                     // Actually, user asked for manual trigger. Let's make it robust by deleting first.
                                     // But `Thumbnailer` struct has `cache_root` private.
                                     // Let's just call `generate` for now. If it already exists it returns silently.
                                     // If we really want "Re-generate", we need to clear it.
                                     // Let's updated `Thumbnailer` in a separate step or just rely on 'generate' doing a check.
                                     // User said "what triggers re-generation... force manually".
                                     // If `generate` returns early on existing folder, this won't do anything if folder exists.
                                     // We should probably modify `Thumbnailer::generate` to accept a `force` boolean.
                                     // I will do that in the next step. For now, let's wire up the UI.
                                     if asset.is_visual() {
                                         thumbs.generate(&asset, true).await;
                                         thumbnail_cache_buster.set(thumbnail_cache_buster() + 1);
                                     }
                                    if asset.is_audio() {
                                        if let Some(project_root) = project_root {
                                            if let Some(source_path) = crate::core::audio::waveform::resolve_audio_source(
                                                &project_root,
                                                &asset,
                                            ) {
                                                let _ = tokio::task::spawn_blocking(move || {
                                                    crate::core::audio::waveform::build_and_store_peak_cache(
                                                        &project_root,
                                                        asset.id,
                                                        &source_path,
                                                        crate::core::audio::waveform::PeakBuildConfig::default(),
                                                    )
                                                })
                                                .await;
                                            } else {
                                            }
                                        }
                                        audio_waveform_cache_buster
                                            .set(audio_waveform_cache_buster() + 1);
                                    }
                                });
                            }
                        },
                        on_delete: move |id| {
                            project.write().remove_asset(id);
                            preview_dirty.set(true);
                        },
                        on_add_to_timeline: move |asset_id| {
                            // Add clip at current playhead position using asset duration when available
                            let time = current_time();
                            let duration = resolve_asset_duration_seconds(project, asset_id)
                                .unwrap_or(DEFAULT_CLIP_DURATION_SECONDS);
                            project.write().add_clip_from_asset(asset_id, time, duration);
                            preview_dirty.set(true);
                            if let Some(asset) = project.read().find_asset(asset_id).cloned() {
                                if asset.is_audio() {
                                    if let Some(project_root) = project.read().project_path.clone() {
                                        if let Some(source_path) = resolve_audio_source(&project_root, &asset) {
                                            let mut audio_waveform_cache_buster = audio_waveform_cache_buster.clone();
                                            spawn(async move {
                                                let needs_build = tokio::task::spawn_blocking({
                                                    let cache_path = peak_cache_path(&project_root, asset_id);
                                                    let source_path = source_path.clone();
                                                    move || {
                                                            if !cache_path.exists() {
                                                                return Ok::<bool, String>(true);
                                                            }
                                                        let cache = load_peak_cache(&cache_path)?;
                                                        Ok(!cache_matches_source(&cache, &source_path)?)
                                                    }
                                                })
                                                .await
                                                .ok()
                                                .unwrap_or(Ok(true))
                                                .unwrap_or(true);

                                                if needs_build {
                                                    let _ = tokio::task::spawn_blocking(move || {
                                                        build_and_store_peak_cache(
                                                            &project_root,
                                                            asset.id,
                                                            &source_path,
                                                            PeakBuildConfig::default(),
                                                        )
                                                    })
                                                    .await;
                                                    audio_waveform_cache_buster
                                                        .set(audio_waveform_cache_buster() + 1);
                                                }
                                            });
                                        }
                                    }
                                }
                            }
                        },
                        on_drag_start: move |id| dragged_asset.set(Some(id)),
                    }
                }

                // Center
                div {
                    class: "center-area",
                    style: "display: flex; flex-direction: column; flex: 1; overflow: hidden;",

                    PreviewPanel {
                        width: project.read().settings.width,
                        height: project.read().settings.height,
                        fps: project.read().settings.fps,
                        preview_frame: preview_frame(),
                        preview_stats: preview_stats(),
                        preview_gpu_upload_ms: preview_gpu_upload_ms(),
                        show_preview_stats: show_preview_stats(),
                        preview_native_active: preview_native_active(),
                    }

                    // Timeline resize handle
                    div {
                        class: "resize-handle",
                        style: "height: 4px; background-color: {BORDER_DEFAULT}; cursor: ns-resize; flex-shrink: 0;",
                        onmousedown: move |e| {
                            if !timeline_collapsed() {
                                e.prevent_default();
                                dragging.set(Some("timeline"));
                                drag_start_pos.set(e.client_coordinates().y);
                                drag_start_size.set(timeline_height());
                            }
                        },
                    }

                        TimelinePanel {
                            height: timeline_h,
                            collapsed: timeline_collapsed(),
                            is_resizing: timeline_resizing,
                            on_toggle: move |_| timeline_collapsed.set(!timeline_collapsed()),
                            // Project data
                            tracks: project.read().tracks.clone(),

                            clips: project.read().clips.clone(),
                            markers: project.read().markers.clone(),
                            assets: project.read().assets.clone(),
                            thumbnailer: thumbnailer.read().clone(),
                            thumbnail_cache_buster: thumbnail_cache_buster(),
                            thumbnail_refresh_tick: thumbnail_refresh_tick(),
                            clip_cache_buckets: clip_cache_buckets(),
                            project_root: project.read().project_path.clone(),
                            audio_waveform_cache_buster: audio_waveform_cache_buster,
                            // Timeline state
                            current_time: current_time(),
                            duration: duration,
                            fps: timeline_fps,
                            zoom: zoom(),
                            min_zoom: timeline_zoom_bounds(
                                duration,
                                timeline_viewport_width(),
                                timeline_fps,
                            )
                            .0,
                            max_zoom: timeline_zoom_bounds(
                                duration,
                                timeline_viewport_width(),
                                timeline_fps,
                            )
                            .1,
                            is_playing: is_playing(),
                            scroll_offset: scroll_offset(),
                            // Callbacks
                            on_seek: {
                                let audio_engine = audio_engine.clone();
                                move |t: f64| {
                                    // Snap to frame boundary (60fps) and clamp to duration
                                    let snapped =
                                        snap_time_to_frame(t, timeline_fps).clamp(0.0, duration);
                                    current_time.set(snapped);
                                    if let Some(engine) = audio_engine.as_ref() {
                                        engine.seek_seconds(snapped);
                                    }
                                }
                            },
                            on_zoom_change: move |z: f64| {
                                let (min_zoom, max_zoom) = timeline_zoom_bounds(
                                    duration,
                                    timeline_viewport_width(),
                                    timeline_fps,
                                );
                                let new_zoom = z.clamp(min_zoom, max_zoom);
                                set_timeline_zoom_anchored(
                                    new_zoom,
                                    duration,
                                    timeline_viewport_width(),
                                    current_time(),
                                    zoom.clone(),
                                    scroll_offset.clone(),
                                );
                            },
                            on_play_pause: {
                                let audio_engine = audio_engine.clone();
                                let audio_sample_cache = audio_sample_cache.clone();
                                let audio_decode_in_flight = audio_decode_in_flight.clone();
                                let project = project.clone();
                                let current_time = current_time.clone();
                                let is_playing = is_playing.clone();
                                move |_| {
                                    toggle_playback(
                                        &audio_engine,
                                        &audio_sample_cache,
                                        &audio_decode_in_flight,
                                        project.clone(),
                                        current_time.clone(),
                                        is_playing.clone(),
                                    );
                                }
                            },
                            on_scroll: move |offset: f64| scroll_offset.set(offset),
                            on_seek_start: {
                                let audio_engine = audio_engine.clone();
                                let audio_sample_cache = audio_sample_cache.clone();
                                let audio_decode_in_flight = audio_decode_in_flight.clone();
                                let project = project.clone();
                                move |e: MouseEvent| {
                                    timeline_focused.set(true);
                                    let was_playing = is_playing();
                                    scrub_was_playing.set(was_playing);
                                    is_scrubbing.set(true);
                                    if was_playing {
                                        is_playing.set(false);
                                    }
                                    if let Some(engine) = audio_engine.as_ref() {
                                        let engine = Arc::clone(engine);
                                        if let Some(project_root) =
                                            project.read().project_path.clone()
                                        {
                                            let project_snapshot = project.read().clone();
                                            let (items, missing) =
                                                build_audio_playback_items(
                                                    &project_snapshot,
                                                    &project_root,
                                                    &engine,
                                                    &audio_sample_cache,
                                                    false,
                                                );
                                            engine.set_items(items);
                                            if !missing.is_empty() {
                                                let mut missing_set =
                                                    HashSet::<uuid::Uuid>::new();
                                                for id in missing {
                                                    missing_set.insert(id);
                                                }
                                                let mut targets =
                                                    audio_decode_targets_for_project(
                                                        &project_snapshot,
                                                        &project_root,
                                                    );
                                                targets.retain(|(id, _)| {
                                                    missing_set.contains(id)
                                                });
                                                let decode_config = AudioDecodeConfig {
                                                    target_rate: engine.sample_rate(),
                                                    target_channels: engine.channels(),
                                                };
                                                schedule_audio_decode_targets(
                                                    targets,
                                                    decode_config,
                                                    Arc::clone(&audio_sample_cache),
                                                    Arc::clone(&audio_decode_in_flight),
                                                    project_snapshot,
                                                    project_root,
                                                    Arc::clone(&engine),
                                                );
                                            }
                                        }
                                        engine.set_scrub_hold(true);
                                        engine.seek_seconds(current_time());
                                        engine.trigger_scrub_preview(
                                            ((engine.sample_rate() as f64) * 0.03).round() as u64,
                                        );
                                        engine.play();
                                    }
                                    dragging.set(Some("playhead"));
                                    drag_start_pos.set(e.client_coordinates().x);
                                    drag_start_size.set(current_time());
                                }
                            },
                            on_seek_end: move |_| dragging.set(None),
                            is_seeking: dragging() == Some("playhead"),
                            // Track management
                            on_add_video_track: move |_| {
                                project.write().add_video_track();
                                preview_dirty.set(true);
                            },
                            on_add_audio_track: move |_| {
                                project.write().add_audio_track();
                                preview_dirty.set(true);
                            },
                            on_track_context_menu: move |(x, y, track_id)| {
                                context_menu.set(Some((x, y, track_id)));
                            },
                            selected_tracks: selection.read().track_ids.clone(),
                            on_track_select: move |track_id| {
                                selection.write().select_track(track_id);
                                timeline_focused.set(true);
                            },
                            // Clip operations
                            on_clip_delete: move |clip_id| {
                                project.write().remove_clip(clip_id);
                                selection.write().remove_clip(clip_id);
                                preview_dirty.set(true);
                            },
                            on_clip_move: move |(clip_id, new_start)| {
                                project.write().move_clip(clip_id, new_start);
                                preview_dirty.set(true);
                            },
                            on_clip_resize: move |(clip_id, new_start, new_duration)| {
                                project.write().resize_clip(clip_id, new_start, new_duration);
                                preview_dirty.set(true);
                            },
                            on_clip_move_track: move |(clip_id, direction)| {
                                if project.write().move_clip_to_adjacent_track(clip_id, direction) {
                                    preview_dirty.set(true);
                                }
                            },
                            selected_clips: selection.read().clip_ids.clone(),
                            on_clip_select: move |clip_id| {
                                selection.write().select_clip(clip_id);
                                timeline_focused.set(true);
                            },
                            on_marker_add: move |time: f64| {
                                let snapped = snap_time_to_frame(time, timeline_fps).clamp(0.0, duration);
                                let target_frame = frames_from_seconds(snapped, timeline_fps).round();
                                let exists = project
                                    .read()
                                    .markers
                                    .iter()
                                    .any(|marker| {
                                        frames_from_seconds(marker.time, timeline_fps).round()
                                            == target_frame
                                    });
                                if !exists {
                                    let marker = crate::state::Marker::new(snapped);
                                    let id = project.write().add_marker(marker);
                                    selection.write().select_marker(id);
                                    timeline_focused.set(true);
                                }
                            },
                            on_marker_move: move |(marker_id, time)| {
                                let snapped = snap_time_to_frame(time, timeline_fps).clamp(0.0, duration);
                                project.write().move_marker(marker_id, snapped);
                                timeline_focused.set(true);
                            },
                            on_marker_delete: move |marker_id| {
                                project.write().remove_marker(marker_id);
                                selection.write().remove_marker(marker_id);
                                timeline_focused.set(true);
                            },
                            selected_markers: selection.read().marker_ids.clone(),
                            on_marker_select: move |marker_id| {
                                selection.write().select_marker(marker_id);
                                timeline_focused.set(true);
                            },
                            snap_targets: timeline_snap_targets.clone(),
                            // Asset Drag & Drop
                            dragged_asset: dragged_asset(),
                            on_asset_drop: {
                                let audio_engine = audio_engine.clone();
                                let audio_sample_cache = audio_sample_cache.clone();
                                let audio_decode_in_flight = audio_decode_in_flight.clone();
                                move |(track_id, time, asset_id)| {
                                let duration = resolve_asset_duration_seconds(project, asset_id)
                                    .unwrap_or(DEFAULT_CLIP_DURATION_SECONDS);
                                let clip = crate::state::Clip::new(asset_id, track_id, time, duration);
                                project.write().add_clip(clip);
                                preview_dirty.set(true);
                                if let Some(asset) = project.read().find_asset(asset_id).cloned() {
                                    if asset.is_audio() || asset.is_video() {
                                        if let Some(project_root) = project.read().project_path.clone() {
                                            if let Some(source_path) = resolve_audio_or_video_source(&project_root, &asset) {
                                                if let Some(engine) = audio_engine.as_ref() {
                                                    let project_snapshot = project.read().clone();
                                                    let decode_config = AudioDecodeConfig {
                                                        target_rate: engine.sample_rate(),
                                                        target_channels: engine.channels(),
                                                    };
                                                    schedule_audio_decode_targets(
                                                        vec![(asset.id, source_path.clone())],
                                                        decode_config,
                                                        Arc::clone(&audio_sample_cache),
                                                        Arc::clone(&audio_decode_in_flight),
                                                        project_snapshot,
                                                        project_root.clone(),
                                                        Arc::clone(engine),
                                                    );
                                                }
                                                if asset.is_audio() {
                                                    let mut audio_waveform_cache_buster = audio_waveform_cache_buster.clone();
                                                    spawn(async move {
                                                        let needs_build = tokio::task::spawn_blocking({
                                                            let cache_path = peak_cache_path(&project_root, asset_id);
                                                            let source_path = source_path.clone();
                                                            move || {
                                                                if !cache_path.exists() {
                                                                    return Ok::<bool, String>(true);
                                                                }
                                                                let cache = load_peak_cache(&cache_path)?;
                                                                Ok(!cache_matches_source(&cache, &source_path)?)
                                                            }
                                                        })
                                                        .await
                                                        .ok()
                                                        .unwrap_or(Ok(true))
                                                        .unwrap_or(true);

                                                        if needs_build {
                                                            let _ = tokio::task::spawn_blocking(move || {
                                                                build_and_store_peak_cache(
                                                                    &project_root,
                                                                    asset.id,
                                                                    &source_path,
                                                                    PeakBuildConfig::default(),
                                                                )
                                                            })
                                                            .await;
                                                            audio_waveform_cache_buster
                                                                .set(audio_waveform_cache_buster() + 1);
                                                        }
                                                    });
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            },
                            // Selection
                            on_deselect_all: move |_| {
                                selection.write().clear();
                                timeline_focused.set(true);
                            },
                            on_focus: move |_| {
                                timeline_focused.set(true);
                            },
                        }
                }

                // Right panel
                SidePanel {
                    title: "Attributes",  
                    width: right_w,
                    collapsed: right_collapsed(),
                    side: "right",
                    is_resizing: right_resizing,
                    on_toggle: move |_| right_collapsed.set(!right_collapsed()),
                    on_resize_start: move |e: MouseEvent| {
                        e.prevent_default();
                        dragging.set(Some("right"));
                        drag_start_pos.set(e.client_coordinates().x);
                        drag_start_size.set(right_width());
                    },
                    
                    // Attributes panel placeholder content
                        AttributesPanelContent {
                            key: "{attributes_key}",
                            project: project,
                            selection: selection,
                            preview_dirty: preview_dirty,
                            providers: provider_entries,
                            on_audio_items_refresh: {
                                let audio_engine = audio_engine.clone();
                                let audio_sample_cache = audio_sample_cache.clone();
                                let audio_decode_in_flight = audio_decode_in_flight.clone();
                                let project = project.clone();
                                move |_| {
                                    if let Some(engine) = audio_engine.as_ref() {
                                        if let Some(project_root) =
                                            project.read().project_path.clone()
                                        {
                                            let project_snapshot = project.read().clone();
                                            let (items, missing) = build_audio_playback_items(
                                                &project_snapshot,
                                                &project_root,
                                                engine,
                                                &audio_sample_cache,
                                                false,
                                            );
                                            engine.set_items(items);
                                            if !missing.is_empty() {
                                                let mut missing_set = HashSet::<uuid::Uuid>::new();
                                                for id in missing {
                                                    missing_set.insert(id);
                                                }
                                                let mut targets = audio_decode_targets_for_project(
                                                    &project_snapshot,
                                                    &project_root,
                                                );
                                                targets.retain(|(id, _)| missing_set.contains(id));
                                                let decode_config = AudioDecodeConfig {
                                                    target_rate: engine.sample_rate(),
                                                    target_channels: engine.channels(),
                                                };
                                                schedule_audio_decode_targets(
                                                    targets,
                                                    decode_config,
                                                    Arc::clone(&audio_sample_cache),
                                                    Arc::clone(&audio_decode_in_flight),
                                                    project_snapshot,
                                                    project_root,
                                                    Arc::clone(engine),
                                                );
                                            }
                                        }
                                    }
                                }
                            },
                            previewer: previewer,
                            thumbnailer: thumbnailer.read().clone(),
                            thumbnail_cache_buster: thumbnail_cache_buster,
                            on_enqueue_generation: on_enqueue_generation,
                        }
                }
            }

            StatusBar {}
            
            TrackContextMenu {
                context_menu: context_menu,
                project: project,
                selection: selection,
                preview_dirty: preview_dirty,
            }

            GenerationQueuePanel {
                open: queue_open(),
                jobs: generation_queue(),
                on_close: move |_| queue_open.set(false),
                on_clear_queue: on_clear_generation_queue,
                on_delete_job: on_delete_generation_job,
                paused: generation_paused(),
                pause_reason: generation_pause_reason(),
                on_resume: on_resume_generation_queue,
            }

            // Startup Modal (Blocks everything until Project is created/loaded)
            if show_startup {
                StartupModal {
                    mode: StartupModalMode::Create,
                    initial_name: None,
                    initial_settings: None,
                    initial_folder: None,
                    on_create: {
                        let audio_engine = audio_engine.clone();
                        let audio_sample_cache = audio_sample_cache.clone();
                        let audio_decode_in_flight = audio_decode_in_flight.clone();
                        move |(parent_dir, name, settings): (std::path::PathBuf, String, crate::state::ProjectSettings)| {
                        // Create full path: parent_dir/name
                        let project_dir = parent_dir.join(&name);
                        let preview_limits = (settings.preview_max_width, settings.preview_max_height);
                        match crate::state::Project::create_in_with_settings(&project_dir, &name, settings) {
                            Ok(new_proj) => {
                                // Initialize thumbnailer with new project path
                                thumbnailer.set(std::sync::Arc::new(crate::core::thumbnailer::Thumbnailer::new(new_proj.project_path.clone().unwrap())));
                                previewer.set(std::sync::Arc::new(
                                    crate::core::preview::PreviewRenderer::new_with_limits(
                                        new_proj.project_path.clone().unwrap(),
                                        PREVIEW_CACHE_BUDGET_BYTES,
                                        preview_limits.0,
                                        preview_limits.1,
                                    ),
                                ));
                                provider_entries.set(load_global_provider_entries_or_empty());
                                project.set(new_proj);
                                preview_dirty.set(true);
                                audio_waveform_cache_buster.set(audio_waveform_cache_buster() + 1);
                                if let Some(engine) = audio_engine.as_ref() {
                                    let project_snapshot = project.read().clone();
                                    if let Some(project_root) =
                                        project_snapshot.project_path.clone()
                                    {
                                        let targets = audio_decode_targets_for_project(
                                            &project_snapshot,
                                            &project_root,
                                        );
                                        if !targets.is_empty() {
                                            let decode_config = AudioDecodeConfig {
                                                target_rate: engine.sample_rate(),
                                                target_channels: engine.channels(),
                                            };
                                            schedule_audio_decode_targets(
                                                targets,
                                                decode_config,
                                                Arc::clone(&audio_sample_cache),
                                                Arc::clone(&audio_decode_in_flight),
                                                project_snapshot,
                                                project_root,
                                                Arc::clone(engine),
                                            );
                                        }
                                    }
                                }
                                spawn_missing_duration_probes(project);
                                startup_done.set(true);
                            },
                            Err(e) => println!("Error creating project: {}", e),
                        }
                    }
                    },
                    on_open: {
                        let audio_engine = audio_engine.clone();
                        let audio_sample_cache = audio_sample_cache.clone();
                        let audio_decode_in_flight = audio_decode_in_flight.clone();
                        move |path: std::path::PathBuf| {
                         match crate::state::Project::load(&path) { // path is the project folder
                            Ok(loaded_proj) => {
                                // Initialize thumbnailer with loaded project path
                                thumbnailer.set(std::sync::Arc::new(crate::core::thumbnailer::Thumbnailer::new(loaded_proj.project_path.clone().unwrap())));
                                let preview_limits = (
                                    loaded_proj.settings.preview_max_width,
                                    loaded_proj.settings.preview_max_height,
                                );
                                previewer.set(std::sync::Arc::new(
                                    crate::core::preview::PreviewRenderer::new_with_limits(
                                        loaded_proj.project_path.clone().unwrap(),
                                        PREVIEW_CACHE_BUDGET_BYTES,
                                        preview_limits.0,
                                        preview_limits.1,
                                    ),
                                ));
                                provider_entries.set(load_global_provider_entries_or_empty());
                                project.set(loaded_proj);
                                preview_dirty.set(true);
                                audio_waveform_cache_buster.set(audio_waveform_cache_buster() + 1);
                                if let Some(engine) = audio_engine.as_ref() {
                                    let project_snapshot = project.read().clone();
                                    if let Some(project_root) =
                                        project_snapshot.project_path.clone()
                                    {
                                        let targets = audio_decode_targets_for_project(
                                            &project_snapshot,
                                            &project_root,
                                        );
                                        if !targets.is_empty() {
                                            let decode_config = AudioDecodeConfig {
                                                target_rate: engine.sample_rate(),
                                                target_channels: engine.channels(),
                                            };
                                            schedule_audio_decode_targets(
                                                targets,
                                                decode_config,
                                                Arc::clone(&audio_sample_cache),
                                                Arc::clone(&audio_decode_in_flight),
                                                project_snapshot,
                                                project_root,
                                                Arc::clone(engine),
                                            );
                                        }
                                    }
                                }
                                spawn_missing_duration_probes(project);
                                startup_done.set(true);
                            },
                            Err(e) => println!("Error loading project: {}", e),
                        }
                    }
                    },
                    on_update: move |_| {},
                    on_close: move |_| {},
                }
            }

            if show_project_settings_dialog() {
                StartupModal {
                    mode: StartupModalMode::Edit,
                    initial_name: Some(project.read().name.clone()),
                    initial_settings: Some(project.read().settings.clone()),
                    initial_folder: project.read().project_path.clone(),
                    on_create: move |_| {},
                    on_open: move |_| {},
                    on_update: move |settings: crate::state::ProjectSettings| {
                        let preview_limits = (settings.preview_max_width, settings.preview_max_height);
                        let project_path = project.read().project_path.clone();
                        {
                            let mut project_mut = project.write();
                            project_mut.settings = settings;
                        }
                        if let Some(path) = project_path {
                            previewer.set(std::sync::Arc::new(
                                crate::core::preview::PreviewRenderer::new_with_limits(
                                    path,
                                    PREVIEW_CACHE_BUDGET_BYTES,
                                    preview_limits.0,
                                    preview_limits.1,
                                ),
                            ));
                        }
                        preview_dirty.set(true);
                        let _ = project.read().save();
                    },
                    on_close: move |_| {
                        show_project_settings_dialog.set(false);
                    },
                }
            }

            NewProjectModal {
                show: show_new_project_dialog,
                on_go_to_wizard: move |_| {
                    project.set(crate::state::Project::default());
                    startup_done.set(false);
                    show_new_project_dialog.set(false);
                }
            }

            // V2 Provider Modals
            ProvidersModalV2 {
                show: show_providers_v2,
                provider_files: provider_files_v2,
                on_new: move |_| {
                    edit_provider_path.set(None);
                    show_builder_v2.set(true);
                },
                on_reload: move |_| {
                    provider_files_v2.set(list_global_provider_files());
                },
                on_delete: move |path| {
                    let _ = std::fs::remove_file(&path);
                    provider_files_v2.set(list_global_provider_files());
                },
                on_edit_builder: move |path| {
                    edit_provider_path.set(Some(path));
                    show_builder_v2.set(true);
                },
                on_edit_json: move |path| {
                    edit_provider_path.set(Some(path));
                    show_json_editor.set(true);
                },
            }

            ProviderJsonEditorModal {
                show: show_json_editor,
                provider_path: edit_provider_path,
                on_saved: move |_| {
                    show_json_editor.set(false);
                    provider_files_v2.set(list_global_provider_files());
                    provider_entries.set(load_global_provider_entries_or_empty());
                },
            }

            ProviderBuilderModalV2 {
                show: show_builder_v2,
                provider_path: edit_provider_path,
                on_saved: move |_| {
                    show_builder_v2.set(false);
                    provider_files_v2.set(list_global_provider_files());
                    provider_entries.set(load_global_provider_entries_or_empty());
                },
            }
        }
    }
}
