use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use reqwest::multipart::{Form, Part};
use reqwest::{Client, Response};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::state::{
    InputRole, InputUi, ProviderConnection, ProviderEntry, ProviderInputField, ProviderInputType,
    ProviderOutputType, ProviderWorkflowKind,
};

use super::{ProviderExecutionError, ProviderOutput, ProviderProgress};

const DEFAULT_ENGINE_URL: &str = "http://127.0.0.1:8765";
const CATALOG_CACHE_FILE: &str = "engine_catalog.json";
const CONNECTION_SETTINGS_FILE: &str = "engine.json";
const CACHED_CATALOG_UNAVAILABLE_REASON: &str =
    "LatentSlate Engine is offline; this tool was loaded from the cached catalog.";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineConnectionSettings {
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default = "default_base_url")]
    pub base_url: String,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default = "default_catalog_timeout_ms")]
    pub catalog_timeout_ms: u64,
}

impl Default for EngineConnectionSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            base_url: default_base_url(),
            api_key: None,
            catalog_timeout_ms: default_catalog_timeout_ms(),
        }
    }
}

fn default_enabled() -> bool {
    true
}

fn default_base_url() -> String {
    DEFAULT_ENGINE_URL.to_string()
}

fn default_catalog_timeout_ms() -> u64 {
    800
}

pub fn connection_settings_path() -> PathBuf {
    crate::core::paths::app_runtime_root().join(CONNECTION_SETTINGS_FILE)
}

pub fn catalog_cache_path() -> PathBuf {
    crate::core::paths::app_runtime_root().join(CATALOG_CACHE_FILE)
}

pub fn load_connection_settings() -> EngineConnectionSettings {
    let mut settings: EngineConnectionSettings = fs::read_to_string(connection_settings_path())
        .ok()
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default();

    if let Ok(base_url) = std::env::var("LATENTSLATE_ENGINE_URL") {
        if !base_url.trim().is_empty() {
            settings.base_url = base_url;
            settings.enabled = true;
        }
    }
    if let Ok(token) = std::env::var("LATENTSLATE_ENGINE_TOKEN") {
        settings.api_key = (!token.trim().is_empty()).then_some(token);
    }
    settings.base_url = settings.base_url.trim_end_matches('/').to_string();
    settings
}

pub fn load_provider_entries() -> Result<Vec<ProviderEntry>, String> {
    let settings = load_connection_settings();
    if !settings.enabled {
        return Ok(Vec::new());
    }

    let (mut catalog, loaded_from_cache) = match fetch_catalog_blocking(&settings) {
        Ok(catalog) => {
            if let Err(err) = save_catalog_cache(&catalog) {
                println!("Failed to cache LatentSlate Engine catalog: {err}");
            }
            (catalog, false)
        }
        Err(live_error) => match load_catalog_cache() {
            Ok(catalog) => {
                println!(
                    "LatentSlate Engine unavailable at {}; using cached catalog: {live_error}",
                    settings.base_url
                );
                (catalog, true)
            }
            Err(_) => return Ok(Vec::new()),
        },
    };

    if loaded_from_cache {
        mark_cached_catalog_unavailable(&mut catalog);
    }
    catalog_to_provider_entries(&catalog, &settings)
}

fn fetch_catalog_blocking(settings: &EngineConnectionSettings) -> Result<EngineCatalog, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_millis(settings.catalog_timeout_ms.max(100)))
        .build()
        .map_err(|err| format!("Failed to build engine catalog client: {err}"))?;
    let mut request = client.get(endpoint(&settings.base_url, "/v1/catalog"));
    if let Some(token) = settings
        .api_key
        .as_deref()
        .filter(|token| !token.trim().is_empty())
    {
        request = request.bearer_auth(token);
    }
    let response = request
        .send()
        .map_err(|err| format!("Engine catalog request failed: {err}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "Engine catalog request failed ({})",
            response.status()
        ));
    }
    response
        .json()
        .map_err(|err| format!("Engine catalog response was invalid: {err}"))
}

