use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use eframe::egui::{
    self, Align, Color32, ColorImage, Context, FontId, Layout, Pos2, Rect, RichText, Sense, Stroke,
    TextureHandle, TextureId, TextureOptions, Ui, Vec2,
};
use uuid::Uuid;

use crate::core::audio::cache::{
    cache_matches_source, load_peak_cache, peak_cache_path, PeakCache,
};
use crate::core::audio::decode::{decode_audio_to_f32, AudioDecodeConfig};
use crate::core::audio::playback::{AudioPlaybackEngine, PlaybackItem};
use crate::core::audio::waveform::{
    build_and_store_peak_cache, resolve_audio_or_video_source, resolve_audio_source,
    PeakBuildConfig,
};
use crate::core::generation::{
    next_version_label, random_seed_i64, resolve_provider_inputs, resolve_seed_field,
    update_seed_inputs,
};
use crate::core::preview::{PreviewDecodeMode, PreviewFrameInfo, PreviewStats};
use crate::core::preview_store;
use crate::core::timeline_snap::{
    best_snap_delta_frames, frames_from_seconds, seconds_from_frames, snap_time_to_frame,
    SnapTarget,
};
use crate::editor::{
    default_generative_video_fps, default_generative_video_frames, default_projects_dir,
    generative_video_duration_label, EditorState,
};
use crate::providers::comfyui;
use crate::state::{
    asset_display_name, input_value_as_bool, input_value_as_f64, input_value_as_i64,
    input_value_as_string, parse_version_index, Asset, AssetKind, Clip, ClipTransform,
    ComfyOutputSelector, ComfyWorkflowRef, GenerationJob, GenerationJobStatus, GenerationRecord,
    GenerativeConfig, InputBinding, InputUi, InputValue, ManifestInput, NodeSelector, Project,
    ProjectSettings, ProviderConnection, ProviderEntry, ProviderInputField, ProviderInputType,
    ProviderManifest, ProviderOutputType, SeedStrategy, TrackType,
};
use crate::ui_kit as kit;
use egui_extras::{Size, StripBuilder};

const PROJECT_WIZARD_SIZE: [f32; 2] = [760.0, 660.0];
const PROJECT_WIZARD_CARD_H: f32 = 526.0;
const PROJECT_WIZARD_MIN_SIZE: [f32; 2] = [560.0, 500.0];
const TIMELINE_LABEL_W: f32 = 140.0;
const TIMELINE_HEADER_H: f32 = 32.0;
const TIMELINE_RULER_H: f32 = 24.0;
const TIMELINE_TRACK_H: f32 = 36.0;
const TIMELINE_ADD_ROW_H: f32 = 42.0;
const TIMELINE_CLIP_H: f32 = 32.0;
const TIMELINE_CLIP_Y_PAD: f32 = 2.0;
const TIMELINE_SCROLLBAR_H: f32 = 12.0;
const TIMELINE_MIN_ZOOM_FLOOR: f32 = 0.1;
const TIMELINE_MAX_PX_PER_FRAME: f32 = 8.0;
const TIMELINE_SNAP_THRESHOLD_PX: f64 = 6.0;
const TIMELINE_THUMB_TILE_W: f32 = 60.0;
const TIMELINE_MAX_THUMB_TILES: usize = 120;
const TIMELINE_MIN_CLIP_W: f32 = 2.0;
const TIMELINE_HANDLE_W: f32 = 8.0;
const TIMELINE_MARKER_HIT_W: f32 = 22.0;
const TIMELINE_MARKER_LABEL_W: f32 = 96.0;
const TIMELINE_MARKER_LABEL_H: f32 = 18.0;
const TIMELINE_SCRUB_PREVIEW_SECONDS: f64 = 0.12;
const TIMELINE_HEADER_PAD_X: f32 = 4.0;
const TIMELINE_HEADER_LEFT_W: f32 = 286.0;
const TIMELINE_HEADER_RIGHT_W: f32 = 102.0;
const TIMELINE_HEADER_CENTER_GAP: f32 = 8.0;
const TIMELINE_TRANSPORT_BUTTON_COUNT: f32 = 5.0;
const MEDIA_EXTENSIONS: &[&str] = &[
    "mp4", "mov", "mkv", "webm", "avi", "png", "jpg", "jpeg", "gif", "webp", "wav", "mp3", "flac",
    "ogg",
];
const VIDEO_EXTENSIONS: &[&str] = &["mp4", "mov", "mkv", "webm", "avi"];
const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp"];
const AUDIO_EXTENSIONS: &[&str] = &["wav", "mp3", "flac", "ogg"];
const JSON_EXTENSIONS: &[&str] = &["json"];
const ASSET_IMPORT_FILTERS: &[kit::FileExtensionFilter<'static>] = &[
    kit::FileExtensionFilter {
        name: "Media",
        extensions: MEDIA_EXTENSIONS,
    },
    kit::FileExtensionFilter {
        name: "Video",
        extensions: VIDEO_EXTENSIONS,
    },
    kit::FileExtensionFilter {
        name: "Image",
        extensions: IMAGE_EXTENSIONS,
    },
    kit::FileExtensionFilter {
        name: "Audio",
        extensions: AUDIO_EXTENSIONS,
    },
];
const JSON_FILE_FILTERS: &[kit::FileExtensionFilter<'static>] = &[kit::FileExtensionFilter {
    name: "JSON",
    extensions: JSON_EXTENSIONS,
}];
const PROVIDERS_MODAL_SIZE: [f32; 2] = [760.0, 560.0];
const PROVIDER_JSON_MODAL_SIZE: [f32; 2] = [920.0, 700.0];
const PROVIDER_BUILDER_MODAL_SIZE: [f32; 2] = [1080.0, 720.0];
const QUEUE_PANEL_W: f32 = 320.0;
const QUEUE_PANEL_MIN_H: f32 = 132.0;
const QUEUE_PANEL_PAD: f32 = 12.0;
const QUEUE_PANEL_HEADER_H: f32 = 30.0;
const QUEUE_PANEL_GAP: f32 = 8.0;
const QUEUE_PANEL_MARGIN: f32 = 10.0;
const QUEUE_PANEL_MAX_APP_GAP: f32 = 60.0;
const QUEUE_EMPTY_BODY_H: f32 = 42.0;
const QUEUE_JOB_GAP: f32 = 8.0;
const QUEUE_JOB_CARD_H: f32 = 64.0;
const QUEUE_JOB_RUNNING_H: f32 = 106.0;
const QUEUE_JOB_FAILED_H: f32 = 84.0;
const MAX_GENERATION_BATCH_COUNT: u32 = 50;

fn project_wizard_size(ctx: &Context) -> Vec2 {
    let available = ctx.content_rect().size();
    let max_w = (available.x - 24.0).max(320.0);
    let max_h = (available.y - 24.0).max(360.0);
    Vec2::new(
        PROJECT_WIZARD_SIZE[0]
            .min(max_w)
            .max(PROJECT_WIZARD_MIN_SIZE[0].min(max_w)),
        PROJECT_WIZARD_SIZE[1]
            .min(max_h)
            .max(PROJECT_WIZARD_MIN_SIZE[1].min(max_h)),
    )
}

fn modal_size(ctx: &Context, desired: [f32; 2], min: [f32; 2]) -> Vec2 {
    let available = ctx.content_rect().size();
    let max_w = (available.x - 24.0).max(min[0].min(available.x));
    let max_h = (available.y - 24.0).max(min[1].min(available.y));
    Vec2::new(
        desired[0].min(max_w).max(min[0].min(max_w)),
        desired[1].min(max_h).max(min[1].min(max_h)),
    )
}

pub fn run() -> eframe::Result<()> {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("NLA AI Video Creator")
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([960.0, 620.0]),
        ..Default::default()
    };

    eframe::run_native(
        "NLA AI Video Creator",
        native_options,
        Box::new(|cc| Ok(Box::new(NlaEguiApp::new(cc)))),
    )
}

pub struct NlaEguiApp {
    editor: EditorState,
    preview_texture: Option<TextureHandle>,
    asset_thumbnails: HashMap<Uuid, AssetThumbnail>,
    asset_thumbnail_misses: HashSet<Uuid>,
    timeline_thumbnails: HashMap<TimelineThumbnailKey, AssetThumbnail>,
    timeline_thumbnail_misses: HashSet<TimelineThumbnailKey>,
    audio_peak_caches: HashMap<Uuid, PeakCache>,
    audio_peak_builds: HashSet<Uuid>,
    audio_engine: Option<Arc<AudioPlaybackEngine>>,
    audio_sample_cache: Arc<Mutex<HashMap<Uuid, Arc<Vec<f32>>>>>,
    audio_decode_in_flight: Arc<Mutex<HashSet<Uuid>>>,
    audio_decode_failures: Arc<Mutex<HashMap<Uuid, String>>>,
    audio_decode_warmup_pending: bool,
    timeline_drag: Option<TimelineDrag>,
    timeline_snap_preview: Option<f64>,
    timeline_scrub_was_playing: bool,
    preview_frame: Option<PreviewFrameInfo>,
    preview_stats: Option<PreviewStats>,
    last_tick: Instant,
    new_project_name: String,
    new_project_parent: PathBuf,
    project_settings: ProjectSettings,
    gen_video_fps: f64,
    gen_video_frames: u32,
    selected_provider_file: Option<PathBuf>,
    provider_json_editor_path: Option<PathBuf>,
    provider_json_text: String,
    provider_json_error: Option<String>,
    provider_builder_open: bool,
    provider_builder: ProviderBuilderState,
    generation_runtime: Option<tokio::runtime::Runtime>,
    generation_events_tx: mpsc::Sender<GenerationEvent>,
    generation_events_rx: mpsc::Receiver<GenerationEvent>,
    generation_active: Option<Uuid>,
    queue_button_rect: Option<Rect>,
}

struct AssetThumbnail {
    texture: TextureHandle,
    size: Vec2,
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
struct TimelineThumbnailKey {
    asset_id: Uuid,
    bucket_millis: u64,
}

#[derive(Clone, Copy, Debug)]
struct TimelineThumbTile {
    texture_id: TextureId,
    size: Vec2,
}

#[derive(Clone, Copy, Debug)]
enum TimelineDrag {
    Playhead,
    ClipMove {
        clip_id: Uuid,
        start_time: f64,
        duration: f64,
    },
    ClipResizeLeft {
        clip_id: Uuid,
        start_time: f64,
        duration: f64,
    },
    ClipResizeRight {
        clip_id: Uuid,
        start_time: f64,
        duration: f64,
    },
    MarkerMove {
        marker_id: Uuid,
        start_time: f64,
    },
}

#[derive(Clone, Copy, Debug)]
enum TimelineHit {
    Ruler,
    TrackLabel(Uuid),
    ClipBody(Uuid),
    ClipLeftEdge(Uuid),
    ClipRightEdge(Uuid),
    Marker(Uuid),
    EmptyTrack,
    Empty,
}

#[derive(Clone, Copy, Debug)]
struct TimelineRects {
    outer: Rect,
    label: Rect,
    ruler: Rect,
    tracks: Rect,
    add_row: Rect,
    scrollbar: Rect,
}

#[derive(Clone, Copy, Debug)]
struct TimelineClipGeom {
    clip_id: Uuid,
    rect: Rect,
}

#[derive(Clone, Copy, Debug)]
struct TimelineMarkerGeom {
    marker_id: Uuid,
    hit_rect: Rect,
}

#[derive(Clone, Copy, Debug)]
struct AssetTimelineDragPayload {
    asset_id: Uuid,
}

#[derive(Debug)]
enum GenerationEvent {
    Progress {
        job_id: Uuid,
        overall: Option<f32>,
        node: Option<f32>,
    },
    Finished {
        job_id: Uuid,
        result: Result<GenerationOutput, GenerationFailure>,
    },
}

#[derive(Debug)]
struct GenerationOutput {
    version: String,
    path: PathBuf,
}

#[derive(Debug)]
enum GenerationFailure {
    Offline(String),
    Error(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ProviderBuilderTab {
    Inputs,
    Output,
}

#[derive(Clone, Debug)]
struct ProviderBuilderState {
    source_path: Option<PathBuf>,
    provider_id: Uuid,
    provider_name: String,
    output_type: ProviderOutputType,
    base_url: String,
    workflow_path: Option<PathBuf>,
    manifest_path: Option<PathBuf>,
    workflow_nodes: Vec<crate::core::comfyui_workflow::ComfyWorkflowNode>,
    workflow_error: Option<String>,
    workflow_search: String,
    selected_node_id: Option<String>,
    output_key: String,
    output_tag: String,
    output_node: Option<ProviderOutputNodeDraft>,
    inputs: Vec<ProviderBuilderInput>,
    tab: ProviderBuilderTab,
    error: Option<String>,
}

#[derive(Clone, Debug)]
struct ProviderOutputNodeDraft {
    class_type: String,
    title: Option<String>,
}

#[derive(Clone, Debug)]
struct ProviderNodeSelectorDraft {
    class_type: String,
    input_key: String,
    title: Option<String>,
}

#[derive(Clone, Debug)]
struct ProviderBuilderInput {
    name: String,
    label: String,
    input_type_key: String,
    required: bool,
    default_text: String,
    enum_options: String,
    tag: String,
    multiline: bool,
    selector: ProviderNodeSelectorDraft,
}

impl NlaEguiApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        kit::configure_style(&cc.egui_ctx);
        let mut editor = EditorState::new();
        let audio_engine = match AudioPlaybackEngine::new() {
            Ok(engine) => Some(Arc::new(engine)),
            Err(err) => {
                editor.status = format!("Audio unavailable: {err}");
                None
            }
        };
        let generation_runtime = match tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .thread_name("nla-generation")
            .build()
        {
            Ok(runtime) => Some(runtime),
            Err(err) => {
                editor.status = format!("Generation runtime unavailable: {err}");
                None
            }
        };
        let (generation_events_tx, generation_events_rx) = mpsc::channel();
        Self {
            project_settings: editor.project.settings.clone(),
            editor,
            preview_texture: None,
            asset_thumbnails: HashMap::new(),
            asset_thumbnail_misses: HashSet::new(),
            timeline_thumbnails: HashMap::new(),
            timeline_thumbnail_misses: HashSet::new(),
            audio_peak_caches: HashMap::new(),
            audio_peak_builds: HashSet::new(),
            audio_engine,
            audio_sample_cache: Arc::new(Mutex::new(HashMap::new())),
            audio_decode_in_flight: Arc::new(Mutex::new(HashSet::new())),
            audio_decode_failures: Arc::new(Mutex::new(HashMap::new())),
            audio_decode_warmup_pending: false,
            timeline_drag: None,
            timeline_snap_preview: None,
            timeline_scrub_was_playing: false,
            preview_frame: None,
            preview_stats: None,
            last_tick: Instant::now(),
            new_project_name: "My New Project".to_string(),
            new_project_parent: default_projects_dir(),
            gen_video_fps: default_generative_video_fps(),
            gen_video_frames: default_generative_video_frames(),
            selected_provider_file: None,
            provider_json_editor_path: None,
            provider_json_text: String::new(),
            provider_json_error: None,
            provider_builder_open: false,
            provider_builder: ProviderBuilderState::default(),
            generation_runtime,
            generation_events_tx,
            generation_events_rx,
            generation_active: None,
            queue_button_rect: None,
        }
    }

    fn poll_automation(&mut self) {
        if !crate::core::automation::is_enabled() {
            return;
        }
        while let Some(envelope) = crate::core::automation::try_recv_command() {
            let previous_project_path = self.editor.project.project_path.clone();
            let response = self.editor.apply_automation_command(&envelope.command);
            self.project_settings = self.editor.project.settings.clone();
            if self.editor.project.project_path != previous_project_path {
                self.clear_project_runtime_cache();
                self.warm_audio_playback_cache();
            }
            envelope.respond(response);
        }
    }

    fn clear_project_runtime_cache(&mut self) {
        self.preview_texture = None;
        self.preview_frame = None;
        self.preview_stats = None;
        self.asset_thumbnails.clear();
        self.asset_thumbnail_misses.clear();
        self.timeline_thumbnails.clear();
        self.timeline_thumbnail_misses.clear();
        self.audio_peak_caches.clear();
        self.audio_peak_builds.clear();
        if let Ok(mut cache) = self.audio_sample_cache.lock() {
            cache.clear();
        }
        if let Ok(mut in_flight) = self.audio_decode_in_flight.lock() {
            in_flight.clear();
        }
        if let Ok(mut failures) = self.audio_decode_failures.lock() {
            failures.clear();
        }
        self.audio_decode_warmup_pending = false;
        if let Some(engine) = &self.audio_engine {
            engine.pause();
            engine.set_items(Vec::new());
            engine.set_scrub_hold(false);
        }
        self.editor.is_playing = false;
        self.timeline_drag = None;
        self.timeline_snap_preview = None;
        self.timeline_scrub_was_playing = false;
        self.generation_active = None;
    }

    fn open_project_folder(&mut self, folder: PathBuf) -> bool {
        match self.editor.open_project(folder) {
            Ok(_) => {
                self.project_settings = self.editor.project.settings.clone();
                self.clear_project_runtime_cache();
                self.warm_audio_playback_cache();
                true
            }
            Err(err) => {
                self.editor.status = err;
                false
            }
        }
    }

    fn keep_automation_responsive(&self, ctx: &Context) {
        if crate::core::automation::is_enabled() {
            ctx.request_repaint_after(Duration::from_millis(50));
        }
    }

    fn warm_audio_playback_cache(&mut self) {
        let Some(engine) = self.audio_engine.as_ref() else {
            return;
        };
        let Some(project_root) = self.editor.project.project_path.clone() else {
            return;
        };
        let targets = audio_decode_targets_for_project(&self.editor.project, &project_root);
        if targets.is_empty() {
            return;
        }
        let decode_config = AudioDecodeConfig {
            target_rate: engine.sample_rate(),
            target_channels: engine.channels(),
        };
        schedule_audio_decode_targets(
            targets,
            decode_config,
            Arc::clone(&self.audio_sample_cache),
            Arc::clone(&self.audio_decode_in_flight),
            Arc::clone(&self.audio_decode_failures),
        );
        self.audio_decode_warmup_pending = true;
    }

    fn service_audio_decode_warmup(&mut self, ctx: &Context) {
        if !self.audio_decode_warmup_pending {
            return;
        }

        self.refresh_audio_playback_items();
        let in_flight = self
            .audio_decode_in_flight
            .lock()
            .ok()
            .map(|in_flight| !in_flight.is_empty())
            .unwrap_or(false);
        if in_flight {
            ctx.request_repaint_after(Duration::from_millis(80));
        } else {
            self.audio_decode_warmup_pending = false;
        }
    }

    fn tick_playback(&mut self, ctx: &Context) {
        let now = Instant::now();
        let delta = now.saturating_duration_since(self.last_tick).as_secs_f64();
        self.last_tick = now;
        if !self.editor.is_playing {
            return;
        }
        let duration = self.editor.project.duration();

        if self.audio_engine.is_some() {
            self.refresh_audio_playback_items();
            let engine = self.audio_engine.as_ref().unwrap();
            let time = engine.playhead_seconds();
            let snapped = snap_time_to_frame(time.min(duration), self.editor.project.settings.fps);
            self.editor.current_time = snapped;
            self.editor.preview_dirty = true;
            if time >= duration {
                engine.pause();
                engine.set_scrub_hold(false);
                self.editor.is_playing = false;
            }
            ctx.request_repaint();
            return;
        }

        let next = self.editor.current_time + delta;
        if next >= duration {
            self.seek_editor(duration, false);
            self.editor.is_playing = false;
        } else {
            self.seek_editor(next, false);
        }
        ctx.request_repaint();
    }

    fn seek_editor(&mut self, time: f64, scrub_audio: bool) {
        let duration = self.editor.project.duration();
        let snapped = snap_time_to_frame(
            time.clamp(0.0, duration),
            self.editor.project.settings.fps.max(1.0),
        );
        self.editor.seek(snapped);
        let Some(engine) = self.audio_engine.as_ref().map(Arc::clone) else {
            return;
        };

        if scrub_audio {
            self.refresh_audio_playback_items();
        }
        engine.seek_seconds(self.editor.current_time);
        if scrub_audio && !self.editor.is_playing {
            engine.set_scrub_hold(true);
            engine.trigger_scrub_preview(
                ((engine.sample_rate() as f64) * TIMELINE_SCRUB_PREVIEW_SECONDS).round() as u64,
            );
            engine.play();
        }
    }

    fn toggle_playback(&mut self) {
        let next_playing = !self.editor.is_playing;
        if let Some(engine) = self.audio_engine.as_ref().map(Arc::clone) {
            if next_playing {
                self.refresh_audio_playback_items();
                engine.set_scrub_hold(false);
                engine.seek_seconds(self.editor.current_time);
                engine.play();
            } else {
                engine.set_scrub_hold(false);
                engine.pause();
            }
        }
        self.editor.is_playing = next_playing;
    }

    fn refresh_audio_playback_items(&mut self) {
        let Some(engine) = self.audio_engine.as_ref().map(Arc::clone) else {
            return;
        };
        let Some(project_root) = self.editor.project.project_path.clone() else {
            engine.set_items(Vec::new());
            return;
        };

        let project_snapshot = self.editor.project.clone();
        let (items, missing) = build_audio_playback_items(
            &project_snapshot,
            &project_root,
            &engine,
            &self.audio_sample_cache,
            &self.audio_decode_failures,
            false,
        );
        engine.set_items(items);
        if missing.is_empty() {
            return;
        }

        let missing: HashSet<Uuid> = missing.into_iter().collect();
        let mut targets = audio_decode_targets_for_project(&project_snapshot, &project_root);
        targets.retain(|(asset_id, _)| missing.contains(asset_id));
        let decode_config = AudioDecodeConfig {
            target_rate: engine.sample_rate(),
            target_channels: engine.channels(),
        };
        schedule_audio_decode_targets(
            targets,
            decode_config,
            Arc::clone(&self.audio_sample_cache),
            Arc::clone(&self.audio_decode_in_flight),
            Arc::clone(&self.audio_decode_failures),
        );
    }

    fn finish_timeline_scrub(&mut self) {
        let Some(engine) = self.audio_engine.as_ref() else {
            self.timeline_scrub_was_playing = false;
            return;
        };
        engine.set_scrub_hold(false);
        if self.timeline_scrub_was_playing {
            engine.seek_seconds(self.editor.current_time);
            engine.play();
            self.editor.is_playing = true;
        } else if !self.editor.is_playing {
            engine.pause();
        }
        self.timeline_scrub_was_playing = false;
    }

    fn update_preview_texture(&mut self, ctx: &Context) {
        if !self.editor.preview_dirty && self.preview_texture.is_some() {
            return;
        }
        if self.editor.project.project_path.is_none() {
            self.preview_texture = None;
            self.preview_frame = None;
            return;
        }

        let output = self.editor.previewer.render_frame(
            &self.editor.project,
            self.editor.current_time,
            PreviewDecodeMode::Seek,
            self.editor.layout.hardware_decode,
        );
        self.preview_stats = Some(output.stats);
        let Some(frame) = output.frame else {
            self.preview_texture = None;
            self.preview_frame = None;
            self.editor.preview_dirty = false;
            return;
        };
        let Some(bytes) = preview_store::get_preview_bytes(frame.version) else {
            return;
        };
        let image = ColorImage::from_rgba_unmultiplied(
            [frame.width as usize, frame.height as usize],
            &bytes,
        );
        if let Some(texture) = self.preview_texture.as_mut() {
            texture.set(image, TextureOptions::LINEAR);
        } else {
            self.preview_texture =
                Some(ctx.load_texture("preview-frame", image, TextureOptions::LINEAR));
        }
        self.preview_frame = Some(frame);
        self.editor.preview_dirty = false;
    }

    fn asset_thumbnail(&mut self, ctx: &Context, asset: &Asset) -> Option<(egui::TextureId, Vec2)> {
        if let Some(thumbnail) = self.asset_thumbnails.get(&asset.id) {
            return Some((thumbnail.texture.id(), thumbnail.size));
        }
        if self.asset_thumbnail_misses.contains(&asset.id) {
            return None;
        }

        let project_root = self.editor.project.project_path.as_deref()?;
        for path in asset_thumbnail_candidates(project_root, asset) {
            if let Some((image, size)) = load_thumbnail_image(&path) {
                let texture = ctx.load_texture(
                    format!("asset-thumbnail-{}", asset.id),
                    image,
                    TextureOptions::LINEAR,
                );
                let texture_id = texture.id();
                self.asset_thumbnails
                    .insert(asset.id, AssetThumbnail { texture, size });
                return Some((texture_id, size));
            }
        }

        self.asset_thumbnail_misses.insert(asset.id);
        None
    }

    fn timeline_thumbnail(
        &mut self,
        ctx: &Context,
        asset: &Asset,
        time_seconds: f64,
    ) -> Option<(egui::TextureId, Vec2)> {
        let bucket_millis = (time_seconds.max(0.0).floor() * 1000.0) as u64;
        let key = TimelineThumbnailKey {
            asset_id: asset.id,
            bucket_millis,
        };
        if let Some(thumbnail) = self.timeline_thumbnails.get(&key) {
            return Some((thumbnail.texture.id(), thumbnail.size));
        }
        if self.timeline_thumbnail_misses.contains(&key) {
            return self.asset_thumbnail(ctx, asset);
        }

        let Some(path) = self
            .editor
            .thumbnailer
            .get_thumbnail_path(asset.id, time_seconds)
        else {
            return self.asset_thumbnail(ctx, asset);
        };
        if let Some((image, size)) = load_thumbnail_image(&path) {
            let texture = ctx.load_texture(
                format!("timeline-thumbnail-{}-{}", asset.id, bucket_millis),
                image,
                TextureOptions::LINEAR,
            );
            let texture_id = texture.id();
            self.timeline_thumbnails
                .insert(key, AssetThumbnail { texture, size });
            return Some((texture_id, size));
        }

        self.timeline_thumbnail_misses.insert(key);
        self.asset_thumbnail(ctx, asset)
    }

    fn timeline_clip_thumbnail_tiles(
        &mut self,
        ctx: &Context,
        asset: &Asset,
        clip: &Clip,
        clip_rect: Rect,
        zoom: f32,
    ) -> Vec<TimelineThumbTile> {
        if !asset.is_visual() || clip_rect.width() <= 8.0 {
            return Vec::new();
        }

        let fallback = self.asset_thumbnail(ctx, asset);
        let mut tile_w = TIMELINE_THUMB_TILE_W.max(1.0);
        let estimated = (clip_rect.width() / tile_w).ceil().max(1.0) as usize;
        if estimated > TIMELINE_MAX_THUMB_TILES {
            tile_w = (clip_rect.width() / TIMELINE_MAX_THUMB_TILES as f32)
                .ceil()
                .max(1.0);
        }
        let tile_count = (clip_rect.width() / tile_w).ceil().max(1.0) as usize;
        let tile_time = tile_w as f64 / zoom.max(TIMELINE_MIN_ZOOM_FLOOR) as f64;
        let mut tiles = Vec::with_capacity(tile_count);

        for index in 0..tile_count {
            let time_in_clip = (index as f64 * tile_time).min(clip.duration.max(0.0));
            let source_time = clip.trim_in_seconds.max(0.0) + time_in_clip;
            let tile = self
                .timeline_thumbnail(ctx, asset, source_time)
                .or(fallback);
            if let Some((texture_id, size)) = tile {
                tiles.push(TimelineThumbTile { texture_id, size });
            }
        }

        tiles
    }

