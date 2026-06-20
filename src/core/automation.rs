//! Loopback-only automation control plane for desktop smoke scenarios.
//!
//! This module exposes both semantic editor commands and a UI-level control
//! plane. Shared egui widgets register their current-frame responses here, so
//! automation can discover visible controls and ask the real widgets to invoke
//! clicks or text changes during the normal UI render pass.

use eframe::egui::{self, Response};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Mutex, OnceLock,
};
use std::time::{Duration, Instant};
use uuid::Uuid;

use crate::core::export::{
    TimestampOverlayPosition, VideoExportCodec, VideoExportFrameFormat, VideoExportQuality,
};
use crate::state::{
    Asset, AssetKind, AssetLabNode, BatchSettings, ClipImageMode, ClipTransform, GenerativeConfig,
    InputValue, Project, ProjectSettings, ProviderEntry, ProviderOutputType, SelectionState,
    TrackType,
};

const DEFAULT_AUTOMATION_PORT: u16 = 47_890;
const RESPONSE_TIMEOUT_SECONDS: u64 = 20;
const PROFILE_RESPONSE_TIMEOUT_SECONDS: u64 = 120;
const DEFAULT_GENERATION_WAIT_TIMEOUT_SECONDS: u64 = 30 * 60;
const MAX_GENERATION_WAIT_TIMEOUT_SECONDS: u64 = 24 * 60 * 60;
const DEFAULT_GENERATION_WAIT_POLL_MS: u64 = 500;
const MIN_GENERATION_WAIT_POLL_MS: u64 = 100;
const MAX_GENERATION_WAIT_POLL_MS: u64 = 5_000;
const MAX_REQUEST_HEADER_BYTES: usize = 64 * 1024;
const MAX_REQUEST_BODY_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_AGENT_CUTSHEET_FRAMES: usize = 24;
pub const MAX_AGENT_CUTSHEET_THUMB_WIDTH: u32 = 1024;

static CONFIG: OnceLock<AutomationConfig> = OnceLock::new();
static COMMAND_TX: OnceLock<Sender<AutomationEnvelope>> = OnceLock::new();
static COMMAND_RX: OnceLock<Mutex<Receiver<AutomationEnvelope>>> = OnceLock::new();
static UI_REGISTRY: OnceLock<Mutex<UiRegistry>> = OnceLock::new();
static ACTIVE: AtomicBool = AtomicBool::new(false);

/// Configuration for the loopback automation server.
#[derive(Clone, Debug)]
pub struct AutomationConfig {
    /// TCP port bound on 127.0.0.1.
    pub port: u16,
}

/// Semantic app commands accepted by the automation API.
#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AutomationCommand {
    /// Return the current app state snapshot.
    GetState {
        #[serde(default)]
        include: Vec<String>,
    },
    /// Return the current visible UI registry.
    GetUi,
    /// Return agent API capability metadata.
    GetCapabilities,
    /// List project folders under the default projects root or a supplied root.
    ListProjects {
        #[serde(default)]
        root: Option<PathBuf>,
    },
    /// Click a visible UI widget by automation ID.
    ClickUi { id: String },
    /// Replace or append text in a visible editable UI widget by automation ID.
    TextUi {
        id: String,
        text: String,
        #[serde(default = "default_text_replace")]
        replace: bool,
    },
    /// Capture the current application viewport to `.tmp/automation-screenshots`.
    Screenshot {
        #[serde(default)]
        name: Option<String>,
    },
    /// Create a project under `parent_dir/name`.
    CreateProject {
        parent_dir: PathBuf,
        name: String,
        #[serde(default)]
        settings: Option<ProjectSettings>,
    },
    /// Open an existing project folder.
    OpenProject { folder: PathBuf },
    /// Patch current project settings.
    SetProjectSettings { patch: ProjectSettingsPatch },
    /// Import a file through the normal project import path.
    ImportAsset { path: PathBuf },
    /// Rename an asset, resolving by ID, name, or first selected asset.
    RenameAsset {
        #[serde(default)]
        asset_id: Option<Uuid>,
        #[serde(default)]
        asset_name: Option<String>,
        name: String,
    },
    /// Duplicate an asset as a new project asset.
    DuplicateAsset {
        #[serde(default)]
        asset_id: Option<Uuid>,
        #[serde(default)]
        asset_name: Option<String>,
    },
    /// Delete project assets and clips that reference them.
    DeleteAssets { asset_ids: Vec<Uuid> },
    /// Update cached asset duration.
    SetAssetDuration {
        asset_id: Uuid,
        duration_seconds: Option<f64>,
    },
    /// Extract a generative asset's active output as a normal project asset.
    ExtractActiveGeneration {
        #[serde(default)]
        asset_id: Option<Uuid>,
        #[serde(default)]
        asset_name: Option<String>,
    },
    /// Extract a specific generative asset version as a normal project asset.
    ExtractGenerationVersion {
        asset_id: Uuid,
        #[serde(default)]
        version: Option<String>,
    },
    /// Render a timeline, clip, or asset still and persist it as a project image asset.
    ExtractStillToAsset {
        source: CaptureSource,
        #[serde(default)]
        time: Option<TimeSelector>,
        #[serde(default)]
        name: Option<String>,
    },
    /// Add an asset to the timeline, resolving by ID, name, or first asset.
    AddAssetToTimeline {
        #[serde(default)]
        asset_id: Option<Uuid>,
        #[serde(default)]
        asset_name: Option<String>,
        #[serde(default)]
        track_id: Option<Uuid>,
        #[serde(default)]
        time: Option<f64>,
        #[serde(default)]
        duration_seconds: Option<f64>,
    },
    /// Create a hollow generative asset.
    CreateGenerativeAsset {
        output_type: ProviderOutputType,
        #[serde(default)]
        name: Option<String>,
        #[serde(default)]
        fps: Option<f64>,
        #[serde(default)]
        frame_count: Option<u32>,
    },
    /// Create an image-to-image generative clip from an existing image clip.
    CreateI2iFromClip {
        clip_id: Uuid,
        #[serde(default)]
        provider_id: Option<Uuid>,
    },
    /// Create an image-to-video generative clip from an image or video clip.
    CreateI2vFromClip {
        clip_id: Uuid,
        reference: I2vReference,
        #[serde(default)]
        provider_id: Option<Uuid>,
    },
    /// Create a first/last-frame video bridge from reference clips.
    CreateBridgeFromClips {
        clip_ids: Vec<Uuid>,
        #[serde(default)]
        provider_id: Option<Uuid>,
    },
    /// Seek the playhead to a timestamp in seconds.
    Seek { time: f64 },
    /// Set preview playback state through the normal preview/audio runtime.
    SetPlayback { playing: bool },
    /// Move the playhead by a signed number of project frames.
    StepTimeline { frames: i64 },
    /// Return current preview cache/timing diagnostics and recent render samples.
    GetPerformanceDiagnostics,
    /// Drive the real egui seek path repeatedly and return preview timing samples.
    ScrubTimelineProfile {
        start_time: f64,
        end_time: f64,
        #[serde(default = "default_scrub_profile_steps")]
        steps: usize,
        #[serde(default = "default_scrub_profile_repeats")]
        repeats: usize,
        #[serde(default)]
        scrub_audio: bool,
        #[serde(default)]
        settle_ms: u64,
    },
    /// Select a clip by ID or by timeline index.
    SelectClip {
        #[serde(default)]
        clip_id: Option<Uuid>,
        #[serde(default)]
        index: Option<usize>,
    },
    /// Select an asset by ID or asset-list index.
    SelectAsset {
        #[serde(default)]
        asset_id: Option<Uuid>,
        #[serde(default)]
        index: Option<usize>,
    },
    /// Select a track by ID or timeline track index.
    SelectTrack {
        #[serde(default)]
        track_id: Option<Uuid>,
        #[serde(default)]
        index: Option<usize>,
    },
    /// Select a marker by ID or marker-list index.
    SelectMarker {
        #[serde(default)]
        marker_id: Option<Uuid>,
        #[serde(default)]
        index: Option<usize>,
    },
    /// Replace the current selection.
    SetSelection {
        #[serde(default)]
        clips: Vec<Uuid>,
        #[serde(default)]
        assets: Vec<Uuid>,
        #[serde(default)]
        tracks: Vec<Uuid>,
        #[serde(default)]
        markers: Vec<Uuid>,
    },
    /// Add a marker at the provided time or current playhead.
    AddMarker {
        #[serde(default)]
        time: Option<f64>,
        #[serde(default)]
        track_id: Option<Uuid>,
        #[serde(default)]
        label: Option<String>,
    },
    /// Patch marker fields.
    SetMarker { marker_id: Uuid, patch: MarkerPatch },
    /// Delete a marker.
    DeleteMarker { marker_id: Uuid },
    /// Add a timeline track.
    AddTrack {
        track_type: TrackType,
        #[serde(default)]
        index: Option<usize>,
        #[serde(default)]
        name: Option<String>,
    },
    /// Patch track fields.
    SetTrack { track_id: Uuid, patch: TrackPatch },
    /// Reorder a track.
    MoveTrack { track_id: Uuid, index: usize },
    /// Delete a track, optionally dry-running item counts.
    DeleteTrack {
        track_id: Uuid,
        #[serde(default)]
        dry_run: bool,
    },
    /// Patch clip fields.
    SetClip { clip_id: Uuid, patch: ClipPatch },
    /// Move a clip.
    MoveClip {
        clip_id: Uuid,
        start_time: f64,
        #[serde(default)]
        track_id: Option<Uuid>,
    },
    /// Move multiple clips atomically, either to absolute targets or by a relative delta.
    MoveClips {
        mode: ClipMoveMode,
        #[serde(default)]
        moves: Vec<ClipMoveTarget>,
        #[serde(default)]
        clip_ids: Vec<Uuid>,
        #[serde(default)]
        delta_seconds: Option<f64>,
        #[serde(default)]
        track_delta: Option<i32>,
        #[serde(default)]
        track_id: Option<Uuid>,
    },
    /// Resize a clip.
    ResizeClip {
        clip_id: Uuid,
        start_time: f64,
        duration: f64,
    },
    /// Delete timeline clips.
    DeleteClips { clip_ids: Vec<Uuid> },
    /// List loaded providers.
    ListProviders,
    /// Reload providers from disk.
    RefreshProviders,
    /// Create a provider from a built-in template.
    CreateProviderFromTemplate { template: ProviderTemplate },
    /// Create a provider entry.
    CreateProvider { provider: ProviderEntry },
    /// Replace a provider entry.
    UpdateProvider {
        provider_id: Uuid,
        provider: ProviderEntry,
    },
    /// Delete a provider entry.
    DeleteProvider { provider_id: Uuid },
    /// Run a non-generating provider connectivity check.
    TestProvider {
        provider_id: Uuid,
        #[serde(default = "default_true")]
        live: bool,
    },
    /// Return whether credential IDs are present without exposing secret values.
    GetCredentialStatus { credential_ids: Vec<String> },
    /// Save or replace a provider credential secret.
    SetCredential {
        credential_id: String,
        label: String,
        value: String,
    },
    /// Delete a provider credential secret.
    DeleteCredential { credential_id: String },
    /// Get a generative asset config.
    GetGenerativeConfig { asset_id: Uuid },
    /// Patch a generative asset config.
    SetGenerativeConfig {
        asset_id: Uuid,
        patch: GenerativeConfigPatch,
    },
    /// Replace a generative asset config.
    ReplaceGenerativeConfig {
        asset_id: Uuid,
        config: GenerativeConfig,
    },
    /// Set the active version for a generative asset.
    SetActiveGenerationVersion { asset_id: Uuid, version: String },
    /// Duplicate a generative version.
    DuplicateGenerationVersion { asset_id: Uuid, version: String },
    /// Delete a generative version.
    DeleteGenerationVersion { asset_id: Uuid, version: String },
    /// Read a generative asset's Asset Lab graph.
    GetAssetLabGraph { asset_id: Uuid },
    /// Add an Asset Lab node.
    AddAssetLabNode {
        asset_id: Uuid,
        #[serde(default)]
        provider_id: Option<Uuid>,
        #[serde(default)]
        parent_node_id: Option<Uuid>,
        #[serde(default)]
        inputs: HashMap<String, InputValue>,
    },
    /// Patch an Asset Lab node.
    SetAssetLabNode {
        asset_id: Uuid,
        node_id: Uuid,
        patch: AssetLabNodePatch,
    },
    /// Delete an Asset Lab node.
    DeleteAssetLabNode { asset_id: Uuid, node_id: Uuid },
    /// Queue generation for an Asset Lab node.
    GenerateAssetLabNode { asset_id: Uuid, node_id: Uuid },
    /// Start generation for a generative asset.
    StartGeneration {
        asset_id: Uuid,
        #[serde(default)]
        context_clip_id: Option<Uuid>,
        #[serde(default)]
        wait: bool,
    },
    /// List generation jobs.
    ListJobs,
    /// Get one generation job.
    GetJob { job_id: Uuid },
    /// Cancel a generation job.
    CancelJob { job_id: Uuid },
    /// Start a video export through the normal export runtime.
    ExportVideo {
        #[serde(default)]
        request: ExportVideoRequest,
    },
    /// Return current video export progress and summary.
    GetExportStatus,
    /// Cancel the current video export.
    CancelExport,
    /// Capture a rendered timeline/clip/asset frame or cutsheet.
    Capture { request: CaptureRequest },
    /// Save the current project.
    SaveProject,
    /// Open the local providers modal.
    OpenProviders,
    /// Close the local providers modal.
    CloseProviders,
    /// Open the project settings modal.
    OpenProjectSettings,
    /// Close the project settings modal.
    CloseProjectSettings,
    /// Open the in-app new project modal.
    OpenNewProject,
    /// Close the in-app new project modal.
    CloseNewProject,
    /// Open the generation queue panel.
    OpenQueue,
    /// Close the generation queue panel.
    CloseQueue,
    /// Open the generative video creation modal.
    OpenGenerativeVideo,
    /// Close the generative video creation modal.
    CloseGenerativeVideo,
    /// Open the export-video modal.
    OpenExportVideo,
    /// Close the export-video modal.
    CloseExportVideo,
    /// Set collapsible layout and preview flags for reference screenshots.
    SetLayout {
        #[serde(default)]
        left_collapsed: Option<bool>,
        #[serde(default)]
        right_collapsed: Option<bool>,
        #[serde(default)]
        timeline_collapsed: Option<bool>,
        #[serde(default)]
        preview_stats: Option<bool>,
        #[serde(default)]
        hardware_decode: Option<bool>,
        #[serde(default)]
        left_width: Option<f32>,
        #[serde(default)]
        right_width: Option<f32>,
        #[serde(default)]
        timeline_height: Option<f32>,
        #[serde(default)]
        timeline_zoom: Option<f32>,
        #[serde(default)]
        timeline_scroll_x: Option<f32>,
        #[serde(default)]
        timeline_scroll_y: Option<f32>,
    },
    /// Close transient modals, panels, and overlays controlled by automation.
    CloseAllOverlays,
}