fn save_catalog_cache(catalog: &EngineCatalog) -> Result<(), String> {
    let path = catalog_cache_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    let tmp = path.with_extension("json.tmp");
    let json = serde_json::to_vec_pretty(catalog).map_err(|err| err.to_string())?;
    fs::write(&tmp, json).map_err(|err| err.to_string())?;
    if path.exists() {
        fs::remove_file(&path).map_err(|err| err.to_string())?;
    }
    fs::rename(tmp, path).map_err(|err| err.to_string())
}

fn load_catalog_cache() -> Result<EngineCatalog, String> {
    let json = fs::read(catalog_cache_path()).map_err(|err| err.to_string())?;
    serde_json::from_slice(&json).map_err(|err| err.to_string())
}

fn mark_cached_catalog_unavailable(catalog: &mut EngineCatalog) {
    for tool in &mut catalog.tools {
        tool.available = false;
        tool.unavailable_reason = Some(CACHED_CATALOG_UNAVAILABLE_REASON.to_string());
    }
}

fn catalog_to_provider_entries(
    catalog: &EngineCatalog,
    settings: &EngineConnectionSettings,
) -> Result<Vec<ProviderEntry>, String> {
    let mut providers = Vec::new();
    for tool in catalog.tools.iter() {
        match tool_to_provider(tool, settings) {
            Ok(provider) => providers.push(provider),
            Err(err) => println!("Skipping engine tool {}: {err}", tool.key),
        }
    }
    Ok(providers)
}

fn tool_to_provider(
    tool: &EngineTool,
    settings: &EngineConnectionSettings,
) -> Result<ProviderEntry, String> {
    let mut description = tool.description.clone();
    if !tool.available {
        let reason = tool
            .unavailable_reason
            .as_deref()
            .unwrap_or("The engine reports this tool as unavailable.");
        description = Some(match description {
            Some(existing) => format!("{existing}\n\nUnavailable: {reason}"),
            None => format!("Unavailable: {reason}"),
        });
    }

    Ok(ProviderEntry {
        id: tool.id,
        name: tool.name.clone(),
        description,
        output_type: parse_output_type(&tool.output.r#type)?,
        workflow_kind: parse_workflow_kind(&tool.workflow_kind)?,
        timeline_bridge: None,
        inputs: tool
            .inputs
            .iter()
            .map(convert_input)
            .collect::<Result<Vec<_>, _>>()?,
        connection: ProviderConnection::LatentSlateEngine {
            base_url: settings.base_url.clone(),
            api_key: settings.api_key.clone(),
            tool_key: tool.key.clone(),
            schema_revision: tool.schema_revision,
            schema_hash: tool.schema_hash.clone(),
            available: tool.available,
            unavailable_reason: tool.unavailable_reason.clone(),
        },
    })
}

fn convert_input(input: &EngineInput) -> Result<ProviderInputField, String> {
    let input_type = match input.r#type.as_str() {
        "image" => ProviderInputType::Image,
        "video" => ProviderInputType::Video,
        "audio" => ProviderInputType::Audio,
        "text" => ProviderInputType::Text,
        "number" => ProviderInputType::Number,
        "integer" => ProviderInputType::Integer,
        "boolean" => ProviderInputType::Boolean,
        "choice" => ProviderInputType::Enum {
            options: input
                .options
                .iter()
                .map(|option| option.value.clone())
                .collect(),
        },
        "resource" => {
            return Err(format!(
                "resource input {:?} is reserved for a later LatentSlate resource picker",
                input.key
            ))
        }
        other => return Err(format!("unsupported input type {other:?}")),
    };

    Ok(ProviderInputField {
        name: input.key.clone(),
        label: input.label.clone(),
        description: input.description.clone(),
        input_type,
        required: input.required,
        default: input.default.clone(),
        role: input.role.as_deref().and_then(parse_input_role),
        ui: input.ui.as_ref().map(|ui| InputUi {
            min: ui.min,
            max: ui.max,
            step: ui.step,
            placeholder: ui.placeholder.clone(),
            multiline: ui.multiline,
            group: ui.group.clone(),
            advanced: ui.advanced,
            unit: ui.unit.clone(),
        }),
    })
}