    fn top_bar(&mut self, root: &mut Ui) {
        let response = egui::Panel::top("top_bar")
            .exact_size(kit::TOP_BAR_H)
            .frame(kit::chrome_frame())
            .show_inside(root, |ui| {
                ui.horizontal_centered(|ui| {
                    menu_button(
                        ui,
                        "File",
                        |ui, this: &mut Self| {
                            if ui.button("New Project...").clicked() {
                                this.editor.overlays.new_project = true;
                                ui.close();
                            }
                            if ui.button("Open Project...").clicked() {
                                let initial_dir = default_projects_dir();
                                let options = kit::BrowsePathOptions::new()
                                    .id_salt("menu_open_project")
                                    .initial_dir(initial_dir.as_path())
                                    .remember_last_dir();
                                if let Some(folder) = kit::pick_folder_dialog(ui, options) {
                                    this.open_project_folder(folder);
                                }
                                ui.close();
                            }
                            ui.add_enabled_ui(this.editor.project.project_path.is_some(), |ui| {
                                if ui.button("Project Settings...").clicked() {
                                    this.project_settings = this.editor.project.settings.clone();
                                    this.editor.overlays.project_settings = true;
                                    ui.close();
                                }
                                if ui.button("Save").clicked() {
                                    if let Err(err) = this.editor.save() {
                                        this.editor.status = err;
                                    }
                                    ui.close();
                                }
                            });
                        },
                        self,
                    );

                    menu_button(
                        ui,
                        "Edit",
                        |ui, this: &mut Self| {
                            if ui.button("Add Marker").clicked() {
                                this.editor.add_marker(None);
                                ui.close();
                            }
                            if ui.button("Create Generative Video...").clicked() {
                                this.editor.overlays.generative_video = true;
                                ui.close();
                            }
                        },
                        self,
                    );

                    menu_button(
                        ui,
                        "View",
                        |ui, this: &mut Self| {
                            ui.checkbox(&mut this.editor.layout.preview_stats, "Preview Stats");
                            ui.checkbox(&mut this.editor.layout.left_collapsed, "Collapse Assets");
                            ui.checkbox(
                                &mut this.editor.layout.right_collapsed,
                                "Collapse Attributes",
                            );
                            ui.checkbox(
                                &mut this.editor.layout.timeline_collapsed,
                                "Collapse Timeline",
                            );
                        },
                        self,
                    );

                    menu_button(
                        ui,
                        "Settings",
                        |ui, this: &mut Self| {
                            if ui.button("AI Providers...").clicked() {
                                this.editor.refresh_providers();
                                this.editor.overlays.providers = true;
                                ui.close();
                            }
                            ui.checkbox(&mut this.editor.layout.hardware_decode, "Hardware Decode");
                        },
                        self,
                    );

                    menu_button(
                        ui,
                        "Help",
                        |ui, this: &mut Self| {
                            ui.label(RichText::new("NLA AI Video Creator").strong());
                            ui.label(
                                RichText::new("egui migration build")
                                    .small()
                                    .color(kit::TEXT_MUTED),
                            );
                            if ui.button("Open Harness Docs").clicked() {
                                this.editor.status = "See docs/DESKTOP_TEST_HARNESS.md".to_string();
                                ui.close();
                            }
                        },
                        self,
                    );

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let active_count = self
                            .editor
                            .generation_queue
                            .iter()
                            .filter(|job| {
                                matches!(
                                    job.status,
                                    crate::state::GenerationJobStatus::Queued
                                        | crate::state::GenerationJobStatus::Running
                                )
                            })
                            .count();
                        let attention = active_count > 0;
                        let queue_response = kit::queue_toggle_button(
                            ui,
                            active_count,
                            self.editor.overlays.queue,
                            attention,
                        );
                        self.queue_button_rect = Some(queue_response.rect);
                        if queue_response.clicked() {
                            self.editor.overlays.queue = !self.editor.overlays.queue;
                        }
                    });
                });
            });
        kit::paint_panel_edge(root, response.response.rect, kit::PanelEdge::Bottom);
    }

    fn left_panel(&mut self, root: &mut Ui) {
        if self.editor.layout.left_collapsed {
            let response = egui::Panel::left("assets_collapsed")
                .exact_size(kit::COLLAPSED_RAIL_W)
                .frame(kit::collapsed_dock_frame())
                .show_inside(root, |ui| {
                    if kit::collapsed_rail_button(ui, "▶").clicked() {
                        self.editor.layout.left_collapsed = false;
                    }
                });
            kit::paint_panel_edge(root, response.response.rect, kit::PanelEdge::Right);
            return;
        }

        let response = egui::Panel::left("assets")
            .resizable(true)
            .default_size(self.editor.layout.left_width)
            .size_range(180.0..=420.0)
            .frame(kit::dock_frame())
            .show_inside(root, |ui| {
                kit::fixed_panel_body(ui, |ui| self.assets_panel(ui));
            });
        kit::paint_panel_edge(root, response.response.rect, kit::PanelEdge::Right);
    }

    fn assets_panel(&mut self, ui: &mut Ui) {
        kit::panel_header(ui, "ASSETS", Some("◀"), || {
            self.editor.layout.left_collapsed = true;
        });
        ui.add_space(8.0);
        let mut create_video = false;
        let mut create_image = false;
        let mut create_audio = false;
        kit::stack_card_panel(ui, ADD_ASSETS_CARD_H, |ui| {
            kit::field_label(ui, "Add Assets");
            ui.add_space(6.0);
            let import_button_w = ui.available_width();
            if kit::secondary_button(ui, "Import Files...", import_button_w).clicked() {
                let initial_dir = self
                    .editor
                    .project
                    .project_path
                    .clone()
                    .unwrap_or_else(default_projects_dir);
                let options = kit::BrowseFileOptions::new()
                    .id_salt("asset_import_file")
                    .initial_dir(initial_dir.as_path())
                    .remember_last_dir()
                    .filters(ASSET_IMPORT_FILTERS);
                if let Some(path) = kit::pick_file_dialog(ui, options) {
                    match self.editor.import_asset(path) {
                        Ok(asset_id) => self.editor.selection.asset_ids = vec![asset_id],
                        Err(err) => self.editor.status = err,
                    }
                }
            }

            ui.add_space(kit::FORM_ROW_GAP);
            kit::field_label(ui, "New Generative");
            ui.add_space(6.0);
            kit::equal_media_pill_row(
                ui,
                &[
                    ("Video", kit::VIDEO),
                    ("Image", kit::IMAGE),
                    ("Audio", kit::AUDIO),
                ],
                |index| match index {
                    0 => create_video = true,
                    1 => create_image = true,
                    2 => create_audio = true,
                    _ => {}
                },
            );
        });
        if create_video {
            self.editor.overlays.generative_video = true;
        }
        if create_image {
            if let Err(err) = self.editor.create_generative_image() {
                self.editor.status = err;
            }
        }
        if create_audio {
            if let Err(err) = self.editor.create_generative_audio() {
                self.editor.status = err;
            }
        }

        ui.add_space(kit::FORM_ROW_GAP);
        let mut clear_selection = false;
        kit::scroll_body(ui, |ui| {
            ui.spacing_mut().item_spacing.y = kit::FORM_ROW_GAP;
            let assets: Vec<Asset> = self.editor.project.assets.clone();
            for asset in assets {
                let selected = self.editor.selection.asset_ids.contains(&asset.id);
                let thumbnail = self.asset_thumbnail(ui.ctx(), &asset);
                let response = asset_row(ui, &asset, selected, thumbnail);
                response.dnd_set_drag_payload(AssetTimelineDragPayload { asset_id: asset.id });
                if response.clicked() {
                    self.editor.selection.clear();
                    self.editor.selection.asset_ids.push(asset.id);
                }
                response.context_menu(|ui| {
                    if ui.button("Add to timeline").clicked() {
                        if let Err(err) = self.editor.add_asset_to_timeline(asset.id, None) {
                            self.editor.status = err;
                        }
                        ui.close();
                    }
                    if ui.button("Delete").clicked() {
                        self.editor.project.remove_asset(asset.id);
                        self.editor.selection.clear();
                        self.asset_thumbnails.remove(&asset.id);
                        self.asset_thumbnail_misses.remove(&asset.id);
                        self.editor.preview_dirty = true;
                        ui.close();
                    }
                });
            }
            let empty_height = (ui.clip_rect().bottom() - ui.cursor().top()).max(0.0);
            if empty_height > 0.0 {
                let (_, response) = ui.allocate_exact_size(
                    Vec2::new(ui.available_width(), empty_height),
                    Sense::click(),
                );
                if response.clicked() {
                    clear_selection = true;
                }
            }
        });
        if clear_selection {
            self.editor.selection.clear();
        }
    }

    fn right_panel(&mut self, root: &mut Ui) {
        if self.editor.layout.right_collapsed {
            let response = egui::Panel::right("attributes_collapsed")
                .exact_size(kit::COLLAPSED_RAIL_W)
                .frame(kit::collapsed_dock_frame())
                .show_inside(root, |ui| {
                    if kit::collapsed_rail_button(ui, "◀").clicked() {
                        self.editor.layout.right_collapsed = false;
                    }
                });
            kit::paint_panel_edge(root, response.response.rect, kit::PanelEdge::Left);
            return;
        }

        let response = egui::Panel::right("attributes")
            .resizable(true)
            .default_size(self.editor.layout.right_width)
            .size_range(200.0..=440.0)
            .frame(kit::dock_frame())
            .show_inside(root, |ui| {
                kit::fixed_panel_body(ui, |ui| self.attributes_panel(ui));
            });
        kit::paint_panel_edge(root, response.response.rect, kit::PanelEdge::Left);
    }

    fn attributes_panel(&mut self, ui: &mut Ui) {
        kit::panel_header(ui, "ATTRIBUTES", Some("▶"), || {
            self.editor.layout.right_collapsed = true;
        });
        ui.add_space(8.0);

        kit::scroll_body(ui, |ui| {
            ui.spacing_mut().item_spacing.y = kit::FORM_ROW_GAP;
            if let Some(clip_id) = self.editor.selected_clip_id() {
                self.clip_attributes(ui, clip_id);
            } else if let Some(asset_id) = self.editor.selected_asset_id() {
                self.asset_attributes(ui, asset_id);
            } else if let Some(marker_id) = self.editor.selected_marker_id() {
                self.marker_attributes(ui, marker_id);
            } else if let Some(track_id) = self.editor.selected_track_id() {
                self.track_attributes(ui, track_id);
            } else {
                kit::sunken_frame().show(ui, |ui| {
                    kit::empty_state(
                        ui,
                        "Nothing selected",
                        "Select a clip, asset, marker, or track.",
                    );
                });
            }
        });
    }

    fn clip_attributes(&mut self, ui: &mut Ui, clip_id: Uuid) {
        let clip_asset_id = self
            .editor
            .project
            .clips
            .iter()
            .find(|clip| clip.id == clip_id)
            .map(|clip| clip.asset_id);
        let asset_name = self
            .editor
            .project
            .clips
            .iter()
            .find(|clip| clip.id == clip_id)
            .and_then(|clip| self.editor.project.find_asset(clip.asset_id))
            .map(|asset| asset.name.clone())
            .unwrap_or_else(|| "Unknown asset".to_string());
        let mut preview_dirty = false;
        if let Some(clip) = self
            .editor
            .project
            .clips
            .iter_mut()
            .find(|clip| clip.id == clip_id)
        {
            inspector_card(ui, "Clip", |ui| {
                kit::field_label(ui, "Source Asset");
                let source_w = ui.available_width();
                kit::readonly_value_box(ui, asset_name, Vec2::new(source_w, kit::FIELD_H));
                ui.add_space(kit::FORM_ROW_GAP);
                let mut label = clip.label.clone().unwrap_or_default();
                if inspector_text_field(ui, "Clip Label", &mut label) {
                    clip.label = if label.trim().is_empty() {
                        None
                    } else {
                        Some(label)
                    };
                }
            });
            ui.add_space(kit::FORM_ROW_GAP);
            inspector_card(ui, "Transform", |ui| {
                transform_editor(ui, &mut clip.transform, &mut preview_dirty);
            });
            ui.add_space(kit::FORM_ROW_GAP);
            inspector_card(ui, "Timing", |ui| {
                preview_dirty |= inspector_two_drag_f64(
                    ui,
                    ("Start", &mut clip.start_time, 0.05),
                    ("Duration", &mut clip.duration, 0.05),
                );
            });
        }
        if let Some(asset_id) = clip_asset_id {
            if generative_output_for_asset(&self.editor.project, asset_id).is_some() {
                ui.add_space(kit::FORM_ROW_GAP);
                self.generative_clip_attributes(ui, clip_id, asset_id);
            }
        }
        if preview_dirty {
            self.editor.preview_dirty = true;
        }
    }

    fn generative_clip_attributes(&mut self, ui: &mut Ui, clip_id: Uuid, asset_id: Uuid) {
        let Some((folder, output_type)) =
            generative_output_for_asset(&self.editor.project, asset_id)
        else {
            return;
        };
        let config_snapshot = self
            .editor
            .project
            .generative_config(asset_id)
            .cloned()
            .unwrap_or_default();
        let folder_path = self
            .editor
            .project
            .project_path
            .as_ref()
            .map(|root| root.join(&folder));
        let asset_label = self
            .editor
            .project
            .find_asset(asset_id)
            .map(|asset| asset.name.clone())
            .unwrap_or_else(|| "Generative Asset".to_string());

        let compatible_providers: Vec<ProviderEntry> = self
            .editor
            .provider_entries
            .iter()
            .filter(|entry| entry.output_type == output_type)
            .cloned()
            .collect();
        let selected_provider_id = config_snapshot.provider_id;
        let selected_provider = selected_provider_id.and_then(|id| {
            compatible_providers
                .iter()
                .find(|entry| entry.id == id)
                .cloned()
        });
        let show_missing_provider = selected_provider_id.is_some() && selected_provider.is_none();

        let mut version_options: Vec<String> = config_snapshot
            .versions
            .iter()
            .map(|record| record.version.clone())
            .collect();
        if let Some(active) = config_snapshot.active_version.as_ref() {
            if !active.trim().is_empty() && !version_options.contains(active) {
                version_options.push(active.clone());
            }
        }
        version_options.sort_by(
            |a, b| match (parse_version_index(a), parse_version_index(b)) {
                (Some(a_num), Some(b_num)) => b_num.cmp(&a_num),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => b.cmp(a),
            },
        );
        version_options.dedup();

        let selected_version_value = config_snapshot.active_version.clone().unwrap_or_default();
        let provider_label = selected_provider
            .as_ref()
            .map(|provider| provider.name.clone())
            .unwrap_or_else(|| "None selected".to_string());

        let batch = config_snapshot.batch.clone();
        let seed_field_options = selected_provider
            .as_ref()
            .map(seed_field_options_for_provider)
            .unwrap_or_default();
        let seed_field_missing = batch
            .seed_field
            .as_ref()
            .map(|field| !seed_field_options.iter().any(|(name, _)| name == field))
            .unwrap_or(false);
        let resolved_seed_field = selected_provider
            .as_ref()
            .and_then(|provider| resolve_seed_field(provider, batch.seed_field.as_deref()));
        let seed_hint = if seed_field_missing {
            batch
                .seed_field
                .as_ref()
                .map(|field| format!("Seed field '{field}' not found in provider inputs."))
        } else if batch.seed_field.is_none() && selected_provider.is_some() {
            Some(match resolved_seed_field.as_ref() {
                Some(field) => format!("Auto-detect: {field}"),
                None => "Auto-detect: none".to_string(),
            })
        } else {
            None
        };
        let batch_hint = if batch.count > 1 {
            match batch.seed_strategy {
                SeedStrategy::Keep => {
                    Some("Identical inputs can be cached; use Increment or Random.".to_string())
                }
                _ if resolved_seed_field.is_none() => {
                    Some("No numeric seed field detected. Pick one to offset seeds.".to_string())
                }
                _ => None,
            }
        } else {
            None
        };

        let mut next_version = selected_version_value.clone();
        let mut next_provider_id = selected_provider_id;
        let mut next_batch_count = batch.count.max(1).min(MAX_GENERATION_BATCH_COUNT) as i64;
        let mut next_seed_strategy = batch.seed_strategy;
        let mut next_seed_field = batch.seed_field.clone().unwrap_or_default();
        let mut open_versions_folder = false;
        let mut generate_clicked = false;

        inspector_card(ui, "Generative", |ui| {
            kit::field_label(ui, "Version");
            let row_w = ui.available_width();
            let (row_rect, _) =
                ui.allocate_exact_size(Vec2::new(row_w, kit::FIELD_H), Sense::hover());
            let mut row_ui = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(row_rect)
                    .layout(Layout::left_to_right(Align::Center)),
            );
            row_ui.shrink_clip_rect(row_rect);
            row_ui.spacing_mut().item_spacing.x = kit::FIELD_COMPOUND_GAP;
            StripBuilder::new(&mut row_ui)
                .clip(true)
                .size(Size::remainder().at_least(86.0))
                .size(Size::exact(kit::BROWSE_BUTTON_W))
                .horizontal(|mut strip| {
                    strip.cell(|ui| {
                        let selected_text = if next_version.trim().is_empty() {
                            "No versions yet".to_string()
                        } else {
                            next_version.clone()
                        };
                        kit::combo_field(
                            ui,
                            ("gen_version", asset_id),
                            selected_text,
                            ui.available_width(),
                            |ui| {
                                if version_options.is_empty() {
                                    ui.label(kit::caption("No versions yet"));
                                } else {
                                    for version in version_options.iter() {
                                        ui.selectable_value(
                                            &mut next_version,
                                            version.clone(),
                                            version,
                                        );
                                    }
                                }
                            },
                        );
                    });
                    strip.cell(|ui| {
                        let button_w = ui.available_width();
                        if kit::field_button(ui, "Manage", button_w).clicked() {
                            open_versions_folder = true;
                        }
                    });
                });

            ui.add_space(kit::FORM_ROW_GAP);
            kit::field_label(ui, "Provider");
            kit::combo_field(
                ui,
                ("gen_provider", asset_id),
                provider_label,
                ui.available_width(),
                |ui| {
                    ui.selectable_value(&mut next_provider_id, None, "None selected");
                    for provider in compatible_providers.iter() {
                        ui.selectable_value(
                            &mut next_provider_id,
                            Some(provider.id),
                            provider.name.as_str(),
                        );
                    }
                },
            );

            if show_missing_provider {
                ui.add_space(kit::FORM_ROW_GAP);
                ui.label(
                    RichText::new("Selected provider is missing from global providers.")
                        .color(kit::MARKER)
                        .size(11.0),
                );
            } else if compatible_providers.is_empty() {
                ui.add_space(kit::FORM_ROW_GAP);
                ui.label(kit::caption(format!(
                    "No {:?} providers configured.",
                    output_type
                )));
            }

            ui.add_space(kit::ACTION_GAP);
            let generate_w = ui.available_width();
            if kit::primary_button(ui, "Generate", generate_w).clicked() {
                generate_clicked = true;
            }
            if let Some(status) = self.generation_status_for_asset(asset_id) {
                ui.add_space(kit::FORM_ROW_GAP);
                ui.label(kit::caption(status));
            }
        });

        ui.add_space(kit::FORM_ROW_GAP);
        inspector_card(ui, "Batch", |ui| {
            if inspector_drag_i64(
                ui,
                "Count",
                &mut next_batch_count,
                1.0,
                ui.available_width(),
            ) {
                next_batch_count = next_batch_count.clamp(1, MAX_GENERATION_BATCH_COUNT as i64);
            }
            ui.add_space(kit::FORM_ROW_GAP);
            let mut draw_strategy = |ui: &mut Ui| {
                kit::labeled_combo_field(
                    ui,
                    "Seed Strategy",
                    ("seed_strategy", asset_id),
                    seed_strategy_label(next_seed_strategy),
                    |ui| {
                        ui.selectable_value(
                            &mut next_seed_strategy,
                            SeedStrategy::Increment,
                            "Increment",
                        );
                        ui.selectable_value(
                            &mut next_seed_strategy,
                            SeedStrategy::Random,
                            "Random",
                        );
                        ui.selectable_value(&mut next_seed_strategy, SeedStrategy::Keep, "Keep");
                    },
                );
            };
            let mut draw_seed_field = |ui: &mut Ui| {
                let selected_text = if next_seed_field.trim().is_empty() {
                    "Auto-detect".to_string()
                } else {
                    seed_field_options
                        .iter()
                        .find(|(name, _)| name == &next_seed_field)
                        .map(|(_, label)| label.clone())
                        .unwrap_or_else(|| next_seed_field.clone())
                };
                kit::labeled_combo_field(
                    ui,
                    "Seed Field",
                    ("seed_field", asset_id),
                    selected_text,
                    |ui| {
                        ui.selectable_value(&mut next_seed_field, String::new(), "Auto-detect");
                        for (name, label) in seed_field_options.iter() {
                            ui.selectable_value(&mut next_seed_field, name.clone(), label);
                        }
                    },
                );
            };
            if ui.available_width() >= 210.0 {
                ui.columns(2, |columns| {
                    draw_strategy(&mut columns[0]);
                    draw_seed_field(&mut columns[1]);
                });
            } else {
                draw_strategy(ui);
                ui.add_space(kit::FORM_ROW_GAP);
                draw_seed_field(ui);
            }
            if let Some(hint) = seed_hint {
                ui.add_space(kit::FORM_ROW_GAP);
                let color = if seed_field_missing {
                    kit::MARKER
                } else {
                    kit::TEXT_DIM
                };
                ui.label(RichText::new(hint).color(color).size(11.0));
            }
            if let Some(hint) = batch_hint {
                ui.add_space(kit::FORM_ROW_GAP);
                ui.label(RichText::new(hint).color(kit::MARKER).size(11.0));
            }
        });

        ui.add_space(kit::FORM_ROW_GAP);
        let input_updates =
            self.provider_inputs_card(ui, asset_id, selected_provider.clone(), &config_snapshot);

        let mut config_dirty = false;
        let mut preview_dirty = false;
        if next_version != selected_version_value {
            let next_active = if next_version.trim().is_empty() {
                None
            } else {
                Some(next_version.trim().to_string())
            };
            self.editor
                .project
                .update_generative_config(asset_id, |config| {
                    config.active_version = next_active.clone();
                    if let Some(version) = next_active.as_ref() {
                        if let Some(record) = config
                            .versions
                            .iter()
                            .find(|record| record.version == *version)
                        {
                            config.inputs = record.inputs_snapshot.clone();
                            config.provider_id = Some(record.provider_id);
                        }
                    }
                });
            config_dirty = true;
            preview_dirty = true;
        }
        if next_provider_id != selected_provider_id {
            self.editor
                .project
                .set_generative_provider_id(asset_id, next_provider_id);
            config_dirty = true;
        }
        let clamped_batch_count =
            next_batch_count.clamp(1, MAX_GENERATION_BATCH_COUNT as i64) as u32;
        let next_seed_field_opt = if next_seed_field.trim().is_empty() {
            None
        } else {
            Some(next_seed_field.trim().to_string())
        };
        if clamped_batch_count != batch.count
            || next_seed_strategy != batch.seed_strategy
            || next_seed_field_opt != batch.seed_field
        {
            self.editor
                .project
                .update_generative_config(asset_id, |config| {
                    config.batch.count = clamped_batch_count;
                    config.batch.seed_strategy = next_seed_strategy;
                    config.batch.seed_field = next_seed_field_opt;
                });
            config_dirty = true;
        }
        if !input_updates.is_empty() {
            self.editor
                .project
                .update_generative_config(asset_id, |config| {
                    for (name, value) in input_updates {
                        config.inputs.insert(name, InputValue::Literal { value });
                    }
                });
            config_dirty = true;
        }
        if config_dirty {
            if let Err(err) = self.editor.project.save_generative_config(asset_id) {
                self.editor.status = format!("Failed to save generative config: {err}");
            }
        }
        if preview_dirty {
            self.invalidate_asset_visual_cache(asset_id);
            self.editor.preview_dirty = true;
        }

        if open_versions_folder {
            if let Some(folder_path) = folder_path.as_ref() {
                if let Err(err) = open_path_in_file_manager(folder_path) {
                    self.editor.status = err;
                }
            } else {
                self.editor.status = "Project folder is unavailable.".to_string();
            }
        }

        if generate_clicked {
            let config_for_generation = self
                .editor
                .project
                .generative_config(asset_id)
                .cloned()
                .unwrap_or(config_snapshot);
            let Some(provider_id) = config_for_generation.provider_id else {
                self.editor.status = "Select a provider first.".to_string();
                return;
            };
            let Some(provider) = compatible_providers
                .into_iter()
                .find(|provider| provider.id == provider_id)
            else {
                self.editor.status = "Selected provider is unavailable.".to_string();
                return;
            };
            let Some(folder_path) = folder_path else {
                self.editor.status = "Project folder is unavailable.".to_string();
                return;
            };
            match self.enqueue_generation_jobs(
                asset_id,
                clip_id,
                provider,
                config_for_generation,
                folder_path,
                asset_label,
            ) {
                Ok(status) => {
                    self.editor.status = status;
                }
                Err(err) => self.editor.status = err,
            }
        }
    }

    fn provider_inputs_card(
        &mut self,
        ui: &mut Ui,
        asset_id: Uuid,
        selected_provider: Option<ProviderEntry>,
        config_snapshot: &GenerativeConfig,
    ) -> Vec<(String, serde_json::Value)> {
        let mut updates = Vec::new();
        inspector_card(ui, "Provider Inputs", |ui| {
            let Some(provider) = selected_provider else {
                ui.label(kit::caption("Select a provider to configure inputs."));
                return;
            };
            if provider.inputs.is_empty() {
                ui.label(kit::caption("No inputs defined."));
                return;
            }

            for (index, input) in provider.inputs.iter().enumerate() {
                if index > 0 {
                    ui.add_space(kit::FORM_ROW_GAP);
                }
                let label = if input.required {
                    format!("{} *", input.label)
                } else {
                    input.label.clone()
                };
                let current_value = literal_config_input(config_snapshot, &input.name)
                    .or_else(|| input.default.clone());
                match &input.input_type {
                    ProviderInputType::Text => {
                        let mut value = current_value
                            .as_ref()
                            .and_then(input_value_as_string)
                            .unwrap_or_default();
                        let multiline = input.ui.as_ref().map(|ui| ui.multiline).unwrap_or(false);
                        let changed = if multiline {
                            inspector_multiline_text_field(
                                ui,
                                &label,
                                &mut value,
                                kit::MultilineTextFieldOptions::rows(3),
                            )
                        } else {
                            inspector_text_field(ui, &label, &mut value)
                        };
                        if changed {
                            updates.push((input.name.clone(), serde_json::Value::String(value)));
                        }
                    }
                    ProviderInputType::Number => {
                        let mut value = current_value
                            .as_ref()
                            .and_then(input_value_as_f64)
                            .unwrap_or(0.0);
                        let step = input.ui.as_ref().and_then(|ui| ui.step).unwrap_or(0.1);
                        let width = ui.available_width();
                        if inspector_drag_f64(ui, &label, &mut value, step, width) {
                            if let Some(number) = serde_json::Number::from_f64(value) {
                                updates
                                    .push((input.name.clone(), serde_json::Value::Number(number)));
                            }
                        }
                    }
                    ProviderInputType::Integer => {
                        let mut value = current_value
                            .as_ref()
                            .and_then(input_value_as_i64)
                            .unwrap_or(0);
                        let width = ui.available_width();
                        if inspector_drag_i64(ui, &label, &mut value, 1.0, width) {
                            updates.push((
                                input.name.clone(),
                                serde_json::Value::Number(value.into()),
                            ));
                        }
                    }
                    ProviderInputType::Boolean => {
                        let mut value = current_value
                            .as_ref()
                            .and_then(input_value_as_bool)
                            .unwrap_or(false);
                        if inspector_bool_field(ui, &label, &mut value) {
                            updates.push((input.name.clone(), serde_json::Value::Bool(value)));
                        }
                    }
                    ProviderInputType::Enum { options } => {
                        let mut value = current_value
                            .as_ref()
                            .and_then(input_value_as_string)
                            .or_else(|| options.first().cloned())
                            .unwrap_or_default();
                        let before = value.clone();
                        kit::labeled_combo_field(
                            ui,
                            &label,
                            ("provider_input_enum", asset_id, &input.name),
                            empty_dash(&value).to_string(),
                            |ui| {
                                for option in options {
                                    ui.selectable_value(&mut value, option.clone(), option);
                                }
                            },
                        );
                        if value != before {
                            updates.push((input.name.clone(), serde_json::Value::String(value)));
                        }
                    }
                    ProviderInputType::Image
                    | ProviderInputType::Video
                    | ProviderInputType::Audio => {
                        ui.label(kit::caption(format!("{label}: asset inputs not wired yet")));
                    }
                }
            }
        });
        updates
    }

    fn asset_attributes(&mut self, ui: &mut Ui, asset_id: Uuid) {
        let mut add_to_timeline = false;
        let asset_snapshot = self
            .editor
            .project
            .assets
            .iter()
            .find(|asset| asset.id == asset_id)
            .cloned();
        let thumbnail = asset_snapshot
            .as_ref()
            .and_then(|asset| self.asset_thumbnail(ui.ctx(), asset));
        if let Some(asset) = self
            .editor
            .project
            .assets
            .iter_mut()
            .find(|asset| asset.id == asset_id)
        {
            let kind_label = asset_kind_label(&asset.kind).to_string();
            let duration = asset.duration_seconds;
            let source = asset_source_label(asset);
            let active_version = asset.active_version().map(str::to_string);
            inspector_card(ui, "Asset", |ui| {
                let accent = asset_accent(asset);
                ui.horizontal(|ui| {
                    let (thumb_rect, _) =
                        ui.allocate_exact_size(INSPECTOR_THUMBNAIL_SIZE, Sense::hover());
                    paint_asset_thumbnail(ui, thumb_rect, asset, accent, thumbnail);
                    ui.add_space(2.0);
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 3.0;
                        ui.label(kit::caption("Type"));
                        ui.label(kit::value(&kind_label));
                        if let Some(duration) = duration {
                            ui.label(kit::caption(format_duration(duration)));
                        }
                    });
                });
                ui.add_space(kit::FORM_ROW_GAP);
                kit::field_label(ui, "Name");
                let name_w = ui.available_width();
                kit::singleline_text_field(ui, &mut asset.name, name_w);
                ui.add_space(kit::FORM_ROW_GAP);
                if let Some(active_version) = active_version {
                    inspector_meta_row(ui, "Version", active_version);
                }
                if let Some(source) = source {
                    inspector_meta_row(ui, "Source", source);
                }
                ui.add_space(kit::ACTION_GAP);
                let add_w = ui.available_width();
                if kit::secondary_button(ui, "Add to timeline", add_w).clicked() {
                    add_to_timeline = true;
                }
            });
        }
        if add_to_timeline {
            if let Err(err) = self.editor.add_asset_to_timeline(asset_id, None) {
                self.editor.status = err;
            }
        }
    }

    fn marker_attributes(&mut self, ui: &mut Ui, marker_id: Uuid) {
        let mut should_sort = false;
        let mut delete_marker = false;
        let mut marker_changed = false;
        if let Some(marker) = self
            .editor
            .project
            .markers
            .iter_mut()
            .find(|marker| marker.id == marker_id)
        {
            inspector_card(ui, "Marker", |ui| {
                let mut changed = false;
                let time_w = ui.available_width();
                changed |= inspector_drag_f64(ui, "Time", &mut marker.time, 0.05, time_w);
                ui.add_space(kit::FORM_ROW_GAP);
                let mut label = marker.label.clone().unwrap_or_default();
                if inspector_text_field(ui, "Label", &mut label) {
                    marker.label = if label.trim().is_empty() {
                        None
                    } else {
                        Some(label)
                    };
                    marker_changed = true;
                }
                ui.add_space(kit::FORM_ROW_GAP);
                let mut description = marker.description.clone().unwrap_or_default();
                if inspector_multiline_text_field(
                    ui,
                    "Description",
                    &mut description,
                    kit::MultilineTextFieldOptions::rows(3),
                ) {
                    marker.description = if description.trim().is_empty() {
                        None
                    } else {
                        Some(description)
                    };
                    marker_changed = true;
                }
                ui.add_space(kit::FORM_ROW_GAP);
                let mut color = marker
                    .color
                    .as_deref()
                    .and_then(parse_hex_color)
                    .unwrap_or(kit::MARKER);
                if inspector_color_field(ui, "Color", &mut color) {
                    marker.color = Some(color_to_hex(color));
                    marker_changed = true;
                }
                if changed {
                    should_sort = true;
                    marker_changed = true;
                }
                ui.add_space(kit::ACTION_GAP);
                let delete_w = ui.available_width();
                if kit::danger_button(ui, "Delete Marker", delete_w).clicked() {
                    delete_marker = true;
                }
            });
        }
        if delete_marker {
            self.editor.project.remove_marker(marker_id);
            self.editor.selection.clear();
            self.editor.preview_dirty = true;
            return;
        }
        if marker_changed {
            self.editor.preview_dirty = true;
        }
        if should_sort {
            self.editor
                .project
                .markers
                .sort_by(|a, b| a.time.partial_cmp(&b.time).unwrap());
            self.editor.preview_dirty = true;
        }
    }

    fn track_attributes(&mut self, ui: &mut Ui, track_id: Uuid) {
        if let Some(track) = self
            .editor
            .project
            .tracks
            .iter_mut()
            .find(|track| track.id == track_id)
        {
            inspector_card(ui, "Track", |ui| {
                kit::field_label(ui, "Name");
                let name_w = ui.available_width();
                kit::singleline_text_field(ui, &mut track.name, name_w);
                ui.add_space(kit::FORM_ROW_GAP);
                inspector_meta_row(ui, "Type", format!("{:?}", track.track_type));
                if track.track_type != TrackType::Marker {
                    ui.add_space(kit::FORM_ROW_GAP);
                    let volume_w = ui.available_width();
                    let _ = inspector_drag_f32(ui, "Volume", &mut track.volume, 0.01, volume_w);
                }
            });
        }
    }

    fn central_preview(&mut self, root: &mut Ui) {
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(kit::PANEL_SUNKEN))
            .show_inside(root, |ui| {
                let header_h = 30.0;
                let (header_rect, _) = ui
                    .allocate_exact_size(Vec2::new(ui.available_width(), header_h), Sense::hover());
                ui.painter().rect_filled(header_rect, 0.0, kit::CHROME);
                ui.painter().line_segment(
                    [header_rect.left_bottom(), header_rect.right_bottom()],
                    Stroke::new(1.0, kit::BORDER),
                );
                let header_inner = header_rect.shrink2(Vec2::new(14.0, 0.0));
                let title_rect = Rect::from_min_max(
                    header_inner.left_top(),
                    Pos2::new(
                        (header_inner.left() + 180.0).min(header_inner.right()),
                        header_inner.bottom(),
                    ),
                );
                let mut title_ui = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(title_rect)
                        .layout(Layout::left_to_right(Align::Center)),
                );
                title_ui.label(kit::section_label("Preview"));
                let s = &self.editor.project.settings;
                ui.painter().text(
                    header_inner.right_center(),
                    egui::Align2::RIGHT_CENTER,
                    format!("{} x {}", s.width, s.height),
                    FontId::monospace(11.0),
                    kit::TEXT_DIM,
                );
                let available = ui.available_size();
                let preview_height = available.y.max(160.0);
                let (rect, _) =
                    ui.allocate_exact_size(Vec2::new(available.x, preview_height), Sense::hover());
                self.paint_preview(ui, rect.shrink(8.0));
            });
    }

    fn paint_preview(&mut self, ui: &mut Ui, rect: Rect) {
        let painter = ui.painter().with_clip_rect(rect);
        if let (Some(texture), Some(frame)) = (&self.preview_texture, self.preview_frame) {
            let scale = (rect.width() / frame.width as f32)
                .min(rect.height() / frame.height as f32)
                .max(0.01);
            let size = Vec2::new(frame.width as f32 * scale, frame.height as f32 * scale);
            let image_rect = Rect::from_center_size(rect.center(), size);
            painter.image(
                texture.id(),
                image_rect,
                Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                Color32::WHITE,
            );
        } else {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "No preview frame",
                FontId::proportional(14.0),
                kit::TEXT_DIM,
            );
        }

        if self.editor.layout.preview_stats {
            if let Some(stats) = &self.preview_stats {
                let text = format!(
                    "total {:.1}ms\nscan {:.1}ms\ncomp {:.1}ms\nstill {:.1}ms\nhit {}\nmiss {}\nlayers {}",
                    stats.total_ms,
                    stats.collect_ms,
                    stats.composite_ms,
                    stats.still_load_ms,
                    stats.cache_hits,
                    stats.cache_misses,
                    stats.layers,
                );
                let stats_rect = Rect::from_min_size(
                    rect.right_top() + Vec2::new(-188.0, 12.0),
                    Vec2::new(172.0, 106.0),
                );
                ui.painter().rect_filled(
                    stats_rect,
                    6.0,
                    Color32::from_rgba_unmultiplied(13, 14, 16, 220),
                );
                ui.painter().rect_stroke(
                    stats_rect,
                    6.0,
                    Stroke::new(1.0, kit::BORDER_SOFT),
                    egui::StrokeKind::Inside,
                );
                ui.painter().text(
                    stats_rect.min + Vec2::new(10.0, 8.0),
                    egui::Align2::LEFT_TOP,
                    text,
                    FontId::monospace(11.0),
                    kit::TEXT_MUTED,
                );
            }
        }
    }

    fn timeline_panel(&mut self, root: &mut Ui) {
        if self.editor.layout.timeline_collapsed {
            let response = egui::Panel::bottom("timeline")
                .exact_size(TIMELINE_HEADER_H + 12.0)
                .frame(kit::timeline_frame())
                .show_inside(root, |ui| {
                    self.timeline_header(ui, true);
                });
            kit::paint_panel_edge(root, response.response.rect, kit::PanelEdge::Top);
            return;
        }

        let response = egui::Panel::bottom("timeline")
            .resizable(true)
            .default_size(self.editor.layout.timeline_height)
            .size_range(150.0..=420.0)
            .frame(kit::timeline_frame())
            .show_inside(root, |ui| {
                ui.set_min_height(150.0);
                self.timeline_header(ui, false);
                self.paint_timeline(ui);
            });
        self.editor.layout.timeline_height = response.response.rect.height().clamp(150.0, 420.0);
        kit::paint_panel_edge(root, response.response.rect, kit::PanelEdge::Top);
    }

    fn timeline_header(&mut self, ui: &mut Ui, collapsed: bool) {
        let duration = self.editor.project.duration().max(10.0);
        let fps = self.editor.project.settings.fps.max(1.0) as f32;
        let viewport_w = (ui.available_width() - TIMELINE_LABEL_W).max(1.0);
        let (fit_zoom, max_zoom) = timeline_zoom_bounds(duration as f32, viewport_w, fps);
        self.editor.layout.timeline_zoom =
            self.editor.layout.timeline_zoom.clamp(fit_zoom, max_zoom);
        let zoom = self.editor.layout.timeline_zoom;
        let zoom_label = if (zoom - fit_zoom).abs() <= 0.5 {
            "Fit".to_string()
        } else if (zoom - max_zoom).abs() <= 0.5 {
            "Frames".to_string()
        } else {
            format!("{zoom:.0}px/s")
        };
        let timecode_label = timecode(self.editor.current_time);

        let header_w = ui.available_width();
        let (header_rect, _) =
            ui.allocate_exact_size(Vec2::new(header_w, TIMELINE_HEADER_H), Sense::hover());
        let inner_rect = header_rect.shrink2(Vec2::new(TIMELINE_HEADER_PAD_X, 0.0));
        let right_w = timeline_header_right_width(ui, &timecode_label)
            .min(TIMELINE_HEADER_RIGHT_W)
            .min(inner_rect.width() * 0.35);
        let left_w = timeline_header_left_width(ui, collapsed, &zoom_label)
            .min(TIMELINE_HEADER_LEFT_W)
            .min((inner_rect.width() - right_w - TIMELINE_HEADER_CENTER_GAP * 2.0).max(0.0));
        let left_rect = Rect::from_min_max(
            inner_rect.left_top(),
            Pos2::new(inner_rect.left() + left_w, inner_rect.bottom()),
        );
        let right_rect = Rect::from_min_max(
            Pos2::new(inner_rect.right() - right_w, inner_rect.top()),
            inner_rect.right_bottom(),
        );
        let center_left = (left_rect.right() + TIMELINE_HEADER_CENTER_GAP).min(right_rect.left());
        let center_right = (right_rect.left() - TIMELINE_HEADER_CENTER_GAP).max(center_left);
        let center_region = Rect::from_min_max(
            Pos2::new(center_left, inner_rect.top()),
            Pos2::new(center_right, inner_rect.bottom()),
        );
        let transport_gap = 4.0;
        let transport_w = TIMELINE_TRANSPORT_BUTTON_COUNT * kit::TIMELINE_TRANSPORT_BUTTON_W
            + (TIMELINE_TRANSPORT_BUTTON_COUNT - 1.0) * transport_gap;
        let transport_rect =
            centered_child_rect(center_region, transport_w, kit::TIMELINE_TRANSPORT_BUTTON_H);

        let mut left_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(left_rect)
                .layout(Layout::left_to_right(Align::Center)),
        );
        left_ui.shrink_clip_rect(left_rect);
        left_ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            ui.label(kit::section_label("Timeline"));
            if !collapsed {
                ui.add_space(8.0);
                if kit::timeline_tool_icon_button(ui, "−").clicked() {
                    self.set_timeline_zoom_anchored(
                        self.editor.layout.timeline_zoom * 0.8,
                        duration,
                        viewport_w,
                    );
                }
                ui.label(kit::caption(&zoom_label));
                if kit::timeline_tool_icon_button(ui, "+").clicked() {
                    self.set_timeline_zoom_anchored(
                        self.editor.layout.timeline_zoom * 1.25,
                        duration,
                        viewport_w,
                    );
                }
                let fit_active = (zoom - fit_zoom).abs() <= 0.5;
                let frames_active = (zoom - max_zoom).abs() <= 0.5;
                if kit::timeline_tool_text_button(ui, "Fit", 42.0, fit_active).clicked() {
                    self.set_timeline_zoom_anchored(fit_zoom, duration, viewport_w);
                }
                if kit::timeline_tool_text_button(ui, "Frames", 58.0, frames_active).clicked() {
                    self.set_timeline_zoom_anchored(max_zoom, duration, viewport_w);
                }
            }
        });

        let mut transport_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(transport_rect)
                .layout(Layout::left_to_right(Align::Center)),
        );
        transport_ui.shrink_clip_rect(transport_rect);
        transport_ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = transport_gap;
            if kit::timeline_transport_icon_button(ui, kit::TimelineTransportIcon::First, false)
                .clicked()
            {
                self.seek_editor(0.0, false);
            }
            if kit::timeline_transport_icon_button(ui, kit::TimelineTransportIcon::Previous, false)
                .clicked()
            {
                self.seek_editor(
                    previous_frame_time(self.editor.current_time, self.editor.project.settings.fps),
                    false,
                );
            }
            let play_icon = if self.editor.is_playing {
                kit::TimelineTransportIcon::Pause
            } else {
                kit::TimelineTransportIcon::Play
            };
            if kit::timeline_transport_icon_button(ui, play_icon, true).clicked() {
                self.toggle_playback();
            }
            if kit::timeline_transport_icon_button(ui, kit::TimelineTransportIcon::Next, false)
                .clicked()
            {
                self.seek_editor(
                    next_frame_time(
                        self.editor.current_time,
                        self.editor.project.duration(),
                        self.editor.project.settings.fps,
                    ),
                    false,
                );
            }
            if kit::timeline_transport_icon_button(ui, kit::TimelineTransportIcon::Last, false)
                .clicked()
            {
                self.seek_editor(self.editor.project.duration(), false);
            }
        });

        let mut right_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(right_rect)
                .layout(Layout::right_to_left(Align::Center)),
        );
        right_ui.shrink_clip_rect(right_rect);
        right_ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let collapse_icon = if collapsed {
                kit::TimelineTransportIcon::CaretUp
            } else {
                kit::TimelineTransportIcon::CaretDown
            };
            if kit::timeline_transport_icon_button(ui, collapse_icon, false).clicked() {
                self.editor.layout.timeline_collapsed = !collapsed;
            }
            ui.label(
                RichText::new(timecode_label)
                    .monospace()
                    .color(kit::TEXT_MUTED)
                    .size(11.0),
            );
        });
        if !collapsed {
            ui.separator();
        }
    }

    fn paint_timeline(&mut self, ui: &mut Ui) {
        self.paint_timeline_v2(ui);
    }

    fn paint_timeline_v2(&mut self, ui: &mut Ui) {
        let available = ui.available_size();
        let duration = self.editor.project.duration().max(10.0);
        let fps = self.editor.project.settings.fps.max(1.0) as f32;
        let track_count = self.editor.project.tracks.len().max(1) as f32;
        let min_h = TIMELINE_RULER_H
            + track_count * TIMELINE_TRACK_H
            + TIMELINE_ADD_ROW_H
            + TIMELINE_SCROLLBAR_H;
        let total_h = available.y.max(min_h);
        let (outer, response) =
            ui.allocate_exact_size(Vec2::new(available.x, total_h), Sense::click_and_drag());
        let rects = timeline_rects(outer);
        let viewport_w = rects.tracks.width().max(1.0);
        let (fit_zoom, max_zoom) = timeline_zoom_bounds(duration as f32, viewport_w, fps);
        self.editor.layout.timeline_zoom =
            self.editor.layout.timeline_zoom.clamp(fit_zoom, max_zoom);
        let zoom = self
            .editor
            .layout
            .timeline_zoom
            .max(TIMELINE_MIN_ZOOM_FLOOR);
        let content_w = (duration as f32 * zoom).max(viewport_w);
        self.clamp_timeline_scroll(content_w, viewport_w);
        self.handle_timeline_keyboard(ui, duration, viewport_w);
        let content_viewport =
            Rect::from_min_max(rects.ruler.left_top(), rects.tracks.right_bottom());
        self.handle_timeline_shift_scroll(ui, content_viewport, content_w, viewport_w);

        let painter = ui.painter_at(outer);
        let content_clip = Rect::from_min_max(
            rects.ruler.left_top(),
            Pos2::new(rects.outer.right(), rects.add_row.top()),
        );
        let content_painter = painter.with_clip_rect(content_clip);
        painter.rect_filled(outer, 0.0, Color32::from_rgb(12, 13, 15));
        painter.rect_filled(rects.label, 0.0, kit::PANEL);
        painter.rect_filled(rects.ruler, 0.0, kit::CHROME);
        painter.line_segment(
            [
                Pos2::new(rects.tracks.left(), outer.top()),
                Pos2::new(rects.tracks.left(), outer.bottom()),
            ],
            Stroke::new(1.0, kit::BORDER),
        );

        let tracks = self.editor.project.tracks.clone();
        let clips = self.editor.project.clips.clone();
        let markers = self.editor.project.markers.clone();
        let assets_by_id: HashMap<Uuid, Asset> = self
            .editor
            .project
            .assets
            .iter()
            .cloned()
            .map(|asset| (asset.id, asset))
            .collect();
        let dragged_asset_id = egui::DragAndDrop::payload::<AssetTimelineDragPayload>(ui.ctx())
            .map(|payload| payload.asset_id);
        let drop_target_track_id = dragged_asset_id.and_then(|asset_id| {
            ui.ctx().pointer_hover_pos().and_then(|pos| {
                timeline_track_at_pos(pos, rects, &tracks)
                    .filter(|track| {
                        self.editor
                            .project
                            .asset_compatible_with_track(asset_id, track.id)
                    })
                    .map(|track| track.id)
            })
        });

        self.paint_timeline_ruler(&content_painter, rects.ruler, duration, zoom, fps);

        let mut clip_geoms = Vec::new();
        let mut marker_geoms = Vec::new();
        for (row, track) in tracks.iter().enumerate() {
            let row_rect = timeline_row_rect(rects, row);
            let label_rect = Rect::from_min_max(
                Pos2::new(outer.left(), row_rect.top()),
                Pos2::new(rects.tracks.left(), row_rect.bottom()),
            );
            let selected = self.editor.selection.track_ids.contains(&track.id);
            let track_color = track_color(track.track_type);
            painter.rect_filled(
                label_rect,
                0.0,
                if selected {
                    Color32::from_rgb(25, 42, 35)
                } else {
                    kit::PANEL
                },
            );
            content_painter.rect_filled(
                row_rect,
                0.0,
                if row % 2 == 0 {
                    Color32::from_rgb(14, 15, 17)
                } else {
                    Color32::from_rgb(11, 12, 14)
                },
            );
            if drop_target_track_id == Some(track.id) {
                content_painter.rect_filled(row_rect, 0.0, kit::BORDER_FOCUS.gamma_multiply(0.10));
                content_painter.rect_stroke(
                    row_rect.shrink(1.0),
                    3.0,
                    Stroke::new(1.0, kit::BORDER_FOCUS.gamma_multiply(0.85)),
                    egui::StrokeKind::Inside,
                );
            }
            painter.line_segment(
                [
                    Pos2::new(outer.left(), row_rect.bottom()),
                    Pos2::new(rects.tracks.left(), row_rect.bottom()),
                ],
                Stroke::new(1.0, kit::BORDER_SOFT),
            );
            content_painter.line_segment(
                [
                    Pos2::new(rects.tracks.left(), row_rect.bottom()),
                    Pos2::new(rects.tracks.right(), row_rect.bottom()),
                ],
                Stroke::new(1.0, kit::BORDER_SOFT),
            );
            painter.rect_filled(
                Rect::from_min_size(
                    Pos2::new(label_rect.left() + 12.0, row_rect.center().y - 8.0),
                    Vec2::new(3.0, 16.0),
                ),
                1.0,
                track_color,
            );
            painter.text(
                Pos2::new(label_rect.left() + 26.0, row_rect.center().y),
                egui::Align2::LEFT_CENTER,
                &track.name,
                FontId::proportional(12.5),
                kit::TEXT,
            );

            for clip in clips.iter().filter(|clip| clip.track_id == track.id) {
                let clip_rect =
                    timeline_clip_rect(clip, row_rect, zoom, self.editor.layout.timeline_scroll_x);
                clip_geoms.push(TimelineClipGeom {
                    clip_id: clip.id,
                    rect: clip_rect,
                });
                let selected = self.editor.selection.clip_ids.contains(&clip.id);
                let asset = assets_by_id.get(&clip.asset_id);
                let thumbnail_tiles = asset
                    .filter(|asset| asset.is_visual())
                    .map(|asset| {
                        self.timeline_clip_thumbnail_tiles(ui.ctx(), asset, clip, clip_rect, zoom)
                    })
                    .unwrap_or_default();
                let waveform = asset
                    .filter(|asset| asset.is_audio())
                    .and_then(|asset| self.audio_peak_cache(ui.ctx(), asset));
                self.paint_timeline_clip(
                    &content_painter,
                    clip,
                    asset,
                    clip_rect,
                    track_color,
                    selected,
                    &thumbnail_tiles,
                    waveform.as_ref(),
                );
            }

            if track.track_type == TrackType::Marker {
                for marker in markers.iter() {
                    let x = time_to_timeline_x(
                        marker.time,
                        rects.tracks.left(),
                        zoom,
                        self.editor.layout.timeline_scroll_x,
                    );
                    let hit_rect = timeline_marker_hit_rect(marker, row_rect, x);
                    marker_geoms.push(TimelineMarkerGeom {
                        marker_id: marker.id,
                        hit_rect,
                    });
                    self.paint_timeline_marker(&content_painter, marker, row_rect, x);
                }
            }
        }

        self.paint_add_track_row(ui, &painter, rects);
        self.paint_timeline_playhead(&content_painter, rects, duration, zoom);
        if let Some(time) = self.timeline_snap_preview {
            let x = time_to_timeline_x(
                time,
                rects.tracks.left(),
                zoom,
                self.editor.layout.timeline_scroll_x,
            );
            content_painter.line_segment(
                [
                    Pos2::new(x, rects.ruler.top()),
                    Pos2::new(x, rects.add_row.top()),
                ],
                Stroke::new(1.0, Color32::from_rgb(229, 187, 47)),
            );
        }
        self.paint_timeline_scrollbar(ui, &painter, rects, content_w, viewport_w);
        if let Some(payload) = response.dnd_release_payload::<AssetTimelineDragPayload>() {
            if let Some(pos) = ui
                .ctx()
                .pointer_interact_pos()
                .or_else(|| ui.ctx().pointer_hover_pos())
            {
                self.drop_asset_on_timeline(payload.asset_id, pos, rects, &tracks, duration, zoom);
            }
        }
        self.handle_timeline_pointer(
            ui,
            &response,
            rects,
            &tracks,
            &clips,
            &clip_geoms,
            &marker_geoms,
            duration,
            zoom,
        );
    }

    fn drop_asset_on_timeline(
        &mut self,
        asset_id: Uuid,
        pos: Pos2,
        rects: TimelineRects,
        tracks: &[crate::state::Track],
        duration: f64,
        zoom: f32,
    ) {
        let Some(track) = timeline_track_at_pos(pos, rects, tracks) else {
            return;
        };
        if !self
            .editor
            .project
            .asset_compatible_with_track(asset_id, track.id)
        {
            self.editor.status = "Asset cannot be placed on that track".to_string();
            return;
        }

        let raw_time = ((pos.x - rects.tracks.left() + self.editor.layout.timeline_scroll_x) / zoom)
            .clamp(0.0, duration as f32) as f64;
        let time = snap_time_to_frame(raw_time, self.editor.project.settings.fps.max(1.0));
        match self
            .editor
            .add_asset_to_timeline_track(asset_id, track.id, Some(time))
        {
            Ok(_) => {
                self.editor.status = format!("Added clip to {}", track.name);
            }
            Err(err) => {
                self.editor.status = err;
            }
        }
    }

    fn set_timeline_zoom_anchored(&mut self, zoom: f32, duration: f64, viewport_w: f32) {
        let fps = self.editor.project.settings.fps.max(1.0) as f32;
        let (fit_zoom, max_zoom) = timeline_zoom_bounds(duration as f32, viewport_w, fps);
        let next_zoom = zoom.clamp(fit_zoom, max_zoom);
        let old_zoom = self
            .editor
            .layout
            .timeline_zoom
            .max(TIMELINE_MIN_ZOOM_FLOOR);
        if (next_zoom - old_zoom).abs() < f32::EPSILON {
            return;
        }
        let current_time = self.editor.current_time as f32;
        let anchor_x = current_time * old_zoom - self.editor.layout.timeline_scroll_x;
        self.editor.layout.timeline_scroll_x = current_time * next_zoom - anchor_x;
        self.editor.layout.timeline_zoom = next_zoom;
        let content_w = (duration as f32 * next_zoom).max(viewport_w);
        self.clamp_timeline_scroll(content_w, viewport_w);
    }

    fn clamp_timeline_scroll(&mut self, content_w: f32, viewport_w: f32) {
        let max_scroll = (content_w - viewport_w).max(0.0);
        if !self.editor.layout.timeline_scroll_x.is_finite() {
            self.editor.layout.timeline_scroll_x = 0.0;
        }
        self.editor.layout.timeline_scroll_x =
            self.editor.layout.timeline_scroll_x.clamp(0.0, max_scroll);
    }

    fn handle_timeline_keyboard(&mut self, ui: &mut Ui, duration: f64, viewport_w: f32) {
        let zoom_in = ui.input(|input| {
            input.key_pressed(egui::Key::Plus) || input.key_pressed(egui::Key::Equals)
        });
        let zoom_out = ui.input(|input| input.key_pressed(egui::Key::Minus));
        if zoom_in {
            self.set_timeline_zoom_anchored(
                self.editor.layout.timeline_zoom * 1.25,
                duration,
                viewport_w,
            );
        }
        if zoom_out {
            self.set_timeline_zoom_anchored(
                self.editor.layout.timeline_zoom * 0.8,
                duration,
                viewport_w,
            );
        }
    }

    fn handle_timeline_shift_scroll(
        &mut self,
        ui: &mut Ui,
        viewport_rect: Rect,
        content_w: f32,
        viewport_w: f32,
    ) {
        let Some(pointer) = ui.ctx().pointer_hover_pos() else {
            return;
        };
        if !viewport_rect.contains(pointer) {
            return;
        }
        let (shift, smooth_delta, wheel_delta) = ui.input(|input| {
            let wheel_delta = input
                .events
                .iter()
                .filter_map(|event| match event {
                    egui::Event::MouseWheel {
                        delta, modifiers, ..
                    } if modifiers.shift => Some(*delta),
                    _ => None,
                })
                .fold(Vec2::ZERO, |sum, delta| sum + delta);
            (
                input.modifiers.shift,
                input.smooth_scroll_delta,
                wheel_delta,
            )
        });
        let content_delta = if wheel_delta.x.abs() > 0.0 {
            wheel_delta.x
        } else if wheel_delta.y.abs() > 0.0 {
            wheel_delta.y
        } else if smooth_delta.x.abs() > 0.0 {
            smooth_delta.x
        } else if shift && smooth_delta.y.abs() > 0.0 {
            smooth_delta.y
        } else {
            0.0
        };
        if content_delta.abs() > 0.0 {
            self.editor.layout.timeline_scroll_x -= content_delta;
            self.clamp_timeline_scroll(content_w, viewport_w);
            ui.ctx().request_repaint();
        }
    }

    fn paint_timeline_ruler(
        &self,
        painter: &egui::Painter,
        rect: Rect,
        duration: f64,
        zoom: f32,
        fps: f32,
    ) {
        let scroll_x = self.editor.layout.timeline_scroll_x;
        let visible_start = (scroll_x / zoom).max(0.0) as f64;
        let visible_end = ((scroll_x + rect.width()) / zoom).min(duration as f32) as f64;
        let target_seconds = (90.0 / zoom.max(0.1)).max(0.5) as f64;
        let major_step = nice_timeline_step(target_seconds);
        let first_tick = (visible_start / major_step).floor() as i32 - 1;
        let last_tick = (visible_end / major_step).ceil() as i32 + 1;

        if zoom >= 240.0 {
            let fps = fps.max(1.0);
            let first_frame = (visible_start * fps as f64).floor() as i64 - 1;
            let last_frame = (visible_end * fps as f64).ceil() as i64 + 1;
            let fps_i = fps.round().max(1.0) as i64;
            for frame in first_frame..=last_frame {
                if frame < 0 || frame % fps_i == 0 {
                    continue;
                }
                let t = frame as f64 / fps as f64;
                let x = time_to_timeline_x(t, rect.left(), zoom, scroll_x);
                if rect.x_range().contains(x) {
                    painter.line_segment(
                        [
                            Pos2::new(x, rect.bottom() - 4.0),
                            Pos2::new(x, rect.bottom()),
                        ],
                        Stroke::new(1.0, kit::BORDER_SOFT),
                    );
                }
            }
        }

        for tick in first_tick..=last_tick {
            if tick < 0 {
                continue;
            }
            let t = tick as f64 * major_step;
            if t > duration {
                continue;
            }
            let x = time_to_timeline_x(t, rect.left(), zoom, scroll_x);
            if x < rect.left() - 80.0 || x > rect.right() + 8.0 {
                continue;
            }
            painter.line_segment(
                [
                    Pos2::new(x, rect.bottom() - 10.0),
                    Pos2::new(x, rect.bottom()),
                ],
                Stroke::new(1.0, Color32::from_rgb(52, 55, 62)),
            );
            painter.line_segment(
                [
                    Pos2::new(x, rect.bottom()),
                    Pos2::new(x, rect.bottom() + rect.height() * 12.0),
                ],
                Stroke::new(1.0, Color32::from_rgb(25, 27, 31)),
            );
            painter.text(
                Pos2::new(x + 4.0, rect.top() + 4.0),
                egui::Align2::LEFT_TOP,
                timeline_ruler_label(t),
                FontId::monospace(9.0),
                kit::TEXT_DIM,
            );
        }
    }

    fn paint_timeline_clip(
        &self,
        painter: &egui::Painter,
        clip: &Clip,
        asset: Option<&Asset>,
        rect: Rect,
        accent: Color32,
        selected: bool,
        thumbnail_tiles: &[TimelineThumbTile],
        waveform: Option<&PeakCache>,
    ) {
        let fill = if selected {
            Color32::from_rgb(18, 50, 36)
        } else {
            Color32::from_rgb(23, 25, 29)
        };
        let type_stroke = if selected {
            accent
        } else {
            accent.gamma_multiply(0.58)
        };
        let selection_stroke = if selected {
            kit::BORDER_FOCUS
        } else {
            type_stroke
        };
        painter.rect_filled(rect, 4.0, fill);
        if !thumbnail_tiles.is_empty() {
            paint_clip_thumbnail_strip(painter, rect, thumbnail_tiles);
            if selected {
                painter.rect_filled(rect, 4.0, Color32::from_rgba_unmultiplied(20, 90, 54, 44));
            }
        }
        if let Some(cache) = waveform {
            paint_clip_waveform(painter, rect.shrink2(Vec2::new(2.0, 4.0)), clip, cache);
        }
        painter.rect_stroke(
            rect,
            4.0,
            Stroke::new(if selected { 2.0 } else { 1.0 }, selection_stroke),
            egui::StrokeKind::Inside,
        );
        painter.rect_filled(
            Rect::from_min_size(rect.left_top(), Vec2::new(4.0, rect.height())),
            2.0,
            type_stroke,
        );
        let label = timeline_clip_title(clip, asset);
        painter.text(
            rect.left_center() + Vec2::new(8.0, -6.5),
            egui::Align2::LEFT_TOP,
            label,
            FontId::proportional(10.5),
            kit::TEXT_ON_ACCENT,
        );
    }

    fn paint_timeline_marker(
        &self,
        painter: &egui::Painter,
        marker: &crate::state::Marker,
        row_rect: Rect,
        x: f32,
    ) {
        let selected = self.editor.selection.marker_ids.contains(&marker.id);
        let color = marker
            .color
            .as_deref()
            .and_then(parse_hex_color)
            .unwrap_or(kit::MARKER);
        let marker_color = if selected {
            color
        } else {
            color.gamma_multiply(0.62)
        };
        painter.line_segment(
            [
                Pos2::new(x, row_rect.top() + 4.0),
                Pos2::new(x, row_rect.bottom() - 4.0),
            ],
            Stroke::new(if selected { 2.0 } else { 1.25 }, marker_color),
        );
        let points = [
            Pos2::new(x - 4.5, row_rect.bottom() - 1.0),
            Pos2::new(x + 4.5, row_rect.bottom() - 1.0),
            Pos2::new(x, row_rect.bottom() - 8.0),
        ];
        painter.add(egui::Shape::convex_polygon(
            points.to_vec(),
            marker_color,
            Stroke::NONE,
        ));
        if let Some((label, label_rect)) = marker_label_and_rect(marker, row_rect, x) {
            painter.rect_filled(
                label_rect,
                5.0,
                if selected {
                    Color32::from_rgb(18, 50, 36)
                } else {
                    kit::PANEL_RAISED
                },
            );
            painter.rect_stroke(
                label_rect,
                5.0,
                Stroke::new(
                    if selected { 2.0 } else { 1.0 },
                    if selected {
                        kit::BORDER_FOCUS
                    } else {
                        marker_color
                    },
                ),
                egui::StrokeKind::Inside,
            );
            painter.text(
                label_rect.center(),
                egui::Align2::CENTER_CENTER,
                label,
                FontId::proportional(10.0),
                kit::TEXT,
            );
        }
    }

    fn paint_timeline_playhead(
        &self,
        painter: &egui::Painter,
        rects: TimelineRects,
        duration: f64,
        zoom: f32,
    ) {
        let t = snap_time_to_frame(self.editor.current_time, self.editor.project.settings.fps)
            .clamp(0.0, duration);
        let x = time_to_timeline_x(
            t,
            rects.tracks.left(),
            zoom,
            self.editor.layout.timeline_scroll_x,
        );
        painter.line_segment(
            [
                Pos2::new(x, rects.ruler.top()),
                Pos2::new(x, rects.add_row.top()),
            ],
            Stroke::new(1.5, kit::PLAYHEAD),
        );
        let head = [
            Pos2::new(x - 6.0, rects.ruler.top()),
            Pos2::new(x + 6.0, rects.ruler.top()),
            Pos2::new(x, rects.ruler.top() + 8.0),
        ];
        painter.add(egui::Shape::convex_polygon(
            head.to_vec(),
            kit::PLAYHEAD,
            Stroke::NONE,
        ));
    }

    fn paint_add_track_row(&mut self, ui: &mut Ui, painter: &egui::Painter, rects: TimelineRects) {
        painter.rect_filled(rects.add_row, 0.0, kit::PANEL);
        painter.line_segment(
            [
                Pos2::new(rects.outer.left(), rects.add_row.top()),
                Pos2::new(rects.outer.right(), rects.add_row.top()),
            ],
            Stroke::new(1.0, kit::BORDER_SOFT),
        );
        let video_rect = Rect::from_min_size(
            rects.add_row.left_top() + Vec2::new(12.0, 10.0),
            Vec2::new(56.0, 24.0),
        );
        let audio_rect = Rect::from_min_size(
            rects.add_row.left_top() + Vec2::new(72.0, 10.0),
            Vec2::new(56.0, 24.0),
        );
        let video_resp = ui.interact(
            video_rect,
            ui.id().with("timeline-add-video"),
            Sense::click(),
        );
        let audio_resp = ui.interact(
            audio_rect,
            ui.id().with("timeline-add-audio"),
            Sense::click(),
        );
        if video_resp.clicked() {
            self.editor.project.add_video_track();
        }
        if audio_resp.clicked() {
            self.editor.project.add_audio_track();
        }
        paint_dashed_timeline_button(
            painter,
            video_rect,
            "+ Video",
            kit::VIDEO,
            video_resp.hovered(),
        );
        paint_dashed_timeline_button(
            painter,
            audio_rect,
            "+ Audio",
            kit::AUDIO,
            audio_resp.hovered(),
        );
    }

    fn paint_timeline_scrollbar(
        &mut self,
        ui: &mut Ui,
        painter: &egui::Painter,
        rects: TimelineRects,
        content_w: f32,
        viewport_w: f32,
    ) {
        if content_w <= viewport_w + 1.0 {
            return;
        }
        let max_scroll = (content_w - viewport_w).max(0.0);
        let handle_w =
            (viewport_w / content_w * rects.scrollbar.width()).clamp(42.0, rects.scrollbar.width());
        let handle_x = rects.scrollbar.left()
            + (self.editor.layout.timeline_scroll_x / max_scroll)
                * (rects.scrollbar.width() - handle_w);
        let handle = Rect::from_min_size(
            Pos2::new(handle_x, rects.scrollbar.center().y - 3.0),
            Vec2::new(handle_w, 6.0),
        );
        painter.rect_filled(
            rects.scrollbar.shrink2(Vec2::new(0.0, 4.0)),
            3.0,
            kit::FIELD_BG,
        );
        painter.rect_filled(handle, 3.0, kit::BORDER);
        let response = ui.interact(
            rects.scrollbar,
            ui.id().with("timeline-scrollbar"),
            Sense::click_and_drag(),
        );
        if (response.dragged() || response.clicked()) && response.interact_pointer_pos().is_some() {
            let pos = response.interact_pointer_pos().unwrap();
            let ratio = ((pos.x - rects.scrollbar.left() - handle_w * 0.5)
                / (rects.scrollbar.width() - handle_w).max(1.0))
            .clamp(0.0, 1.0);
            self.editor.layout.timeline_scroll_x = ratio * max_scroll;
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn handle_timeline_pointer(
        &mut self,
        ui: &mut Ui,
        response: &egui::Response,
        rects: TimelineRects,
        tracks: &[crate::state::Track],
        clips: &[Clip],
        clip_geoms: &[TimelineClipGeom],
        marker_geoms: &[TimelineMarkerGeom],
        duration: f64,
        zoom: f32,
    ) {
        if let Some(pos) = ui
            .ctx()
            .pointer_hover_pos()
            .filter(|pos| rects.outer.contains(*pos))
        {
            let cursor = if let Some(payload) =
                egui::DragAndDrop::payload::<AssetTimelineDragPayload>(ui.ctx())
            {
                timeline_track_at_pos(pos, rects, tracks)
                    .map(|track| {
                        if self
                            .editor
                            .project
                            .asset_compatible_with_track(payload.asset_id, track.id)
                        {
                            egui::CursorIcon::Copy
                        } else {
                            egui::CursorIcon::NoDrop
                        }
                    })
                    .unwrap_or(egui::CursorIcon::NoDrop)
            } else {
                match self.timeline_drag {
                    Some(TimelineDrag::ClipResizeLeft { .. })
                    | Some(TimelineDrag::ClipResizeRight { .. })
                    | Some(TimelineDrag::MarkerMove { .. })
                    | Some(TimelineDrag::Playhead) => egui::CursorIcon::ResizeHorizontal,
                    Some(TimelineDrag::ClipMove { .. }) => egui::CursorIcon::Grabbing,
                    None => match timeline_hit(pos, rects, tracks, clip_geoms, marker_geoms) {
                        TimelineHit::ClipLeftEdge(_)
                        | TimelineHit::ClipRightEdge(_)
                        | TimelineHit::Marker(_)
                        | TimelineHit::Ruler => egui::CursorIcon::ResizeHorizontal,
                        TimelineHit::ClipBody(_) => egui::CursorIcon::Grab,
                        TimelineHit::TrackLabel(_) => egui::CursorIcon::PointingHand,
                        TimelineHit::EmptyTrack | TimelineHit::Empty => egui::CursorIcon::Default,
                    },
                }
            };
            ui.ctx().set_cursor_icon(cursor);
        }

        if response.drag_started() {
            if let Some(pos) = response.interact_pointer_pos() {
                match timeline_hit(pos, rects, tracks, clip_geoms, marker_geoms) {
                    TimelineHit::Ruler => {
                        self.timeline_scrub_was_playing = self.editor.is_playing;
                        if self.editor.is_playing {
                            self.editor.is_playing = false;
                        }
                        self.timeline_drag = Some(TimelineDrag::Playhead);
                        self.seek_from_timeline_pos(
                            pos,
                            rects,
                            duration,
                            zoom,
                            true,
                            timeline_snapping_enabled(ui),
                        );
                    }
                    TimelineHit::ClipLeftEdge(id) => {
                        if let Some(clip) = clips.iter().find(|clip| clip.id == id) {
                            self.editor.selection.select_clip(id);
                            self.timeline_drag = Some(TimelineDrag::ClipResizeLeft {
                                clip_id: id,
                                start_time: clip.start_time,
                                duration: clip.duration,
                            });
                        }
                    }
                    TimelineHit::ClipRightEdge(id) => {
                        if let Some(clip) = clips.iter().find(|clip| clip.id == id) {
                            self.editor.selection.select_clip(id);
                            self.timeline_drag = Some(TimelineDrag::ClipResizeRight {
                                clip_id: id,
                                start_time: clip.start_time,
                                duration: clip.duration,
                            });
                        }
                    }
                    TimelineHit::ClipBody(id) => {
                        if let Some(clip) = clips.iter().find(|clip| clip.id == id) {
                            self.editor.selection.select_clip(id);
                            self.timeline_drag = Some(TimelineDrag::ClipMove {
                                clip_id: id,
                                start_time: clip.start_time,
                                duration: clip.duration,
                            });
                        }
                    }
                    TimelineHit::Marker(id) => {
                        if let Some(marker) = self
                            .editor
                            .project
                            .markers
                            .iter()
                            .find(|marker| marker.id == id)
                        {
                            self.editor.selection.select_marker(id);
                            self.timeline_drag = Some(TimelineDrag::MarkerMove {
                                marker_id: id,
                                start_time: marker.time,
                            });
                        }
                    }
                    TimelineHit::TrackLabel(id) => self.editor.selection.select_track(id),
                    TimelineHit::EmptyTrack | TimelineHit::Empty => {}
                }
            }
        }

        if response.dragged() {
            if let Some(pos) = response.interact_pointer_pos() {
                let drag_delta_x = response
                    .total_drag_delta()
                    .map(|delta| delta.x)
                    .unwrap_or_else(|| response.drag_delta().x);
                self.apply_timeline_drag(
                    drag_delta_x,
                    pos,
                    rects,
                    duration,
                    zoom,
                    timeline_snapping_enabled(ui),
                );
            }
        }

        let primary_down = ui.input(|input| input.pointer.primary_down());
        if !primary_down && self.timeline_drag.is_some() {
            let was_playhead_drag = matches!(self.timeline_drag, Some(TimelineDrag::Playhead));
            self.timeline_drag = None;
            self.timeline_snap_preview = None;
            if was_playhead_drag {
                self.finish_timeline_scrub();
            }
        }

        if response.clicked() {
            if let Some(pos) = response.interact_pointer_pos() {
                match timeline_hit(pos, rects, tracks, clip_geoms, marker_geoms) {
                    TimelineHit::Ruler => self.seek_from_timeline_pos(
                        pos,
                        rects,
                        duration,
                        zoom,
                        false,
                        timeline_snapping_enabled(ui),
                    ),
                    TimelineHit::ClipLeftEdge(id)
                    | TimelineHit::ClipRightEdge(id)
                    | TimelineHit::ClipBody(id) => self.editor.selection.select_clip(id),
                    TimelineHit::Marker(id) => self.editor.selection.select_marker(id),
                    TimelineHit::TrackLabel(id) => self.editor.selection.select_track(id),
                    TimelineHit::EmptyTrack => self.editor.selection.clear(),
                    TimelineHit::Empty => {}
                }
            }
        }
    }

    fn seek_from_timeline_pos(
        &mut self,
        pos: Pos2,
        rects: TimelineRects,
        duration: f64,
        zoom: f32,
        scrub_audio: bool,
        snap_enabled: bool,
    ) {
        let raw_time = ((pos.x - rects.tracks.left() + self.editor.layout.timeline_scroll_x) / zoom)
            .clamp(0.0, duration as f32) as f64;
        let fps = self.editor.project.settings.fps.max(1.0);
        let raw_frames = frames_from_seconds(raw_time, fps);
        let snap_threshold_frames =
            (TIMELINE_SNAP_THRESHOLD_PX / zoom.max(TIMELINE_MIN_ZOOM_FLOOR) as f64) * fps;
        let seek_frames = if snap_enabled {
            let targets = self.timeline_snap_targets(None, None, false);
            if let Some(hit) =
                best_snap_delta_frames(&[raw_frames], &targets, snap_threshold_frames)
            {
                self.timeline_snap_preview = if scrub_audio {
                    Some(seconds_from_frames(hit.target.frame, fps))
                } else {
                    None
                };
                raw_frames + hit.delta_frames
            } else {
                self.timeline_snap_preview = None;
                raw_frames
            }
        } else {
            self.timeline_snap_preview = None;
            raw_frames
        };
        let max_frames = frames_from_seconds(duration, fps).round();
        let time = seconds_from_frames(seek_frames.round().clamp(0.0, max_frames), fps);
        self.seek_editor(time, scrub_audio);
    }

    fn apply_timeline_drag(
        &mut self,
        delta_x: f32,
        pos: Pos2,
        rects: TimelineRects,
        duration: f64,
        zoom: f32,
        snap_enabled: bool,
    ) {
        let Some(drag) = self.timeline_drag else {
            return;
        };
        let fps = self.editor.project.settings.fps.max(1.0);
        let delta_frames = (delta_x as f64 / zoom.max(TIMELINE_MIN_ZOOM_FLOOR) as f64) * fps;
        let min_duration_frames = (0.1 * fps).ceil().max(1.0);
        let snap_threshold_frames = (TIMELINE_SNAP_THRESHOLD_PX / zoom as f64) * fps;
        match drag {
            TimelineDrag::Playhead => {
                self.seek_from_timeline_pos(pos, rects, duration, zoom, true, snap_enabled)
            }
            TimelineDrag::ClipMove {
                clip_id,
                start_time,
                duration: clip_duration,
            } => {
                let start_frames = frames_from_seconds(start_time, fps).round();
                let duration_frames = frames_from_seconds(clip_duration, fps).round();
                let mut new_start_frames = start_frames + delta_frames;
                if snap_enabled {
                    let targets = self.timeline_snap_targets(Some(clip_id), None, true);
                    if let Some(hit) = best_snap_delta_frames(
                        &[new_start_frames, new_start_frames + duration_frames],
                        &targets,
                        snap_threshold_frames,
                    ) {
                        new_start_frames += hit.delta_frames;
                        self.timeline_snap_preview =
                            Some(seconds_from_frames(hit.target.frame, fps));
                    } else {
                        self.timeline_snap_preview = None;
                    }
                } else {
                    self.timeline_snap_preview = None;
                }
                let new_start = seconds_from_frames(new_start_frames.round().max(0.0), fps);
                if self.editor.project.move_clip(clip_id, new_start) {
                    self.editor.preview_dirty = true;
                }
            }
            TimelineDrag::ClipResizeLeft {
                clip_id,
                start_time,
                duration: clip_duration,
            } => {
                let end_frames = frames_from_seconds(start_time + clip_duration, fps).round();
                let mut new_start_frames =
                    frames_from_seconds(start_time, fps).round() + delta_frames;
                if snap_enabled {
                    let targets = self.timeline_snap_targets(Some(clip_id), None, true);
                    if let Some(hit) =
                        best_snap_delta_frames(&[new_start_frames], &targets, snap_threshold_frames)
                    {
                        new_start_frames += hit.delta_frames;
                        self.timeline_snap_preview =
                            Some(seconds_from_frames(hit.target.frame, fps));
                    } else {
                        self.timeline_snap_preview = None;
                    }
                } else {
                    self.timeline_snap_preview = None;
                }
                new_start_frames = new_start_frames.clamp(0.0, end_frames - min_duration_frames);
                let new_duration_frames = (end_frames - new_start_frames).max(min_duration_frames);
                let new_start = seconds_from_frames(new_start_frames.round(), fps);
                let new_duration = seconds_from_frames(new_duration_frames.round(), fps);
                if self
                    .editor
                    .project
                    .resize_clip(clip_id, new_start, new_duration)
                {
                    self.editor.preview_dirty = true;
                }
            }
            TimelineDrag::ClipResizeRight {
                clip_id,
                start_time,
                duration: clip_duration,
            } => {
                let start_frames = frames_from_seconds(start_time, fps).round();
                let mut new_end_frames =
                    start_frames + frames_from_seconds(clip_duration, fps).round() + delta_frames;
                if snap_enabled {
                    let targets = self.timeline_snap_targets(Some(clip_id), None, true);
                    if let Some(hit) =
                        best_snap_delta_frames(&[new_end_frames], &targets, snap_threshold_frames)
                    {
                        new_end_frames += hit.delta_frames;
                        self.timeline_snap_preview =
                            Some(seconds_from_frames(hit.target.frame, fps));
                    } else {
                        self.timeline_snap_preview = None;
                    }
                } else {
                    self.timeline_snap_preview = None;
                }
                let new_duration_frames = (new_end_frames - start_frames).max(min_duration_frames);
                let new_duration = seconds_from_frames(new_duration_frames.round(), fps);
                if self
                    .editor
                    .project
                    .resize_clip(clip_id, start_time, new_duration)
                {
                    self.editor.preview_dirty = true;
                }
            }
            TimelineDrag::MarkerMove {
                marker_id,
                start_time,
            } => {
                let mut new_frames = frames_from_seconds(start_time, fps).round() + delta_frames;
                if snap_enabled {
                    let targets = self.timeline_snap_targets(None, Some(marker_id), true);
                    if let Some(hit) =
                        best_snap_delta_frames(&[new_frames], &targets, snap_threshold_frames)
                    {
                        new_frames += hit.delta_frames;
                        self.timeline_snap_preview =
                            Some(seconds_from_frames(hit.target.frame, fps));
                    } else {
                        self.timeline_snap_preview = None;
                    }
                } else {
                    self.timeline_snap_preview = None;
                }
                let max_frames = frames_from_seconds(duration, fps).round();
                let new_time = seconds_from_frames(new_frames.round().clamp(0.0, max_frames), fps);
                if self.editor.project.move_marker(marker_id, new_time) {
                    self.editor.preview_dirty = true;
                }
            }
        }
    }

    fn timeline_snap_targets(
        &self,
        exclude_clip: Option<Uuid>,
        exclude_marker: Option<Uuid>,
        include_playhead: bool,
    ) -> Vec<SnapTarget> {
        let fps = self.editor.project.settings.fps.max(1.0);
        let mut targets = Vec::new();
        if include_playhead {
            targets.push(SnapTarget::playhead(frames_from_seconds(
                self.editor.current_time,
                fps,
            )));
        }
        for clip in self.editor.project.clips.iter() {
            if Some(clip.id) == exclude_clip {
                continue;
            }
            targets.push(SnapTarget::clip_edge(
                frames_from_seconds(clip.start_time, fps).round(),
                clip.id,
            ));
            targets.push(SnapTarget::clip_edge(
                frames_from_seconds(clip.end_time(), fps).round(),
                clip.id,
            ));
        }
        for marker in self.editor.project.markers.iter() {
            if Some(marker.id) == exclude_marker {
                continue;
            }
            targets.push(SnapTarget::marker(
                frames_from_seconds(marker.time, fps).round(),
                marker.id,
            ));
        }
        targets
    }

    fn audio_peak_cache(&mut self, ctx: &Context, asset: &Asset) -> Option<PeakCache> {
        if let Some(cache) = self.audio_peak_caches.get(&asset.id) {
            return Some(cache.clone());
        }
        let project_root = self.editor.project_root()?.to_path_buf();
        let source = resolve_audio_source(&project_root, asset)?;
        let cache_path = peak_cache_path(&project_root, asset.id);
        if cache_path.exists() {
            if let Ok(cache) = load_peak_cache(&cache_path) {
                if cache_matches_source(&cache, &source).unwrap_or(false) {
                    self.audio_peak_caches.insert(asset.id, cache.clone());
                    return Some(cache);
                }
            }
        }
        if self.audio_peak_builds.insert(asset.id) {
            let ctx = ctx.clone();
            let asset_id = asset.id;
            std::thread::spawn(move || {
                let _ = build_and_store_peak_cache(
                    &project_root,
                    asset_id,
                    &source,
                    PeakBuildConfig::default(),
                );
                ctx.request_repaint();
            });
        }
        None
    }

    fn modals(&mut self, ctx: &Context) {
        let startup_open = self.editor.show_startup();
        if startup_open {
            self.startup_modal(ctx);
        }
        if self.editor.overlays.new_project {
            self.new_project_modal(ctx, false);
        }
        if self.editor.overlays.project_settings {
            self.project_settings_modal(ctx);
        }
        if self.editor.overlays.generative_video {
            self.generative_video_modal(ctx);
        }
        if self.editor.overlays.queue {
            self.queue_panel(ctx);
        }
        if self.editor.overlays.providers {
            self.providers_modal(ctx);
        }
        if self.provider_json_editor_path.is_some() {
            self.provider_json_editor_modal(ctx);
        }
        if self.provider_builder_open {
            self.provider_builder_modal(ctx);
        }
    }

    fn startup_modal(&mut self, ctx: &Context) {
        let wizard_size = project_wizard_size(ctx);
        kit::modal_scrim(ctx, "startup");
        egui::Window::new("startup")
            .title_bar(false)
            .order(egui::Order::Foreground)
            .collapsible(false)
            .resizable(false)
            .fixed_size(wizard_size)
            .frame(kit::modal_frame())
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                kit::modal_header(
                    ui,
                    "NLA AI Video Creator",
                    Some("Create a new project or open an existing one"),
                );
                kit::modal_body(ui, |ui| self.new_project_modal_contents(ui, true));
            });
    }

    fn new_project_modal(&mut self, ctx: &Context, startup: bool) {
        let mut open = true;
        let close_enabled = !startup && self.editor.project_root().is_some();
        let mut close_clicked = false;
        let wizard_size = project_wizard_size(ctx);
        kit::modal_scrim(ctx, "new_project");
        egui::Window::new(if startup {
            "Create Project"
        } else {
            "New Project"
        })
        .title_bar(false)
        .order(egui::Order::Foreground)
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .fixed_size(wizard_size)
        .frame(kit::modal_frame())
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            close_clicked = kit::modal_header_with_close(
                ui,
                "New Project",
                Some("Choose project settings and save location."),
                close_enabled,
            );
            kit::modal_body(ui, |ui| self.new_project_modal_contents(ui, startup));
        });
        if close_clicked || (!open && close_enabled) {
            self.editor.overlays.new_project = false;
        }
    }

    fn new_project_modal_contents(&mut self, ui: &mut Ui, _startup: bool) {
        let gap = 10.0;
        let available_w = ui.available_width();
        let card_h = ui.available_height().min(PROJECT_WIZARD_CARD_H).max(360.0);
        let left_w = ((available_w - gap) * 2.0 / 3.0).max(360.0);
        let right_w = (available_w - gap - left_w).max(180.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            ui.allocate_ui_with_layout(
                Vec2::new(left_w, card_h),
                Layout::top_down(Align::Min),
                |ui| {
                    ui.set_width(left_w);
                    kit::card_panel(ui, card_h, |ui| self.new_project_create_card(ui));
                },
            );
            ui.allocate_ui_with_layout(
                Vec2::new(right_w, card_h),
                Layout::top_down(Align::Min),
                |ui| {
                    ui.set_width(right_w);
                    kit::card_panel(ui, card_h, |ui| {
                        kit::field_label(ui, "Recent Projects");
                        let recent = recent_projects(&self.new_project_parent);
                        let mut selected_project: Option<PathBuf> = None;
                        let mut browse_clicked = false;
                        kit::body_with_footer(
                            ui,
                            120.0,
                            kit::SECONDARY_BUTTON_H,
                            |ui| {
                                ui.add_space(kit::FORM_ROW_GAP);
                                kit::scroll_body(ui, |ui| {
                                    ui.spacing_mut().item_spacing.y = kit::FORM_ROW_GAP;
                                    if recent.is_empty() {
                                        kit::empty_state(
                                            ui,
                                            "No recent projects",
                                            "Browse to open an existing project folder.",
                                        );
                                    }
                                    for folder in recent {
                                        if kit::secondary_button(
                                            ui,
                                            folder
                                                .file_name()
                                                .and_then(|v| v.to_str())
                                                .unwrap_or("Project"),
                                            ui.available_width(),
                                        )
                                        .clicked()
                                        {
                                            selected_project = Some(folder);
                                        }
                                    }
                                });
                            },
                            |ui| {
                                if kit::secondary_button(
                                    ui,
                                    "Browse for Project...",
                                    ui.available_width(),
                                )
                                .clicked()
                                {
                                    browse_clicked = true;
                                }
                            },
                        );
                        if let Some(folder) = selected_project {
                            if self.open_project_folder(folder) {
                                self.editor.overlays.new_project = false;
                            }
                        } else if browse_clicked {
                            let initial_dir = self.new_project_parent.clone();
                            let options = kit::BrowsePathOptions::new()
                                .id_salt("new_project_open_existing")
                                .initial_dir(initial_dir.as_path())
                                .remember_last_dir();
                            if let Some(folder) = kit::pick_folder_dialog(ui, options) {
                                if self.open_project_folder(folder) {
                                    self.editor.overlays.new_project = false;
                                }
                            }
                        }
                    });
                },
            );
        });
    }

    fn new_project_create_card(&mut self, ui: &mut Ui) {
        let footer_h =
            kit::labeled_field_height(kit::VALUE_FIELD_H) + kit::ACTION_GAP + kit::PRIMARY_BUTTON_H;
        let new_project_name = &mut self.new_project_name;
        let project_settings = &mut self.project_settings;
        let new_project_parent = &mut self.new_project_parent;
        let mut create_clicked = false;

        kit::body_with_footer(
            ui,
            180.0,
            footer_h,
            |ui| {
                kit::scroll_body(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = kit::FORM_ROW_GAP;
                    kit::field_label(ui, "Create New Project");
                    ui.add_space(kit::FORM_ROW_GAP);
                    kit::labeled_text_field(ui, "Project Name", new_project_name);
                    ui.add_space(10.0);
                    settings_fields(ui, project_settings);
                });
            },
            |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                let parent_display = new_project_parent.display().to_string();
                let options = kit::BrowsePathOptions::new()
                    .id_salt("new_project_save_location")
                    .initial_dir(new_project_parent.as_path())
                    .remember_last_dir();
                if let Some(folder) =
                    kit::labeled_browse_folder_field(ui, "Save Location", parent_display, options)
                {
                    *new_project_parent = folder;
                }
                ui.add_space(kit::ACTION_GAP);
                let create_w = ui.available_width();
                if kit::primary_button(ui, "Create Project", create_w).clicked() {
                    create_clicked = true;
                }
            },
        );

        if create_clicked {
            match self.editor.create_project(
                &self.new_project_parent,
                self.new_project_name.trim(),
                self.project_settings.clone(),
            ) {
                Ok(_) => {
                    self.clear_project_runtime_cache();
                    self.editor.overlays.new_project = false;
                }
                Err(err) => self.editor.status = err,
            }
        }
    }

    fn project_settings_modal(&mut self, ctx: &Context) {
        let mut open = true;
        let mut close_clicked = false;
        kit::modal_scrim(ctx, "project_settings");
        egui::Window::new("Project Settings")
            .title_bar(false)
            .order(egui::Order::Foreground)
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .fixed_size([560.0, 520.0])
            .frame(kit::modal_frame())
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                close_clicked = kit::modal_header_with_close(
                    ui,
                    "Project Settings",
                    Some("Update resolution, timing, and preview scale."),
                    true,
                );
                kit::modal_body(ui, |ui| {
                    kit::card_frame()
                        .show(ui, |ui| settings_fields(ui, &mut self.project_settings));
                    ui.add_space(14.0);
                    ui.horizontal(|ui| {
                        if kit::secondary_button(ui, "Cancel", 120.0).clicked() {
                            self.project_settings = self.editor.project.settings.clone();
                            self.editor.overlays.project_settings = false;
                        }
                        if kit::primary_button(ui, "Save Changes", 180.0).clicked() {
                            self.editor.project.settings = self.project_settings.clone();
                            self.editor.preview_dirty = true;
                            self.editor.overlays.project_settings = false;
                        }
                    });
                });
            });
        if close_clicked || !open {
            self.editor.overlays.project_settings = false;
        }
    }

    fn generative_video_modal(&mut self, ctx: &Context) {
        let mut open = true;
        let mut close_clicked = false;
        kit::modal_scrim(ctx, "generative_video");
        egui::Window::new("New Generative Video")
            .title_bar(false)
            .order(egui::Order::Foreground)
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .fixed_size([380.0, 220.0])
            .frame(kit::modal_frame())
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                close_clicked = kit::modal_header_with_close(
                    ui,
                    "New Generative Video",
                    Some("Define the target duration for this asset."),
                    true,
                );
                kit::modal_body(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::DragValue::new(&mut self.gen_video_fps)
                                .speed(1.0)
                                .prefix("FPS "),
                        );
                        ui.add(
                            egui::DragValue::new(&mut self.gen_video_frames)
                                .speed(1)
                                .prefix("Frames "),
                        );
                    });
                    ui.add_space(8.0);
                    ui.label(kit::body(format!(
                        "Duration {}",
                        generative_video_duration_label(self.gen_video_fps, self.gen_video_frames)
                    )));
                    ui.add_space(18.0);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if kit::primary_button(ui, "Create", 120.0).clicked() {
                            if let Err(err) = self
                                .editor
                                .create_generative_video(self.gen_video_fps, self.gen_video_frames)
                            {
                                self.editor.status = err;
                            }
                            self.editor.overlays.generative_video = false;
                        }
                        if kit::secondary_button(ui, "Cancel", 100.0).clicked() {
                            self.editor.overlays.generative_video = false;
                        }
                    });
                });
            });
        if close_clicked || !open {
            self.editor.overlays.generative_video = false;
        }
    }

    fn queue_panel(&mut self, ctx: &Context) {
        let mut close_clicked = false;
        let mut clear_clicked = false;
        let app_rect = ctx.content_rect();
        let fallback_anchor = Rect::from_min_size(
            Pos2::new(app_rect.right() - 72.0, app_rect.top() + 4.0),
            Vec2::new(62.0, kit::TOP_BAR_BUTTON_H),
        );
        let anchor = self.queue_button_rect.unwrap_or(fallback_anchor);
        let bounds = app_rect.shrink(QUEUE_PANEL_MARGIN);
        let jobs = self.editor.generation_queue.clone();
        let has_attention = jobs.iter().any(|job| {
            matches!(
                job.status,
                GenerationJobStatus::Queued | GenerationJobStatus::Running
            )
        });
        let has_clearable = jobs
            .iter()
            .any(|job| job.status != GenerationJobStatus::Running);
        let desired_body_h = queue_list_height(&jobs);
        let desired_h =
            QUEUE_PANEL_PAD * 2.0 + QUEUE_PANEL_HEADER_H + QUEUE_PANEL_GAP + desired_body_h;
        let max_h_by_window = (app_rect.height() - QUEUE_PANEL_MAX_APP_GAP).max(QUEUE_PANEL_MIN_H);
        let panel_top =
            (anchor.bottom() + QUEUE_PANEL_GAP).clamp(bounds.top(), bounds.bottom() - 24.0);
        let max_h_below = (bounds.bottom() - panel_top).max(QUEUE_PANEL_MIN_H);
        let panel_h = desired_h.clamp(
            QUEUE_PANEL_MIN_H,
            max_h_by_window.min(max_h_below).max(QUEUE_PANEL_MIN_H),
        );
        let max_x = (bounds.right() - QUEUE_PANEL_W).max(bounds.left());
        let panel_pos = Pos2::new(
            (anchor.right() - QUEUE_PANEL_W).clamp(bounds.left(), max_x),
            panel_top,
        );

        if kit::modal_scrim(ctx, "queue").clicked() {
            close_clicked = true;
        }

        egui::Area::new(egui::Id::new("generation_queue_popover"))
            .order(egui::Order::Foreground)
            .fixed_pos(panel_pos)
            .show(ctx, |ui| {
                let (panel_rect, _) =
                    ui.allocate_exact_size(Vec2::new(QUEUE_PANEL_W, panel_h), Sense::hover());
                paint_queue_panel_shell(ui, panel_rect, has_attention);

                let content_rect = panel_rect.shrink(QUEUE_PANEL_PAD);
                let mut child = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(content_rect)
                        .layout(Layout::top_down(Align::Min)),
                );
                child.set_min_size(content_rect.size());
                child.shrink_clip_rect(content_rect);
                child.set_width(content_rect.width());

                let header_rect = Rect::from_min_size(
                    content_rect.min,
                    Vec2::new(content_rect.width(), QUEUE_PANEL_HEADER_H),
                );
                let body_rect = Rect::from_min_max(
                    Pos2::new(content_rect.left(), header_rect.bottom() + QUEUE_PANEL_GAP),
                    content_rect.right_bottom(),
                );

                queue_header(
                    &mut child,
                    header_rect,
                    jobs.len(),
                    has_clearable,
                    &mut clear_clicked,
                    &mut close_clicked,
                );
                queue_body(&mut child, body_rect, &jobs);
            });

        if clear_clicked {
            let before = self.editor.generation_queue.len();
            self.editor
                .generation_queue
                .retain(|job| job.status == GenerationJobStatus::Running);
            let cleared = before.saturating_sub(self.editor.generation_queue.len());
            self.editor.status = if cleared == 1 {
                "Cleared 1 generation job.".to_string()
            } else {
                format!("Cleared {cleared} generation jobs.")
            };
        }
        if close_clicked {
            self.editor.overlays.queue = false;
        }
    }

    fn service_generation_queue(&mut self, ctx: &Context) {
        while let Ok(event) = self.generation_events_rx.try_recv() {
            self.handle_generation_event(event);
        }

        if self.generation_active.is_none() {
            self.start_next_generation_job();
        }

        if self.generation_active.is_some()
            || self
                .editor
                .generation_queue
                .iter()
                .any(|job| job.status == GenerationJobStatus::Queued)
        {
            ctx.request_repaint_after(Duration::from_millis(120));
        }
    }

    fn start_next_generation_job(&mut self) {
        let Some(index) = self
            .editor
            .generation_queue
            .iter()
            .position(|job| job.status == GenerationJobStatus::Queued)
        else {
            return;
        };

        let Some(runtime) = self.generation_runtime.as_ref() else {
            if let Some(job) = self.editor.generation_queue.get_mut(index) {
                job.status = GenerationJobStatus::Failed;
                job.error = Some("Generation runtime unavailable.".to_string());
            }
            self.editor.status = "Generation runtime unavailable.".to_string();
            return;
        };

        let version = {
            let asset_id = self.editor.generation_queue[index].asset_id;
            let config = self
                .editor
                .project
                .generative_config(asset_id)
                .cloned()
                .unwrap_or_default();
            next_version_label(&config)
        };
        let job = {
            let entry = &mut self.editor.generation_queue[index];
            entry.status = GenerationJobStatus::Running;
            entry.progress_overall = Some(0.0);
            entry.progress_node = Some(0.0);
            entry.error = None;
            entry.version = Some(version.clone());
            entry.clone()
        };
        self.generation_active = Some(job.id);

        let events = self.generation_events_tx.clone();
        runtime.spawn(async move {
            let (progress_tx, mut progress_rx) =
                tokio::sync::mpsc::unbounded_channel::<comfyui::ComfyUiProgress>();
            let progress_job_id = job.id;
            let progress_events = events.clone();
            tokio::spawn(async move {
                while let Some(progress) = progress_rx.recv().await {
                    let _ = progress_events.send(GenerationEvent::Progress {
                        job_id: progress_job_id,
                        overall: progress.overall,
                        node: progress.node,
                    });
                }
            });

            let job_id = job.id;
            let result = execute_generation_job_async(job, version, Some(progress_tx)).await;
            let _ = events.send(GenerationEvent::Finished { job_id, result });
        });
    }

    fn handle_generation_event(&mut self, event: GenerationEvent) {
        match event {
            GenerationEvent::Progress {
                job_id,
                overall,
                node,
            } => {
                if let Some(job) = self
                    .editor
                    .generation_queue
                    .iter_mut()
                    .find(|job| job.id == job_id)
                {
                    if job.status == GenerationJobStatus::Running {
                        if let Some(overall) = overall {
                            job.progress_overall = Some(overall.clamp(0.0, 1.0));
                        }
                        if let Some(node) = node {
                            job.progress_node = Some(node.clamp(0.0, 1.0));
                        }
                    }
                }
            }
            GenerationEvent::Finished { job_id, result } => {
                if self.generation_active == Some(job_id) {
                    self.generation_active = None;
                }
                let job_snapshot = self
                    .editor
                    .generation_queue
                    .iter()
                    .find(|job| job.id == job_id)
                    .cloned();

                match result {
                    Ok(output) => {
                        if let Some(job) = job_snapshot {
                            if let Some(entry) = self
                                .editor
                                .generation_queue
                                .iter_mut()
                                .find(|job| job.id == job_id)
                            {
                                entry.status = GenerationJobStatus::Succeeded;
                                entry.version = Some(output.version.clone());
                                entry.progress_overall = Some(1.0);
                                entry.progress_node = Some(1.0);
                                entry.error = None;
                            }
                            self.finish_generation_success(job, output);
                        }
                    }
                    Err(err) => {
                        let message = match err {
                            GenerationFailure::Offline(err) => format!("Provider offline: {err}"),
                            GenerationFailure::Error(err) => err,
                        };
                        if let Some(entry) = self
                            .editor
                            .generation_queue
                            .iter_mut()
                            .find(|job| job.id == job_id)
                        {
                            entry.status = GenerationJobStatus::Failed;
                            entry.progress_overall = None;
                            entry.progress_node = None;
                            entry.error = Some(message.clone());
                        }
                        self.editor.status = message;
                    }
                }
            }
        }
    }

    fn finish_generation_success(&mut self, job: GenerationJob, output: GenerationOutput) {
        if self.editor.project.find_asset(job.asset_id).is_none() {
            return;
        }

        let version = output.version.clone();
        let record = GenerationRecord {
            version: version.clone(),
            timestamp: chrono::Utc::now(),
            provider_id: job.provider.id,
            inputs_snapshot: job.inputs_snapshot.clone(),
        };
        self.editor
            .project
            .update_generative_config(job.asset_id, |config| {
                config.provider_id = Some(job.provider.id);
                config.active_version = Some(version.clone());
                config.inputs = job.inputs_snapshot.clone();
                if let Some(existing) = config
                    .versions
                    .iter_mut()
                    .find(|record| record.version == version)
                {
                    *existing = record;
                } else {
                    config.versions.push(record);
                }
            });
        if let Err(err) = self.editor.project.save_generative_config(job.asset_id) {
            self.editor.status = format!("Generated, but config save failed: {err}");
        } else {
            self.editor.status = format!(
                "Generated {} {} ({})",
                job.asset_label,
                output.version,
                path_label(&output.path)
            );
        }

        self.editor.previewer.invalidate_folder(&job.folder_path);
        self.invalidate_asset_visual_cache(job.asset_id);
        self.editor.preview_dirty = true;

        if let (Some(runtime), Some(asset)) = (
            self.generation_runtime.as_ref(),
            self.editor.project.find_asset(job.asset_id).cloned(),
        ) {
            let thumbnailer = Arc::clone(&self.editor.thumbnailer);
            runtime.spawn(async move {
                let _ = thumbnailer.generate(&asset, true).await;
            });
        }
    }

    fn invalidate_asset_visual_cache(&mut self, asset_id: Uuid) {
        self.asset_thumbnails.remove(&asset_id);
        self.asset_thumbnail_misses.remove(&asset_id);
        self.timeline_thumbnails
            .retain(|key, _| key.asset_id != asset_id);
        self.timeline_thumbnail_misses
            .retain(|key| key.asset_id != asset_id);
    }

    fn generation_status_for_asset(&self, asset_id: Uuid) -> Option<String> {
        self.editor
            .generation_queue
            .iter()
            .rev()
            .find(|job| job.asset_id == asset_id)
            .map(|job| match job.status {
                GenerationJobStatus::Queued => "Queued".to_string(),
                GenerationJobStatus::Running => {
                    let pct = job
                        .progress_overall
                        .or(job.progress_node)
                        .map(|value| format!(" {:.0}%", value * 100.0))
                        .unwrap_or_default();
                    format!("Generating{pct}")
                }
                GenerationJobStatus::Succeeded => job
                    .version
                    .as_ref()
                    .map(|version| format!("Generated {version}"))
                    .unwrap_or_else(|| "Generated".to_string()),
                GenerationJobStatus::Failed => job
                    .error
                    .as_ref()
                    .map(|error| format!("Failed: {error}"))
                    .unwrap_or_else(|| "Failed".to_string()),
            })
    }

    fn enqueue_generation_jobs(
        &mut self,
        asset_id: Uuid,
        clip_id: Uuid,
        provider: ProviderEntry,
        config_snapshot: GenerativeConfig,
        folder_path: PathBuf,
        asset_label: String,
    ) -> Result<String, String> {
        if provider.output_type == ProviderOutputType::Audio {
            return Err("Audio generation is not supported in the queue yet.".to_string());
        }

        let resolved = resolve_provider_inputs(&provider, &config_snapshot);
        if !resolved.missing_required.is_empty() {
            return Err(format!(
                "Missing inputs: {}",
                resolved.missing_required.join(", ")
            ));
        }

        let batch = config_snapshot.batch.clone();
        let batch_count = batch.count.max(1).min(MAX_GENERATION_BATCH_COUNT);
        let seed_field = resolve_seed_field(&provider, batch.seed_field.as_deref());
        let mut seed_base = seed_field
            .as_ref()
            .and_then(|field| resolved.values.get(field))
            .and_then(input_value_as_i64);
        let mut seed_base_randomized = false;
        if seed_base.is_none()
            && seed_field.is_some()
            && batch.seed_strategy == SeedStrategy::Increment
        {
            seed_base = Some(random_seed_i64());
            seed_base_randomized = true;
        }

        for index in 0..batch_count {
            let (inputs, inputs_snapshot) = match (batch.seed_strategy, seed_field.as_ref()) {
                (SeedStrategy::Keep, _) | (_, None) => {
                    (resolved.values.clone(), resolved.snapshot.clone())
                }
                (SeedStrategy::Increment, Some(field)) => {
                    let seed = seed_base.unwrap_or(0) + index as i64;
                    update_seed_inputs(&resolved.values, &resolved.snapshot, field, seed)
                }
                (SeedStrategy::Random, Some(field)) => {
                    let seed = random_seed_i64();
                    update_seed_inputs(&resolved.values, &resolved.snapshot, field, seed)
                }
            };

            self.editor.generation_queue.push(GenerationJob {
                id: Uuid::new_v4(),
                created_at: chrono::Utc::now(),
                status: GenerationJobStatus::Queued,
                progress_overall: None,
                progress_node: None,
                attempts: 0,
                next_attempt_at: None,
                provider: provider.clone(),
                output_type: provider.output_type,
                asset_id,
                clip_id,
                asset_label: asset_label.clone(),
                folder_path: folder_path.clone(),
                inputs,
                inputs_snapshot,
                version: None,
                error: None,
            });
        }

        let mut status = if batch_count > 1 {
            format!("Queued {batch_count} jobs")
        } else {
            "Queued".to_string()
        };
        if batch_count > 1 {
            if batch.seed_strategy == SeedStrategy::Keep {
                status.push_str(" (identical inputs may be cached)");
            } else if seed_field.is_none() {
                status.push_str(" (no seed field detected)");
            } else if seed_base_randomized {
                status.push_str(" (seed missing, randomized base)");
            }
        }
        Ok(status)
    }

    fn providers_modal(&mut self, ctx: &Context) {
        let mut open = true;
        let mut close_clicked = false;
        let modal_size = modal_size(ctx, PROVIDERS_MODAL_SIZE, [620.0, 460.0]);
        kit::modal_scrim(ctx, "providers");
        egui::Window::new("AI Providers (Global)")
            .title_bar(false)
            .order(egui::Order::Foreground)
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .fixed_size(modal_size)
            .frame(kit::modal_frame())
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                close_clicked = kit::modal_header_with_close(
                    ui,
                    "AI Providers",
                    Some("Global provider definitions and manifests."),
                    true,
                );
                kit::modal_body(ui, |ui| {
                    ui.label(kit::caption(
                        crate::core::provider_store::global_providers_root()
                            .display()
                            .to_string(),
                    ));
                    ui.add_space(12.0);
                    StripBuilder::new(ui)
                        .clip(true)
                        .size(Size::exact(300.0))
                        .size(Size::exact(12.0))
                        .size(Size::remainder().at_least(260.0))
                        .horizontal(|mut strip| {
                            strip.cell(|ui| self.provider_list_card(ui));
                            strip.empty();
                            strip.cell(|ui| self.provider_editor_choice_card(ui));
                        });
                });
            });
        if close_clicked || !open {
            self.editor.overlays.providers = false;
        }
    }

    fn provider_list_card(&mut self, ui: &mut Ui) {
        let card_h = ui.available_height();
        kit::card_panel(ui, card_h, |ui| {
            let mut top_action = None;
            kit::equal_secondary_button_row(ui, &["New", "Reload"], |index| {
                top_action = Some(index);
            });
            match top_action {
                Some(0) => self.open_provider_builder(None),
                Some(1) => self.refresh_provider_files(),
                _ => {}
            }

            ui.add_space(kit::FORM_ROW_GAP);
            let selected = self.selected_provider_file.clone();
            let provider_files = self.editor.provider_files.clone();
            let footer_h = if selected.is_some() {
                kit::ACTION_GAP + kit::SECONDARY_BUTTON_H
            } else {
                0.0
            };
            let mut next_selection: Option<PathBuf> = None;
            let mut delete_clicked = false;

            kit::body_with_footer(
                ui,
                120.0,
                footer_h,
                |ui| {
                    kit::scroll_body(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = kit::FORM_ROW_GAP;
                        if provider_files.is_empty() {
                            kit::empty_state(
                                ui,
                                "No providers yet",
                                "Create a provider or reload the global provider folder.",
                            );
                        }
                        for path in provider_files.iter() {
                            let summary = provider_file_summary(path);
                            let selected = self.selected_provider_file.as_ref() == Some(path);
                            let response = provider_row(ui, path, &summary, selected);
                            if response.clicked() {
                                next_selection = Some(path.clone());
                            }
                        }
                    });
                },
                |ui| {
                    if selected.is_some() {
                        ui.add_space(kit::ACTION_GAP);
                        if kit::danger_button(ui, "Delete", ui.available_width()).clicked() {
                            delete_clicked = true;
                        }
                    }
                },
            );

            if let Some(path) = next_selection {
                self.selected_provider_file = Some(path);
            }
            if delete_clicked {
                if let Some(path) = selected {
                    match std::fs::remove_file(&path) {
                        Ok(()) => {
                            self.editor.status = format!("Deleted provider {}", path_label(&path));
                            self.selected_provider_file = None;
                            self.refresh_provider_files();
                        }
                        Err(err) => {
                            self.editor.status =
                                format!("Failed to delete provider {}: {err}", path_label(&path));
                        }
                    }
                }
            }
        });
    }

    fn provider_editor_choice_card(&mut self, ui: &mut Ui) {
        let card_h = ui.available_height();
        kit::card_panel(ui, card_h, |ui| {
            let Some(path) = self.selected_provider_file.clone() else {
                kit::empty_state(
                    ui,
                    "Select a provider",
                    "Choose a provider from the list to edit it.",
                );
                return;
            };

            if !path.exists() {
                kit::empty_state(
                    ui,
                    "Provider missing",
                    "Reload the provider list to refresh this selection.",
                );
                return;
            }

            let summary = provider_file_summary(&path);
            let mut open_builder = false;
            let mut open_json = false;
            ui.centered_and_justified(|ui| {
                ui.vertical_centered(|ui| {
                    ui.label(
                        RichText::new(&summary.name)
                            .color(kit::TEXT)
                            .strong()
                            .size(15.0),
                    );
                    ui.add_space(4.0);
                    ui.label(kit::caption("Select an editor:"));
                    ui.add_space(24.0);
                    if kit::secondary_button(ui, "Edit in Builder", 250.0).clicked() {
                        open_builder = true;
                    }
                    ui.add_space(8.0);
                    if kit::secondary_button(ui, "Edit as JSON", 250.0).clicked() {
                        open_json = true;
                    }
                });
            });

            if open_builder {
                self.open_provider_builder(Some(path.clone()));
            }
            if open_json {
                self.open_provider_json_editor(path);
            }
        });
    }

    fn provider_json_editor_modal(&mut self, ctx: &Context) {
        let Some(path) = self.provider_json_editor_path.clone() else {
            return;
        };
        let mut open = true;
        let mut close_clicked = false;
        let mut save_clicked = false;
        let size = modal_size(ctx, PROVIDER_JSON_MODAL_SIZE, [680.0, 520.0]);

        kit::modal_scrim(ctx, "provider_json_editor");
        egui::Window::new("Edit Provider JSON")
            .title_bar(false)
            .order(egui::Order::Foreground)
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .fixed_size(size)
            .frame(kit::modal_frame())
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                let file_name = path
                    .file_name()
                    .and_then(|value| value.to_str())
                    .unwrap_or("provider.json");
                close_clicked =
                    kit::modal_header_with_close(ui, "Edit Provider JSON", Some(file_name), true);
                kit::modal_body(ui, |ui| {
                    if let Some(error) = &self.provider_json_error {
                        ui.label(RichText::new(error).color(kit::MARKER).size(12.0));
                        ui.add_space(kit::FORM_ROW_GAP);
                    }

                    kit::body_with_footer(
                        ui,
                        320.0,
                        kit::SECONDARY_BUTTON_H,
                        |ui| {
                            kit::code_editor_field(
                                ui,
                                &mut self.provider_json_text,
                                "provider_json_editor",
                            );
                        },
                        |ui| {
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if kit::primary_button(ui, "Save JSON", 130.0).clicked() {
                                    save_clicked = true;
                                }
                                if kit::secondary_button(ui, "Cancel", 110.0).clicked() {
                                    close_clicked = true;
                                }
                            });
                        },
                    );
                });
            });

        if save_clicked {
            self.save_provider_json_editor(&path);
        }
        if close_clicked || !open {
            self.provider_json_editor_path = None;
            self.provider_json_error = None;
        }
    }

    fn refresh_provider_files(&mut self) {
        self.editor.refresh_providers();
        if let Some(selected) = &self.selected_provider_file {
            if !self
                .editor
                .provider_files
                .iter()
                .any(|path| path == selected)
            {
                self.selected_provider_file = None;
            }
        }
    }

    fn open_provider_json_editor(&mut self, path: PathBuf) {
        self.provider_json_text =
            crate::core::provider_store::read_provider_file(&path).unwrap_or_default();
        self.provider_json_error = if self.provider_json_text.is_empty() {
            Some(format!("Failed to read provider {}", path.display()))
        } else {
            None
        };
        self.provider_json_editor_path = Some(path);
    }

    fn save_provider_json_editor(&mut self, path: &Path) {
        let entry = match serde_json::from_str::<ProviderEntry>(&self.provider_json_text) {
            Ok(entry) => entry,
            Err(err) => {
                self.provider_json_error = Some(format!("Invalid provider JSON: {err}"));
                return;
            }
        };
        let pretty = match serde_json::to_string_pretty(&entry) {
            Ok(pretty) => pretty,
            Err(err) => {
                self.provider_json_error = Some(format!("Failed to format provider JSON: {err}"));
                return;
            }
        };
        if let Err(err) = crate::core::provider_store::write_provider_file(path, &pretty) {
            self.provider_json_error = Some(format!("Failed to save provider: {err}"));
            return;
        }

        self.provider_json_text = pretty;
        self.provider_json_error = None;
        self.selected_provider_file = Some(path.to_path_buf());
        self.refresh_provider_files();
        self.provider_json_editor_path = None;
        self.editor.status = format!("Saved provider {}", path_label(path));
    }

    fn open_provider_builder(&mut self, path: Option<PathBuf>) {
        let mut state = match path.as_ref() {
            Some(path) => ProviderBuilderState::from_path(path),
            None => ProviderBuilderState::from_entry(
                None,
                crate::core::provider_store::default_provider_entry(),
            ),
        };
        if state.source_path.is_none() {
            state.source_path = path;
        }
        self.provider_builder = state;
        self.provider_builder_open = true;
    }

    fn provider_builder_modal(&mut self, ctx: &Context) {
        let mut open = true;
        let mut close_clicked = false;
        let mut save_clicked = false;
        let size = modal_size(ctx, PROVIDER_BUILDER_MODAL_SIZE, [780.0, 560.0]);

        kit::modal_scrim(ctx, "provider_builder");
        egui::Window::new("Provider Builder")
            .title_bar(false)
            .order(egui::Order::Foreground)
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .fixed_size(size)
            .frame(kit::modal_frame())
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                close_clicked = kit::modal_header_with_close(
                    ui,
                    "Provider Builder (ComfyUI)",
                    Some(if self.provider_builder.source_path.is_some() {
                        "Mode: Edit"
                    } else {
                        "Mode: New"
                    }),
                    true,
                );
                kit::modal_body(ui, |ui| {
                    self.provider_builder_topbar(ui);
                    self.provider_builder_errors(ui);
                    ui.add_space(kit::FORM_ROW_GAP);
                    self.provider_builder_tabs(ui);
                    ui.add_space(kit::FORM_ROW_GAP);
                    kit::body_with_footer(
                        ui,
                        360.0,
                        kit::SECONDARY_BUTTON_H,
                        |ui| self.provider_builder_columns(ui),
                        |ui| {
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if kit::primary_button(ui, "Save Provider", 150.0).clicked() {
                                    save_clicked = true;
                                }
                                if kit::secondary_button(ui, "Cancel", 110.0).clicked() {
                                    close_clicked = true;
                                }
                            });
                        },
                    );
                });
            });

        if save_clicked {
            self.save_provider_builder();
        }
        if close_clicked || !open {
            self.provider_builder_open = false;
            self.provider_builder.error = None;
            self.provider_builder.workflow_error = None;
        }
    }

    fn provider_builder_topbar(&mut self, ui: &mut Ui) {
        let workflow_display = self
            .provider_builder
            .workflow_path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "No workflow selected".to_string());
        ui.horizontal(|ui| {
            ui.add_sized(
                [(ui.available_width() - 160.0).max(80.0), 18.0],
                egui::Label::new(kit::caption(workflow_display)).truncate(),
            );
            if kit::secondary_button(ui, "Choose Workflow...", 148.0).clicked() {
                let initial = self
                    .provider_builder
                    .workflow_path
                    .as_ref()
                    .and_then(|path| path.parent().map(Path::to_path_buf))
                    .or_else(|| crate::core::paths::resource_dir("workflows"));
                let mut options = kit::BrowseFileOptions::new()
                    .id_salt("provider_builder_workflow")
                    .filters(JSON_FILE_FILTERS)
                    .remember_last_dir();
                if let Some(initial) = initial.as_deref() {
                    options = options.initial_dir(initial);
                }
                if let Some(path) = kit::pick_file_dialog(ui, options) {
                    self.set_provider_builder_workflow(path);
                }
            }
        });
    }

    fn provider_builder_errors(&mut self, ui: &mut Ui) {
        if let Some(error) = &self.provider_builder.workflow_error {
            ui.label(RichText::new(error).color(kit::MARKER).size(12.0));
        }
        if let Some(error) = &self.provider_builder.error {
            ui.label(RichText::new(error).color(kit::MARKER).size(12.0));
        }
    }

    fn provider_builder_tabs(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            let inputs_active = self.provider_builder.tab == ProviderBuilderTab::Inputs;
            if kit::timeline_tool_text_button(ui, "Inputs", 74.0, inputs_active).clicked() {
                self.provider_builder.tab = ProviderBuilderTab::Inputs;
            }
            if kit::timeline_tool_text_button(ui, "Output", 74.0, !inputs_active).clicked() {
                self.provider_builder.tab = ProviderBuilderTab::Output;
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(kit::caption(self.provider_builder.output_status_label()));
            });
        });
    }

    fn provider_builder_columns(&mut self, ui: &mut Ui) {
        kit::fixed_panel_body(ui, |ui| {
            StripBuilder::new(ui)
                .clip(true)
                .size(Size::exact(280.0))
                .size(Size::exact(12.0))
                .size(Size::exact(230.0))
                .size(Size::exact(12.0))
                .size(Size::remainder().at_least(260.0))
                .horizontal(|mut strip| {
                    strip.cell(|ui| self.provider_builder_node_list(ui));
                    strip.empty();
                    strip.cell(|ui| self.provider_builder_node_details(ui));
                    strip.empty();
                    strip.cell(|ui| self.provider_builder_settings(ui));
                });
        });
    }

    fn provider_builder_node_list(&mut self, ui: &mut Ui) {
        kit::card_panel(ui, ui.available_height(), |ui| {
            kit::singleline_text_field(
                ui,
                &mut self.provider_builder.workflow_search,
                ui.available_width(),
            );
            ui.add_space(kit::FORM_ROW_GAP);
            let filtered = self.provider_builder.filtered_nodes();
            kit::scroll_body(ui, |ui| {
                ui.spacing_mut().item_spacing.y = kit::FORM_ROW_GAP;
                if filtered.is_empty() {
                    kit::empty_state(
                        ui,
                        if self.provider_builder.workflow_nodes.is_empty() {
                            "No workflow nodes"
                        } else {
                            "No matching nodes"
                        },
                        "Choose a workflow or adjust the search.",
                    );
                }
                for node in filtered {
                    let selected = self
                        .provider_builder
                        .selected_node_id
                        .as_ref()
                        .is_some_and(|id| id == &node.id);
                    let response = workflow_node_row(ui, &node, selected);
                    if response.clicked() {
                        self.provider_builder.selected_node_id = Some(node.id);
                    }
                }
            });
        });
    }

    fn provider_builder_node_details(&mut self, ui: &mut Ui) {
        kit::card_panel(ui, ui.available_height(), |ui| {
            let selected_node = self.provider_builder.selected_node();
            let Some(node) = selected_node else {
                kit::empty_state(
                    ui,
                    "Select a node",
                    if self.provider_builder.tab == ProviderBuilderTab::Inputs {
                        "Expose workflow inputs from the selected node."
                    } else {
                        "Use the selected node as the output source."
                    },
                );
                return;
            };

            ui.label(kit::value(
                node.title.clone().unwrap_or_else(|| "Untitled".to_string()),
            ));
            ui.label(kit::caption(format!("Class: {}", node.class_type)));
            ui.label(kit::caption(format!("Node ID: {}", node.id)));
            ui.add_space(kit::ACTION_GAP);

            match self.provider_builder.tab {
                ProviderBuilderTab::Inputs => {
                    kit::field_label(ui, "Inputs");
                    ui.add_space(kit::FORM_ROW_GAP);
                    if node.inputs.is_empty() {
                        ui.label(kit::caption("No inputs found on this node."));
                    }
                    let mut expose_key: Option<String> = None;
                    for input_key in node.inputs.iter() {
                        ui.horizontal(|ui| {
                            ui.add_sized(
                                [(ui.available_width() - 76.0).max(60.0), 18.0],
                                egui::Label::new(kit::body(input_key)).truncate(),
                            );
                            if kit::field_button(ui, "Expose", 68.0).clicked() {
                                expose_key = Some(input_key.clone());
                            }
                        });
                    }
                    if let Some(input_key) = expose_key {
                        self.expose_provider_builder_input(&node, &input_key);
                    }
                }
                ProviderBuilderTab::Output => {
                    kit::field_label(ui, "Output Node");
                    ui.add_space(kit::FORM_ROW_GAP);
                    let use_output_w = ui.available_width();
                    if kit::secondary_button(ui, "Use as Output", use_output_w).clicked() {
                        self.provider_builder.output_node = Some(ProviderOutputNodeDraft {
                            class_type: node.class_type,
                            title: node.title,
                        });
                        self.provider_builder.error = None;
                    }
                }
            }
        });
    }

    fn provider_builder_settings(&mut self, ui: &mut Ui) {
        kit::scroll_body(ui, |ui| {
            kit::card_frame().show(ui, |ui| {
                kit::field_label(ui, "Provider Settings");
                ui.add_space(kit::FORM_ROW_GAP);
                StripBuilder::new(ui)
                    .clip(true)
                    .size(Size::remainder().at_least(180.0))
                    .size(Size::exact(kit::FORM_ROW_GAP))
                    .size(Size::exact(120.0))
                    .horizontal(|mut strip| {
                        strip.cell(|ui| {
                            kit::labeled_text_field(
                                ui,
                                "Name",
                                &mut self.provider_builder.provider_name,
                            );
                        });
                        strip.empty();
                        strip.cell(|ui| {
                            provider_output_type_field(
                                ui,
                                "Type",
                                &mut self.provider_builder.output_type,
                            );
                        });
                    });
                ui.add_space(kit::FORM_ROW_GAP);
                kit::labeled_text_field(ui, "Base URL", &mut self.provider_builder.base_url);
                ui.add_space(kit::FORM_ROW_GAP);

                let workflow_display = self.provider_builder.workflow_path_display();
                let workflow_initial = self
                    .provider_builder
                    .workflow_path
                    .as_ref()
                    .and_then(|path| path.parent())
                    .or_else(|| {
                        self.provider_builder
                            .source_path
                            .as_deref()
                            .and_then(Path::parent)
                    });
                let mut workflow_options = kit::BrowseFileOptions::new()
                    .id_salt("provider_builder_workflow_field")
                    .filters(JSON_FILE_FILTERS)
                    .remember_last_dir();
                if let Some(initial) = workflow_initial {
                    workflow_options = workflow_options.initial_dir(initial);
                }
                if let Some(path) = kit::labeled_browse_file_field(
                    ui,
                    "Workflow",
                    workflow_display,
                    workflow_options,
                ) {
                    self.set_provider_builder_workflow(path);
                }
                ui.add_space(kit::FORM_ROW_GAP);

                let manifest_display = self.provider_builder.manifest_path_display();
                let manifest_initial = self
                    .provider_builder
                    .manifest_path
                    .as_ref()
                    .and_then(|path| path.parent())
                    .or_else(|| {
                        self.provider_builder
                            .workflow_path
                            .as_deref()
                            .and_then(Path::parent)
                    });
                let mut manifest_options = kit::BrowseFileOptions::new()
                    .id_salt("provider_builder_manifest_field")
                    .filters(JSON_FILE_FILTERS)
                    .remember_last_dir();
                if let Some(initial) = manifest_initial {
                    manifest_options = manifest_options.initial_dir(initial);
                }
                if let Some(path) = kit::labeled_browse_file_field(
                    ui,
                    "Manifest",
                    manifest_display,
                    manifest_options,
                ) {
                    self.set_provider_builder_manifest(path);
                }
            });

            ui.add_space(kit::ACTION_GAP);
            match self.provider_builder.tab {
                ProviderBuilderTab::Inputs => self.provider_builder_inputs_editor(ui),
                ProviderBuilderTab::Output => self.provider_builder_output_editor(ui),
            }
        });
    }

    fn provider_builder_inputs_editor(&mut self, ui: &mut Ui) {
        kit::card_frame().show(ui, |ui| {
            ui.horizontal(|ui| {
                kit::field_label(
                    ui,
                    &format!("Exposed Inputs ({})", self.provider_builder.inputs.len()),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if kit::field_button(ui, "Add Input", 86.0).clicked() {
                        self.provider_builder
                            .inputs
                            .push(ProviderBuilderInput::blank());
                    }
                });
            });
            ui.add_space(kit::FORM_ROW_GAP);
            if self.provider_builder.inputs.is_empty() {
                ui.label(kit::caption(
                    "No inputs exposed. Select a workflow node and expose its inputs.",
                ));
                return;
            }

            let mut action = None;
            let len = self.provider_builder.inputs.len();
            for index in 0..len {
                if index > 0 {
                    ui.add_space(kit::FORM_ROW_GAP);
                }
                provider_builder_input_editor(
                    ui,
                    index,
                    len,
                    &mut self.provider_builder.inputs[index],
                    &mut action,
                );
            }
            if let Some(action) = action {
                self.apply_provider_input_action(action);
            }
        });
    }

    fn provider_builder_output_editor(&mut self, ui: &mut Ui) {
        kit::card_frame().show(ui, |ui| {
            kit::field_label(ui, "Output Configuration");
            ui.add_space(kit::FORM_ROW_GAP);
            let output_label = self
                .provider_builder
                .output_node
                .as_ref()
                .map(|node| {
                    node.title
                        .clone()
                        .unwrap_or_else(|| node.class_type.clone())
                })
                .unwrap_or_else(|| "No output node selected".to_string());
            ui.label(kit::caption(format!("Node: {output_label}")));
            ui.add_space(kit::FORM_ROW_GAP);
            StripBuilder::new(ui)
                .clip(true)
                .size(Size::remainder().at_least(120.0))
                .size(Size::exact(kit::FORM_ROW_GAP))
                .size(Size::remainder().at_least(120.0))
                .horizontal(|mut strip| {
                    strip.cell(|ui| {
                        kit::labeled_text_field(
                            ui,
                            "Output Key",
                            &mut self.provider_builder.output_key,
                        );
                    });
                    strip.empty();
                    strip.cell(|ui| {
                        kit::labeled_text_field(
                            ui,
                            "Output Tag",
                            &mut self.provider_builder.output_tag,
                        );
                    });
                });
        });
    }

    fn set_provider_builder_workflow(&mut self, path: PathBuf) {
        match load_workflow_nodes_resolved(&path) {
            Ok(nodes) => {
                self.provider_builder.workflow_path = Some(path);
                self.provider_builder.workflow_nodes = nodes;
                self.provider_builder.workflow_error = None;
                self.provider_builder.selected_node_id = None;
            }
            Err(err) => {
                self.provider_builder.workflow_path = Some(path);
                self.provider_builder.workflow_nodes.clear();
                self.provider_builder.workflow_error = Some(err);
                self.provider_builder.selected_node_id = None;
            }
        }
    }

    fn set_provider_builder_manifest(&mut self, path: PathBuf) {
        match load_provider_manifest_resolved(&path) {
            Ok(manifest) => {
                self.provider_builder.apply_manifest(manifest);
                self.provider_builder.manifest_path = Some(path);
                self.provider_builder.error = None;
                if let Some(workflow_path) = self.provider_builder.workflow_path.clone() {
                    match load_workflow_nodes_resolved(&workflow_path) {
                        Ok(nodes) => {
                            self.provider_builder.workflow_nodes = nodes;
                            self.provider_builder.workflow_error = None;
                            self.provider_builder.selected_node_id = None;
                        }
                        Err(err) => {
                            self.provider_builder.workflow_nodes.clear();
                            self.provider_builder.workflow_error = Some(err);
                        }
                    }
                }
            }
            Err(err) => {
                self.provider_builder.manifest_path = Some(path);
                self.provider_builder.error = Some(err);
            }
        }
    }

    fn expose_provider_builder_input(
        &mut self,
        node: &crate::core::comfyui_workflow::ComfyWorkflowNode,
        input_key: &str,
    ) {
        if self
            .provider_builder
            .inputs
            .iter()
            .any(|input| input.name == input_key)
        {
            self.provider_builder.error = Some("Input already exposed.".to_string());
            return;
        }
        self.provider_builder
            .inputs
            .push(ProviderBuilderInput::from_node(node, input_key));
        self.provider_builder.error = None;
    }

    fn apply_provider_input_action(&mut self, action: ProviderInputAction) {
        match action {
            ProviderInputAction::MoveUp(index) => {
                if index > 0 && index < self.provider_builder.inputs.len() {
                    self.provider_builder.inputs.swap(index - 1, index);
                }
            }
            ProviderInputAction::MoveDown(index) => {
                if index + 1 < self.provider_builder.inputs.len() {
                    self.provider_builder.inputs.swap(index, index + 1);
                }
            }
            ProviderInputAction::Delete(index) => {
                if index < self.provider_builder.inputs.len() {
                    self.provider_builder.inputs.remove(index);
                }
            }
        }
    }

    fn save_provider_builder(&mut self) {
        let save = match self.provider_builder.build_save_payload() {
            Ok(save) => save,
            Err(err) => {
                self.provider_builder.error = Some(err);
                return;
            }
        };

        let manifest_json = match serde_json::to_string_pretty(&save.manifest) {
            Ok(json) => json,
            Err(err) => {
                self.provider_builder.error = Some(format!("Failed to serialize manifest: {err}"));
                return;
            }
        };
        if let Some(parent) = save.manifest_path.parent() {
            if let Err(err) = std::fs::create_dir_all(parent) {
                self.provider_builder.error =
                    Some(format!("Failed to create manifest folder: {err}"));
                return;
            }
        }
        if let Err(err) = std::fs::write(&save.manifest_path, manifest_json) {
            self.provider_builder.error = Some(format!("Failed to write manifest: {err}"));
            return;
        }

        let provider_json = match serde_json::to_string_pretty(&save.entry) {
            Ok(json) => json,
            Err(err) => {
                self.provider_builder.error = Some(format!("Failed to serialize provider: {err}"));
                return;
            }
        };
        if let Err(err) =
            crate::core::provider_store::write_provider_file(&save.provider_path, &provider_json)
        {
            self.provider_builder.error = Some(format!("Failed to save provider: {err}"));
            return;
        }

        self.provider_builder.source_path = Some(save.provider_path.clone());
        self.provider_builder.manifest_path = Some(save.manifest_path);
        self.provider_builder.error = None;
        self.selected_provider_file = Some(save.provider_path.clone());
        self.refresh_provider_files();
        self.provider_builder_open = false;
        self.editor.status = format!("Saved provider {}", path_label(&save.provider_path));
    }

    fn status_bar(&mut self, root: &mut Ui) {
        let response = egui::Panel::bottom("status")
            .exact_size(kit::STATUS_BAR_H)
            .frame(kit::chrome_frame())
            .show_inside(root, |ui| {
                ui.horizontal(|ui| {
                    let status_text = if self.editor.project.project_path.is_some() {
                        format!("{} ({})", self.editor.status, self.editor.project_name())
                    } else {
                        self.editor.status.clone()
                    };
                    ui.label(RichText::new(status_text).small().color(kit::TEXT_MUTED));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new(format!("{:.0} fps", self.editor.project.settings.fps))
                                .small()
                                .color(kit::TEXT_MUTED),
                        );
                    });
                });
            });
        kit::paint_panel_edge(root, response.response.rect, kit::PanelEdge::Top);
    }
}

