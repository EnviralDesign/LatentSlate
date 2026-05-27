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
    export_video, TimestampOverlayPosition, TimestampOverlaySettings, VideoExportCodec,
    VideoExportEvent, VideoExportFrameFormat, VideoExportJob, VideoExportPreview,
    VideoExportQuality, VideoExportSettings, VideoExportSummary,
};
use crate::core::generation::{
    compatible_asset_for_provider_input, next_version_label, random_seed_i64,
    resolve_provider_inputs, resolve_seed_field, semantic_reference_slot, update_seed_inputs,
};
use crate::core::preview::{PreviewDecodeMode, PreviewLayerStack, PreviewStats, RenderOutput};
use crate::core::timeline_snap::{
    best_snap_delta_frames, frames_from_seconds, seconds_from_frames, snap_time_to_frame,
    SnapTarget,
};
use crate::editor::{
    default_generative_video_fps, default_generative_video_frames, default_projects_dir,
    EditorState,
};
use crate::providers::ProviderProgress;
use crate::state::{
    asset_display_name, input_value_as_bool, input_value_as_f64, input_value_as_i64,
    input_value_as_string, parse_version_index, Asset, AssetKind, Clip, ClipImageMode,
    ClipTransform, ComfyOutputSelector, ComfyWorkflowRef, GenerationJob, GenerationJobStatus,
    GenerationRecord, GenerationSeedAdvance, GenerativeConfig, InputBinding, InputUi, InputValue,
    ManifestInput, NodeSelector, Project, ProjectSettings, ProviderConnection, ProviderEntry,
    ProviderInputField, ProviderInputType, ProviderManifest, ProviderOutputType, SeedStrategy,
    TrackType,
};
use crate::ui_kit as kit;
use egui_extras::{Size, StripBuilder};
use serde::Serialize;

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
    provider_template_kind: ProviderTemplateKind,
    asset_delete_confirmation: Option<AssetDeleteConfirmation>,
    track_delete_confirmation: Option<TrackDeleteConfirmation>,
    bridge_keyframe_confirmation: Option<BridgeKeyframeConfirmation>,
    generation_runtime: Option<tokio::runtime::Runtime>,
    generation_events_tx: mpsc::Sender<GenerationEvent>,
    generation_events_rx: mpsc::Receiver<GenerationEvent>,
    generation_active: Option<Uuid>,
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