fn parse_output_type(value: &str) -> Result<ProviderOutputType, String> {
    match value {
        "image" => Ok(ProviderOutputType::Image),
        "video" => Ok(ProviderOutputType::Video),
        "audio" => Ok(ProviderOutputType::Audio),
        other => Err(format!("unsupported output type {other:?}")),
    }
}

fn parse_workflow_kind(value: &str) -> Result<ProviderWorkflowKind, String> {
    match value {
        "text_to_image" => Ok(ProviderWorkflowKind::TextToImage),
        "image_to_image" => Ok(ProviderWorkflowKind::ImageToImage),
        "text_to_video" => Ok(ProviderWorkflowKind::TextToVideo),
        "image_to_video" => Ok(ProviderWorkflowKind::ImageToVideo),
        "first_frame_last_frame_video" => Ok(ProviderWorkflowKind::FirstFrameLastFrameVideo),
        "video_to_video" => Ok(ProviderWorkflowKind::VideoToVideo),
        "video_to_bridge" => Ok(ProviderWorkflowKind::VideoToBridge),
        "text_to_audio" => Ok(ProviderWorkflowKind::TextToAudio),
        "audio_to_audio" => Ok(ProviderWorkflowKind::AudioToAudio),
        "custom" => Ok(ProviderWorkflowKind::Custom),
        other => Err(format!("unsupported workflow kind {other:?}")),
    }
}

fn parse_input_role(value: &str) -> Option<InputRole> {
    match value {
        "width" => Some(InputRole::Width),
        "height" => Some(InputRole::Height),
        "seed" => Some(InputRole::Seed),
        "duration_seconds" => Some(InputRole::DurationSeconds),
        "fps" => Some(InputRole::Fps),
        "frame_count" => Some(InputRole::FrameCount),
        "start_image" | "source_image" => Some(InputRole::StartImage),
        "end_image" => Some(InputRole::EndImage),
        "left_video" => Some(InputRole::LeftVideo),
        "right_video" => Some(InputRole::RightVideo),
        "left_replace_frames" => Some(InputRole::LeftReplaceFrames),
        "right_replace_frames" => Some(InputRole::RightReplaceFrames),
        "edge_blend_frames" => Some(InputRole::EdgeBlendFrames),
        _ => None,
    }
}

pub async fn test_connection(
    provider: &ProviderEntry,
    base_url: &str,
    api_key: Option<&str>,
    live: bool,
) -> Result<Value, String> {
    if live {
        let client = build_async_client(Duration::from_secs(8)).map_err(provider_error_message)?;
        let response = send_with_auth(client.get(endpoint(base_url, "/v1/health")), api_key)
            .send()
            .await
            .map_err(|err| format!("LatentSlate Engine connection failed: {err}"))?;
        ensure_success(response, "LatentSlate Engine health check")
            .await
            .map_err(provider_error_message)?;
    }
    Ok(json!({
        "provider_id": provider.id,
        "name": provider.name,
        "kind": "latentslate_engine",
        "live": live,
        "ok": true,
        "base_url": base_url,
        "api_key_present": api_key.is_some_and(|value| !value.trim().is_empty()),
    }))
}

