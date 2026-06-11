use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    mpsc, Arc, Mutex,
};
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
use crate::core::export::{
    export_video, TimestampOverlayPosition, VideoExportCodec, VideoExportEvent,
    VideoExportFrameFormat, VideoExportJob, VideoExportPreview, VideoExportQuality,
};
use crate::core::generation::{
    asset_source_available_for_provider_input, compatible_asset_for_provider_input,
    next_version_label, random_seed_i64, resolve_provider_inputs, resolve_seed_field,
    semantic_reference_slot, update_seed_inputs,
};
use crate::core::media::probe_duration_seconds;
use crate::core::preview::{PreviewDecodeMode, PreviewLayerStack, PreviewStats, RenderOutput};
use crate::core::timeline_snap::{
    best_snap_delta_frames, frames_from_seconds, seconds_from_frames, snap_time_to_frame,
    SnapTarget,
};
use crate::core::video_decode::VideoDecodeWorker;
use crate::editor::{
    default_generative_video_fps, default_generative_video_frames, default_projects_dir,
    EditorState,
};
use crate::providers::ProviderProgress;
use crate::state::{
    asset_display_name, delete_generative_version_files, generation_record_source_inputs,
    input_value_as_bool, input_value_as_f64, input_value_as_i64, input_value_as_string,
    parse_version_index, Asset, AssetKind, Clip, ClipImageMode, ClipTransform, GenerationJob,
    GenerationJobStatus, GenerationRecord, GenerationSeedAdvance, GenerativeConfig, InputValue,
    Project, ProjectSettings, ProviderConnection, ProviderEntry, ProviderInputField,
    ProviderInputType, ProviderOutputType, ProviderWorkflowKind, SeedStrategy,
    SourceFrameReference, TrackType,
};
use crate::ui_kit as kit;
use egui_extras::{Size, StripBuilder};
use serde::Serialize;

mod asset_lab;
mod asset_panel;
mod attributes_panel;
mod automation_ui;
mod confirmations;
mod export_modal;
mod export_modal_ui;
mod generation_runtime;
mod preview_canvas;
mod preview_runtime;
mod preview_transform;
mod project_modals;
mod provider_builder;
mod provider_modal;
mod queue_panel;
mod shell_chrome;
mod timeline_geometry;
mod timeline_paint;
mod timeline_panel;
use asset_lab::*;
use asset_panel::*;
use confirmations::*;
use export_modal::*;
use generation_runtime::*;
use preview_transform::*;
use provider_builder::*;
use provider_modal::*;
use timeline_geometry::*;
use timeline_paint::*;