#[derive(Clone, Debug, Default)]
struct ApiKeyModalState {
    credential_id: String,
    label: String,
    value: String,
    saved: bool,
    masked_existing: bool,
    error: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ProviderTemplateKind {
    ComfyUi,
    OpenAiImage,
    XaiImage,
    XaiVideo,
}

impl Default for ProviderTemplateKind {
    fn default() -> Self {
        ProviderTemplateKind::ComfyUi
    }
}

impl ProviderTemplateKind {
    const ALL: [ProviderTemplateKind; 4] = [
        ProviderTemplateKind::ComfyUi,
        ProviderTemplateKind::OpenAiImage,
        ProviderTemplateKind::XaiImage,
        ProviderTemplateKind::XaiVideo,
    ];
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
enum PreviewTransformDrag {
    Pan {
        start_pan: Vec2,
        start_pointer: Pos2,
    },
    Move {
        clip_id: Uuid,
        start_transform: ClipTransform,
        start_pointer_project: Pos2,
        start_half_size: Vec2,
    },
    Scale {
        clip_id: Uuid,
        handle: PreviewScaleHandle,
        start_transform: ClipTransform,
        start_center_project: Pos2,
        start_half_size: Vec2,
    },
    Rotate {
        clip_id: Uuid,
        start_transform: ClipTransform,
        start_center_project: Pos2,
        start_pointer_angle: f32,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PreviewScaleHandle {
    NorthWest,
    North,
    NorthEast,
    East,
    SouthEast,
    South,
    SouthWest,
    West,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PreviewScaleSnapAxis {
    X,
    Y,
}

#[derive(Clone, Copy, Debug)]
struct PreviewSnapGuide {
    start: Pos2,
    end: Pos2,
}

#[derive(Clone, Debug)]
struct PreviewObjectGeometry {
    clip_id: Uuid,
    project_rect: Rect,
    screen_corners: [Pos2; 4],
    screen_center: Pos2,
    project_to_screen: f32,
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
    track_scroll_y: f32,
}

#[derive(Clone, Copy, Debug)]
struct TimelineClipGeom {
    clip_id: Uuid,
    rect: Rect,
    keyframe: bool,
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

#[derive(Clone, Debug, PartialEq, Eq)]
enum ExportRunStatus {
    Idle,
    Running,
    Finished,
    Cancelled,
    Failed,
}

#[derive(Clone, Debug)]
struct ExportModalState {
    output_path: String,
    codec: VideoExportCodec,
    width: String,
    height: String,
    fps: String,
    start_seconds: String,
    duration_seconds: String,
    include_audio: bool,
    quality: VideoExportQuality,
    frame_format: VideoExportFrameFormat,
    timestamp_overlay_enabled: bool,
    timestamp_overlay_position: TimestampOverlayPosition,
    status: ExportRunStatus,
    progress: f32,
    stage: String,
    message: String,
    frame_label: String,
    error: Option<String>,
    summary: Option<VideoExportSummary>,
    warnings: Vec<String>,
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

#[derive(Clone, Debug)]
struct AssetDeleteConfirmation {
    asset_ids: Vec<Uuid>,
    asset_count: usize,
    clip_count: usize,
    sample_names: Vec<String>,
}

#[derive(Clone, Debug)]
struct TrackDeleteConfirmation {
    track_ids: Vec<Uuid>,
    track_count: usize,
    clip_count: usize,
    marker_count: usize,
    sample_names: Vec<String>,
}

#[derive(Clone, Debug)]
struct BridgeKeyframeConfirmation {
    clip_ids: Vec<Uuid>,
    convert_clip_ids: Vec<Uuid>,
    sample_names: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ProviderBuilderTab {
    Output,
    Inputs,
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
    node_id: Option<String>,
    class_type: String,
    title: Option<String>,
}

#[derive(Clone, Debug)]
struct ProviderNodeSelectorDraft {
    node_id: Option<String>,
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
            provider_template_kind: ProviderTemplateKind::default(),
            asset_delete_confirmation: None,
            track_delete_confirmation: None,
            bridge_keyframe_confirmation: None,
            generation_runtime,
            generation_events_tx,
            generation_events_rx,
            generation_active: None,
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

    fn open_export_modal(&mut self) {
        if self.export_cancel.is_none() {
            self.export_modal = ExportModalState::for_project(&self.editor.project);
            self.export_preview_texture = None;
        }
        self.editor.overlays.export_video = true;
    }

    fn close_or_cancel_export_modal(&mut self) {
        if let Some(cancel) = &self.export_cancel {
            cancel.store(true, Ordering::Relaxed);
            self.export_modal.message = "Cancelling export...".to_string();
            self.export_modal.status = ExportRunStatus::Running;
        } else {
            self.editor.overlays.export_video = false;
        }
    }

    fn keyboard_shortcuts_suppressed(&self, ctx: &Context) -> bool {
        ctx.text_edit_focused()
            || self.editor.show_startup()
            || self.editor.overlays.new_project
            || self.editor.overlays.project_settings
            || self.editor.overlays.generative_video
            || self.editor.overlays.export_video
            || self.editor.overlays.providers
            || self.editor.overlays.api_keys
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
                            automation_checkbox(
                                ui,
                                &mut this.editor.layout.left_collapsed,
                                "Collapse Assets",
                            );
                            automation_checkbox(
                                ui,
                                &mut this.editor.layout.right_collapsed,
                                "Collapse Attributes",
                            );
                            automation_checkbox(
                                ui,
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
            self.asset_drop_target_rect = Some(response.response.rect);
            self.asset_drop_target_hovered = response.response.hovered();
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
        self.asset_drop_target_rect = Some(response.response.rect);
        self.asset_drop_target_hovered = response.response.hovered();
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
            let assets: Vec<Asset> = self.editor.project.assets.clone();
            for asset in assets {
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
                response.context_menu(|ui| {
                    if automation_button(ui.button("Add to timeline"), "Add to timeline").clicked()
                    {
                        if let Err(err) = self.editor.add_asset_to_timeline(asset.id, None) {
                            self.editor.status = err;
                        }
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

    fn request_delete_selected_assets(&mut self) {
        let asset_ids = self.editor.selection.asset_ids.clone();
        self.request_delete_assets(&asset_ids);
    }

    fn request_delete_assets(&mut self, asset_ids: &[Uuid]) {
        if let Some(confirmation) = self.asset_delete_confirmation(asset_ids) {
            self.asset_delete_confirmation = Some(confirmation);
        }
    }

    fn asset_delete_confirmation(&self, asset_ids: &[Uuid]) -> Option<AssetDeleteConfirmation> {
        let unique_asset_ids = unique_uuid_list(asset_ids);
        if unique_asset_ids.is_empty() {
            return None;
        }

        let existing_asset_ids: Vec<Uuid> = unique_asset_ids
            .into_iter()
            .filter(|asset_id| self.editor.project.find_asset(*asset_id).is_some())
            .collect();
        if existing_asset_ids.is_empty() {
            return None;
        }

        let clip_count = self
            .editor
            .project
            .clips
            .iter()
            .filter(|clip| existing_asset_ids.contains(&clip.asset_id))
            .count();
        let sample_names = existing_asset_ids
            .iter()
            .filter_map(|asset_id| self.editor.project.find_asset(*asset_id))
            .take(3)
            .map(|asset| asset.name.clone())
            .collect();

        Some(AssetDeleteConfirmation {
            asset_count: existing_asset_ids.len(),
            asset_ids: existing_asset_ids,
            clip_count,
            sample_names,
        })
    }

    fn perform_delete_assets(&mut self, asset_ids: &[Uuid]) -> (usize, usize) {
        let unique_asset_ids = unique_uuid_list(asset_ids);
        let result = self.editor.delete_assets(&unique_asset_ids);
        if result.0 > 0 {
            for asset_id in unique_asset_ids {
                self.invalidate_asset_visual_cache(asset_id);
            }
            self.editor.preview_dirty = true;
        }
        result
    }

    fn request_delete_selected_tracks(&mut self) {
        let track_ids = self.editor.selection.track_ids.clone();
        self.request_delete_tracks(&track_ids);
    }

    fn request_delete_tracks(&mut self, track_ids: &[Uuid]) {
        if let Some(confirmation) = self.track_delete_confirmation(track_ids) {
            self.track_delete_confirmation = Some(confirmation);
        }
    }

    fn track_delete_confirmation(&self, track_ids: &[Uuid]) -> Option<TrackDeleteConfirmation> {
        let unique_track_ids = unique_uuid_list(track_ids);
        if unique_track_ids.is_empty() {
            return None;
        }

        let existing_track_ids: Vec<Uuid> = unique_track_ids
            .into_iter()
            .filter(|track_id| {
                self.editor
                    .project
                    .tracks
                    .iter()
                    .any(|track| track.id == *track_id)
            })
            .collect();
        if existing_track_ids.is_empty() {
            return None;
        }

        let mut clip_count = 0usize;
        let mut marker_count = 0usize;
        for track_id in existing_track_ids.iter().copied() {
            let (clips, markers) = self.editor.project.track_delete_counts(track_id);
            clip_count += clips;
            marker_count += markers;
        }
        let sample_names = existing_track_ids
            .iter()
            .filter_map(|track_id| {
                self.editor
                    .project
                    .tracks
                    .iter()
                    .find(|track| track.id == *track_id)
            })
            .take(4)
            .map(|track| track.name.clone())
            .collect();

        Some(TrackDeleteConfirmation {
            track_count: existing_track_ids.len(),
            track_ids: existing_track_ids,
            clip_count,
            marker_count,
            sample_names,
        })
    }

    fn perform_delete_tracks(&mut self, track_ids: &[Uuid]) -> usize {
        let unique_track_ids = unique_uuid_list(track_ids);
        let mut deleted = 0usize;
        for track_id in unique_track_ids {
            if self.editor.project.remove_track(track_id) {
                deleted += 1;
            }
        }
        if deleted > 0 {
            self.editor.selection.clear();
            self.editor.preview_dirty = true;
            self.editor.status = if deleted == 1 {
                "Deleted track".to_string()
            } else {
                format!("Deleted {deleted} tracks")
            };
            self.refresh_audio_playback_items();
        }
        deleted
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
        let mut create_bridge = false;
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

        let image_clip_ids: Vec<Uuid> = clips
            .iter()
            .filter_map(|clip| {
                self.editor
                    .project
                    .find_asset(clip.asset_id)
                    .filter(|asset| asset.is_image())
                    .map(|_| clip.id)
            })
            .collect();
        if image_clip_ids.len() >= 2 {
            ui.add_space(kit::FORM_ROW_GAP);
            inspector_card(ui, "Generation", |ui| {
                ui.label(kit::caption(
                    "Create a new generative video asset using the first and last selected image clips as pinned references.",
                ));
                ui.add_space(kit::ACTION_GAP);
                if kit::primary_button(ui, "Generate Between Keyframes", ui.available_width())
                    .clicked()
                {
                    create_bridge = true;
                }
            });
        }

        if apply_spacing && clips.len() >= 2 {
            self.space_selected_clips(&clips);
        }
        if create_bridge {
            self.request_bridge_video_from_selected_clips(&clips);
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

    fn request_bridge_video_from_selected_clips(&mut self, clips: &[Clip]) {
        let image_clips = self.bridge_image_clips(clips);
        let (Some(first), Some(last)) = (image_clips.first(), image_clips.last()) else {
            self.editor.status = "Select at least two image clips first.".to_string();
            return;
        };
        if first.id == last.id {
            self.editor.status = "Select two different image clips.".to_string();
            return;
        }

        let convert_clip_ids: Vec<Uuid> = [first, last]
            .iter()
            .filter(|clip| {
                self.editor
                    .project
                    .find_asset(clip.asset_id)
                    .is_some_and(|asset| !clip_is_keyframe_image(clip, Some(asset)))
            })
            .map(|clip| clip.id)
            .collect();
        if convert_clip_ids.is_empty() {
            self.create_bridge_video_from_selected_clips(&image_clips);
            return;
        }

        let sample_names = [first, last]
            .iter()
            .filter_map(|clip| self.editor.project.find_asset(clip.asset_id))
            .map(|asset| asset.name.clone())
            .collect();
        self.bridge_keyframe_confirmation = Some(BridgeKeyframeConfirmation {
            clip_ids: image_clips.iter().map(|clip| clip.id).collect(),
            convert_clip_ids,
            sample_names,
        });
    }

    fn bridge_image_clips(&self, clips: &[Clip]) -> Vec<Clip> {
        let mut image_clips: Vec<Clip> = clips
            .iter()
            .filter(|clip| {
                self.editor
                    .project
                    .find_asset(clip.asset_id)
                    .is_some_and(|asset| asset.is_image())
            })
            .cloned()
            .collect();
        image_clips.sort_by(|a, b| {
            a.start_time
                .partial_cmp(&b.start_time)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.id.cmp(&b.id))
        });
        image_clips
    }

    fn create_bridge_video_from_clip_ids(&mut self, clip_ids: &[Uuid]) {
        let clips: Vec<Clip> = self
            .editor
            .project
            .clips
            .iter()
            .filter(|clip| clip_ids.contains(&clip.id))
            .cloned()
            .collect();
        let image_clips = self.bridge_image_clips(&clips);
        self.create_bridge_video_from_selected_clips(&image_clips);
    }

    fn create_bridge_video_from_selected_clips(&mut self, clips: &[Clip]) {
        let image_clips: Vec<Clip> = clips
            .iter()
            .filter(|clip| {
                self.editor
                    .project
                    .find_asset(clip.asset_id)
                    .is_some_and(|asset| asset.is_image())
            })
            .cloned()
            .collect();
        let (Some(first), Some(last)) = (image_clips.first(), image_clips.last()) else {
            self.editor.status = "Select at least two image clips first.".to_string();
            return;
        };
        if first.id == last.id {
            self.editor.status = "Select two different image clips.".to_string();
            return;
        }

        let fallback_duration =
            default_generative_video_frames() as f64 / default_generative_video_fps();
        let duration = (last.start_time - first.start_time)
            .abs()
            .max(fallback_duration);
        let fps = default_generative_video_fps();
        let frame_count = frames_from_seconds(duration, fps).round().max(1.0) as u32;
        let start_time = first.start_time.min(last.start_time);
        let target_track_id = self.bridge_target_track_id(first.track_id);

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
                config.reference_slots.insert(
                    "start_image".to_string(),
                    InputValue::AssetRef {
                        asset_id: first.asset_id,
                        source_clip_id: Some(first.id),
                        pinned: true,
                    },
                );
                config.reference_slots.insert(
                    "end_image".to_string(),
                    InputValue::AssetRef {
                        asset_id: last.asset_id,
                        source_clip_id: Some(last.id),
                        pinned: true,
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
        self.editor.status = "Created generative video bridge from selected keyframes.".to_string();
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
                        let width = ui.available_width();
                        if inspector_drag_i64(ui, &label, &mut value, 1.0, width) {
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
                })
                .or(current_binding.clone()),
            Some(value) => Some(value),
            None => auto_candidate
                .as_ref()
                .map(|candidate| InputValue::AssetRef {
                    asset_id: candidate.asset_id,
                    source_clip_id: candidate.source_clip_id,
                    pinned: false,
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
                let label = format!("Auto: {}", candidate.label);
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
                    });
                    ui.close();
                }
            }
        });

        if let Some(InputValue::AssetRef {
            asset_id,
            source_clip_id,
            pinned,
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
            .map(|clip| format!(" @ {}", timecode(clip.start_time)))
            .unwrap_or_default();
        Some(format!(
            "{prefix}: {}{}",
            asset_display_name(asset),
            clip_suffix
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
            if !compatible_asset_for_provider_input(asset, &input.input_type) {
                continue;
            }
            let distance = target_time
                .map(|target| (clip.start_time - target).abs())
                .unwrap_or(0.0);
            let track_score = match (
                context_track_index,
                self.editor
                    .project
                    .tracks
                    .iter()
                    .position(|track| track.id == clip.track_id),
            ) {
                (Some(context_index), Some(index)) if index > context_index => 0.0,
                (Some(context_index), Some(index)) => {
                    (context_index as f64 - index as f64).abs() * 0.25
                }
                _ => 0.0,
            };
            candidates.push(AssetInputCandidate {
                asset_id: asset.id,
                source_clip_id: Some(clip.id),
                label: asset_display_name(asset),
                detail: timecode(clip.start_time),
                contextual: context_clip_id.is_some(),
                score: distance + track_score,
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
        if asset_snapshot
            .as_ref()
            .is_some_and(|asset| asset.is_generative())
        {
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
                let auto_rect = Rect::from_min_size(
                    Pos2::new(title_rect.right() + 8.0, header_rect.center().y - 11.0),
                    Vec2::new(44.0, 22.0),
                );
                let mut auto_ui = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(auto_rect)
                        .layout(Layout::left_to_right(Align::Center)),
                );
                if kit::timeline_tool_text_button(&mut auto_ui, "Auto", 44.0, self.preview_auto_fit)
                    .on_hover_text("Auto-fit preview canvas")
                    .clicked()
                {
                    self.preview_auto_fit = !self.preview_auto_fit;
                    if self.preview_auto_fit {
                        self.preview_pan = Vec2::ZERO;
                    }
                }
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
                let (rect, response) = ui.allocate_exact_size(
                    Vec2::new(available.x, preview_height),
                    Sense::click_and_drag(),
                );
                self.paint_preview(ui, rect.shrink(8.0), &response);
            });
    }

    fn paint_preview(&mut self, ui: &mut Ui, rect: Rect, response: &egui::Response) {
        let painter = ui.painter().with_clip_rect(rect);
        if let Some(layers) = self.preview_layers.clone() {
            let fit_scale = preview_fit_scale(rect, &layers);
            self.handle_preview_view_input(ui, response, rect, &layers, fit_scale);
            let scale = self.preview_canvas_screen_scale(fit_scale);
            let canvas_size = Vec2::new(
                layers.canvas_width as f32 * scale,
                layers.canvas_height as f32 * scale,
            );
            let canvas_rect = Rect::from_center_size(rect.center() + self.preview_pan, canvas_size);
            let layer_painter = painter.with_clip_rect(canvas_rect.intersect(rect));
            for layer in layers.layers.iter() {
                let Some(texture) = self.preview_layer_textures.get(&layer.texture_key) else {
                    continue;
                };
                let placement = layer.placement;
                let layer_rect = Rect::from_min_size(
                    canvas_rect.min
                        + Vec2::new(placement.offset_x * scale, placement.offset_y * scale),
                    Vec2::new(placement.scaled_w * scale, placement.scaled_h * scale),
                );
                let alpha = (placement.opacity.clamp(0.0, 1.0) * 255.0).round() as u8;
                let tint = Color32::from_white_alpha(alpha);
                if placement.rotation_deg.abs() <= 0.01 {
                    layer_painter.image(
                        texture.texture.id(),
                        layer_rect,
                        Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                        tint,
                    );
                } else {
                    paint_rotated_texture(
                        &layer_painter,
                        texture.texture.id(),
                        layer_rect,
                        placement.rotation_deg,
                        tint,
                    );
                }
            }
            let mut object_geometries = self.preview_object_geometries(&layers, canvas_rect, scale);
            object_geometries.extend(self.paint_preview_keyframe_reference_ghosts(
                ui,
                &layer_painter,
                canvas_rect,
                &layers,
                scale,
            ));
            self.paint_preview_transform_overlay(
                ui,
                rect,
                canvas_rect,
                &layers,
                &object_geometries,
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
                let cache = self.editor.previewer.cache_stats();
                let cache_mb = cache.total_bytes as f64 / (1024.0 * 1024.0);
                let cache_max_mb = cache.max_bytes as f64 / (1024.0 * 1024.0);
                let render_state = if self.preview_render_in_flight.load(Ordering::Relaxed) {
                    format!(
                        "busy {:.1}ms",
                        self.preview_render_busy_since
                            .map(|start| start.elapsed().as_secs_f64() * 1000.0)
                            .unwrap_or_default()
                    )
                } else {
                    "idle".to_string()
                };
                let hit_total = stats.cache_hits + stats.cache_misses;
                let hit_rate = if hit_total > 0 {
                    stats.cache_hits as f64 / hit_total as f64 * 100.0
                } else {
                    0.0
                };
                let text = format!(
                    concat!(
                        "async {}\n",
                        "worker {:.1}ms  delivery {:.1}ms\n",
                        "total {:.1}ms  upload {:.1}ms\n",
                        "scan {:.1}ms  comp {:.1}ms\n",
                        "vdec {:.1}ms  still {:.1}ms\n",
                        "  seek {:.1}  pkt {:.1}\n",
                        "  xfer {:.1}  scale {:.1}  copy {:.1}\n",
                        "cache {:.0}/{:.0}MB  entries {}\n",
                        "hit {} miss {} ({:.0}%)\n",
                        "indexed assets {} frames {}\n",
                        "layers {}  stale {}"
                    ),
                    render_state,
                    self.preview_render_last_worker_ms.unwrap_or_default(),
                    self.preview_render_last_delivery_ms.unwrap_or_default(),
                    stats.total_ms,
                    stats.encode_ms,
                    stats.collect_ms,
                    stats.composite_ms,
                    stats.video_decode_ms,
                    stats.still_load_ms,
                    stats.video_decode_seek_ms,
                    stats.video_decode_packet_ms,
                    stats.video_decode_transfer_ms,
                    stats.video_decode_scale_ms,
                    stats.video_decode_copy_ms,
                    cache_mb,
                    cache_max_mb,
                    cache.entry_count,
                    stats.cache_hits,
                    stats.cache_misses,
                    hit_rate,
                    cache.indexed_asset_count,
                    cache.indexed_frame_count,
                    stats.layers,
                    self.preview_render_stale_count,
                );
                let stats_rect = Rect::from_min_size(
                    rect.right_top() + Vec2::new(-274.0, 12.0),
                    Vec2::new(258.0, 176.0),
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

    fn preview_canvas_screen_scale(&mut self, fit_scale: f32) -> f32 {
        if self.preview_auto_fit {
            self.preview_zoom = fit_scale;
            self.preview_pan = Vec2::ZERO;
            fit_scale
        } else {
            if !self.preview_zoom.is_finite() || self.preview_zoom <= 0.0 {
                self.preview_zoom = fit_scale;
            }
            self.preview_zoom
                .clamp(PREVIEW_ZOOM_MIN.max(fit_scale * 0.1), PREVIEW_ZOOM_MAX)
        }
    }

    fn handle_preview_view_input(
        &mut self,
        ui: &mut Ui,
        _response: &egui::Response,
        rect: Rect,
        layers: &PreviewLayerStack,
        fit_scale: f32,
    ) {
        let pointer = ui
            .ctx()
            .pointer_interact_pos()
            .or_else(|| ui.ctx().pointer_hover_pos());
        let pointer_in_preview = pointer.map(|point| rect.contains(point)).unwrap_or(false);
        let secondary_pressed_in_preview =
            ui.input(|input| input.pointer.secondary_pressed()) && pointer_in_preview;
        if secondary_pressed_in_preview {
            if let Some(start_pointer) = pointer {
                self.preview_auto_fit = false;
                if !self.preview_zoom.is_finite() || self.preview_zoom <= 0.0 {
                    self.preview_zoom = fit_scale;
                }
                self.preview_drag = Some(PreviewTransformDrag::Pan {
                    start_pan: self.preview_pan,
                    start_pointer,
                });
            }
        }
        if let Some(PreviewTransformDrag::Pan {
            start_pan,
            start_pointer,
        }) = self.preview_drag
        {
            if ui.input(|input| input.pointer.secondary_down()) {
                if let Some(pointer) = pointer {
                    self.preview_pan = start_pan + (pointer - start_pointer);
                    ui.ctx().set_cursor_icon(egui::CursorIcon::AllScroll);
                    ui.ctx().request_repaint();
                }
            } else {
                self.preview_drag = None;
            }
        }

        let scroll_delta = preview_scroll_delta(ui, rect);
        if scroll_delta.abs() <= 0.0 {
            return;
        }

        let old_scale = if self.preview_auto_fit {
            fit_scale
        } else {
            self.preview_zoom
        }
        .clamp(PREVIEW_ZOOM_MIN, PREVIEW_ZOOM_MAX);
        let old_canvas_rect = Rect::from_center_size(
            rect.center() + self.preview_pan,
            Vec2::new(
                layers.canvas_width as f32 * old_scale,
                layers.canvas_height as f32 * old_scale,
            ),
        );
        let pointer = ui.ctx().pointer_hover_pos().unwrap_or(rect.center());
        let canvas_point = (pointer - old_canvas_rect.min) / old_scale.max(0.0001);
        let zoom_factor = (scroll_delta * PREVIEW_SCROLL_ZOOM_SENSITIVITY)
            .exp()
            .clamp(0.25, 4.0);

        self.preview_auto_fit = false;
        self.preview_zoom = (old_scale * zoom_factor).clamp(PREVIEW_ZOOM_MIN, PREVIEW_ZOOM_MAX);
        let new_size = Vec2::new(
            layers.canvas_width as f32 * self.preview_zoom,
            layers.canvas_height as f32 * self.preview_zoom,
        );
        let new_min = pointer - canvas_point * self.preview_zoom;
        let new_center = new_min + new_size * 0.5;
        self.preview_pan = new_center - rect.center();
        ui.ctx().request_repaint();
    }

    fn preview_object_geometries(
        &self,
        layers: &PreviewLayerStack,
        canvas_rect: Rect,
        canvas_scale: f32,
    ) -> Vec<PreviewObjectGeometry> {
        let project_w = self.editor.project.settings.width.max(1) as f32;
        let project_h = self.editor.project.settings.height.max(1) as f32;
        let preview_scale = layers.canvas_width.max(1) as f32 / project_w;
        let project_to_screen = (preview_scale * canvas_scale).max(0.0001);
        let project_center = Pos2::new(project_w * 0.5, project_h * 0.5);
        let mut geometries = Vec::new();

        for layer in layers.layers.iter() {
            let Some(clip_id) = layer.clip_id else {
                continue;
            };
            let Some(clip) = self
                .editor
                .project
                .clips
                .iter()
                .find(|clip| clip.id == clip_id)
            else {
                continue;
            };
            let half_size = Vec2::new(
                (layer.placement.scaled_w / preview_scale).max(1.0) * 0.5,
                (layer.placement.scaled_h / preview_scale).max(1.0) * 0.5,
            );
            let center =
                project_center + Vec2::new(clip.transform.position_x, clip.transform.position_y);
            let project_rect = Rect::from_center_size(center, half_size * 2.0);
            let corners_project = [
                project_rect.left_top(),
                project_rect.right_top(),
                project_rect.right_bottom(),
                project_rect.left_bottom(),
            ];
            let screen_corners = corners_project.map(|point| {
                let rotated = rotate_point(point, center, clip.transform.rotation_deg);
                preview_project_to_screen(rotated, canvas_rect, preview_scale, canvas_scale)
            });
            geometries.push(PreviewObjectGeometry {
                clip_id,
                project_rect,
                screen_corners,
                screen_center: preview_project_to_screen(
                    center,
                    canvas_rect,
                    preview_scale,
                    canvas_scale,
                ),
                project_to_screen,
            });
        }

        geometries
    }

    fn paint_preview_keyframe_reference_ghosts(
        &mut self,
        ui: &mut Ui,
        painter: &egui::Painter,
        canvas_rect: Rect,
        layers: &PreviewLayerStack,
        canvas_scale: f32,
    ) -> Vec<PreviewObjectGeometry> {
        let active_clip_ids: HashSet<Uuid> = layers
            .layers
            .iter()
            .filter_map(|layer| layer.clip_id)
            .collect();
        let fps = self.editor.project.settings.fps.max(1.0);
        let current_frame = timeline_floor_frame(self.editor.current_time, fps);
        let candidates: Vec<(Clip, Asset)> = self
            .editor
            .project
            .clips
            .iter()
            .filter(|clip| self.editor.selection.clip_ids.contains(&clip.id))
            .filter(|clip| !active_clip_ids.contains(&clip.id))
            .filter(|clip| timeline_floor_frame(clip.start_time, fps) != current_frame)
            .filter(|clip| {
                self.editor
                    .project
                    .find_track(clip.track_id)
                    .is_some_and(|track| !track.muted)
            })
            .filter_map(|clip| {
                let asset = self.editor.project.find_asset(clip.asset_id)?;
                if !clip_is_keyframe_image(clip, Some(asset)) {
                    return None;
                }
                Some((clip.clone(), asset.clone()))
            })
            .collect();
        let mut geometries = Vec::new();

        for (clip, asset) in candidates {
            let Some((texture_id, fallback_size)) = self.asset_thumbnail(ui.ctx(), &asset) else {
                continue;
            };
            let source_size = self
                .asset_source_dimensions(&asset)
                .unwrap_or(fallback_size)
                .max(Vec2::splat(1.0));
            let geometry = preview_geometry_for_clip(
                &self.editor.project,
                &clip,
                source_size,
                canvas_rect,
                layers,
                canvas_scale,
            );
            let screen_rect = Rect::from_center_size(
                geometry.screen_center,
                Vec2::new(
                    source_size.x
                        * preview_project_scale(layers, self.editor.project.settings.width)
                        * canvas_scale
                        * clip.transform.scale_x.max(0.01),
                    source_size.y
                        * preview_project_scale(layers, self.editor.project.settings.width)
                        * canvas_scale
                        * clip.transform.scale_y.max(0.01),
                ),
            );
            let tint = Color32::from_white_alpha(82);
            if clip.transform.rotation_deg.abs() <= 0.01 {
                painter.image(
                    texture_id,
                    screen_rect,
                    Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                    tint,
                );
            } else {
                paint_rotated_texture(
                    painter,
                    texture_id,
                    screen_rect,
                    clip.transform.rotation_deg,
                    tint,
                );
            }
            for index in 0..4 {
                painter.line_segment(
                    [
                        geometry.screen_corners[index],
                        geometry.screen_corners[(index + 1) % 4],
                    ],
                    Stroke::new(1.0, kit::BORDER_FOCUS.gamma_multiply(0.42)),
                );
            }
            geometries.push(geometry);
        }

        geometries
    }

    fn paint_preview_transform_overlay(
        &mut self,
        ui: &mut Ui,
        rect: Rect,
        canvas_rect: Rect,
        layers: &PreviewLayerStack,
        objects: &[PreviewObjectGeometry],
    ) {
        let painter = ui.painter().with_clip_rect(rect);
        for guide in self.preview_snap_guides.iter() {
            painter.line_segment(
                [guide.start, guide.end],
                Stroke::new(1.0, Color32::from_rgb(229, 187, 47)),
            );
        }

        let Some(selected_clip_id) = self.editor.selection.primary_clip() else {
            if !matches!(self.preview_drag, Some(PreviewTransformDrag::Pan { .. })) {
                self.preview_drag = None;
                self.preview_snap_guides.clear();
            }
            return;
        };
        let Some(selected) = objects
            .iter()
            .find(|object| object.clip_id == selected_clip_id)
            .cloned()
        else {
            if !matches!(self.preview_drag, Some(PreviewTransformDrag::Pan { .. })) {
                self.preview_drag = None;
                self.preview_snap_guides.clear();
            }
            return;
        };

        self.apply_preview_transform_drag(ui, canvas_rect, layers, objects, &selected);

        let stroke = Stroke::new(1.0, kit::BORDER_FOCUS);
        for index in 0..4 {
            painter.line_segment(
                [
                    selected.screen_corners[index],
                    selected.screen_corners[(index + 1) % 4],
                ],
                stroke,
            );
        }

        let body_rect = rect_from_points(&selected.screen_corners).expand(4.0);
        let body_response = ui.interact(
            body_rect,
            ui.id().with(("preview-transform-body", selected.clip_id)),
            Sense::click_and_drag(),
        );
        if body_response.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
        }
        if body_response.drag_started() {
            if let Some(pointer) = body_response.interact_pointer_pos() {
                if let Some(transform) = self.clip_transform(selected.clip_id) {
                    let project_point = preview_screen_to_project(
                        pointer,
                        canvas_rect,
                        layers,
                        self.editor.project.settings.width,
                    );
                    self.preview_auto_fit = false;
                    self.preview_drag = Some(PreviewTransformDrag::Move {
                        clip_id: selected.clip_id,
                        start_transform: transform,
                        start_pointer_project: project_point,
                        start_half_size: selected.project_rect.size() * 0.5,
                    });
                }
            }
        }

        for (handle, point) in preview_scale_handle_points(&selected) {
            let handle_rect = Rect::from_center_size(point, Vec2::splat(PREVIEW_HANDLE_SIZE));
            let response = ui.interact(
                handle_rect,
                ui.id()
                    .with(("preview-scale-handle", selected.clip_id, handle as u8)),
                Sense::click_and_drag(),
            );
            painter.rect_filled(handle_rect, 2.0, kit::FIELD_BG);
            painter.rect_stroke(
                handle_rect,
                2.0,
                Stroke::new(
                    1.0,
                    if response.hovered() {
                        kit::TEXT
                    } else {
                        kit::BORDER_FOCUS
                    },
                ),
                egui::StrokeKind::Inside,
            );
            if response.hovered() {
                ui.ctx().set_cursor_icon(preview_scale_cursor(handle));
            }
            if response.drag_started() {
                if let Some(transform) = self.clip_transform(selected.clip_id) {
                    self.preview_auto_fit = false;
                    self.preview_drag = Some(PreviewTransformDrag::Scale {
                        clip_id: selected.clip_id,
                        handle,
                        start_transform: transform,
                        start_center_project: selected.project_rect.center(),
                        start_half_size: selected.project_rect.size() * 0.5,
                    });
                }
            }
        }

        let rotate_point = preview_rotate_handle_point(&selected);
        painter.line_segment(
            [selected.screen_center, rotate_point],
            Stroke::new(1.0, kit::BORDER_FOCUS.gamma_multiply(0.65)),
        );
        let rotate_rect =
            Rect::from_center_size(rotate_point, Vec2::splat(PREVIEW_HANDLE_SIZE + 2.0));
        let rotate_response = ui.interact(
            rotate_rect,
            ui.id().with(("preview-rotate-handle", selected.clip_id)),
            Sense::click_and_drag(),
        );
        painter.circle_filled(
            rotate_point,
            (PREVIEW_HANDLE_SIZE + 1.0) * 0.5,
            kit::FIELD_BG,
        );
        painter.circle_stroke(
            rotate_point,
            (PREVIEW_HANDLE_SIZE + 1.0) * 0.5,
            Stroke::new(
                1.0,
                if rotate_response.hovered() {
                    kit::TEXT
                } else {
                    kit::BORDER_FOCUS
                },
            ),
        );
        if rotate_response.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
        }
        if rotate_response.drag_started() {
            if let Some(pointer) = rotate_response.interact_pointer_pos() {
                if let Some(transform) = self.clip_transform(selected.clip_id) {
                    let project_point = preview_screen_to_project(
                        pointer,
                        canvas_rect,
                        layers,
                        self.editor.project.settings.width,
                    );
                    let center = selected.project_rect.center();
                    self.preview_auto_fit = false;
                    self.preview_drag = Some(PreviewTransformDrag::Rotate {
                        clip_id: selected.clip_id,
                        start_transform: transform,
                        start_center_project: center,
                        start_pointer_angle: vector_angle_deg(project_point - center),
                    });
                }
            }
        }
    }

    fn apply_preview_transform_drag(
        &mut self,
        ui: &mut Ui,
        canvas_rect: Rect,
        layers: &PreviewLayerStack,
        objects: &[PreviewObjectGeometry],
        selected: &PreviewObjectGeometry,
    ) {
        let primary_down = ui.input(|input| input.pointer.primary_down());
        if !primary_down {
            if !matches!(self.preview_drag, Some(PreviewTransformDrag::Pan { .. })) {
                self.preview_drag = None;
                self.preview_snap_guides.clear();
            }
            return;
        }

        let Some(pointer) = ui.ctx().pointer_interact_pos() else {
            return;
        };
        let pointer_project = preview_screen_to_project(
            pointer,
            canvas_rect,
            layers,
            self.editor.project.settings.width,
        );
        let alt_down = ui.input(|input| input.modifiers.alt);
        let shift_down = ui.input(|input| input.modifiers.shift);
        let Some(drag) = self.preview_drag else {
            return;
        };

        let mut next_transform = match drag {
            PreviewTransformDrag::Move {
                clip_id,
                start_transform,
                start_pointer_project,
                start_half_size,
            } if clip_id == selected.clip_id => {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                let delta = pointer_project - start_pointer_project;
                let mut transform = start_transform;
                transform.position_x = start_transform.position_x + delta.x;
                transform.position_y = start_transform.position_y + delta.y;
                if !alt_down {
                    let snapped = self.snap_preview_transform_position(
                        clip_id,
                        transform,
                        start_half_size,
                        objects,
                        canvas_rect,
                        layers,
                        selected.project_to_screen,
                        selected.project_to_screen
                            / preview_project_scale(layers, self.editor.project.settings.width)
                                .max(0.0001),
                    );
                    transform = snapped.0;
                    self.preview_snap_guides = snapped.1;
                } else {
                    self.preview_snap_guides.clear();
                }
                transform
            }
            PreviewTransformDrag::Scale {
                clip_id,
                handle,
                start_transform,
                start_center_project,
                start_half_size,
            } if clip_id == selected.clip_id => {
                let constrain_aspect = !shift_down;
                let (scale_pointer, snap_axis, guides) = if alt_down {
                    (pointer_project, None, Vec::new())
                } else {
                    self.snap_preview_scale_pointer(
                        clip_id,
                        pointer_project,
                        handle,
                        objects,
                        canvas_rect,
                        layers,
                        selected.project_to_screen,
                        selected.project_to_screen
                            / preview_project_scale(layers, self.editor.project.settings.width)
                                .max(0.0001),
                    )
                };
                let transform = preview_scaled_transform(
                    start_transform,
                    start_center_project,
                    scale_pointer,
                    handle,
                    start_half_size,
                    constrain_aspect,
                    snap_axis,
                    selected.project_to_screen,
                );
                self.preview_snap_guides = guides;
                transform
            }
            PreviewTransformDrag::Rotate {
                clip_id,
                start_transform,
                start_center_project,
                start_pointer_angle,
            } if clip_id == selected.clip_id => {
                let current_angle = vector_angle_deg(pointer_project - start_center_project);
                let mut rotation =
                    start_transform.rotation_deg + current_angle - start_pointer_angle;
                if !shift_down {
                    rotation = (rotation / 15.0).round() * 15.0;
                }
                let mut transform = start_transform;
                transform.rotation_deg = rotation;
                self.preview_snap_guides.clear();
                transform
            }
            _ => return,
        };

        next_transform.scale_x = next_transform.scale_x.clamp(0.01, 100.0);
        next_transform.scale_y = next_transform.scale_y.clamp(0.01, 100.0);
        next_transform.opacity = next_transform.opacity.clamp(0.0, 1.0);
        if self
            .editor
            .project
            .set_clip_transform(selected.clip_id, next_transform)
        {
            self.editor.preview_dirty = true;
            ui.ctx().request_repaint();
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn snap_preview_transform_position(
        &self,
        clip_id: Uuid,
        transform: ClipTransform,
        half_size: Vec2,
        objects: &[PreviewObjectGeometry],
        canvas_rect: Rect,
        layers: &PreviewLayerStack,
        project_to_screen: f32,
        canvas_scale: f32,
    ) -> (ClipTransform, Vec<PreviewSnapGuide>) {
        let project_w = self.editor.project.settings.width.max(1) as f32;
        let project_h = self.editor.project.settings.height.max(1) as f32;
        let project_center = Pos2::new(project_w * 0.5, project_h * 0.5);
        let center = project_center + Vec2::new(transform.position_x, transform.position_y);
        let rect = Rect::from_center_size(center, half_size * 2.0);
        let threshold = (PREVIEW_SNAP_THRESHOLD_PX / project_to_screen.max(0.0001)).max(0.5);

        let mut x_targets = vec![0.0, project_w * 0.5, project_w];
        let mut y_targets = vec![0.0, project_h * 0.5, project_h];
        for object in objects.iter().filter(|object| object.clip_id != clip_id) {
            x_targets.extend([
                object.project_rect.left(),
                object.project_rect.center().x,
                object.project_rect.right(),
            ]);
            y_targets.extend([
                object.project_rect.top(),
                object.project_rect.center().y,
                object.project_rect.bottom(),
            ]);
        }

        let mut transform = transform;
        let mut guides = Vec::new();
        let preview_scale = preview_project_scale(layers, self.editor.project.settings.width);
        if let Some((delta, target)) = nearest_snap_delta(
            [rect.left(), rect.center().x, rect.right()],
            &x_targets,
            threshold,
        ) {
            transform.position_x += delta;
            let p0 = preview_project_to_screen(
                Pos2::new(target, 0.0),
                canvas_rect,
                preview_scale,
                canvas_scale,
            );
            let p1 = preview_project_to_screen(
                Pos2::new(target, project_h),
                canvas_rect,
                preview_scale,
                canvas_scale,
            );
            guides.push(PreviewSnapGuide { start: p0, end: p1 });
        }
        if let Some((delta, target)) = nearest_snap_delta(
            [rect.top(), rect.center().y, rect.bottom()],
            &y_targets,
            threshold,
        ) {
            transform.position_y += delta;
            let p0 = preview_project_to_screen(
                Pos2::new(0.0, target),
                canvas_rect,
                preview_scale,
                canvas_scale,
            );
            let p1 = preview_project_to_screen(
                Pos2::new(project_w, target),
                canvas_rect,
                preview_scale,
                canvas_scale,
            );
            guides.push(PreviewSnapGuide { start: p0, end: p1 });
        }

        (transform, guides)
    }

    #[allow(clippy::too_many_arguments)]
    fn snap_preview_scale_pointer(
        &self,
        clip_id: Uuid,
        pointer_project: Pos2,
        handle: PreviewScaleHandle,
        objects: &[PreviewObjectGeometry],
        canvas_rect: Rect,
        layers: &PreviewLayerStack,
        project_to_screen: f32,
        canvas_scale: f32,
    ) -> (Pos2, Option<PreviewScaleSnapAxis>, Vec<PreviewSnapGuide>) {
        let (sx, sy) = preview_scale_handle_signs(handle);
        let project_w = self.editor.project.settings.width.max(1) as f32;
        let project_h = self.editor.project.settings.height.max(1) as f32;
        let threshold = (PREVIEW_SNAP_THRESHOLD_PX / project_to_screen.max(0.0001)).max(0.5);
        let preview_scale = preview_project_scale(layers, self.editor.project.settings.width);

        let mut x_targets = vec![0.0, project_w * 0.5, project_w];
        let mut y_targets = vec![0.0, project_h * 0.5, project_h];
        for object in objects.iter().filter(|object| object.clip_id != clip_id) {
            x_targets.extend([
                object.project_rect.left(),
                object.project_rect.center().x,
                object.project_rect.right(),
            ]);
            y_targets.extend([
                object.project_rect.top(),
                object.project_rect.center().y,
                object.project_rect.bottom(),
            ]);
        }

        let x_snap = if sx != 0.0 {
            nearest_snap_delta([pointer_project.x], &x_targets, threshold).map(|(delta, target)| {
                let p0 = preview_project_to_screen(
                    Pos2::new(target, 0.0),
                    canvas_rect,
                    preview_scale,
                    canvas_scale,
                );
                let p1 = preview_project_to_screen(
                    Pos2::new(target, project_h),
                    canvas_rect,
                    preview_scale,
                    canvas_scale,
                );
                (
                    delta,
                    PreviewSnapGuide { start: p0, end: p1 },
                    PreviewScaleSnapAxis::X,
                )
            })
        } else {
            None
        };
        let y_snap = if sy != 0.0 {
            nearest_snap_delta([pointer_project.y], &y_targets, threshold).map(|(delta, target)| {
                let p0 = preview_project_to_screen(
                    Pos2::new(0.0, target),
                    canvas_rect,
                    preview_scale,
                    canvas_scale,
                );
                let p1 = preview_project_to_screen(
                    Pos2::new(project_w, target),
                    canvas_rect,
                    preview_scale,
                    canvas_scale,
                );
                (
                    delta,
                    PreviewSnapGuide { start: p0, end: p1 },
                    PreviewScaleSnapAxis::Y,
                )
            })
        } else {
            None
        };

        let selected_snap = match (x_snap, y_snap) {
            (Some(x), Some(y)) if x.0.abs() <= y.0.abs() => Some(x),
            (Some(_), Some(y)) => Some(y),
            (Some(x), None) => Some(x),
            (None, Some(y)) => Some(y),
            (None, None) => None,
        };

        let mut pointer = pointer_project;
        let mut guides = Vec::new();
        if let Some((delta, guide, axis)) = selected_snap {
            match axis {
                PreviewScaleSnapAxis::X => pointer.x += delta,
                PreviewScaleSnapAxis::Y => pointer.y += delta,
            }
            guides.push(guide);
            (pointer, Some(axis), guides)
        } else {
            (pointer, None, guides)
        }
    }

    fn clip_transform(&self, clip_id: Uuid) -> Option<ClipTransform> {
        self.editor
            .project
            .clips
            .iter()
            .find(|clip| clip.id == clip_id)
            .map(|clip| clip.transform)
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
                timeline_track_at_pos(pos, rects, &tracks)
                    .filter(|track| {
                        self.editor
                            .project
                            .asset_compatible_with_track(asset_id, track.id)
                    })
                    .map(|track| track.id)
            })
        });

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
            if drop_target_track_id == Some(track.id) {
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
                timeline_track_at_pos(pos, rects, &tracks)
                    .filter(|track| track.track_type == TrackType::Marker)
                    .map(|track| track.id)
            });

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
            let can_space = selected_clips.len() >= 2;
            if automation_button(
                ui.add_enabled(can_space, egui::Button::new("Space Selected Clips")),
                "Space Selected Clips",
            )
            .clicked()
            {
                self.space_selected_clips(&selected_clips);
                ui.close();
            }
            let image_count = selected_clips
                .iter()
                .filter(|clip| {
                    self.editor
                        .project
                        .find_asset(clip.asset_id)
                        .is_some_and(|asset| asset.is_image())
                })
                .count();
            if automation_button(
                ui.add_enabled(
                    image_count >= 2,
                    egui::Button::new("Generate Between Keyframes"),
                ),
                "Generate Between Keyframes",
            )
            .clicked()
            {
                let mut sorted = selected_clips.clone();
                sorted.sort_by(|a, b| {
                    a.start_time
                        .partial_cmp(&b.start_time)
                        .unwrap_or(std::cmp::Ordering::Equal)
                });
                self.request_bridge_video_from_selected_clips(&sorted);
                ui.close();
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

    fn asset_delete_confirmation_modal(&mut self, ctx: &Context) {
        let Some(confirmation) = self.asset_delete_confirmation.clone() else {
            return;
        };

        let mut open = true;
        let mut close_clicked = false;
        let mut cancel_clicked = false;
        let mut delete_clicked = false;
        let outside_clicked = kit::dismissible_modal_scrim(ctx, "asset_delete", true);
        let size = modal_size(ctx, ASSET_DELETE_MODAL_SIZE, [380.0, 260.0]);
        egui::Window::new("Delete Assets")
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
                    "Delete Assets?",
                    Some("Remove selected project assets and dependent timeline clips."),
                    true,
                );
                kit::modal_body(ui, |ui| {
                    kit::body_with_footer(
                        ui,
                        120.0,
                        kit::SECONDARY_BUTTON_H,
                        |ui| {
                            let asset_word = if confirmation.asset_count == 1 {
                                "asset"
                            } else {
                                "assets"
                            };
                            let clip_context = match confirmation.clip_count {
                                0 => "No timeline clips reference this selection.".to_string(),
                                1 => "1 timeline clip references this selection.".to_string(),
                                count => {
                                    format!("{count} timeline clips reference this selection.")
                                }
                            };
                            ui.label(
                                RichText::new(format!(
                                    "You are about to delete {} {}.",
                                    confirmation.asset_count, asset_word
                                ))
                                .color(kit::TEXT)
                                .strong(),
                            );
                            ui.add_space(kit::FORM_ROW_GAP);
                            ui.add(
                                egui::Label::new(
                                    RichText::new(clip_context).color(kit::TEXT_MUTED),
                                )
                                .wrap(),
                            );
                            ui.add(
                                egui::Label::new(
                                    RichText::new(
                                        "Timeline clip instances will be removed. Source media files on disk are left in place.",
                                    )
                                    .color(kit::TEXT_MUTED),
                                )
                                .wrap(),
                            );
                            if !confirmation.sample_names.is_empty() {
                                ui.add_space(kit::ACTION_GAP);
                                kit::field_label(ui, "Assets");
                                ui.add_space(kit::FORM_ROW_GAP);
                                for name in confirmation.sample_names.iter() {
                                    ui.label(RichText::new(name).color(kit::TEXT));
                                }
                                let remaining =
                                    confirmation.asset_count.saturating_sub(confirmation.sample_names.len());
                                if remaining > 0 {
                                    ui.label(
                                        RichText::new(format!("+ {remaining} more"))
                                            .color(kit::TEXT_MUTED),
                                    );
                                }
                            }
                        },
                        |ui| {
                            kit::equal_width_action_row(
                                ui,
                                2,
                                kit::SECONDARY_BUTTON_H,
                                kit::ACTION_GAP,
                                |ui, index, button_w| match index {
                                    0 => {
                                        cancel_clicked =
                                            kit::secondary_button(ui, "Cancel", button_w)
                                                .clicked();
                                    }
                                    _ => {
                                        delete_clicked =
                                            kit::danger_button(ui, "Delete Assets", button_w)
                                                .clicked();
                                    }
                                },
                            );
                        },
                    );
                });
            });

        if delete_clicked {
            let asset_ids = confirmation.asset_ids.clone();
            self.asset_delete_confirmation = None;
            self.perform_delete_assets(&asset_ids);
        } else if cancel_clicked || close_clicked || outside_clicked || !open {
            self.asset_delete_confirmation = None;
        }
    }

    fn track_delete_confirmation_modal(&mut self, ctx: &Context) {
        let Some(confirmation) = self.track_delete_confirmation.clone() else {
            return;
        };

        let mut open = true;
        let mut close_clicked = false;
        let mut cancel_clicked = false;
        let mut delete_clicked = false;
        let outside_clicked = kit::dismissible_modal_scrim(ctx, "track_delete", true);
        let size = modal_size(ctx, TRACK_DELETE_MODAL_SIZE, [380.0, 260.0]);
        egui::Window::new("Delete Tracks")
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
                    "Delete Tracks?",
                    Some("Remove selected timeline tracks and their contents."),
                    true,
                );
                kit::modal_body(ui, |ui| {
                    kit::body_with_footer(
                        ui,
                        110.0,
                        kit::SECONDARY_BUTTON_H,
                        |ui| {
                            let track_word = if confirmation.track_count == 1 {
                                "track"
                            } else {
                                "tracks"
                            };
                            ui.label(
                                RichText::new(format!(
                                    "You are about to delete {} {}.",
                                    confirmation.track_count, track_word
                                ))
                                .color(kit::TEXT)
                                .strong(),
                            );
                            ui.add_space(kit::FORM_ROW_GAP);
                            let clip_context = match confirmation.clip_count {
                                0 => "No clips will be removed.".to_string(),
                                1 => "1 clip will be removed.".to_string(),
                                count => format!("{count} clips will be removed."),
                            };
                            let marker_context = match confirmation.marker_count {
                                0 => "No markers will be removed.".to_string(),
                                1 => "1 marker will be removed.".to_string(),
                                count => format!("{count} markers will be removed."),
                            };
                            ui.label(RichText::new(clip_context).color(kit::TEXT_MUTED));
                            ui.label(RichText::new(marker_context).color(kit::TEXT_MUTED));
                            if !confirmation.sample_names.is_empty() {
                                ui.add_space(kit::ACTION_GAP);
                                kit::field_label(ui, "Tracks");
                                ui.add_space(kit::FORM_ROW_GAP);
                                for name in confirmation.sample_names.iter() {
                                    ui.label(RichText::new(name).color(kit::TEXT));
                                }
                                let remaining = confirmation
                                    .track_count
                                    .saturating_sub(confirmation.sample_names.len());
                                if remaining > 0 {
                                    ui.label(
                                        RichText::new(format!("+ {remaining} more"))
                                            .color(kit::TEXT_MUTED),
                                    );
                                }
                            }
                        },
                        |ui| {
                            kit::equal_width_action_row(
                                ui,
                                2,
                                kit::SECONDARY_BUTTON_H,
                                kit::ACTION_GAP,
                                |ui, index, button_w| match index {
                                    0 => {
                                        cancel_clicked =
                                            kit::secondary_button(ui, "Cancel", button_w).clicked();
                                    }
                                    _ => {
                                        delete_clicked =
                                            kit::danger_button(ui, "Delete Tracks", button_w)
                                                .clicked();
                                    }
                                },
                            );
                        },
                    );
                });
            });

        if delete_clicked {
            let track_ids = confirmation.track_ids.clone();
            self.track_delete_confirmation = None;
            self.perform_delete_tracks(&track_ids);
        } else if cancel_clicked || close_clicked || outside_clicked || !open {
            self.track_delete_confirmation = None;
        }
    }

    fn bridge_keyframe_confirmation_modal(&mut self, ctx: &Context) {
        let Some(confirmation) = self.bridge_keyframe_confirmation.clone() else {
            return;
        };

        let mut open = true;
        let mut close_clicked = false;
        let mut cancel_clicked = false;
        let mut create_clicked = false;
        let outside_clicked = kit::dismissible_modal_scrim(ctx, "bridge_keyframes", true);
        let size = modal_size(ctx, BRIDGE_KEYFRAME_MODAL_SIZE, [400.0, 280.0]);
        egui::Window::new("Convert Keyframes")
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
                    "Use Images as Keyframes?",
                    Some("The bridge will anchor to image start times."),
                    true,
                );
                kit::modal_body(ui, |ui| {
                    kit::body_with_footer(
                        ui,
                        130.0,
                        kit::SECONDARY_BUTTON_H,
                        |ui| {
                            ui.label(
                                RichText::new(format!(
                                    "{} referenced image clips will switch to keyframe display mode.",
                                    confirmation.convert_clip_ids.len()
                                ))
                                .color(kit::TEXT)
                                .strong(),
                            );
                            ui.add_space(kit::FORM_ROW_GAP);
                            ui.label(
                                RichText::new(
                                    "Keyframe image clips keep their asset and timing data, but draw as marker-like pins on the timeline so the generated video span reads from keyframe to keyframe.",
                                )
                                .color(kit::TEXT_MUTED),
                            );
                            ui.label(
                                RichText::new(
                                    "Normal still-image behavior remains available per clip in the Attributes panel.",
                                )
                                .color(kit::TEXT_MUTED),
                            );
                            if !confirmation.sample_names.is_empty() {
                                ui.add_space(kit::ACTION_GAP);
                                kit::field_label(ui, "Referenced Images");
                                ui.add_space(kit::FORM_ROW_GAP);
                                for name in confirmation.sample_names.iter() {
                                    ui.label(RichText::new(name).color(kit::TEXT));
                                }
                            }
                        },
                        |ui| {
                            kit::equal_width_action_row(
                                ui,
                                2,
                                kit::SECONDARY_BUTTON_H,
                                kit::ACTION_GAP,
                                |ui, index, button_w| match index {
                                    0 => {
                                        cancel_clicked =
                                            kit::secondary_button(ui, "Cancel", button_w)
                                                .clicked();
                                    }
                                    _ => {
                                        create_clicked =
                                            kit::primary_button(ui, "Convert + Create", button_w)
                                                .clicked();
                                    }
                                },
                            );
                        },
                    );
                });
            });

        if create_clicked {
            let clip_ids = confirmation.clip_ids.clone();
            for clip_id in confirmation.convert_clip_ids.iter() {
                self.editor
                    .project
                    .set_clip_image_mode(*clip_id, ClipImageMode::Keyframe);
            }
            self.bridge_keyframe_confirmation = None;
            self.create_bridge_video_from_clip_ids(&clip_ids);
            self.editor.preview_dirty = true;
        } else if cancel_clicked || close_clicked || outside_clicked || !open {
            self.bridge_keyframe_confirmation = None;
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
        let outside_clicked = kit::dismissible_modal_scrim(ctx, "new_project", close_enabled);
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
        if close_clicked || outside_clicked || (!open && close_enabled) {
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
                    self.export_modal = ExportModalState::for_project(&self.editor.project);
                    self.export_preview_texture = None;
                    self.editor.overlays.new_project = false;
                }
                Err(err) => self.editor.status = err,
            }
        }
    }

    fn project_settings_modal(&mut self, ctx: &Context) {
        let mut open = true;
        let mut close_clicked = false;
        let outside_clicked = kit::dismissible_modal_scrim(ctx, "project_settings", true);
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
        if close_clicked || outside_clicked || !open {
            self.editor.overlays.project_settings = false;
        }
    }

    fn generative_video_modal(&mut self, ctx: &Context) {
        let mut open = true;
        let mut close_clicked = false;
        let outside_clicked = kit::dismissible_modal_scrim(ctx, "generative_video", true);
        egui::Window::new("New Generative Video")
            .title_bar(false)
            .order(egui::Order::Foreground)
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .fixed_size([480.0, 210.0])
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
                    let mut fps = self.gen_video_fps.max(1.0);
                    let mut frames = self.gen_video_frames.max(1) as i64;
                    let mut seconds = frames as f64 / fps;
                    let mut seconds_changed = false;

                    kit::field_grid_row_with_height(
                        ui,
                        &[1.0, 1.0, 1.0],
                        kit::FIELD_H,
                        kit::FORM_ROW_GAP,
                        |ui, index| match index {
                            0 => {
                                let width = ui.available_width();
                                inspector_drag_f64(ui, "FPS", &mut fps, 1.0, width);
                            }
                            1 => {
                                let width = ui.available_width();
                                inspector_drag_i64(ui, "Frames", &mut frames, 1.0, width);
                            }
                            _ => {
                                let width = ui.available_width();
                                seconds_changed =
                                    inspector_drag_f64(ui, "Seconds", &mut seconds, 0.1, width);
                            }
                        },
                    );

                    fps = fps.clamp(1.0, 240.0);
                    if seconds_changed {
                        let min_seconds = 1.0 / fps;
                        frames = (seconds.max(min_seconds) * fps).round().max(1.0) as i64;
                    }
                    self.gen_video_fps = fps;
                    self.gen_video_frames = frames.clamp(1, 1_000_000) as u32;

                    ui.add_space(kit::ACTION_GAP);
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
        if close_clicked || outside_clicked || !open {
            self.editor.overlays.generative_video = false;
        }
    }

    fn export_video_modal(&mut self, ctx: &Context) {
        let mut open = true;
        let mut close_clicked = false;
        let size = modal_size(ctx, EXPORT_MODAL_SIZE, [580.0, 500.0]);
        let outside_clicked = kit::dismissible_modal_scrim(ctx, "export_video", true);
        egui::Window::new("Export Video")
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
                    "Export Video",
                    Some("Render the current timeline to an MP4 file."),
                    true,
                );
                kit::modal_body(ui, |ui| self.export_video_modal_contents(ui));
            });

        if close_clicked || outside_clicked || !open {
            self.close_or_cancel_export_modal();
        }
    }

    fn export_video_modal_contents(&mut self, ui: &mut Ui) {
        let footer_h = kit::PRIMARY_BUTTON_H;
        let full_rect = ui.available_rect_before_wrap();
        let (rect, _) = ui.allocate_exact_size(full_rect.size(), Sense::hover());
        let footer_rect = Rect::from_min_max(
            Pos2::new(rect.left(), rect.bottom() - footer_h),
            rect.right_bottom(),
        );
        let body_rect =
            Rect::from_min_max(rect.left_top(), Pos2::new(rect.right(), footer_rect.top()));

        let mut body_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(body_rect)
                .layout(Layout::left_to_right(Align::Min)),
        );
        body_ui.shrink_clip_rect(body_rect);
        let available_w = body_ui.available_width();
        let gap = 12.0;
        let left_w = ((available_w - gap) * 0.58).max(300.0);
        let right_w = (available_w - gap - left_w).max(220.0);
        body_ui.spacing_mut().item_spacing.x = gap;
        body_ui.allocate_ui_with_layout(
            Vec2::new(left_w, body_rect.height()),
            Layout::top_down(Align::Min),
            |ui| {
                ui.set_width(left_w);
                self.export_settings_card(ui);
            },
        );
        body_ui.allocate_ui_with_layout(
            Vec2::new(right_w, body_rect.height()),
            Layout::top_down(Align::Min),
            |ui| {
                ui.set_width(right_w);
                self.export_progress_card(ui);
            },
        );

        let mut footer_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(footer_rect)
                .layout(Layout::right_to_left(Align::Center)),
        );
        footer_ui.shrink_clip_rect(footer_rect);
        self.export_footer(&mut footer_ui);
    }

