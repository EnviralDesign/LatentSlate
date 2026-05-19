//! Loopback-only automation control plane for desktop smoke scenarios.
//!
//! This module deliberately exposes semantic editor commands instead of pixel
//! clicking. The UI remains responsible for applying commands on its normal
//! thread, so automation follows the same project/state paths as the visible UI.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
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

static CONFIG: OnceLock<AutomationConfig> = OnceLock::new();
static COMMAND_TX: OnceLock<Sender<AutomationEnvelope>> = OnceLock::new();
static COMMAND_RX: OnceLock<Mutex<Receiver<AutomationEnvelope>>> = OnceLock::new();

/// Configuration for the loopback automation server.
#[derive(Clone, Debug)]
pub struct AutomationConfig {
    /// TCP port bound on 127.0.0.1.
    pub port: u16,
}

/// Semantic app commands accepted by the automation API.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AutomationCommand {
    /// Return the current app state snapshot.
    GetState,
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
    /// Select a clip by ID or by timeline index.
    SelectClip {
        #[serde(default)]
        clip_id: Option<Uuid>,
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
}

impl AutomationResponse {
    /// Build a successful response with a JSON payload.
    pub fn ok(data: Value) -> Self {
        Self {
            ok: true,
            message: None,
            data,
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
        }
    }
}

/// Parse automation configuration from CLI args and environment variables.
pub fn config_from_args(args: &[String]) -> Option<AutomationConfig> {
    let env_enabled = std::env::var("NLA_AUTOMATION")
        .map(|value| matches!(value.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"))
        .unwrap_or(false);

    let mut enabled = env_enabled;
    let mut port = std::env::var("NLA_AUTOMATION_PORT")
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
        .name("nla-automation-http".to_string())
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
        ("POST", "/command") => match serde_json::from_slice::<AutomationCommand>(&request.body) {
            Ok(command) => dispatch_command(command),
            Err(err) => AutomationResponse::error(format!("Invalid command JSON: {}", err)),
        },
        _ => AutomationResponse::error(format!(
            "Unsupported endpoint: {} {}",
            request.method, request.path
        )),
    };

    let status = if response.ok { 200 } else { 400 };
    let _ = write_json(&mut stream, status, &response);
}

fn dispatch_command(command: AutomationCommand) -> AutomationResponse {
    let Some(tx) = COMMAND_TX.get() else {
        return AutomationResponse::error("Automation command queue is not initialized.");
    };

    let (response_tx, response_rx) = mpsc::channel::<AutomationResponse>();
    let envelope = AutomationEnvelope {
        command,
        responder: response_tx,
    };
    if tx.send(envelope).is_err() {
        return AutomationResponse::error("Automation command queue is closed.");
    }

    response_rx
        .recv_timeout(Duration::from_secs(RESPONSE_TIMEOUT_SECONDS))
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