const TIMELINE_LABEL_W: f32 = 140.0;
const TIMELINE_HEADER_H: f32 = 32.0;
const TIMELINE_RULER_H: f32 = 24.0;
const TIMELINE_TRACK_H: f32 = 36.0;
const TIMELINE_ADD_ROW_H: f32 = 42.0;
const TIMELINE_CLIP_H: f32 = 32.0;
const TIMELINE_CLIP_Y_PAD: f32 = 2.0;
const TIMELINE_KEYFRAME_HIT_W: f32 = 40.0;
const TIMELINE_KEYFRAME_THUMB: f32 = 24.0;
const TIMELINE_KEYFRAME_LABEL_W: f32 = 140.0;
const TIMELINE_SCROLLBAR_H: f32 = 12.0;
const TIMELINE_MIN_ZOOM_FLOOR: f32 = 0.1;
const TIMELINE_MAX_PX_PER_FRAME: f32 = 8.0;
const TIMELINE_SNAP_THRESHOLD_PX: f64 = 6.0;
const TIMELINE_THUMB_TILE_W: f32 = 60.0;
const TIMELINE_MAX_THUMB_TILES: usize = 120;
const PREVIEW_PERF_HISTORY_LIMIT: usize = 256;
const PREVIEW_LAYER_TEXTURE_LIMIT: usize = 512;
const PREVIEW_PREFETCH_SCRUB_SECONDS: f64 = 0.5;
const PREVIEW_PREFETCH_PLAYBACK_SECONDS: f64 = 3.0;
const PREVIEW_IDLE_PREFETCH_DELAY_MS: u64 = 800;
const PREVIEW_IDLE_PREFETCH_AHEAD_SECONDS: f64 = 5.0;
const PREVIEW_IDLE_PREFETCH_BEHIND_SECONDS: f64 = 1.0;
const PREVIEW_RENDER_RETRY_MS: u64 = 16;
const PREVIEW_ZOOM_MIN: f32 = 0.05;
const PREVIEW_ZOOM_MAX: f32 = 16.0;
const PREVIEW_SCROLL_ZOOM_SENSITIVITY: f32 = 0.012;
const PREVIEW_HANDLE_SIZE: f32 = 9.0;
const PREVIEW_ROTATE_HANDLE_DISTANCE: f32 = 34.0;
const PREVIEW_SNAP_THRESHOLD_PX: f32 = 8.0;
const AUTOMATION_SCRUB_MAX_STEPS: usize = 240;
const AUTOMATION_SCRUB_MAX_REPEATS: usize = 20;
const TIMELINE_MIN_CLIP_W: f32 = 2.0;
const TIMELINE_HANDLE_W: f32 = 8.0;
const TIMELINE_MARKER_HIT_W: f32 = 22.0;
const TIMELINE_MARKER_LABEL_W: f32 = 96.0;
const TIMELINE_MARKER_LABEL_H: f32 = 18.0;
const TIMELINE_SCRUB_PREVIEW_SECONDS: f64 = 0.03;
const TIMELINE_WHEEL_ZOOM_SENSITIVITY: f32 = 0.01;
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
const MP4_EXTENSIONS: &[&str] = &["mp4"];
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
const MP4_FILE_FILTERS: &[kit::FileExtensionFilter<'static>] = &[kit::FileExtensionFilter {
    name: "MP4 Video",
    extensions: MP4_EXTENSIONS,
}];
const PROVIDERS_MODAL_SIZE: [f32; 2] = [760.0, 560.0];
const API_KEYS_MODAL_SIZE: [f32; 2] = [480.0, 280.0];
const PROVIDER_JSON_MODAL_SIZE: [f32; 2] = [920.0, 700.0];
const PROVIDER_BUILDER_MODAL_SIZE: [f32; 2] = [1080.0, 720.0];
const EXPORT_MODAL_SIZE: [f32; 2] = [780.0, 640.0];
const ASSET_LAB_MODAL_SIZE: [f32; 2] = [1520.0, 940.0];
const ASSET_DELETE_MODAL_SIZE: [f32; 2] = [460.0, 310.0];
const TRACK_DELETE_MODAL_SIZE: [f32; 2] = [460.0, 300.0];
const BRIDGE_KEYFRAME_MODAL_SIZE: [f32; 2] = [500.0, 340.0];
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
#[allow(dead_code)]
const ASSET_LAB_VERSION_ROW_H: f32 = 54.0;
const ASSET_LAB_PREVIEW_H: f32 = 200.0;
const ASSET_LAB_PREVIEW_SCRUB_GAP: f32 = 6.0;

const APP_ICON_PNG: &[u8] = include_bytes!("../assets/app-icon.png");

fn modal_size(ctx: &Context, desired: [f32; 2], min: [f32; 2]) -> Vec2 {
    let available = ctx.content_rect().size();
    let max_w = (available.x - 24.0).max(min[0].min(available.x));
    let max_h = (available.y - 24.0).max(min[1].min(available.y));
    Vec2::new(
        desired[0].min(max_w).max(min[0].min(max_w)),
        desired[1].min(max_h).max(min[1].min(max_h)),
    )
}

fn app_icon() -> egui::IconData {
    let icon = image::load_from_memory(APP_ICON_PNG)
        .expect("embedded LatentSlate app icon should decode")
        .into_rgba8();
    let (width, height) = icon.dimensions();
    egui::IconData {
        rgba: icon.into_raw(),
        width,
        height,
    }
}