pub async fn generate_output(
    provider: &ProviderEntry,
    base_url: &str,
    api_key: Option<&str>,
    schema_revision: u32,
    schema_hash: &str,
    available: bool,
    unavailable_reason: Option<&str>,
    inputs: &HashMap<String, Value>,
    progress_tx: Option<mpsc::UnboundedSender<ProviderProgress>>,
) -> Result<ProviderOutput, ProviderExecutionError> {
    if !available {
        return Err(ProviderExecutionError::Error(
            unavailable_reason
                .unwrap_or("LatentSlate Engine tool is unavailable.")
                .to_string(),
        ));
    }

    let client = build_async_client(Duration::from_secs(60 * 60 * 3))?;
    let prepared_inputs = prepare_inputs(&client, provider, base_url, api_key, inputs).await?;
    let response = send_with_auth(client.post(endpoint(base_url, "/v1/jobs")), api_key)
        .json(&json!({
            "tool_id": provider.id,
            "schema_revision": schema_revision,
            "schema_hash": schema_hash,
            "inputs": prepared_inputs,
        }))
        .send()
        .await
        .map_err(|err| offline("LatentSlate Engine job submission", err))?;
    let mut job: EngineJob =
        parse_json_response(response, "LatentSlate Engine job submission").await?;
    let mut unchanged_polls = 0_u32;

    loop {
        if let Some(progress) = job.progress {
            if let Some(tx) = progress_tx.as_ref() {
                let _ = tx.send(ProviderProgress::overall(progress.clamp(0.0, 1.0) as f32));
            }
        }
        match job.status.as_str() {
            "queued" | "running" => {
                tokio::time::sleep(engine_poll_delay(unchanged_polls)).await;
                let response = send_with_auth(
                    client.get(endpoint(base_url, &format!("/v1/jobs/{}", job.id))),
                    api_key,
                )
                .send()
                .await
                .map_err(|err| offline("LatentSlate Engine job polling", err))?;
                let next_job: EngineJob =
                    parse_json_response(response, "LatentSlate Engine job polling").await?;
                if engine_job_poll_changed(&job, &next_job) {
                    unchanged_polls = 0;
                } else {
                    unchanged_polls = unchanged_polls.saturating_add(1);
                }
                job = next_job;
            }
            "succeeded" => break,
            "canceled" => {
                return Err(ProviderExecutionError::Error(job.message.unwrap_or_else(
                    || "LatentSlate Engine job was canceled.".to_string(),
                )))
            }
            "failed" => {
                return Err(ProviderExecutionError::Error(
                    job.error
                        .map(|error| error.message)
                        .or(job.message)
                        .unwrap_or_else(|| "LatentSlate Engine job failed.".to_string()),
                ))
            }
            other => {
                return Err(ProviderExecutionError::Error(format!(
                    "LatentSlate Engine returned unknown job status {other:?}"
                )))
            }
        }
    }

    let artifact = job
        .artifacts
        .iter()
        .find(|artifact| artifact.role == "primary")
        .or_else(|| job.artifacts.first())
        .ok_or_else(|| {
            ProviderExecutionError::Error(
                "LatentSlate Engine completed without a downloadable artifact.".to_string(),
            )
        })?;
    let response = send_with_auth(
        client.get(endpoint(base_url, &artifact.download_url)),
        api_key,
    )
    .send()
    .await
    .map_err(|err| offline("LatentSlate Engine artifact download", err))?;
    let response = ensure_success(response, "LatentSlate Engine artifact download").await?;
    let bytes = response
        .bytes()
        .await
        .map_err(|err| offline("LatentSlate Engine artifact download", err))?
        .to_vec();
    let extension = Path::new(&artifact.filename)
        .extension()
        .and_then(|extension| extension.to_str())
        .filter(|extension| !extension.is_empty())
        .unwrap_or_else(|| match provider.output_type {
            ProviderOutputType::Image => "png",
            ProviderOutputType::Video => "mp4",
            ProviderOutputType::Audio => "wav",
        })
        .to_string();
    Ok(ProviderOutput { bytes, extension })
}

async fn prepare_inputs(
    client: &Client,
    provider: &ProviderEntry,
    base_url: &str,
    api_key: Option<&str>,
    inputs: &HashMap<String, Value>,
) -> Result<HashMap<String, Value>, ProviderExecutionError> {
    let mut prepared = inputs.clone();
    let mut uploads = HashMap::<PathBuf, Uuid>::new();
    for input in provider.inputs.iter() {
        if !matches!(
            input.input_type,
            ProviderInputType::Image | ProviderInputType::Video | ProviderInputType::Audio
        ) {
            continue;
        }
        let Some(path_text) = inputs.get(&input.name).and_then(Value::as_str) else {
            if input.required {
                return Err(ProviderExecutionError::Error(format!(
                    "Missing media input {}",
                    input.label
                )));
            }
            prepared.remove(&input.name);
            continue;
        };
        if path_text.trim().is_empty() {
            if input.required {
                return Err(ProviderExecutionError::Error(format!(
                    "Missing media input {}",
                    input.label
                )));
            }
            prepared.remove(&input.name);
            continue;
        }
        let path = PathBuf::from(path_text);
        let asset_id = if let Some(asset_id) = uploads.get(&path) {
            *asset_id
        } else {
            let asset_id = upload_asset(client, base_url, api_key, &path).await?;
            uploads.insert(path.clone(), asset_id);
            asset_id
        };
        prepared.insert(
            input.name.clone(),
            json!({ "type": "asset", "asset_id": asset_id }),
        );
    }
    Ok(prepared)
}