impl eframe::App for NlaEguiApp {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.poll_automation();
        self.keep_automation_responsive(&ctx);
        self.tick_playback(&ctx);
        self.service_generation_queue(&ctx);
        self.update_preview_texture(&ctx);

        self.top_bar(ui);
        // App-wide bars claim root space first; docked editor panels sit above the status bar.
        self.status_bar(ui);
        self.left_panel(ui);
        self.right_panel(ui);
        self.timeline_panel(ui);
        self.central_preview(ui);

        self.modals(&ctx);
        self.service_audio_decode_warmup(&ctx);
    }
}

async fn execute_generation_job_async(
    job: GenerationJob,
    version: String,
    progress_tx: Option<tokio::sync::mpsc::UnboundedSender<comfyui::ComfyUiProgress>>,
) -> Result<GenerationOutput, GenerationFailure> {
    if job.output_type == ProviderOutputType::Audio {
        return Err(GenerationFailure::Error(
            "Audio outputs are not supported in the queue yet.".to_string(),
        ));
    }

    let output = match job.provider.connection.clone() {
        ProviderConnection::ComfyUi {
            base_url,
            workflow_path,
            manifest_path,
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
                progress_tx,
            )
            .await
            .map_err(GenerationFailure::Error)
        }
        ProviderConnection::CustomHttp { .. } => Err(GenerationFailure::Error(
            "Provider connection not supported yet.".to_string(),
        )),
    };

    let output = match output {
        Ok(output) => output,
        Err(GenerationFailure::Error(err)) => {
            if let ProviderConnection::ComfyUi { base_url, .. } = job.provider.connection.clone() {
                if let Err(health_err) = comfyui::check_health(&base_url).await {
                    return Err(GenerationFailure::Offline(health_err));
                }
            }
            return Err(GenerationFailure::Error(err));
        }
        Err(err) => return Err(err),
    };

    std::fs::create_dir_all(&job.folder_path).map_err(|err| {
        GenerationFailure::Error(format!("Failed to create output folder: {err}"))
    })?;
    let output_path = job
        .folder_path
        .join(format!("{}.{}", version, output.extension));
    std::fs::write(&output_path, &output.bytes)
        .map_err(|err| GenerationFailure::Error(format!("Failed to save output: {err}")))?;

    Ok(GenerationOutput {
        version,
        path: output_path,
    })
}