pub fn run() -> eframe::Result<()> {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("LatentSlate")
            .with_icon(app_icon())
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([960.0, 620.0]),
        ..Default::default()
    };

    eframe::run_native(
        "LatentSlate",
        native_options,
        Box::new(|cc| Ok(Box::new(LatentSlateApp::new(cc)))),
    )
}

pub struct LatentSlateApp {
    editor: EditorState,
    preview_layers: Option<PreviewLayerStack>,
    preview_layer_textures: HashMap<u64, PreviewLayerTexture>,
    preview_layer_texture_sequence: u64,
    preview_last_render_time: Option<f64>,
    preview_last_interaction: Instant,
    preview_idle_prefetched_time: Option<f64>,
    preview_render_tx: mpsc::Sender<PreviewRenderResult>,
    preview_render_rx: mpsc::Receiver<PreviewRenderResult>,
    preview_render_in_flight: Arc<AtomicBool>,
    preview_render_request_id: Arc<AtomicU64>,
    preview_render_busy_since: Option<Instant>,
    preview_render_completed_count: u64,
    preview_render_stale_count: u64,
    preview_render_last_worker_ms: Option<f64>,
    preview_render_last_delivery_ms: Option<f64>,
    preview_prefetch_in_flight: Arc<AtomicBool>,
    asset_thumbnails: HashMap<Uuid, AssetThumbnail>,
    asset_thumbnail_misses: HashSet<Uuid>,
    asset_source_dimensions: HashMap<Uuid, Vec2>,
    asset_source_dimension_misses: HashSet<Uuid>,
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
    timeline_context_menu_pos: Option<Pos2>,
    timeline_snap_preview: Option<f64>,
    timeline_scrub_was_playing: bool,
    timeline_last_scrub_audio_time: Option<f64>,
    clip_spacing_seconds: f64,
    clip_spacing_frames: i64,
    clip_spacing_set_duration: bool,
    preview_auto_fit: bool,
    preview_zoom: f32,
    preview_pan: Vec2,
    preview_drag: Option<PreviewTransformDrag>,
    preview_snap_guides: Vec<PreviewSnapGuide>,
    preview_stats: Option<PreviewStats>,
    preview_perf_samples: VecDeque<PreviewPerfSample>,
    preview_perf_sequence: u64,
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
    api_key_modal: ApiKeyModalState,
    asset_lab: AssetLabState,
    asset_lab_preview_texture: Option<AssetLabPreviewTexture>,
    asset_lab_node_preview_textures: HashMap<AssetLabNodePreviewKey, AssetLabPreviewTexture>,
    asset_lab_video_decoder: VideoDecodeWorker,
    provider_template_kind: ProviderTemplateKind,
    asset_delete_confirmation: Option<AssetDeleteConfirmation>,
    track_delete_confirmation: Option<TrackDeleteConfirmation>,
    bridge_keyframe_confirmation: Option<BridgeKeyframeConfirmation>,
    generation_runtime: Option<tokio::runtime::Runtime>,
    generation_events_tx: mpsc::Sender<GenerationEvent>,
    generation_events_rx: mpsc::Receiver<GenerationEvent>,
    generation_active: Option<Uuid>,
    generation_cancel_tokens: HashMap<Uuid, Arc<AtomicBool>>,
    export_modal: ExportModalState,
    export_events_tx: mpsc::Sender<VideoExportEvent>,
    export_events_rx: mpsc::Receiver<VideoExportEvent>,
    export_cancel: Option<Arc<AtomicBool>>,
    export_preview_texture: Option<TextureHandle>,
    queue_button_rect: Option<Rect>,
    asset_drop_target_rect: Option<Rect>,
    asset_drop_target_hovered: bool,
    unsaved_close_confirmation_open: bool,
    allow_close_without_prompt: bool,
    last_window_title_dirty: Option<bool>,
    pending_automation_ui_actions: Vec<PendingAutomationUiAction>,
    pending_automation_screenshot: Option<PendingAutomationScreenshot>,
}

struct PendingAutomationUiAction {
    id: String,
    action: &'static str,
    envelope: crate::core::automation::AutomationEnvelope,
}

