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

    fn left_panel(&mut self, root: &mut Ui) {
        if self.editor.layout.left_collapsed {
            let response = egui::Panel::left(self.project_panel_id("assets_collapsed"))
                .exact_size(kit::COLLAPSED_RAIL_W)
                .frame(kit::collapsed_dock_frame())
                .show_inside(root, |ui| {
                    if kit::collapsed_rail_button(ui, "▶").clicked() {
                        self.editor.layout.left_collapsed = false;
                    }
                });
            self.asset_drop_target_rect = Some(response.response.rect);
            self.asset_drop_target_hovered = response.response.hovered();
            kit::paint_panel_edge(root, response.response.rect, kit::PanelEdge::Right);
            return;
        }

        let response = egui::Panel::left(self.project_panel_id("assets"))
            .resizable(true)
            .default_size(self.editor.layout.left_width)
            .size_range(180.0..=420.0)
            .frame(kit::dock_frame())
            .show_inside(root, |ui| {
                kit::fixed_panel_body(ui, |ui| self.assets_panel(ui));
            });
        self.asset_drop_target_rect = Some(response.response.rect);
        self.asset_drop_target_hovered = response.response.hovered();
        self.editor.layout.left_width = response.response.rect.width().clamp(180.0, 420.0);
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
                    self.import_asset_files(vec![path]);
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
            let mut assets: Vec<(usize, Asset)> = self
                .editor
                .project
                .assets
                .iter()
                .cloned()
                .enumerate()
                .collect();
            assets.sort_by(|(a_index, a), (b_index, b)| {
                asset_natural_cmp(a, b).then_with(|| a_index.cmp(b_index))
            });
            for (_, asset) in assets {
                let selected = self.editor.selection.asset_ids.contains(&asset.id);
                let thumbnail = self.asset_thumbnail(ui.ctx(), &asset);
                let response = asset_row(ui, &asset, selected, thumbnail);
                response.dnd_set_drag_payload(AssetTimelineDragPayload { asset_id: asset.id });
                if response.clicked() {
                    if multi_select_modifier(ui) {
                        self.editor.selection.toggle_asset(asset.id);
                    } else {
                        self.editor.selection.select_asset(asset.id);
                    }
                }
                if response.double_clicked() {
                    self.open_asset_lab(asset.id);
                }
                response.context_menu(|ui| {
                    if automation_button(ui.button("Add to timeline"), "Add to timeline").clicked()
                    {
                        if let Err(err) = self.editor.add_asset_to_timeline(asset.id, None) {
                            self.editor.status = err;
                        }
                        ui.close();
                    }
                    if automation_button(ui.button("Duplicate"), "Duplicate").clicked() {
                        let asset_ids = if selected && self.editor.selection.asset_ids.len() > 1 {
                            self.editor.selection.asset_ids.clone()
                        } else {
                            vec![asset.id]
                        };
                        self.duplicate_assets(&asset_ids);
                        ui.close();
                    }
                    if asset.is_generative()
                        && automation_button(
                            ui.button("Extract active generation"),
                            "Extract active generation",
                        )
                        .clicked()
                    {
                        self.extract_active_generation(asset.id);
                        ui.close();
                    }
                    if automation_button(ui.button("Delete"), "Delete").clicked() {
                        let asset_ids = if selected && self.editor.selection.asset_ids.len() > 1 {
                            self.editor.selection.asset_ids.clone()
                        } else {
                            vec![asset.id]
                        };
                        self.request_delete_assets(&asset_ids);
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

    fn duplicate_assets(&mut self, asset_ids: &[Uuid]) {
        match self.editor.duplicate_assets(asset_ids) {
            Ok(new_asset_ids) => self.warm_asset_thumbnails(&new_asset_ids),
            Err(err) => self.editor.status = err,
        }
    }

    fn extract_active_generation(&mut self, asset_id: Uuid) {
        match self.editor.extract_active_generation_as_asset(asset_id) {
            Ok(new_asset_id) => self.warm_asset_thumbnails(&[new_asset_id]),
            Err(err) => self.editor.status = err,
        }
    }

    fn warm_asset_thumbnails(&mut self, asset_ids: &[Uuid]) {
        let Some(runtime) = self.generation_runtime.as_ref() else {
            return;
        };
        let thumbnailer = Arc::clone(&self.editor.thumbnailer);
        let assets: Vec<Asset> = asset_ids
            .iter()
            .filter_map(|asset_id| self.editor.project.find_asset(*asset_id).cloned())
            .filter(|asset| asset.is_visual())
            .collect();
        for asset in assets {
            let thumbnailer = Arc::clone(&thumbnailer);
            runtime.spawn(async move {
                let _ = thumbnailer.generate(&asset, true).await;
            });
        }
    }

    fn handle_asset_file_drops(&mut self, ctx: &Context) {
        let dropped_files = ctx.input(|input| input.raw.dropped_files.clone());
        if dropped_files.is_empty() {
            return;
        }

        let paths: Vec<PathBuf> = dropped_files
            .into_iter()
            .filter_map(|file| file.path)
            .collect();
        if paths.is_empty() {
            self.editor.status = "Dropped data did not include filesystem paths.".to_string();
            return;
        }

        let pointer_pos =
            ctx.input(|input| input.pointer.interact_pos().or(input.pointer.hover_pos()));
        let drop_in_assets = self
            .asset_drop_target_rect
            .is_some_and(|rect| pointer_pos.is_some_and(|pos| rect.contains(pos)))
            || self.asset_drop_target_hovered;
        let supported_media_drop = paths
            .iter()
            .any(|path| is_supported_asset_import_path(path));
        let fallback_media_drop = pointer_pos.is_none() && supported_media_drop;
        if !drop_in_assets && !fallback_media_drop {
            return;
        }

        if self.editor.project_root().is_none() {
            self.editor.status =
                "Open or create a project before dropping media files to import.".to_string();
            return;
        }

        self.import_asset_files(paths);
        if self.editor.layout.left_collapsed {
            self.editor.layout.left_collapsed = false;
        }
    }

    fn import_asset_files(&mut self, paths: Vec<PathBuf>) {
        if paths.is_empty() {
            return;
        }
        if self.editor.project_root().is_none() {
            self.editor.status =
                "Open or create a project before importing media files.".to_string();
            return;
        }

        let mut imported = Vec::new();
        let mut failures = Vec::new();
        for path in paths {
            match self.editor.import_asset(&path) {
                Ok(asset_id) => imported.push(asset_id),
                Err(err) => failures.push(format!("{}: {err}", path.display())),
            }
        }

        let imported_count = imported.len();
        if imported_count > 0 {
            self.editor.selection.asset_ids = imported;
            self.editor.selection.clip_ids.clear();
            self.editor.selection.marker_ids.clear();
            self.editor.selection.track_ids.clear();
        }

        match (imported_count, failures.len()) {
            (0, 0) => {}
            (imported_count, 0) => {
                self.editor.status = if imported_count == 1 {
                    "Asset imported".to_string()
                } else {
                    format!("Imported {imported_count} assets")
                };
            }
            (0, failed_count) => {
                self.editor.status = if failed_count == 1 {
                    failures
                        .pop()
                        .unwrap_or_else(|| "Failed to import dropped file.".to_string())
                } else {
                    format!("Failed to import {failed_count} files")
                };
            }
            (imported_count, failed_count) => {
                self.editor.status =
                    format!("Imported {imported_count} assets; {failed_count} files failed");
            }
        }
    }

    fn right_panel(&mut self, root: &mut Ui) {
        if self.editor.layout.right_collapsed {
            let response = egui::Panel::right(self.project_panel_id("attributes_collapsed"))
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

        let response = egui::Panel::right(self.project_panel_id("attributes"))
            .resizable(true)
            .default_size(self.editor.layout.right_width)
            .size_range(200.0..=440.0)
            .frame(kit::dock_frame())
            .show_inside(root, |ui| {
                kit::fixed_panel_body(ui, |ui| self.attributes_panel(ui));
            });
        self.editor.layout.right_width = response.response.rect.width().clamp(200.0, 440.0);
        kit::paint_panel_edge(root, response.response.rect, kit::PanelEdge::Left);
    }

    fn attributes_panel(&mut self, ui: &mut Ui) {
        kit::panel_header(ui, "ATTRIBUTES", Some("▶"), || {
            self.editor.layout.right_collapsed = true;
        });
        ui.add_space(8.0);

        kit::scroll_body(ui, |ui| {
            ui.spacing_mut().item_spacing.y = kit::FORM_ROW_GAP;
            if self.editor.selection.clip_ids.len() > 1 {
                self.multi_clip_attributes(ui);
            } else if let Some(clip_id) = self.editor.selected_clip_id() {
                self.clip_attributes(ui, clip_id);
            } else if self.editor.selection.asset_ids.len() > 1 {
                self.multi_asset_attributes(ui);
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

    fn multi_clip_attributes(&mut self, ui: &mut Ui) {
        let selected_ids = self.editor.selection.clip_ids.clone();
        let mut clips: Vec<Clip> = self
            .editor
            .project
            .clips
            .iter()
            .filter(|clip| selected_ids.contains(&clip.id))
            .cloned()
            .collect();
        clips.sort_by(|a, b| {
            a.start_time
                .partial_cmp(&b.start_time)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.id.cmp(&b.id))
        });

        inspector_card(ui, "Selection", |ui| {
            ui.label(kit::value(format!("{} clips selected", clips.len())));
            if clips.len() >= 2 {
                let first = clips.first().map(|clip| clip.start_time).unwrap_or(0.0);
                let last = clips.last().map(|clip| clip.start_time).unwrap_or(first);
                ui.add_space(kit::FORM_ROW_GAP);
                ui.label(kit::caption(format!(
                    "Start span {} - {}",
                    timecode(first),
                    timecode(last)
                )));
            }
        });

        ui.add_space(kit::FORM_ROW_GAP);
        let mut apply_spacing = false;
        let mut bridge_provider_id: Option<Option<Uuid>> = None;
        inspector_card(ui, "Spacing", |ui| {
            let fps = self.editor.project.settings.fps.max(1.0);
            let mut seconds = self.clip_spacing_seconds.max(0.0);
            let mut frames = self.clip_spacing_frames.max(1);
            let (left_rect, right_rect) = inspector_numeric_pair_rects(ui);
            if inspector_drag_f64_in_rect(ui, left_rect, "Seconds", &mut seconds, 0.05) {
                self.clip_spacing_seconds = seconds.max(0.0);
                self.clip_spacing_frames = frames_from_seconds(self.clip_spacing_seconds, fps)
                    .round()
                    .max(1.0) as i64;
            }
            if inspector_drag_i64_in_rect(ui, right_rect, "Frames", &mut frames, 1.0) {
                self.clip_spacing_frames = frames.max(1);
                self.clip_spacing_seconds =
                    seconds_from_frames(self.clip_spacing_frames as f64, fps);
            }
            ui.add_space(kit::FORM_ROW_GAP);
            automation_checkbox(
                ui,
                &mut self.clip_spacing_set_duration,
                "Set clip duration to interval",
            );
            ui.add_space(kit::ACTION_GAP);
            if kit::primary_button(ui, "Space Selected Clips", ui.available_width()).clicked() {
                apply_spacing = true;
            }
        });

        let visual_reference_clip_ids: Vec<Uuid> = clips
            .iter()
            .filter_map(|clip| {
                self.editor
                    .project
                    .find_asset(clip.asset_id)
                    .filter(|asset| asset.is_image() || asset.is_video())
                    .map(|_| clip.id)
            })
            .collect();
        if visual_reference_clip_ids.len() >= 2 {
            ui.add_space(kit::FORM_ROW_GAP);
            inspector_card(ui, "Generation", |ui| {
                ui.label(kit::caption(
                    "Create a new generative video asset using selected image keyframes or video boundaries as pinned references.",
                ));
                ui.add_space(kit::ACTION_GAP);
                ui.menu_button("Generate Between Keyframes", |ui| {
                    if let Some(provider_id) = self.provider_choice_menu(
                        ui,
                        ProviderWorkflowKind::FirstFrameLastFrameVideo,
                        "Configure provider later",
                    ) {
                        bridge_provider_id = Some(provider_id);
                        ui.close();
                    }
                });
            });
        }

        if apply_spacing && clips.len() >= 2 {
            self.space_selected_clips(&clips);
        }
        if let Some(provider_id) = bridge_provider_id {
            self.request_bridge_video_from_selected_clips(&clips, provider_id);
        }
    }

    fn multi_asset_attributes(&mut self, ui: &mut Ui) {
        let count = self.editor.selection.asset_ids.len();
        inspector_card(ui, "Selection", |ui| {
            ui.label(kit::value(format!("{count} assets selected")));
            ui.add_space(kit::FORM_ROW_GAP);
            ui.label(kit::caption(
                "Timeline and generation actions are available after placing assets as clips.",
            ));
        });
    }

    fn space_selected_clips(&mut self, clips: &[Clip]) {
        let interval = self.clip_spacing_seconds.max(0.0);
        if clips.len() < 2 || interval <= 0.0 {
            self.editor.status =
                "Select at least two clips and use a positive interval.".to_string();
            return;
        }

        let mut previous_anchor = None;
        for clip in clips.iter() {
            let start_time = previous_anchor
                .map(|anchor| anchor + interval)
                .unwrap_or(clip.start_time);
            let duration = if self.clip_spacing_set_duration {
                interval.max(0.1)
            } else {
                clip.duration
            };
            if self.clip_spacing_set_duration {
                self.editor
                    .project
                    .resize_clip(clip.id, start_time, duration);
            } else {
                self.editor.project.move_clip(clip.id, start_time);
            }
            previous_anchor = if self.clip_spacing_uses_point_anchor(clip.asset_id) {
                Some(start_time)
            } else {
                Some(start_time + duration)
            };
        }
        self.editor.preview_dirty = true;
        self.editor.status = format!(
            "Spaced {} clips by {}",
            clips.len(),
            format_duration(interval)
        );
    }

    fn clip_spacing_uses_point_anchor(&self, asset_id: Uuid) -> bool {
        self.editor
            .project
            .find_asset(asset_id)
            .is_some_and(|asset| asset.is_image())
    }

    fn provider_choices_for_kind(&self, kind: ProviderWorkflowKind) -> Vec<ProviderEntry> {
        self.editor
            .provider_entries
            .iter()
            .filter(|provider| provider.resolved_workflow_kind() == kind)
            .cloned()
            .collect()
    }

    fn provider_choice_menu(
        &self,
        ui: &mut Ui,
        kind: ProviderWorkflowKind,
        configure_later_label: &str,
    ) -> Option<Option<Uuid>> {
        let providers = self.provider_choices_for_kind(kind);
        if automation_button(ui.button(configure_later_label), configure_later_label).clicked() {
            return Some(None);
        }
        if providers.is_empty() {
            ui.separator();
            ui.label(kit::caption(format!(
                "No {} providers configured.",
                kind.label()
            )));
            return None;
        }
        ui.separator();
        for provider in providers {
            if automation_button(ui.button(&provider.name), &provider.name).clicked() {
                return Some(Some(provider.id));
            }
        }
        None
    }

    fn request_bridge_video_from_selected_clips(
        &mut self,
        clips: &[Clip],
        provider_id: Option<Uuid>,
    ) {
        let reference_clips = self.bridge_reference_clips(clips);
        let (Some(first), Some(last)) = (reference_clips.first(), reference_clips.last()) else {
            self.editor.status =
                "Select at least two image or video reference clips first.".to_string();
            return;
        };
        if first.clip.id == last.clip.id {
            self.editor.status = "Select two different reference clips.".to_string();
            return;
        }

        let convert_clip_ids: Vec<Uuid> = [first, last]
            .iter()
            .filter(|reference| {
                self.editor
                    .project
                    .find_asset(reference.clip.asset_id)
                    .is_some_and(|asset| {
                        asset.is_image() && !clip_is_keyframe_image(&reference.clip, Some(asset))
                    })
            })
            .map(|reference| reference.clip.id)
            .collect();
        if convert_clip_ids.is_empty() {
            self.create_bridge_video_from_selected_clips(clips, provider_id);
            return;
        }

        let sample_names = [first, last]
            .iter()
            .filter_map(|reference| {
                self.editor
                    .project
                    .find_asset(reference.clip.asset_id)
                    .map(|asset| {
                        if let Some(frame) = reference.frame_reference {
                            format!("{} ({})", asset.name, frame.label())
                        } else {
                            asset.name.clone()
                        }
                    })
            })
            .collect();
        self.bridge_keyframe_confirmation = Some(BridgeKeyframeConfirmation {
            clip_ids: reference_clips
                .iter()
                .map(|reference| reference.clip.id)
                .collect(),
            convert_clip_ids,
            sample_names,
            provider_id,
        });
    }

    fn bridge_reference_clips(&self, clips: &[Clip]) -> Vec<BridgeReferenceClip> {
        let mut visual_clips: Vec<Clip> = clips
            .iter()
            .filter(|clip| {
                self.editor
                    .project
                    .find_asset(clip.asset_id)
                    .is_some_and(|asset| asset.is_image() || asset.is_video())
            })
            .cloned()
            .collect();
        visual_clips.sort_by(|a, b| {
            a.start_time
                .partial_cmp(&b.start_time)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.id.cmp(&b.id))
        });
        if visual_clips.len() < 2 {
            return Vec::new();
        }

        let first_clip = visual_clips.first().cloned();
        let last_clip = visual_clips.last().cloned();
        let (Some(first_clip), Some(last_clip)) = (first_clip, last_clip) else {
            return Vec::new();
        };
        let Some(first_asset) = self.editor.project.find_asset(first_clip.asset_id) else {
            return Vec::new();
        };
        let Some(last_asset) = self.editor.project.find_asset(last_clip.asset_id) else {
            return Vec::new();
        };

        let mut references = vec![
            bridge_reference_for_clip(first_clip, first_asset, true),
            bridge_reference_for_clip(last_clip, last_asset, false),
        ];
        references.sort_by(|a, b| {
            a.anchor_time
                .partial_cmp(&b.anchor_time)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.clip.id.cmp(&b.clip.id))
        });
        references
    }

    fn create_bridge_video_from_clip_ids(&mut self, clip_ids: &[Uuid], provider_id: Option<Uuid>) {
        let clips: Vec<Clip> = self
            .editor
            .project
            .clips
            .iter()
            .filter(|clip| clip_ids.contains(&clip.id))
            .cloned()
            .collect();
        self.create_bridge_video_from_selected_clips(&clips, provider_id);
    }

    fn create_i2v_from_single_clip(
        &mut self,
        clip_id: Uuid,
        reference: SingleI2VReference,
        provider_id: Option<Uuid>,
    ) {
        let Some(source_clip) = self
            .editor
            .project
            .clips
            .iter()
            .find(|clip| clip.id == clip_id)
            .cloned()
        else {
            self.editor.status = "Selected clip was not found.".to_string();
            return;
        };
        let Some(source_asset) = self
            .editor
            .project
            .find_asset(source_clip.asset_id)
            .cloned()
        else {
            self.editor.status = "Selected clip source asset was not found.".to_string();
            return;
        };

        let (start_time, frame_reference, status_reference) = match reference {
            SingleI2VReference::Image if source_asset.is_image() => {
                (source_clip.start_time, None, "image")
            }
            SingleI2VReference::VideoFirstFrame if source_asset.is_video() => (
                source_clip.start_time,
                Some(SourceFrameReference::First),
                "video first frame",
            ),
            SingleI2VReference::VideoLastFrame if source_asset.is_video() => (
                source_clip.end_time(),
                Some(SourceFrameReference::Last),
                "video last frame",
            ),
            _ => {
                self.editor.status =
                    "Selected clip is not compatible with that I2V action.".to_string();
                return;
            }
        };

        let fps = default_generative_video_fps();
        let frame_count = default_generative_video_frames();
        let duration = frame_count as f64 / fps.max(1.0);
        let target_track_id = self.bridge_target_track_id(source_clip.track_id);
        let asset_id = match self.editor.create_generative_video(fps, frame_count) {
            Ok(asset_id) => asset_id,
            Err(err) => {
                self.editor.status = err;
                return;
            }
        };

        self.editor
            .project
            .update_generative_config(asset_id, |config| {
                config.provider_id = provider_id;
                config.reference_slots.insert(
                    "start_image".to_string(),
                    InputValue::AssetRef {
                        asset_id: source_clip.asset_id,
                        source_clip_id: Some(source_clip.id),
                        pinned: true,
                        frame_reference,
                    },
                );
            });
        let config_save_error = self.editor.project.save_generative_config(asset_id).err();

        let mut clip = Clip::new(asset_id, target_track_id, start_time, duration);
        clip.label = Some("I2V".to_string());
        let new_clip_id = self.editor.project.add_clip(clip);
        self.editor.selection.select_clip(new_clip_id);
        self.editor.preview_dirty = true;
        self.editor.status = if let Some(err) = config_save_error {
            format!("I2V clip created from {status_reference}, but config save failed: {err}")
        } else {
            format!("Created I2V clip from {status_reference}.")
        };
    }

    fn create_i2i_from_single_clip(&mut self, clip_id: Uuid, provider_id: Option<Uuid>) {
        let Some(source_clip) = self
            .editor
            .project
            .clips
            .iter()
            .find(|clip| clip.id == clip_id)
            .cloned()
        else {
            self.editor.status = "Selected clip was not found.".to_string();
            return;
        };
        let Some(source_asset) = self
            .editor
            .project
            .find_asset(source_clip.asset_id)
            .cloned()
        else {
            self.editor.status = "Selected clip source asset was not found.".to_string();
            return;
        };
        if !source_asset.is_image() {
            self.editor.status = "Select an image clip for I2I generation.".to_string();
            return;
        }

        let target_track_id = self.bridge_target_track_id(source_clip.track_id);
        let asset_id = match self.editor.create_generative_image() {
            Ok(asset_id) => asset_id,
            Err(err) => {
                self.editor.status = err;
                return;
            }
        };

        self.editor
            .project
            .update_generative_config(asset_id, |config| {
                config.provider_id = provider_id;
                config.reference_slots.insert(
                    "image".to_string(),
                    InputValue::AssetRef {
                        asset_id: source_clip.asset_id,
                        source_clip_id: Some(source_clip.id),
                        pinned: true,
                        frame_reference: None,
                    },
                );
            });
        let config_save_error = self.editor.project.save_generative_config(asset_id).err();

        let mut clip = Clip::new(
            asset_id,
            target_track_id,
            source_clip.start_time,
            source_clip.duration,
        );
        clip.label = Some("I2I".to_string());
        let new_clip_id = self.editor.project.add_clip(clip);
        self.editor.selection.select_clip(new_clip_id);
        self.editor.preview_dirty = true;
        self.editor.status = if let Some(err) = config_save_error {
            format!("I2I clip created, but config save failed: {err}")
        } else {
            "Created I2I clip from image.".to_string()
        };
    }

    fn create_generative_image_clip_on_track(
        &mut self,
        track_id: Uuid,
        start_time: f64,
        provider_id: Option<Uuid>,
    ) {
        let Some(track) = self
            .editor
            .project
            .tracks
            .iter()
            .find(|track| track.id == track_id)
            .cloned()
        else {
            self.editor.status = "Timeline track was not found.".to_string();
            return;
        };
        if track.track_type != TrackType::Video {
            self.editor.status =
                "Generative images can only be placed on video tracks.".to_string();
            return;
        }

        let asset_id = match self.editor.create_generative_image() {
            Ok(asset_id) => asset_id,
            Err(err) => {
                self.editor.status = err;
                return;
            }
        };
        if provider_id.is_some() {
            self.editor
                .project
                .set_generative_provider_id(asset_id, provider_id);
            if let Err(err) = self.editor.project.save_generative_config(asset_id) {
                self.editor.status =
                    format!("Created generative image, but config save failed: {err}");
            }
        }

        match self
            .editor
            .add_asset_to_timeline_track(asset_id, track_id, Some(start_time))
        {
            Ok(_) => {
                self.editor.status = format!("Created generative image clip on {}", track.name);
            }
            Err(err) => {
                self.editor.status = err;
            }
        }
    }

    fn create_bridge_video_from_selected_clips(
        &mut self,
        clips: &[Clip],
        provider_id: Option<Uuid>,
    ) {
        let reference_clips = self.bridge_reference_clips(clips);
        let (Some(first), Some(last)) = (reference_clips.first(), reference_clips.last()) else {
            self.editor.status =
                "Select at least two image or video reference clips first.".to_string();
            return;
        };
        if first.clip.id == last.clip.id {
            self.editor.status = "Select two different reference clips.".to_string();
            return;
        }

        let fallback_duration =
            default_generative_video_frames() as f64 / default_generative_video_fps();
        let duration = (last.anchor_time - first.anchor_time)
            .abs()
            .max(fallback_duration);
        let fps = default_generative_video_fps();
        let frame_count = frames_from_seconds(duration, fps).round().max(1.0) as u32;
        let start_time = first.anchor_time.min(last.anchor_time);
        let target_track_id = self.bridge_target_track_id(first.clip.track_id);

        let asset_id = match self.editor.create_generative_video(fps, frame_count) {
            Ok(asset_id) => asset_id,
            Err(err) => {
                self.editor.status = err;
                return;
            }
        };

        self.editor
            .project
            .update_generative_config(asset_id, |config| {
                config.provider_id = provider_id;
                config.reference_slots.insert(
                    "start_image".to_string(),
                    InputValue::AssetRef {
                        asset_id: first.clip.asset_id,
                        source_clip_id: Some(first.clip.id),
                        pinned: true,
                        frame_reference: first.frame_reference,
                    },
                );
                config.reference_slots.insert(
                    "end_image".to_string(),
                    InputValue::AssetRef {
                        asset_id: last.clip.asset_id,
                        source_clip_id: Some(last.clip.id),
                        pinned: true,
                        frame_reference: last.frame_reference,
                    },
                );
            });
        if let Err(err) = self.editor.project.save_generative_config(asset_id) {
            self.editor.status = format!("Bridge created, but config save failed: {err}");
        }

        let mut clip = Clip::new(asset_id, target_track_id, start_time, duration);
        clip.label = Some("I2V bridge".to_string());
        let clip_id = self.editor.project.add_clip(clip);
        self.editor.selection.select_clip(clip_id);
        self.editor.preview_dirty = true;
        self.editor.status =
            "Created generative video bridge from selected references.".to_string();
    }

    fn bridge_target_track_id(&mut self, source_track_id: Uuid) -> Uuid {
        let source_index = self
            .editor
            .project
            .tracks
            .iter()
            .position(|track| track.id == source_track_id)
            .unwrap_or(0);
        if let Some(track) = self
            .editor
            .project
            .tracks
            .iter()
            .take(source_index)
            .rev()
            .find(|track| track.track_type == TrackType::Video)
        {
            return track.id;
        }

        let track_id = self.editor.project.add_video_track();
        while self
            .editor
            .project
            .tracks
            .iter()
            .position(|track| track.id == track_id)
            .is_some_and(|index| index > source_index)
        {
            if !self.editor.project.move_track_up(track_id) {
                break;
            }
        }
        track_id
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
        let clip_asset_is_image = clip_asset_id
            .and_then(|asset_id| self.editor.project.find_asset(asset_id))
            .is_some_and(|asset| asset.is_image());
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
                if clip_asset_is_image {
                    ui.add_space(kit::FORM_ROW_GAP);
                    let mut next_mode = clip.image_mode;
                    kit::labeled_combo_field(
                        ui,
                        "Timeline Display",
                        ("clip_image_mode", clip.id),
                        clip_image_mode_label(next_mode),
                        |ui| {
                            automation_selectable_value(
                                ui,
                                &mut next_mode,
                                ClipImageMode::Still,
                                "Still Image",
                            );
                            automation_selectable_value(
                                ui,
                                &mut next_mode,
                                ClipImageMode::Keyframe,
                                "Keyframe Reference",
                            );
                        },
                    );
                    if next_mode != clip.image_mode {
                        clip.image_mode = next_mode;
                    }
                }
            });
            ui.add_space(kit::FORM_ROW_GAP);
            inspector_card(ui, "Transform", |ui| {
                transform_editor(ui, &mut clip.transform, &mut preview_dirty);
            });
            ui.add_space(kit::FORM_ROW_GAP);
            inspector_card(ui, "Timing", |ui| {
                if clip_asset_is_image && clip.image_mode == ClipImageMode::Keyframe {
                    preview_dirty |= inspector_drag_f64(
                        ui,
                        "Time",
                        &mut clip.start_time,
                        0.05,
                        ui.available_width(),
                    );
                } else {
                    preview_dirty |= inspector_two_drag_f64(
                        ui,
                        ("Start", &mut clip.start_time, 0.05),
                        ("Duration", &mut clip.duration, 0.05),
                    );
                }
            });
        }
        if let Some(asset_id) = clip_asset_id {
            if generative_output_for_asset(&self.editor.project, asset_id).is_some() {
                ui.add_space(kit::FORM_ROW_GAP);
                self.generative_asset_attributes(ui, asset_id, Some(clip_id));
            }
        }
        if preview_dirty {
            self.editor.preview_dirty = true;
        }
    }

    fn generative_asset_attributes(
        &mut self,
        ui: &mut Ui,
        asset_id: Uuid,
        context_clip_id: Option<Uuid>,
    ) {
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
        let mut open_asset_lab = false;
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
                                        automation_selectable_value(
                                            ui,
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
                            open_asset_lab = true;
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
                    automation_selectable_value(ui, &mut next_provider_id, None, "None selected");
                    for provider in compatible_providers.iter() {
                        automation_selectable_value(
                            ui,
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
                        automation_selectable_value(
                            ui,
                            &mut next_seed_strategy,
                            SeedStrategy::Increment,
                            "Increment",
                        );
                        automation_selectable_value(
                            ui,
                            &mut next_seed_strategy,
                            SeedStrategy::Random,
                            "Random",
                        );
                        automation_selectable_value(
                            ui,
                            &mut next_seed_strategy,
                            SeedStrategy::Keep,
                            "Keep",
                        );
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
                        automation_selectable_value(
                            ui,
                            &mut next_seed_field,
                            String::new(),
                            "Auto-detect",
                        );
                        for (name, label) in seed_field_options.iter() {
                            automation_selectable_value(
                                ui,
                                &mut next_seed_field,
                                name.clone(),
                                label,
                            );
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
        let input_updates = self.provider_inputs_card(
            ui,
            asset_id,
            context_clip_id,
            selected_provider.clone(),
            &config_snapshot,
        );

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
                        config.inputs.insert(name, value);
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
            self.invalidate_generative_asset_runtime(asset_id);
        }

        if open_asset_lab {
            let local_time = context_clip_id.and_then(|clip_id| {
                self.editor.project.clips.iter().find_map(|clip| {
                    (clip.id == clip_id).then(|| {
                        (self.editor.current_time - clip.start_time + clip.trim_in_seconds).max(0.0)
                    })
                })
            });
            self.open_asset_lab_at_time(asset_id, local_time);
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
                context_clip_id,
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
        context_clip_id: Option<Uuid>,
        selected_provider: Option<ProviderEntry>,
        config_snapshot: &GenerativeConfig,
    ) -> Vec<(String, InputValue)> {
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
                            updates.push((
                                input.name.clone(),
                                InputValue::Literal {
                                    value: serde_json::Value::String(value),
                                },
                            ));
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
                                updates.push((
                                    input.name.clone(),
                                    InputValue::Literal {
                                        value: serde_json::Value::Number(number),
                                    },
                                ));
                            }
                        }
                    }
                    ProviderInputType::Integer => {
                        let mut value = current_value
                            .as_ref()
                            .and_then(input_value_as_i64)
                            .unwrap_or(0);
                        let step = input.ui.as_ref().and_then(|ui| ui.step).unwrap_or(1.0);
                        let width = ui.available_width();
                        if inspector_drag_i64(ui, &label, &mut value, step, width) {
                            updates.push((
                                input.name.clone(),
                                InputValue::Literal {
                                    value: serde_json::Value::Number(value.into()),
                                },
                            ));
                        }
                    }
                    ProviderInputType::Boolean => {
                        let mut value = current_value
                            .as_ref()
                            .and_then(input_value_as_bool)
                            .unwrap_or(false);
                        if inspector_bool_field(ui, &label, &mut value) {
                            updates.push((
                                input.name.clone(),
                                InputValue::Literal {
                                    value: serde_json::Value::Bool(value),
                                },
                            ));
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
                                    automation_selectable_value(
                                        ui,
                                        &mut value,
                                        option.clone(),
                                        option,
                                    );
                                }
                            },
                        );
                        if value != before {
                            updates.push((
                                input.name.clone(),
                                InputValue::Literal {
                                    value: serde_json::Value::String(value),
                                },
                            ));
                        }
                    }
                    ProviderInputType::Image
                    | ProviderInputType::Video
                    | ProviderInputType::Audio => {
                        if let Some(update) =
                            self.provider_asset_input_field(ui, asset_id, context_clip_id, input)
                        {
                            updates.push((input.name.clone(), update));
                        }
                    }
                }
            }
        });
        updates
    }

    fn provider_asset_input_field(
        &self,
        ui: &mut Ui,
        asset_id: Uuid,
        context_clip_id: Option<Uuid>,
        input: &ProviderInputField,
    ) -> Option<InputValue> {
        let config = self.editor.project.generative_config(asset_id);
        let field_value = config
            .and_then(|config| config.inputs.get(&input.name))
            .cloned();
        let reference_slot = semantic_reference_slot(input);
        let slot_value = reference_slot.and_then(|slot| {
            config
                .and_then(|config| config.reference_slots.get(slot))
                .cloned()
        });
        let current_binding = field_value.clone().or(slot_value.clone());
        let candidates = self.asset_input_candidates(input, context_clip_id);
        let auto_candidate = candidates
            .iter()
            .find(|candidate| candidate.contextual)
            .cloned();
        let resolved_binding = match current_binding.clone() {
            Some(InputValue::AssetRef { pinned: false, .. }) => auto_candidate
                .as_ref()
                .map(|candidate| InputValue::AssetRef {
                    asset_id: candidate.asset_id,
                    source_clip_id: candidate.source_clip_id,
                    pinned: false,
                    frame_reference: candidate.frame_reference,
                })
                .or(current_binding.clone()),
            Some(value) => Some(value),
            None => auto_candidate
                .as_ref()
                .map(|candidate| InputValue::AssetRef {
                    asset_id: candidate.asset_id,
                    source_clip_id: candidate.source_clip_id,
                    pinned: false,
                    frame_reference: candidate.frame_reference,
                }),
        };

        let current_label = resolved_binding
            .as_ref()
            .and_then(|value| self.asset_input_label(value, context_clip_id))
            .unwrap_or_else(|| {
                if context_clip_id.is_some() {
                    "Auto: no match".to_string()
                } else {
                    "None selected".to_string()
                }
            });
        let combo_id = ("provider_asset_input", asset_id, &input.name);
        let before = current_binding.clone();
        let mut next = current_binding.clone();

        kit::labeled_combo_field(ui, &input.label, combo_id, current_label, |ui| {
            if let Some(candidate) = auto_candidate.as_ref() {
                let label = format!("Auto: {}  {}", candidate.label, candidate.detail);
                if ui
                    .selectable_label(
                        matches!(next, Some(InputValue::AssetRef { pinned: false, .. })),
                        label,
                    )
                    .clicked()
                {
                    next = Some(InputValue::AssetRef {
                        asset_id: candidate.asset_id,
                        source_clip_id: candidate.source_clip_id,
                        pinned: false,
                        frame_reference: candidate.frame_reference,
                    });
                    ui.close();
                }
                ui.separator();
            } else if context_clip_id.is_some() {
                ui.label(kit::caption("No timeline match"));
                ui.separator();
            }

            let mut drew_context_header = false;
            let mut drew_other_header = false;
            for candidate in candidates.iter() {
                if candidate.contextual && !drew_context_header {
                    ui.label(kit::caption("Timeline context"));
                    drew_context_header = true;
                } else if !candidate.contextual && !drew_other_header {
                    if drew_context_header {
                        ui.separator();
                    }
                    ui.label(kit::caption("Other project assets"));
                    drew_other_header = true;
                }
                let selected = binding_matches_candidate(next.as_ref(), candidate, true);
                if ui
                    .selectable_label(
                        selected,
                        format!("{}  {}", candidate.label, candidate.detail),
                    )
                    .clicked()
                {
                    next = Some(InputValue::AssetRef {
                        asset_id: candidate.asset_id,
                        source_clip_id: candidate.source_clip_id,
                        pinned: true,
                        frame_reference: candidate.frame_reference,
                    });
                    ui.close();
                }
            }
        });

        if let Some(InputValue::AssetRef {
            asset_id,
            source_clip_id,
            pinned,
            frame_reference,
        }) = resolved_binding.as_ref()
        {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let hint = if *pinned {
                    if source_clip_id.is_some() {
                        "Pinned to timeline clip"
                    } else {
                        "Pinned to asset"
                    }
                } else if context_clip_id.is_some() {
                    "Auto from timeline proximity"
                } else {
                    "No timeline context; using saved asset"
                };
                ui.label(kit::caption(hint));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let action = if *pinned { "Unpin" } else { "Pin" };
                    if ui.small_button(action).clicked() {
                        next = Some(InputValue::AssetRef {
                            asset_id: *asset_id,
                            source_clip_id: *source_clip_id,
                            pinned: !*pinned,
                            frame_reference: *frame_reference,
                        });
                    }
                });
            });
        }

        if next != before {
            next
        } else {
            None
        }
    }

    fn asset_input_label(
        &self,
        value: &InputValue,
        context_clip_id: Option<Uuid>,
    ) -> Option<String> {
        let InputValue::AssetRef {
            asset_id,
            source_clip_id,
            pinned,
            frame_reference,
        } = value
        else {
            return None;
        };
        let asset = self.editor.project.find_asset(*asset_id)?;
        let prefix = if *pinned {
            "Pinned"
        } else if context_clip_id.is_some() {
            "Auto"
        } else {
            "Saved"
        };
        let clip_suffix = source_clip_id
            .and_then(|clip_id| {
                self.editor
                    .project
                    .clips
                    .iter()
                    .find(|clip| clip.id == clip_id)
            })
            .map(|clip| {
                let anchor = if *frame_reference == Some(SourceFrameReference::Last) {
                    clip.end_time()
                } else {
                    clip.start_time
                };
                format!(" @ {}", timecode(anchor))
            })
            .unwrap_or_default();
        let frame_suffix = frame_reference
            .map(|frame| format!(" ({})", frame.label()))
            .unwrap_or_default();
        Some(format!(
            "{prefix}: {}{}{}",
            asset_display_name(asset),
            clip_suffix,
            frame_suffix
        ))
    }

    fn asset_input_candidates(
        &self,
        input: &ProviderInputField,
        context_clip_id: Option<Uuid>,
    ) -> Vec<AssetInputCandidate> {
        let mut candidates = Vec::new();
        let context_clip = context_clip_id
            .and_then(|id| self.editor.project.clips.iter().find(|clip| clip.id == id));
        let slot = semantic_reference_slot(input).unwrap_or("asset");
        let target_time = context_clip.map(|clip| {
            if slot.starts_with("end") {
                clip.end_time()
            } else {
                clip.start_time
            }
        });
        let context_track_index = context_clip.and_then(|clip| {
            self.editor
                .project
                .tracks
                .iter()
                .position(|track| track.id == clip.track_id)
        });

        for clip in self.editor.project.clips.iter() {
            if Some(clip.id) == context_clip_id {
                continue;
            }
            let Some(asset) = self.editor.project.find_asset(clip.asset_id) else {
                continue;
            };
            let Some((time_distance, detail, frame_reference)) =
                self.asset_input_clip_candidate(input, slot, target_time, clip, asset)
            else {
                continue;
            };
            let track_score = match (
                context_track_index,
                self.editor
                    .project
                    .tracks
                    .iter()
                    .position(|track| track.id == clip.track_id),
            ) {
                (Some(context_index), Some(index)) if index == context_index + 1 => 0.0,
                (Some(context_index), Some(index)) if index == context_index => 0.05,
                (Some(context_index), Some(index)) if index > context_index => {
                    0.1 + (index - context_index - 1) as f64 * 0.15
                }
                (Some(context_index), Some(index)) => (context_index - index) as f64 * 0.5,
                _ => 0.0,
            };
            candidates.push(AssetInputCandidate {
                asset_id: asset.id,
                source_clip_id: Some(clip.id),
                frame_reference,
                label: asset_display_name(asset),
                detail,
                contextual: context_clip_id.is_some(),
                score: time_distance + track_score,
            });
        }

        let mut seen_assets: HashSet<Uuid> = candidates
            .iter()
            .map(|candidate| candidate.asset_id)
            .collect();
        for asset in self.editor.project.assets.iter() {
            if seen_assets.contains(&asset.id)
                || !compatible_asset_for_provider_input(asset, &input.input_type)
            {
                continue;
            }
            seen_assets.insert(asset.id);
            candidates.push(AssetInputCandidate {
                asset_id: asset.id,
                source_clip_id: None,
                frame_reference: None,
                label: asset_display_name(asset),
                detail: asset_kind_label(&asset.kind).to_string(),
                contextual: false,
                score: f64::MAX / 2.0,
            });
        }

        candidates.sort_by(|a, b| {
            b.contextual
                .cmp(&a.contextual)
                .then_with(|| {
                    a.score
                        .partial_cmp(&b.score)
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .then_with(|| a.label.cmp(&b.label))
        });
        candidates
    }

    fn asset_input_clip_candidate(
        &self,
        input: &ProviderInputField,
        slot: &str,
        target_time: Option<f64>,
        clip: &Clip,
        asset: &Asset,
    ) -> Option<(f64, String, Option<SourceFrameReference>)> {
        match input.input_type {
            ProviderInputType::Image => {
                if asset.is_image() {
                    let anchor = clip.start_time;
                    let distance = target_time
                        .map(|target| (anchor - target).abs())
                        .unwrap_or(0.0);
                    return Some((distance, timecode(anchor), None));
                }
                if asset.is_video() {
                    let target = target_time?;
                    let start_distance = (clip.start_time - target).abs();
                    let end_distance = (clip.end_time() - target).abs();
                    let prefer_last_for_start =
                        slot.starts_with("start") && end_distance <= start_distance;
                    let prefer_first_for_end =
                        slot.starts_with("end") && start_distance <= end_distance;
                    let frame = if prefer_last_for_start {
                        SourceFrameReference::Last
                    } else if prefer_first_for_end || start_distance <= end_distance {
                        SourceFrameReference::First
                    } else {
                        SourceFrameReference::Last
                    };
                    let (anchor, distance) = match frame {
                        SourceFrameReference::First => (clip.start_time, start_distance),
                        SourceFrameReference::Last => (clip.end_time(), end_distance),
                    };
                    let slot_hint = if slot.starts_with("end") {
                        "end ref"
                    } else if slot.starts_with("start") {
                        "start ref"
                    } else {
                        "image ref"
                    };
                    return Some((
                        distance,
                        format!("{} {} @ {}", slot_hint, frame.label(), timecode(anchor)),
                        Some(frame),
                    ));
                }
                None
            }
            ProviderInputType::Video | ProviderInputType::Audio => {
                if !compatible_asset_for_provider_input(asset, &input.input_type) {
                    return None;
                }
                let anchor = clip.start_time;
                let distance = target_time
                    .map(|target| (anchor - target).abs())
                    .unwrap_or(0.0);
                Some((distance, timecode(anchor), None))
            }
            _ => None,
        }
    }

    fn asset_attributes(&mut self, ui: &mut Ui, asset_id: Uuid) {
        let mut add_to_timeline = false;
        let mut duplicate_asset = false;
        let mut extract_generation = false;
        let mut open_asset_lab = false;
        let mut rename_to: Option<String> = None;
        let Some(asset_snapshot) = self
            .editor
            .project
            .assets
            .iter()
            .find(|asset| asset.id == asset_id)
            .cloned()
        else {
            return;
        };
        let thumbnail = self.asset_thumbnail(ui.ctx(), &asset_snapshot);
        let kind_label = asset_kind_label(&asset_snapshot.kind).to_string();
        let duration = asset_snapshot.duration_seconds;
        let source = asset_source_label(&asset_snapshot);
        let active_version = asset_snapshot.active_version().map(str::to_string);
        let is_generative = asset_snapshot.is_generative();
        inspector_card(ui, "Asset", |ui| {
            let accent = asset_accent(&asset_snapshot);
            ui.horizontal(|ui| {
                let (thumb_rect, _) =
                    ui.allocate_exact_size(INSPECTOR_THUMBNAIL_SIZE, Sense::hover());
                paint_asset_thumbnail(ui, thumb_rect, &asset_snapshot, accent, thumbnail);
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
            let mut name = asset_snapshot.name.clone();
            if kit::singleline_text_field(ui, &mut name, ui.available_width()).changed() {
                rename_to = Some(name);
            }
            ui.add_space(kit::FORM_ROW_GAP);
            if let Some(active_version) = active_version {
                inspector_meta_row(ui, "Version", active_version);
            }
            if let Some(source) = source {
                inspector_meta_row(ui, "Source", source);
            }
            ui.add_space(kit::ACTION_GAP);
            kit::equal_width_action_row(
                ui,
                2,
                kit::SECONDARY_BUTTON_H,
                kit::FIELD_COMPOUND_GAP,
                |ui, index, width| match index {
                    0 => {
                        if kit::secondary_button(ui, "Add to timeline", width).clicked() {
                            add_to_timeline = true;
                        }
                    }
                    _ => {
                        if kit::secondary_button(ui, "Duplicate", width).clicked() {
                            duplicate_asset = true;
                        }
                    }
                },
            );
            if is_generative {
                ui.add_space(kit::FORM_ROW_GAP);
                kit::equal_width_action_row(
                    ui,
                    2,
                    kit::SECONDARY_BUTTON_H,
                    kit::FIELD_COMPOUND_GAP,
                    |ui, index, width| match index {
                        0 => {
                            if kit::secondary_button(ui, "Extract", width).clicked() {
                                extract_generation = true;
                            }
                        }
                        _ => {
                            if kit::secondary_button(ui, "Asset Lab", width).clicked() {
                                open_asset_lab = true;
                            }
                        }
                    },
                );
            }
        });
        if let Some(name) = rename_to {
            if let Err(err) = self.editor.rename_asset(asset_id, name) {
                self.editor.status = err;
            }
        }
        if add_to_timeline {
            if let Err(err) = self.editor.add_asset_to_timeline(asset_id, None) {
                self.editor.status = err;
            }
        }
        if duplicate_asset {
            self.duplicate_assets(&[asset_id]);
        }
        if extract_generation {
            self.extract_active_generation(asset_id);
        }
        if open_asset_lab {
            self.open_asset_lab(asset_id);
        }
        if is_generative {
            ui.add_space(kit::FORM_ROW_GAP);
            self.generative_asset_attributes(ui, asset_id, None);
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
        let mut track_mute_changed = false;
        let mut preview_dirty = false;
        let mut delete_track = false;
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
                    let before = track.muted;
                    automation_checkbox(ui, &mut track.muted, "Muted");
                    if track.muted != before {
                        track_mute_changed = true;
                        preview_dirty = true;
                    }
                    ui.add_space(kit::FORM_ROW_GAP);
                    let volume_w = ui.available_width();
                    let _ = inspector_drag_f32(ui, "Volume", &mut track.volume, 0.01, volume_w);
                }
                ui.add_space(kit::ACTION_GAP);
                let delete_w = ui.available_width();
                if kit::danger_button(ui, "Delete Track", delete_w).clicked() {
                    delete_track = true;
                }
            });
        }
        if delete_track {
            self.request_delete_tracks(&[track_id]);
        }
        if preview_dirty {
            self.editor.preview_dirty = true;
        }
        if track_mute_changed {
            self.refresh_audio_playback_items();
        }
    }

    #[allow(clippy::too_many_arguments)]
    #[allow(clippy::too_many_arguments)]
    fn timeline_panel(&mut self, root: &mut Ui) {
        if self.editor.layout.timeline_collapsed {
            let response = egui::Panel::bottom(self.project_panel_id("timeline_collapsed"))
                .exact_size(TIMELINE_HEADER_H + 12.0)
                .frame(kit::timeline_frame())
                .show_inside(root, |ui| {
                    self.timeline_header(ui, true);
                });
            kit::paint_panel_edge(root, response.response.rect, kit::PanelEdge::Top);
            return;
        }

        let response = egui::Panel::bottom(self.project_panel_id("timeline"))
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
                    self.set_timeline_zoom_to_next_coarse(-1, duration, viewport_w);
                }
                ui.label(kit::caption(&zoom_label));
                if kit::timeline_tool_icon_button(ui, "+").clicked() {
                    self.set_timeline_zoom_to_next_coarse(1, duration, viewport_w);
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
        let min_h = TIMELINE_RULER_H + TIMELINE_TRACK_H + TIMELINE_ADD_ROW_H;
        let total_h = available.y.max(min_h);
        let (outer, response) =
            ui.allocate_exact_size(Vec2::new(available.x, total_h), Sense::click_and_drag());
        let track_content_h = track_count * TIMELINE_TRACK_H;
        let max_scroll_y =
            (track_content_h - (total_h - TIMELINE_RULER_H - TIMELINE_ADD_ROW_H).max(1.0)).max(0.0);
        self.clamp_timeline_vertical_scroll(max_scroll_y);
        let rects = timeline_rects(outer, self.editor.layout.timeline_scroll_y);
        let viewport_w = rects.tracks.width().max(1.0);
        let (fit_zoom, max_zoom) = timeline_zoom_bounds(duration as f32, viewport_w, fps);
        self.editor.layout.timeline_zoom =
            self.editor.layout.timeline_zoom.clamp(fit_zoom, max_zoom);
        self.handle_timeline_keyboard(ui, duration, viewport_w);
        let zoom = self
            .editor
            .layout
            .timeline_zoom
            .clamp(fit_zoom, max_zoom)
            .max(TIMELINE_MIN_ZOOM_FLOOR);
        self.editor.layout.timeline_zoom = zoom;
        let content_w = (duration as f32 * zoom).max(viewport_w);
        self.clamp_timeline_scroll(content_w, viewport_w);
        let content_viewport =
            Rect::from_min_max(rects.outer.left_top(), rects.tracks.right_bottom());
        self.handle_timeline_wheel(
            ui,
            content_viewport,
            duration,
            content_w,
            viewport_w,
            max_scroll_y,
        );
        let cache_buckets = if self.editor.layout.preview_stats {
            let bucket_hint_seconds = ((6.0 / zoom.max(TIMELINE_MIN_ZOOM_FLOOR)) as f64)
                .max(1.0 / self.editor.project.settings.fps.max(1.0));
            self.editor
                .previewer
                .cached_buckets_for_project(&self.editor.project, bucket_hint_seconds)
        } else {
            HashMap::new()
        };

        let painter = ui.painter_at(outer);
        let overlay_clip = Rect::from_min_max(
            rects.ruler.left_top(),
            Pos2::new(rects.outer.right(), rects.add_row.top()),
        );
        let overlay_painter = painter.with_clip_rect(overlay_clip);
        let ruler_painter = painter.with_clip_rect(rects.ruler);
        let track_painter = painter.with_clip_rect(rects.tracks);
        let label_viewport = Rect::from_min_max(
            Pos2::new(outer.left(), rects.tracks.top()),
            Pos2::new(rects.tracks.left(), rects.tracks.bottom()),
        );
        let label_painter = painter.with_clip_rect(label_viewport);
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
                timeline_track_row_at_pos(pos, rects, &tracks)
                    .filter(|track| {
                        self.editor
                            .project
                            .asset_compatible_with_track(asset_id, track.id)
                    })
                    .map(|track| track.id)
            })
        });
        let clip_drag_target_track_id = match self.timeline_drag {
            Some(TimelineDrag::ClipMove { clip_id, .. }) => {
                ui.ctx().pointer_hover_pos().and_then(|pos| {
                    let clip = clips.iter().find(|clip| clip.id == clip_id)?;
                    timeline_track_row_at_pos(pos, rects, &tracks)
                        .filter(|track| {
                            self.editor
                                .project
                                .asset_compatible_with_track(clip.asset_id, track.id)
                        })
                        .map(|track| track.id)
                })
            }
            _ => None,
        };

        self.paint_timeline_ruler(&ruler_painter, rects.ruler, duration, zoom, fps);

        let mut clip_geoms = Vec::new();
        let mut marker_geoms = Vec::new();
        for (row, track) in tracks.iter().enumerate() {
            let row_rect = timeline_row_rect(rects, row);
            if row_rect.bottom() < rects.tracks.top() || row_rect.top() > rects.tracks.bottom() {
                continue;
            }
            let label_rect = Rect::from_min_max(
                Pos2::new(outer.left(), row_rect.top()),
                Pos2::new(rects.tracks.left(), row_rect.bottom()),
            );
            let label_hit_rect = label_rect.intersect(label_viewport);
            let selected = self.editor.selection.track_ids.contains(&track.id);
            let track_muted = track.muted && track.track_type != TrackType::Marker;
            let track_color = if track_muted {
                track_color(track.track_type).gamma_multiply(0.45)
            } else {
                track_color(track.track_type)
            };
            let track_response = ui.interact(
                label_hit_rect,
                ui.id().with(("timeline-track-label", track.id)),
                Sense::click(),
            );
            if track_response.clicked() {
                self.editor.selection.select_track(track.id);
            }
            track_response.context_menu(|ui| {
                self.editor.selection.select_track(track.id);
                let can_move_up = self
                    .editor
                    .project
                    .tracks
                    .iter()
                    .position(|candidate| candidate.id == track.id)
                    .map(|index| index > 0)
                    .unwrap_or(false);
                let can_move_down = self
                    .editor
                    .project
                    .tracks
                    .iter()
                    .position(|candidate| candidate.id == track.id)
                    .map(|index| index + 1 < self.editor.project.tracks.len())
                    .unwrap_or(false);
                if automation_button(
                    ui.add_enabled(can_move_up, egui::Button::new("Move Up")),
                    "Move Up",
                )
                .clicked()
                {
                    if self.editor.project.move_track_up(track.id) {
                        self.editor.preview_dirty = true;
                        self.editor.status = format!("Moved {} up", track.name);
                    }
                    ui.close();
                }
                if automation_button(
                    ui.add_enabled(can_move_down, egui::Button::new("Move Down")),
                    "Move Down",
                )
                .clicked()
                {
                    if self.editor.project.move_track_down(track.id) {
                        self.editor.preview_dirty = true;
                        self.editor.status = format!("Moved {} down", track.name);
                    }
                    ui.close();
                }
                if track.track_type != TrackType::Marker {
                    ui.separator();
                    let label = if track.muted {
                        "Unmute Track"
                    } else {
                        "Mute Track"
                    };
                    if automation_button(ui.button(label), label).clicked() {
                        if let Some(project_track) = self
                            .editor
                            .project
                            .tracks
                            .iter_mut()
                            .find(|candidate| candidate.id == track.id)
                        {
                            project_track.muted = !project_track.muted;
                            self.editor.preview_dirty = true;
                            self.editor.status = if project_track.muted {
                                format!("Muted {}", project_track.name)
                            } else {
                                format!("Unmuted {}", project_track.name)
                            };
                        }
                        self.refresh_audio_playback_items();
                        ui.close();
                    }
                }
                ui.separator();
                if automation_button(ui.button("Delete Track..."), "Delete Track").clicked() {
                    self.request_delete_tracks(&[track.id]);
                    ui.close();
                }
            });
            label_painter.rect_filled(
                label_rect,
                0.0,
                if selected {
                    Color32::from_rgb(25, 42, 35)
                } else {
                    kit::PANEL
                },
            );
            track_painter.rect_filled(
                row_rect,
                0.0,
                if row % 2 == 0 {
                    Color32::from_rgb(14, 15, 17)
                } else {
                    Color32::from_rgb(11, 12, 14)
                },
            );
            if drop_target_track_id == Some(track.id) || clip_drag_target_track_id == Some(track.id)
            {
                track_painter.rect_filled(row_rect, 0.0, kit::BORDER_FOCUS.gamma_multiply(0.10));
                track_painter.rect_stroke(
                    row_rect.shrink(1.0),
                    3.0,
                    Stroke::new(1.0, kit::BORDER_FOCUS.gamma_multiply(0.85)),
                    egui::StrokeKind::Inside,
                );
            }
            label_painter.line_segment(
                [
                    Pos2::new(outer.left(), row_rect.bottom()),
                    Pos2::new(rects.tracks.left(), row_rect.bottom()),
                ],
                Stroke::new(1.0, kit::BORDER_SOFT),
            );
            track_painter.line_segment(
                [
                    Pos2::new(rects.tracks.left(), row_rect.bottom()),
                    Pos2::new(rects.tracks.right(), row_rect.bottom()),
                ],
                Stroke::new(1.0, kit::BORDER_SOFT),
            );
            label_painter.rect_filled(
                Rect::from_min_size(
                    Pos2::new(label_rect.left() + 12.0, row_rect.center().y - 8.0),
                    Vec2::new(3.0, 16.0),
                ),
                1.0,
                track_color,
            );
            label_painter.text(
                Pos2::new(label_rect.left() + 26.0, row_rect.center().y),
                egui::Align2::LEFT_CENTER,
                &track.name,
                FontId::proportional(12.5),
                if track_muted {
                    kit::TEXT_DIM
                } else {
                    kit::TEXT
                },
            );
            if track_muted {
                label_painter.text(
                    label_rect.right_center() - Vec2::new(14.0, 0.0),
                    egui::Align2::CENTER_CENTER,
                    "M",
                    FontId::monospace(10.0),
                    kit::TEXT_DIM,
                );
            }

            for clip in clips.iter().filter(|clip| clip.track_id == track.id) {
                let asset = assets_by_id.get(&clip.asset_id);
                let keyframe = clip_is_keyframe_image(clip, asset);
                let clip_rect = timeline_clip_rect(
                    clip,
                    asset,
                    row_rect,
                    zoom,
                    self.editor.layout.timeline_scroll_x,
                );
                clip_geoms.push(TimelineClipGeom {
                    clip_id: clip.id,
                    rect: clip_rect,
                    keyframe,
                });
                let selected = self.editor.selection.clip_ids.contains(&clip.id);
                let thumbnail_tiles = asset
                    .filter(|asset| asset.is_visual())
                    .map(|asset| {
                        self.timeline_clip_thumbnail_tiles(ui.ctx(), asset, clip, clip_rect, zoom)
                    })
                    .unwrap_or_default();
                let waveform = asset
                    .filter(|asset| asset.is_audio())
                    .and_then(|asset| self.audio_peak_cache(ui.ctx(), asset));
                let contextual_keyframe_label = keyframe
                    && (selected
                        || ui
                            .ctx()
                            .pointer_hover_pos()
                            .is_some_and(|pos| clip_rect.contains(pos)));
                self.paint_timeline_clip(
                    &track_painter,
                    clip,
                    asset,
                    clip_rect,
                    track_color,
                    selected,
                    &thumbnail_tiles,
                    waveform.as_ref(),
                    cache_buckets.get(&clip.id).map(Vec::as_slice),
                    contextual_keyframe_label,
                );
            }
            if track_muted {
                track_painter.rect_filled(
                    row_rect,
                    0.0,
                    Color32::from_rgba_unmultiplied(3, 4, 6, 96),
                );
            }

            if track.track_type == TrackType::Marker {
                for marker in markers.iter().filter(|marker| {
                    self.editor
                        .project
                        .marker_belongs_to_track(marker, track.id)
                }) {
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
                    self.paint_timeline_marker(&track_painter, marker, row_rect, x);
                }
            }
        }

        self.paint_add_track_row(ui, &painter, rects);
        self.paint_timeline_grid_overlay(&track_painter, rects, duration, zoom);
        self.paint_timeline_playhead(&overlay_painter, rects, duration, zoom);
        if let Some(time) = self.timeline_snap_preview {
            let x = time_to_timeline_x(
                time,
                rects.tracks.left(),
                zoom,
                self.editor.layout.timeline_scroll_x,
            );
            overlay_painter.line_segment(
                [
                    Pos2::new(x, rects.ruler.top()),
                    Pos2::new(x, rects.add_row.top()),
                ],
                Stroke::new(1.0, Color32::from_rgb(229, 187, 47)),
            );
        }
        self.paint_timeline_scrollbar(ui, &painter, rects, content_w, viewport_w);
        self.paint_timeline_vertical_scrollbar(ui, &painter, rects, track_content_h);
        if let Some(payload) = response.dnd_release_payload::<AssetTimelineDragPayload>() {
            if let Some(pos) = ui
                .ctx()
                .pointer_interact_pos()
                .or_else(|| ui.ctx().pointer_hover_pos())
            {
                self.drop_asset_on_timeline(payload.asset_id, pos, rects, &tracks, duration, zoom);
            }
        }
        response.context_menu(|ui| {
            let context_pos = ui
                .ctx()
                .pointer_interact_pos()
                .or_else(|| ui.ctx().pointer_hover_pos());
            let marker_time = context_pos
                .map(|pos| {
                    if pos.x >= rects.tracks.left() {
                        let raw_time =
                            ((pos.x - rects.tracks.left() + self.editor.layout.timeline_scroll_x)
                                / zoom)
                                .clamp(0.0, duration as f32) as f64;
                        snap_time_to_frame(raw_time, self.editor.project.settings.fps.max(1.0))
                    } else {
                        self.editor.current_time
                    }
                })
                .unwrap_or(self.editor.current_time);
            let marker_track_id = context_pos.and_then(|pos| {
                timeline_track_row_at_pos(pos, rects, &tracks)
                    .filter(|track| track.track_type == TrackType::Marker)
                    .map(|track| track.id)
            });
            let context_track = context_pos
                .and_then(|pos| timeline_track_row_at_pos(pos, rects, &tracks))
                .cloned();

            if let Some(pos) = context_pos {
                match timeline_hit(pos, rects, &tracks, &clip_geoms, &marker_geoms) {
                    TimelineHit::ClipBody(id)
                    | TimelineHit::ClipLeftEdge(id)
                    | TimelineHit::ClipRightEdge(id) => {
                        if !self.editor.selection.clip_ids.contains(&id) {
                            self.editor.selection.select_clip(id);
                        }
                    }
                    _ => {}
                }
            }

            if automation_button(ui.button("Add Marker Here"), "Add Marker Here").clicked() {
                self.editor
                    .add_marker_to_track(Some(marker_time), marker_track_id);
                ui.close();
                return;
            }

            if let Some(track) = context_track
                .as_ref()
                .filter(|track| track.track_type == TrackType::Video)
            {
                ui.separator();
                ui.menu_button("New Generative Image", |ui| {
                    if let Some(provider_id) = self.provider_choice_menu(
                        ui,
                        ProviderWorkflowKind::TextToImage,
                        "Configure provider later",
                    ) {
                        self.create_generative_image_clip_on_track(
                            track.id,
                            marker_time,
                            provider_id,
                        );
                        ui.close();
                    }
                });
            }

            let mut selected_clips: Vec<Clip> = self
                .editor
                .project
                .clips
                .iter()
                .filter(|clip| self.editor.selection.clip_ids.contains(&clip.id))
                .cloned()
                .collect();
            selected_clips.sort_by(|a, b| {
                a.start_time
                    .partial_cmp(&b.start_time)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| a.id.cmp(&b.id))
            });
            let visual_count = selected_clips
                .iter()
                .filter(|clip| {
                    self.editor
                        .project
                        .find_asset(clip.asset_id)
                        .is_some_and(|asset| asset.is_image() || asset.is_video())
                })
                .count();
            if selected_clips.len() >= 2 {
                ui.separator();
                if automation_button(ui.button("Space Selected Clips"), "Space Selected Clips")
                    .clicked()
                {
                    self.space_selected_clips(&selected_clips);
                    ui.close();
                }
            }
            if let Some(single_clip) = selected_clips
                .first()
                .filter(|_| selected_clips.len() == 1)
                .cloned()
            {
                if let Some(asset) = self
                    .editor
                    .project
                    .find_asset(single_clip.asset_id)
                    .cloned()
                {
                    if asset.is_generative()
                        && automation_button(ui.button("Open Asset Lab"), "Open Asset Lab")
                            .clicked()
                    {
                        let local_time = (self.editor.current_time - single_clip.start_time
                            + single_clip.trim_in_seconds)
                            .max(0.0);
                        self.open_asset_lab_at_time(asset.id, Some(local_time));
                        ui.close();
                    }
                    if asset.is_image() || asset.is_video() {
                        ui.separator();
                        ui.label(kit::caption("Generate"));
                    }
                    if asset.is_image() {
                        ui.menu_button("Create I2I from Image", |ui| {
                            if let Some(provider_id) = self.provider_choice_menu(
                                ui,
                                ProviderWorkflowKind::ImageToImage,
                                "Configure provider later",
                            ) {
                                self.create_i2i_from_single_clip(single_clip.id, provider_id);
                                ui.close();
                            }
                        });
                        ui.menu_button("Create I2V from Image", |ui| {
                            if let Some(provider_id) = self.provider_choice_menu(
                                ui,
                                ProviderWorkflowKind::ImageToVideo,
                                "Configure provider later",
                            ) {
                                self.create_i2v_from_single_clip(
                                    single_clip.id,
                                    SingleI2VReference::Image,
                                    provider_id,
                                );
                                ui.close();
                            }
                        });
                    }
                    if asset.is_video() {
                        ui.menu_button("Create I2V from First Frame", |ui| {
                            if let Some(provider_id) = self.provider_choice_menu(
                                ui,
                                ProviderWorkflowKind::ImageToVideo,
                                "Configure provider later",
                            ) {
                                self.create_i2v_from_single_clip(
                                    single_clip.id,
                                    SingleI2VReference::VideoFirstFrame,
                                    provider_id,
                                );
                                ui.close();
                            }
                        });
                        ui.menu_button("Extend I2V from Last Frame", |ui| {
                            if let Some(provider_id) = self.provider_choice_menu(
                                ui,
                                ProviderWorkflowKind::ImageToVideo,
                                "Configure provider later",
                            ) {
                                self.create_i2v_from_single_clip(
                                    single_clip.id,
                                    SingleI2VReference::VideoLastFrame,
                                    provider_id,
                                );
                                ui.close();
                            }
                        });
                    }
                }
            }
            if visual_count >= 2 {
                ui.separator();
                ui.label(kit::caption("Generate"));
                ui.menu_button("Generate Between Keyframes", |ui| {
                    if let Some(provider_id) = self.provider_choice_menu(
                        ui,
                        ProviderWorkflowKind::FirstFrameLastFrameVideo,
                        "Configure provider later",
                    ) {
                        let mut sorted = selected_clips.clone();
                        sorted.sort_by(|a, b| {
                            a.start_time
                                .partial_cmp(&b.start_time)
                                .unwrap_or(std::cmp::Ordering::Equal)
                        });
                        self.request_bridge_video_from_selected_clips(&sorted, provider_id);
                        ui.close();
                    }
                });
            }
            if !selected_clips.is_empty() {
                ui.separator();
            }
            if automation_button(
                ui.add_enabled(
                    !selected_clips.is_empty(),
                    egui::Button::new("Delete Clip(s)"),
                ),
                "Delete Clip(s)",
            )
            .clicked()
            {
                self.editor.delete_selected_clips();
                ui.close();
            }
        });
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
        let Some(track) = timeline_track_row_at_pos(pos, rects, tracks) else {
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

    fn set_timeline_zoom_at_view_x(
        &mut self,
        zoom: f32,
        duration: f64,
        viewport_w: f32,
        anchor_x: f32,
    ) {
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

        let anchor_x = anchor_x.clamp(0.0, viewport_w.max(0.0));
        let anchor_time = ((self.editor.layout.timeline_scroll_x + anchor_x) / old_zoom)
            .clamp(0.0, duration as f32);
        self.editor.layout.timeline_scroll_x = anchor_time * next_zoom - anchor_x;
        self.editor.layout.timeline_zoom = next_zoom;
        let content_w = (duration as f32 * next_zoom).max(viewport_w);
        self.clamp_timeline_scroll(content_w, viewport_w);
    }

    fn set_timeline_zoom_to_next_coarse(&mut self, direction: i32, duration: f64, viewport_w: f32) {
        let fps = self.editor.project.settings.fps.max(1.0) as f32;
        let (fit_zoom, max_zoom) = timeline_zoom_bounds(duration as f32, viewport_w, fps);
        let next_zoom = next_timeline_coarse_zoom(
            self.editor.layout.timeline_zoom,
            direction,
            fit_zoom,
            max_zoom,
        );
        self.set_timeline_zoom_anchored(next_zoom, duration, viewport_w);
    }

    fn clamp_timeline_scroll(&mut self, content_w: f32, viewport_w: f32) {
        let max_scroll = (content_w - viewport_w).max(0.0);
        if !self.editor.layout.timeline_scroll_x.is_finite() {
            self.editor.layout.timeline_scroll_x = 0.0;
        }
        self.editor.layout.timeline_scroll_x =
            self.editor.layout.timeline_scroll_x.clamp(0.0, max_scroll);
    }

    fn clamp_timeline_vertical_scroll(&mut self, max_scroll_y: f32) {
        if !self.editor.layout.timeline_scroll_y.is_finite() {
            self.editor.layout.timeline_scroll_y = 0.0;
        }
        self.editor.layout.timeline_scroll_y = self
            .editor
            .layout
            .timeline_scroll_y
            .clamp(0.0, max_scroll_y.max(0.0));
    }

    fn handle_timeline_keyboard(&mut self, ui: &mut Ui, duration: f64, viewport_w: f32) {
        if self.keyboard_shortcuts_suppressed(ui.ctx()) {
            return;
        }

        let zoom_in = ui.input(|input| {
            input.key_pressed(egui::Key::Plus) || input.key_pressed(egui::Key::Equals)
        });
        let zoom_out = ui.input(|input| input.key_pressed(egui::Key::Minus));
        if zoom_in {
            self.set_timeline_zoom_to_next_coarse(1, duration, viewport_w);
        }
        if zoom_out {
            self.set_timeline_zoom_to_next_coarse(-1, duration, viewport_w);
        }
    }

    fn handle_timeline_wheel(
        &mut self,
        ui: &mut Ui,
        viewport_rect: Rect,
        duration: f64,
        content_w: f32,
        viewport_w: f32,
        max_scroll_y: f32,
    ) {
        if self.modal_background_input_blocked() {
            return;
        }
        let Some(pointer) = ui.ctx().pointer_hover_pos() else {
            return;
        };
        if !viewport_rect.contains(pointer) {
            return;
        }
        let (ctrl_down, ctrl_zoom_delta, shift, smooth_delta, wheel_delta, plain_wheel_delta) = ui
            .input(|input| {
                let mut ctrl_zoom_delta = 0.0;
                let mut shift_wheel_delta = Vec2::ZERO;
                let mut plain_wheel_delta = Vec2::ZERO;
                for event in input.events.iter() {
                    if let egui::Event::MouseWheel {
                        delta, modifiers, ..
                    } = event
                    {
                        if modifiers.command || modifiers.ctrl || modifiers.mac_cmd {
                            ctrl_zoom_delta += if delta.y.abs() > 0.0 {
                                delta.y
                            } else {
                                delta.x
                            };
                        } else if modifiers.shift {
                            shift_wheel_delta += *delta;
                        } else {
                            plain_wheel_delta += *delta;
                        }
                    }
                }
                (
                    input.modifiers.command || input.modifiers.ctrl || input.modifiers.mac_cmd,
                    ctrl_zoom_delta,
                    input.modifiers.shift,
                    input.smooth_scroll_delta,
                    shift_wheel_delta,
                    plain_wheel_delta,
                )
            });

        let ctrl_zoom_delta = if ctrl_zoom_delta.abs() > 0.0 {
            ctrl_zoom_delta
        } else if ctrl_down && smooth_delta.y.abs() > 0.0 {
            smooth_delta.y
        } else {
            0.0
        };
        if ctrl_zoom_delta.abs() > 0.0 {
            let zoom_factor = (ctrl_zoom_delta * TIMELINE_WHEEL_ZOOM_SENSITIVITY)
                .exp()
                .clamp(0.5, 2.0);
            let anchor_x = pointer.x - viewport_rect.left();
            self.set_timeline_zoom_at_view_x(
                self.editor.layout.timeline_zoom * zoom_factor,
                duration,
                viewport_w,
                anchor_x,
            );
            ui.ctx().request_repaint();
            return;
        }

        if max_scroll_y > 0.0 && !shift && !ctrl_down {
            let vertical_delta = if plain_wheel_delta.y.abs() > 0.0 {
                plain_wheel_delta.y
            } else if smooth_delta.y.abs() > 0.0 && smooth_delta.x.abs() <= smooth_delta.y.abs() {
                smooth_delta.y
            } else {
                0.0
            };
            if vertical_delta.abs() > 0.0 {
                self.editor.layout.timeline_scroll_y -= vertical_delta;
                self.clamp_timeline_vertical_scroll(max_scroll_y);
                ui.ctx().request_repaint();
                return;
            }
        }

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
            painter.text(
                Pos2::new(x + 4.0, rect.top() + 4.0),
                egui::Align2::LEFT_TOP,
                timeline_ruler_label(t),
                FontId::monospace(9.0),
                kit::TEXT_DIM,
            );
        }
    }

    fn paint_timeline_grid_overlay(
        &self,
        painter: &egui::Painter,
        rects: TimelineRects,
        duration: f64,
        zoom: f32,
    ) {
        let scroll_x = self.editor.layout.timeline_scroll_x;
        let visible_start = (scroll_x / zoom).max(0.0) as f64;
        let visible_end = ((scroll_x + rects.tracks.width()) / zoom).min(duration as f32) as f64;
        let target_seconds = (90.0 / zoom.max(0.1)).max(0.5) as f64;
        let major_step = nice_timeline_step(target_seconds);
        let first_tick = (visible_start / major_step).floor() as i32 - 1;
        let last_tick = (visible_end / major_step).ceil() as i32 + 1;
        let stroke = Stroke::new(1.0, Color32::from_rgba_unmultiplied(86, 92, 104, 42));

        for tick in first_tick..=last_tick {
            if tick < 0 {
                continue;
            }
            let t = tick as f64 * major_step;
            if t > duration {
                continue;
            }
            let x = time_to_timeline_x(t, rects.tracks.left(), zoom, scroll_x);
            if x < rects.tracks.left() - 1.0 || x > rects.tracks.right() + 1.0 {
                continue;
            }
            painter.line_segment(
                [
                    Pos2::new(x, rects.tracks.top()),
                    Pos2::new(x, rects.tracks.bottom()),
                ],
                stroke,
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
        cache_buckets: Option<&[bool]>,
        contextual_keyframe_label: bool,
    ) {
        if clip_is_keyframe_image(clip, asset) {
            self.paint_timeline_keyframe_clip(
                painter,
                clip,
                asset,
                rect,
                accent,
                selected,
                thumbnail_tiles,
                contextual_keyframe_label,
            );
            return;
        }

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
        if let Some(buckets) = cache_buckets {
            paint_clip_cache_buckets(painter, rect, buckets);
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

    fn paint_timeline_keyframe_clip(
        &self,
        painter: &egui::Painter,
        clip: &Clip,
        asset: Option<&Asset>,
        rect: Rect,
        accent: Color32,
        selected: bool,
        thumbnail_tiles: &[TimelineThumbTile],
        show_label: bool,
    ) {
        let anchor_x = rect.left() + 4.0;
        let color = if selected {
            kit::BORDER_FOCUS
        } else {
            accent.gamma_multiply(0.82)
        };
        painter.line_segment(
            [
                Pos2::new(anchor_x, rect.top() + 2.0),
                Pos2::new(anchor_x, rect.bottom() - 2.0),
            ],
            Stroke::new(if selected { 2.0 } else { 1.35 }, color),
        );
        let head = [
            Pos2::new(anchor_x - 4.5, rect.top() + 1.0),
            Pos2::new(anchor_x + 4.5, rect.top() + 1.0),
            Pos2::new(anchor_x, rect.top() + 8.0),
        ];
        painter.add(egui::Shape::convex_polygon(
            head.to_vec(),
            color,
            Stroke::NONE,
        ));

        let thumb_size = TIMELINE_KEYFRAME_THUMB.min(rect.height() - 8.0).max(12.0);
        let thumb_rect = Rect::from_min_size(
            Pos2::new(anchor_x + 6.0, rect.top() + 2.0),
            Vec2::splat(thumb_size),
        );
        if thumb_rect.right() > rect.right() {
            return;
        }

        let thumb_frame = thumb_rect.expand(2.0);
        painter.rect_filled(
            thumb_frame,
            4.0,
            if selected {
                Color32::from_rgb(18, 50, 36)
            } else {
                Color32::from_rgb(21, 23, 27)
            },
        );
        painter.rect_stroke(
            thumb_frame,
            4.0,
            Stroke::new(if selected { 1.5 } else { 1.0 }, color),
            egui::StrokeKind::Inside,
        );

        if let Some(tile) = thumbnail_tiles.first() {
            let clip_painter = painter.with_clip_rect(thumb_rect);
            let scale = (thumb_rect.width() / tile.size.x)
                .max(thumb_rect.height() / tile.size.y)
                .max(0.01);
            let image_rect = Rect::from_center_size(thumb_rect.center(), tile.size * scale);
            clip_painter.image(
                tile.texture_id,
                image_rect,
                Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                Color32::from_white_alpha(if selected { 255 } else { 210 }),
            );
            painter.rect_stroke(
                thumb_rect,
                3.0,
                Stroke::new(1.0, color.gamma_multiply(0.75)),
                egui::StrokeKind::Inside,
            );
        } else {
            painter.rect_filled(thumb_rect, 3.0, kit::FIELD_BG);
            painter.text(
                thumb_rect.center(),
                egui::Align2::CENTER_CENTER,
                "IMG",
                FontId::proportional(8.5),
                kit::IMAGE,
            );
        }

        if show_label {
            let label_left = thumb_frame.right() + 5.0;
            let label = timeline_clip_title(clip, asset);
            let font_id = FontId::proportional(10.0);
            let text_w = painter
                .layout_no_wrap(label.clone(), font_id.clone(), kit::TEXT)
                .size()
                .x;
            let desired_w = (text_w + 12.0).clamp(36.0, TIMELINE_KEYFRAME_LABEL_W);
            let label_w = desired_w.min((painter.clip_rect().right() - label_left).max(0.0));
            if label_w >= 36.0 {
                let label_rect = Rect::from_min_size(
                    Pos2::new(label_left, rect.center().y - TIMELINE_MARKER_LABEL_H * 0.5),
                    Vec2::new(label_w, TIMELINE_MARKER_LABEL_H),
                );
                painter.rect_filled(label_rect, 4.0, Color32::from_rgb(21, 23, 27));
                painter.rect_stroke(
                    label_rect,
                    4.0,
                    Stroke::new(1.0, color.gamma_multiply(if selected { 1.0 } else { 0.72 })),
                    egui::StrokeKind::Inside,
                );
                let text_painter = painter.with_clip_rect(label_rect.shrink2(Vec2::new(6.0, 1.0)));
                text_painter.text(
                    label_rect.left_center() + Vec2::new(6.0, 0.0),
                    egui::Align2::LEFT_CENTER,
                    label,
                    font_id,
                    kit::TEXT,
                );
            }
        }
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
        let button_y = rects.add_row.center().y - 12.0;
        let video_rect = Rect::from_min_size(
            Pos2::new(rects.add_row.left() + 12.0, button_y),
            Vec2::new(56.0, 24.0),
        );
        let audio_rect = Rect::from_min_size(
            Pos2::new(video_rect.right() + 6.0, button_y),
            Vec2::new(56.0, 24.0),
        );
        let marker_rect = Rect::from_min_size(
            Pos2::new(audio_rect.right() + 6.0, button_y),
            Vec2::new(66.0, 24.0),
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
        let marker_resp = ui.interact(
            marker_rect,
            ui.id().with("timeline-add-marker"),
            Sense::click(),
        );
        if video_resp.clicked() {
            let track_id = self.editor.project.add_video_track();
            self.editor.selection.select_track(track_id);
            self.editor.status = "Added video track".to_string();
        }
        if audio_resp.clicked() {
            let track_id = self.editor.project.add_audio_track();
            self.editor.selection.select_track(track_id);
            self.editor.status = "Added audio track".to_string();
        }
        if marker_resp.clicked() {
            let track_id = self.editor.project.add_marker_track();
            self.editor.selection.select_track(track_id);
            self.editor.status = "Added marker track".to_string();
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
        paint_dashed_timeline_button(
            painter,
            marker_rect,
            "+ Marker",
            kit::MARKER,
            marker_resp.hovered(),
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

    fn paint_timeline_vertical_scrollbar(
        &mut self,
        ui: &mut Ui,
        painter: &egui::Painter,
        rects: TimelineRects,
        content_h: f32,
    ) {
        let viewport_h = rects.tracks.height().max(1.0);
        if content_h <= viewport_h + 1.0 {
            return;
        }

        let max_scroll = (content_h - viewport_h).max(0.0);
        let rail = Rect::from_min_max(
            Pos2::new(rects.tracks.right() - 7.0, rects.tracks.top() + 4.0),
            Pos2::new(rects.tracks.right() - 3.0, rects.tracks.bottom() - 4.0),
        );
        let handle_h = (viewport_h / content_h * rail.height()).clamp(28.0, rail.height());
        let handle_y = rail.top()
            + (self.editor.layout.timeline_scroll_y / max_scroll) * (rail.height() - handle_h);
        let handle = Rect::from_min_size(
            Pos2::new(rail.left(), handle_y),
            Vec2::new(rail.width(), handle_h),
        );
        painter.rect_filled(rail, 2.0, Color32::from_rgba_unmultiplied(5, 7, 10, 110));
        painter.rect_filled(handle, 2.0, kit::BORDER.gamma_multiply(1.25));

        let response = ui.interact(
            rail.expand2(Vec2::new(6.0, 0.0)),
            ui.id().with("timeline-vertical-scrollbar"),
            Sense::click_and_drag(),
        );
        if (response.dragged() || response.clicked()) && response.interact_pointer_pos().is_some() {
            let pos = response.interact_pointer_pos().unwrap();
            let ratio = ((pos.y - rail.top() - handle_h * 0.5)
                / (rail.height() - handle_h).max(1.0))
            .clamp(0.0, 1.0);
            self.editor.layout.timeline_scroll_y = ratio * max_scroll;
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
                timeline_track_row_at_pos(pos, rects, tracks)
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
                let toggle_select = multi_select_modifier(ui);
                match timeline_hit(pos, rects, tracks, clip_geoms, marker_geoms) {
                    TimelineHit::Ruler => {
                        self.timeline_scrub_was_playing = self.editor.is_playing;
                        self.timeline_last_scrub_audio_time = None;
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
                            if !self.editor.selection.clip_ids.contains(&id) {
                                self.editor.selection.select_clip(id);
                            }
                            self.timeline_drag = Some(TimelineDrag::ClipResizeLeft {
                                clip_id: id,
                                start_time: clip.start_time,
                                duration: clip.duration,
                            });
                        }
                    }
                    TimelineHit::ClipRightEdge(id) => {
                        if let Some(clip) = clips.iter().find(|clip| clip.id == id) {
                            if !self.editor.selection.clip_ids.contains(&id) {
                                self.editor.selection.select_clip(id);
                            }
                            self.timeline_drag = Some(TimelineDrag::ClipResizeRight {
                                clip_id: id,
                                start_time: clip.start_time,
                                duration: clip.duration,
                            });
                        }
                    }
                    TimelineHit::ClipBody(id) => {
                        if let Some(clip) = clips.iter().find(|clip| clip.id == id) {
                            if toggle_select {
                                self.editor.selection.toggle_clip(id);
                            } else if !self.editor.selection.clip_ids.contains(&id) {
                                self.editor.selection.select_clip(id);
                            }
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

        if response.double_clicked() {
            if let Some(pos) = response.interact_pointer_pos() {
                match timeline_hit(pos, rects, tracks, clip_geoms, marker_geoms) {
                    TimelineHit::ClipLeftEdge(id)
                    | TimelineHit::ClipRightEdge(id)
                    | TimelineHit::ClipBody(id) => {
                        if let Some(clip) = clips.iter().find(|clip| clip.id == id) {
                            let local_time = (self.editor.current_time - clip.start_time
                                + clip.trim_in_seconds)
                                .max(0.0);
                            self.open_asset_lab_at_time(clip.asset_id, Some(local_time));
                        }
                    }
                    _ => {}
                }
            }
        }

        if response.clicked() {
            if let Some(pos) = response.interact_pointer_pos() {
                let toggle_select = multi_select_modifier(ui);
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
                    | TimelineHit::ClipBody(id) => {
                        if toggle_select {
                            self.editor.selection.toggle_clip(id);
                        } else {
                            self.editor.selection.select_clip(id);
                        }
                    }
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
                    let is_keyframe_reference = self
                        .editor
                        .project
                        .clips
                        .iter()
                        .find(|clip| clip.id == clip_id)
                        .is_some_and(|clip| self.editor.project.is_keyframe_reference_clip(clip));
                    let source_frames = if is_keyframe_reference {
                        vec![new_start_frames]
                    } else {
                        vec![new_start_frames, new_start_frames + duration_frames]
                    };
                    if let Some(hit) =
                        best_snap_delta_frames(&source_frames, &targets, snap_threshold_frames)
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
                let new_start = seconds_from_frames(new_start_frames.round().max(0.0), fps);
                let mut changed = self.editor.project.move_clip(clip_id, new_start);
                if let Some(track_id) =
                    timeline_track_row_at_pos(pos, rects, &self.editor.project.tracks)
                        .map(|track| track.id)
                {
                    changed |= self.editor.project.move_clip_to_track(clip_id, track_id);
                }
                if changed {
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
            if !self.editor.project.is_keyframe_reference_clip(clip) {
                targets.push(SnapTarget::clip_edge(
                    frames_from_seconds(clip.end_time(), fps).round(),
                    clip.id,
                ));
            }
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