fn menu_button(
    ui: &mut Ui,
    label: &str,
    add_contents: impl FnOnce(&mut Ui, &mut NlaEguiApp),
    app: &mut NlaEguiApp,
) {
    kit::top_bar_menu_button(ui, label, |ui| add_contents(ui, app));
}

const ADD_ASSETS_CARD_H: f32 = kit::SECTION_PAD as f32 * 2.0
    + kit::FIELD_LABEL_H
    + 6.0
    + kit::STANDALONE_BUTTON_H
    + kit::FORM_ROW_GAP
    + kit::FIELD_LABEL_H
    + 6.0
    + kit::MEDIA_PILL_H
    + kit::FORM_ROW_GAP;
const ASSET_ROW_H: f32 = 56.0;
const ASSET_ROW_THUMBNAIL_SIZE: Vec2 = Vec2::new(40.0, 40.0);
const ASSET_THUMBNAIL_IMAGE_INSET: f32 = 3.0;
const ASSET_ROW_TEXT_GAP: f32 = 10.0;
const INSPECTOR_THUMBNAIL_SIZE: Vec2 = Vec2::new(68.0, 50.0);

fn timeline_rects(outer: Rect) -> TimelineRects {
    let ruler = Rect::from_min_max(
        Pos2::new(outer.left() + TIMELINE_LABEL_W, outer.top()),
        Pos2::new(outer.right(), outer.top() + TIMELINE_RULER_H),
    );
    let add_row = Rect::from_min_max(
        Pos2::new(
            outer.left(),
            outer.bottom() - TIMELINE_ADD_ROW_H - TIMELINE_SCROLLBAR_H,
        ),
        Pos2::new(outer.right(), outer.bottom() - TIMELINE_SCROLLBAR_H),
    );
    let tracks = Rect::from_min_max(
        Pos2::new(outer.left() + TIMELINE_LABEL_W, ruler.bottom()),
        Pos2::new(outer.right(), add_row.top()),
    );
    let label = Rect::from_min_max(
        outer.left_top(),
        Pos2::new(outer.left() + TIMELINE_LABEL_W, add_row.top()),
    );
    let scrollbar = Rect::from_min_max(
        Pos2::new(outer.left() + TIMELINE_LABEL_W, add_row.bottom()),
        outer.right_bottom(),
    );
    TimelineRects {
        outer,
        label,
        ruler,
        tracks,
        add_row,
        scrollbar,
    }
}