struct PendingAutomationScreenshot {
    path: PathBuf,
    requested_at: Instant,
    envelope: crate::core::automation::AutomationEnvelope,
}

struct PreviewLayerTexture {
    texture: TextureHandle,
    size: [usize; 2],
    last_used: u64,
}

struct PreviewRenderResult {
    request_id: u64,
    time_seconds: f64,
    decode_mode: PreviewDecodeMode,
    requested_at: Instant,
    finished_at: Instant,
    output: RenderOutput,
}

#[derive(Clone, Debug, Serialize)]
struct PreviewPerfSample {
    sequence: u64,
    request_id: Option<u64>,
    captured_at_ms: i64,
    playhead_seconds: f64,
    render_worker_ms: Option<f64>,
    delivery_ms: Option<f64>,
    stats: PreviewStats,
}

#[derive(Clone, Debug, Default, Serialize)]
struct PreviewPerfSummary {
    samples: usize,
    total_ms_min: f64,
    total_ms_avg: f64,
    total_ms_max: f64,
    collect_ms_avg: f64,
    composite_ms_avg: f64,
    encode_ms_avg: f64,
    video_decode_ms_avg: f64,
    video_decode_queue_ms_avg: f64,
    video_decode_seek_ms_avg: f64,
    still_load_ms_avg: f64,
    layers_avg: f64,
    cache_hits: usize,
    cache_misses: usize,
    cache_hit_rate: f64,
}

#[derive(Clone, Debug, Serialize)]
struct ScrubProfileSample {
    repeat: usize,
    step: usize,
    requested_time: f64,
    actual_time: f64,
    seek_ms: f64,
    render_wall_ms: f64,
    stats: PreviewStats,
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

#[derive(Clone)]
struct AssetInputCandidate {
    asset_id: Uuid,
    source_clip_id: Option<Uuid>,
    frame_reference: Option<SourceFrameReference>,
    label: String,
    detail: String,
    contextual: bool,
    score: f64,
}

#[derive(Clone, Copy, Debug)]
struct TimelineClipMoveData {
    clip_id: Uuid,
    asset_id: Uuid,
    track_id: Uuid,
    start_time: f64,
    duration: f64,
}

#[derive(Clone, Debug)]
enum TimelineDrag {
    Playhead,
    ClipMove {
        anchor_clip_id: Uuid,
        clips: Vec<TimelineClipMoveData>,
        allow_track_move: bool,
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
    TrackReorder {
        track_id: Uuid,
        insertion_index: usize,
    },
}

#[derive(Clone, Copy, Debug)]
struct AssetTimelineDragPayload {
    asset_id: Uuid,
}

#[derive(Clone)]
struct BridgeReferenceClip {
    clip: Clip,
    anchor_time: f64,
    frame_reference: Option<SourceFrameReference>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SingleI2VReference {
    Image,
    VideoFirstFrame,
    VideoLastFrame,
}

impl LatentSlateApp {
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
            .thread_name("latentslate-generation")
            .build()
        {
            Ok(runtime) => Some(runtime),
            Err(err) => {
                editor.status = format!("Generation runtime unavailable: {err}");
                None
            }
        };
        let (generation_events_tx, generation_events_rx) = mpsc::channel();
        let (export_events_tx, export_events_rx) = mpsc::channel();
        let (preview_render_tx, preview_render_rx) = mpsc::channel();
        let export_modal = ExportModalState::for_project(&editor.project);
        let now = Instant::now();
        Self {
            project_settings: editor.project.settings.clone(),
            editor,
            preview_layers: None,
            preview_layer_textures: HashMap::new(),
            preview_layer_texture_sequence: 0,
            preview_last_render_time: None,
            preview_last_interaction: now,
            preview_idle_prefetched_time: None,
            preview_render_tx,
            preview_render_rx,
            preview_render_in_flight: Arc::new(AtomicBool::new(false)),
            preview_render_request_id: Arc::new(AtomicU64::new(0)),
            preview_render_busy_since: None,
            preview_render_completed_count: 0,
            preview_render_stale_count: 0,
            preview_render_last_worker_ms: None,
            preview_render_last_delivery_ms: None,
            preview_prefetch_in_flight: Arc::new(AtomicBool::new(false)),
            asset_thumbnails: HashMap::new(),
            asset_thumbnail_misses: HashSet::new(),
            asset_source_dimensions: HashMap::new(),
            asset_source_dimension_misses: HashSet::new(),
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
            timeline_context_menu_pos: None,
            timeline_snap_preview: None,
            timeline_scrub_was_playing: false,
            timeline_last_scrub_audio_time: None,
            clip_spacing_seconds: default_generative_video_frames() as f64
                / default_generative_video_fps(),
            clip_spacing_frames: default_generative_video_frames() as i64,
            clip_spacing_set_duration: false,
            preview_auto_fit: true,
            preview_zoom: 1.0,
            preview_pan: Vec2::ZERO,
            preview_drag: None,
            preview_snap_guides: Vec::new(),
            preview_stats: None,
            preview_perf_samples: VecDeque::new(),
            preview_perf_sequence: 0,
            last_tick: now,
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
            api_key_modal: ApiKeyModalState::default(),
            asset_lab: AssetLabState::default(),
            asset_lab_preview_texture: None,
            asset_lab_node_preview_textures: HashMap::new(),
            asset_lab_video_decoder: VideoDecodeWorker::new(8192, 8192),
            provider_template_kind: ProviderTemplateKind::default(),
            asset_delete_confirmation: None,
            track_delete_confirmation: None,
            bridge_keyframe_confirmation: None,
            generation_runtime,
            generation_events_tx,
            generation_events_rx,
            generation_active: None,
            generation_cancel_tokens: HashMap::new(),
            export_modal,
            export_events_tx,
            export_events_rx,
            export_cancel: None,
            export_preview_texture: None,
            queue_button_rect: None,
            asset_drop_target_rect: None,
            asset_drop_target_hovered: false,
            unsaved_close_confirmation_open: false,
            allow_close_without_prompt: false,
            last_window_title_dirty: None,
            pending_automation_ui_actions: Vec::new(),
            pending_automation_screenshot: None,
        }
    }