/// Command envelope passed from the HTTP server to the app runtime.
pub struct AutomationEnvelope {
    /// Command to apply on the app runtime.
    pub command: AutomationCommand,
    responder: Sender<AutomationResponse>,
}

impl AutomationEnvelope {
    /// Send a response back to the HTTP request handler.
    pub fn respond(self, response: AutomationResponse) {
        let _ = self.responder.send(response);
    }
}

/// Screen-space rectangle reported by the UI registry.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct UiRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl From<egui::Rect> for UiRect {
    fn from(rect: egui::Rect) -> Self {
        Self {
            x: rect.left(),
            y: rect.top(),
            width: rect.width(),
            height: rect.height(),
        }
    }
}

/// A visible widget captured from the most recent egui frame.
#[derive(Clone, Debug, Serialize)]
pub struct UiElement {
    /// Stable for the current widget identity. Query `/ui` before using it.
    pub id: String,
    /// Widget class such as `button`, `text_field`, `combo`, `row`, or `color_field`.
    pub kind: String,
    /// Human-facing label/value when the widget helper knows one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Full paint rectangle in egui points.
    pub rect: UiRect,
    /// Interactive rectangle after egui clipping.
    pub interact_rect: UiRect,
    /// Whether egui considered the widget enabled this frame.
    pub enabled: bool,
    /// Whether `/ui/click` can invoke it.
    pub clickable: bool,
    /// Whether `/ui/text` can replace or append text.
    pub editable: bool,
    /// Whether the widget currently has keyboard focus.
    pub focused: bool,
}

#[derive(Clone, Debug)]
struct UiElementRecord {
    element: UiElement,
}

#[derive(Default)]
struct PendingText {
    text: String,
    replace: bool,
}

#[derive(Default)]
struct UiRegistry {
    elements: Vec<UiElementRecord>,
    pending_clicks: HashSet<String>,
    pending_text: HashMap<String, PendingText>,
    consumed_actions: HashSet<String>,
    frame_index: u64,
}

/// JSON response returned by the automation API.
#[derive(Clone, Debug, Serialize)]
pub struct AutomationResponse {
    /// Whether the command succeeded.
    pub ok: bool,
    /// Optional error or status message.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// Command-specific response payload.
    #[serde(default)]
    pub data: Value,
    /// HTTP status to use for this response.
    #[serde(skip)]
    pub http_status: u16,
}

impl AutomationResponse {
    /// Build a successful response with a JSON payload.
    pub fn ok(data: Value) -> Self {
        Self {
            ok: true,
            message: None,
            data,
            http_status: 200,
        }
    }

    /// Build a successful response with an empty payload.
    pub fn empty_ok() -> Self {
        Self::ok(json!({}))
    }

    /// Build a failed response with an error message.
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            ok: false,
            message: Some(message.into()),
            data: json!({}),
            http_status: 400,
        }
    }

    /// Build a 404 response.
    pub fn not_found(message: impl Into<String>) -> Self {
        Self {
            ok: false,
            message: Some(message.into()),
            data: json!({}),
            http_status: 404,
        }
    }

    /// Build a 409 response for stateful UI/action conflicts.
    pub fn conflict(message: impl Into<String>) -> Self {
        Self {
            ok: false,
            message: Some(message.into()),
            data: json!({}),
            http_status: 409,
        }
    }

    /// Build a failed response with an explicit HTTP status.
    pub fn with_status(message: impl Into<String>, http_status: u16) -> Self {
        Self {
            ok: false,
            message: Some(message.into()),
            data: json!({}),
            http_status,
        }
    }

    /// Build a failed response with an explicit HTTP status and JSON payload.
    pub fn with_status_data(message: impl Into<String>, http_status: u16, data: Value) -> Self {
        Self {
            ok: false,
            message: Some(message.into()),
            data,
            http_status,
        }
    }
}

#[derive(Debug, Deserialize)]
struct UiClickRequest {
    id: String,
}

#[derive(Debug, Deserialize)]
struct UiTextRequest {
    id: String,
    text: String,
    #[serde(default = "default_text_replace")]
    replace: bool,
}

#[derive(Debug, Default, Deserialize)]
struct GenerationWaitRequest {
    #[serde(default)]
    job_id: Option<Uuid>,
    #[serde(default)]
    timeout_seconds: Option<u64>,
    #[serde(default)]
    poll_interval_ms: Option<u64>,
}