fn timeline_row_rect(rects: TimelineRects, row: usize) -> Rect {
    let top = rects.tracks.top() + row as f32 * TIMELINE_TRACK_H;
    Rect::from_min_max(
        Pos2::new(rects.tracks.left(), top),
        Pos2::new(rects.tracks.right(), top + TIMELINE_TRACK_H),
    )
}

fn centered_child_rect(parent: Rect, width: f32, height: f32) -> Rect {
    let size = Vec2::new(
        width.min(parent.width()).max(0.0),
        height.min(parent.height()).max(0.0),
    );
    let left = (parent.center().x - size.x * 0.5).clamp(parent.left(), parent.right() - size.x);
    let top = (parent.center().y - size.y * 0.5).clamp(parent.top(), parent.bottom() - size.y);
    Rect::from_min_size(Pos2::new(left, top), size)
}

fn timeline_header_left_width(ui: &Ui, collapsed: bool, zoom_label: &str) -> f32 {
    let title_w = measured_text_width(ui, "TIMELINE", FontId::proportional(10.5));
    if collapsed {
        return title_w + 12.0;
    }

    title_w
        + 8.0
        + kit::TIMELINE_TOOL_ICON_W
        + 4.0
        + measured_text_width(ui, zoom_label, FontId::proportional(11.0)).max(26.0)
        + 4.0
        + kit::TIMELINE_TOOL_ICON_W
        + 4.0
        + 42.0
        + 4.0
        + 58.0
        + 8.0
}