async fn upload_asset(
    client: &Client,
    base_url: &str,
    api_key: Option<&str>,
    path: &Path,
) -> Result<Uuid, ProviderExecutionError> {
    let bytes = tokio::fs::read(path).await.map_err(|err| {
        ProviderExecutionError::Error(format!("Failed to read {}: {err}", path.display()))
    })?;
    let filename = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("asset.bin")
        .to_string();
    let mut part = Part::bytes(bytes).file_name(filename);
    if let Some(content_type) = content_type_for_path(path) {
        part = part.mime_str(content_type).map_err(|err| {
            ProviderExecutionError::Error(format!("Invalid media content type: {err}"))
        })?;
    }
    let response = send_with_auth(client.post(endpoint(base_url, "/v1/assets")), api_key)
        .multipart(Form::new().part("file", part))
        .send()
        .await
        .map_err(|err| offline("LatentSlate Engine asset upload", err))?;
    let asset: EngineAsset =
        parse_json_response(response, "LatentSlate Engine asset upload").await?;
    Ok(asset.id)
}

fn build_async_client(timeout: Duration) -> Result<Client, ProviderExecutionError> {
    Client::builder().timeout(timeout).build().map_err(|err| {
        ProviderExecutionError::Error(format!("Failed to build engine client: {err}"))
    })
}

fn send_with_auth(
    request: reqwest::RequestBuilder,
    api_key: Option<&str>,
) -> reqwest::RequestBuilder {
    match api_key.filter(|value| !value.trim().is_empty()) {
        Some(token) => request.bearer_auth(token),
        None => request,
    }
}

async fn parse_json_response<T: for<'de> Deserialize<'de>>(
    response: Response,
    context: &str,
) -> Result<T, ProviderExecutionError> {
    let response = ensure_success(response, context).await?;
    response.json().await.map_err(|err| {
        ProviderExecutionError::Error(format!("{context} returned invalid JSON: {err}"))
    })
}

async fn ensure_success(
    response: Response,
    context: &str,
) -> Result<Response, ProviderExecutionError> {
    if response.status().is_success() {
        return Ok(response);
    }
    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    let message = serde_json::from_str::<EngineErrorResponse>(&text)
        .ok()
        .map(|payload| payload.error.message)
        .filter(|message| !message.is_empty())
        .unwrap_or(text);
    Err(ProviderExecutionError::Error(format!(
        "{context} failed ({status}): {message}"
    )))
}

fn provider_error_message(error: ProviderExecutionError) -> String {
    match error {
        ProviderExecutionError::Offline(message) | ProviderExecutionError::Error(message) => {
            message
        }
    }
}

fn offline(context: &str, err: reqwest::Error) -> ProviderExecutionError {
    if err.is_connect() || err.is_timeout() {
        ProviderExecutionError::Offline(format!("{context} failed: {err}"))
    } else {
        ProviderExecutionError::Error(format!("{context} failed: {err}"))
    }
}

fn endpoint(base_url: &str, path: &str) -> String {
    if path.starts_with("http://") || path.starts_with("https://") {
        return path.to_string();
    }
    format!(
        "{}{}",
        base_url.trim_end_matches('/'),
        if path.starts_with('/') {
            path.to_string()
        } else {
            format!("/{path}")
        }
    )
}

