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
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use uuid::Uuid;

use crate::state::ProjectSettings;

const DEFAULT_AUTOMATION_PORT: u16 = 47_890;
const RESPONSE_TIMEOUT_SECONDS: u64 = 20;
const PROFILE_RESPONSE_TIMEOUT_SECONDS: u64 = 120;

static CONFIG: OnceLock<AutomationConfig> = OnceLock::new();
static COMMAND_TX: OnceLock<Sender<AutomationEnvelope>> = OnceLock::new();
static COMMAND_RX: OnceLock<Mutex<Receiver<AutomationEnvelope>>> = OnceLock::new();
static UI_REGISTRY: OnceLock<Mutex<UiRegistry>> = OnceLock::new();

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
    GetState,
    /// Return the current visible UI registry.
    GetUi,
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
    /// Extract a generative asset's active output as a normal project asset.
    ExtractActiveGeneration {
        #[serde(default)]
        asset_id: Option<Uuid>,
        #[serde(default)]
        asset_name: Option<String>,
    },
    /// Add an asset to the timeline, resolving by ID, name, or first asset.
    AddAssetToTimeline {
        #[serde(default)]
        asset_id: Option<Uuid>,
        #[serde(default)]
        asset_name: Option<String>,
        #[serde(default)]
        time: Option<f64>,
    },
    /// Seek the playhead to a timestamp in seconds.
    Seek { time: f64 },
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
    /// Add a marker at the provided time or current playhead.
    AddMarker {
        #[serde(default)]
        time: Option<f64>,
    },
    /// Save the current project.
    SaveProject,
    /// Open the global providers modal.
    OpenProviders,
    /// Close the global providers modal.
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

#[derive(Default, Debug, Deserialize)]
struct ScreenshotRequest {
    #[serde(default)]
    name: Option<String>,
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
        .spawn(move || run_server(config))
        .map_err(|err| err.to_string())?;

    Ok(())
}

/// Return whether automation mode is enabled for this process.
pub fn is_enabled() -> bool {
    CONFIG.get().is_some()
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

fn run_server(config: AutomationConfig) {
    let address = format!("127.0.0.1:{}", config.port);
    let listener = match TcpListener::bind(&address) {
        Ok(listener) => listener,
        Err(err) => {
            eprintln!("[AUTOMATION ERROR] Failed to bind {}: {}", address, err);
            return;
        }
    };

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

    let response = match (request.method.as_str(), request.path.as_str()) {
        ("GET", "/health") => AutomationResponse::ok(json!({
            "enabled": true,
            "port": CONFIG.get().map(|config| config.port),
        })),
        ("GET", "/state") => dispatch_command(AutomationCommand::GetState),
        ("GET", "/ui") => dispatch_command(AutomationCommand::GetUi),
        ("POST", "/ui/click") => match serde_json::from_slice::<UiClickRequest>(&request.body) {
            Ok(payload) => dispatch_command(AutomationCommand::ClickUi { id: payload.id }),
            Err(err) => AutomationResponse::error(format!("Invalid UI click JSON: {}", err)),
        },
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
        ("POST", "/command") => match serde_json::from_slice::<AutomationCommand>(&request.body) {
            Ok(command) => dispatch_command(command),
            Err(err) => AutomationResponse::error(format!("Invalid command JSON: {}", err)),
        },
        _ => AutomationResponse::error(format!(
            "Unsupported endpoint: {} {}",
            request.method, request.path
        )),
    };

    let _ = write_json(&mut stream, response.http_status, &response);
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
    body: Vec<u8>,
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
        if buffer.len() > 64 * 1024 {
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
    let path = parts
        .next()
        .ok_or_else(|| "Missing request path.".to_string())?
        .split('?')
        .next()
        .unwrap_or("/")
        .to_string();

    let mut content_length = 0_usize;
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if name.eq_ignore_ascii_case("content-length") {
            content_length = value.trim().parse::<usize>().unwrap_or(0);
        }
    }

    let body_start = header_end + 4;
    let mut body = buffer.get(body_start..).unwrap_or_default().to_vec();
    while body.len() < content_length {
        let read = stream.read(&mut chunk).map_err(|err| err.to_string())?;
        if read == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..read]);
    }
    body.truncate(content_length);

    Ok(HttpRequest { method, path, body })
}

fn find_header_end(buffer: &[u8]) -> Option<usize> {
    buffer.windows(4).position(|window| window == b"\r\n\r\n")
}

fn write_json(
    stream: &mut TcpStream,
    status: u16,
    response: &AutomationResponse,
) -> std::io::Result<()> {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        409 => "Conflict",
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