fn timeline_header_right_width(ui: &Ui, timecode_label: &str) -> f32 {
    measured_text_width(ui, timecode_label, FontId::monospace(11.0))
        + 8.0
        + kit::TIMELINE_TRANSPORT_BUTTON_W
        + 8.0
}

fn measured_text_width(ui: &Ui, text: &str, font_id: FontId) -> f32 {
    egui::WidgetText::from(text.to_string())
        .into_galley(ui, Some(egui::TextWrapMode::Extend), f32::INFINITY, font_id)
        .size()
        .x
}

fn timeline_snapping_enabled(ui: &Ui) -> bool {
    ui.input(|input| !input.modifiers.alt)
}

fn timeline_clip_title(clip: &Clip, asset: Option<&Asset>) -> String {
    clip.label
        .as_deref()
        .map(str::trim)
        .filter(|label| !label.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| asset.map(asset_display_name))
        .unwrap_or_else(|| "Clip".to_string())
}

fn marker_label_and_rect(
    marker: &crate::state::Marker,
    row_rect: Rect,
    x: f32,
) -> Option<(&str, Rect)> {
    let label = marker
        .label
        .as_deref()
        .filter(|label| !label.trim().is_empty())?;
    Some((
        label,
        Rect::from_min_size(
            Pos2::new(x + 8.0, row_rect.top() + 7.0),
            Vec2::new(TIMELINE_MARKER_LABEL_W, TIMELINE_MARKER_LABEL_H),
        ),
    ))
}

fn timeline_marker_hit_rect(marker: &crate::state::Marker, row_rect: Rect, x: f32) -> Rect {
    let stem_hit = Rect::from_center_size(
        Pos2::new(x, row_rect.center().y),
        Vec2::new(TIMELINE_MARKER_HIT_W, row_rect.height()),
    );
    if let Some((_, label_rect)) = marker_label_and_rect(marker, row_rect, x) {
        rect_union(stem_hit, label_rect.expand2(Vec2::new(6.0, 4.0)))
    } else {
        stem_hit
    }
}

fn rect_union(a: Rect, b: Rect) -> Rect {
    Rect::from_min_max(
        Pos2::new(a.left().min(b.left()), a.top().min(b.top())),
        Pos2::new(a.right().max(b.right()), a.bottom().max(b.bottom())),
    )
}

fn timeline_clip_rect(clip: &Clip, row_rect: Rect, zoom: f32, scroll_x: f32) -> Rect {
    let x1 = time_to_timeline_x(clip.start_time, row_rect.left(), zoom, scroll_x);
    let x2 = time_to_timeline_x(clip.end_time(), row_rect.left(), zoom, scroll_x);
    let y = row_rect.top() + TIMELINE_CLIP_Y_PAD;
    Rect::from_min_max(
        Pos2::new(x1, y),
        Pos2::new(
            x2.max(x1 + TIMELINE_MIN_CLIP_W),
            y + TIMELINE_CLIP_H.min(row_rect.height() - TIMELINE_CLIP_Y_PAD * 2.0),
        ),
    )
}

fn time_to_timeline_x(time: f64, left: f32, zoom: f32, scroll_x: f32) -> f32 {
    left + time as f32 * zoom - scroll_x
}

fn timeline_zoom_bounds(duration: f32, viewport_w: f32, fps: f32) -> (f32, f32) {
    let duration = duration.max(0.01);
    let min_zoom = (viewport_w / duration).max(TIMELINE_MIN_ZOOM_FLOOR);
    let max_zoom = (fps.max(1.0) * TIMELINE_MAX_PX_PER_FRAME).max(min_zoom);
    (min_zoom, max_zoom)
}

fn nice_timeline_step(target_seconds: f64) -> f64 {
    const STEPS: &[f64] = &[0.5, 1.0, 2.0, 5.0, 10.0, 15.0, 30.0, 60.0, 120.0, 300.0];
    STEPS
        .iter()
        .copied()
        .find(|step| *step >= target_seconds)
        .unwrap_or(*STEPS.last().unwrap())
}

fn timeline_ruler_label(seconds: f64) -> String {
    let total_seconds = seconds.round().max(0.0) as u64;
    let minutes = total_seconds / 60;
    let seconds = total_seconds % 60;
    format!("{minutes}:{seconds:02}")
}

fn track_color(track_type: TrackType) -> Color32 {
    match track_type {
        TrackType::Video => kit::VIDEO,
        TrackType::Audio => kit::AUDIO,
        TrackType::Marker => kit::MARKER,
    }
}

fn build_audio_playback_items(
    project: &Project,
    project_root: &Path,
    engine: &AudioPlaybackEngine,
    sample_cache: &Arc<Mutex<HashMap<Uuid, Arc<Vec<f32>>>>>,
    failure_cache: &Arc<Mutex<HashMap<Uuid, String>>>,
    allow_decode: bool,
) -> (Vec<PlaybackItem>, Vec<Uuid>) {
    let mut track_types = HashMap::new();
    let mut track_volumes = HashMap::new();
    for track in project.tracks.iter() {
        track_types.insert(track.id, track.track_type);
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

        let known_failure = failure_cache
            .lock()
            .ok()
            .map(|failures| failures.contains_key(&asset.id))
            .unwrap_or(false);
        if known_failure {
            continue;
        }

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
                    if let Ok(mut failures) = failure_cache.lock() {
                        failures.insert(asset.id, err);
                    }
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
        let gain = (track_volume * clip.volume).max(0.0);

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
    project: &Project,
    project_root: &Path,
) -> Vec<(Uuid, PathBuf)> {
    let mut track_types = HashMap::new();
    for track in project.tracks.iter() {
        track_types.insert(track.id, track.track_type);
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
    targets: Vec<(Uuid, PathBuf)>,
    decode_config: AudioDecodeConfig,
    sample_cache: Arc<Mutex<HashMap<Uuid, Arc<Vec<f32>>>>>,
    in_flight: Arc<Mutex<HashSet<Uuid>>>,
    failure_cache: Arc<Mutex<HashMap<Uuid, String>>>,
) {
    for (asset_id, source_path) in targets {
        let known_failure = failure_cache
            .lock()
            .ok()
            .map(|failures| failures.contains_key(&asset_id))
            .unwrap_or(false);
        if known_failure {
            continue;
        }

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
        let failure_cache = Arc::clone(&failure_cache);

        std::thread::spawn(move || {
            let result = decode_audio_to_f32(&source_path, decode_config);

            match result {
                Ok(decoded) => {
                    let samples = Arc::new(decoded.samples);
                    if let Ok(mut cache) = sample_cache.lock() {
                        cache.insert(asset_id, Arc::clone(&samples));
                    }
                }
                Err(err) => {
                    let first_failure = failure_cache
                        .lock()
                        .ok()
                        .map(|mut failures| failures.insert(asset_id, err.clone()).is_none())
                        .unwrap_or(false);
                    if first_failure {
                        eprintln!(
                            "[AUDIO WARN] Playback decode skipped asset_id={} err={}",
                            asset_id, err
                        );
                    }
                }
            }

            if let Ok(mut inflight) = in_flight.lock() {
                inflight.remove(&asset_id);
            }
        });
    }
}

fn timeline_hit(
    pos: Pos2,
    rects: TimelineRects,
    tracks: &[crate::state::Track],
    clip_geoms: &[TimelineClipGeom],
    marker_geoms: &[TimelineMarkerGeom],
) -> TimelineHit {
    if rects.ruler.contains(pos) {
        return TimelineHit::Ruler;
    }
    if pos.x < rects.tracks.left() && pos.y >= rects.tracks.top() && pos.y < rects.add_row.top() {
        let row = ((pos.y - rects.tracks.top()) / TIMELINE_TRACK_H)
            .floor()
            .max(0.0) as usize;
        return tracks
            .get(row)
            .map(|track| TimelineHit::TrackLabel(track.id))
            .unwrap_or(TimelineHit::Empty);
    }
    for geom in clip_geoms.iter().rev() {
        let hit_rect = geom.rect.expand2(Vec2::new(TIMELINE_HANDLE_W, 0.0));
        if !hit_rect.contains(pos) {
            continue;
        }
        if (pos.x - geom.rect.left()).abs() <= TIMELINE_HANDLE_W {
            return TimelineHit::ClipLeftEdge(geom.clip_id);
        }
        if (pos.x - geom.rect.right()).abs() <= TIMELINE_HANDLE_W {
            return TimelineHit::ClipRightEdge(geom.clip_id);
        }
        if geom.rect.contains(pos) {
            return TimelineHit::ClipBody(geom.clip_id);
        }
    }
    for geom in marker_geoms.iter().rev() {
        if geom.hit_rect.contains(pos) {
            return TimelineHit::Marker(geom.marker_id);
        }
    }
    if rects.tracks.contains(pos) {
        TimelineHit::EmptyTrack
    } else {
        TimelineHit::Empty
    }
}

fn timeline_track_at_pos<'a>(
    pos: Pos2,
    rects: TimelineRects,
    tracks: &'a [crate::state::Track],
) -> Option<&'a crate::state::Track> {
    if !rects.tracks.contains(pos) {
        return None;
    }
    let row = ((pos.y - rects.tracks.top()) / TIMELINE_TRACK_H)
        .floor()
        .max(0.0) as usize;
    tracks.get(row)
}

fn paint_clip_thumbnail_strip(painter: &egui::Painter, rect: Rect, tiles: &[TimelineThumbTile]) {
    if tiles.is_empty() {
        return;
    }
    let clip_painter = painter.with_clip_rect(rect.shrink(1.0));
    let tile_w = (rect.width() / tiles.len() as f32).max(1.0);
    for (index, tile) in tiles.iter().enumerate() {
        let x = rect.left() + index as f32 * tile_w;
        if x > rect.right() {
            break;
        }
        let image_bounds = Rect::from_min_max(
            Pos2::new(x, rect.top()),
            Pos2::new((x + tile_w).min(rect.right()), rect.bottom()),
        );
        let scale = (image_bounds.width() / tile.size.x)
            .max(image_bounds.height() / tile.size.y)
            .max(0.01);
        let image_rect = Rect::from_center_size(image_bounds.center(), tile.size * scale);
        clip_painter.image(
            tile.texture_id,
            image_rect,
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
            Color32::from_white_alpha(145),
        );
    }
}

fn paint_clip_waveform(painter: &egui::Painter, rect: Rect, clip: &Clip, cache: &PeakCache) {
    let Some(level) = cache.levels.first() else {
        return;
    };
    if level.peaks.is_empty() || rect.width() <= 1.0 {
        return;
    }
    let sample_rate = cache.sample_rate.max(1) as f64;
    let start_frame = (clip.trim_in_seconds.max(0.0) * sample_rate).floor() as usize;
    let end_frame = ((clip.trim_in_seconds + clip.duration).max(0.0) * sample_rate).ceil() as usize;
    let start_index = start_frame / level.block_size.max(1);
    let end_index = (end_frame / level.block_size.max(1))
        .min(level.peaks.len())
        .max(start_index + 1);
    if start_index >= level.peaks.len() {
        return;
    }
    let peaks = &level.peaks[start_index..end_index];
    let width = rect.width().round().max(1.0) as usize;
    let step = peaks.len() as f32 / width as f32;
    let center_y = rect.center().y;
    let amp = rect.height() * 0.44;
    let clip_painter = painter.with_clip_rect(rect);
    for x in 0..width {
        let start = (x as f32 * step).floor() as usize;
        let end = ((x + 1) as f32 * step).ceil() as usize;
        let end = end.min(peaks.len()).max(start + 1);
        let mut min = i16::MAX;
        let mut max = i16::MIN;
        for peak in &peaks[start..end] {
            min = min.min(peak.min_l.min(peak.min_r));
            max = max.max(peak.max_l.max(peak.max_r));
        }
        let min = min as f32 / i16::MAX as f32;
        let max = max as f32 / i16::MAX as f32;
        let y1 = center_y - max * amp;
        let y2 = center_y - min * amp;
        let px = rect.left() + x as f32;
        clip_painter.line_segment(
            [Pos2::new(px, y1), Pos2::new(px, y2)],
            Stroke::new(1.0, Color32::from_gray(158)),
        );
    }
}