fn engine_poll_delay(unchanged_polls: u32) -> Duration {
    match unchanged_polls {
        0..=3 => Duration::from_millis(350),
        4..=9 => Duration::from_secs(1),
        _ => Duration::from_secs(2),
    }
}

fn engine_job_poll_changed(previous: &EngineJob, next: &EngineJob) -> bool {
    let progress_changed = match (previous.progress, next.progress) {
        (Some(previous), Some(next)) => (previous - next).abs() > 0.000_001,
        (None, None) => false,
        _ => true,
    };
    previous.status != next.status || previous.message != next.message || progress_changed
}

fn content_type_for_path(path: &Path) -> Option<&'static str> {
    match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "webp" => Some("image/webp"),
        "gif" => Some("image/gif"),
        "mp4" => Some("video/mp4"),
        "mov" => Some("video/quicktime"),
        "webm" => Some("video/webm"),
        "mkv" => Some("video/x-matroska"),
        "wav" => Some("audio/wav"),
        "mp3" => Some("audio/mpeg"),
        "flac" => Some("audio/flac"),
        "ogg" => Some("audio/ogg"),
        _ => None,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EngineCatalog {
    protocol_version: String,
    engine_version: String,
    tools: Vec<EngineTool>,
    #[serde(default)]
    bundles: Vec<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EngineTool {
    id: Uuid,
    key: String,
    schema_revision: u32,
    schema_hash: String,
    name: String,
    #[serde(default)]
    description: Option<String>,
    workflow_kind: String,
    output: EngineOutput,
    #[serde(default)]
    inputs: Vec<EngineInput>,
    #[serde(default = "default_true")]
    available: bool,
    #[serde(default)]
    unavailable_reason: Option<String>,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EngineOutput {
    #[serde(rename = "type")]
    r#type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EngineInput {
    key: String,
    label: String,
    #[serde(rename = "type")]
    r#type: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    required: bool,
    #[serde(default)]
    default: Option<Value>,
    #[serde(default)]
    role: Option<String>,
    #[serde(default)]
    options: Vec<EngineChoice>,
    #[serde(default)]
    ui: Option<EngineInputUi>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EngineChoice {
    value: String,
    #[serde(default)]
    label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EngineInputUi {
    #[serde(default)]
    min: Option<f64>,
    #[serde(default)]
    max: Option<f64>,
    #[serde(default)]
    step: Option<f64>,
    #[serde(default)]
    placeholder: Option<String>,
    #[serde(default)]
    multiline: bool,
    #[serde(default)]
    group: Option<String>,
    #[serde(default)]
    advanced: bool,
    #[serde(default)]
    unit: Option<String>,
}

#[derive(Debug, Deserialize)]
struct EngineAsset {
    id: Uuid,
}

#[derive(Debug, Deserialize)]
struct EngineJob {
    id: Uuid,
    status: String,
    #[serde(default)]
    progress: Option<f64>,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    artifacts: Vec<EngineArtifact>,
    #[serde(default)]
    error: Option<EngineErrorBody>,
}

#[derive(Debug, Deserialize)]
struct EngineArtifact {
    #[serde(default)]
    role: String,
    filename: String,
    download_url: String,
}

#[derive(Debug, Deserialize)]
struct EngineErrorResponse {
    error: EngineErrorBody,
}

#[derive(Debug, Deserialize)]
struct EngineErrorBody {
    message: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_engine_job(status: &str, progress: Option<f64>, message: Option<&str>) -> EngineJob {
        EngineJob {
            id: Uuid::nil(),
            status: status.to_string(),
            progress,
            message: message.map(str::to_string),
            artifacts: Vec::new(),
            error: None,
        }
    }

    #[test]
    fn engine_poll_delay_is_responsive_then_backs_off() {
        assert_eq!(engine_poll_delay(0), Duration::from_millis(350));
        assert_eq!(engine_poll_delay(3), Duration::from_millis(350));
        assert_eq!(engine_poll_delay(4), Duration::from_secs(1));
        assert_eq!(engine_poll_delay(9), Duration::from_secs(1));
        assert_eq!(engine_poll_delay(10), Duration::from_secs(2));
        assert_eq!(engine_poll_delay(u32::MAX), Duration::from_secs(2));
    }

    #[test]
    fn engine_job_poll_change_detects_meaningful_updates() {
        let base = test_engine_job("running", Some(0.25), Some("Generating"));
        assert!(!engine_job_poll_changed(
            &base,
            &test_engine_job("running", Some(0.25), Some("Generating")),
        ));
        assert!(engine_job_poll_changed(
            &base,
            &test_engine_job("running", Some(0.5), Some("Generating")),
        ));
        assert!(engine_job_poll_changed(
            &base,
            &test_engine_job("running", Some(0.25), Some("Encoding")),
        ));
        assert!(engine_job_poll_changed(
            &base,
            &test_engine_job("succeeded", Some(1.0), Some("Complete")),
        ));
    }

    #[test]
    fn catalog_tools_normalize_into_provider_entries() {
        let catalog: EngineCatalog = serde_json::from_value(json!({
            "protocol_version": "1.0",
            "engine_version": "0.1.0",
            "bundles": [],
            "tools": [{
                "id": "8c038628-e5bd-4954-80e3-32956321089b",
                "key": "h3.first_last_frame_video",
                "schema_revision": 1,
                "schema_hash": "sha256:test",
                "name": "First/Last Frame Video",
                "description": "Generate a shot.",
                "workflow_kind": "first_frame_last_frame_video",
                "output": { "type": "video" },
                "inputs": [
                    { "key": "prompt", "label": "Prompt", "type": "text", "required": true },
                    { "key": "start_image", "label": "First Frame", "type": "image", "required": true, "role": "start_image" },
                    { "key": "end_image", "label": "Last Frame", "type": "image", "required": false, "role": "end_image" },
                    { "key": "quality", "label": "Quality", "type": "choice", "required": true, "default": "draft", "options": [
                        { "value": "draft", "label": "Draft" },
                        { "value": "final", "label": "Final" }
                    ]}
                ],
                "available": true
            }]
        }))
        .expect("catalog");
        let entries = catalog_to_provider_entries(&catalog, &EngineConnectionSettings::default())
            .expect("providers");
        let provider = &entries[0];
        assert_eq!(provider.id, catalog.tools[0].id);
        assert_eq!(
            provider.workflow_kind,
            ProviderWorkflowKind::FirstFrameLastFrameVideo
        );
        assert_eq!(provider.inputs[1].role, Some(InputRole::StartImage));
        assert_eq!(provider.inputs[2].role, Some(InputRole::EndImage));
        assert!(matches!(
            &provider.inputs[3].input_type,
            ProviderInputType::Enum { .. }
        ));
        assert!(matches!(
            &provider.connection,
            ProviderConnection::LatentSlateEngine { .. }
        ));
    }

    #[test]
    fn klein_image_tools_normalize_into_existing_image_categories() {
        let catalog: EngineCatalog = serde_json::from_value(json!({
            "protocol_version": "1.0",
            "engine_version": "0.1.0",
            "bundles": [],
            "tools": [
                {
                    "id": "e329a7d2-c145-4299-96ef-f2b70376d499",
                    "key": "flux2_klein9b.text_to_image",
                    "schema_revision": 1,
                    "schema_hash": "sha256:t2i",
                    "name": "Text to Image",
                    "workflow_kind": "text_to_image",
                    "output": { "type": "image" },
                    "inputs": [
                        { "key": "prompt", "label": "Prompt", "type": "text", "required": true },
                        { "key": "width", "label": "Width", "type": "integer", "required": true, "default": 1024, "role": "width" },
                        { "key": "height", "label": "Height", "type": "integer", "required": true, "default": 1024, "role": "height" },
                        { "key": "seed", "label": "Seed", "type": "integer", "required": true, "default": 0, "role": "seed" }
                    ],
                    "available": true
                },
                {
                    "id": "3333a6bd-8e71-4236-9372-bad407161803",
                    "key": "flux2_klein9b.image_to_image",
                    "schema_revision": 1,
                    "schema_hash": "sha256:i2i",
                    "name": "Image to Image",
                    "workflow_kind": "image_to_image",
                    "output": { "type": "image" },
                    "inputs": [
                        { "key": "prompt", "label": "Prompt", "type": "text", "required": true },
                        { "key": "source_image", "label": "Source Image", "type": "image", "required": true, "role": "source_image" },
                        { "key": "width", "label": "Width", "type": "integer", "required": false, "role": "width" },
                        { "key": "height", "label": "Height", "type": "integer", "required": false, "role": "height" },
                        { "key": "seed", "label": "Seed", "type": "integer", "required": true, "default": 0, "role": "seed" }
                    ],
                    "available": true
                }
            ]
        }))
        .expect("catalog");

        let entries = catalog_to_provider_entries(&catalog, &EngineConnectionSettings::default())
            .expect("providers");
        assert_eq!(entries.len(), 2);

        let text = &entries[0];
        assert_eq!(text.name, "Text to Image");
        assert_eq!(text.output_type, ProviderOutputType::Image);
        assert_eq!(text.workflow_kind, ProviderWorkflowKind::TextToImage);
        assert_eq!(text.inputs[1].role, Some(InputRole::Width));
        assert_eq!(text.inputs[2].role, Some(InputRole::Height));
        assert!(matches!(
            text.inputs[1].input_type,
            ProviderInputType::Integer
        ));
        assert!(matches!(
            text.inputs[2].input_type,
            ProviderInputType::Integer
        ));

        let edit = &entries[1];
        assert_eq!(edit.name, "Image to Image");
        assert_eq!(edit.output_type, ProviderOutputType::Image);
        assert_eq!(edit.workflow_kind, ProviderWorkflowKind::ImageToImage);
        assert_eq!(edit.inputs[1].role, Some(InputRole::StartImage));
        assert_eq!(edit.inputs[2].role, Some(InputRole::Width));
        assert_eq!(edit.inputs[3].role, Some(InputRole::Height));
        assert!(!edit.inputs[2].required);
        assert!(!edit.inputs[3].required);
        assert_eq!(edit.inputs[2].default, None);
        assert_eq!(edit.inputs[3].default, None);
        assert!(matches!(
            edit.inputs[1].input_type,
            ProviderInputType::Image
        ));
        assert!(matches!(
            &edit.connection,
            ProviderConnection::LatentSlateEngine { tool_key, .. }
                if tool_key == "flux2_klein9b.image_to_image"
        ));
    }

    #[test]
    fn cached_catalog_tools_are_inspectable_but_unavailable() {
        let mut catalog: EngineCatalog = serde_json::from_value(json!({
            "protocol_version": "1.0",
            "engine_version": "0.1.0",
            "bundles": [],
            "tools": [{
                "id": "369a630e-4d64-4e3c-8f15-1809757a10e5",
                "key": "h3.text_to_video",
                "schema_revision": 1,
                "schema_hash": "sha256:test",
                "name": "Text to Video",
                "description": "Cached description.",
                "workflow_kind": "text_to_video",
                "output": { "type": "video" },
                "inputs": [],
                "available": true
            }]
        }))
        .expect("catalog");

        mark_cached_catalog_unavailable(&mut catalog);
        let entries = catalog_to_provider_entries(&catalog, &EngineConnectionSettings::default())
            .expect("providers");
        let provider = &entries[0];
        let ProviderConnection::LatentSlateEngine {
            available,
            unavailable_reason,
            ..
        } = &provider.connection
        else {
            panic!("expected LatentSlate Engine provider");
        };

        assert!(!*available);
        assert_eq!(
            unavailable_reason.as_deref(),
            Some(CACHED_CATALOG_UNAVAILABLE_REASON)
        );
        assert!(provider
            .description
            .as_deref()
            .unwrap_or_default()
            .contains(CACHED_CATALOG_UNAVAILABLE_REASON));
    }

    #[test]
    fn endpoint_joining_is_location_agnostic() {
        assert_eq!(
            endpoint("https://example.test/engine/", "/v1/catalog"),
            "https://example.test/engine/v1/catalog"
        );
    }
}