    fn export_settings_card(&mut self, ui: &mut Ui) {
        let running = self.export_cancel.is_some();
        kit::card_panel(ui, ui.available_height(), |ui| {
            ui.add_enabled_ui(!running, |ui| {
                kit::scroll_body(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = kit::FORM_ROW_GAP;
                    kit::field_label(ui, "Output");
                    let initial_dir = self
                        .editor
                        .project
                        .project_path
                        .as_ref()
                        .map(|root| root.join("exports"))
                        .unwrap_or_else(|| default_projects_dir().join("exports"));
                    let options = kit::BrowseFileOptions::new()
                        .button_label("Browse")
                        .initial_dir(initial_dir.as_path())
                        .remember_last_dir()
                        .id_salt("export_output_file")
                        .filters(MP4_FILE_FILTERS);
                    if let Some(path) = kit::labeled_save_file_field(
                        ui,
                        "Output File",
                        &mut self.export_modal.output_path,
                        options,
                    ) {
                        self.export_modal.output_path =
                            ensure_mp4_extension(path).display().to_string();
                    }
                    ui.add_space(kit::ACTION_GAP);

                    kit::field_label(ui, "Video");
                    kit::field_grid_row(ui, &[1.0, 1.0, 1.0], |ui, index| match index {
                        0 => {
                            kit::labeled_text_field(ui, "Width", &mut self.export_modal.width);
                        }
                        1 => {
                            kit::labeled_text_field(ui, "Height", &mut self.export_modal.height);
                        }
                        _ => {
                            kit::labeled_text_field(ui, "FPS", &mut self.export_modal.fps);
                        }
                    });
                    ui.add_space(kit::FORM_ROW_GAP);
                    kit::field_grid_row(ui, &[1.0, 2.0], |ui, index| match index {
                        0 => {
                            kit::labeled_combo_field(
                                ui,
                                "Codec",
                                "export_codec",
                                self.export_modal.codec.label(),
                                |ui| {
                                    automation_selectable_value(
                                        ui,
                                        &mut self.export_modal.codec,
                                        VideoExportCodec::H264,
                                        "H.264",
                                    );
                                    automation_selectable_value(
                                        ui,
                                        &mut self.export_modal.codec,
                                        VideoExportCodec::H265,
                                        "H.265",
                                    );
                                },
                            );
                        }
                        _ => {
                            kit::labeled_combo_field(
                                ui,
                                "Perceptual Quality",
                                "export_quality",
                                self.export_modal.quality.label(),
                                |ui| {
                                    automation_selectable_value(
                                        ui,
                                        &mut self.export_modal.quality,
                                        VideoExportQuality::Compact,
                                        "Compact",
                                    );
                                    automation_selectable_value(
                                        ui,
                                        &mut self.export_modal.quality,
                                        VideoExportQuality::Balanced,
                                        "Balanced",
                                    );
                                    automation_selectable_value(
                                        ui,
                                        &mut self.export_modal.quality,
                                        VideoExportQuality::High,
                                        "High Quality",
                                    );
                                    automation_selectable_value(
                                        ui,
                                        &mut self.export_modal.quality,
                                        VideoExportQuality::NearLossless,
                                        "Near Lossless",
                                    );
                                },
                            );
                        }
                    });
                    ui.add_space(kit::FORM_ROW_GAP);
                    kit::field_grid_row(ui, &[1.0], |ui, _index| {
                        kit::labeled_combo_field(
                            ui,
                            "Intermediate Format",
                            "export_frame_format",
                            self.export_modal.frame_format.label(),
                            |ui| {
                                automation_selectable_value(
                                    ui,
                                    &mut self.export_modal.frame_format,
                                    VideoExportFrameFormat::Png,
                                    "PNG",
                                );
                                automation_selectable_value(
                                    ui,
                                    &mut self.export_modal.frame_format,
                                    VideoExportFrameFormat::Bmp,
                                    "BMP (Fast)",
                                );
                            },
                        );
                    });
                    ui.add_space(kit::ACTION_GAP);

                    kit::field_label(ui, "Range");
                    kit::field_grid_row(ui, &[1.0, 1.0], |ui, index| match index {
                        0 => {
                            kit::labeled_text_field(
                                ui,
                                "Start Seconds",
                                &mut self.export_modal.start_seconds,
                            );
                        }
                        _ => {
                            kit::labeled_text_field(
                                ui,
                                "Duration Seconds",
                                &mut self.export_modal.duration_seconds,
                            );
                        }
                    });
                    ui.add_space(kit::FORM_ROW_GAP);
                    automation_checkbox(ui, &mut self.export_modal.include_audio, "Include audio");
                    ui.add_space(kit::ACTION_GAP);

                    kit::field_label(ui, "Burn In");
                    automation_checkbox(
                        ui,
                        &mut self.export_modal.timestamp_overlay_enabled,
                        "Timestamp overlay",
                    );
                    ui.add_enabled_ui(self.export_modal.timestamp_overlay_enabled, |ui| {
                        ui.add_space(kit::FORM_ROW_GAP);
                        kit::labeled_combo_field(
                            ui,
                            "Timestamp Position",
                            "export_timestamp_position",
                            self.export_modal.timestamp_overlay_position.label(),
                            |ui| {
                                automation_selectable_value(
                                    ui,
                                    &mut self.export_modal.timestamp_overlay_position,
                                    TimestampOverlayPosition::TopCenter,
                                    "Top Center",
                                );
                                automation_selectable_value(
                                    ui,
                                    &mut self.export_modal.timestamp_overlay_position,
                                    TimestampOverlayPosition::BottomCenter,
                                    "Bottom Center",
                                );
                            },
                        );
                    });
                });
            });
        });
    }