fn paint_dashed_timeline_button(
    painter: &egui::Painter,
    rect: Rect,
    label: &str,
    color: Color32,
    hovered: bool,
) {
    painter.rect_filled(
        rect,
        4.0,
        if hovered {
            color.gamma_multiply(0.16)
        } else {
            Color32::TRANSPARENT
        },
    );
    painter.rect_stroke(
        rect,
        4.0,
        Stroke::new(1.0, color.gamma_multiply(if hovered { 0.8 } else { 0.45 })),
        egui::StrokeKind::Inside,
    );
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        FontId::proportional(10.5),
        if hovered { color } else { kit::TEXT_DIM },
    );
}

fn parse_hex_color(value: &str) -> Option<Color32> {
    let value = value.trim().trim_start_matches('#');
    if value.len() != 6 {
        return None;
    }
    let rgb = u32::from_str_radix(value, 16).ok()?;
    Some(Color32::from_rgb(
        ((rgb >> 16) & 0xff) as u8,
        ((rgb >> 8) & 0xff) as u8,
        (rgb & 0xff) as u8,
    ))
}

fn color_to_hex(color: Color32) -> String {
    let [r, g, b, _] = color.to_srgba_unmultiplied();
    format!("#{r:02x}{g:02x}{b:02x}")
}

fn previous_frame_time(current: f64, fps: f64) -> f64 {
    let fps = fps.max(1.0);
    let frame = (current * fps).round() - 1.0;
    (frame.max(0.0)) / fps
}

fn next_frame_time(current: f64, duration: f64, fps: f64) -> f64 {
    let fps = fps.max(1.0);
    let frame = (current * fps).round() + 1.0;
    (frame / fps).min(duration)
}

fn asset_row(
    ui: &mut Ui,
    asset: &Asset,
    selected: bool,
    thumbnail: Option<(egui::TextureId, Vec2)>,
) -> egui::Response {
    let accent = asset_accent(asset);
    kit::draw_accent_row(ui, ASSET_ROW_H, selected, accent, |ui, content_rect| {
        let thumb_rect = Rect::from_min_size(
            Pos2::new(
                content_rect.left(),
                content_rect.center().y - ASSET_ROW_THUMBNAIL_SIZE.y * 0.5,
            ),
            ASSET_ROW_THUMBNAIL_SIZE,
        );
        paint_asset_thumbnail(ui, thumb_rect, asset, accent, thumbnail);

        let text_left = thumb_rect.right() + ASSET_ROW_TEXT_GAP;
        let text_width = (content_rect.right() - text_left).max(24.0);
        paint_truncated_row_text_top(
            ui,
            Pos2::new(text_left, thumb_rect.top()),
            kit::value(asset_display_name(asset)),
            12.0,
            text_width,
            kit::TEXT,
        );
        paint_truncated_row_text_bottom(
            ui,
            Pos2::new(text_left, thumb_rect.bottom()),
            kit::caption(asset_row_subtitle(asset)),
            11.0,
            text_width,
            kit::TEXT_MUTED,
        );
    })
}

fn paint_truncated_row_text_top(
    ui: &mut Ui,
    pos: Pos2,
    text: RichText,
    font_size: f32,
    max_width: f32,
    fallback_color: Color32,
) -> Vec2 {
    let font_id = FontId::proportional(font_size);
    let galley = egui::WidgetText::from(text).into_galley(
        ui,
        Some(egui::TextWrapMode::Truncate),
        max_width,
        font_id,
    );
    let size = galley.size();
    ui.painter().galley(pos, galley, fallback_color);
    size
}

fn paint_truncated_row_text_bottom(
    ui: &mut Ui,
    bottom_left: Pos2,
    text: RichText,
    font_size: f32,
    max_width: f32,
    fallback_color: Color32,
) -> Vec2 {
    let font_id = FontId::proportional(font_size);
    let galley = egui::WidgetText::from(text).into_galley(
        ui,
        Some(egui::TextWrapMode::Truncate),
        max_width,
        font_id,
    );
    let size = galley.size();
    ui.painter().galley(
        Pos2::new(bottom_left.x, bottom_left.y - size.y),
        galley,
        fallback_color,
    );
    size
}

fn paint_asset_thumbnail(
    ui: &mut Ui,
    rect: Rect,
    asset: &Asset,
    accent: Color32,
    thumbnail: Option<(egui::TextureId, Vec2)>,
) {
    ui.painter()
        .rect_filled(rect, kit::field_radius(), kit::FIELD_BG);
    ui.painter().rect_stroke(
        rect,
        kit::field_radius(),
        Stroke::new(1.0, kit::BORDER_SOFT),
        egui::StrokeKind::Inside,
    );

    if let Some((texture_id, size)) = thumbnail {
        let image_bounds = rect.shrink(ASSET_THUMBNAIL_IMAGE_INSET);
        let scale = (image_bounds.width() / size.x)
            .min(image_bounds.height() / size.y)
            .max(0.01);
        let image_rect = Rect::from_center_size(image_bounds.center(), size * scale);
        ui.painter().image(
            texture_id,
            image_rect,
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
            Color32::WHITE,
        );
        ui.painter().rect_stroke(
            rect,
            kit::field_radius(),
            Stroke::new(1.0, accent.gamma_multiply(0.7)),
            egui::StrokeKind::Inside,
        );
        return;
    }

    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        asset_icon(asset),
        FontId::proportional(10.5),
        accent,
    );
}

fn asset_row_subtitle(asset: &Asset) -> String {
    let mut parts = vec![asset_kind_label(&asset.kind).to_string()];
    if let Some(duration) = asset.duration_seconds {
        parts.push(format_duration(duration));
    }
    parts.join("  ")
}

fn asset_icon(asset: &Asset) -> &'static str {
    match asset.kind {
        AssetKind::Video { .. } | AssetKind::GenerativeVideo { .. } => "VID",
        AssetKind::Image { .. } | AssetKind::GenerativeImage { .. } => "IMG",
        AssetKind::Audio { .. } | AssetKind::GenerativeAudio { .. } => "AUD",
    }
}

fn asset_accent(asset: &Asset) -> Color32 {
    match asset.kind {
        AssetKind::Video { .. } | AssetKind::GenerativeVideo { .. } => kit::VIDEO,
        AssetKind::Image { .. } | AssetKind::GenerativeImage { .. } => kit::IMAGE,
        AssetKind::Audio { .. } | AssetKind::GenerativeAudio { .. } => kit::AUDIO,
    }
}

fn asset_kind_label(kind: &AssetKind) -> &'static str {
    match kind {
        AssetKind::Video { .. } => "Video",
        AssetKind::Image { .. } => "Image",
        AssetKind::Audio { .. } => "Audio",
        AssetKind::GenerativeVideo { .. } => "Generative Video",
        AssetKind::GenerativeImage { .. } => "Generative Image",
        AssetKind::GenerativeAudio { .. } => "Generative Audio",
    }
}

fn asset_source_label(asset: &Asset) -> Option<String> {
    match &asset.kind {
        AssetKind::Video { path } | AssetKind::Image { path } | AssetKind::Audio { path } => {
            Some(path_label(path))
        }
        AssetKind::GenerativeVideo { folder, .. }
        | AssetKind::GenerativeImage { folder, .. }
        | AssetKind::GenerativeAudio { folder, .. } => Some(path_label(folder)),
    }
}

fn asset_thumbnail_candidates(project_root: &Path, asset: &Asset) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    match &asset.kind {
        AssetKind::Image { path } => {
            candidates.push(project_root.join(path));
        }
        AssetKind::GenerativeImage {
            folder,
            active_version,
        } => {
            if let Some(path) = resolve_generative_file(
                project_root,
                folder,
                active_version.as_deref(),
                IMAGE_EXTENSIONS,
            ) {
                candidates.push(path);
            }
        }
        AssetKind::Video { .. } | AssetKind::GenerativeVideo { .. } => {}
        AssetKind::Audio { .. } | AssetKind::GenerativeAudio { .. } => return candidates,
    }

    if asset.is_visual() {
        if let Some(path) = cached_asset_thumbnail(project_root, asset.id) {
            candidates.push(path);
        }
    }
    candidates
}

fn cached_asset_thumbnail(project_root: &Path, asset_id: Uuid) -> Option<PathBuf> {
    let dir = project_root
        .join(".cache")
        .join("thumbnails")
        .join(asset_id.to_string());
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file()
                && path
                    .metadata()
                    .map(|metadata| metadata.len() > 1024)
                    .unwrap_or(false)
                && path
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .is_some_and(|ext| {
                        ["jpg", "jpeg", "png", "webp"]
                            .iter()
                            .any(|allowed| allowed.eq_ignore_ascii_case(ext))
                    })
        })
        .collect();
    entries.sort();
    entries.into_iter().next()
}

fn resolve_generative_file(
    project_root: &Path,
    folder: &Path,
    active_version: Option<&str>,
    extensions: &[&str],
) -> Option<PathBuf> {
    let folder_path = project_root.join(folder);
    if let Some(version) = active_version {
        for ext in extensions {
            let candidate = folder_path.join(format!("{version}.{ext}"));
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }

    let mut entries: Vec<PathBuf> = std::fs::read_dir(folder_path)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file()
                && path
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .is_some_and(|ext| {
                        extensions
                            .iter()
                            .any(|allowed| allowed.eq_ignore_ascii_case(ext))
                    })
        })
        .collect();
    entries.sort();
    entries.into_iter().next()
}

fn load_thumbnail_image(path: &Path) -> Option<(ColorImage, Vec2)> {
    let image = image::open(path).ok()?.thumbnail(96, 96).to_rgba8();
    let size = [image.width() as usize, image.height() as usize];
    let display_size = Vec2::new(size[0] as f32, size[1] as f32);
    let color_image = ColorImage::from_rgba_unmultiplied(size, image.as_raw());
    Some((color_image, display_size))
}

#[derive(Clone, Copy, Debug)]
enum ProviderInputAction {
    MoveUp(usize),
    MoveDown(usize),
    Delete(usize),
}

struct ProviderBuilderSave {
    entry: ProviderEntry,
    manifest: ProviderManifest,
    provider_path: PathBuf,
    manifest_path: PathBuf,
}

impl Default for ProviderBuilderState {
    fn default() -> Self {
        Self::from_entry(None, crate::core::provider_store::default_provider_entry())
    }
}

impl ProviderBuilderState {
    fn from_path(path: &Path) -> Self {
        let Some(json) = crate::core::provider_store::read_provider_file(path) else {
            let mut state = Self::default();
            state.source_path = Some(path.to_path_buf());
            state.error = Some(format!("Failed to read provider {}", path.display()));
            return state;
        };
        match serde_json::from_str::<ProviderEntry>(&json) {
            Ok(entry) => Self::from_entry(Some(path.to_path_buf()), entry),
            Err(err) => {
                let mut state = Self::default();
                state.source_path = Some(path.to_path_buf());
                state.error = Some(format!("Failed to parse provider JSON: {err}"));
                state
            }
        }
    }

    fn from_entry(source_path: Option<PathBuf>, entry: ProviderEntry) -> Self {
        let (base_url, workflow_path, manifest_path) = match &entry.connection {
            ProviderConnection::ComfyUi {
                base_url,
                workflow_path,
                manifest_path,
            } => (
                base_url.clone(),
                workflow_path.as_ref().map(PathBuf::from),
                manifest_path.as_ref().map(PathBuf::from),
            ),
            ProviderConnection::CustomHttp { base_url, .. } => (base_url.clone(), None, None),
        };

        let (workflow_nodes, workflow_error) = workflow_path
            .as_ref()
            .map(|path| match load_workflow_nodes_resolved(path) {
                Ok(nodes) => (nodes, None),
                Err(err) => (Vec::new(), Some(err)),
            })
            .unwrap_or_else(|| (Vec::new(), None));

        let mut state = Self {
            source_path,
            provider_id: entry.id,
            provider_name: entry.name.clone(),
            output_type: entry.output_type,
            base_url,
            workflow_path,
            manifest_path: manifest_path.clone(),
            workflow_nodes,
            workflow_error,
            workflow_search: String::new(),
            selected_node_id: None,
            output_key: default_output_key(entry.output_type).to_string(),
            output_tag: String::new(),
            output_node: None,
            inputs: entry
                .inputs
                .iter()
                .map(ProviderBuilderInput::from_provider_input)
                .collect(),
            tab: ProviderBuilderTab::Inputs,
            error: None,
        };

        if let Some(path) = manifest_path {
            match load_provider_manifest_resolved(&path) {
                Ok(manifest) => state.apply_manifest(manifest),
                Err(err) => state.error = Some(err),
            }
        }
        if state.workflow_nodes.is_empty() {
            if let Some(path) = state.workflow_path.as_ref() {
                match load_workflow_nodes_resolved(path) {
                    Ok(nodes) => {
                        state.workflow_nodes = nodes;
                        state.workflow_error = None;
                    }
                    Err(err) => state.workflow_error = Some(err),
                }
            }
        }
        state
    }

    fn filtered_nodes(&self) -> Vec<crate::core::comfyui_workflow::ComfyWorkflowNode> {
        let query = self.workflow_search.trim().to_lowercase();
        if query.is_empty() {
            return self.workflow_nodes.clone();
        }
        self.workflow_nodes
            .iter()
            .filter(|node| {
                node.id.to_lowercase().contains(&query)
                    || node.class_type.to_lowercase().contains(&query)
                    || node
                        .title
                        .as_ref()
                        .is_some_and(|title| title.to_lowercase().contains(&query))
                    || node
                        .inputs
                        .iter()
                        .any(|input| input.to_lowercase().contains(&query))
            })
            .cloned()
            .collect()
    }

    fn selected_node(&self) -> Option<crate::core::comfyui_workflow::ComfyWorkflowNode> {
        let selected_id = self.selected_node_id.as_ref()?;
        self.workflow_nodes
            .iter()
            .find(|node| &node.id == selected_id)
            .cloned()
    }

    fn apply_manifest(&mut self, manifest: ProviderManifest) {
        match manifest {
            ProviderManifest::ComfyUi {
                name,
                output_type,
                workflow,
                inputs,
                output,
                ..
            } => {
                if let Some(name) = name {
                    self.provider_name = name;
                }
                self.output_type = output_type;
                self.workflow_path = Some(PathBuf::from(workflow.workflow_path));
                self.output_key = if output.selector.input_key.trim().is_empty() {
                    default_output_key(output_type).to_string()
                } else {
                    output.selector.input_key
                };
                self.output_tag = output.selector.tag.unwrap_or_default();
                self.output_node = Some(ProviderOutputNodeDraft {
                    class_type: output.selector.class_type,
                    title: output.selector.title,
                });
                self.inputs = inputs
                    .into_iter()
                    .map(ProviderBuilderInput::from_manifest_input)
                    .collect();
            }
            ProviderManifest::CustomHttp {
                name,
                output_type,
                inputs,
                ..
            } => {
                if let Some(name) = name {
                    self.provider_name = name;
                }
                self.output_type = output_type;
                self.inputs = inputs
                    .into_iter()
                    .map(ProviderBuilderInput::from_custom_http_input)
                    .collect();
                self.error = Some(
                    "Loaded a Custom HTTP manifest. Saving from this builder writes ComfyUI settings."
                        .to_string(),
                );
            }
        }
    }

    fn output_status_label(&self) -> String {
        self.output_node
            .as_ref()
            .map(|node| {
                format!(
                    "Output: {} ({})",
                    node.title.clone().unwrap_or_else(|| "Untitled".to_string()),
                    node.class_type
                )
            })
            .unwrap_or_else(|| "Output: Not set".to_string())
    }

    fn workflow_path_display(&self) -> String {
        self.workflow_path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "Choose a workflow JSON".to_string())
    }

    fn manifest_path_display(&self) -> String {
        self.manifest_path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| {
                self.workflow_path
                    .as_ref()
                    .map(|path| derive_manifest_path(path).display().to_string())
                    .unwrap_or_else(|| "Derived from workflow on save".to_string())
            })
    }

    fn build_save_payload(&self) -> Result<ProviderBuilderSave, String> {
        let workflow_path = self
            .workflow_path
            .clone()
            .ok_or_else(|| "Select a workflow first.".to_string())?;
        let provider_name = self.provider_name.trim();
        if provider_name.is_empty() {
            return Err("Provider name is required.".to_string());
        }
        let base_url = self.base_url.trim();
        if base_url.is_empty() {
            return Err("Base URL is required.".to_string());
        }
        let output_node = self
            .output_node
            .clone()
            .ok_or_else(|| "Select an output node.".to_string())?;
        let output_key = self.output_key.trim();
        if output_key.is_empty() {
            return Err("Output key is required.".to_string());
        }

        let mut manifest_inputs = Vec::new();
        let mut provider_inputs = Vec::new();
        for input in &self.inputs {
            let input_type = parse_provider_input_type(input)?;
            let default = parse_provider_default_value(&input_type, &input.default_text)?;
            let tag = input.tag.trim();
            let selector = NodeSelector {
                tag: if tag.is_empty() {
                    None
                } else {
                    Some(tag.to_string())
                },
                class_type: input.selector.class_type.clone(),
                input_key: input.selector.input_key.clone(),
                title: input.selector.title.clone(),
            };
            if selector.class_type.trim().is_empty() || selector.input_key.trim().is_empty() {
                return Err(format!(
                    "Input '{}' needs a workflow binding. Select a node and expose the input again.",
                    input.name
                ));
            }

            let input_ui = build_provider_input_ui(input);
            manifest_inputs.push(ManifestInput {
                name: input.name.clone(),
                label: input.label.clone(),
                input_type: input_type.clone(),
                required: input.required,
                default: default.clone(),
                ui: input_ui.clone(),
                bind: InputBinding {
                    selector,
                    transform: None,
                },
            });
            provider_inputs.push(ProviderInputField {
                name: input.name.clone(),
                label: input.label.clone(),
                input_type,
                required: input.required,
                default,
                ui: input_ui,
            });
        }

        let output_tag = self.output_tag.trim();
        let output_selector = NodeSelector {
            tag: if output_tag.is_empty() {
                None
            } else {
                Some(output_tag.to_string())
            },
            class_type: output_node.class_type,
            input_key: output_key.to_string(),
            title: output_node.title,
        };

        let manifest_path = self
            .manifest_path
            .clone()
            .unwrap_or_else(|| derive_manifest_path(&workflow_path));
        let provider_path = self.source_path.clone().unwrap_or_else(|| {
            crate::core::provider_store::provider_path_for_entry(&ProviderEntry {
                id: self.provider_id,
                name: provider_name.to_string(),
                output_type: self.output_type,
                inputs: Vec::new(),
                connection: ProviderConnection::ComfyUi {
                    base_url: base_url.to_string(),
                    workflow_path: Some(workflow_path.display().to_string()),
                    manifest_path: Some(manifest_path.display().to_string()),
                },
            })
        });

        let workflow_path_string = workflow_path.display().to_string();
        let manifest_path_string = manifest_path.display().to_string();
        let manifest = ProviderManifest::ComfyUi {
            schema_version: 1,
            name: Some(provider_name.to_string()),
            output_type: self.output_type,
            workflow: ComfyWorkflowRef {
                workflow_path: workflow_path_string.clone(),
                workflow_hash: None,
            },
            inputs: manifest_inputs,
            output: ComfyOutputSelector {
                selector: output_selector,
                index: None,
            },
        };
        let entry = ProviderEntry {
            id: self.provider_id,
            name: provider_name.to_string(),
            output_type: self.output_type,
            inputs: provider_inputs,
            connection: ProviderConnection::ComfyUi {
                base_url: base_url.to_string(),
                workflow_path: Some(workflow_path_string),
                manifest_path: Some(manifest_path_string),
            },
        };

        Ok(ProviderBuilderSave {
            entry,
            manifest,
            provider_path,
            manifest_path,
        })
    }
}

impl ProviderBuilderInput {
    fn blank() -> Self {
        Self {
            name: "input".to_string(),
            label: "Input".to_string(),
            input_type_key: "text".to_string(),
            required: false,
            default_text: String::new(),
            enum_options: String::new(),
            tag: String::new(),
            multiline: false,
            selector: ProviderNodeSelectorDraft {
                class_type: String::new(),
                input_key: String::new(),
                title: None,
            },
        }
    }

    fn from_node(node: &crate::core::comfyui_workflow::ComfyWorkflowNode, input_key: &str) -> Self {
        Self {
            name: input_key.to_string(),
            label: friendly_provider_label(input_key),
            input_type_key: "text".to_string(),
            required: false,
            default_text: String::new(),
            enum_options: String::new(),
            tag: String::new(),
            multiline: false,
            selector: ProviderNodeSelectorDraft {
                class_type: node.class_type.clone(),
                input_key: input_key.to_string(),
                title: node.title.clone(),
            },
        }
    }

    fn from_provider_input(input: &ProviderInputField) -> Self {
        let (input_type_key, enum_options) = provider_input_type_to_key(&input.input_type);
        Self {
            name: input.name.clone(),
            label: input.label.clone(),
            input_type_key,
            required: input.required,
            default_text: default_value_to_text(input.default.as_ref()),
            enum_options,
            tag: String::new(),
            multiline: input.ui.as_ref().is_some_and(|ui| ui.multiline),
            selector: ProviderNodeSelectorDraft {
                class_type: String::new(),
                input_key: input.name.clone(),
                title: None,
            },
        }
    }

    fn from_manifest_input(input: ManifestInput) -> Self {
        let (input_type_key, enum_options) = provider_input_type_to_key(&input.input_type);
        Self {
            name: input.name,
            label: input.label,
            input_type_key,
            required: input.required,
            default_text: default_value_to_text(input.default.as_ref()),
            enum_options,
            tag: input.bind.selector.tag.unwrap_or_default(),
            multiline: input.ui.as_ref().is_some_and(|ui| ui.multiline),
            selector: ProviderNodeSelectorDraft {
                class_type: input.bind.selector.class_type,
                input_key: input.bind.selector.input_key,
                title: input.bind.selector.title,
            },
        }
    }

    fn from_custom_http_input(input: crate::state::CustomHttpInput) -> Self {
        let (input_type_key, enum_options) = provider_input_type_to_key(&input.input_type);
        Self {
            name: input.name,
            label: input.label,
            input_type_key,
            required: input.required,
            default_text: default_value_to_text(input.default.as_ref()),
            enum_options,
            tag: String::new(),
            multiline: input.ui.as_ref().is_some_and(|ui| ui.multiline),
            selector: ProviderNodeSelectorDraft {
                class_type: String::new(),
                input_key: String::new(),
                title: None,
            },
        }
    }
}

struct ProviderFileSummary {
    name: String,
    subtitle: String,
    output_type: Option<ProviderOutputType>,
}

fn provider_row(
    ui: &mut Ui,
    _path: &Path,
    summary: &ProviderFileSummary,
    selected: bool,
) -> egui::Response {
    let accent = summary
        .output_type
        .map(provider_output_color)
        .unwrap_or(kit::AUDIO);
    kit::draw_accent_row(ui, 52.0, selected, accent, |ui, rect| {
        paint_text_button_row(ui, rect, &summary.name, &summary.subtitle);
    })
}

fn paint_text_button_row(ui: &mut Ui, rect: Rect, title: &str, subtitle: &str) {
    let text_width = rect.width().max(24.0);
    paint_truncated_row_text_top(
        ui,
        Pos2::new(rect.left(), rect.top() + 2.0),
        kit::value(title),
        12.0,
        text_width,
        kit::TEXT,
    );
    paint_truncated_row_text_bottom(
        ui,
        Pos2::new(rect.left(), rect.bottom() - 2.0),
        kit::caption(subtitle),
        11.0,
        text_width,
        kit::TEXT_MUTED,
    );
}

fn provider_file_summary(path: &Path) -> ProviderFileSummary {
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("provider.json")
        .to_string();
    let Ok(text) = std::fs::read_to_string(path) else {
        return ProviderFileSummary {
            name: file_name,
            subtitle: "Unreadable provider file".to_string(),
            output_type: None,
        };
    };
    let Ok(entry) = serde_json::from_str::<ProviderEntry>(&text) else {
        return ProviderFileSummary {
            name: file_name,
            subtitle: "Invalid provider JSON".to_string(),
            output_type: None,
        };
    };
    ProviderFileSummary {
        name: entry.name,
        subtitle: format!(
            "{}  {}",
            provider_output_type_label(entry.output_type),
            path_label(path)
        ),
        output_type: Some(entry.output_type),
    }
}

fn provider_output_color(output_type: ProviderOutputType) -> Color32 {
    match output_type {
        ProviderOutputType::Image => kit::IMAGE,
        ProviderOutputType::Video => kit::VIDEO,
        ProviderOutputType::Audio => kit::AUDIO,
    }
}

fn provider_output_type_label(output_type: ProviderOutputType) -> &'static str {
    match output_type {
        ProviderOutputType::Image => "Image",
        ProviderOutputType::Video => "Video",
        ProviderOutputType::Audio => "Audio",
    }
}

fn provider_output_type_field(ui: &mut Ui, label: &str, value: &mut ProviderOutputType) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = kit::FIELD_LABEL_GAP;
        kit::field_label(ui, label);
        let width = ui.available_width();
        kit::configure_field_widget_style(ui, width);
        let combo_id = ui.next_auto_id();
        ui.skip_ahead_auto_ids(1);
        egui::ComboBox::from_id_salt(combo_id)
            .width(width)
            .selected_text(provider_output_type_label(*value))
            .show_ui(ui, |ui| {
                ui.selectable_value(value, ProviderOutputType::Image, "Image");
                ui.selectable_value(value, ProviderOutputType::Video, "Video");
                ui.selectable_value(value, ProviderOutputType::Audio, "Audio");
            });
    });
}

fn workflow_node_row(
    ui: &mut Ui,
    node: &crate::core::comfyui_workflow::ComfyWorkflowNode,
    selected: bool,
) -> egui::Response {
    kit::draw_accent_row(ui, 54.0, selected, kit::IMAGE, |ui, rect| {
        let title = node.title.as_deref().unwrap_or("Untitled");
        let subtitle = format!("{}  Node {}", node.class_type, node.id);
        paint_text_button_row(ui, rect, title, &subtitle);
    })
}

fn provider_builder_input_editor(
    ui: &mut Ui,
    index: usize,
    len: usize,
    input: &mut ProviderBuilderInput,
    action: &mut Option<ProviderInputAction>,
) {
    kit::sunken_frame().show(ui, |ui| {
        StripBuilder::new(ui)
            .clip(true)
            .size(Size::remainder().at_least(120.0))
            .size(Size::exact(kit::FORM_ROW_GAP))
            .size(Size::remainder().at_least(120.0))
            .horizontal(|mut strip| {
                strip.cell(|ui| {
                    kit::labeled_text_field(ui, "Name", &mut input.name);
                });
                strip.empty();
                strip.cell(|ui| {
                    kit::labeled_text_field(ui, "Label", &mut input.label);
                });
            });
        ui.add_space(kit::FORM_ROW_GAP);
        StripBuilder::new(ui)
            .clip(true)
            .size(Size::exact(124.0))
            .size(Size::exact(kit::FORM_ROW_GAP))
            .size(Size::remainder().at_least(120.0))
            .horizontal(|mut strip| {
                strip.cell(|ui| {
                    provider_input_type_field(ui, "Type", &mut input.input_type_key);
                });
                strip.empty();
                strip.cell(|ui| {
                    kit::labeled_text_field(ui, "Default", &mut input.default_text);
                });
            });
        ui.add_space(kit::FORM_ROW_GAP);
        if input.input_type_key == "enum" {
            kit::labeled_text_field(ui, "Enum Options", &mut input.enum_options);
            ui.add_space(kit::FORM_ROW_GAP);
        }
        StripBuilder::new(ui)
            .clip(true)
            .size(Size::remainder().at_least(130.0))
            .size(Size::exact(kit::FORM_ROW_GAP))
            .size(Size::remainder().at_least(130.0))
            .horizontal(|mut strip| {
                strip.cell(|ui| {
                    kit::labeled_text_field(ui, "Tag", &mut input.tag);
                });
                strip.empty();
                strip.cell(|ui| {
                    ui.add_space(kit::FIELD_LABEL_H + kit::FIELD_LABEL_GAP);
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut input.required, "Required");
                        if input.input_type_key == "text" {
                            ui.checkbox(&mut input.multiline, "Multiline");
                        } else {
                            input.multiline = false;
                        }
                    });
                });
            });
        ui.add_space(kit::FORM_ROW_GAP);
        ui.horizontal(|ui| {
            ui.add_sized(
                [(ui.available_width() - 178.0).max(80.0), 18.0],
                egui::Label::new(kit::caption(format!(
                    "-> {}.{}",
                    empty_dash(&input.selector.class_type),
                    empty_dash(&input.selector.input_key)
                )))
                .truncate(),
            );
            if kit::field_button(ui, "Up", 42.0).clicked() && index > 0 {
                *action = Some(ProviderInputAction::MoveUp(index));
            }
            if kit::field_button(ui, "Down", 52.0).clicked() && index + 1 < len {
                *action = Some(ProviderInputAction::MoveDown(index));
            }
            if kit::danger_button(ui, "Delete", 66.0).clicked() {
                *action = Some(ProviderInputAction::Delete(index));
            }
        });
    });
}