#[derive(Default, Debug, Deserialize)]
struct ScreenshotRequest {
    #[serde(default)]
    name: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct ProjectSettingsPatch {
    #[serde(default)]
    pub width: Option<u32>,
    #[serde(default)]
    pub height: Option<u32>,
    #[serde(default)]
    pub fps: Option<f64>,
    #[serde(default)]
    pub duration_seconds: Option<f64>,
    #[serde(default)]
    pub preview_max_width: Option<u32>,
    #[serde(default)]
    pub preview_max_height: Option<u32>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct TrackPatch {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub muted: Option<bool>,
    #[serde(default)]
    pub volume: Option<f32>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct ClipPatch {
    #[serde(default)]
    pub track_id: Option<Uuid>,
    #[serde(default)]
    pub start_time: Option<f64>,
    #[serde(default)]
    pub duration: Option<f64>,
    #[serde(default)]
    pub trim_in_seconds: Option<f64>,
    #[serde(default)]
    pub volume: Option<f32>,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub image_mode: Option<ClipImageMode>,
    #[serde(default)]
    pub transform: Option<ClipTransform>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ClipMoveMode {
    Absolute,
    Relative,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct ClipMoveTarget {
    pub clip_id: Uuid,
    #[serde(default)]
    pub start_time: Option<f64>,
    #[serde(default)]
    pub track_id: Option<Uuid>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct MarkerPatch {
    #[serde(default)]
    pub track_id: Option<Uuid>,
    #[serde(default)]
    pub time: Option<f64>,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub color: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderTemplate {
    ComfyUi,
    #[serde(alias = "openai_image")]
    OpenAiImage,
    XaiImage,
    XaiVideo,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct GenerativeConfigPatch {
    #[serde(default)]
    pub provider_id: Option<Uuid>,
    #[serde(default)]
    pub inputs: Option<HashMap<String, InputValue>>,
    #[serde(default)]
    pub reference_slots: Option<HashMap<String, InputValue>>,
    #[serde(default)]
    pub batch: Option<BatchSettings>,
    #[serde(default)]
    pub active_version: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct AssetLabNodePatch {
    #[serde(default)]
    pub provider_id: Option<Option<Uuid>>,
    #[serde(default)]
    pub parent_node_id: Option<Option<Uuid>>,
    #[serde(default)]
    pub inputs: Option<HashMap<String, InputValue>>,
    #[serde(default)]
    pub output_version: Option<Option<String>>,
    #[serde(default)]
    pub selected: Option<bool>,
    #[serde(default)]
    pub replace: Option<AssetLabNode>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct ExportVideoRequest {
    #[serde(default)]
    pub output_path: Option<PathBuf>,
    #[serde(default)]
    pub codec: Option<VideoExportCodec>,
    #[serde(default)]
    pub width: Option<u32>,
    #[serde(default)]
    pub height: Option<u32>,
    #[serde(default)]
    pub fps: Option<f64>,
    #[serde(default)]
    pub start_seconds: Option<f64>,
    #[serde(default)]
    pub duration_seconds: Option<f64>,
    #[serde(default)]
    pub include_audio: Option<bool>,
    #[serde(default)]
    pub quality: Option<VideoExportQuality>,
    #[serde(default)]
    pub frame_format: Option<VideoExportFrameFormat>,
    #[serde(default)]
    pub timestamp_overlay: Option<TimestampOverlayRequest>,
    #[serde(default = "default_true")]
    pub open_panel: bool,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum I2vReference {
    Image,
    VideoFirstFrame,
    VideoLastFrame,
}

#[derive(Clone, Debug, Deserialize)]
pub struct TimestampOverlayRequest {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_timestamp_overlay_position")]
    pub position: TimestampOverlayPosition,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CaptureRequest {
    Frame {
        source: CaptureSource,
        #[serde(default)]
        time: Option<TimeSelector>,
        #[serde(default)]
        mode: CaptureMode,
        #[serde(default = "default_capture_format")]
        format: String,
        #[serde(default)]
        annotate: bool,
        #[serde(default)]
        seek_ui: bool,
        #[serde(default)]
        name: Option<String>,
    },
    Cutsheet {
        source: CaptureSource,
        frames: Vec<CaptureFrameRequest>,
        #[serde(default)]
        layout: CaptureSheetLayout,
        #[serde(default)]
        mode: CaptureMode,
        #[serde(default = "default_capture_format")]
        format: String,
        #[serde(default)]
        annotate: bool,
        #[serde(default)]
        seek_ui: bool,
        #[serde(default)]
        name: Option<String>,
    },
}

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CaptureSource {
    Timeline,
    Clip {
        clip_id: Uuid,
    },
    Asset {
        asset_id: Uuid,
        #[serde(default)]
        version: Option<String>,
    },
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct CaptureFrameRequest {
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub time: Option<TimeSelector>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct CaptureSheetLayout {
    #[serde(default = "default_cutsheet_columns")]
    pub columns: usize,
    #[serde(default = "default_cutsheet_thumb_width")]
    pub thumb_width: u32,
}

impl Default for CaptureSheetLayout {
    fn default() -> Self {
        Self {
            columns: default_cutsheet_columns(),
            thumb_width: default_cutsheet_thumb_width(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CaptureMode {
    #[default]
    Normal,
    Enhanced,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct TimeSelector {
    #[serde(default)]
    pub seconds: Option<f64>,
    #[serde(default)]
    pub frame: Option<i64>,
    #[serde(default)]
    pub percent: Option<f64>,
    #[serde(default)]
    pub key: Option<TimeKey>,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimeKey {
    First,
    Last,
    Current,
}

fn default_capture_format() -> String {
    "png".to_string()
}

fn default_true() -> bool {
    true
}

fn default_timestamp_overlay_position() -> TimestampOverlayPosition {
    TimestampOverlayPosition::BottomCenter
}

fn default_cutsheet_columns() -> usize {
    3
}

fn default_cutsheet_thumb_width() -> u32 {
    384
}

fn default_text_replace() -> bool {
    true
}

fn default_scrub_profile_steps() -> usize {
    24
}

fn default_scrub_profile_repeats() -> usize {
    1
}

/// Parse automation configuration from CLI args and environment variables.
pub fn config_from_args(args: &[String]) -> Option<AutomationConfig> {
    let env_enabled = std::env::var("LATENTSLATE_AUTOMATION")
        .map(|value| matches!(value.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"))
        .unwrap_or(false);

    let mut enabled = env_enabled;
    let mut port = std::env::var("LATENTSLATE_AUTOMATION_PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(DEFAULT_AUTOMATION_PORT);

    let mut index = 1;
    while index < args.len() {
        match args[index].as_str() {
            "--automation" => {
                enabled = true;
                index += 1;
            }
            "--automation-port" => {
                if let Some(value) = args.get(index + 1) {
                    if let Ok(parsed) = value.parse::<u16>() {
                        port = parsed;
                    }
                }
                index += 2;
            }
            _ => {
                index += 1;
            }
        }
    }

    enabled.then_some(AutomationConfig { port })
}

/// Start the loopback HTTP automation server and command queue.
pub fn start(config: AutomationConfig) -> Result<(), String> {
    if CONFIG.get().is_some() {
        ACTIVE.store(true, Ordering::Relaxed);
        return Ok(());
    }

    let address = format!("127.0.0.1:{}", config.port);
    let listener =
        TcpListener::bind(&address).map_err(|err| format!("Failed to bind {address}: {err}"))?;

    let (tx, rx) = mpsc::channel::<AutomationEnvelope>();
    COMMAND_TX
        .set(tx)
        .map_err(|_| "automation sender already initialized".to_string())?;
    COMMAND_RX
        .set(Mutex::new(rx))
        .map_err(|_| "automation receiver already initialized".to_string())?;
    CONFIG
        .set(config.clone())
        .map_err(|_| "automation config already initialized".to_string())?;

    std::thread::Builder::new()
        .name("latentslate-automation-http".to_string())
        .spawn(move || run_server(config, listener))
        .map_err(|err| err.to_string())?;

    ACTIVE.store(true, Ordering::Relaxed);
    Ok(())
}

/// Return whether the automation server has been initialized for this process.
pub fn is_enabled() -> bool {
    CONFIG.get().is_some()
}

/// Return whether non-health automation requests are currently allowed.
pub fn is_active() -> bool {
    is_enabled() && ACTIVE.load(Ordering::Relaxed)
}

/// Set whether non-health automation requests are currently allowed.
pub fn set_active(active: bool) {
    if is_enabled() {
        ACTIVE.store(active, Ordering::Relaxed);
    }
}

/// Return the currently configured automation port, if the server was started.
pub fn current_port() -> Option<u16> {
    CONFIG.get().map(|config| config.port)
}

/// Return the default localhost automation port.
pub fn default_port() -> u16 {
    DEFAULT_AUTOMATION_PORT
}

/// Poll one pending automation command for the app runtime to apply.
pub fn try_recv_command() -> Option<AutomationEnvelope> {
    let receiver = COMMAND_RX.get()?;
    let guard = receiver.lock().ok()?;
    guard.try_recv().ok()
}

/// Clear the current-frame registry before the egui tree is drawn.
pub fn begin_ui_frame() {
    if !is_enabled() {
        return;
    }
    if let Ok(mut registry) = ui_registry().lock() {
        registry.elements.clear();
        registry.consumed_actions.clear();
        registry.frame_index = registry.frame_index.saturating_add(1);
    }
}

/// Register a real egui response and consume any queued click targeting it.
pub fn instrument_response(
    mut response: Response,
    kind: &'static str,
    label: Option<String>,
    clickable: bool,
    editable: bool,
) -> Response {
    if !is_enabled() {
        return response;
    }

    let id = ui_element_id(response.id);
    let enabled = response.enabled();
    let senses_click = response.sense.senses_click();
    let element_clickable = clickable && senses_click;
    let element = UiElement {
        id: id.clone(),
        kind: kind.to_string(),
        label,
        rect: response.rect.into(),
        interact_rect: response.interact_rect.into(),
        enabled,
        clickable: element_clickable,
        editable,
        focused: response.has_focus(),
    };

    let mut consume_click = false;
    if let Ok(mut registry) = ui_registry().lock() {
        registry.elements.push(UiElementRecord { element });
        if enabled && element_clickable && registry.pending_clicks.remove(&id) {
            registry.consumed_actions.insert(id.clone());
            consume_click = true;
        }
    }

    if consume_click {
        response
            .flags
            .insert(egui::response::Flags::FAKE_PRIMARY_CLICKED);
        response.request_focus();
    }

    response
}

/// Apply a pending text operation to a text widget, if one targets this response.
pub fn apply_pending_text(response: &mut Response, value: &mut String) {
    if !is_enabled() {
        return;
    }
    let id = ui_element_id(response.id);
    let pending = {
        let Ok(mut registry) = ui_registry().lock() else {
            return;
        };
        registry.pending_text.remove(&id)
    };
    if let Some(pending) = pending {
        if pending.replace {
            *value = pending.text;
        } else {
            value.push_str(&pending.text);
        }
        response.mark_changed();
        response.request_focus();
        mark_action_consumed(&id);
    }
}

/// Return the visible UI registry from the last completed frame.
pub fn ui_snapshot() -> Vec<UiElement> {
    ui_registry()
        .lock()
        .map(|registry| {
            registry
                .elements
                .iter()
                .map(|record| record.element.clone())
                .collect()
        })
        .unwrap_or_default()
}

/// Find a visible UI element in the latest registry.
pub fn find_ui_element(id: &str) -> Option<UiElement> {
    ui_registry().lock().ok().and_then(|registry| {
        registry
            .elements
            .iter()
            .find(|record| record.element.id == id)
            .map(|record| record.element.clone())
    })
}

/// Queue a click for consumption by the widget during the next render pass.
pub fn queue_ui_click(id: String) {
    if let Ok(mut registry) = ui_registry().lock() {
        registry.pending_clicks.insert(id);
    }
}

/// Queue a text edit for consumption by the target text widget during the next render pass.
pub fn queue_ui_text(id: String, text: String, replace: bool) {
    if let Ok(mut registry) = ui_registry().lock() {
        registry
            .pending_text
            .insert(id, PendingText { text, replace });
    }
}

/// Consume a pending click before a widget with internal click handling is shown.
pub fn consume_pending_click_for_egui_id(egui_id: egui::Id) -> bool {
    let id = ui_element_id(egui_id);
    let Ok(mut registry) = ui_registry().lock() else {
        return false;
    };
    if registry.pending_clicks.remove(&id) {
        registry.consumed_actions.insert(id);
        true
    } else {
        false
    }
}

/// Return whether a queued UI action was consumed in the current frame.
pub fn was_action_consumed(id: &str) -> bool {
    ui_registry()
        .lock()
        .map(|registry| registry.consumed_actions.contains(id))
        .unwrap_or(false)
}

/// Remove any queued action targeting `id`.
pub fn clear_pending_ui_action(id: &str) {
    if let Ok(mut registry) = ui_registry().lock() {
        registry.pending_clicks.remove(id);
        registry.pending_text.remove(id);
        registry.consumed_actions.remove(id);
    }
}

/// Build a deterministic screenshot path under `.tmp/automation-screenshots`.
pub fn screenshot_path(name: Option<&str>) -> Result<PathBuf, String> {
    let root = std::env::current_dir()
        .map_err(|err| format!("Failed to resolve current directory: {err}"))?;
    let dir = root.join(".tmp").join("automation-screenshots");
    fs::create_dir_all(&dir).map_err(|err| {
        format!(
            "Failed to create automation screenshot directory {}: {err}",
            dir.display()
        )
    })?;
    let suffix = name
        .map(sanitize_file_stem)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "capture".to_string());
    let timestamp = chrono::Local::now().format("%Y%m%d-%H%M%S-%3f");
    Ok(dir.join(format!("automation-{timestamp}-{suffix}.png")))
}

/// Build a deterministic capture directory under `.tmp/agent-captures`.
pub fn agent_capture_dir(name: Option<&str>) -> Result<PathBuf, String> {
    let root = std::env::current_dir()
        .map_err(|err| format!("Failed to resolve current directory: {err}"))?;
    let parent = root.join(".tmp").join("agent-captures");
    fs::create_dir_all(&parent).map_err(|err| {
        format!(
            "Failed to create agent capture directory {}: {err}",
            parent.display()
        )
    })?;
    let suffix = name
        .map(sanitize_file_stem)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "capture".to_string());
    let timestamp = chrono::Local::now().format("%Y%m%d-%H%M%S-%3f");
    let dir = parent.join(format!("{timestamp}-{suffix}"));
    fs::create_dir_all(&dir).map_err(|err| {
        format!(
            "Failed to create agent capture directory {}: {err}",
            dir.display()
        )
    })?;
    Ok(dir)
}

fn ui_registry() -> &'static Mutex<UiRegistry> {
    UI_REGISTRY.get_or_init(|| Mutex::new(UiRegistry::default()))
}

fn ui_element_id(id: egui::Id) -> String {
    format!("{:016x}", id.value())
}

fn mark_action_consumed(id: &str) {
    if let Ok(mut registry) = ui_registry().lock() {
        registry.consumed_actions.insert(id.to_string());
    }
}

fn sanitize_file_stem(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

fn run_server(config: AutomationConfig, listener: TcpListener) {
    let address = format!("127.0.0.1:{}", config.port);
    eprintln!("[AUTOMATION] Listening on http://{}", address);
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                std::thread::spawn(move || handle_connection(stream));
            }
            Err(err) => {
                eprintln!("[AUTOMATION WARN] Incoming connection failed: {}", err);
            }
        }
    }
}

fn handle_connection(mut stream: TcpStream) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));

    let request = match read_request(&mut stream) {
        Ok(request) => request,
        Err(err) => {
            let response = AutomationResponse::error(err);
            let _ = write_json(&mut stream, 400, &response);
            return;
        }
    };
    if let Some(response) = request_guard_response(&request) {
        let _ = write_json(&mut stream, response.http_status, &response);
        return;
    }

    let is_public_request = matches!(
        (request.method.as_str(), request.path.as_str()),
        ("GET", "/health")
            | ("GET", "/agent/v1/health")
            | ("GET", "/agent/v1/capabilities")
            | ("GET", "/agent/v1/help")
            | ("GET", "/agent/v1/schema")
    );
    let response = if !is_public_request && !is_active() {
        AutomationResponse::with_status("Agent API is disabled.", 403)
    } else {
        match (request.method.as_str(), request.path.as_str()) {
            ("GET", "/health") => AutomationResponse::ok(json!({
                "enabled": is_active(),
                "server_started": is_enabled(),
                "port": CONFIG.get().map(|config| config.port),
            })),
            ("GET", "/agent/v1/health") => AutomationResponse::ok(json!({
                "api_version": "agent-v1",
                "enabled": is_active(),
                "server_started": is_enabled(),
                "port": CONFIG.get().map(|config| config.port),
                "bind": "127.0.0.1",
            })),
            ("GET", "/agent/v1/capabilities") => AutomationResponse::ok(agent_capabilities_json()),
            ("GET", "/agent/v1/help") => AutomationResponse::ok(agent_help_json()),
            ("GET", "/agent/v1/schema") => AutomationResponse::ok(agent_schema_json()),
            ("GET", "/agent/v1/projects") => {
                dispatch_command(AutomationCommand::ListProjects { root: None })
            }
            ("GET", "/agent/v1/state") => dispatch_command(AutomationCommand::GetState {
                include: query_values(request.query.as_deref(), "include"),
            }),
            ("GET", "/agent/v1/jobs") => dispatch_command(AutomationCommand::ListJobs),
            ("POST", "/agent/v1/wait/generation") => {
                match parse_generation_wait_request(&request) {
                    Ok(payload) => wait_for_generation(payload),
                    Err(err) => AutomationResponse::error(err),
                }
            }
            ("GET", "/agent/v1/export") => dispatch_command(AutomationCommand::GetExportStatus),
            ("GET", "/state") => dispatch_command(AutomationCommand::GetState { include: vec![] }),
            ("GET", "/ui") => dispatch_command(AutomationCommand::GetUi),
            ("POST", "/ui/click") => {
                match serde_json::from_slice::<UiClickRequest>(&request.body) {
                    Ok(payload) => dispatch_command(AutomationCommand::ClickUi { id: payload.id }),
                    Err(err) => {
                        AutomationResponse::error(format!("Invalid UI click JSON: {}", err))
                    }
                }
            }
            ("POST", "/ui/text") => match serde_json::from_slice::<UiTextRequest>(&request.body) {
                Ok(payload) => dispatch_command(AutomationCommand::TextUi {
                    id: payload.id,
                    text: payload.text,
                    replace: payload.replace,
                }),
                Err(err) => AutomationResponse::error(format!("Invalid UI text JSON: {}", err)),
            },
            ("POST", "/screenshot") => {
                let payload = if request.body.is_empty() {
                    Ok(ScreenshotRequest::default())
                } else {
                    serde_json::from_slice::<ScreenshotRequest>(&request.body)
                        .map_err(|err| format!("Invalid screenshot JSON: {}", err))
                };
                match payload {
                    Ok(payload) => {
                        dispatch_command(AutomationCommand::Screenshot { name: payload.name })
                    }
                    Err(err) => AutomationResponse::error(err),
                }
            }
            ("POST", "/command") | ("POST", "/agent/v1/command") => {
                match serde_json::from_slice::<AutomationCommand>(&request.body) {
                    Ok(command) => dispatch_command(command),
                    Err(err) => AutomationResponse::error(format!("Invalid command JSON: {}", err)),
                }
            }
            ("POST", "/agent/v1/capture") => {
                match serde_json::from_slice::<CaptureRequest>(&request.body) {
                    Ok(request) => dispatch_command(AutomationCommand::Capture { request }),
                    Err(err) => AutomationResponse::error(format!("Invalid capture JSON: {}", err)),
                }
            }
            ("GET", path) if path.starts_with("/agent/v1/jobs/") => {
                let id = path.trim_start_matches("/agent/v1/jobs/");
                match Uuid::parse_str(id) {
                    Ok(job_id) => dispatch_command(AutomationCommand::GetJob { job_id }),
                    Err(err) => AutomationResponse::error(format!("Invalid job ID: {}", err)),
                }
            }
            _ => AutomationResponse::error(format!(
                "Unsupported endpoint: {} {}",
                request.method, request.path
            )),
        }
    };

    let _ = write_json(&mut stream, response.http_status, &response);
}

fn parse_generation_wait_request(request: &HttpRequest) -> Result<GenerationWaitRequest, String> {
    if request.body.is_empty() {
        Ok(GenerationWaitRequest::default())
    } else {
        serde_json::from_slice::<GenerationWaitRequest>(&request.body)
            .map_err(|err| format!("Invalid generation wait JSON: {err}"))
    }
}

fn wait_for_generation(request: GenerationWaitRequest) -> AutomationResponse {
    let timeout_seconds = request
        .timeout_seconds
        .unwrap_or(DEFAULT_GENERATION_WAIT_TIMEOUT_SECONDS)
        .min(MAX_GENERATION_WAIT_TIMEOUT_SECONDS);
    let poll_interval = Duration::from_millis(
        request
            .poll_interval_ms
            .unwrap_or(DEFAULT_GENERATION_WAIT_POLL_MS)
            .clamp(MIN_GENERATION_WAIT_POLL_MS, MAX_GENERATION_WAIT_POLL_MS),
    );
    let deadline = Instant::now() + Duration::from_secs(timeout_seconds);

    match request.job_id {
        Some(job_id) => wait_for_generation_job(job_id, timeout_seconds, poll_interval, deadline),
        None => wait_for_generation_queue(timeout_seconds, poll_interval, deadline),
    }
}

fn wait_for_generation_job(
    job_id: Uuid,
    timeout_seconds: u64,
    poll_interval: Duration,
    deadline: Instant,
) -> AutomationResponse {
    loop {
        let response = dispatch_command(AutomationCommand::GetJob { job_id });
        if !response.ok {
            return response;
        }

        let job = response
            .data
            .get("job")
            .cloned()
            .unwrap_or_else(|| json!({}));
        let status = job
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if generation_job_status_is_terminal(status) {
            return AutomationResponse::ok(json!({
                "completed": true,
                "timed_out": false,
                "job_id": job_id,
                "job": job,
            }));
        }

        if Instant::now() >= deadline {
            return AutomationResponse::with_status_data(
                format!("Timed out waiting for generation job {job_id}."),
                408,
                json!({
                    "completed": false,
                    "timed_out": true,
                    "timeout_seconds": timeout_seconds,
                    "job_id": job_id,
                    "job": job,
                }),
            );
        }

        sleep_until_next_generation_wait_poll(poll_interval, deadline);
    }
}

fn wait_for_generation_queue(
    timeout_seconds: u64,
    poll_interval: Duration,
    deadline: Instant,
) -> AutomationResponse {
    loop {
        let response = dispatch_command(AutomationCommand::ListJobs);
        if !response.ok {
            return response;
        }

        let jobs = response
            .data
            .get("jobs")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        if generation_queue_is_idle(&jobs) {
            return AutomationResponse::ok(json!({
                "completed": true,
                "timed_out": false,
                "jobs": jobs,
            }));
        }

        if Instant::now() >= deadline {
            return AutomationResponse::with_status_data(
                "Timed out waiting for the generation queue.",
                408,
                json!({
                    "completed": false,
                    "timed_out": true,
                    "timeout_seconds": timeout_seconds,
                    "jobs": jobs,
                }),
            );
        }

        sleep_until_next_generation_wait_poll(poll_interval, deadline);
    }
}

fn sleep_until_next_generation_wait_poll(poll_interval: Duration, deadline: Instant) {
    let now = Instant::now();
    if now >= deadline {
        return;
    }
    std::thread::sleep(poll_interval.min(deadline.saturating_duration_since(now)));
}

fn generation_queue_is_idle(jobs: &[Value]) -> bool {
    jobs.iter().all(|job| {
        let status = job
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or_default();
        generation_job_status_is_terminal(status)
    })
}

fn generation_job_status_is_terminal(status: &str) -> bool {
    matches!(
        status.to_ascii_lowercase().as_str(),
        "succeeded" | "failed" | "canceled" | "cancelled"
    )
}

fn dispatch_command(command: AutomationCommand) -> AutomationResponse {
    let Some(tx) = COMMAND_TX.get() else {
        return AutomationResponse::error("Automation command queue is not initialized.");
    };

    let (response_tx, response_rx) = mpsc::channel::<AutomationResponse>();
    let timeout_seconds = match &command {
        AutomationCommand::ScrubTimelineProfile { .. } => PROFILE_RESPONSE_TIMEOUT_SECONDS,
        _ => RESPONSE_TIMEOUT_SECONDS,
    };
    let envelope = AutomationEnvelope {
        command,
        responder: response_tx,
    };
    if tx.send(envelope).is_err() {
        return AutomationResponse::error("Automation command queue is closed.");
    }

    response_rx
        .recv_timeout(Duration::from_secs(timeout_seconds))
        .unwrap_or_else(|_| AutomationResponse::error("Timed out waiting for app command result."))
}

struct HttpRequest {
    method: String,
    path: String,
    query: Option<String>,
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

impl HttpRequest {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .get(&name.to_ascii_lowercase())
            .map(String::as_str)
    }
}

fn read_request(stream: &mut TcpStream) -> Result<HttpRequest, String> {
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 1024];
    let header_end = loop {
        let read = stream.read(&mut chunk).map_err(|err| err.to_string())?;
        if read == 0 {
            return Err("Client closed connection before headers completed.".to_string());
        }
        buffer.extend_from_slice(&chunk[..read]);
        if let Some(index) = find_header_end(&buffer) {
            break index;
        }
        if buffer.len() > MAX_REQUEST_HEADER_BYTES {
            return Err("Request headers are too large.".to_string());
        }
    };

    let header_bytes = &buffer[..header_end];
    let header_text = String::from_utf8_lossy(header_bytes);
    let mut lines = header_text.lines();
    let request_line = lines
        .next()
        .ok_or_else(|| "Missing request line.".to_string())?;
    let mut parts = request_line.split_whitespace();
    let method = parts
        .next()
        .ok_or_else(|| "Missing request method.".to_string())?
        .to_string();
    let target = parts
        .next()
        .ok_or_else(|| "Missing request path.".to_string())?
        .to_string();
    let (path, query) = match target.split_once('?') {
        Some((path, query)) => (path.to_string(), Some(query.to_string())),
        None => (target, None),
    };

    let mut content_length = 0_usize;
    let mut headers = HashMap::new();
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let header_name = name.trim().to_ascii_lowercase();
        let header_value = value.trim().to_string();
        if header_name == "content-length" {
            content_length = header_value
                .parse::<usize>()
                .map_err(|_| "Invalid Content-Length header.".to_string())?;
        }
        headers.insert(header_name, header_value);
    }
    if content_length > MAX_REQUEST_BODY_BYTES {
        return Err(format!(
            "Request body is too large. Limit is {} bytes.",
            MAX_REQUEST_BODY_BYTES
        ));
    }

    let body_start = header_end + 4;
    let mut body = buffer.get(body_start..).unwrap_or_default().to_vec();
    if body.len() > MAX_REQUEST_BODY_BYTES {
        return Err(format!(
            "Request body is too large. Limit is {} bytes.",
            MAX_REQUEST_BODY_BYTES
        ));
    }
    while body.len() < content_length {
        let read = stream.read(&mut chunk).map_err(|err| err.to_string())?;
        if read == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..read]);
        if body.len() > MAX_REQUEST_BODY_BYTES {
            return Err(format!(
                "Request body is too large. Limit is {} bytes.",
                MAX_REQUEST_BODY_BYTES
            ));
        }
    }
    body.truncate(content_length);

    Ok(HttpRequest {
        method,
        path,
        query,
        headers,
        body,
    })
}

fn request_guard_response(request: &HttpRequest) -> Option<AutomationResponse> {
    if let Some(origin) = request.header("origin") {
        if !is_allowed_loopback_origin(origin) {
            return Some(AutomationResponse::with_status(
                "Browser Origin is not allowed for the agent API.",
                403,
            ));
        }
    }
    if request.method == "POST" && !request.body.is_empty() {
        let content_type = request.header("content-type").unwrap_or_default();
        let media_type = content_type
            .split(';')
            .next()
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase();
        if media_type != "application/json" {
            return Some(AutomationResponse::with_status(
                "POST requests with a body must use Content-Type: application/json.",
                415,
            ));
        }
    }
    None
}

fn is_allowed_loopback_origin(origin: &str) -> bool {
    let origin = origin.trim();
    if origin.eq_ignore_ascii_case("null") {
        return true;
    }
    let Some((scheme, rest)) = origin.split_once("://") else {
        return false;
    };
    if !scheme.eq_ignore_ascii_case("http") && !scheme.eq_ignore_ascii_case("https") {
        return false;
    }
    let authority = rest.split('/').next().unwrap_or_default();
    let host = if authority.starts_with('[') {
        authority
            .split(']')
            .next()
            .map(|value| format!("{value}]"))
            .unwrap_or_default()
    } else {
        authority
            .split('@')
            .next_back()
            .unwrap_or(authority)
            .split(':')
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase()
    };
    matches!(host.as_str(), "localhost" | "127.0.0.1" | "::1" | "[::1]")
}

fn find_header_end(buffer: &[u8]) -> Option<usize> {
    buffer.windows(4).position(|window| window == b"\r\n\r\n")
}

fn query_values(query: Option<&str>, key: &str) -> Vec<String> {
    let Some(query) = query else {
        return Vec::new();
    };
    query
        .split('&')
        .filter_map(|part| {
            let (name, value) = part.split_once('=').unwrap_or((part, ""));
            (name == key).then_some(value)
        })
        .flat_map(|value| value.split(','))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect()
}

/// Static command and transport metadata for agent bootstrap.
pub fn agent_capabilities_json() -> Value {
    json!({
        "api_version": "agent-v1",
        "self_documenting": {
            "help": "GET /agent/v1/help",
            "schema": "GET /agent/v1/schema",
            "command_endpoint": "POST /agent/v1/command",
            "capture_endpoint": "POST /agent/v1/capture",
            "wait_generation_endpoint": "POST /agent/v1/wait/generation"
        },
        "commands": agent_command_names(),
        "capture": {
            "sources": ["timeline", "clip", "asset"],
            "formats": ["png"],
            "modes": ["normal", "enhanced"],
            "cutsheet": true,
            "limits": {
                "max_cutsheet_frames": MAX_AGENT_CUTSHEET_FRAMES,
                "max_cutsheet_thumb_width": MAX_AGENT_CUTSHEET_THUMB_WIDTH
            }
        },
        "transport": {
            "bind": "127.0.0.1",
            "max_request_body_bytes": MAX_REQUEST_BODY_BYTES,
            "versioned_routes": true,
            "enabled_by": "API top-bar popover, --automation, or LATENTSLATE_AUTOMATION=1"
        }
    })
}

/// Human/agent-readable bootstrap help returned by `/agent/v1/help`.
pub fn agent_help_json() -> Value {
    json!({
        "title": "LatentSlate Agent API",
        "api_version": "agent-v1",
        "summary": "Localhost-only control API for operating a running LatentSlate editor through semantic commands, UI harness fallbacks, rendered captures, generation jobs, and export.",
        "quick_start": [
            "GET /agent/v1/health",
            "GET /agent/v1/schema",
            "GET /agent/v1/state?include=diagnostics",
            "POST /agent/v1/command with one JSON object whose type is a command name",
            "POST /agent/v1/capture for visual frames or cutsheets",
            "POST /agent/v1/wait/generation to block until one job or the whole queue completes"
        ],
        "routes": [
            { "method": "GET", "path": "/agent/v1/health", "purpose": "Check server, toggle, bind, and port." },
            { "method": "GET", "path": "/agent/v1/capabilities", "purpose": "Compact supported command and capture list." },
            { "method": "GET", "path": "/agent/v1/help", "purpose": "Bootstrap help, workflow hints, and examples." },
            { "method": "GET", "path": "/agent/v1/schema", "purpose": "Command, capture, enum, and response shapes." },
            { "method": "GET", "path": "/agent/v1/state?include=diagnostics", "purpose": "Read project, selection, providers, queue, layout, and optional diagnostics." },
            { "method": "GET", "path": "/agent/v1/projects", "purpose": "List project folders." },
            { "method": "GET", "path": "/agent/v1/jobs", "purpose": "List generation jobs." },
            { "method": "GET", "path": "/agent/v1/jobs/{job_id}", "purpose": "Read one generation job." },
            { "method": "POST", "path": "/agent/v1/wait/generation", "purpose": "Long-poll until one generation job reaches a terminal status, or until the whole queue is idle when job_id is omitted." },
            { "method": "GET", "path": "/agent/v1/export", "purpose": "Read export status." },
            { "method": "POST", "path": "/agent/v1/command", "purpose": "Apply one semantic editor command." },
            { "method": "POST", "path": "/agent/v1/capture", "purpose": "Render visual frame/cutsheet artifacts." }
        ],
        "response_envelope": {
            "ok": true,
            "message": null,
            "data": {}
        },
        "recommended_workflow": [
            "Read /agent/v1/health and /agent/v1/schema.",
            "Read /agent/v1/state to get IDs and current editor state.",
            "Use semantic commands first; use get_ui/click_ui/text_ui only for UI fallback.",
            "Set provider media parameters with inputs.<field> = { type: \"asset_ref\", asset_id: \"uuid\", pinned: true }. reference_slots are compatibility aliases and timeline hints.",
            "Use start_generation as a non-blocking enqueue step, then POST /agent/v1/wait/generation with a returned job_id when you need to synchronize.",
            "After state-changing commands, read state or capture an enhanced frame/cutsheet.",
            "Use export_video and poll /agent/v1/export for final validation."
        ],
        "examples": {
            "seek": { "type": "seek", "time": 4.25 },
            "set_project_settings": {
                "type": "set_project_settings",
                "patch": { "width": 1280, "height": 720, "fps": 24.0, "duration_seconds": 30.0 }
            },
            "add_marker": {
                "type": "add_marker",
                "time": 10.0,
                "label": "turn"
            },
            "wait_for_generation_job": {
                "endpoint": "POST /agent/v1/wait/generation",
                "body": { "job_id": "uuid", "timeout_seconds": 1800, "poll_interval_ms": 500 }
            },
            "wait_for_generation_queue": {
                "endpoint": "POST /agent/v1/wait/generation",
                "body": { "timeout_seconds": 1800 }
            },
            "set_i2i_input_asset": {
                "type": "set_generative_config",
                "asset_id": "uuid",
                "patch": {
                    "provider_id": "uuid",
                    "inputs": {
                        "nla_input_image": {
                            "type": "asset_ref",
                            "asset_id": "source-asset-uuid",
                            "pinned": true
                        },
                        "nla_pos_prompt": {
                            "type": "literal",
                            "value": "turn the input image into a polished product still"
                        }
                    }
                }
            },
            "capture_cutsheet": {
                "type": "cutsheet",
                "source": { "type": "timeline" },
                "frames": [
                    { "label": "start", "time": { "percent": 0.0 } },
                    { "label": "middle", "time": { "percent": 0.5 } },
                    { "label": "end", "time": { "key": "last" } }
                ],
                "layout": { "columns": 3, "thumb_width": 384 },
                "mode": "enhanced",
                "annotate": true
            }
        }
    })
}

/// Build copyable, skill-style text for handing this running editor to an agent.
pub fn build_agent_bootstrap(
    project: &Project,
    selection: &SelectionState,
    current_time: f64,
    provider_count: usize,
    job_count: usize,
) -> String {
    let port = current_port().unwrap_or_else(default_port);
    let status = if is_active() {
        "enabled"
    } else if is_enabled() {
        "server started, disabled by API popover toggle"
    } else {
        "not started"
    };
    let mut lines = vec![
        "LatentSlate Agent API Skill".to_string(),
        format!("Host: 127.0.0.1"),
        format!("Port: {port}"),
        format!("Base URL: http://127.0.0.1:{port}"),
        format!("Current API status: {status}"),
        "Scope: loopback-only (127.0.0.1); this API is not exposed to the network.".to_string(),
        "Transport: POST bodies must be application/json; request bodies are capped at 2 MiB."
            .to_string(),
        String::new(),
        "Usage".to_string(),
        "1. Call GET /agent/v1/health to confirm the server and API popover toggle.".to_string(),
        "2. Call GET /agent/v1/help and GET /agent/v1/schema to bootstrap command shapes."
            .to_string(),
        "3. Call GET /agent/v1/state?include=diagnostics to discover project IDs.".to_string(),
        "4. Prefer POST /agent/v1/command with semantic commands over UI clicks.".to_string(),
        "5. Use GET /ui plus POST /ui/click or /ui/text only as a fallback for visible widgets."
            .to_string(),
        "6. Use POST /agent/v1/capture for rendered timeline, clip, or asset frames/cutsheets."
            .to_string(),
        "7. Use POST /agent/v1/wait/generation to block on one generation job, or the whole queue when no job_id is supplied."
            .to_string(),
        "8. Use POST /screenshot only for viewport/UI debugging.".to_string(),
        "9. After state-changing commands, re-read state or request an enhanced capture.".to_string(),
        String::new(),
        "Endpoint Topology".to_string(),
        "- GET /agent/v1/health".to_string(),
        "- GET /agent/v1/capabilities".to_string(),
        "- GET /agent/v1/help".to_string(),
        "- GET /agent/v1/schema".to_string(),
        "- GET /agent/v1/state?include=diagnostics".to_string(),
        "- GET /agent/v1/projects".to_string(),
        "- GET /agent/v1/jobs".to_string(),
        "- GET /agent/v1/jobs/{job_id}".to_string(),
        "- POST /agent/v1/wait/generation".to_string(),
        "- GET /agent/v1/export".to_string(),
        "- POST /agent/v1/command".to_string(),
        "- POST /agent/v1/capture".to_string(),
        "- GET /ui".to_string(),
        "- POST /ui/click".to_string(),
        "- POST /ui/text".to_string(),
        "- POST /screenshot".to_string(),
        String::new(),
        "High-Value Commands".to_string(),
        "- Project: list_projects, create_project, open_project, save_project, set_project_settings"
            .to_string(),
        "- Timeline: seek, set_playback, step_timeline, add_asset_to_timeline, move_clip, move_clips, set_clip"
            .to_string(),
        "- Tracks/markers: add_track, set_track, move_track, delete_track, add_marker, set_marker"
            .to_string(),
        "- Assets: import_asset, rename_asset, duplicate_asset, delete_assets, extract_still_to_asset"
            .to_string(),
        "- Providers/credentials: list_providers, create_provider, update_provider, test_provider, set_credential"
            .to_string(),
        "- Generation: create_generative_asset, set_generative_config, start_generation, wait_for_generation endpoint, cancel_job"
            .to_string(),
        "- Asset Lab: get_asset_lab_graph, add_asset_lab_node, set_asset_lab_node, generate_asset_lab_node"
            .to_string(),
        "- Export: export_video, get_export_status, cancel_export".to_string(),
        String::new(),
        "Current Project".to_string(),
        format!("Name: {}", project.name),
        format!(
            "Path: {}",
            project
                .project_path
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "(not saved)".to_string())
        ),
        format!(
            "Settings: {}x{} @ {:.3} fps, duration {:.3}s",
            project.settings.width,
            project.settings.height,
            project.settings.fps,
            project.settings.duration_seconds
        ),
        format!("Current time: {:.3}s", current_time),
        format!(
            "Counts: providers={}, jobs={}, tracks={}, assets={}, clips={}, markers={}, generative_configs={}",
            provider_count,
            job_count,
            project.tracks.len(),
            project.assets.len(),
            project.clips.len(),
            project.markers.len(),
            project.generative_configs.len()
        ),
        String::new(),
        "Current Selection".to_string(),
        format!("Assets: {}", uuid_list_or_empty(&selection.asset_ids)),
        format!("Clips: {}", uuid_list_or_empty(&selection.clip_ids)),
        format!("Tracks: {}", uuid_list_or_empty(&selection.track_ids)),
        format!("Markers: {}", uuid_list_or_empty(&selection.marker_ids)),
        String::new(),
        "Known Tracks".to_string(),
    ];

    if project.tracks.is_empty() {
        lines.push("- None".to_string());
    } else {
        for track in project.tracks.iter().take(12) {
            lines.push(format!(
                "- id={} | type={:?} | name={} | muted={} | volume={:.2}",
                track.id, track.track_type, track.name, track.muted, track.volume
            ));
        }
    }

    lines.push(String::new());
    lines.push("Known Assets".to_string());
    if project.assets.is_empty() {
        lines.push("- None".to_string());
    } else {
        for asset in project.assets.iter().take(12) {
            lines.push(format!(
                "- id={} | type={} | name={} | duration={}",
                asset.id,
                asset_kind_label(asset),
                asset.name,
                asset
                    .duration_seconds
                    .map(|duration| format!("{duration:.3}s"))
                    .unwrap_or_else(|| "unknown".to_string())
            ));
        }
        if project.assets.len() > 12 {
            lines.push(format!(
                "- ... {} more assets; call GET /agent/v1/state for the full list.",
                project.assets.len() - 12
            ));
        }
    }

    lines.push(String::new());
    lines.push("Known Clips".to_string());
    if project.clips.is_empty() {
        lines.push("- None".to_string());
    } else {
        for clip in project.clips.iter().take(12) {
            lines.push(format!(
                "- id={} | asset_id={} | track_id={} | start={:.3}s | duration={:.3}s | label={}",
                clip.id,
                clip.asset_id,
                clip.track_id,
                clip.start_time,
                clip.duration,
                clip.label.as_deref().unwrap_or("")
            ));
        }
        if project.clips.len() > 12 {
            lines.push(format!(
                "- ... {} more clips; call GET /agent/v1/state for the full list.",
                project.clips.len() - 12
            ));
        }
    }

    lines.push(String::new());
    lines.push("Notes".to_string());
    if !is_active() {
        lines.push(
            "- The API is currently disabled for read/write editor routes. Enable the top-right API popover before calling state or command endpoints."
                .to_string(),
        );
    }
    lines.push("- Provider secrets are write-only/redacted in API responses.".to_string());
    lines.push(
        "- Rendered captures are saved under .tmp/agent-captures and return absolute paths."
            .to_string(),
    );
    lines.push(
        "- Use enhanced capture mode when you need visual clip boundaries and inspection overlays."
            .to_string(),
    );

    lines.join("\n")
}

fn uuid_list_or_empty(ids: &[Uuid]) -> String {
    if ids.is_empty() {
        "(none)".to_string()
    } else {
        ids.iter()
            .map(Uuid::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    }
}

fn asset_kind_label(asset: &Asset) -> &'static str {
    match &asset.kind {
        AssetKind::Video { .. } => "video",
        AssetKind::Image { .. } => "image",
        AssetKind::Audio { .. } => "audio",
        AssetKind::GenerativeVideo { .. } => "generative_video",
        AssetKind::GenerativeImage { .. } => "generative_image",
        AssetKind::GenerativeAudio { .. } => "generative_audio",
    }
}

/// JSON schema-style reference for agent command construction.
pub fn agent_schema_json() -> Value {
    json!({
        "api_version": "agent-v1",
        "envelope": {
            "success": { "ok": true, "data": "command-specific payload" },
            "error": { "ok": false, "message": "human-readable error" }
        },
        "command_endpoint": {
            "method": "POST",
            "path": "/agent/v1/command",
            "body": { "type": "command_name", "...": "command-specific fields" }
        },
        "capture_endpoint": {
            "method": "POST",
            "path": "/agent/v1/capture",
            "body": { "type": "frame|cutsheet", "...": "capture-specific fields" }
        },
        "wait_generation_endpoint": {
            "method": "POST",
            "path": "/agent/v1/wait/generation",
            "body": {
                "job_id?": "uuid; omit to wait until the entire generation queue is idle",
                "timeout_seconds?": "u64, default 1800, max 86400",
                "poll_interval_ms?": "u64, default 500, clamped 100..5000"
            },
            "success_data": {
                "completed": true,
                "timed_out": false,
                "job?": "present when job_id was supplied",
                "jobs?": "present when waiting for the whole queue"
            },
            "timeout": {
                "http_status": 408,
                "data": "current job or queue snapshot plus timed_out=true"
            }
        },
        "commands": agent_command_schema_json(),
        "input_value": {
            "literal": { "type": "literal", "value": "json value" },
            "asset_ref": {
                "type": "asset_ref",
                "asset_id": "uuid",
                "source_clip_id?": "uuid",
                "pinned?": "bool, default true; false lets context_clip_id choose a nearby timeline source",
                "frame_reference?": "first|last"
            },
            "generation_ref": {
                "type": "generation_ref",
                "asset_id": "uuid",
                "version": "generation version such as v1",
                "frame_reference?": "first|last"
            }
        },
        "capture": capture_schema_json(),
        "provider_entry": {
            "fields": {
                "id": "uuid",
                "name": "string",
                "description?": "optional multi-line provider guidance for humans and agents",
                "output_type": "image|video|audio",
                "workflow_kind?": "provider workflow kind such as text_to_image or image_to_video",
                "inputs": ["ProviderInputField"],
                "connection": "ProviderConnection"
            }
        },
        "provider_input_field": {
            "fields": {
                "name": "string",
                "label": "string",
                "description?": "optional multi-line parameter guidance for humans and agents",
                "input_type": "ProviderInputType",
                "required?": "bool",
                "default?": "json value",
                "role?": "width|height|seed",
                "ui?": "InputUi"
            }
        },
        "enums": {
            "track_type": ["Video", "Audio", "Marker"],
            "provider_output_type": ["image", "video", "audio"],
            "provider_template": ["comfy_ui", "openai_image", "xai_image", "xai_video"],
            "capture_mode": ["normal", "enhanced"],
            "time_key": ["first", "last", "current"],
            "i2v_reference": ["image", "video_first_frame", "video_last_frame"],
            "export_codec": ["h264", "h265"],
            "export_quality": ["compact", "balanced", "high", "near_lossless"],
            "export_frame_format": ["png", "bmp"],
            "timestamp_overlay_position": ["top_center", "bottom_center"]
        },
        "notes": [
            "UUID fields can be discovered from /agent/v1/state.",
            "Secrets are write-only: set_credential never returns the secret value.",
            "Provider and provider-input descriptions are returned with provider metadata and should guide tool selection and parameter values.",
            "start_generation is non-blocking; use /agent/v1/wait/generation to wait for a returned job_id or for the queue to drain.",
            "Read-only captures do not move the UI unless seek_ui is true.",
            "Use enhanced capture mode when visual boundaries, labels, and timecode help inspection."
        ]
    })
}

fn agent_command_names() -> Vec<&'static str> {
    vec![
        "get_state",
        "get_capabilities",
        "get_ui",
        "click_ui",
        "text_ui",
        "screenshot",
        "list_projects",
        "create_project",
        "open_project",
        "save_project",
        "set_project_settings",
        "import_asset",
        "rename_asset",
        "duplicate_asset",
        "delete_assets",
        "set_asset_duration",
        "create_generative_asset",
        "create_i2i_from_clip",
        "create_i2v_from_clip",
        "create_bridge_from_clips",
        "extract_active_generation",
        "extract_generation_version",
        "extract_still_to_asset",
        "add_asset_to_timeline",
        "seek",
        "set_playback",
        "step_timeline",
        "set_selection",
        "select_asset",
        "select_clip",
        "select_track",
        "select_marker",
        "add_track",
        "set_track",
        "move_track",
        "delete_track",
        "set_clip",
        "move_clip",
        "move_clips",
        "resize_clip",
        "delete_clips",
        "add_marker",
        "set_marker",
        "delete_marker",
        "list_providers",
        "refresh_providers",
        "create_provider_from_template",
        "create_provider",
        "update_provider",
        "delete_provider",
        "test_provider",
        "get_credential_status",
        "set_credential",
        "delete_credential",
        "get_generative_config",
        "set_generative_config",
        "replace_generative_config",
        "set_active_generation_version",
        "duplicate_generation_version",
        "delete_generation_version",
        "get_asset_lab_graph",
        "add_asset_lab_node",
        "set_asset_lab_node",
        "delete_asset_lab_node",
        "generate_asset_lab_node",
        "start_generation",
        "list_jobs",
        "get_job",
        "cancel_job",
        "export_video",
        "get_export_status",
        "cancel_export",
        "capture",
        "get_performance_diagnostics",
        "scrub_timeline_profile",
        "open_providers",
        "close_providers",
        "open_project_settings",
        "close_project_settings",
        "open_new_project",
        "close_new_project",
        "open_queue",
        "close_queue",
        "open_generative_video",
        "close_generative_video",
        "open_export_video",
        "close_export_video",
        "set_layout",
        "close_all_overlays",
    ]
}

fn agent_command_schema_json() -> Value {
    json!({
        "project": [
            { "type": "list_projects", "fields": { "root?": "folder path" } },
            { "type": "create_project", "fields": { "parent_dir": "folder path", "name": "string", "settings?": "ProjectSettings" } },
            { "type": "open_project", "fields": { "folder": "project folder path" } },
            { "type": "save_project", "fields": {} },
            { "type": "set_project_settings", "fields": { "patch": { "width?": "u32", "height?": "u32", "fps?": "f64", "duration_seconds?": "f64", "preview_max_width?": "u32", "preview_max_height?": "u32" } } }
        ],
        "assets": [
            { "type": "import_asset", "fields": { "path": "media file path" } },
            { "type": "rename_asset", "fields": { "asset_id?": "uuid", "asset_name?": "string", "name": "string" } },
            { "type": "duplicate_asset", "fields": { "asset_id?": "uuid", "asset_name?": "string" } },
            { "type": "delete_assets", "fields": { "asset_ids": ["uuid"] } },
            { "type": "set_asset_duration", "fields": { "asset_id": "uuid", "duration_seconds": "f64|null" } },
            { "type": "extract_active_generation", "fields": { "asset_id?": "uuid", "asset_name?": "string" } },
            { "type": "extract_generation_version", "fields": { "asset_id": "uuid", "version?": "string" } },
            { "type": "extract_still_to_asset", "fields": { "source": "CaptureSource", "time?": "TimeSelector", "name?": "string" } }
        ],
        "timeline": [
            { "type": "add_asset_to_timeline", "fields": { "asset_id?": "uuid", "asset_name?": "string", "track_id?": "uuid", "time?": "seconds", "duration_seconds?": "seconds" } },
            { "type": "seek", "fields": { "time": "seconds" } },
            { "type": "set_playback", "fields": { "playing": "bool" } },
            { "type": "step_timeline", "fields": { "frames": "signed frame count" } },
            { "type": "set_selection", "fields": { "assets": ["uuid"], "clips": ["uuid"], "tracks": ["uuid"], "markers": ["uuid"] } },
            { "type": "select_asset|select_clip|select_track|select_marker", "fields": { "asset_id/clip_id/track_id/marker_id?": "uuid", "index?": "usize" } },
            { "type": "add_track", "fields": { "track_type": "Video|Audio|Marker", "index?": "usize", "name?": "string" } },
            { "type": "set_track", "fields": { "track_id": "uuid", "patch": { "name?": "string", "muted?": "bool", "volume?": "0.0..4.0" } } },
            { "type": "move_track", "fields": { "track_id": "uuid", "index": "usize" } },
            { "type": "delete_track", "fields": { "track_id": "uuid", "dry_run?": "bool" } },
            { "type": "set_clip", "fields": { "clip_id": "uuid", "patch": "ClipPatch" } },
            { "type": "move_clip", "fields": { "clip_id": "uuid", "start_time": "seconds", "track_id?": "uuid" } },
            { "type": "move_clips", "fields": {
                "absolute": { "mode": "absolute", "moves": [{ "clip_id": "uuid", "start_time?": "seconds", "track_id?": "uuid" }] },
                "relative": { "mode": "relative", "clip_ids": ["uuid"], "delta_seconds?": "seconds", "track_delta?": "signed track index delta", "track_id?": "uuid target track for all clips" }
            } },
            { "type": "resize_clip", "fields": { "clip_id": "uuid", "start_time": "seconds", "duration": "seconds" } },
            { "type": "delete_clips", "fields": { "clip_ids": ["uuid"] } },
            { "type": "add_marker", "fields": { "time?": "seconds", "track_id?": "uuid", "label?": "string" } },
            { "type": "set_marker", "fields": { "marker_id": "uuid", "patch": { "track_id?": "uuid", "time?": "seconds", "label?": "string", "description?": "string", "color?": "#rrggbb" } } },
            { "type": "delete_marker", "fields": { "marker_id": "uuid" } }
        ],
        "providers": [
            { "type": "list_providers", "fields": {} },
            { "type": "refresh_providers", "fields": {} },
            { "type": "create_provider_from_template", "fields": { "template": "comfy_ui|openai_image|xai_image|xai_video" } },
            { "type": "create_provider", "fields": { "provider": "ProviderEntry" } },
            { "type": "update_provider", "fields": { "provider_id": "uuid", "provider": "ProviderEntry" } },
            { "type": "delete_provider", "fields": { "provider_id": "uuid" } },
            { "type": "test_provider", "fields": { "provider_id": "uuid", "live?": "bool" } },
            { "type": "get_credential_status", "fields": { "credential_ids": ["openai_api_key", "xai_api_key"] } },
            { "type": "set_credential", "fields": { "credential_id": "string", "label": "string", "value": "secret string" } },
            { "type": "delete_credential", "fields": { "credential_id": "string" } }
        ],
        "generation": [
            { "type": "create_generative_asset", "fields": { "output_type": "image|video|audio", "name?": "string", "fps?": "f64", "frame_count?": "u32" } },
            { "type": "get_generative_config", "fields": { "asset_id": "uuid" } },
            { "type": "set_generative_config", "fields": { "asset_id": "uuid", "patch": { "provider_id?": "uuid", "inputs?": "map of provider field name to InputValue; canonical for literal and media provider parameters", "reference_slots?": "compatibility/timeline-hint map; media slots matching a provider field name or semantic slots like image/start_image/end_image are copied into inputs when inputs.<field> is absent", "batch?": "BatchSettings", "active_version?": "string" } } },
            { "type": "replace_generative_config", "fields": { "asset_id": "uuid", "config": "GenerativeConfig" } },
            { "type": "start_generation", "fields": { "asset_id": "uuid", "context_clip_id?": "uuid", "wait?": "bool" } },
            { "type": "list_jobs|get_job|cancel_job", "fields": { "job_id?": "uuid" } },
            { "type": "set_active_generation_version|duplicate_generation_version|delete_generation_version", "fields": { "asset_id": "uuid", "version": "string" } },
            { "type": "create_i2i_from_clip", "fields": { "clip_id": "uuid", "provider_id?": "uuid" } },
            { "type": "create_i2v_from_clip", "fields": { "clip_id": "uuid", "reference": "image|video_first_frame|video_last_frame", "provider_id?": "uuid" } },
            { "type": "create_bridge_from_clips", "fields": { "clip_ids": ["uuid"], "provider_id?": "uuid" } }
        ],
        "asset_lab": [
            { "type": "get_asset_lab_graph", "fields": { "asset_id": "uuid" } },
            { "type": "add_asset_lab_node", "fields": { "asset_id": "uuid", "provider_id?": "uuid", "parent_node_id?": "uuid", "inputs?": "map" } },
            { "type": "set_asset_lab_node", "fields": { "asset_id": "uuid", "node_id": "uuid", "patch": "AssetLabNodePatch" } },
            { "type": "delete_asset_lab_node", "fields": { "asset_id": "uuid", "node_id": "uuid" } },
            { "type": "generate_asset_lab_node", "fields": { "asset_id": "uuid", "node_id": "uuid" } }
        ],
        "export_and_diagnostics": [
            { "type": "export_video", "fields": { "request?": "ExportVideoRequest" } },
            { "type": "get_export_status", "fields": {} },
            { "type": "cancel_export", "fields": {} },
            { "type": "capture", "fields": { "request": "CaptureRequest; same body shape accepted by POST /agent/v1/capture" } },
            { "type": "get_performance_diagnostics", "fields": {} },
            { "type": "scrub_timeline_profile", "fields": { "start_time": "seconds", "end_time": "seconds", "steps?": "usize", "repeats?": "usize", "scrub_audio?": "bool", "settle_ms?": "u64" } }
        ],
        "ui_fallback": [
            { "type": "get_ui", "fields": {} },
            { "type": "click_ui", "fields": { "id": "ui element id from get_ui" } },
            { "type": "text_ui", "fields": { "id": "ui element id from get_ui", "text": "string", "replace?": "bool" } },
            { "type": "screenshot", "fields": { "name?": "string" } },
            { "type": "open_providers|close_providers|open_project_settings|close_project_settings|open_new_project|close_new_project|open_queue|close_queue|open_generative_video|close_generative_video|open_export_video|close_export_video|close_all_overlays", "fields": {} },
            { "type": "set_layout", "fields": { "left_collapsed?": "bool", "right_collapsed?": "bool", "timeline_collapsed?": "bool", "preview_stats?": "bool", "hardware_decode?": "bool", "left_width?": "f32", "right_width?": "f32", "timeline_height?": "f32", "timeline_zoom?": "f32", "timeline_scroll_x?": "f32", "timeline_scroll_y?": "f32" } }
        ]
    })
}

fn capture_schema_json() -> Value {
    json!({
        "time_selector": {
            "seconds": "absolute seconds",
            "frame": "frame number at project fps",
            "percent": "0.0..1.0 within timeline/clip/asset scope",
            "key": "first|last|current"
        },
        "source": {
            "timeline": { "type": "timeline" },
            "clip": { "type": "clip", "clip_id": "uuid" },
            "asset": { "type": "asset", "asset_id": "uuid", "version?": "generation version" }
        },
        "frame": {
            "type": "frame",
            "source": { "type": "timeline" },
            "time?": { "seconds": 4.25 },
            "mode?": "normal|enhanced",
            "format?": "png",
            "annotate?": "bool",
            "seek_ui?": "bool",
            "name?": "folder slug"
        },
        "cutsheet": {
            "type": "cutsheet",
            "source": { "type": "timeline" },
            "frames": [
                { "label": "start", "time": { "percent": 0.0 } },
                { "label": "middle", "time": { "percent": 0.5 } },
                { "label": "end", "time": { "key": "last" } }
            ],
            "layout?": { "columns": 3, "thumb_width": 384 },
            "limits": {
                "max_frames": MAX_AGENT_CUTSHEET_FRAMES,
                "max_thumb_width": MAX_AGENT_CUTSHEET_THUMB_WIDTH
            },
            "mode?": "normal|enhanced",
            "annotate?": "bool",
            "seek_ui?": "bool",
            "name?": "folder slug"
        }
    })
}

fn write_json(
    stream: &mut TcpStream,
    status: u16,
    response: &AutomationResponse,
) -> std::io::Result<()> {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        408 => "Request Timeout",
        409 => "Conflict",
        415 => "Unsupported Media Type",
        500 => "Internal Server Error",
        _ => "OK",
    };
    let body = serde_json::to_vec_pretty(response).unwrap_or_else(|_| b"{}".to_vec());
    write!(
        stream,
        "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        status,
        reason,
        body.len()
    )?;
    stream.write_all(&body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_values_supports_repeated_and_comma_separated_values() {
        let values = query_values(
            Some("include=diagnostics,providers&foo=bar&include=queue"),
            "include",
        );
        assert_eq!(values, vec!["diagnostics", "providers", "queue"]);
    }

    #[test]
    fn query_values_ignores_empty_values_and_other_keys() {
        let values = query_values(
            Some("include=&other=diagnostics&include=  ,queue"),
            "include",
        );
        assert_eq!(values, vec!["queue"]);
    }

    #[test]
    fn agent_schema_includes_bootstrap_routes_and_extract_still() {
        let capabilities = agent_capabilities_json();
        let commands = capabilities["commands"].as_array().expect("commands array");
        assert!(commands
            .iter()
            .any(|command| command.as_str() == Some("extract_still_to_asset")));

        let help = agent_help_json();
        let routes = help["routes"].as_array().expect("routes array");
        assert!(routes
            .iter()
            .any(|route| route["path"].as_str() == Some("/agent/v1/schema")));
    }

    #[test]
    fn schema_capture_endpoint_matches_implemented_capture_types() {
        let schema = agent_schema_json();
        assert_eq!(
            schema["capture_endpoint"]["body"]["type"].as_str(),
            Some("frame|cutsheet")
        );
        assert_eq!(
            schema["wait_generation_endpoint"]["path"].as_str(),
            Some("/agent/v1/wait/generation")
        );
        assert_eq!(
            schema["capture"]["cutsheet"]["limits"]["max_frames"].as_u64(),
            Some(MAX_AGENT_CUTSHEET_FRAMES as u64)
        );
        let export_and_diagnostics = schema["commands"]["export_and_diagnostics"]
            .as_array()
            .expect("export and diagnostics commands");
        assert!(export_and_diagnostics
            .iter()
            .any(|command| command["type"].as_str() == Some("capture")));
    }

    #[test]
    fn agent_help_examples_use_runtime_command_shapes() {
        let help = agent_help_json();
        let seek = help["examples"]["seek"].clone();
        assert!(matches!(
            serde_json::from_value::<AutomationCommand>(seek),
            Ok(AutomationCommand::Seek { .. })
        ));
        let marker = help["examples"]["add_marker"].clone();
        assert!(matches!(
            serde_json::from_value::<AutomationCommand>(marker),
            Ok(AutomationCommand::AddMarker { .. })
        ));
        let extract = json!({
            "type": "extract_still_to_asset",
            "source": { "type": "asset", "asset_id": Uuid::nil(), "version": "v1" },
            "time": { "percent": 0.5 }
        });
        assert!(matches!(
            serde_json::from_value::<AutomationCommand>(extract),
            Ok(AutomationCommand::ExtractStillToAsset { .. })
        ));
    }

    #[test]
    fn start_generation_wait_defaults_to_false() {
        let asset_id = Uuid::new_v4();
        let command = serde_json::from_value::<AutomationCommand>(json!({
            "type": "start_generation",
            "asset_id": asset_id
        }))
        .expect("minimal start_generation should deserialize");
        assert!(matches!(
            command,
            AutomationCommand::StartGeneration {
                asset_id: parsed_asset_id,
                context_clip_id: None,
                wait: false,
            } if parsed_asset_id == asset_id
        ));
    }

    #[test]
    fn agent_bootstrap_primer_includes_live_context_and_bootstrap_routes() {
        let project = Project::new("Primer Project");
        let primer = build_agent_bootstrap(&project, &SelectionState::default(), 3.25, 2, 1);
        assert!(primer.contains("LatentSlate Agent API Skill"));
        assert!(primer.contains("Base URL: http://127.0.0.1:"));
        assert!(primer.contains("GET /agent/v1/health"));
        assert!(primer.contains("GET /agent/v1/schema"));
        assert!(primer.contains("POST /agent/v1/wait/generation"));
        assert!(primer.contains("POST /agent/v1/command"));
        assert!(primer.contains("GET /ui"));
        assert!(primer.contains("POST /ui/click"));
        assert!(primer.contains("Name: Primer Project"));
        assert!(primer.contains("providers=2"));
        assert!(primer.contains("jobs=1"));
    }

    #[test]
    fn loopback_origin_guard_allows_only_local_browser_origins() {
        assert!(is_allowed_loopback_origin("null"));
        assert!(is_allowed_loopback_origin("http://localhost:47890"));
        assert!(is_allowed_loopback_origin("https://127.0.0.1:47890"));
        assert!(is_allowed_loopback_origin("http://[::1]:47890"));
        assert!(!is_allowed_loopback_origin("https://example.com"));
        assert!(!is_allowed_loopback_origin("http://localhost.example.com"));
    }

    #[test]
    fn generation_wait_status_helpers_match_runtime_status_json() {
        assert!(generation_job_status_is_terminal("Succeeded"));
        assert!(generation_job_status_is_terminal("Failed"));
        assert!(generation_job_status_is_terminal("Canceled"));
        assert!(!generation_job_status_is_terminal("Queued"));
        assert!(!generation_job_status_is_terminal("Running"));

        assert!(generation_queue_is_idle(&[
            json!({ "status": "Succeeded" }),
            json!({ "status": "Failed" }),
        ]));
        assert!(!generation_queue_is_idle(&[
            json!({ "status": "Succeeded" }),
            json!({ "status": "Running" }),
        ]));
    }

    #[test]
    fn post_body_guard_requires_json_content_type() {
        let mut headers = HashMap::new();
        let request = HttpRequest {
            method: "POST".to_string(),
            path: "/agent/v1/command".to_string(),
            query: None,
            headers: headers.clone(),
            body: b"{}".to_vec(),
        };
        let response = request_guard_response(&request).expect("missing content-type rejected");
        assert_eq!(response.http_status, 415);

        headers.insert(
            "content-type".to_string(),
            "application/json; charset=utf-8".to_string(),
        );
        let request = HttpRequest {
            method: "POST".to_string(),
            path: "/agent/v1/command".to_string(),
            query: None,
            headers,
            body: b"{}".to_vec(),
        };
        assert!(request_guard_response(&request).is_none());
    }
}
