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
    compatible_asset_for_provider_input, next_version_label, random_seed_i64,
    resolve_provider_inputs, resolve_seed_field, semantic_reference_slot, update_seed_inputs,
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
    asset_display_name, delete_generative_version_files, input_value_as_bool, input_value_as_f64,
    input_value_as_i64, input_value_as_string, parse_version_index, Asset, AssetKind, Clip,
    ClipImageMode, ClipTransform, GenerationJob, GenerationJobStatus, GenerationRecord,
    GenerationSeedAdvance, GenerativeConfig, InputValue, Project, ProjectSettings,
    ProviderConnection, ProviderEntry, ProviderInputField, ProviderInputType, ProviderOutputType,
    ProviderWorkflowKind, SeedStrategy, SourceFrameReference, TrackType,
};
use crate::ui_kit as kit;
use egui_extras::{Size, StripBuilder};
use serde::Serialize;

mod asset_lab;
mod asset_panel;
mod attributes_panel;
mod confirmations;
mod export_modal;
mod export_modal_ui;
mod generation_runtime;
mod preview_canvas;
mod preview_transform;
mod project_modals;
mod provider_builder;
mod provider_modal;
mod queue_panel;
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
use queue_panel::*;
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
const ASSET_LAB_MODAL_SIZE: [f32; 2] = [900.0, 640.0];
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
const ASSET_LAB_VERSION_ROW_H: f32 = 54.0;
const ASSET_LAB_PREVIEW_H: f32 = 220.0;
const ASSET_LAB_PREVIEW_SCRUB_GAP: f32 = 6.0;

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
            pending_automation_ui_actions: Vec::new(),
            pending_automation_screenshot: None,
        }
    }

    fn poll_automation(&mut self, ctx: &Context) {
        if !crate::core::automation::is_enabled() {
            return;
        }
        while let Some(envelope) = crate::core::automation::try_recv_command() {
            match envelope.command.clone() {
                crate::core::automation::AutomationCommand::GetUi => {
                    envelope.respond(crate::core::automation::AutomationResponse::ok(
                        serde_json::json!({
                            "elements": crate::core::automation::ui_snapshot(),
                        }),
                    ));
                }
                crate::core::automation::AutomationCommand::ClickUi { id } => {
                    self.queue_automation_click(ctx, envelope, id);
                }
                crate::core::automation::AutomationCommand::TextUi { id, text, replace } => {
                    self.queue_automation_text(ctx, envelope, id, text, replace);
                }
                crate::core::automation::AutomationCommand::Screenshot { name } => {
                    self.queue_automation_screenshot(ctx, envelope, name.as_deref());
                }
                crate::core::automation::AutomationCommand::GetPerformanceDiagnostics => {
                    envelope.respond(self.performance_diagnostics_response());
                }
                crate::core::automation::AutomationCommand::ScrubTimelineProfile {
                    start_time,
                    end_time,
                    steps,
                    repeats,
                    scrub_audio,
                    settle_ms,
                } => {
                    let response = self.run_automation_scrub_profile(
                        ctx,
                        start_time,
                        end_time,
                        steps,
                        repeats,
                        scrub_audio,
                        settle_ms,
                    );
                    envelope.respond(response);
                }
                crate::core::automation::AutomationCommand::OpenExportVideo => {
                    self.open_export_modal();
                    envelope.respond(crate::core::automation::AutomationResponse::empty_ok());
                }
                crate::core::automation::AutomationCommand::CloseExportVideo => {
                    self.close_or_cancel_export_modal();
                    envelope.respond(crate::core::automation::AutomationResponse::empty_ok());
                }
                _ => {
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
        }
    }

    fn queue_automation_click(
        &mut self,
        ctx: &Context,
        envelope: crate::core::automation::AutomationEnvelope,
        id: String,
    ) {
        let Some(element) = crate::core::automation::find_ui_element(&id) else {
            envelope.respond(crate::core::automation::AutomationResponse::not_found(
                format!("No visible UI element with id {id}. Refresh /ui and try again."),
            ));
            return;
        };
        if !element.enabled {
            envelope.respond(crate::core::automation::AutomationResponse::conflict(
                format!("UI element {id} is visible but disabled."),
            ));
            return;
        }
        if !element.clickable {
            envelope.respond(crate::core::automation::AutomationResponse::conflict(
                format!("UI element {id} is not clickable."),
            ));
            return;
        }

        crate::core::automation::queue_ui_click(id.clone());
        self.pending_automation_ui_actions
            .push(PendingAutomationUiAction {
                id,
                action: "click",
                envelope,
            });
        ctx.request_repaint();
    }

    fn queue_automation_text(
        &mut self,
        ctx: &Context,
        envelope: crate::core::automation::AutomationEnvelope,
        id: String,
        text: String,
        replace: bool,
    ) {
        let Some(element) = crate::core::automation::find_ui_element(&id) else {
            envelope.respond(crate::core::automation::AutomationResponse::not_found(
                format!("No visible UI element with id {id}. Refresh /ui and try again."),
            ));
            return;
        };
        if !element.enabled {
            envelope.respond(crate::core::automation::AutomationResponse::conflict(
                format!("UI element {id} is visible but disabled."),
            ));
            return;
        }
        if !element.editable {
            envelope.respond(crate::core::automation::AutomationResponse::conflict(
                format!("UI element {id} is not editable."),
            ));
            return;
        }

        crate::core::automation::queue_ui_text(id.clone(), text, replace);
        self.pending_automation_ui_actions
            .push(PendingAutomationUiAction {
                id,
                action: "text",
                envelope,
            });
        ctx.request_repaint();
    }

    fn queue_automation_screenshot(
        &mut self,
        ctx: &Context,
        envelope: crate::core::automation::AutomationEnvelope,
        name: Option<&str>,
    ) {
        if self.pending_automation_screenshot.is_some() {
            envelope.respond(crate::core::automation::AutomationResponse::conflict(
                "A screenshot request is already pending.",
            ));
            return;
        }

        let path = match crate::core::automation::screenshot_path(name) {
            Ok(path) => path,
            Err(err) => {
                envelope.respond(crate::core::automation::AutomationResponse::with_status(
                    err, 500,
                ));
                return;
            }
        };
        self.pending_automation_screenshot = Some(PendingAutomationScreenshot {
            path,
            requested_at: Instant::now(),
            envelope,
        });
        ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::new(
            "automation_screenshot".to_string(),
        )));
        ctx.request_repaint();
    }

    fn performance_diagnostics_response(&self) -> crate::core::automation::AutomationResponse {
        let samples: Vec<PreviewPerfSample> = self.preview_perf_samples.iter().cloned().collect();
        let recent_summary = summarize_preview_perf_samples(&samples);
        crate::core::automation::AutomationResponse::ok(serde_json::json!({
            "current_time": self.editor.current_time,
            "is_playing": self.editor.is_playing,
            "preview_dirty": self.editor.preview_dirty,
            "project_loaded": self.editor.project.project_path.is_some(),
            "latest_stats": self.preview_stats.clone(),
            "cache": self.editor.previewer.cache_stats(),
            "render": {
                "in_flight": self.preview_render_in_flight.load(Ordering::Relaxed),
                "latest_request_id": self.preview_render_request_id.load(Ordering::Relaxed),
                "busy_ms": self.preview_render_busy_since.map(|start| start.elapsed().as_secs_f64() * 1000.0),
                "completed": self.preview_render_completed_count,
                "stale": self.preview_render_stale_count,
                "last_worker_ms": self.preview_render_last_worker_ms,
                "last_delivery_ms": self.preview_render_last_delivery_ms,
            },
            "recent_summary": recent_summary,
            "recent_samples": samples,
        }))
    }

    fn run_automation_scrub_profile(
        &mut self,
        ctx: &Context,
        start_time: f64,
        end_time: f64,
        steps: usize,
        repeats: usize,
        scrub_audio: bool,
        settle_ms: u64,
    ) -> crate::core::automation::AutomationResponse {
        if self.editor.project.project_path.is_none() {
            return crate::core::automation::AutomationResponse::conflict(
                "Open a project before running a scrub profile.",
            );
        }

        let duration = self.editor.project.duration();
        let start_time = start_time.clamp(0.0, duration);
        let end_time = end_time.clamp(0.0, duration);
        let steps = steps.clamp(1, AUTOMATION_SCRUB_MAX_STEPS);
        let repeats = repeats.clamp(1, AUTOMATION_SCRUB_MAX_REPEATS);
        let settle_ms = settle_ms.min(500);
        let was_playing = self.editor.is_playing;
        if scrub_audio {
            self.timeline_scrub_was_playing = was_playing;
            self.timeline_last_scrub_audio_time = None;
        }

        let profile_start = Instant::now();
        let mut samples = Vec::with_capacity(steps.saturating_mul(repeats));
        for repeat in 0..repeats {
            for step in 0..steps {
                let alpha = if steps <= 1 {
                    0.0
                } else {
                    step as f64 / (steps - 1) as f64
                };
                let requested_time = start_time + (end_time - start_time) * alpha;
                let seek_start = Instant::now();
                self.seek_editor(requested_time, scrub_audio);
                let seek_ms = seek_start.elapsed().as_secs_f64() * 1000.0;

                let render_start = Instant::now();
                let stats = self.render_preview_sync_for_profile(ctx);
                let render_wall_ms = render_start.elapsed().as_secs_f64() * 1000.0;

                samples.push(ScrubProfileSample {
                    repeat,
                    step,
                    requested_time,
                    actual_time: self.editor.current_time,
                    seek_ms,
                    render_wall_ms,
                    stats,
                });

                if settle_ms > 0 {
                    std::thread::sleep(Duration::from_millis(settle_ms));
                }
            }
        }

        if scrub_audio {
            self.timeline_scrub_was_playing = was_playing;
            self.finish_timeline_scrub();
        }

        ctx.request_repaint();
        let summary = summarize_scrub_profile_samples(&samples);
        crate::core::automation::AutomationResponse::ok(serde_json::json!({
            "profile": {
                "start_time": start_time,
                "end_time": end_time,
                "steps": steps,
                "repeats": repeats,
                "scrub_audio": scrub_audio,
                "settle_ms": settle_ms,
                "wall_ms": profile_start.elapsed().as_secs_f64() * 1000.0,
                "summary": summary,
                "samples": samples,
                "cache": self.editor.previewer.cache_stats(),
            }
        }))
    }

    fn record_preview_perf_sample(
        &mut self,
        playhead_seconds: f64,
        stats: PreviewStats,
        request_id: Option<u64>,
        render_worker_ms: Option<f64>,
        delivery_ms: Option<f64>,
    ) {
        self.preview_perf_sequence = self.preview_perf_sequence.wrapping_add(1);
        self.preview_perf_samples.push_back(PreviewPerfSample {
            sequence: self.preview_perf_sequence,
            request_id,
            captured_at_ms: chrono::Utc::now().timestamp_millis(),
            playhead_seconds,
            render_worker_ms,
            delivery_ms,
            stats,
        });
        while self.preview_perf_samples.len() > PREVIEW_PERF_HISTORY_LIMIT {
            self.preview_perf_samples.pop_front();
        }
    }

    fn clear_project_runtime_cache(&mut self) {
        self.preview_layers = None;
        self.preview_layer_textures.clear();
        self.preview_layer_texture_sequence = 0;
        self.preview_last_render_time = None;
        self.preview_last_interaction = Instant::now();
        self.preview_idle_prefetched_time = None;
        self.preview_prefetch_in_flight
            .store(false, Ordering::Relaxed);
        self.preview_stats = None;
        self.preview_perf_samples.clear();
        self.preview_perf_sequence = 0;
        self.asset_thumbnails.clear();
        self.asset_thumbnail_misses.clear();
        self.asset_source_dimensions.clear();
        self.asset_source_dimension_misses.clear();
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
        self.timeline_last_scrub_audio_time = None;
        self.preview_auto_fit = true;
        self.preview_zoom = 1.0;
        self.preview_pan = Vec2::ZERO;
        self.preview_drag = None;
        self.preview_snap_guides.clear();
        self.generation_active = None;
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

    fn open_asset_lab(&mut self, asset_id: Uuid) {
        self.open_asset_lab_at_time(asset_id, None);
    }

    fn open_asset_lab_at_time(&mut self, asset_id: Uuid, local_time_seconds: Option<f64>) {
        let selected_version = self
            .editor
            .project
            .generative_config(asset_id)
            .and_then(preferred_asset_lab_version)
            .or_else(|| {
                self.editor
                    .project
                    .find_asset(asset_id)
                    .and_then(|asset| asset.active_version().map(str::to_string))
            });
        self.asset_lab = AssetLabState {
            asset_id: Some(asset_id),
            selected_version,
            pending_delete_version: None,
            local_time_seconds: local_time_seconds.unwrap_or(0.0).max(0.0),
            preview_auto_fit: true,
            preview_zoom: 1.0,
            preview_pan: Vec2::ZERO,
            preview_pan_drag: None,
        };
        self.asset_lab_preview_texture = None;
        self.editor.overlays.asset_lab = true;
    }

    fn close_asset_lab(&mut self) {
        self.editor.overlays.asset_lab = false;
        self.asset_lab = AssetLabState::default();
        self.asset_lab_preview_texture = None;
    }

    fn keyboard_shortcuts_suppressed(&self, ctx: &Context) -> bool {
        ctx.text_edit_focused() || self.modal_background_input_blocked()
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
            self.timeline_snap_targets(None, None, false)
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

    fn keep_automation_responsive(&self, ctx: &Context) {
        if crate::core::automation::is_enabled() {
            ctx.request_repaint_after(Duration::from_millis(50));
        }
    }

    fn handle_automation_screenshot_events(&mut self, ctx: &Context) {
        if self.pending_automation_screenshot.is_none() {
            return;
        }

        let screenshot = ctx.input(|input| {
            input.events.iter().find_map(|event| match event {
                egui::Event::Screenshot { image, .. } => Some(Arc::clone(image)),
                _ => None,
            })
        });

        if let Some(image) = screenshot {
            let pending = self
                .pending_automation_screenshot
                .take()
                .expect("pending screenshot checked above");
            match save_color_image_png(&pending.path, &image) {
                Ok(()) => {
                    pending
                        .envelope
                        .respond(crate::core::automation::AutomationResponse::ok(
                            serde_json::json!({ "path": pending.path }),
                        ))
                }
                Err(err) => pending.envelope.respond(
                    crate::core::automation::AutomationResponse::with_status(err, 500),
                ),
            }
            return;
        }

        let expired = self
            .pending_automation_screenshot
            .as_ref()
            .map(|pending| pending.requested_at.elapsed() > Duration::from_secs(18))
            .unwrap_or(false);
        if expired {
            let pending = self
                .pending_automation_screenshot
                .take()
                .expect("pending screenshot checked above");
            pending
                .envelope
                .respond(crate::core::automation::AutomationResponse::with_status(
                    "Timed out waiting for eframe screenshot event.",
                    500,
                ));
        }
    }

    fn finish_automation_ui_actions(&mut self) {
        if self.pending_automation_ui_actions.is_empty() {
            return;
        }

        let pending = std::mem::take(&mut self.pending_automation_ui_actions);
        for action in pending {
            if crate::core::automation::was_action_consumed(&action.id) {
                action
                    .envelope
                    .respond(crate::core::automation::AutomationResponse::ok(
                        serde_json::json!({
                            "id": action.id,
                            "action": action.action,
                        }),
                    ));
            } else {
                crate::core::automation::clear_pending_ui_action(&action.id);
                action.envelope.respond(
                    crate::core::automation::AutomationResponse::conflict(format!(
                        "UI element {} was visible in the previous frame but did not consume the queued {} action. It may have disappeared or is not instrumented yet.",
                        action.id, action.action
                    )),
                );
            }
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
        } else {
            self.timeline_last_scrub_audio_time = None;
        }
        engine.seek_seconds(self.editor.current_time);
        if scrub_audio && !self.editor.is_playing {
            engine.set_scrub_hold(true);
            let frame_epsilon = (0.5 / self.editor.project.settings.fps.max(1.0)).max(0.000_001);
            let should_preview = self
                .timeline_last_scrub_audio_time
                .map(|last| (last - self.editor.current_time).abs() > frame_epsilon)
                .unwrap_or(true);
            if should_preview {
                self.timeline_last_scrub_audio_time = Some(self.editor.current_time);
                engine.trigger_scrub_preview(
                    ((engine.sample_rate() as f64) * TIMELINE_SCRUB_PREVIEW_SECONDS).round() as u64,
                );
                engine.play();
            }
        }
    }

    fn toggle_playback(&mut self) {
        let next_playing = !self.editor.is_playing;
        if let Some(engine) = self.audio_engine.as_ref().map(Arc::clone) {
            if next_playing {
                self.refresh_audio_playback_items();
                engine.set_scrub_hold(false);
                self.timeline_last_scrub_audio_time = None;
                engine.seek_seconds(self.editor.current_time);
                engine.play();
            } else {
                engine.set_scrub_hold(false);
                self.timeline_last_scrub_audio_time = None;
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
            self.timeline_last_scrub_audio_time = None;
            return;
        };
        engine.set_scrub_hold(false);
        self.timeline_last_scrub_audio_time = None;
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
        self.poll_preview_render_results(ctx);
        if !self.editor.preview_dirty && self.preview_layers.is_some() {
            return;
        }
        if self.editor.project.project_path.is_none() {
            self.preview_layers = None;
            return;
        }

        self.schedule_preview_render(ctx);
    }

    fn render_preview_sync_for_profile(&mut self, ctx: &Context) -> PreviewStats {
        self.invalidate_preview_render_jobs();
        let decode_mode = if self.editor.is_playing {
            PreviewDecodeMode::Sequential
        } else {
            PreviewDecodeMode::Seek
        };
        let output = self.editor.previewer.render_layers(
            &self.editor.project,
            self.editor.current_time,
            decode_mode,
            self.editor.layout.hardware_decode,
        );
        let mut stats = output.stats;
        let Some(layers) = output.layers else {
            self.preview_layers = None;
            self.preview_stats = Some(stats.clone());
            self.record_preview_perf_sample(
                self.editor.current_time,
                stats.clone(),
                None,
                None,
                None,
            );
            self.editor.preview_dirty = false;
            return stats;
        };

        let upload_start = Instant::now();
        self.prepare_preview_layer_textures(ctx, &layers);
        stats.encode_ms = upload_start.elapsed().as_secs_f64() * 1000.0;
        stats.total_ms += stats.encode_ms;
        self.record_preview_perf_sample(self.editor.current_time, stats.clone(), None, None, None);
        self.preview_stats = Some(stats);
        let direction = self.preview_render_direction(self.editor.current_time);
        self.preview_last_interaction = Instant::now();
        self.preview_idle_prefetched_time = None;
        self.preview_layers = Some(layers);
        self.editor.preview_dirty = false;
        self.schedule_preview_prefetch(direction, decode_mode);
        if !self.editor.is_playing {
            ctx.request_repaint_after(Duration::from_millis(PREVIEW_IDLE_PREFETCH_DELAY_MS + 25));
        }
        self.preview_stats.clone().unwrap_or_default()
    }

    fn poll_preview_render_results(&mut self, ctx: &Context) {
        let mut latest = None;
        while let Ok(result) = self.preview_render_rx.try_recv() {
            latest = Some(result);
        }

        let Some(result) = latest else {
            return;
        };

        self.preview_render_busy_since = None;
        let latest_id = self.preview_render_request_id.load(Ordering::Relaxed);
        let time_matches = (result.time_seconds - self.editor.current_time).abs() < 0.0001;
        if result.request_id != latest_id
            || !time_matches
            || self.editor.project.project_path.is_none()
        {
            self.preview_render_stale_count = self.preview_render_stale_count.saturating_add(1);
            self.editor.preview_dirty = true;
            ctx.request_repaint();
            return;
        }

        let worker_ms = result
            .finished_at
            .saturating_duration_since(result.requested_at)
            .as_secs_f64()
            * 1000.0;
        let delivery_ms = result.requested_at.elapsed().as_secs_f64() * 1000.0;
        let mut stats = result.output.stats;
        let Some(layers) = result.output.layers else {
            self.preview_layers = None;
            self.preview_stats = Some(stats.clone());
            self.preview_render_completed_count =
                self.preview_render_completed_count.saturating_add(1);
            self.preview_render_last_worker_ms = Some(worker_ms);
            self.preview_render_last_delivery_ms = Some(delivery_ms);
            self.record_preview_perf_sample(
                result.time_seconds,
                stats,
                Some(result.request_id),
                Some(worker_ms),
                Some(delivery_ms),
            );
            self.editor.preview_dirty = false;
            return;
        };

        let upload_start = Instant::now();
        self.prepare_preview_layer_textures(ctx, &layers);
        stats.encode_ms = upload_start.elapsed().as_secs_f64() * 1000.0;
        stats.total_ms += stats.encode_ms;
        self.preview_stats = Some(stats.clone());
        self.preview_layers = Some(layers);
        self.preview_render_completed_count = self.preview_render_completed_count.saturating_add(1);
        self.preview_render_last_worker_ms = Some(worker_ms);
        self.preview_render_last_delivery_ms = Some(delivery_ms);
        self.record_preview_perf_sample(
            result.time_seconds,
            stats,
            Some(result.request_id),
            Some(worker_ms),
            Some(delivery_ms),
        );
        let direction = self.preview_render_direction(result.time_seconds);
        self.editor.preview_dirty = false;
        self.schedule_preview_prefetch(direction, result.decode_mode);
        if !self.editor.is_playing {
            ctx.request_repaint_after(Duration::from_millis(PREVIEW_IDLE_PREFETCH_DELAY_MS + 25));
        }
    }

    fn schedule_preview_render(&mut self, ctx: &Context) {
        if self
            .preview_render_in_flight
            .compare_exchange(false, true, Ordering::Relaxed, Ordering::Relaxed)
            .is_err()
        {
            ctx.request_repaint_after(Duration::from_millis(PREVIEW_RENDER_RETRY_MS));
            return;
        }

        let request_id = self
            .preview_render_request_id
            .fetch_add(1, Ordering::Relaxed)
            .wrapping_add(1);
        let time_seconds = self.editor.current_time;
        let decode_mode = if self.editor.is_playing {
            PreviewDecodeMode::Sequential
        } else {
            PreviewDecodeMode::Seek
        };
        let project = self.editor.project.clone();
        let renderer = Arc::clone(&self.editor.previewer);
        let allow_hw_decode = self.editor.layout.hardware_decode;
        let tx = self.preview_render_tx.clone();
        let flag = Arc::clone(&self.preview_render_in_flight);
        let repaint_ctx = ctx.clone();
        let requested_at = Instant::now();
        self.preview_render_busy_since = Some(requested_at);
        self.preview_last_interaction = requested_at;
        self.preview_idle_prefetched_time = None;

        std::thread::spawn(move || {
            let output =
                renderer.render_layers(&project, time_seconds, decode_mode, allow_hw_decode);
            let finished_at = Instant::now();
            let _ = tx.send(PreviewRenderResult {
                request_id,
                time_seconds,
                decode_mode,
                requested_at,
                finished_at,
                output,
            });
            flag.store(false, Ordering::Relaxed);
            repaint_ctx.request_repaint();
        });
    }

    fn invalidate_preview_render_jobs(&mut self) {
        self.preview_render_request_id
            .fetch_add(1, Ordering::Relaxed);
        self.preview_render_in_flight
            .store(false, Ordering::Relaxed);
        self.preview_render_busy_since = None;
        while self.preview_render_rx.try_recv().is_ok() {}
    }

    fn preview_render_direction(&mut self, time: f64) -> i32 {
        let direction = match self.preview_last_render_time {
            Some(last) if time > last + 0.0001 => 1,
            Some(last) if time < last - 0.0001 => -1,
            _ => 0,
        };
        self.preview_last_render_time = Some(time);
        direction
    }

    fn schedule_preview_prefetch(&mut self, direction: i32, decode_mode: PreviewDecodeMode) {
        if direction == 0 || self.editor.project.project_path.is_none() {
            return;
        }
        let seconds = if self.editor.is_playing {
            PREVIEW_PREFETCH_PLAYBACK_SECONDS
        } else {
            PREVIEW_PREFETCH_SCRUB_SECONDS
        };
        let fps = self.editor.project.settings.fps.max(1.0);
        let frames = (fps * seconds).round().max(1.0) as u32;
        self.schedule_preview_prefetch_windows(vec![(direction, frames)], decode_mode);
    }

    fn schedule_preview_prefetch_windows(
        &mut self,
        windows: Vec<(i32, u32)>,
        decode_mode: PreviewDecodeMode,
    ) -> bool {
        if windows.is_empty() || self.editor.project.project_path.is_none() {
            return false;
        }
        if self
            .preview_prefetch_in_flight
            .compare_exchange(false, true, Ordering::Relaxed, Ordering::Relaxed)
            .is_err()
        {
            return false;
        }

        let project = self.editor.project.clone();
        let renderer = Arc::clone(&self.editor.previewer);
        let time = self.editor.current_time;
        let allow_hw_decode = self.editor.layout.hardware_decode;
        let flag = Arc::clone(&self.preview_prefetch_in_flight);
        std::thread::spawn(move || {
            for (direction, frames) in windows {
                renderer.prefetch_frames(
                    &project,
                    time,
                    direction,
                    frames,
                    decode_mode,
                    allow_hw_decode,
                );
            }
            flag.store(false, Ordering::Relaxed);
        });
        true
    }

    fn service_preview_idle_prefetch(&mut self, ctx: &Context) {
        if self.editor.project.project_path.is_none()
            || self.editor.is_playing
            || self.editor.preview_dirty
            || self.preview_layers.is_none()
        {
            return;
        }

        let elapsed = self.preview_last_interaction.elapsed();
        let delay = Duration::from_millis(PREVIEW_IDLE_PREFETCH_DELAY_MS);
        if elapsed < delay {
            ctx.request_repaint_after(delay - elapsed);
            return;
        }

        let time = self.editor.current_time;
        if self
            .preview_idle_prefetched_time
            .map(|last| (last - time).abs() < 0.0001)
            .unwrap_or(false)
        {
            return;
        }

        let fps = self.editor.project.settings.fps.max(1.0);
        let ahead_frames = (fps * PREVIEW_IDLE_PREFETCH_AHEAD_SECONDS).round().max(1.0) as u32;
        let behind_frames = (fps * PREVIEW_IDLE_PREFETCH_BEHIND_SECONDS)
            .round()
            .max(1.0) as u32;
        let scheduled = self.schedule_preview_prefetch_windows(
            vec![(1, ahead_frames), (-1, behind_frames)],
            PreviewDecodeMode::Sequential,
        );
        if scheduled {
            self.preview_idle_prefetched_time = Some(time);
        } else {
            ctx.request_repaint_after(Duration::from_millis(80));
        }
    }

    fn prepare_preview_layer_textures(&mut self, ctx: &Context, layers: &PreviewLayerStack) {
        self.preview_layer_texture_sequence = self.preview_layer_texture_sequence.wrapping_add(1);
        let sequence = self.preview_layer_texture_sequence;

        for layer in layers.layers.iter() {
            let size = [
                layer.image.width().max(1) as usize,
                layer.image.height().max(1) as usize,
            ];
            if let Some(existing) = self.preview_layer_textures.get_mut(&layer.texture_key) {
                if existing.size == size {
                    existing.last_used = sequence;
                    continue;
                }
            }

            let image = ColorImage::from_rgba_unmultiplied(size, layer.image.as_raw());
            let texture = ctx.load_texture(
                format!("preview-layer-{}", layer.texture_key),
                image,
                TextureOptions::LINEAR,
            );
            self.preview_layer_textures.insert(
                layer.texture_key,
                PreviewLayerTexture {
                    texture,
                    size,
                    last_used: sequence,
                },
            );
        }

        if self.preview_layer_textures.len() > PREVIEW_LAYER_TEXTURE_LIMIT {
            let mut entries: Vec<(u64, u64)> = self
                .preview_layer_textures
                .iter()
                .map(|(key, texture)| (*key, texture.last_used))
                .collect();
            entries.sort_by_key(|(_, last_used)| *last_used);
            let evict_count = entries
                .len()
                .saturating_sub(PREVIEW_LAYER_TEXTURE_LIMIT)
                .max(PREVIEW_LAYER_TEXTURE_LIMIT / 8);
            for (key, _) in entries.into_iter().take(evict_count) {
                self.preview_layer_textures.remove(&key);
            }
        }
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

    fn asset_source_dimensions(&mut self, asset: &Asset) -> Option<Vec2> {
        if let Some(size) = self.asset_source_dimensions.get(&asset.id) {
            return Some(*size);
        }
        if self.asset_source_dimension_misses.contains(&asset.id) {
            return None;
        }

        let project_root = self.editor.project.project_path.as_deref()?;
        for path in asset_thumbnail_candidates(project_root, asset) {
            if let Ok((width, height)) = image::image_dimensions(&path) {
                let size = Vec2::new(width.max(1) as f32, height.max(1) as f32);
                self.asset_source_dimensions.insert(asset.id, size);
                return Some(size);
            }
        }

        self.asset_source_dimension_misses.insert(asset.id);
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
                            if automation_button(ui.button("New Project..."), "New Project...")
                                .clicked()
                            {
                                this.editor.overlays.new_project = true;
                                ui.close();
                            }
                            if automation_button(ui.button("Open Project..."), "Open Project...")
                                .clicked()
                            {
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
                                if automation_button(
                                    ui.button("Project Settings..."),
                                    "Project Settings...",
                                )
                                .clicked()
                                {
                                    this.project_settings = this.editor.project.settings.clone();
                                    this.editor.overlays.project_settings = true;
                                    ui.close();
                                }
                                if automation_button(ui.button("Save"), "Save").clicked() {
                                    if let Err(err) = this.editor.save() {
                                        this.editor.status = err;
                                    }
                                    ui.close();
                                }
                                if automation_button(
                                    ui.button("Export Video..."),
                                    "Export Video...",
                                )
                                .clicked()
                                {
                                    this.open_export_modal();
                                    ui.close();
                                }
                            });
                            ui.separator();
                            if automation_button(ui.button("Quit"), "Quit").clicked() {
                                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                                ui.close();
                            }
                        },
                        self,
                    );

                    menu_button(
                        ui,
                        "Edit",
                        |ui, this: &mut Self| {
                            if automation_button(ui.button("Add Marker"), "Add Marker").clicked() {
                                this.editor.add_marker(None);
                                ui.close();
                            }
                            if automation_button(
                                ui.button("Create Generative Video..."),
                                "Create Generative Video...",
                            )
                            .clicked()
                            {
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
                            automation_checkbox(
                                ui,
                                &mut this.editor.layout.preview_stats,
                                "Preview Stats",
                            );
                            let mut show_assets = !this.editor.layout.left_collapsed;
                            if automation_checkbox(ui, &mut show_assets, "Assets").changed() {
                                this.editor.layout.left_collapsed = !show_assets;
                            }
                            let mut show_attributes = !this.editor.layout.right_collapsed;
                            if automation_checkbox(ui, &mut show_attributes, "Attributes").changed()
                            {
                                this.editor.layout.right_collapsed = !show_attributes;
                            }
                            let mut show_timeline = !this.editor.layout.timeline_collapsed;
                            if automation_checkbox(ui, &mut show_timeline, "Timeline").changed() {
                                this.editor.layout.timeline_collapsed = !show_timeline;
                            }
                        },
                        self,
                    );

                    menu_button(
                        ui,
                        "Settings",
                        |ui, this: &mut Self| {
                            if automation_button(ui.button("AI Providers..."), "AI Providers...")
                                .clicked()
                            {
                                this.editor.refresh_providers();
                                this.editor.overlays.providers = true;
                                ui.close();
                            }
                            automation_checkbox(
                                ui,
                                &mut this.editor.layout.hardware_decode,
                                "Hardware Decode",
                            );
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
                            if automation_button(
                                ui.button("Open Harness Docs"),
                                "Open Harness Docs",
                            )
                            .clicked()
                            {
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

    fn project_panel_id(&self, name: &'static str) -> egui::Id {
        egui::Id::new((name, self.editor.project.project_path.clone()))
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
        if self.editor.overlays.export_video {
            self.export_video_modal(ctx);
        }
        if self.editor.overlays.queue {
            self.queue_panel(ctx);
        }
        if self.editor.overlays.providers {
            self.providers_modal(ctx);
        }
        if self.editor.overlays.api_keys {
            self.api_keys_modal(ctx);
        }
        if self.editor.overlays.asset_lab {
            self.asset_lab_modal(ctx);
        }
        if self.asset_delete_confirmation.is_some() {
            self.asset_delete_confirmation_modal(ctx);
        }
        if self.track_delete_confirmation.is_some() {
            self.track_delete_confirmation_modal(ctx);
        }
        if self.bridge_keyframe_confirmation.is_some() {
            self.bridge_keyframe_confirmation_modal(ctx);
        }
        if self.provider_json_editor_path.is_some() {
            self.provider_json_editor_modal(ctx);
        }
        if self.provider_builder_open {
            self.provider_builder_modal(ctx);
        }
    }

    fn asset_lab_modal(&mut self, ctx: &Context) {
        let Some(asset_id) = self.asset_lab.asset_id else {
            self.close_asset_lab();
            return;
        };
        let Some(asset) = self.editor.project.find_asset(asset_id).cloned() else {
            self.close_asset_lab();
            return;
        };

        let config_snapshot = self.editor.project.generative_config(asset_id).cloned();
        if asset.is_generative() {
            let selected_is_valid = self
                .asset_lab
                .selected_version
                .as_deref()
                .is_some_and(|version| asset_lab_version_exists(config_snapshot.as_ref(), version));
            if !selected_is_valid {
                self.asset_lab.selected_version = config_snapshot
                    .as_ref()
                    .and_then(preferred_asset_lab_version)
                    .or_else(|| asset.active_version().map(str::to_string));
            }
        }

        let mut open = true;
        let mut close_clicked = false;
        let mut action: Option<AssetLabAction> = None;
        let outside_clicked = kit::dismissible_modal_scrim(ctx, "asset_lab", true);
        let size = modal_size(ctx, ASSET_LAB_MODAL_SIZE, [660.0, 460.0]);
        let subtitle = format!("{}  |  {}", asset.name, asset_kind_label(&asset.kind));

        egui::Window::new("Asset Lab")
            .title_bar(false)
            .order(egui::Order::Foreground)
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .fixed_size(size)
            .frame(kit::modal_frame())
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                close_clicked =
                    kit::modal_header_with_close(ui, "Asset Lab", Some(&subtitle), true);
                kit::modal_body(ui, |ui| {
                    self.asset_lab_modal_contents(
                        ui,
                        &asset,
                        config_snapshot.as_ref(),
                        &mut action,
                    );
                });
            });

        if let Some(action) = action {
            self.handle_asset_lab_action(asset_id, action);
        } else if close_clicked || outside_clicked || !open {
            self.close_asset_lab();
        }
    }

    fn asset_lab_modal_contents(
        &mut self,
        ui: &mut Ui,
        asset: &Asset,
        config: Option<&GenerativeConfig>,
        action: &mut Option<AssetLabAction>,
    ) {
        if !asset.is_generative() {
            self.asset_lab_basic_asset_contents(ui, asset, action);
            return;
        }

        let versions = config.map(sorted_generation_records).unwrap_or_default();
        let selected_version = self.asset_lab.selected_version.clone();
        let active_version = asset.active_version().map(str::to_string);
        let pending_delete = self.asset_lab.pending_delete_version.clone();

        StripBuilder::new(ui)
            .clip(true)
            .size(Size::exact(285.0))
            .size(Size::remainder().at_least(320.0))
            .horizontal(|mut strip| {
                strip.cell(|ui| {
                    kit::card_frame().show(ui, |ui| {
                        kit::field_label(ui, "Versions");
                        ui.add_space(kit::FORM_ROW_GAP);
                        if versions.is_empty() {
                            ui.label(kit::caption("No generated versions yet."));
                        } else {
                            let list_height = (ui.available_height() - 4.0).max(120.0);
                            egui::ScrollArea::vertical()
                                .id_salt(("asset_lab_versions", asset.id))
                                .max_height(list_height)
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    ui.spacing_mut().item_spacing.y = kit::FORM_ROW_GAP;
                                    for record in versions.iter() {
                                        let selected = selected_version.as_deref()
                                            == Some(record.version.as_str());
                                        let active = active_version.as_deref()
                                            == Some(record.version.as_str());
                                        let response =
                                            asset_lab_version_row(ui, record, selected, active);
                                        if response.clicked() {
                                            *action = Some(AssetLabAction::SelectVersion(
                                                record.version.clone(),
                                            ));
                                        }
                                    }
                                });
                        }
                    });
                });
                strip.cell(|ui| {
                    kit::card_frame().show(ui, |ui| {
                        let selected_output_path =
                            selected_version.as_deref().and_then(|version| {
                                self.editor.project.project_path.as_ref().and_then(|root| {
                                    generative_output_file_for_version(root, asset, Some(version))
                                })
                            });
                        let selected_video_duration = asset
                            .duration_seconds
                            .filter(|duration| *duration > 0.0)
                            .or_else(|| {
                                selected_output_path
                                    .as_deref()
                                    .filter(|_| asset.is_video())
                                    .and_then(probe_duration_seconds)
                            })
                            .unwrap_or(0.0)
                            .max(0.0);
                        let selected_video_fps =
                            asset_lab_video_fps(asset, self.editor.project.settings.fps);

                        let preview_timecode = (asset.is_video() && selected_output_path.is_some())
                            .then(|| {
                                let current = self
                                    .asset_lab
                                    .local_time_seconds
                                    .min(selected_video_duration);
                                format!(
                                    "{} / {}",
                                    timecode(current),
                                    timecode(selected_video_duration)
                                )
                            });
                        asset_lab_preview_header(ui, preview_timecode);
                        ui.add_space(kit::FORM_ROW_GAP);
                        let preview = self.asset_lab_preview_texture(
                            ui.ctx(),
                            asset,
                            selected_version.as_deref(),
                        );
                        asset_lab_preview(ui, asset, preview, &mut self.asset_lab);
                        if asset.is_video() && selected_output_path.is_some() {
                            self.asset_lab_video_scrubber(
                                ui,
                                selected_video_duration,
                                selected_video_fps,
                            );
                        }

                        ui.add_space(kit::ACTION_GAP);
                        let details_height = ui.available_height().max(80.0);
                        egui::ScrollArea::vertical()
                            .id_salt(("asset_lab_details", asset.id))
                            .max_height(details_height)
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                kit::field_label(ui, "Version Details");
                                ui.add_space(kit::FORM_ROW_GAP);
                                if let Some(version) = selected_version.as_deref() {
                                    asset_lab_meta_row(ui, "Version", version);
                                    if asset.is_video() {
                                        asset_lab_meta_row(
                                            ui,
                                            "Local Time",
                                            timecode(self.asset_lab.local_time_seconds),
                                        );
                                    }
                                    if let Some(record) = config.and_then(|config| {
                                        config
                                            .versions
                                            .iter()
                                            .find(|record| record.version == version)
                                    }) {
                                        asset_lab_meta_row(
                                            ui,
                                            "Created",
                                            record
                                                .timestamp
                                                .with_timezone(&chrono::Local)
                                                .format("%Y-%m-%d %H:%M:%S")
                                                .to_string(),
                                        );
                                        asset_lab_meta_row(
                                            ui,
                                            "Provider",
                                            asset_lab_provider_name(
                                                &self.editor.provider_entries,
                                                record.provider_id,
                                            ),
                                        );
                                        asset_lab_meta_row(
                                            ui,
                                            "Inputs",
                                            format!("{} captured", record.inputs_snapshot.len()),
                                        );
                                    }
                                    if let Some(path) = selected_output_path.as_ref() {
                                        asset_lab_meta_row(ui, "File", path_label(path));
                                    }
                                } else {
                                    ui.label(kit::caption("Select a generated version."));
                                }

                                ui.add_space(kit::ACTION_GAP);
                                self.asset_lab_action_rows(
                                    ui,
                                    asset,
                                    selected_version.as_deref(),
                                    active_version.as_deref(),
                                    pending_delete.as_deref(),
                                    action,
                                );
                            });
                    });
                });
            });
    }

    fn asset_lab_basic_asset_contents(
        &mut self,
        ui: &mut Ui,
        asset: &Asset,
        action: &mut Option<AssetLabAction>,
    ) {
        kit::card_frame().show(ui, |ui| {
            let media_path = self
                .editor
                .project
                .project_path
                .as_ref()
                .and_then(|root| asset_lab_media_path(root, asset, None));
            let media_duration = asset
                .duration_seconds
                .filter(|duration| *duration > 0.0)
                .or_else(|| {
                    media_path
                        .as_deref()
                        .filter(|_| asset.is_video())
                        .and_then(probe_duration_seconds)
                })
                .unwrap_or(0.0)
                .max(0.0);
            let media_fps = asset_lab_video_fps(asset, self.editor.project.settings.fps);

            let preview_timecode = (asset.is_video() && media_path.is_some()).then(|| {
                let current = self.asset_lab.local_time_seconds.min(media_duration);
                format!("{} / {}", timecode(current), timecode(media_duration))
            });
            asset_lab_preview_header(ui, preview_timecode);
            ui.add_space(kit::FORM_ROW_GAP);
            let preview = self.asset_lab_preview_texture(ui.ctx(), asset, None);
            asset_lab_preview(ui, asset, preview, &mut self.asset_lab);
            if asset.is_video() && media_path.is_some() {
                self.asset_lab_video_scrubber(ui, media_duration, media_fps);
            }

            ui.add_space(kit::ACTION_GAP);
            let details_height = ui.available_height().max(80.0);
            egui::ScrollArea::vertical()
                .id_salt(("asset_lab_basic_details", asset.id))
                .max_height(details_height)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    kit::field_label(ui, "Asset Details");
                    ui.add_space(kit::FORM_ROW_GAP);
                    asset_lab_meta_row(ui, "Name", asset_display_name(asset));
                    asset_lab_meta_row(ui, "Type", asset_kind_label(&asset.kind));
                    if let Some(dimensions) = self.asset_source_dimensions(asset) {
                        asset_lab_meta_row(
                            ui,
                            "Dimensions",
                            format!("{:.0} x {:.0}", dimensions.x, dimensions.y),
                        );
                    }
                    if asset.is_video() {
                        asset_lab_meta_row(
                            ui,
                            "Local Time",
                            timecode(self.asset_lab.local_time_seconds),
                        );
                        if media_duration > 0.0 {
                            asset_lab_meta_row(ui, "Duration", timecode(media_duration));
                        }
                    }
                    if let Some(source) = asset_source_label(asset) {
                        asset_lab_meta_row(ui, "Source", source);
                    }
                    if let Some(path) = media_path.as_ref() {
                        asset_lab_meta_row(ui, "File", path_label(path));
                    }

                    ui.add_space(kit::ACTION_GAP);
                    self.asset_lab_basic_action_rows(ui, action);
                });
        });
    }

    fn asset_lab_basic_action_rows(&mut self, ui: &mut Ui, action: &mut Option<AssetLabAction>) {
        kit::equal_width_action_row(
            ui,
            2,
            kit::SECONDARY_BUTTON_H,
            kit::FIELD_COMPOUND_GAP,
            |ui, index, width| match index {
                0 => {
                    if kit::secondary_button(ui, "Duplicate Asset", width).clicked() {
                        *action = Some(AssetLabAction::DuplicateAsset);
                    }
                }
                _ => {
                    if kit::secondary_button(ui, "Add to Timeline", width).clicked() {
                        *action = Some(AssetLabAction::AddAssetToTimeline);
                    }
                }
            },
        );
        ui.add_space(kit::FORM_ROW_GAP);
        if kit::secondary_button(ui, "Open Location", ui.available_width()).clicked() {
            *action = Some(AssetLabAction::OpenLocation);
        }
        ui.add_space(kit::FORM_ROW_GAP);
        if kit::danger_button(ui, "Delete Asset...", ui.available_width()).clicked() {
            *action = Some(AssetLabAction::RequestDeleteAsset);
        }
    }

    fn asset_lab_action_rows(
        &mut self,
        ui: &mut Ui,
        asset: &Asset,
        selected_version: Option<&str>,
        active_version: Option<&str>,
        pending_delete: Option<&str>,
        action: &mut Option<AssetLabAction>,
    ) {
        let Some(version) = selected_version else {
            return;
        };

        if let Some(pending_version) = pending_delete {
            ui.label(
                RichText::new(format!(
                    "Delete {pending_version}? This removes its output file."
                ))
                .color(kit::DANGER)
                .strong(),
            );
            ui.add_space(kit::FORM_ROW_GAP);
            kit::equal_width_action_row(
                ui,
                2,
                kit::SECONDARY_BUTTON_H,
                kit::FIELD_COMPOUND_GAP,
                |ui, index, width| match index {
                    0 => {
                        if kit::secondary_button(ui, "Cancel", width).clicked() {
                            *action = Some(AssetLabAction::CancelDelete);
                        }
                    }
                    _ => {
                        if kit::danger_button(ui, "Delete Version", width).clicked() {
                            *action =
                                Some(AssetLabAction::ConfirmDelete(pending_version.to_string()));
                        }
                    }
                },
            );
            return;
        }

        let can_set_active = active_version != Some(version);
        kit::equal_width_action_row(
            ui,
            2,
            kit::SECONDARY_BUTTON_H,
            kit::FIELD_COMPOUND_GAP,
            |ui, index, width| match index {
                0 => {
                    ui.add_enabled_ui(can_set_active, |ui| {
                        if kit::secondary_button(ui, "Set Active", width).clicked() {
                            *action = Some(AssetLabAction::SetActive(version.to_string()));
                        }
                    });
                }
                _ => {
                    if kit::secondary_button(ui, "Duplicate Version", width).clicked() {
                        *action = Some(AssetLabAction::DuplicateVersion(version.to_string()));
                    }
                }
            },
        );
        ui.add_space(kit::FORM_ROW_GAP);
        kit::equal_width_action_row(
            ui,
            2,
            kit::SECONDARY_BUTTON_H,
            kit::FIELD_COMPOUND_GAP,
            |ui, index, width| match index {
                0 => {
                    if kit::secondary_button(ui, "Extract Version", width).clicked() {
                        *action = Some(AssetLabAction::ExtractVersion(version.to_string()));
                    }
                }
                _ => {
                    if asset.is_video() {
                        if kit::secondary_button(ui, "Extract Current Frame", width).clicked() {
                            *action =
                                Some(AssetLabAction::ExtractCurrentFrame(version.to_string()));
                        }
                    } else if kit::secondary_button(ui, "Open Location", width).clicked() {
                        *action = Some(AssetLabAction::OpenLocation);
                    }
                }
            },
        );
        ui.add_space(kit::FORM_ROW_GAP);
        if asset.is_video() {
            if kit::secondary_button(ui, "Open Location", ui.available_width()).clicked() {
                *action = Some(AssetLabAction::OpenLocation);
            }
            ui.add_space(kit::FORM_ROW_GAP);
        }
        if kit::danger_button(ui, "Delete Version", ui.available_width()).clicked() {
            *action = Some(AssetLabAction::RequestDelete(version.to_string()));
        }

        if active_version != Some(version) {
            ui.add_space(kit::FORM_ROW_GAP);
            ui.label(kit::caption(
                "Timeline and preview use the active version. Set this version active to route it into existing clips.",
            ));
        }
        if !asset.is_visual() {
            ui.add_space(kit::FORM_ROW_GAP);
            ui.label(kit::caption(
                "Non-visual assets do not show preview thumbnails yet.",
            ));
        }
    }

    fn asset_lab_video_scrubber(&mut self, ui: &mut Ui, duration: f64, fps: f64) {
        let duration = duration.max(0.0);
        let fps = fps.max(1.0);

        if !ui.ctx().text_edit_focused() {
            let (left, right) = ui.input(|input| {
                (
                    input.key_pressed(egui::Key::ArrowLeft),
                    input.key_pressed(egui::Key::ArrowRight),
                )
            });
            if left {
                self.asset_lab.local_time_seconds =
                    previous_frame_time(self.asset_lab.local_time_seconds, fps).min(duration);
                self.asset_lab_preview_texture = None;
            }
            if right {
                self.asset_lab.local_time_seconds =
                    next_frame_time(self.asset_lab.local_time_seconds, duration, fps);
                self.asset_lab_preview_texture = None;
            }
        }

        ui.add_space(ASSET_LAB_PREVIEW_SCRUB_GAP);
        let desired_size = Vec2::new(ui.available_width(), 42.0);
        let (rect, response) = ui.allocate_exact_size(desired_size, Sense::click_and_drag());

        if duration > 0.0 && (response.clicked() || response.dragged()) {
            if let Some(pointer) = response.interact_pointer_pos() {
                let fraction = ((pointer.x - rect.left()) / rect.width().max(1.0)).clamp(0.0, 1.0);
                let raw_time = duration * fraction as f64;
                let frame = frames_from_seconds(raw_time, fps).round();
                self.asset_lab.local_time_seconds = seconds_from_frames(frame, fps).min(duration);
                self.asset_lab_preview_texture = None;
            }
        }

        let painter = ui.painter().with_clip_rect(rect);
        painter.rect_filled(rect, kit::field_radius(), kit::FIELD_BG);
        painter.rect_stroke(
            rect,
            kit::field_radius(),
            Stroke::new(1.0, kit::BORDER_SOFT),
            egui::StrokeKind::Inside,
        );

        let inner = rect.shrink2(Vec2::new(8.0, 7.0));
        if duration > 0.0 && inner.width() > 1.0 {
            let step = asset_lab_scrub_tick_step(duration, inner.width());
            let mut tick = 0.0;
            while tick <= duration + step * 0.5 {
                let x = inner.left() + inner.width() * (tick / duration).clamp(0.0, 1.0) as f32;
                painter.line_segment(
                    [
                        Pos2::new(x, inner.top() + 12.0),
                        Pos2::new(x, inner.bottom() - 6.0),
                    ],
                    Stroke::new(1.0, kit::BORDER_SOFT),
                );
                painter.text(
                    Pos2::new(x + 3.0, inner.top()),
                    egui::Align2::LEFT_TOP,
                    timecode(tick),
                    FontId::monospace(9.0),
                    kit::TEXT_DIM,
                );
                tick += step;
            }

            let current = self.asset_lab.local_time_seconds.min(duration);
            let x = inner.left() + inner.width() * (current / duration).clamp(0.0, 1.0) as f32;
            painter.line_segment(
                [Pos2::new(x, inner.top()), Pos2::new(x, inner.bottom())],
                Stroke::new(2.0, kit::PLAYHEAD),
            );
            painter.circle_filled(Pos2::new(x, inner.top()), 4.0, kit::PLAYHEAD);
        } else {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "00:00.00",
                FontId::monospace(10.0),
                kit::TEXT_DIM,
            );
        }
    }

    fn handle_asset_lab_action(&mut self, asset_id: Uuid, action: AssetLabAction) {
        match action {
            AssetLabAction::SelectVersion(version) => {
                self.asset_lab.selected_version = Some(version);
                self.asset_lab.pending_delete_version = None;
                self.asset_lab_preview_texture = None;
            }
            AssetLabAction::SetActive(version) => {
                self.set_generative_active_version(asset_id, &version);
                self.asset_lab.selected_version = Some(version);
                self.asset_lab.pending_delete_version = None;
            }
            AssetLabAction::DuplicateVersion(version) => {
                self.duplicate_generative_version(asset_id, &version);
                self.asset_lab.pending_delete_version = None;
            }
            AssetLabAction::ExtractVersion(version) => {
                match self
                    .editor
                    .extract_generation_version_as_asset(asset_id, Some(&version))
                {
                    Ok(new_asset_id) => self.warm_asset_thumbnails(&[new_asset_id]),
                    Err(err) => self.editor.status = err,
                }
            }
            AssetLabAction::ExtractCurrentFrame(version) => {
                self.extract_asset_lab_current_frame(asset_id, &version);
            }
            AssetLabAction::DuplicateAsset => match self.editor.duplicate_asset(asset_id) {
                Ok(new_asset_id) => self.warm_asset_thumbnails(&[new_asset_id]),
                Err(err) => self.editor.status = err,
            },
            AssetLabAction::AddAssetToTimeline => {
                match self.editor.add_asset_to_timeline(asset_id, None) {
                    Ok(_) => {
                        self.editor.status = "Added asset to timeline".to_string();
                    }
                    Err(err) => self.editor.status = err,
                }
            }
            AssetLabAction::RequestDeleteAsset => {
                self.close_asset_lab();
                self.request_delete_assets(&[asset_id]);
            }
            AssetLabAction::RequestDelete(version) => {
                self.asset_lab.pending_delete_version = Some(version);
            }
            AssetLabAction::ConfirmDelete(version) => {
                self.delete_generative_version(asset_id, &version);
                self.asset_lab.pending_delete_version = None;
            }
            AssetLabAction::CancelDelete => {
                self.asset_lab.pending_delete_version = None;
            }
            AssetLabAction::OpenLocation => {
                let result = self
                    .editor
                    .project
                    .project_path
                    .as_ref()
                    .and_then(|root| {
                        self.editor
                            .project
                            .find_asset(asset_id)
                            .and_then(|asset| asset_lab_location_path(root, asset))
                    })
                    .ok_or_else(|| "Project folder is unavailable.".to_string())
                    .and_then(|path| open_path_in_file_manager(&path));
                if let Err(err) = result {
                    self.editor.status = err;
                }
            }
        }
    }

    fn extract_asset_lab_current_frame(&mut self, asset_id: Uuid, version: &str) {
        let Some(project_root) = self.editor.project.project_path.clone() else {
            self.editor.status = "Project folder is unavailable.".to_string();
            return;
        };
        let Some(asset) = self.editor.project.find_asset(asset_id).cloned() else {
            self.editor.status = "Asset not found.".to_string();
            return;
        };
        if !asset.is_video() {
            self.editor.status =
                "Current-frame extraction is only available for video assets.".to_string();
            return;
        }
        let Some(path) = generative_output_file_for_version(&project_root, &asset, Some(version))
        else {
            self.editor.status = format!("No output file was found for {version}.");
            return;
        };

        let fps = asset_lab_video_fps(&asset, self.editor.project.settings.fps);
        let duration = probe_duration_seconds(&path)
            .or(asset.duration_seconds)
            .unwrap_or(0.0)
            .max(0.0);
        let frame_time = if duration > 0.0 {
            self.asset_lab.local_time_seconds.min(duration)
        } else {
            self.asset_lab.local_time_seconds.max(0.0)
        };
        let frame_index = frames_from_seconds(frame_time, fps).round().max(0.0);
        let snapped_time = seconds_from_frames(frame_index, fps);

        let Some(response) = self.asset_lab_video_decoder.decode(
            &path,
            snapped_time,
            0,
            self.editor.layout.hardware_decode,
        ) else {
            self.editor.status = "Could not decode current video frame.".to_string();
            return;
        };
        let Some(frame) = response.image else {
            self.editor.status = "Could not decode current video frame.".to_string();
            return;
        };

        match self
            .editor
            .add_extracted_frame_asset(asset_id, Some(version), snapped_time, frame)
        {
            Ok(new_asset_id) => self.warm_asset_thumbnails(&[new_asset_id]),
            Err(err) => self.editor.status = err,
        }
    }

    fn set_generative_active_version(&mut self, asset_id: Uuid, version: &str) {
        let config_snapshot = self.editor.project.generative_config(asset_id).cloned();
        let Some(record) = config_snapshot
            .as_ref()
            .and_then(|config| {
                config
                    .versions
                    .iter()
                    .find(|record| record.version == version)
            })
            .cloned()
        else {
            self.editor.status = format!("Version {version} was not found.");
            return;
        };

        self.editor
            .project
            .update_generative_config(asset_id, |config| {
                config.active_version = Some(version.to_string());
                config.provider_id = Some(record.provider_id);
                config.inputs = record.inputs_snapshot;
            });
        if let Err(err) = self.editor.project.save_generative_config(asset_id) {
            self.editor.status = format!("Failed to save active version: {err}");
            return;
        }
        self.invalidate_generative_asset_runtime(asset_id);
        self.editor.status = format!("Set {version} active.");
    }

    fn duplicate_generative_version(&mut self, asset_id: Uuid, version: &str) {
        let Some(project_root) = self.editor.project.project_path.clone() else {
            self.editor.status = "Project folder is unavailable.".to_string();
            return;
        };
        let Some(asset) = self.editor.project.find_asset(asset_id).cloned() else {
            self.editor.status = "Asset not found.".to_string();
            return;
        };
        let Some(folder) = generative_folder_for_asset(&asset).cloned() else {
            self.editor.status = "Asset has no generation folder.".to_string();
            return;
        };
        let Some(config_snapshot) = self.editor.project.generative_config(asset_id).cloned() else {
            self.editor.status = "Generation config was not found.".to_string();
            return;
        };
        let Some(source_record) = config_snapshot
            .versions
            .iter()
            .find(|record| record.version == version)
            .cloned()
        else {
            self.editor.status = format!("Version {version} was not found.");
            return;
        };
        let new_version = next_version_label(&config_snapshot);
        let folder_path = project_root.join(&folder);
        if let Err(err) = copy_generative_version_files(&folder_path, version, &new_version) {
            self.editor.status = err;
            return;
        }

        let new_record = GenerationRecord {
            version: new_version.clone(),
            timestamp: chrono::Utc::now(),
            provider_id: source_record.provider_id,
            inputs_snapshot: source_record.inputs_snapshot,
        };
        self.editor
            .project
            .update_generative_config(asset_id, |config| {
                config.active_version = Some(new_version.clone());
                config.provider_id = Some(new_record.provider_id);
                config.inputs = new_record.inputs_snapshot.clone();
                config.versions.push(new_record);
            });
        if let Err(err) = self.editor.project.save_generative_config(asset_id) {
            self.editor.status = format!("Duplicated version, but config save failed: {err}");
            return;
        }
        self.asset_lab.selected_version = Some(new_version.clone());
        self.invalidate_generative_asset_runtime(asset_id);
        self.editor.status = format!("Duplicated {version} as {new_version}.");
    }

    fn delete_generative_version(&mut self, asset_id: Uuid, version: &str) {
        let Some(project_root) = self.editor.project.project_path.clone() else {
            self.editor.status = "Project folder is unavailable.".to_string();
            return;
        };
        let Some(asset) = self.editor.project.find_asset(asset_id).cloned() else {
            self.editor.status = "Asset not found.".to_string();
            return;
        };
        let Some(folder) = generative_folder_for_asset(&asset).cloned() else {
            self.editor.status = "Asset has no generation folder.".to_string();
            return;
        };
        if let Err(err) = delete_generative_version_files(&project_root.join(folder), version) {
            self.editor.status = format!("Failed to delete version files: {err}");
            return;
        }

        self.editor
            .project
            .update_generative_config(asset_id, |config| {
                config.versions.retain(|record| record.version != version);
                if config.active_version.as_deref() == Some(version) {
                    config.active_version = preferred_asset_lab_version(config);
                    if let Some(active) = config.active_version.clone() {
                        if let Some(record) = config
                            .versions
                            .iter()
                            .find(|record| record.version == active)
                            .cloned()
                        {
                            config.provider_id = Some(record.provider_id);
                            config.inputs = record.inputs_snapshot;
                        }
                    }
                }
            });
        if let Err(err) = self.editor.project.save_generative_config(asset_id) {
            self.editor.status = format!("Deleted version, but config save failed: {err}");
            return;
        }
        let next_selected = self
            .editor
            .project
            .generative_config(asset_id)
            .and_then(preferred_asset_lab_version);
        self.asset_lab.selected_version = next_selected;
        self.invalidate_generative_asset_runtime(asset_id);
        self.editor.status = format!("Deleted {version}.");
    }

    fn invalidate_generative_asset_runtime(&mut self, asset_id: Uuid) {
        if let (Some(project_root), Some(asset)) = (
            self.editor.project.project_path.as_ref(),
            self.editor.project.find_asset(asset_id),
        ) {
            if let Some(folder) = generative_folder_for_asset(asset) {
                self.editor
                    .previewer
                    .invalidate_folder(&project_root.join(folder));
            }
        }
        self.invalidate_preview_render_jobs();
        self.preview_prefetch_in_flight
            .store(false, Ordering::Relaxed);
        self.preview_idle_prefetched_time = None;
        self.invalidate_asset_visual_cache(asset_id);
        self.asset_lab_preview_texture = None;
        self.editor.preview_dirty = true;
    }

    fn asset_lab_preview_texture(
        &mut self,
        ctx: &Context,
        asset: &Asset,
        version: Option<&str>,
    ) -> Option<(TextureId, Vec2)> {
        let project_root = self.editor.project.project_path.as_ref()?;
        let path = asset_lab_media_path(project_root, asset, version)?;
        if !asset.is_visual() {
            return None;
        }
        let fps = asset_lab_video_fps(asset, self.editor.project.settings.fps);
        let frame_index = asset.is_video().then(|| {
            frames_from_seconds(self.asset_lab.local_time_seconds.max(0.0), fps)
                .round()
                .max(0.0) as i64
        });

        if let Some(cached) = self.asset_lab_preview_texture.as_ref() {
            if cached.asset_id == asset.id
                && cached.version.as_deref() == version
                && cached.path == path
                && cached.frame_index == frame_index
            {
                return Some((cached.texture.id(), cached.size));
            }
        }

        let (image, size) = if asset.is_video() {
            let frame_time = seconds_from_frames(frame_index.unwrap_or(0) as f64, fps);
            let response = self.asset_lab_video_decoder.decode(
                &path,
                frame_time,
                0,
                self.editor.layout.hardware_decode,
            )?;
            let frame = response.image?;
            let size = Vec2::new(frame.width().max(1) as f32, frame.height().max(1) as f32);
            let image = ColorImage::from_rgba_unmultiplied(
                [frame.width() as usize, frame.height() as usize],
                frame.as_raw(),
            );
            (image, size)
        } else {
            load_preview_image(&path, 512)?
        };
        let texture = ctx.load_texture(
            format!(
                "asset-lab-preview-{}-{}-{}",
                asset.id,
                version.unwrap_or("fallback"),
                frame_index.unwrap_or(-1)
            ),
            image,
            TextureOptions::LINEAR,
        );
        let texture_id = texture.id();
        self.asset_lab_preview_texture = Some(AssetLabPreviewTexture {
            asset_id: asset.id,
            version: version.map(str::to_string),
            path,
            frame_index,
            texture,
            size,
        });
        Some((texture_id, size))
    }

    fn queue_panel(&mut self, ctx: &Context) {
        let mut close_clicked = false;
        let mut clear_clicked = false;
        let mut cancel_job_id = None;
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
        let has_clearable = jobs.iter().any(|job| queue_job_is_terminal(job.status));
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
                queue_body(&mut child, body_rect, &jobs, &mut cancel_job_id);
            });

        if let Some(job_id) = cancel_job_id {
            self.cancel_generation_job(job_id);
        }
        if clear_clicked {
            let before = self.editor.generation_queue.len();
            self.editor
                .generation_queue
                .retain(|job| !queue_job_is_terminal(job.status));
            let cleared = before.saturating_sub(self.editor.generation_queue.len());
            self.editor.status = if cleared == 1 {
                "Cleared 1 completed generation job.".to_string()
            } else {
                format!("Cleared {cleared} completed generation jobs.")
            };
        }
        if close_clicked {
            self.editor.overlays.queue = false;
        }
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

impl eframe::App for NlaEguiApp {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.handle_automation_screenshot_events(&ctx);
        self.poll_automation(&ctx);
        self.keep_automation_responsive(&ctx);
        crate::core::automation::begin_ui_frame();
        self.tick_playback(&ctx);
        self.service_generation_queue(&ctx);
        self.service_export_events(&ctx);
        self.update_preview_texture(&ctx);
        self.service_preview_idle_prefetch(&ctx);
        self.handle_app_keyboard(&ctx);

        self.top_bar(ui);
        // App-wide bars claim root space first; docked editor panels sit above the status bar.
        self.status_bar(ui);
        self.left_panel(ui);
        self.handle_asset_file_drops(&ctx);
        self.right_panel(ui);
        self.timeline_panel(ui);
        self.central_preview(ui);

        self.modals(&ctx);
        self.service_audio_decode_warmup(&ctx);
        self.finish_automation_ui_actions();
    }
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