fn provider_input_type_field(ui: &mut Ui, label: &str, value: &mut String) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = kit::FIELD_LABEL_GAP;
        kit::field_label(ui, label);
        let width = ui.available_width();
        kit::configure_field_widget_style(ui, width);
        let combo_id = ui.next_auto_id();
        ui.skip_ahead_auto_ids(1);
        egui::ComboBox::from_id_salt(combo_id)
            .width(width)
            .selected_text(provider_input_type_label(value))
            .show_ui(ui, |ui| {
                for (key, label) in [
                    ("text", "Text"),
                    ("number", "Number"),
                    ("integer", "Integer"),
                    ("boolean", "Boolean"),
                    ("enum", "Enum"),
                    ("image", "Image"),
                    ("video", "Video"),
                    ("audio", "Audio"),
                ] {
                    ui.selectable_value(value, key.to_string(), label);
                }
            });
    });
}

fn provider_input_type_label(value: &str) -> &'static str {
    match value {
        "number" => "Number",
        "integer" => "Integer",
        "boolean" => "Boolean",
        "enum" => "Enum",
        "image" => "Image",
        "video" => "Video",
        "audio" => "Audio",
        _ => "Text",
    }
}

fn provider_input_type_to_key(input_type: &ProviderInputType) -> (String, String) {
    match input_type {
        ProviderInputType::Text => ("text".to_string(), String::new()),
        ProviderInputType::Number => ("number".to_string(), String::new()),
        ProviderInputType::Integer => ("integer".to_string(), String::new()),
        ProviderInputType::Boolean => ("boolean".to_string(), String::new()),
        ProviderInputType::Enum { options } => ("enum".to_string(), options.join(",")),
        ProviderInputType::Image => ("image".to_string(), String::new()),
        ProviderInputType::Video => ("video".to_string(), String::new()),
        ProviderInputType::Audio => ("audio".to_string(), String::new()),
    }
}

fn parse_provider_input_type(input: &ProviderBuilderInput) -> Result<ProviderInputType, String> {
    match input.input_type_key.as_str() {
        "text" => Ok(ProviderInputType::Text),
        "number" => Ok(ProviderInputType::Number),
        "integer" => Ok(ProviderInputType::Integer),
        "boolean" => Ok(ProviderInputType::Boolean),
        "image" => Ok(ProviderInputType::Image),
        "video" => Ok(ProviderInputType::Video),
        "audio" => Ok(ProviderInputType::Audio),
        "enum" => {
            let options: Vec<String> = input
                .enum_options
                .split(',')
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .collect();
            if options.is_empty() {
                Err(format!(
                    "Enum input '{}' needs at least one option.",
                    input.name
                ))
            } else {
                Ok(ProviderInputType::Enum { options })
            }
        }
        other => Err(format!("Unknown input type: {other}")),
    }
}

fn parse_provider_default_value(
    input_type: &ProviderInputType,
    text: &str,
) -> Result<Option<serde_json::Value>, String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    let value = match input_type {
        ProviderInputType::Text => serde_json::Value::String(trimmed.to_string()),
        ProviderInputType::Number => {
            let parsed = trimmed
                .parse::<f64>()
                .map_err(|_| format!("Invalid number default '{trimmed}'."))?;
            serde_json::Value::Number(
                serde_json::Number::from_f64(parsed)
                    .ok_or_else(|| format!("Invalid number default '{trimmed}'."))?,
            )
        }
        ProviderInputType::Integer => {
            let parsed = trimmed
                .parse::<i64>()
                .map_err(|_| format!("Invalid integer default '{trimmed}'."))?;
            serde_json::Value::Number(parsed.into())
        }
        ProviderInputType::Boolean => {
            let parsed = trimmed
                .parse::<bool>()
                .map_err(|_| format!("Invalid boolean default '{trimmed}'."))?;
            serde_json::Value::Bool(parsed)
        }
        ProviderInputType::Enum { .. } => serde_json::Value::String(trimmed.to_string()),
        ProviderInputType::Image | ProviderInputType::Video | ProviderInputType::Audio => {
            return Ok(None)
        }
    };
    Ok(Some(value))
}

fn build_provider_input_ui(input: &ProviderBuilderInput) -> Option<InputUi> {
    if input.input_type_key == "text" && input.multiline {
        Some(InputUi {
            min: None,
            max: None,
            step: None,
            placeholder: None,
            multiline: true,
            group: None,
            advanced: false,
            unit: None,
        })
    } else {
        None
    }
}

fn default_value_to_text(value: Option<&serde_json::Value>) -> String {
    value
        .map(|value| match value {
            serde_json::Value::String(text) => text.clone(),
            serde_json::Value::Number(number) => number.to_string(),
            serde_json::Value::Bool(flag) => flag.to_string(),
            _ => String::new(),
        })
        .unwrap_or_default()
}

fn derive_manifest_path(workflow_path: &Path) -> PathBuf {
    let mut path = workflow_path.to_path_buf();
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("workflow");
    path.set_file_name(format!("{stem}_manifest.json"));
    path
}

fn load_workflow_nodes_resolved(
    path: &Path,
) -> Result<Vec<crate::core::comfyui_workflow::ComfyWorkflowNode>, String> {
    let resolved = crate::core::paths::resolve_resource_path(path);
    crate::core::comfyui_workflow::load_workflow_nodes(&resolved)
}

fn load_provider_manifest_resolved(path: &Path) -> Result<ProviderManifest, String> {
    let resolved = crate::core::paths::resolve_resource_path(path);
    let text = std::fs::read_to_string(&resolved)
        .map_err(|err| format!("Failed to read manifest {}: {err}", path.display()))?;
    serde_json::from_str::<ProviderManifest>(&text)
        .map_err(|err| format!("Failed to parse manifest {}: {err}", path.display()))
}

fn friendly_provider_label(name: &str) -> String {
    name.replace('_', " ")
        .split_whitespace()
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn default_output_key(output_type: ProviderOutputType) -> &'static str {
    match output_type {
        ProviderOutputType::Image => "images",
        ProviderOutputType::Video => "videos",
        ProviderOutputType::Audio => "audio",
    }
}

fn empty_dash(value: &str) -> &str {
    if value.trim().is_empty() {
        "-"
    } else {
        value
    }
}

fn path_label(path: &Path) -> String {
    let text = path.display().to_string();
    let len = text.chars().count();
    if len > 48 {
        format!(
            "...{}",
            text.chars()
                .skip(len.saturating_sub(45))
                .collect::<String>()
        )
    } else {
        text
    }
}

fn open_path_in_file_manager(path: &Path) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(path)
            .spawn()
            .map_err(|err| format!("Failed to open folder: {err}"))?;
        return Ok(());
    }

    #[cfg(not(target_os = "windows"))]
    {
        let command = if cfg!(target_os = "macos") {
            "open"
        } else {
            "xdg-open"
        };
        std::process::Command::new(command)
            .arg(path)
            .spawn()
            .map_err(|err| format!("Failed to open folder: {err}"))?;
        Ok(())
    }
}

fn generative_output_for_asset(
    project: &Project,
    asset_id: Uuid,
) -> Option<(PathBuf, ProviderOutputType)> {
    let asset = project.find_asset(asset_id)?;
    match &asset.kind {
        AssetKind::GenerativeImage { folder, .. } => {
            Some((folder.clone(), ProviderOutputType::Image))
        }
        AssetKind::GenerativeVideo { folder, .. } => {
            Some((folder.clone(), ProviderOutputType::Video))
        }
        AssetKind::GenerativeAudio { folder, .. } => {
            Some((folder.clone(), ProviderOutputType::Audio))
        }
        _ => None,
    }
}

fn literal_config_input(config: &GenerativeConfig, name: &str) -> Option<serde_json::Value> {
    config.inputs.get(name).and_then(|input| match input {
        InputValue::Literal { value } => Some(value.clone()),
        InputValue::AssetRef { .. } => None,
    })
}

fn seed_field_options_for_provider(provider: &ProviderEntry) -> Vec<(String, String)> {
    provider
        .inputs
        .iter()
        .filter(|input| {
            matches!(
                input.input_type,
                ProviderInputType::Integer | ProviderInputType::Number
            )
        })
        .map(|input| {
            let label = if input.label.trim().is_empty() || input.label == input.name {
                input.name.clone()
            } else {
                format!("{} ({})", input.label, input.name)
            };
            (input.name.clone(), label)
        })
        .collect()
}

fn seed_strategy_label(strategy: SeedStrategy) -> &'static str {
    match strategy {
        SeedStrategy::Increment => "Increment",
        SeedStrategy::Random => "Random",
        SeedStrategy::Keep => "Keep",
    }
}

fn inspector_text_field(ui: &mut Ui, label: &str, value: &mut String) -> bool {
    kit::field_label(ui, label);
    let width = ui.available_width();
    kit::singleline_text_field(ui, value, width).changed()
}

fn inspector_multiline_text_field(
    ui: &mut Ui,
    label: &str,
    value: &mut String,
    options: kit::MultilineTextFieldOptions,
) -> bool {
    kit::field_label(ui, label);
    let width = ui.available_width();
    kit::multiline_text_field(ui, value, width, options).changed()
}

fn inspector_color_field(ui: &mut Ui, label: &str, color: &mut Color32) -> bool {
    kit::field_label(ui, label);
    let width = ui.available_width();
    kit::color_field(ui, color, width).changed()
}

fn inspector_bool_field(ui: &mut Ui, label: &str, value: &mut bool) -> bool {
    kit::field_label(ui, label);
    let before = *value;
    let label = if *value { "On" } else { "Off" };
    let width = ui.available_width();
    if kit::field_button(ui, label, width).clicked() {
        *value = !*value;
    }
    *value != before
}

fn inspector_card(ui: &mut Ui, title: &str, add_contents: impl FnOnce(&mut Ui)) {
    kit::card_frame().show(ui, |ui| {
        let clip_rect = ui.clip_rect();
        let content_width = ui.available_width().max(0.0);
        ui.shrink_clip_rect(clip_rect);
        ui.set_width(content_width);
        ui.set_max_width(content_width);
        kit::field_label(ui, title);
        ui.add_space(kit::FORM_ROW_GAP);
        add_contents(ui);
    });
}

fn inspector_meta_row(ui: &mut Ui, label: &str, value: impl Into<String>) {
    let value = value.into();
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = kit::FORM_ROW_GAP;
        ui.add_sized([62.0, 18.0], egui::Label::new(kit::caption(label)));
        ui.add_sized(
            [ui.available_width(), 18.0],
            egui::Label::new(kit::body(value)).truncate(),
        );
    });
}

const INSPECTOR_NUMERIC_H: f32 = kit::FIELD_H;
const INSPECTOR_NUMERIC_GAP: f32 = kit::FORM_ROW_GAP;

fn inspector_numeric_rect(ui: &mut Ui, width: f32) -> Rect {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, INSPECTOR_NUMERIC_H), Sense::hover());
    rect
}

fn inspector_numeric_pair_rects(ui: &mut Ui) -> (Rect, Rect) {
    kit::paired_field_rects(
        ui,
        kit::FieldPairLayout::default()
            .gap(INSPECTOR_NUMERIC_GAP)
            .height(INSPECTOR_NUMERIC_H)
            .min_column_width(0.0),
    )
}

fn inspector_numeric_field(
    ui: &mut Ui,
    rect: Rect,
    add_control: impl FnOnce(&mut Ui, f32) -> egui::Response,
) -> bool {
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(Layout::left_to_right(Align::Center)),
    );
    child.set_min_size(rect.size());
    child.shrink_clip_rect(rect);
    kit::configure_field_widget_style(&mut child, rect.width());
    add_control(&mut child, rect.width()).changed()
}

fn inspector_drag_f32(ui: &mut Ui, label: &str, value: &mut f32, speed: f64, width: f32) -> bool {
    let rect = inspector_numeric_rect(ui, width);
    inspector_drag_f32_in_rect(ui, rect, label, value, speed)
}

fn inspector_drag_f32_in_rect(
    ui: &mut Ui,
    rect: Rect,
    label: &str,
    value: &mut f32,
    speed: f64,
) -> bool {
    inspector_numeric_field(ui, rect, |ui, width| {
        ui.add_sized(
            [width, INSPECTOR_NUMERIC_H],
            egui::DragValue::new(value)
                .prefix(kit::field_prefix(label))
                .speed(speed),
        )
    })
}

fn inspector_drag_f64(ui: &mut Ui, label: &str, value: &mut f64, speed: f64, width: f32) -> bool {
    let rect = inspector_numeric_rect(ui, width);
    inspector_drag_f64_in_rect(ui, rect, label, value, speed)
}

fn inspector_drag_f64_in_rect(
    ui: &mut Ui,
    rect: Rect,
    label: &str,
    value: &mut f64,
    speed: f64,
) -> bool {
    inspector_numeric_field(ui, rect, |ui, width| {
        ui.add_sized(
            [width, INSPECTOR_NUMERIC_H],
            egui::DragValue::new(value)
                .prefix(kit::field_prefix(label))
                .speed(speed),
        )
    })
}

fn inspector_drag_i64(ui: &mut Ui, label: &str, value: &mut i64, speed: f64, width: f32) -> bool {
    let rect = inspector_numeric_rect(ui, width);
    inspector_drag_i64_in_rect(ui, rect, label, value, speed)
}

fn inspector_drag_i64_in_rect(
    ui: &mut Ui,
    rect: Rect,
    label: &str,
    value: &mut i64,
    speed: f64,
) -> bool {
    inspector_numeric_field(ui, rect, |ui, width| {
        ui.add_sized(
            [width, INSPECTOR_NUMERIC_H],
            egui::DragValue::new(value)
                .prefix(kit::field_prefix(label))
                .speed(speed),
        )
    })
}

fn inspector_drag_u32_in_rect(
    ui: &mut Ui,
    rect: Rect,
    label: &str,
    value: &mut u32,
    speed: f64,
) -> bool {
    inspector_numeric_field(ui, rect, |ui, width| {
        ui.add_sized(
            [width, INSPECTOR_NUMERIC_H],
            egui::DragValue::new(value)
                .prefix(kit::field_prefix(label))
                .speed(speed),
        )
    })
}

fn inspector_two_drag_f32(
    ui: &mut Ui,
    left: (&str, &mut f32, f64),
    right: (&str, &mut f32, f64),
) -> bool {
    let mut changed = false;
    let (left_rect, right_rect) = inspector_numeric_pair_rects(ui);
    changed |= inspector_drag_f32_in_rect(ui, left_rect, left.0, left.1, left.2);
    changed |= inspector_drag_f32_in_rect(ui, right_rect, right.0, right.1, right.2);
    changed
}

fn inspector_two_drag_f64(
    ui: &mut Ui,
    left: (&str, &mut f64, f64),
    right: (&str, &mut f64, f64),
) -> bool {
    let mut changed = false;
    let (left_rect, right_rect) = inspector_numeric_pair_rects(ui);
    changed |= inspector_drag_f64_in_rect(ui, left_rect, left.0, left.1, left.2);
    changed |= inspector_drag_f64_in_rect(ui, right_rect, right.0, right.1, right.2);
    changed
}

fn inspector_two_drag_u32(
    ui: &mut Ui,
    left: (&str, &mut u32, f64),
    right: (&str, &mut u32, f64),
) -> bool {
    let mut changed = false;
    let (left_rect, right_rect) = inspector_numeric_pair_rects(ui);
    changed |= inspector_drag_u32_in_rect(ui, left_rect, left.0, left.1, left.2);
    changed |= inspector_drag_u32_in_rect(ui, right_rect, right.0, right.1, right.2);
    changed
}

fn transform_editor(ui: &mut Ui, transform: &mut ClipTransform, preview_dirty: &mut bool) {
    *preview_dirty |= inspector_two_drag_f32(
        ui,
        ("Pos X", &mut transform.position_x, 1.0),
        ("Pos Y", &mut transform.position_y, 1.0),
    );
    *preview_dirty |= inspector_two_drag_f32(
        ui,
        ("SX", &mut transform.scale_x, 0.01),
        ("SY", &mut transform.scale_y, 0.01),
    );
    *preview_dirty |= inspector_two_drag_f32(
        ui,
        ("Rot", &mut transform.rotation_deg, 1.0),
        ("Opacity", &mut transform.opacity, 0.01),
    );
}

fn queue_list_height(jobs: &[GenerationJob]) -> f32 {
    if jobs.is_empty() {
        return QUEUE_EMPTY_BODY_H;
    }
    jobs.iter().map(queue_job_height).sum::<f32>()
        + QUEUE_JOB_GAP * jobs.len().saturating_sub(1) as f32
}

fn queue_job_height(job: &GenerationJob) -> f32 {
    match job.status {
        GenerationJobStatus::Running => QUEUE_JOB_RUNNING_H,
        GenerationJobStatus::Failed => QUEUE_JOB_FAILED_H,
        GenerationJobStatus::Queued | GenerationJobStatus::Succeeded => QUEUE_JOB_CARD_H,
    }
}

fn paint_queue_panel_shell(ui: &mut Ui, rect: Rect, attention: bool) {
    let radius = egui::CornerRadius::same(10);
    let shadow_rect = rect.translate(Vec2::new(0.0, 10.0)).expand(10.0);
    ui.painter().rect_filled(
        shadow_rect,
        egui::CornerRadius::same(14),
        Color32::from_rgba_unmultiplied(2, 4, 7, 116),
    );
    ui.painter().rect_filled(rect, radius, kit::PANEL_RAISED);
    ui.painter().rect_stroke(
        rect,
        radius,
        Stroke::new(1.0, kit::MODAL_STROKE),
        egui::StrokeKind::Inside,
    );

    if attention {
        let time = ui.input(|input| input.time);
        let pulse = ((time * std::f64::consts::TAU / 1.6).sin() as f32 + 1.0) * 0.5;
        let alpha = (42.0 + pulse * 92.0).round() as u8;
        ui.painter().rect_stroke(
            rect.expand(1.0),
            radius,
            Stroke::new(1.0, Color32::from_rgba_unmultiplied(244, 127, 45, alpha)),
            egui::StrokeKind::Inside,
        );
    }
}

fn queue_header(
    ui: &mut Ui,
    rect: Rect,
    job_count: usize,
    has_clearable: bool,
    clear_clicked: &mut bool,
    close_clicked: &mut bool,
) {
    let mut header_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(Layout::left_to_right(Align::Center)),
    );
    header_ui.set_min_size(rect.size());
    header_ui.shrink_clip_rect(rect);

    let count_label = if job_count == 0 {
        "Empty".to_string()
    } else {
        job_count.to_string()
    };
    header_ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 1.0;
        ui.add_sized(
            [112.0, 16.0],
            egui::Label::new(
                RichText::new("Generation Queue")
                    .color(kit::TEXT)
                    .size(12.0),
            )
            .truncate(),
        );
        ui.add_sized(
            [112.0, 12.0],
            egui::Label::new(
                RichText::new(count_label.to_ascii_uppercase())
                    .color(kit::TEXT_MUTED)
                    .size(10.0),
            )
            .truncate(),
        );
    });
    header_ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        if kit::popover_button(ui, "Close", 50.0, true).clicked() {
            *close_clicked = true;
        }
        if kit::popover_button(ui, "Clear All", 68.0, has_clearable).clicked() {
            *clear_clicked = true;
        }
    });
}

fn queue_body(ui: &mut Ui, rect: Rect, jobs: &[GenerationJob]) {
    let mut body_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(Layout::top_down(Align::Min)),
    );
    body_ui.set_min_size(rect.size());
    body_ui.shrink_clip_rect(rect);
    body_ui.set_width(rect.width());
    body_ui.set_height(rect.height());

    kit::clipped_scroll_body(&mut body_ui, "generation_queue_body", |ui| {
        ui.spacing_mut().item_spacing.y = QUEUE_JOB_GAP;
        if jobs.is_empty() {
            queue_empty_state(ui);
        } else {
            for job in jobs.iter().rev() {
                queue_job_card(ui, job);
            }
        }
    });
}

fn queue_empty_state(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), QUEUE_EMPTY_BODY_H),
        Sense::hover(),
    );
    ui.painter().rect_stroke(
        rect,
        egui::CornerRadius::same(8),
        Stroke::new(1.0, kit::BORDER_SOFT),
        egui::StrokeKind::Inside,
    );
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        "No generation jobs yet.",
        FontId::proportional(11.0),
        kit::TEXT_DIM,
    );
}

fn queue_job_card(ui: &mut Ui, job: &GenerationJob) {
    let height = queue_job_height(job);
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
    let radius = egui::CornerRadius::same(8);
    ui.painter().rect_filled(rect, radius, kit::PANEL);
    ui.painter().rect_stroke(
        rect,
        radius,
        Stroke::new(1.0, kit::BORDER_SOFT),
        egui::StrokeKind::Inside,
    );

    let content = rect.shrink(10.0);
    let (status_label, status_color) = queue_status_style(job.status);
    let output_label = queue_output_label(job.output_type);
    let status_w = match job.status {
        GenerationJobStatus::Succeeded => 56.0,
        GenerationJobStatus::Running => 64.0,
        GenerationJobStatus::Failed => 60.0,
        GenerationJobStatus::Queued => 62.0,
    };
    let title_rect = Rect::from_min_max(
        content.left_top(),
        Pos2::new(content.right() - status_w - 8.0, content.top() + 18.0),
    );
    let status_rect = Rect::from_min_size(
        Pos2::new(content.right() - status_w, content.top()),
        Vec2::new(status_w, 18.0),
    );
    queue_clipped_label(ui, title_rect, &job.asset_label, kit::TEXT, 12.0, true);
    paint_queue_status_pill(ui, status_rect, status_label, status_color);

    let meta_y = content.top() + 24.0;
    let provider_rect = Rect::from_min_size(
        Pos2::new(content.left(), meta_y),
        Vec2::new((content.width() - 54.0).max(0.0), 14.0),
    );
    let output_rect = Rect::from_min_size(
        Pos2::new(content.right() - 52.0, meta_y),
        Vec2::new(52.0, 14.0),
    );
    queue_clipped_label(
        ui,
        provider_rect,
        &job.provider.name,
        kit::TEXT_MUTED,
        10.0,
        false,
    );
    queue_clipped_label(ui, output_rect, output_label, kit::TEXT_DIM, 10.0, false);

    match job.status {
        GenerationJobStatus::Running => {
            let workflow = job.progress_overall.unwrap_or(0.0).clamp(0.0, 1.0);
            let node = job.progress_node.unwrap_or(0.0).clamp(0.0, 1.0);
            let progress_rect = Rect::from_min_max(
                Pos2::new(content.left(), content.top() + 44.0),
                content.right_bottom(),
            );
            queue_progress_rows(ui, progress_rect, workflow, node);
        }
        GenerationJobStatus::Failed => {
            if let Some(error) = job.error.as_ref() {
                let error_rect = Rect::from_min_size(
                    Pos2::new(content.left(), content.top() + 44.0),
                    Vec2::new(content.width(), 30.0),
                );
                queue_clipped_label(ui, error_rect, error, kit::DANGER, 10.0, false);
            }
        }
        GenerationJobStatus::Queued | GenerationJobStatus::Succeeded => {}
    }
}

fn queue_progress_rows(ui: &mut Ui, rect: Rect, workflow: f32, node: f32) {
    let row_h = 26.0;
    queue_progress_row(
        ui,
        Rect::from_min_size(rect.min, Vec2::new(rect.width(), row_h)),
        "Workflow",
        workflow,
        kit::PRIMARY,
    );
    queue_progress_row(
        ui,
        Rect::from_min_size(
            Pos2::new(rect.left(), rect.top() + row_h),
            Vec2::new(rect.width(), row_h),
        ),
        "Node",
        node,
        kit::MARKER,
    );
}

fn queue_progress_row(ui: &mut Ui, rect: Rect, label: &str, progress: f32, color: Color32) {
    let pct = (progress.clamp(0.0, 1.0) * 100.0).round() as u32;
    ui.painter().text(
        rect.left_top(),
        egui::Align2::LEFT_TOP,
        label,
        FontId::proportional(9.0),
        kit::TEXT_DIM,
    );
    ui.painter().text(
        rect.right_top(),
        egui::Align2::RIGHT_TOP,
        format!("{pct}%"),
        FontId::proportional(9.0),
        kit::TEXT_DIM,
    );

    let track_rect = Rect::from_min_size(
        Pos2::new(rect.left(), rect.top() + 15.0),
        Vec2::new(rect.width(), 6.0),
    );
    ui.painter()
        .rect_filled(track_rect, egui::CornerRadius::same(3), kit::PANEL_SUNKEN);
    let fill_rect = Rect::from_min_size(
        track_rect.min,
        Vec2::new(
            track_rect.width() * progress.clamp(0.0, 1.0),
            track_rect.height(),
        ),
    );
    ui.painter()
        .rect_filled(fill_rect, egui::CornerRadius::same(3), color);
}

fn paint_queue_status_pill(ui: &mut Ui, rect: Rect, label: &str, color: Color32) {
    let fill = Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 22);
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(9), fill);
    ui.painter().rect_stroke(
        rect,
        egui::CornerRadius::same(9),
        Stroke::new(1.0, color),
        egui::StrokeKind::Inside,
    );
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label.to_ascii_uppercase(),
        FontId::proportional(9.0),
        color,
    );
}

fn queue_clipped_label(
    ui: &mut Ui,
    rect: Rect,
    text: &str,
    color: Color32,
    size: f32,
    strong: bool,
) {
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(Layout::left_to_right(Align::Center)),
    );
    child.set_min_size(rect.size());
    child.shrink_clip_rect(rect);
    let mut text = RichText::new(text).color(color).size(size);
    if strong {
        text = text.strong();
    }
    child.add_sized(rect.size(), egui::Label::new(text).truncate());
}

fn queue_status_style(status: GenerationJobStatus) -> (&'static str, Color32) {
    match status {
        GenerationJobStatus::Queued => ("Queued", kit::TEXT_MUTED),
        GenerationJobStatus::Running => ("Running", kit::MARKER),
        GenerationJobStatus::Succeeded => ("Done", kit::PRIMARY_HOVER),
        GenerationJobStatus::Failed => ("Failed", kit::DANGER),
    }
}

fn queue_output_label(output_type: ProviderOutputType) -> &'static str {
    match output_type {
        ProviderOutputType::Image => "Image",
        ProviderOutputType::Video => "Video",
        ProviderOutputType::Audio => "Audio",
    }
}

fn settings_fields(ui: &mut Ui, settings: &mut ProjectSettings) {
    kit::field_label(ui, "Resolution");
    ui.horizontal_wrapped(|ui| {
        for preset in RESOLUTION_PRESETS {
            let selected = settings.width == preset.width && settings.height == preset.height;
            let color = if selected {
                kit::PRIMARY_HOVER
            } else {
                kit::TEXT_MUTED
            };
            if kit::media_pill(ui, preset.label, color).clicked() {
                settings.width = preset.width;
                settings.height = preset.height;
                settings.preview_max_width = preset.preview_width;
                settings.preview_max_height = preset.preview_height;
            }
        }
    });
    ui.add_space(6.0);
    let _ = inspector_two_drag_u32(
        ui,
        ("W", &mut settings.width, 8.0),
        ("H", &mut settings.height, 8.0),
    );
    ui.add_space(8.0);
    kit::field_label(ui, "Preview Downsample");
    ui.add_space(6.0);
    let _ = inspector_two_drag_u32(
        ui,
        ("W", &mut settings.preview_max_width, 8.0),
        ("H", &mut settings.preview_max_height, 8.0),
    );
    ui.add_space(8.0);
    kit::field_label(ui, "Timing");
    ui.add_space(6.0);
    let mut minutes = settings.duration_seconds / 60.0;
    if inspector_two_drag_f64(
        ui,
        ("FPS", &mut settings.fps, 1.0),
        ("Min", &mut minutes, 0.25),
    ) {
        settings.duration_seconds = (minutes * 60.0).max(1.0);
    }
}

struct ResolutionPreset {
    label: &'static str,
    width: u32,
    height: u32,
    preview_width: u32,
    preview_height: u32,
}

const RESOLUTION_PRESETS: &[ResolutionPreset] = &[
    ResolutionPreset {
        label: "1080p",
        width: 1920,
        height: 1080,
        preview_width: 960,
        preview_height: 540,
    },
    ResolutionPreset {
        label: "4K",
        width: 3840,
        height: 2160,
        preview_width: 1280,
        preview_height: 720,
    },
    ResolutionPreset {
        label: "9:16",
        width: 1080,
        height: 1920,
        preview_width: 540,
        preview_height: 960,
    },
    ResolutionPreset {
        label: "1:1",
        width: 512,
        height: 512,
        preview_width: 512,
        preview_height: 512,
    },
];

fn recent_projects(parent: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(parent)
        .ok()
        .into_iter()
        .flat_map(|read_dir| read_dir.filter_map(Result::ok))
        .map(|entry| entry.path())
        .filter(|path| path.join("project.json").exists())
        .take(8)
        .collect()
}

fn timecode(seconds: f64) -> String {
    let seconds = seconds.max(0.0);
    let minutes = (seconds / 60.0).floor() as u32;
    let secs = seconds % 60.0;
    format!("{minutes:02}:{secs:05.2}")
}

fn format_duration(seconds: f64) -> String {
    let seconds = seconds.max(0.0);
    if seconds >= 60.0 {
        let total_seconds = seconds.round() as u32;
        let minutes = total_seconds / 60;
        let secs = total_seconds % 60;
        format!("{minutes}:{secs:02}")
    } else {
        format!("{seconds:.1}s")
    }
}