    fn export_progress_card(&mut self, ui: &mut Ui) {
        kit::card_frame().show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = kit::FORM_ROW_GAP;
            kit::field_label(ui, "Progress");
            ui.label(kit::body(&self.export_modal.message));
            ui.add(
                egui::ProgressBar::new(self.export_modal.progress.clamp(0.0, 1.0))
                    .show_percentage()
                    .animate(self.export_cancel.is_some())
                    .desired_width(ui.available_width()),
            );
            if !self.export_modal.frame_label.is_empty() {
                ui.label(kit::caption(&self.export_modal.frame_label));
            } else {
                ui.label(kit::caption(&self.export_modal.stage));
            }
            ui.add_space(kit::FORM_ROW_GAP);
            self.export_preview(ui);
            if let Some(error) = &self.export_modal.error {
                ui.add_space(kit::FORM_ROW_GAP);
                ui.label(RichText::new(error).color(kit::DANGER).size(11.0));
            }
            if let Some(summary) = &self.export_modal.summary {
                ui.add_space(kit::FORM_ROW_GAP);
                ui.label(kit::caption(format!(
                    "{} {}, {}, {} frames, {:.2}s{}",
                    summary.codec.label(),
                    self.export_modal.quality.label(),
                    summary.frame_format.label(),
                    summary.frame_count,
                    summary.duration_seconds,
                    if summary.audio_included {
                        ", audio"
                    } else {
                        ""
                    }
                )));
                ui.label(kit::caption(path_label(&summary.output_path)));
            }
            if !self.export_modal.warnings.is_empty() {
                ui.add_space(kit::FORM_ROW_GAP);
                ui.label(kit::caption(format!(
                    "{} warning{}",
                    self.export_modal.warnings.len(),
                    if self.export_modal.warnings.len() == 1 {
                        ""
                    } else {
                        "s"
                    }
                )));
            }
        });
    }

    fn export_preview(&mut self, ui: &mut Ui) {
        let size = Vec2::new(ui.available_width(), 150.0);
        let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
        ui.painter()
            .rect_filled(rect, kit::field_radius(), kit::FIELD_BG);
        ui.painter().rect_stroke(
            rect,
            kit::field_radius(),
            Stroke::new(1.0, kit::BORDER_SOFT),
            egui::StrokeKind::Inside,
        );
        let Some(texture) = &self.export_preview_texture else {
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "Preview appears during export",
                FontId::proportional(11.0),
                kit::TEXT_DIM,
            );
            return;
        };
        let texture_size = texture.size_vec2();
        let scale = (rect.width() / texture_size.x.max(1.0))
            .min(rect.height() / texture_size.y.max(1.0))
            .min(1.0);
        let image_size = texture_size * scale;
        let image_rect = Rect::from_center_size(rect.center(), image_size);
        ui.painter().image(
            texture.id(),
            image_rect,
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
            Color32::WHITE,
        );
    }

    fn export_footer(&mut self, ui: &mut Ui) {
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if self.export_cancel.is_some() {
                if kit::danger_button(ui, "Cancel Export", 150.0).clicked() {
                    self.close_or_cancel_export_modal();
                }
                return;
            }
            if matches!(self.export_modal.status, ExportRunStatus::Finished) {
                if kit::primary_button(ui, "Close", 120.0).clicked() {
                    self.editor.overlays.export_video = false;
                }
                if kit::secondary_button(ui, "Export Again", 130.0).clicked() {
                    self.start_export_video();
                }
                return;
            }
            if kit::primary_button(ui, "Export Video", 150.0).clicked() {
                self.start_export_video();
            }
            if kit::secondary_button(ui, "Cancel", 110.0).clicked() {
                self.editor.overlays.export_video = false;
            }
        });
    }

    fn start_export_video(&mut self) {
        let settings = match self.export_modal.to_settings() {
            Ok(settings) => settings,
            Err(err) => {
                self.export_modal.status = ExportRunStatus::Failed;
                self.export_modal.error = Some(err);
                self.export_modal.message = "Export settings need attention.".to_string();
                self.export_modal.progress = 0.0;
                return;
            }
        };
        let project = self.editor.project.clone();
        let job = VideoExportJob { project, settings };
        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_for_thread = Arc::clone(&cancel);
        let events = self.export_events_tx.clone();

        self.export_modal.status = ExportRunStatus::Running;
        self.export_modal.progress = 0.0;
        self.export_modal.stage = "preparing".to_string();
        self.export_modal.message = "Preparing export".to_string();
        self.export_modal.frame_label.clear();
        self.export_modal.error = None;
        self.export_modal.summary = None;
        self.export_modal.warnings.clear();
        self.export_preview_texture = None;
        self.export_cancel = Some(cancel);
        self.editor.status = "Export started".to_string();

        std::thread::spawn(move || {
            export_video(job, cancel_for_thread, |event| {
                let _ = events.send(event);
            });
        });
    }

    fn service_export_events(&mut self, ctx: &Context) {
        while let Ok(event) = self.export_events_rx.try_recv() {
            match event {
                VideoExportEvent::Progress {
                    stage,
                    message,
                    progress,
                    frame_index,
                    total_frames,
                    preview,
                } => {
                    self.export_modal.stage = stage.to_string();
                    self.export_modal.message = message;
                    self.export_modal.progress = progress.clamp(0.0, 1.0);
                    self.export_modal.frame_label = match (frame_index, total_frames) {
                        (Some(frame), Some(total)) => format!("Frame {frame} of {total}"),
                        _ => self.export_modal.stage.clone(),
                    };
                    if let Some(preview) = preview {
                        self.update_export_preview_texture(ctx, preview);
                    }
                }
                VideoExportEvent::Finished(summary) => {
                    self.export_cancel = None;
                    self.export_modal.status = ExportRunStatus::Finished;
                    self.export_modal.progress = 1.0;
                    self.export_modal.stage = "complete".to_string();
                    self.export_modal.message = "Export complete".to_string();
                    self.export_modal.frame_label.clear();
                    self.export_modal.warnings = summary.warnings.clone();
                    self.editor.status = format!("Exported {}", path_label(&summary.output_path));
                    self.export_modal.summary = Some(summary);
                }
                VideoExportEvent::Cancelled => {
                    self.export_cancel = None;
                    self.export_modal.status = ExportRunStatus::Cancelled;
                    self.export_modal.stage = "cancelled".to_string();
                    self.export_modal.message = "Export cancelled".to_string();
                    self.export_modal.error = None;
                    self.editor.status = "Export cancelled".to_string();
                }
                VideoExportEvent::Failed(err) => {
                    self.export_cancel = None;
                    self.export_modal.status = ExportRunStatus::Failed;
                    self.export_modal.stage = "failed".to_string();
                    self.export_modal.message = "Export failed".to_string();
                    self.export_modal.error = Some(err.clone());
                    self.editor.status = format!("Export failed: {err}");
                }
            }
        }

        if self.export_cancel.is_some() {
            ctx.request_repaint_after(Duration::from_millis(120));
        }
    }

    fn update_export_preview_texture(&mut self, ctx: &Context, preview: VideoExportPreview) {
        if preview.width == 0 || preview.height == 0 || preview.rgba.is_empty() {
            return;
        }
        let image = ColorImage::from_rgba_unmultiplied(
            [preview.width as usize, preview.height as usize],
            &preview.rgba,
        );
        if let Some(texture) = self.export_preview_texture.as_mut() {
            texture.set(image, TextureOptions::LINEAR);
        } else {
            self.export_preview_texture =
                Some(ctx.load_texture("export-preview", image, TextureOptions::LINEAR));
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
                tokio::sync::mpsc::unbounded_channel::<ProviderProgress>();
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
                            self.finish_generation_success(job.clone(), output);
                            if let Err(err) = self.advance_generation_seed_after_attempt(&job) {
                                self.editor.status =
                                    format!("Generated, but seed advance save failed: {err}");
                            }
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
                        let seed_save_error = job_snapshot
                            .as_ref()
                            .and_then(|job| self.advance_generation_seed_after_attempt(job).err());
                        self.editor.status = if let Some(err) = seed_save_error {
                            format!("{message} (seed advance save failed: {err})")
                        } else {
                            message
                        };
                    }
                }
            }
        }
    }

    fn advance_generation_seed_after_attempt(&mut self, job: &GenerationJob) -> Result<(), String> {
        let Some(seed_advance) = job.seed_advance.as_ref() else {
            return Ok(());
        };
        if self.editor.project.find_asset(job.asset_id).is_none() {
            return Ok(());
        }

        let next_seed_value = serde_json::Value::Number(seed_advance.next_seed.into());
        self.editor
            .project
            .update_generative_config(job.asset_id, |config| {
                config.inputs.insert(
                    seed_advance.field.clone(),
                    InputValue::Literal {
                        value: next_seed_value,
                    },
                );
            });

        self.editor
            .project
            .save_generative_config(job.asset_id)
            .map_err(|err| err.to_string())
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
        self.asset_source_dimensions.remove(&asset_id);
        self.asset_source_dimension_misses.remove(&asset_id);
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
        context_clip_id: Option<Uuid>,
        provider: ProviderEntry,
        config_snapshot: GenerativeConfig,
        folder_path: PathBuf,
        asset_label: String,
    ) -> Result<String, String> {
        if provider.output_type == ProviderOutputType::Audio {
            return Err("Audio generation is not supported in the queue yet.".to_string());
        }

        let resolved = resolve_provider_inputs(
            &self.editor.project,
            context_clip_id,
            &provider,
            &config_snapshot,
        );
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
            let (inputs, inputs_snapshot, seed_advance) =
                match (batch.seed_strategy, seed_field.as_ref()) {
                    (SeedStrategy::Keep, _) | (_, None) => {
                        (resolved.values.clone(), resolved.snapshot.clone(), None)
                    }
                    (SeedStrategy::Increment, Some(field)) => {
                        let seed = seed_base.unwrap_or(0) + index as i64;
                        let (inputs, inputs_snapshot) =
                            update_seed_inputs(&resolved.values, &resolved.snapshot, field, seed);
                        (
                            inputs,
                            inputs_snapshot,
                            Some(GenerationSeedAdvance {
                                field: field.clone(),
                                next_seed: seed.saturating_add(1),
                            }),
                        )
                    }
                    (SeedStrategy::Random, Some(field)) => {
                        let seed = random_seed_i64();
                        let (inputs, inputs_snapshot) =
                            update_seed_inputs(&resolved.values, &resolved.snapshot, field, seed);
                        (inputs, inputs_snapshot, None)
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
                clip_id: context_clip_id,
                asset_label: asset_label.clone(),
                folder_path: folder_path.clone(),
                inputs,
                inputs_snapshot,
                seed_advance,
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

    fn open_api_key_modal(&mut self, credential_id: &str, label: &str) {
        let saved = crate::core::credentials::has_secret(credential_id);
        let mut error = None;
        let value = if saved {
            match crate::core::credentials::secret_char_count(credential_id) {
                Ok(count) => "*".repeat(count.max(1)),
                Err(err) => {
                    error = Some(err);
                    String::new()
                }
            }
        } else {
            String::new()
        };
        self.api_key_modal = ApiKeyModalState {
            credential_id: credential_id.to_string(),
            label: label.to_string(),
            value,
            saved,
            masked_existing: saved && error.is_none(),
            error,
        };
        self.editor.overlays.api_keys = true;
    }

    fn api_keys_modal(&mut self, ctx: &Context) {
        let mut open = true;
        let mut close_clicked = false;
        let mut save_clicked = false;
        let mut remove_clicked = false;
        let size = modal_size(ctx, API_KEYS_MODAL_SIZE, [420.0, 300.0]);
        let title = format!("{} API Key", self.api_key_modal.label);
        let subtitle = if self.api_key_modal.saved {
            "Stored. Enter a new key to replace it."
        } else {
            "Not stored yet."
        };

        let outside_clicked = kit::dismissible_modal_scrim(ctx, "api_keys", true);
        egui::Window::new("API Key")
            .title_bar(false)
            .order(egui::Order::Foreground)
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .fixed_size(size)
            .frame(kit::modal_frame())
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                close_clicked = kit::modal_header_with_close(ui, &title, Some(subtitle), true);
                kit::modal_body(ui, |ui| {
                    if let Some(error) = &self.api_key_modal.error {
                        ui.label(RichText::new(error).color(kit::DANGER).size(12.0));
                        ui.add_space(kit::FORM_ROW_GAP);
                    }

                    kit::body_with_footer(
                        ui,
                        132.0,
                        kit::SECONDARY_BUTTON_H,
                        |ui| {
                            kit::card_panel(ui, ui.available_height(), |ui| {
                                ui.label(kit::caption(
                                    "Keys are stored locally with Windows user-level encryption.",
                                ));
                                if self.api_key_modal.masked_existing {
                                    ui.add_space(kit::FORM_ROW_GAP);
                                    ui.label(kit::caption(
                                        "The saved key is shown as a length-matched placeholder.",
                                    ));
                                }
                                ui.add_space(kit::ACTION_GAP);
                                let response = kit::labeled_password_field(
                                    ui,
                                    "API Key",
                                    &mut self.api_key_modal.value,
                                );
                                if self.api_key_modal.masked_existing
                                    && (response.changed()
                                        || response.has_focus()
                                            && self.api_key_modal.value.chars().any(|ch| ch != '*'))
                                {
                                    self.api_key_modal.masked_existing = false;
                                }
                            });
                        },
                        |ui| {
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if kit::primary_button(ui, "Save Key", 120.0).clicked() {
                                    save_clicked = true;
                                }
                                if kit::secondary_button(ui, "Close", 110.0).clicked() {
                                    close_clicked = true;
                                }
                                if self.api_key_modal.saved
                                    && kit::danger_button(ui, "Remove", 100.0).clicked()
                                {
                                    remove_clicked = true;
                                }
                            });
                        },
                    );
                });
            });

        if remove_clicked {
            match crate::core::credentials::delete_secret(&self.api_key_modal.credential_id) {
                Ok(()) => {
                    self.editor.status = format!("Removed {} API key.", self.api_key_modal.label);
                    self.editor.overlays.api_keys = false;
                }
                Err(err) => self.api_key_modal.error = Some(err),
            }
        }
        if save_clicked {
            self.save_api_key_modal();
        }
        if close_clicked || outside_clicked || !open {
            self.api_key_modal.value.clear();
            self.api_key_modal.error = None;
            self.editor.overlays.api_keys = false;
        }
    }

    fn save_api_key_modal(&mut self) {
        if self.api_key_modal.masked_existing {
            self.editor.status = format!("Kept existing {} API key.", self.api_key_modal.label);
            self.editor.overlays.api_keys = false;
            return;
        }
        if self.api_key_modal.value.trim().is_empty() {
            self.api_key_modal.error = Some("Enter an API key before saving.".to_string());
            return;
        }
        let storage_label = format!("{} API Key", self.api_key_modal.label);
        if let Err(err) = crate::core::credentials::save_secret(
            &self.api_key_modal.credential_id,
            &storage_label,
            &self.api_key_modal.value,
        ) {
            self.api_key_modal.error = Some(err);
            return;
        }

        self.editor.status = format!("Saved {} API key.", self.api_key_modal.label);
        self.api_key_modal.value.clear();
        self.api_key_modal.error = None;
        self.api_key_modal.saved = true;
        self.editor.overlays.api_keys = false;
    }

    fn providers_modal(&mut self, ctx: &Context) {
        let mut open = true;
        let mut close_clicked = false;
        let modal_size = modal_size(ctx, PROVIDERS_MODAL_SIZE, [620.0, 460.0]);
        let outside_clicked = kit::dismissible_modal_scrim(ctx, "providers", true);
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
        if close_clicked || outside_clicked || !open {
            self.editor.overlays.providers = false;
        }
    }

    fn provider_list_card(&mut self, ui: &mut Ui) {
        let card_h = ui.available_height();
        kit::card_panel(ui, card_h, |ui| {
            self.add_provider_controls(ui);

            ui.add_space(kit::ACTION_GAP);
            let selected = self.selected_provider_file.clone();
            let provider_files = self.editor.provider_files.clone();
            let mut next_selection: Option<PathBuf> = None;

            ui.horizontal(|ui| {
                ui.label(kit::section_label("Installed"));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if kit::secondary_button(ui, "Reload", 76.0).clicked() {
                        self.editor.refresh_providers();
                    }
                });
            });
            ui.add_space(kit::FORM_ROW_GAP);
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
                    let is_selected = selected.as_ref() == Some(path);
                    let response = provider_row(ui, path, &summary, is_selected);
                    if response.clicked() {
                        next_selection = Some(path.clone());
                    }
                }
            });

            if let Some(path) = next_selection {
                self.selected_provider_file = Some(path);
            }
        });
    }

    fn add_provider_controls(&mut self, ui: &mut Ui) {
        kit::field_label(ui, "Add Provider");
        ui.add_space(kit::FORM_ROW_GAP);

        let selected_label = provider_template_dropdown_label(
            self.provider_template_kind,
            self.provider_template_unavailable(self.provider_template_kind),
        );
        let mut selected_kind = self.provider_template_kind;
        ui.horizontal(|ui| {
            let button_w = kit::FIELD_H;
            let combo_w = (ui.available_width() - kit::FIELD_COMPOUND_GAP - button_w).max(80.0);
            kit::combo_field(
                ui,
                "provider_template_kind",
                selected_label,
                combo_w,
                |ui| {
                    for kind in ProviderTemplateKind::ALL {
                        let unavailable = self.provider_template_unavailable(kind);
                        let label = provider_template_dropdown_label(kind, unavailable);
                        ui.add_enabled_ui(!unavailable, |ui| {
                            automation_selectable_value(ui, &mut selected_kind, kind, &label);
                        });
                    }
                },
            );
            let unavailable = self.provider_template_unavailable(self.provider_template_kind);
            ui.add_enabled_ui(!unavailable, |ui| {
                if kit::primary_button(ui, "+", button_w).clicked() {
                    self.create_selected_provider_template();
                }
            });
        });
        self.provider_template_kind = selected_kind;
    }

    fn provider_template_unavailable(&self, kind: ProviderTemplateKind) -> bool {
        match kind {
            ProviderTemplateKind::ComfyUi => false,
            ProviderTemplateKind::OpenAiImage => self
                .editor
                .provider_entries
                .iter()
                .any(|entry| matches!(entry.connection, ProviderConnection::OpenAiImage { .. })),
            ProviderTemplateKind::XaiImage => self
                .editor
                .provider_entries
                .iter()
                .any(|entry| matches!(entry.connection, ProviderConnection::XaiImage { .. })),
            ProviderTemplateKind::XaiVideo => self
                .editor
                .provider_entries
                .iter()
                .any(|entry| matches!(entry.connection, ProviderConnection::XaiVideo { .. })),
        }
    }

    fn create_selected_provider_template(&mut self) {
        match self.provider_template_kind {
            ProviderTemplateKind::ComfyUi => self.open_provider_builder(None),
            ProviderTemplateKind::OpenAiImage => self.save_provider_template(
                crate::core::provider_store::default_openai_image_provider_entry(),
            ),
            ProviderTemplateKind::XaiImage => self.save_provider_template(
                crate::core::provider_store::default_xai_image_provider_entry(),
            ),
            ProviderTemplateKind::XaiVideo => self.save_provider_template(
                crate::core::provider_store::default_xai_video_provider_entry(),
            ),
        }
    }

    fn provider_editor_choice_card(&mut self, ui: &mut Ui) {
        let card_h = ui.available_height();
        kit::card_panel(ui, card_h, |ui| {
            let Some(path) = self.selected_provider_file.clone() else {
                kit::empty_state(
                    ui,
                    "Select a provider",
                    "Choose an installed provider to edit, or add one from the cloud provider catalog.",
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
            let supports_builder = provider_file_supports_comfy_builder(&path);
            let credential = provider_file_credential(&path);
            let mut open_builder = false;
            let mut open_json = false;
            let mut open_key = false;
            let mut delete_clicked = false;
            ui.centered_and_justified(|ui| {
                ui.vertical_centered(|ui| {
                    ui.label(
                        RichText::new(&summary.name)
                            .color(kit::TEXT)
                            .strong()
                            .size(15.0),
                    );
                    ui.add_space(4.0);
                    ui.label(kit::caption(if supports_builder {
                        "Select an editor:"
                    } else {
                        "Cloud providers use direct settings and app API keys."
                    }));
                    ui.add_space(24.0);
                    if supports_builder {
                        if kit::secondary_button(ui, "Edit in Builder", 250.0).clicked() {
                            open_builder = true;
                        }
                        ui.add_space(8.0);
                    }
                    if kit::secondary_button(ui, "Edit as JSON", 250.0).clicked() {
                        open_json = true;
                    }
                    if credential.is_some() {
                        ui.add_space(8.0);
                        if kit::secondary_button(ui, "API Key", 250.0).clicked() {
                            open_key = true;
                        }
                    }
                    ui.add_space(8.0);
                    if kit::danger_button(ui, "Delete Provider", 250.0).clicked() {
                        delete_clicked = true;
                    }
                });
            });

            if open_builder {
                self.open_provider_builder(Some(path.clone()));
            }
            if open_json {
                self.open_provider_json_editor(path.clone());
            }
            if open_key {
                if let Some((credential_id, label)) = credential {
                    self.open_api_key_modal(credential_id, label);
                }
            }
            if delete_clicked {
                self.delete_provider_file(path);
            }
        });
    }

    fn delete_provider_file(&mut self, path: PathBuf) {
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

    fn provider_json_editor_modal(&mut self, ctx: &Context) {
        let Some(path) = self.provider_json_editor_path.clone() else {
            return;
        };
        let mut open = true;
        let mut close_clicked = false;
        let mut save_clicked = false;
        let size = modal_size(ctx, PROVIDER_JSON_MODAL_SIZE, [680.0, 520.0]);

        let outside_clicked = kit::dismissible_modal_scrim(ctx, "provider_json_editor", true);
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
        if close_clicked || outside_clicked || !open {
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

    fn save_provider_template(&mut self, entry: ProviderEntry) {
        match crate::core::provider_store::save_global_provider_entry(&entry) {
            Ok(path) => {
                self.selected_provider_file = Some(path.clone());
                self.refresh_provider_files();
                self.editor.status = format!("Created provider {}", path_label(&path));
            }
            Err(err) => {
                self.editor.status = format!("Failed to create provider: {err}");
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

        let outside_clicked = kit::dismissible_modal_scrim(ctx, "provider_builder", true);
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
        if close_clicked || outside_clicked || !open {
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
        self.provider_builder.ensure_valid_tab();
        let output_active = self.provider_builder.tab == ProviderBuilderTab::Output;
        let inputs_active = self.provider_builder.tab == ProviderBuilderTab::Inputs;
        let inputs_enabled = self.provider_builder.output_configured();

        ui.horizontal(|ui| {
            if kit::timeline_tool_text_button(ui, "Output", 74.0, output_active).clicked() {
                self.provider_builder.tab = ProviderBuilderTab::Output;
            }
            let inputs_response = ui
                .add_enabled_ui(inputs_enabled, |ui| {
                    kit::timeline_tool_text_button(ui, "Inputs", 74.0, inputs_active)
                })
                .inner;
            let inputs_clicked = inputs_response.clicked();
            if !inputs_enabled {
                inputs_response.on_disabled_hover_text("Select an output node first.");
            }
            if inputs_clicked {
                self.provider_builder.tab = ProviderBuilderTab::Inputs;
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
                    let output_selected = self.provider_builder.node_is_output(&node.id);
                    let exposed_input_count =
                        self.provider_builder.exposed_input_count_for_node(&node.id);
                    let response = workflow_node_row(
                        ui,
                        &node,
                        selected,
                        output_selected,
                        exposed_input_count,
                        self.provider_builder.output_type,
                    );
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
                    if !self.provider_builder.output_configured() {
                        kit::empty_state(
                            ui,
                            "Set output first",
                            "Choose the workflow node that produces the final media before exposing inputs.",
                        );
                        return;
                    }
                    kit::field_label(ui, "Inputs");
                    ui.add_space(kit::FORM_ROW_GAP);
                    if node.inputs.is_empty() {
                        ui.label(kit::caption("No inputs found on this node."));
                    }
                    let mut expose_key: Option<String> = None;
                    for input_key in node.inputs.iter() {
                        let already_exposed =
                            self.provider_builder.input_exposed(&node.id, input_key);
                        ui.horizontal(|ui| {
                            ui.add_sized(
                                [(ui.available_width() - 76.0).max(60.0), 18.0],
                                egui::Label::new(kit::body(input_key)).truncate(),
                            );
                            let label = if already_exposed { "Exposed" } else { "Expose" };
                            let response = ui
                                .add_enabled_ui(!already_exposed, |ui| {
                                    kit::field_button(ui, label, 68.0)
                                })
                                .inner;
                            let clicked = response.clicked();
                            if already_exposed {
                                response.on_disabled_hover_text(
                                    "This workflow input is already exposed.",
                                );
                            }
                            if clicked {
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
                    let output_selected = self.provider_builder.node_is_output(&node.id);
                    let use_output_w = ui.available_width();
                    let label = if output_selected {
                        "Output Selected"
                    } else {
                        "Use as Output"
                    };
                    let response = ui
                        .add_enabled_ui(!output_selected, |ui| {
                            kit::secondary_button(ui, label, use_output_w)
                        })
                        .inner;
                    let clicked = response.clicked();
                    if output_selected {
                        response
                            .on_disabled_hover_text("This node is already the provider output.");
                    }
                    if clicked {
                        self.provider_builder.output_node = Some(ProviderOutputNodeDraft {
                            node_id: Some(node.id),
                            class_type: node.class_type,
                            title: node.title,
                        });
                        self.provider_builder.output_key = self
                            .provider_builder
                            .output_node
                            .as_ref()
                            .map(|node| {
                                inferred_output_key_for_node(
                                    node,
                                    self.provider_builder.output_type,
                                )
                            })
                            .unwrap_or_else(|| {
                                default_output_key(self.provider_builder.output_type).to_string()
                            });
                        self.provider_builder.output_tag = "output".to_string();
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
                kit::field_grid_row(ui, &[1.0, 0.46], |ui, index| match index {
                    0 => {
                        kit::labeled_text_field(
                            ui,
                            "Name",
                            &mut self.provider_builder.provider_name,
                        );
                    }
                    1 => {
                        provider_output_type_field(
                            ui,
                            "Type",
                            &mut self.provider_builder.output_type,
                        );
                    }
                    _ => {}
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
            self.provider_builder.ensure_valid_tab();
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
            if let Some(node) = self.provider_builder.output_node.as_ref() {
                let output_label = node
                    .title
                    .clone()
                    .unwrap_or_else(|| node.class_type.clone());
                ui.label(kit::value(output_label));
                ui.label(kit::caption(format!(
                    "Node {} / {}",
                    node.node_id.as_deref().unwrap_or("-"),
                    node.class_type
                )));
                ui.add_space(kit::FORM_ROW_GAP);
                ui.label(kit::caption(format!(
                    "The app will read the selected node's ComfyUI history and pick the first {} file it produced.",
                    provider_output_type_label(self.provider_builder.output_type)
                )));
            } else {
                ui.label(kit::caption(
                    "Select a saver/output node, then click Use as Output.",
                ));
            }
        });
    }

    fn set_provider_builder_workflow(&mut self, path: PathBuf) {
        match load_workflow_nodes_resolved(&path) {
            Ok(nodes) => {
                self.provider_builder.workflow_path = Some(path);
                self.provider_builder.workflow_nodes = nodes;
                self.provider_builder.workflow_error = None;
                self.provider_builder.selected_node_id = None;
                self.provider_builder.reset_workflow_bindings();
            }
            Err(err) => {
                self.provider_builder.workflow_path = Some(path);
                self.provider_builder.workflow_nodes.clear();
                self.provider_builder.workflow_error = Some(err);
                self.provider_builder.selected_node_id = None;
                self.provider_builder.reset_workflow_bindings();
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
        if self.provider_builder.inputs.iter().any(|input| {
            input.selector.node_id.as_deref() == Some(node.id.as_str())
                && input.selector.input_key == input_key
        }) {
            self.provider_builder.error = Some("Input already exposed.".to_string());
            return;
        }
        let (name, label) = provider_input_name_and_label(
            node.title.as_deref(),
            input_key,
            &self.provider_builder.inputs,
        );
        self.provider_builder
            .inputs
            .push(ProviderBuilderInput::from_node(
                node, input_key, name, label,
            ));
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

fn preview_fit_scale(rect: Rect, layers: &PreviewLayerStack) -> f32 {
    (rect.width() / layers.canvas_width.max(1) as f32)
        .min(rect.height() / layers.canvas_height.max(1) as f32)
        .max(0.01)
}

fn preview_project_scale(layers: &PreviewLayerStack, project_width: u32) -> f32 {
    layers.canvas_width.max(1) as f32 / project_width.max(1) as f32
}

fn preview_geometry_for_clip(
    project: &Project,
    clip: &Clip,
    source_size: Vec2,
    canvas_rect: Rect,
    layers: &PreviewLayerStack,
    canvas_scale: f32,
) -> PreviewObjectGeometry {
    let project_w = project.settings.width.max(1) as f32;
    let project_h = project.settings.height.max(1) as f32;
    let preview_scale = preview_project_scale(layers, project.settings.width);
    let project_to_screen = (preview_scale * canvas_scale).max(0.0001);
    let project_center = Pos2::new(project_w * 0.5, project_h * 0.5);
    let half_size = Vec2::new(
        source_size.x.max(1.0) * clip.transform.scale_x.max(0.01) * 0.5,
        source_size.y.max(1.0) * clip.transform.scale_y.max(0.01) * 0.5,
    );
    let center = project_center + Vec2::new(clip.transform.position_x, clip.transform.position_y);
    let project_rect = Rect::from_center_size(center, half_size * 2.0);
    let corners_project = [
        project_rect.left_top(),
        project_rect.right_top(),
        project_rect.right_bottom(),
        project_rect.left_bottom(),
    ];
    let screen_corners = corners_project.map(|point| {
        let rotated = rotate_point(point, center, clip.transform.rotation_deg);
        preview_project_to_screen(rotated, canvas_rect, preview_scale, canvas_scale)
    });

    PreviewObjectGeometry {
        clip_id: clip.id,
        project_rect,
        screen_corners,
        screen_center: preview_project_to_screen(center, canvas_rect, preview_scale, canvas_scale),
        project_to_screen,
    }
}

fn preview_scroll_delta(ui: &Ui, rect: Rect) -> f32 {
    let pointer_in_rect = ui
        .ctx()
        .pointer_hover_pos()
        .map(|pointer| rect.contains(pointer))
        .unwrap_or(false);
    if !pointer_in_rect {
        return 0.0;
    }
    ui.input(|input| {
        input
            .events
            .iter()
            .filter_map(|event| match event {
                egui::Event::MouseWheel { delta, .. } => Some(delta.y),
                _ => None,
            })
            .sum()
    })
}

fn preview_project_to_screen(
    point: Pos2,
    canvas_rect: Rect,
    preview_scale: f32,
    canvas_scale: f32,
) -> Pos2 {
    canvas_rect.min + Vec2::new(point.x, point.y) * preview_scale * canvas_scale
}

fn preview_screen_to_project(
    point: Pos2,
    canvas_rect: Rect,
    layers: &PreviewLayerStack,
    project_width: u32,
) -> Pos2 {
    let preview_scale = preview_project_scale(layers, project_width);
    let canvas_scale = (canvas_rect.width() / layers.canvas_width.max(1) as f32).max(0.0001);
    let project = (point - canvas_rect.min) / (preview_scale * canvas_scale).max(0.0001);
    Pos2::new(project.x, project.y)
}

fn rotate_point(point: Pos2, center: Pos2, degrees: f32) -> Pos2 {
    center + rotate_vec(point - center, degrees)
}

fn rotate_vec(vec: Vec2, degrees: f32) -> Vec2 {
    let radians = degrees.to_radians();
    let (sin, cos) = radians.sin_cos();
    Vec2::new(vec.x * cos - vec.y * sin, vec.x * sin + vec.y * cos)
}

fn vector_angle_deg(vec: Vec2) -> f32 {
    vec.y.atan2(vec.x).to_degrees()
}

fn rect_from_points(points: &[Pos2]) -> Rect {
    let mut min = Pos2::new(f32::INFINITY, f32::INFINITY);
    let mut max = Pos2::new(f32::NEG_INFINITY, f32::NEG_INFINITY);
    for point in points {
        min.x = min.x.min(point.x);
        min.y = min.y.min(point.y);
        max.x = max.x.max(point.x);
        max.y = max.y.max(point.y);
    }
    Rect::from_min_max(min, max)
}

fn timeline_floor_frame(time_seconds: f64, fps: f64) -> i64 {
    (time_seconds.max(0.0) * fps.max(1.0)).floor() as i64
}

fn preview_scale_handle_points(object: &PreviewObjectGeometry) -> [(PreviewScaleHandle, Pos2); 8] {
    let [nw, ne, se, sw] = object.screen_corners;
    [
        (PreviewScaleHandle::NorthWest, nw),
        (
            PreviewScaleHandle::North,
            Pos2::new((nw.x + ne.x) * 0.5, (nw.y + ne.y) * 0.5),
        ),
        (PreviewScaleHandle::NorthEast, ne),
        (
            PreviewScaleHandle::East,
            Pos2::new((ne.x + se.x) * 0.5, (ne.y + se.y) * 0.5),
        ),
        (PreviewScaleHandle::SouthEast, se),
        (
            PreviewScaleHandle::South,
            Pos2::new((se.x + sw.x) * 0.5, (se.y + sw.y) * 0.5),
        ),
        (PreviewScaleHandle::SouthWest, sw),
        (
            PreviewScaleHandle::West,
            Pos2::new((sw.x + nw.x) * 0.5, (sw.y + nw.y) * 0.5),
        ),
    ]
}

fn preview_rotate_handle_point(object: &PreviewObjectGeometry) -> Pos2 {
    let top_mid = preview_scale_handle_points(object)
        .iter()
        .find(|(handle, _)| *handle == PreviewScaleHandle::North)
        .map(|(_, point)| *point)
        .unwrap_or(object.screen_center);
    let offset = top_mid - object.screen_center;
    let direction = if offset.length_sq() > 0.0001 {
        offset.normalized()
    } else {
        Vec2::new(0.0, -1.0)
    };
    top_mid + direction * PREVIEW_ROTATE_HANDLE_DISTANCE
}

fn preview_scaled_transform(
    start_transform: ClipTransform,
    start_center_project: Pos2,
    pointer_project: Pos2,
    handle: PreviewScaleHandle,
    start_half_size: Vec2,
    constrain_aspect: bool,
    snap_axis: Option<PreviewScaleSnapAxis>,
    project_to_screen: f32,
) -> ClipTransform {
    let (sx, sy) = preview_scale_handle_signs(handle);
    let min_half = (4.0 / project_to_screen.max(0.0001)).max(0.5);
    let start_half_x = start_half_size.x.max(min_half);
    let start_half_y = start_half_size.y.max(min_half);
    let pointer_local = rotate_vec(
        pointer_project - start_center_project,
        -start_transform.rotation_deg,
    );

    let mut new_half_x = start_half_x;
    let mut new_half_y = start_half_y;
    let mut center_local = Vec2::ZERO;

    if sx != 0.0 {
        let anchor_x = -sx * start_half_x;
        let handle_x = pointer_local.x;
        new_half_x = ((handle_x - anchor_x).abs() * 0.5).max(min_half);
        center_local.x = (handle_x + anchor_x) * 0.5;
    }
    if sy != 0.0 {
        let anchor_y = -sy * start_half_y;
        let handle_y = pointer_local.y;
        new_half_y = ((handle_y - anchor_y).abs() * 0.5).max(min_half);
        center_local.y = (handle_y + anchor_y) * 0.5;
    }

    if constrain_aspect && (sx != 0.0 || sy != 0.0) {
        let factor_x = if sx != 0.0 {
            new_half_x / start_half_x.max(0.0001)
        } else {
            1.0
        };
        let factor_y = if sy != 0.0 {
            new_half_y / start_half_y.max(0.0001)
        } else {
            1.0
        };
        let mut factor = match (sx != 0.0, sy != 0.0, snap_axis) {
            (true, true, Some(PreviewScaleSnapAxis::X)) => factor_x,
            (true, true, Some(PreviewScaleSnapAxis::Y)) => factor_y,
            (true, true, None) => factor_x.max(factor_y),
            (true, false, _) => factor_x,
            (false, true, _) => factor_y,
            _ => 1.0,
        };
        factor = factor
            .max(min_half / start_half_x.max(0.0001))
            .max(min_half / start_half_y.max(0.0001));
        new_half_x = start_half_x * factor;
        new_half_y = start_half_y * factor;

        center_local = Vec2::ZERO;
        if sx != 0.0 {
            let anchor_x = -sx * start_half_x;
            center_local.x = anchor_x + sx * new_half_x;
        }
        if sy != 0.0 {
            let anchor_y = -sy * start_half_y;
            center_local.y = anchor_y + sy * new_half_y;
        }
    }

    let start_project_origin = Pos2::new(
        start_center_project.x - start_transform.position_x,
        start_center_project.y - start_transform.position_y,
    );
    let next_center = start_center_project + rotate_vec(center_local, start_transform.rotation_deg);
    let mut transform = start_transform;
    transform.position_x = next_center.x - start_project_origin.x;
    transform.position_y = next_center.y - start_project_origin.y;
    if start_half_x > 0.0 {
        transform.scale_x = start_transform.scale_x * (new_half_x / start_half_x);
    }
    if start_half_y > 0.0 {
        transform.scale_y = start_transform.scale_y * (new_half_y / start_half_y);
    }
    transform
}

fn preview_scale_cursor(handle: PreviewScaleHandle) -> egui::CursorIcon {
    match handle {
        PreviewScaleHandle::North | PreviewScaleHandle::South => egui::CursorIcon::ResizeVertical,
        PreviewScaleHandle::East | PreviewScaleHandle::West => egui::CursorIcon::ResizeHorizontal,
        PreviewScaleHandle::NorthEast | PreviewScaleHandle::SouthWest => {
            egui::CursorIcon::ResizeNeSw
        }
        PreviewScaleHandle::NorthWest | PreviewScaleHandle::SouthEast => {
            egui::CursorIcon::ResizeNwSe
        }
    }
}

fn preview_scale_handle_signs(handle: PreviewScaleHandle) -> (f32, f32) {
    match handle {
        PreviewScaleHandle::NorthWest => (-1.0, -1.0),
        PreviewScaleHandle::North => (0.0, -1.0),
        PreviewScaleHandle::NorthEast => (1.0, -1.0),
        PreviewScaleHandle::East => (1.0, 0.0),
        PreviewScaleHandle::SouthEast => (1.0, 1.0),
        PreviewScaleHandle::South => (0.0, 1.0),
        PreviewScaleHandle::SouthWest => (-1.0, 1.0),
        PreviewScaleHandle::West => (-1.0, 0.0),
    }
}

fn nearest_snap_delta<const N: usize>(
    candidates: [f32; N],
    targets: &[f32],
    threshold: f32,
) -> Option<(f32, f32)> {
    let mut best: Option<(f32, f32, f32)> = None;
    for candidate in candidates {
        for target in targets {
            let delta = *target - candidate;
            let distance = delta.abs();
            if distance <= threshold
                && best
                    .map(|(_, _, best_distance)| distance < best_distance)
                    .unwrap_or(true)
            {
                best = Some((delta, *target, distance));
            }
        }
    }
    best.map(|(delta, target, _)| (delta, target))
}

fn paint_rotated_texture(
    painter: &egui::Painter,
    texture_id: TextureId,
    rect: Rect,
    rotation_deg: f32,
    color: Color32,
) {
    let center = rect.center();
    let radians = rotation_deg.to_radians();
    let (sin, cos) = radians.sin_cos();
    let rotate = |pos: Pos2| {
        let offset = pos - center;
        Pos2::new(
            center.x + offset.x * cos - offset.y * sin,
            center.y + offset.x * sin + offset.y * cos,
        )
    };

    let corners = [
        rotate(rect.left_top()),
        rotate(rect.right_top()),
        rotate(rect.right_bottom()),
        rotate(rect.left_bottom()),
    ];
    let uvs = [
        Pos2::new(0.0, 0.0),
        Pos2::new(1.0, 0.0),
        Pos2::new(1.0, 1.0),
        Pos2::new(0.0, 1.0),
    ];

    let mut mesh = egui::epaint::Mesh::with_texture(texture_id);
    let base = mesh.vertices.len() as u32;
    for index in 0..4 {
        mesh.vertices.push(egui::epaint::Vertex {
            pos: corners[index],
            uv: uvs[index],
            color,
        });
    }
    mesh.indices
        .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    painter.add(egui::Shape::mesh(mesh));
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

fn timeline_rects(outer: Rect, track_scroll_y: f32) -> TimelineRects {
    let ruler = Rect::from_min_max(
        Pos2::new(outer.left() + TIMELINE_LABEL_W, outer.top()),
        Pos2::new(outer.right(), outer.top() + TIMELINE_RULER_H),
    );
    let add_row = Rect::from_min_max(
        Pos2::new(outer.left(), outer.bottom() - TIMELINE_ADD_ROW_H),
        outer.right_bottom(),
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
        Pos2::new(
            outer.left() + TIMELINE_LABEL_W,
            add_row.bottom() - TIMELINE_SCROLLBAR_H,
        ),
        Pos2::new(outer.right(), add_row.bottom()),
    );
    TimelineRects {
        outer,
        label,
        ruler,
        tracks,
        add_row,
        scrollbar,
        track_scroll_y,
    }
}

fn timeline_row_rect(rects: TimelineRects, row: usize) -> Rect {
    let top = rects.tracks.top() + row as f32 * TIMELINE_TRACK_H - rects.track_scroll_y;
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
        }) if *asset_id == candidate.asset_id
            && *source_clip_id == candidate.source_clip_id
            && *value_pinned == pinned
    )
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

fn clip_is_keyframe_image(clip: &Clip, asset: Option<&Asset>) -> bool {
    clip.image_mode == ClipImageMode::Keyframe && asset.is_some_and(|asset| asset.is_image())
}

fn timeline_clip_rect(
    clip: &Clip,
    asset: Option<&Asset>,
    row_rect: Rect,
    zoom: f32,
    scroll_x: f32,
) -> Rect {
    let x1 = time_to_timeline_x(clip.start_time, row_rect.left(), zoom, scroll_x);
    if clip_is_keyframe_image(clip, asset) {
        let y = row_rect.top() + TIMELINE_CLIP_Y_PAD;
        let h = TIMELINE_CLIP_H.min(row_rect.height() - TIMELINE_CLIP_Y_PAD * 2.0);
        let w = TIMELINE_KEYFRAME_HIT_W
            .min(row_rect.right() - x1 + 4.0)
            .max(0.0);
        return Rect::from_min_size(Pos2::new(x1 - 4.0, y), Vec2::new(w, h));
    }
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

fn next_timeline_coarse_zoom(current: f32, direction: i32, fit_zoom: f32, max_zoom: f32) -> f32 {
    let fit_zoom = fit_zoom.max(TIMELINE_MIN_ZOOM_FLOOR);
    let max_zoom = max_zoom.max(fit_zoom);
    if (max_zoom - fit_zoom).abs() <= f32::EPSILON {
        return fit_zoom;
    }

    let current = current.clamp(fit_zoom, max_zoom);
    let mut stops = vec![fit_zoom, max_zoom];
    const MANTISSAS: &[f32] = &[1.0, 1.5, 2.0, 3.0, 4.0, 5.0, 6.0, 8.0];
    let min_exp = fit_zoom.log10().floor() as i32 - 1;
    let max_exp = max_zoom.log10().ceil() as i32 + 1;
    for exp in min_exp..=max_exp {
        let decade = 10_f32.powi(exp);
        for mantissa in MANTISSAS {
            let stop = mantissa * decade;
            if stop >= fit_zoom * 0.999 && stop <= max_zoom * 1.001 {
                stops.push(stop.clamp(fit_zoom, max_zoom));
            }
        }
    }
    stops.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    stops.dedup_by(|a, b| (*a - *b).abs() <= ((*a).max(*b) * 0.002).max(0.25));

    let epsilon = (current.abs() * 0.00001).max(0.001);
    if direction >= 0 {
        stops
            .into_iter()
            .find(|stop| *stop > current + epsilon)
            .unwrap_or(max_zoom)
    } else {
        stops
            .into_iter()
            .rev()
            .find(|stop| *stop < current - epsilon)
            .unwrap_or(fit_zoom)
    }
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
        let row = ((pos.y - rects.tracks.top() + rects.track_scroll_y) / TIMELINE_TRACK_H)
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
        if geom.keyframe {
            return TimelineHit::ClipBody(geom.clip_id);
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
    let row = ((pos.y - rects.tracks.top() + rects.track_scroll_y) / TIMELINE_TRACK_H)
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

fn paint_clip_cache_buckets(painter: &egui::Painter, rect: Rect, buckets: &[bool]) {
    if buckets.is_empty() || rect.width() <= 2.0 {
        return;
    }
    let y = rect.bottom() - 4.0;
    let strip_rect = Rect::from_min_max(
        Pos2::new(rect.left(), y),
        Pos2::new(rect.right(), rect.bottom() - 1.0),
    );
    let clip_painter = painter.with_clip_rect(rect.shrink(1.0));
    clip_painter.rect_filled(
        strip_rect,
        0.0,
        Color32::from_rgba_unmultiplied(95, 58, 42, 85),
    );

    let bucket_count = buckets.len() as f32;
    let cached_color = Color32::from_rgba_unmultiplied(45, 220, 165, 175);
    let mut run_start: Option<usize> = None;
    for (index, cached) in buckets
        .iter()
        .copied()
        .chain(std::iter::once(false))
        .enumerate()
    {
        match (cached, run_start) {
            (true, None) => run_start = Some(index),
            (false, Some(start)) => {
                let x0 = rect.left() + rect.width() * start as f32 / bucket_count;
                let x1 = rect.left() + rect.width() * index as f32 / bucket_count;
                if x1 > x0 {
                    clip_painter.rect_filled(
                        Rect::from_min_max(
                            Pos2::new(x0, y),
                            Pos2::new(x1.min(rect.right()), rect.bottom() - 1.0),
                        ),
                        0.0,
                        cached_color,
                    );
                }
                run_start = None;
            }
            _ => {}
        }
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
            ProviderConnection::OpenAiImage { base_url, .. }
            | ProviderConnection::XaiImage { base_url, .. }
            | ProviderConnection::XaiVideo { base_url, .. } => {
                (base_url.clone().unwrap_or_default(), None, None)
            }
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
            tab: ProviderBuilderTab::Output,
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

    fn node_is_output(&self, node_id: &str) -> bool {
        self.output_node
            .as_ref()
            .and_then(|node| node.node_id.as_deref())
            .is_some_and(|id| id == node_id)
    }

    fn exposed_input_count_for_node(&self, node_id: &str) -> usize {
        self.inputs
            .iter()
            .filter(|input| {
                input
                    .selector
                    .node_id
                    .as_deref()
                    .is_some_and(|id| id == node_id)
            })
            .count()
    }

    fn input_exposed(&self, node_id: &str, input_key: &str) -> bool {
        self.inputs.iter().any(|input| {
            input
                .selector
                .node_id
                .as_deref()
                .is_some_and(|id| id == node_id)
                && input.selector.input_key == input_key
        })
    }

    fn output_configured(&self) -> bool {
        self.output_node
            .as_ref()
            .and_then(|node| node.node_id.as_deref())
            .is_some_and(|node_id| !node_id.trim().is_empty())
    }

    fn ensure_valid_tab(&mut self) {
        if !self.output_configured() && self.tab == ProviderBuilderTab::Inputs {
            self.tab = ProviderBuilderTab::Output;
        }
    }

    fn reset_workflow_bindings(&mut self) {
        self.output_node = None;
        self.output_key = default_output_key(self.output_type).to_string();
        self.output_tag.clear();
        self.inputs.clear();
        self.tab = ProviderBuilderTab::Output;
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
                    node_id: output.selector.node_id,
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
        match self.output_node.as_ref() {
            Some(node)
                if node
                    .node_id
                    .as_deref()
                    .is_some_and(|node_id| !node_id.trim().is_empty()) =>
            {
                format!(
                    "Output: {} ({})",
                    node.title.clone().unwrap_or_else(|| "Untitled".to_string()),
                    node.class_type
                )
            }
            Some(_) => "Output: Re-select node".to_string(),
            None => "Output: Not set".to_string(),
        }
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
        let output_key = inferred_output_key_for_node(&output_node, self.output_type);

        let mut manifest_inputs = Vec::new();
        let mut provider_inputs = Vec::new();
        for input in &self.inputs {
            let input_type = parse_provider_input_type(input)?;
            let default = parse_provider_default_value(&input_type, &input.default_text)?;
            let tag = input.tag.trim();
            let selector = NodeSelector {
                node_id: input.selector.node_id.clone(),
                tag: if tag.is_empty() {
                    None
                } else {
                    Some(tag.to_string())
                },
                class_type: input.selector.class_type.clone(),
                input_key: input.selector.input_key.clone(),
                title: input.selector.title.clone(),
            };
            if selector
                .node_id
                .as_ref()
                .is_none_or(|node_id| node_id.trim().is_empty())
                || selector.input_key.trim().is_empty()
            {
                return Err(format!(
                    "Input '{}' needs a node_id workflow binding. Select a node and expose the input again.",
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
            node_id: output_node.node_id,
            tag: if output_tag.is_empty() {
                None
            } else {
                Some(output_tag.to_string())
            },
            class_type: output_node.class_type,
            input_key: output_key,
            title: output_node.title,
        };
        if output_selector
            .node_id
            .as_ref()
            .is_none_or(|node_id| node_id.trim().is_empty())
        {
            return Err(
                "Output node needs a node_id binding. Select the output node again.".to_string(),
            );
        }

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
    fn from_node(
        node: &crate::core::comfyui_workflow::ComfyWorkflowNode,
        input_key: &str,
        name: String,
        label: String,
    ) -> Self {
        let (input_type_key, multiline) = infer_provider_input_from_workflow_node(node, input_key);
        Self {
            name,
            label,
            input_type_key,
            required: false,
            default_text: String::new(),
            enum_options: String::new(),
            tag: String::new(),
            multiline,
            selector: ProviderNodeSelectorDraft {
                node_id: Some(node.id.clone()),
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
                node_id: None,
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
                node_id: input.bind.selector.node_id,
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
                node_id: None,
                class_type: String::new(),
                input_key: String::new(),
                title: None,
            },
        }
    }
}

impl ExportModalState {
    fn for_project(project: &Project) -> Self {
        let settings = &project.settings;
        let duration = settings.duration_seconds.max(1.0);
        Self {
            output_path: export_default_output_path(project).display().to_string(),
            codec: VideoExportCodec::H264,
            width: settings.width.to_string(),
            height: settings.height.to_string(),
            fps: format_export_number(settings.fps),
            start_seconds: "0".to_string(),
            duration_seconds: format_export_number(duration),
            include_audio: true,
            quality: VideoExportQuality::Balanced,
            frame_format: VideoExportFrameFormat::Png,
            timestamp_overlay_enabled: false,
            timestamp_overlay_position: TimestampOverlayPosition::BottomCenter,
            status: ExportRunStatus::Idle,
            progress: 0.0,
            stage: "ready".to_string(),
            message: "Ready to export".to_string(),
            frame_label: String::new(),
            error: None,
            summary: None,
            warnings: Vec::new(),
        }
    }

    fn to_settings(&self) -> Result<VideoExportSettings, String> {
        let output_path = ensure_mp4_extension(PathBuf::from(self.output_path.trim()));
        let width = parse_export_u32("Width", &self.width)?;
        let height = parse_export_u32("Height", &self.height)?;
        let fps = parse_export_f64("FPS", &self.fps)?;
        let start_seconds = parse_export_f64("Start Seconds", &self.start_seconds)?;
        let duration_seconds = parse_export_f64("Duration Seconds", &self.duration_seconds)?;
        Ok(VideoExportSettings {
            output_path,
            codec: self.codec,
            width,
            height,
            fps,
            start_seconds,
            duration_seconds,
            include_audio: self.include_audio,
            quality: self.quality,
            frame_format: self.frame_format,
            timestamp_overlay: TimestampOverlaySettings {
                enabled: self.timestamp_overlay_enabled,
                position: self.timestamp_overlay_position,
            },
        })
    }
}

fn export_default_output_path(project: &Project) -> PathBuf {
    let file_name = format!("{}.mp4", sanitize_export_stem(&project.name));
    project
        .project_path
        .as_ref()
        .map(|root| root.join("exports").join(&file_name))
        .unwrap_or_else(|| default_projects_dir().join("exports").join(file_name))
}

fn sanitize_export_stem(value: &str) -> String {
    let stem = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim_matches('_')
        .to_string();
    if stem.is_empty() {
        "export".to_string()
    } else {
        stem
    }
}

fn ensure_mp4_extension(mut path: PathBuf) -> PathBuf {
    let needs_extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| !extension.eq_ignore_ascii_case("mp4"))
        .unwrap_or(true);
    if needs_extension {
        path.set_extension("mp4");
    }
    path
}

fn parse_export_u32(label: &str, value: &str) -> Result<u32, String> {
    value
        .trim()
        .parse::<u32>()
        .map_err(|_| format!("{label} must be a whole number."))
}

fn parse_export_f64(label: &str, value: &str) -> Result<f64, String> {
    value
        .trim()
        .parse::<f64>()
        .map_err(|_| format!("{label} must be a number."))
}

fn format_export_number(value: f64) -> String {
    if (value - value.round()).abs() < 0.0001 {
        format!("{value:.0}")
    } else {
        format!("{value:.3}")
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string()
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

fn provider_template_dropdown_label(kind: ProviderTemplateKind, unavailable: bool) -> String {
    if unavailable {
        format!("{} (already added)", provider_template_label(kind))
    } else {
        provider_template_label(kind).to_string()
    }
}

fn provider_template_label(kind: ProviderTemplateKind) -> &'static str {
    match kind {
        ProviderTemplateKind::ComfyUi => "ComfyUI Workflow",
        ProviderTemplateKind::OpenAiImage => "OpenAI Image",
        ProviderTemplateKind::XaiImage => "xAI Image",
        ProviderTemplateKind::XaiVideo => "xAI Grok Video",
    }
}

fn provider_file_credential(path: &Path) -> Option<(&'static str, &'static str)> {
    let text = std::fs::read_to_string(path).ok()?;
    let entry = serde_json::from_str::<ProviderEntry>(&text).ok()?;
    match entry.connection {
        ProviderConnection::OpenAiImage { .. } => {
            Some((crate::core::credentials::OPENAI_CREDENTIAL_ID, "OpenAI"))
        }
        ProviderConnection::XaiImage { .. } => {
            Some((crate::core::credentials::XAI_CREDENTIAL_ID, "xAI"))
        }
        ProviderConnection::XaiVideo { .. } => {
            Some((crate::core::credentials::XAI_CREDENTIAL_ID, "xAI"))
        }
        ProviderConnection::ComfyUi { .. } | ProviderConnection::CustomHttp { .. } => None,
    }
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

fn provider_file_supports_comfy_builder(path: &Path) -> bool {
    let Ok(text) = std::fs::read_to_string(path) else {
        return false;
    };
    let Ok(entry) = serde_json::from_str::<ProviderEntry>(&text) else {
        return false;
    };
    matches!(entry.connection, ProviderConnection::ComfyUi { .. })
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

fn clip_image_mode_label(mode: ClipImageMode) -> &'static str {
    match mode {
        ClipImageMode::Still => "Still Image",
        ClipImageMode::Keyframe => "Keyframe Reference",
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
                automation_selectable_value(ui, value, ProviderOutputType::Image, "Image");
                automation_selectable_value(ui, value, ProviderOutputType::Video, "Video");
                automation_selectable_value(ui, value, ProviderOutputType::Audio, "Audio");
            });
    });
}

fn workflow_node_row(
    ui: &mut Ui,
    node: &crate::core::comfyui_workflow::ComfyWorkflowNode,
    selected: bool,
    output_selected: bool,
    exposed_input_count: usize,
    output_type: ProviderOutputType,
) -> egui::Response {
    let status_accent = if output_selected {
        Some(provider_output_color(output_type))
    } else if exposed_input_count > 0 {
        Some(kit::BORDER_FOCUS)
    } else {
        None
    };
    kit::draw_accent_row_with_status(ui, 54.0, selected, kit::IMAGE, status_accent, |ui, rect| {
        let title = node.title.as_deref().unwrap_or("Untitled");
        let status = match (output_selected, exposed_input_count) {
            (true, 0) => "  Output".to_string(),
            (true, 1) => "  Output + 1 input".to_string(),
            (true, count) => format!("  Output + {count} inputs"),
            (false, 1) => "  1 input exposed".to_string(),
            (false, count) if count > 1 => format!("  {count} inputs exposed"),
            _ => String::new(),
        };
        let subtitle = format!("{}  Node {}{}", node.class_type, node.id, status);
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
    let card_w = ui.available_width().max(0.0);
    ui.scope(|ui| {
        ui.set_width(card_w);
        ui.set_min_width(card_w);
        ui.set_max_width(card_w);
        kit::sunken_frame().show(ui, |ui| {
            let content_w = (card_w - 16.0).max(0.0);
            ui.set_width(content_w);
            ui.set_min_width(content_w);
            ui.set_max_width(content_w);
            provider_builder_input_editor_contents(ui, index, len, input, action);
        });
    });
}

fn provider_builder_input_editor_contents(
    ui: &mut Ui,
    index: usize,
    len: usize,
    input: &mut ProviderBuilderInput,
    action: &mut Option<ProviderInputAction>,
) {
    kit::field_grid_row(ui, &[1.0, 1.0], |ui, column| match column {
        0 => {
            kit::labeled_text_field(ui, "Name", &mut input.name);
        }
        1 => {
            kit::labeled_text_field(ui, "Label", &mut input.label);
        }
        _ => {}
    });
    ui.add_space(kit::FORM_ROW_GAP);
    kit::field_grid_row(ui, &[0.44, 1.0], |ui, column| match column {
        0 => {
            provider_input_type_field(ui, "Type", &mut input.input_type_key);
        }
        1 => {
            provider_builder_default_field(ui, input);
        }
        _ => {}
    });
    ui.add_space(kit::FORM_ROW_GAP);
    if input.input_type_key == "enum" {
        kit::labeled_text_field(ui, "Enum Options", &mut input.enum_options);
        ui.add_space(kit::FORM_ROW_GAP);
    }
    kit::field_grid_row(ui, &[1.0, 1.0], |ui, column| match column {
        0 => {
            kit::labeled_text_field(ui, "Tag", &mut input.tag);
        }
        1 => {
            ui.add_space(kit::FIELD_LABEL_H + kit::FIELD_LABEL_GAP);
            ui.horizontal(|ui| {
                automation_checkbox(ui, &mut input.required, "Required");
                if input.input_type_key == "text" {
                    automation_checkbox(ui, &mut input.multiline, "Multiline");
                } else {
                    input.multiline = false;
                }
            });
        }
        _ => {}
    });
    ui.add_space(kit::FORM_ROW_GAP);
    ui.horizontal(|ui| {
        let gap = ui.spacing().item_spacing.x;
        let buttons_w = 42.0 + 52.0 + 66.0 + gap * 3.0;
        ui.add_sized(
            [(ui.available_width() - buttons_w).max(0.0), 18.0],
            egui::Label::new(kit::caption(format!(
                "-> node {} / {}.{}",
                input.selector.node_id.as_deref().unwrap_or("-"),
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
}

fn provider_builder_default_field(ui: &mut Ui, input: &mut ProviderBuilderInput) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = kit::FIELD_LABEL_GAP;
        kit::field_label(ui, "Default");
        match input.input_type_key.as_str() {
            "boolean" => {
                let mut value = parse_bool_default_text(&input.default_text).unwrap_or(false);
                let response = automation_checkbox(ui, &mut value, "True");
                if response.changed() {
                    input.default_text = value.to_string();
                }
            }
            "integer" => {
                let mut value = input.default_text.trim().parse::<i64>().unwrap_or(0);
                let width = ui.available_width();
                let rect = inspector_numeric_rect(ui, width);
                if provider_builder_integer_default_in_rect(ui, rect, &mut value) {
                    input.default_text = value.to_string();
                }
            }
            "number" => {
                let mut value = input.default_text.trim().parse::<f64>().unwrap_or(0.0);
                let width = ui.available_width();
                let rect = inspector_numeric_rect(ui, width);
                if provider_builder_number_default_in_rect(ui, rect, &mut value) {
                    input.default_text = value.to_string();
                }
            }
            "enum" => {
                let options = provider_builder_enum_options(input);
                if options.is_empty() {
                    kit::singleline_text_field(ui, &mut input.default_text, ui.available_width());
                } else {
                    if input.default_text.trim().is_empty() {
                        input.default_text = options[0].clone();
                    }
                    let selected = input.default_text.clone();
                    kit::combo_field(
                        ui,
                        ("provider_default_enum", &input.name),
                        selected,
                        ui.available_width(),
                        |ui| {
                            for option in options {
                                automation_selectable_value(
                                    ui,
                                    &mut input.default_text,
                                    option.clone(),
                                    &option,
                                );
                            }
                        },
                    );
                }
            }
            "image" | "video" | "audio" => {
                input.default_text.clear();
                kit::readonly_value_box(
                    ui,
                    "Runtime asset binding",
                    Vec2::new(ui.available_width(), kit::FIELD_H),
                );
            }
            _ => {
                kit::singleline_text_field(ui, &mut input.default_text, ui.available_width());
            }
        }
    });
}

fn provider_builder_integer_default_in_rect(ui: &mut Ui, rect: Rect, value: &mut i64) -> bool {
    inspector_numeric_field(ui, rect, |ui, width| {
        ui.add_sized(
            [width, INSPECTOR_NUMERIC_H],
            egui::DragValue::new(value).speed(1.0),
        )
    })
}

fn provider_builder_number_default_in_rect(ui: &mut Ui, rect: Rect, value: &mut f64) -> bool {
    inspector_numeric_field(ui, rect, |ui, width| {
        ui.add_sized(
            [width, INSPECTOR_NUMERIC_H],
            egui::DragValue::new(value).speed(0.1),
        )
    })
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
                    automation_selectable_value(ui, value, key.to_string(), label);
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
            let parsed = parse_bool_default_text(trimmed)
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

fn parse_bool_default_text(text: &str) -> Result<bool, ()> {
    match text.trim().to_ascii_lowercase().as_str() {
        "true" | "t" | "yes" | "y" | "on" | "1" => Ok(true),
        "false" | "f" | "no" | "n" | "off" | "0" => Ok(false),
        _ => Err(()),
    }
}

fn provider_builder_enum_options(input: &ProviderBuilderInput) -> Vec<String> {
    input
        .enum_options
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect()
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

fn provider_input_name_and_label(
    node_title: Option<&str>,
    input_key: &str,
    inputs: &[ProviderBuilderInput],
) -> (String, String) {
    let existing = inputs
        .iter()
        .map(|input| input.name.as_str())
        .collect::<HashSet<_>>();
    let key_base = sanitize_provider_input_name(input_key).unwrap_or_else(|| "input".to_string());
    let use_title = is_generic_provider_input_key(input_key)
        || existing.contains(key_base.as_str())
        || input_key.trim().is_empty();
    let title_base = node_title
        .and_then(sanitize_provider_input_name)
        .filter(|value| !value.is_empty());
    let base = if use_title {
        title_base.unwrap_or(key_base)
    } else {
        key_base
    };
    if !existing.contains(base.as_str()) {
        let label_source = if use_title {
            node_title.unwrap_or(input_key)
        } else {
            input_key
        };
        return (base, friendly_provider_label(label_source));
    }
    for index in 2.. {
        let candidate = format!("{base}_{index}");
        if !existing.contains(candidate.as_str()) {
            let label_source = if use_title {
                node_title.unwrap_or(input_key)
            } else {
                input_key
            };
            return (candidate, friendly_provider_label(label_source));
        }
    }
    unreachable!()
}

fn is_generic_provider_input_key(input_key: &str) -> bool {
    matches!(
        input_key.trim().to_ascii_lowercase().as_str(),
        "text" | "image" | "video" | "audio" | "value" | "filename" | "file"
    )
}

fn sanitize_provider_input_name(value: &str) -> Option<String> {
    let mut output = String::new();
    let mut last_was_separator = false;
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            output.push(ch.to_ascii_lowercase());
            last_was_separator = false;
        } else if !last_was_separator && !output.is_empty() {
            output.push('_');
            last_was_separator = true;
        }
    }
    while output.ends_with('_') {
        output.pop();
    }
    if output.is_empty() {
        None
    } else if output
        .chars()
        .next()
        .is_some_and(|first| first.is_ascii_digit())
    {
        Some(format!("input_{output}"))
    } else {
        Some(output)
    }
}

fn infer_provider_input_from_workflow_node(
    node: &crate::core::comfyui_workflow::ComfyWorkflowNode,
    input_key: &str,
) -> (String, bool) {
    let key = input_key.to_ascii_lowercase();
    let class_type = node.class_type.to_ascii_lowercase();
    let title = node.title.as_deref().unwrap_or("").to_ascii_lowercase();
    if key.contains("image") || class_type.contains("loadimage") {
        return ("image".to_string(), false);
    }
    if key.contains("video") || class_type.contains("loadvideo") {
        return ("video".to_string(), false);
    }
    if key.contains("audio") || class_type.contains("loadaudio") {
        return ("audio".to_string(), false);
    }
    if key.contains("seed")
        || matches!(
            key.as_str(),
            "steps" | "width" | "height" | "batch_size" | "frames" | "frame_load_cap"
        )
    {
        return ("integer".to_string(), false);
    }
    if key.contains("cfg")
        || key.contains("denoise")
        || key.contains("duration")
        || key.contains("rate")
        || key.contains("crf")
    {
        return ("number".to_string(), false);
    }
    if class_type.contains("boolean") || matches!(key.as_str(), "enabled" | "save_output") {
        return ("boolean".to_string(), false);
    }
    let multiline =
        class_type.contains("multiline") || title.contains("prompt") || key.contains("prompt");
    ("text".to_string(), multiline)
}

fn default_output_key(output_type: ProviderOutputType) -> &'static str {
    match output_type {
        ProviderOutputType::Image => "images",
        ProviderOutputType::Video => "images",
        ProviderOutputType::Audio => "audio",
    }
}

fn inferred_output_key_for_node(
    node: &ProviderOutputNodeDraft,
    output_type: ProviderOutputType,
) -> String {
    let class_type = node.class_type.to_ascii_lowercase();
    if class_type.contains("savevideo") || class_type.contains("videocombine") {
        // ComfyUI video saver/combine nodes commonly report downloadable mp4s
        // under the historical `images` output key.
        return "images".to_string();
    }
    if class_type.contains("saveimage") {
        return "images".to_string();
    }
    if class_type.contains("saveaudio") || class_type.contains("audio") {
        return "audio".to_string();
    }
    default_output_key(output_type).to_string()
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