    fn open_project_folder(&mut self, folder: PathBuf) -> bool {
        match self.editor.open_project(folder) {
            Ok(_) => {
                self.project_settings = self.editor.project.settings.clone();
                self.export_modal = ExportModalState::for_project(&self.editor.project);
                self.export_preview_texture = None;
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

    fn keyboard_shortcuts_suppressed(&self, ctx: &Context) -> bool {
        ctx.text_edit_focused() || ctx.any_popup_open() || self.modal_background_input_blocked()
    }

    fn modal_background_input_blocked(&self) -> bool {
        self.editor.show_startup()
            || self.editor.overlays.new_project
            || self.editor.overlays.project_settings
            || self.editor.overlays.generative_video
            || self.editor.overlays.export_video
            || self.editor.overlays.providers
            || self.editor.overlays.api_keys
            || self.editor.overlays.asset_lab
            || self.editor.overlays.queue
            || self.unsaved_close_confirmation_open
            || self.asset_delete_confirmation.is_some()
            || self.track_delete_confirmation.is_some()
            || self.bridge_keyframe_confirmation.is_some()
            || self.provider_json_editor_path.is_some()
            || self.provider_builder_open
    }

    fn handle_app_keyboard(&mut self, ctx: &Context) {
        if self.keyboard_shortcuts_suppressed(ctx) || self.editor.project.project_path.is_none() {
            return;
        }

        let (save, delete, play_pause, left, right, command_down) = ctx.input(|input| {
            (
                input.modifiers.command && input.key_pressed(egui::Key::S),
                !input.modifiers.command
                    && !input.modifiers.ctrl
                    && !input.modifiers.mac_cmd
                    && (input.key_pressed(egui::Key::Delete)
                        || input.key_pressed(egui::Key::Backspace)),
                !input.modifiers.command
                    && !input.modifiers.alt
                    && !input.modifiers.ctrl
                    && !input.modifiers.mac_cmd
                    && input.key_pressed(egui::Key::Space),
                input.key_pressed(egui::Key::ArrowLeft),
                input.key_pressed(egui::Key::ArrowRight),
                input.modifiers.command,
            )
        });

        if save {
            if let Err(err) = self.editor.save() {
                self.editor.status = err;
            }
            ctx.request_repaint();
            return;
        }

        if delete && !self.editor.selection.clip_ids.is_empty() {
            self.editor.delete_selected_clips();
            ctx.request_repaint();
            return;
        }

        if delete && !self.editor.selection.asset_ids.is_empty() {
            self.request_delete_selected_assets();
            ctx.request_repaint();
            return;
        }

        if delete && !self.editor.selection.track_ids.is_empty() {
            self.request_delete_selected_tracks();
            ctx.request_repaint();
            return;
        }

        if play_pause {
            self.toggle_playback();
            ctx.request_repaint();
            return;
        }

        if left {
            if command_down {
                self.seek_to_adjacent_timeline_snap(-1);
            } else {
                self.seek_editor(
                    previous_frame_time(self.editor.current_time, self.editor.project.settings.fps),
                    false,
                );
            }
            ctx.request_repaint();
        } else if right {
            if command_down {
                self.seek_to_adjacent_timeline_snap(1);
            } else {
                self.seek_editor(
                    next_frame_time(
                        self.editor.current_time,
                        self.editor.project.duration(),
                        self.editor.project.settings.fps,
                    ),
                    false,
                );
            }
            ctx.request_repaint();
        }
    }

    fn seek_to_adjacent_timeline_snap(&mut self, direction: i32) {
        let fps = self.editor.project.settings.fps.max(1.0);
        let duration = self.editor.project.duration().max(0.0);
        let current_frame = frames_from_seconds(self.editor.current_time, fps).round() as i64;
        let max_frame = frames_from_seconds(duration, fps).round().max(0.0) as i64;
        let mut frames = Vec::with_capacity(self.editor.project.clips.len() * 2 + 8);
        frames.push(0);
        frames.push(max_frame);
        frames.extend(
            self.timeline_snap_targets(&[], None, false)
                .into_iter()
                .map(|target| target.frame.round().clamp(0.0, max_frame as f64) as i64),
        );
        frames.sort_unstable();
        frames.dedup();

        let target_frame = if direction < 0 {
            frames
                .into_iter()
                .rev()
                .find(|frame| *frame < current_frame)
        } else {
            frames.into_iter().find(|frame| *frame > current_frame)
        };

        if let Some(frame) = target_frame {
            self.seek_editor(seconds_from_frames(frame as f64, fps), false);
        }
    }
}

fn timeline_floor_frame(time_seconds: f64, fps: f64) -> i64 {
    (time_seconds.max(0.0) * fps.max(1.0)).floor() as i64
}

fn summarize_preview_perf_samples(samples: &[PreviewPerfSample]) -> PreviewPerfSummary {
    summarize_preview_stats(samples.iter().map(|sample| &sample.stats))
}

fn summarize_scrub_profile_samples(samples: &[ScrubProfileSample]) -> PreviewPerfSummary {
    summarize_preview_stats(samples.iter().map(|sample| &sample.stats))
}

fn summarize_preview_stats<'a>(
    stats_iter: impl IntoIterator<Item = &'a PreviewStats>,
) -> PreviewPerfSummary {
    let mut summary = PreviewPerfSummary::default();
    let mut total_sum = 0.0;
    let mut collect_sum = 0.0;
    let mut composite_sum = 0.0;
    let mut encode_sum = 0.0;
    let mut video_decode_sum = 0.0;
    let mut video_decode_queue_sum = 0.0;
    let mut video_decode_seek_sum = 0.0;
    let mut still_load_sum = 0.0;
    let mut layers_sum = 0.0;
    summary.total_ms_min = f64::INFINITY;

    for stats in stats_iter {
        summary.samples += 1;
        summary.total_ms_min = summary.total_ms_min.min(stats.total_ms);
        summary.total_ms_max = summary.total_ms_max.max(stats.total_ms);
        total_sum += stats.total_ms;
        collect_sum += stats.collect_ms;
        composite_sum += stats.composite_ms;
        encode_sum += stats.encode_ms;
        video_decode_sum += stats.video_decode_ms;
        video_decode_queue_sum += stats.video_decode_queue_ms;
        video_decode_seek_sum += stats.video_decode_seek_ms;
        still_load_sum += stats.still_load_ms;
        layers_sum += stats.layers as f64;
        summary.cache_hits += stats.cache_hits;
        summary.cache_misses += stats.cache_misses;
    }

    if summary.samples == 0 {
        summary.total_ms_min = 0.0;
        return summary;
    }

    let count = summary.samples as f64;
    summary.total_ms_avg = total_sum / count;
    summary.collect_ms_avg = collect_sum / count;
    summary.composite_ms_avg = composite_sum / count;
    summary.encode_ms_avg = encode_sum / count;
    summary.video_decode_ms_avg = video_decode_sum / count;
    summary.video_decode_queue_ms_avg = video_decode_queue_sum / count;
    summary.video_decode_seek_ms_avg = video_decode_seek_sum / count;
    summary.still_load_ms_avg = still_load_sum / count;
    summary.layers_avg = layers_sum / count;

    let cache_total = summary.cache_hits + summary.cache_misses;
    if cache_total > 0 {
        summary.cache_hit_rate = summary.cache_hits as f64 / cache_total as f64;
    }

    summary
}

fn save_color_image_png(path: &Path, image: &ColorImage) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| {
            format!(
                "Failed to create screenshot directory {}: {err}",
                parent.display()
            )
        })?;
    }

    let mut rgba = Vec::with_capacity(image.pixels.len() * 4);
    for pixel in &image.pixels {
        rgba.extend_from_slice(&pixel.to_srgba_unmultiplied());
    }

    image::save_buffer(
        path,
        &rgba,
        image.size[0] as u32,
        image.size[1] as u32,
        image::ColorType::Rgba8,
    )
    .map_err(|err| format!("Failed to save screenshot {}: {err}", path.display()))
}

fn automation_button(response: egui::Response, label: &str) -> egui::Response {
    crate::core::automation::instrument_response(
        response,
        "button",
        Some(label.to_string()),
        true,
        false,
    )
}

fn automation_checkbox(ui: &mut Ui, value: &mut bool, label: &str) -> egui::Response {
    let response = ui.checkbox(value, label);
    let real_clicked = response.clicked();
    let mut response = crate::core::automation::instrument_response(
        response,
        "checkbox",
        Some(label.to_string()),
        true,
        false,
    );
    if response.clicked() && !real_clicked {
        *value = !*value;
        response.mark_changed();
    }
    response
}

fn automation_selectable_value<T>(
    ui: &mut Ui,
    value: &mut T,
    selected_value: T,
    label: &str,
) -> egui::Response
where
    T: PartialEq + Clone,
{
    let response = ui.selectable_value(value, selected_value.clone(), label);
    let real_clicked = response.clicked();
    let mut response = crate::core::automation::instrument_response(
        response,
        "selectable",
        Some(label.to_string()),
        true,
        false,
    );
    if response.clicked() && !real_clicked {
        *value = selected_value;
        response.mark_changed();
        ui.close();
    }
    response
}

async fn execute_generation_job_async(
    job: GenerationJob,
    version: String,
    progress_tx: Option<tokio::sync::mpsc::UnboundedSender<ProviderProgress>>,
    cancel_token: Arc<AtomicBool>,
) -> Result<GenerationOutput, GenerationFailure> {
    if job.output_type == ProviderOutputType::Audio {
        return Err(GenerationFailure::Error(
            "Audio outputs are not supported in the queue yet.".to_string(),
        ));
    }

    let output = crate::providers::execute_generation(
        &job.provider,
        &job.inputs,
        job.output_type,
        progress_tx,
    )
    .await
    .map_err(|err| match err {
        crate::providers::ProviderExecutionError::Offline(err) => GenerationFailure::Offline(err),
        crate::providers::ProviderExecutionError::Error(err) => GenerationFailure::Error(err),
    })?;

    if cancel_token.load(Ordering::Relaxed) {
        return Err(GenerationFailure::Canceled);
    }

    std::fs::create_dir_all(&job.folder_path).map_err(|err| {
        GenerationFailure::Error(format!("Failed to create output folder: {err}"))
    })?;
    let output_path = job
        .folder_path
        .join(format!("{}.{}", version, output.extension));
    if cancel_token.load(Ordering::Relaxed) {
        return Err(GenerationFailure::Canceled);
    }
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
    add_contents: impl FnOnce(&mut Ui, &mut LatentSlateApp),
    app: &mut LatentSlateApp,
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

fn centered_child_rect(parent: Rect, width: f32, height: f32) -> Rect {
    let size = Vec2::new(
        width.min(parent.width()).max(0.0),
        height.min(parent.height()).max(0.0),
    );
    let left = (parent.center().x - size.x * 0.5).clamp(parent.left(), parent.right() - size.x);
    let top = (parent.center().y - size.y * 0.5).clamp(parent.top(), parent.bottom() - size.y);
    Rect::from_min_size(Pos2::new(left, top), size)
}

fn multi_select_modifier(ui: &Ui) -> bool {
    ui.input(|input| {
        input.modifiers.shift
            || input.modifiers.command
            || input.modifiers.ctrl
            || input.modifiers.mac_cmd
    })
}

fn binding_matches_candidate(
    value: Option<&InputValue>,
    candidate: &AssetInputCandidate,
    pinned: bool,
) -> bool {
    matches!(
        value,
        Some(InputValue::AssetRef {
            asset_id,
            source_clip_id,
            pinned: value_pinned,
            frame_reference,
        }) if *asset_id == candidate.asset_id
            && *source_clip_id == candidate.source_clip_id
            && *value_pinned == pinned
            && *frame_reference == candidate.frame_reference
    )
}

fn bridge_reference_for_clip(
    clip: Clip,
    asset: &Asset,
    start_reference: bool,
) -> BridgeReferenceClip {
    if asset.is_video() {
        let (anchor_time, frame_reference) = if start_reference {
            (clip.end_time(), Some(SourceFrameReference::Last))
        } else {
            (clip.start_time, Some(SourceFrameReference::First))
        };
        BridgeReferenceClip {
            clip,
            anchor_time,
            frame_reference,
        }
    } else {
        BridgeReferenceClip {
            anchor_time: clip.start_time,
            clip,
            frame_reference: None,
        }
    }
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
    let mut track_mutes = HashMap::new();
    for track in project.tracks.iter() {
        track_types.insert(track.id, track.track_type);
        track_volumes.insert(track.id, track.volume);
        track_mutes.insert(track.id, track.muted);
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
        if track_mutes.get(&clip.track_id).copied().unwrap_or(false) {
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
    let mut track_mutes = HashMap::new();
    for track in project.tracks.iter() {
        track_types.insert(track.id, track.track_type);
        track_mutes.insert(track.id, track.muted);
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
        if track_mutes.get(&clip.track_id).copied().unwrap_or(false) {
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

fn is_supported_asset_import_path(path: &Path) -> bool {
    let Some(ext) = path.extension().and_then(|ext| ext.to_str()) else {
        return false;
    };
    matches!(
        ext.to_ascii_lowercase().as_str(),
        "mp4"
            | "mov"
            | "avi"
            | "mkv"
            | "webm"
            | "mp3"
            | "wav"
            | "ogg"
            | "flac"
            | "png"
            | "jpg"
            | "jpeg"
            | "gif"
            | "webp"
    )
}

fn unique_uuid_list(ids: &[Uuid]) -> Vec<Uuid> {
    let mut unique = Vec::new();
    for id in ids {
        if !unique.contains(id) {
            unique.push(*id);
        }
    }
    unique
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
        InputValue::AssetRef { .. } | InputValue::GenerationRef { .. } => None,
    })
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
