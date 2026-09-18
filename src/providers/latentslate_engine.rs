use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use reqwest::multipart::{Form, Part};
use reqwest::{Client, Response};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::state::{
    CanvasContract, InputRole, InputUi, ProviderConnection, ProviderEntry, ProviderInputField,
    ProviderInputType, ProviderOutputType, ProviderTiming, ProviderWorkflowKind,
};

use super::{
    ProviderExecutionError, ProviderOutput, ProviderProgress, ProviderProgressLane,
    ProviderProgressStage,
};

const DEFAULT_ENGINE_URL: &str = "http://127.0.0.1:8765";
const DEFAULT_CONNECTION_NAME: &str = "LatentSlate Engine";
const DEFAULT_CONNECTION_ID: &str = "6c617465-6e74-736c-6174-650000000001";
const CATALOG_CACHE_FILE: &str = "engine_catalog.json";
const CATALOG_CACHE_DIR: &str = "engine_catalogs";
const CONNECTION_SETTINGS_FILE: &str = "engine.json";
const CACHED_CATALOG_UNAVAILABLE_REASON: &str =
    "LatentSlate Engine is offline; this tool was loaded from the cached catalog.";

#[derive(Debug)]
enum EngineCancellationRequest {
    NotRequested,
    Acknowledged,
    Uncertain(ProviderExecutionError),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EngineConnectionSettings {
    #[serde(default = "default_connection_id")]
    pub id: Uuid,
    #[serde(default = "default_connection_name")]
    pub name: String,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default = "default_base_url")]
    pub base_url: String,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default = "default_catalog_timeout_ms")]
    pub catalog_timeout_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineCatalogLoadPhase {
    Disabled,
    Live,
    Cached,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineCatalogFailureKind {
    CredentialsRejected,
    Unreachable,
    InvalidResponse,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineCatalogLoadReport {
    pub connection_id: Uuid,
    pub phase: EngineCatalogLoadPhase,
    pub discovered_count: usize,
    pub available_count: usize,
    pub failure_kind: Option<EngineCatalogFailureKind>,
    pub technical_detail: Option<String>,
}

impl Default for EngineConnectionSettings {
    fn default() -> Self {
        Self {
            id: default_connection_id(),
            name: default_connection_name(),
            enabled: true,
            base_url: default_base_url(),
            api_key: None,
            catalog_timeout_ms: default_catalog_timeout_ms(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EngineConnectionsFile {
    connections: Vec<EngineConnectionSettings>,
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

pub fn default_connection_id() -> Uuid {
    Uuid::parse_str(DEFAULT_CONNECTION_ID).expect("default Engine connection id")
}

pub fn default_connection_name() -> String {
    DEFAULT_CONNECTION_NAME.to_string()
}

pub fn normalize_engine_base_url(url: &str) -> String {
    url.trim().trim_end_matches('/').to_string()
}

pub fn engine_base_urls_match(left: &str, right: &str) -> bool {
    normalize_engine_base_url(left).eq_ignore_ascii_case(&normalize_engine_base_url(right))
}

pub fn connection_settings_path() -> PathBuf {
    crate::core::paths::app_runtime_root().join(CONNECTION_SETTINGS_FILE)
}

#[allow(dead_code)]
pub fn catalog_cache_path() -> PathBuf {
    catalog_cache_path_for(default_connection_id())
}

pub fn catalog_cache_path_for(connection_id: Uuid) -> PathBuf {
    let root = crate::core::paths::app_runtime_root();
    if connection_id == default_connection_id() {
        root.join(CATALOG_CACHE_FILE)
    } else {
        root.join(CATALOG_CACHE_DIR)
            .join(format!("{connection_id}.json"))
    }
}

pub fn parse_engine_connections_json(json: &str) -> Option<Vec<EngineConnectionSettings>> {
    let mut connections = if let Ok(file) = serde_json::from_str::<EngineConnectionsFile>(json) {
        file.connections
    } else if let Ok(single) = serde_json::from_str::<EngineConnectionSettings>(json) {
        vec![single]
    } else {
        return None;
    };
    for connection in &mut connections {
        connection.base_url = normalize_engine_base_url(&connection.base_url);
        if connection.name.trim().is_empty() {
            connection.name = default_connection_name();
        }
    }
    ensure_unique_connection_ids(&mut connections);
    Some(connections)
}

pub fn load_connections() -> Vec<EngineConnectionSettings> {
    let mut connections = match fs::read_to_string(connection_settings_path()) {
        Ok(json) => parse_engine_connections_json(&json)
            .unwrap_or_else(|| vec![EngineConnectionSettings::default()]),
        Err(_) => vec![EngineConnectionSettings::default()],
    };
    apply_env_overrides(&mut connections);
    connections
}

#[allow(dead_code)]
pub fn load_connection_settings() -> EngineConnectionSettings {
    load_connections().into_iter().next().unwrap_or_default()
}

pub fn save_connections(connections: &[EngineConnectionSettings]) -> Result<(), String> {
    let path = connection_settings_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    let mut normalized = connections.to_vec();
    for connection in &mut normalized {
        connection.base_url = normalize_engine_base_url(&connection.base_url);
        if connection.name.trim().is_empty() {
            connection.name = default_connection_name();
        }
    }
    ensure_unique_connection_ids(&mut normalized);
    let file = EngineConnectionsFile {
        connections: normalized,
    };
    let json = serde_json::to_string_pretty(&file).map_err(|err| err.to_string())?;
    fs::write(path, json).map_err(|err| err.to_string())
}

pub fn next_engine_connection_name(existing: &[EngineConnectionSettings]) -> String {
    if !existing
        .iter()
        .any(|connection| connection.name == DEFAULT_CONNECTION_NAME)
    {
        return default_connection_name();
    }
    for index in 2.. {
        let name = format!("{DEFAULT_CONNECTION_NAME} {index}");
        if !existing.iter().any(|connection| connection.name == name) {
            return name;
        }
    }
    default_connection_name()
}

pub fn new_engine_connection(existing: &[EngineConnectionSettings]) -> EngineConnectionSettings {
    EngineConnectionSettings {
        id: Uuid::new_v4(),
        name: next_engine_connection_name(existing),
        enabled: true,
        base_url: default_base_url(),
        api_key: None,
        catalog_timeout_ms: default_catalog_timeout_ms(),
    }
}

fn ensure_unique_connection_ids(connections: &mut [EngineConnectionSettings]) {
    let mut seen = HashSet::new();
    for connection in connections.iter_mut() {
        if connection.id.is_nil() || !seen.insert(connection.id) {
            connection.id = Uuid::new_v4();
            seen.insert(connection.id);
        }
    }
}

fn apply_env_overrides(connections: &mut [EngineConnectionSettings]) {
    if connections.is_empty() {
        return;
    }
    let index = connections
        .iter()
        .position(|connection| connection.id == default_connection_id())
        .unwrap_or(0);
    let target = &mut connections[index];
    if let Ok(base_url) = std::env::var("LATENTSLATE_ENGINE_URL") {
        if !base_url.trim().is_empty() {
            target.base_url = normalize_engine_base_url(&base_url);
            target.enabled = true;
        }
    }
    if let Ok(token) = std::env::var("LATENTSLATE_ENGINE_TOKEN") {
        target.api_key = (!token.trim().is_empty()).then_some(token);
    }
}

pub fn load_provider_entries_for_connections_with_reports(
    connections: &[EngineConnectionSettings],
) -> (Vec<ProviderEntry>, Vec<EngineCatalogLoadReport>) {
    let mut providers = Vec::new();
    let mut reports = Vec::new();
    for settings in connections.iter().cloned() {
        if !settings.enabled {
            reports.push(EngineCatalogLoadReport {
                connection_id: settings.id,
                phase: EngineCatalogLoadPhase::Disabled,
                discovered_count: 0,
                available_count: 0,
                failure_kind: None,
                technical_detail: None,
            });
            continue;
        }
        if settings.base_url.trim().is_empty() {
            reports.push(EngineCatalogLoadReport {
                connection_id: settings.id,
                phase: EngineCatalogLoadPhase::Failed,
                discovered_count: 0,
                available_count: 0,
                failure_kind: Some(EngineCatalogFailureKind::Other),
                technical_detail: Some("The Engine endpoint is empty.".to_string()),
            });
            continue;
        }
        let (entries, report) = load_provider_entries_for_connection(&settings);
        if report.phase == EngineCatalogLoadPhase::Failed {
            if let Some(detail) = report.technical_detail.as_deref() {
                println!(
                    "Failed to load LatentSlate Engine tools from {}: {detail}",
                    settings.base_url
                );
            }
        }
        providers.extend(entries);
        reports.push(report);
    }
    (providers, reports)
}

fn load_provider_entries_for_connection(
    settings: &EngineConnectionSettings,
) -> (Vec<ProviderEntry>, EngineCatalogLoadReport) {
    let cache_path = catalog_cache_path_for(settings.id);
    let (mut catalog, phase, live_error) = match fetch_catalog_blocking(settings) {
        Ok(catalog) => {
            if let Err(err) = save_catalog_cache(&cache_path, &catalog) {
                println!("Failed to cache LatentSlate Engine catalog: {err}");
            }
            (catalog, EngineCatalogLoadPhase::Live, None)
        }
        Err(live_error) => match load_catalog_cache(&cache_path) {
            Ok(catalog) => {
                println!(
                    "LatentSlate Engine unavailable at {}; using cached catalog: {live_error}",
                    settings.base_url
                );
                (catalog, EngineCatalogLoadPhase::Cached, Some(live_error))
            }
            Err(_) => {
                let report = EngineCatalogLoadReport {
                    connection_id: settings.id,
                    phase: EngineCatalogLoadPhase::Failed,
                    discovered_count: 0,
                    available_count: 0,
                    failure_kind: Some(engine_catalog_failure_kind(&live_error)),
                    technical_detail: Some(live_error),
                };
                return (Vec::new(), report);
            }
        },
    };

    if phase == EngineCatalogLoadPhase::Cached {
        mark_cached_catalog_unavailable(&mut catalog);
    }
    let entries = match catalog_to_provider_entries(&catalog, settings) {
        Ok(entries) => entries,
        Err(err) => {
            return (
                Vec::new(),
                EngineCatalogLoadReport {
                    connection_id: settings.id,
                    phase: EngineCatalogLoadPhase::Failed,
                    discovered_count: 0,
                    available_count: 0,
                    failure_kind: Some(EngineCatalogFailureKind::InvalidResponse),
                    technical_detail: Some(err),
                },
            );
        }
    };
    let available_count = entries
        .iter()
        .filter(|provider| provider_is_available(provider))
        .count();
    let report = EngineCatalogLoadReport {
        connection_id: settings.id,
        phase,
        discovered_count: entries.len(),
        available_count,
        failure_kind: live_error.as_deref().map(engine_catalog_failure_kind),
        technical_detail: live_error,
    };
    (entries, report)
}

fn provider_is_available(provider: &ProviderEntry) -> bool {
    !matches!(
        provider.connection,
        ProviderConnection::LatentSlateEngine {
            available: false,
            ..
        }
    )
}

fn engine_catalog_failure_kind(error: &str) -> EngineCatalogFailureKind {
    let lower = error.to_ascii_lowercase();
    if lower.contains("401") || lower.contains("403") || lower.contains("unauthorized") {
        EngineCatalogFailureKind::CredentialsRejected
    } else if lower.contains("invalid") || lower.contains("response") {
        EngineCatalogFailureKind::InvalidResponse
    } else if lower.contains("request failed")
        || lower.contains("connect")
        || lower.contains("timeout")
    {
        EngineCatalogFailureKind::Unreachable
    } else {
        EngineCatalogFailureKind::Other
    }
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

fn save_catalog_cache(path: &Path, catalog: &EngineCatalog) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    let tmp = path.with_extension("json.tmp");
    let json = serde_json::to_vec_pretty(catalog).map_err(|err| err.to_string())?;
    fs::write(&tmp, json).map_err(|err| err.to_string())?;
    if path.exists() {
        fs::remove_file(path).map_err(|err| err.to_string())?;
    }
    fs::rename(tmp, path).map_err(|err| err.to_string())
}

fn load_catalog_cache(path: &Path) -> Result<EngineCatalog, String> {
    let json = fs::read(path).map_err(|err| err.to_string())?;
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
            Err(err) if err.contains("missing operation") => return Err(err),
            Err(err) => println!("Skipping engine tool {}: {err}", tool.key),
        }
    }
    Ok(providers)
}

fn tool_to_provider(
    tool: &EngineTool,
    settings: &EngineConnectionSettings,
) -> Result<ProviderEntry, String> {
    if tool.operation.trim().is_empty() {
        return Err(format!(
            "Engine catalog tool {} is missing operation",
            tool.key
        ));
    }
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

    let inputs = tool
        .inputs
        .iter()
        .map(convert_input)
        .collect::<Result<Vec<_>, _>>()?;
    for input in &inputs {
        if let Some(video_key) = &input.paired_video_input {
            if input.input_type != ProviderInputType::Audio
                || input.required
                || !inputs.iter().any(|video| {
                    video.name == *video_key
                        && video.input_type == ProviderInputType::Video
                        && video.paired_video_input.is_none()
                })
            {
                return Err(format!("Invalid soundtrack pairing for {}", input.name));
            }
        }
    }
    let canvas = tool.canvas.clone().or_else(|| {
        let width = inputs
            .iter()
            .find(|input| input.role == Some(InputRole::Width));
        let height = inputs
            .iter()
            .find(|input| input.role == Some(InputRole::Height));
        crate::core::canvas::canvas_from_dimension_ui(
            width.and_then(|input| input.ui.as_ref()),
            height.and_then(|input| input.ui.as_ref()),
        )
    });

    Ok(ProviderEntry {
        id: tool.id,
        name: tool.name.clone(),
        description,
        output_type: parse_output_type(&tool.output.r#type)?,
        workflow_kind: parse_workflow_kind(&tool.workflow_kind)?,
        timeline_bridge: None,
        inputs,
        canvas,
        timing: tool.timing.clone(),
        connection: ProviderConnection::LatentSlateEngine {
            base_url: settings.base_url.clone(),
            api_key: settings.api_key.clone(),
            tool_key: tool.key.clone(),
            operation: Some(tool.operation.clone()),
            schema_revision: tool.schema_revision,
            schema_hash: tool.schema_hash.clone(),
            recipe: tool.recipe.clone(),
            available: tool.available,
            unavailable_reason: tool.unavailable_reason.clone(),
        },
    })
}

fn convert_input(input: &EngineInput) -> Result<ProviderInputField, String> {
    if let Some(choices) = input.ui.as_ref().and_then(|ui| ui.choices.as_ref()) {
        let supported = !choices.is_empty()
            && choices.iter().all(|choice| match input.r#type.as_str() {
                "number" => choice.is_number(),
                "integer" => choice.as_i64().is_some() || choice.as_u64().is_some(),
                _ => false,
            });
        if !supported {
            return Err(format!(
                "unsupported contract for input {}: unsupported numeric choices",
                input.key
            ));
        }
    }
    let optional_media =
        !input.required && matches!(input.r#type.as_str(), "image" | "video" | "audio");
    if (input.nullable && !optional_media)
        || input.collection != input.ordered
        || (input.collection && (input.r#type != "number" || input.role.is_some()))
    {
        return Err(format!(
            "unsupported contract for input {}: nullable={}, collection={}, ordered={}, type={}",
            input.key, input.nullable, input.collection, input.ordered, input.r#type
        ));
    }
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
            ));
        }
        other => return Err(format!("unsupported input type {other:?}")),
    };

    if input.image_dimensions.is_some() && input_type != ProviderInputType::Image {
        return Err(format!(
            "image_dimensions requires an image input: {}",
            input.key
        ));
    }
    Ok(ProviderInputField {
        ordered_collection: input.collection,
        image_dimensions: input.image_dimensions,
        paired_video_input: input.paired_video_input.clone(),
        prompt_reference_token: input.prompt_reference_token.clone(),
        name: input.key.clone(),
        label: input.label.clone(),
        description: input.description.clone(),
        input_type,
        required: input.required,
        default: input.default.clone(),
        role: input.role.as_deref().and_then(parse_input_role),
        ui: input.ui.as_ref().map(|ui| InputUi {
            choices: ui.choices.clone(),
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
        "reference_to_video" => Ok(ProviderWorkflowKind::ReferenceToVideo),
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
    cancel_token: Option<Arc<AtomicBool>>,
) -> Result<ProviderOutput, ProviderExecutionError> {
    if !available {
        return Err(ProviderExecutionError::Error(
            unavailable_reason
                .unwrap_or("LatentSlate Engine tool is unavailable.")
                .to_string(),
        ));
    }

    let client = build_async_client(Duration::from_secs(60 * 60 * 3))?;
    check_canceled(cancel_token.as_deref())?;
    let prepared_inputs = prepare_inputs(
        &client,
        provider,
        base_url,
        api_key,
        inputs,
        cancel_token.as_deref(),
    )
    .await?;
    check_canceled(cancel_token.as_deref())?;
    let mut body = json!({
        "tool_id": provider.id,
        "schema_revision": schema_revision,
        "schema_hash": schema_hash,
        "inputs": prepared_inputs,
    });
    if let ProviderConnection::LatentSlateEngine {
        recipe: Some(recipe),
        ..
    } = &provider.connection
    {
        body["recipe"] =
            serde_json::to_value(recipe).expect("Engine recipe identity is serializable");
    }
    let response = send_with_auth(client.post(endpoint(base_url, "/v1/jobs")), api_key)
        .json(&body)
        .send()
        .await
        .map_err(|err| offline("LatentSlate Engine job submission", err))?;
    let mut job: EngineJob =
        parse_json_response(response, "LatentSlate Engine job submission").await?;
    let mut unchanged_polls = 0_u32;
    let mut logged_transition = None;
    let mut cancellation_request = EngineCancellationRequest::NotRequested;

    loop {
        let next_transition = engine_job_log_snapshot(&job);
        if logged_transition.as_ref() != Some(&next_transition) {
            println!(
                "[LATENTSLATE ENGINE] job {}: {}",
                job.id,
                format_engine_job_transition(&next_transition)
            );
            logged_transition = Some(next_transition);
        }
        if let Some(tx) = progress_tx.as_ref() {
            if let Some(progress) = engine_job_provider_progress(&job) {
                let _ = tx.send(progress);
            }
        }
        match job.status.as_str() {
            "queued" | "running" => {
                let cancellation_just_attempted =
                    matches!(
                        &cancellation_request,
                        EngineCancellationRequest::NotRequested
                    ) && cancellation_requested(cancel_token.as_deref());
                if cancellation_just_attempted {
                    let cancellation_result = match build_async_client(Duration::from_secs(8)) {
                        Ok(cancellation_client) => {
                            async {
                                let response = send_with_auth(
                                    cancellation_client.delete(endpoint(
                                        base_url,
                                        &format!("/v1/jobs/{}", job.id),
                                    )),
                                    api_key,
                                )
                                .send()
                                .await
                                .map_err(|err| {
                                    offline("LatentSlate Engine job cancellation", err)
                                })?;
                                ensure_success(response, "LatentSlate Engine job cancellation")
                                    .await
                            }
                            .await
                        }
                        Err(error) => Err(error),
                    };
                    cancellation_request = match cancellation_result {
                        Ok(_) => {
                            println!(
                                "[LATENTSLATE ENGINE] job {}: cancellation requested",
                                job.id
                            );
                            EngineCancellationRequest::Acknowledged
                        }
                        Err(error) => {
                            // The request is bounded to a single DELETE.  Keep polling instead
                            // of releasing the local queue based on an unacknowledged click.
                            println!(
                                "[LATENTSLATE ENGINE] job {}: cancellation request uncertain; polling terminal status",
                                job.id
                            );
                            EngineCancellationRequest::Uncertain(error)
                        }
                    };
                }
                tokio::time::sleep(if cancellation_just_attempted {
                    Duration::ZERO
                } else {
                    engine_poll_delay(unchanged_polls)
                })
                .await;
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
            "succeeded" => {
                match cancellation_request {
                    EngineCancellationRequest::Acknowledged => {
                        return Err(ProviderExecutionError::Canceled(
                            "LatentSlate Engine completed before the cancellation request took effect."
                                .to_string(),
                        ));
                    }
                    EngineCancellationRequest::Uncertain(error) => return Err(error),
                    EngineCancellationRequest::NotRequested
                        if cancellation_requested(cancel_token.as_deref()) =>
                    {
                        return Err(ProviderExecutionError::Canceled(
                            "LatentSlate Engine completed before the cancellation request could be sent."
                                .to_string(),
                        ));
                    }
                    EngineCancellationRequest::NotRequested => {}
                }
                break;
            }
            "canceled" => {
                return Err(ProviderExecutionError::Canceled(
                    job.message
                        .unwrap_or_else(|| "LatentSlate Engine job was canceled.".to_string()),
                ));
            }
            "failed" => {
                return Err(ProviderExecutionError::Error(
                    job.error
                        .map(|error| error.message)
                        .or(job.message)
                        .unwrap_or_else(|| "LatentSlate Engine job failed.".to_string()),
                ));
            }
            other => {
                return Err(ProviderExecutionError::Error(format!(
                    "LatentSlate Engine returned unknown job status {other:?}"
                )));
            }
        }
    }

    check_canceled(cancel_token.as_deref())?;
    if matches!(
        &provider.connection,
        ProviderConnection::LatentSlateEngine {
            recipe: Some(_),
            ..
        }
    ) && job.provenance.is_none()
    {
        return Err(ProviderExecutionError::Error(
            "LatentSlate Engine completed the recipe without accepted execution provenance.".into(),
        ));
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
    check_canceled(cancel_token.as_deref())?;
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
    Ok(ProviderOutput {
        bytes,
        extension,
        engine_execution: job.provenance,
    })
}

async fn prepare_inputs(
    client: &Client,
    provider: &ProviderEntry,
    base_url: &str,
    api_key: Option<&str>,
    inputs: &HashMap<String, Value>,
    cancel_token: Option<&AtomicBool>,
) -> Result<HashMap<String, Value>, ProviderExecutionError> {
    let mut prepared = inputs.clone();
    let mut uploads = HashMap::<PathBuf, Uuid>::new();
    for input in provider.inputs.iter() {
        check_canceled(cancel_token)?;
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
            check_canceled(cancel_token)?;
            let asset_id = upload_asset(client, base_url, api_key, &path).await?;
            check_canceled(cancel_token)?;
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

fn cancellation_requested(cancel_token: Option<&AtomicBool>) -> bool {
    cancel_token.is_some_and(|token| token.load(Ordering::Relaxed))
}

fn check_canceled(cancel_token: Option<&AtomicBool>) -> Result<(), ProviderExecutionError> {
    if cancellation_requested(cancel_token) {
        Err(ProviderExecutionError::Canceled(
            "Generation cancellation requested.".to_string(),
        ))
    } else {
        Ok(())
    }
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

/// Unloads every Engine runtime wrapper and clears its bounded caches.
pub async fn release_resources(base_url: &str, api_key: Option<&str>) -> Result<Value, String> {
    let client = build_async_client(Duration::from_secs(10)).map_err(provider_error_message)?;
    let response = send_with_auth(client.delete(endpoint(base_url, "/v1/runtime")), api_key)
        .send()
        .await
        .map_err(|err| {
            provider_error_message(offline("LatentSlate Engine resource release", err))
        })?;
    let status = response.status();
    let text = response.text().await.map_err(|err| {
        format!("LatentSlate Engine resource release response read failed: {err}")
    })?;
    if !status.is_success() {
        let payload = serde_json::from_str::<Value>(&text).unwrap_or(Value::Null);
        let detail = payload
            .get("detail")
            .and_then(Value::as_str)
            .or_else(|| {
                payload
                    .get("error")
                    .and_then(|error| error.get("message"))
                    .and_then(Value::as_str)
            })
            .filter(|message| !message.is_empty())
            .unwrap_or_else(|| text.trim());
        return Err(if detail.is_empty() {
            format!("LatentSlate Engine resource release failed ({status})")
        } else {
            format!("LatentSlate Engine resource release failed ({status}): {detail}")
        });
    }
    serde_json::from_str(&text)
        .map_err(|err| format!("LatentSlate Engine resource release returned invalid JSON: {err}"))
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
    if status == reqwest::StatusCode::CONFLICT {
        return Err(ProviderExecutionError::RefreshRequired(format!(
            "{context}: provider refresh required ({status}): {message}. Review the refreshed inputs and submit again."
        )));
    }
    Err(ProviderExecutionError::Error(format!(
        "{context} failed ({status}): {message}"
    )))
}

fn provider_error_message(error: ProviderExecutionError) -> String {
    match error {
        ProviderExecutionError::Offline(message)
        | ProviderExecutionError::RefreshRequired(message)
        | ProviderExecutionError::Error(message)
        | ProviderExecutionError::Canceled(message) => message,
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
    previous.status != next.status
        || previous.message != next.message
        || previous.stage != next.stage
        || progress_changed
}

/// A privacy-safe, low-volume view of an Engine job update for process logging.
///
/// Engine status messages are intentionally reduced to a recognized phase instead of being
/// written verbatim: an Engine implementation must never cause prompts, paths, or credentials
/// to enter the LatentSlate process log.
#[derive(Debug, Clone, PartialEq, Eq)]
struct EngineJobLogSnapshot {
    status: EngineJobLogStatus,
    phase: Option<EngineJobPhase>,
    progress_percent: Option<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EngineJobLogStatus {
    Queued,
    Running,
    Succeeded,
    Failed,
    Canceled,
    Unknown,
}

impl EngineJobLogStatus {
    fn from_untrusted(value: &str) -> Self {
        match value {
            "queued" => Self::Queued,
            "running" => Self::Running,
            "succeeded" => Self::Succeeded,
            "failed" => Self::Failed,
            "canceled" => Self::Canceled,
            _ => Self::Unknown,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Canceled => "canceled",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EngineJobPhase {
    Validation,
    WaitingForWorker,
    WorkerStart,
    Import,
    Materialization,
    Preparation,
    Generation,
    Encoding,
    Finalizing,
    Download,
    Complete,
}

impl EngineJobPhase {
    fn label(self) -> &'static str {
        match self {
            Self::Validation => "validation",
            Self::WaitingForWorker => "waiting for worker",
            Self::WorkerStart => "worker start",
            Self::Import => "import",
            Self::Materialization => "materialization",
            Self::Preparation => "preparation",
            Self::Generation => "generation",
            Self::Encoding => "encoding",
            Self::Finalizing => "finalizing",
            Self::Download => "download",
            Self::Complete => "complete",
        }
    }
}

fn engine_job_log_snapshot(job: &EngineJob) -> EngineJobLogSnapshot {
    EngineJobLogSnapshot {
        status: EngineJobLogStatus::from_untrusted(&job.status),
        phase: engine_job_phase(job.message.as_deref()),
        progress_percent: job.progress.map(engine_log_progress_percent),
    }
}

fn engine_log_progress_percent(progress: f64) -> u8 {
    // Five-point buckets preserve useful movement without writing an entry for every poll.
    ((progress.clamp(0.0, 1.0) * 20.0).floor() as u8).saturating_mul(5)
}

fn engine_job_phase(message: Option<&str>) -> Option<EngineJobPhase> {
    let message = message?.trim().to_ascii_lowercase();
    if message.contains("materializ") {
        Some(EngineJobPhase::Materialization)
    } else if message.contains("inspect") {
        Some(EngineJobPhase::Validation)
    } else if message.contains("validat") {
        Some(EngineJobPhase::Validation)
    } else if message.contains("waiting") && message.contains("worker") {
        Some(EngineJobPhase::WaitingForWorker)
    } else if message.contains("worker")
        && (message.contains("start") || message.contains("launch") || message.contains("ready"))
    {
        Some(EngineJobPhase::WorkerStart)
    } else if message.contains("import") {
        Some(EngineJobPhase::Import)
    } else if message.contains("prepar")
        || message.contains("initializ")
        || message.contains("build")
        || message.contains("plan")
    {
        Some(EngineJobPhase::Preparation)
    } else if message.contains("generat") || message.contains("sampl") || message.contains("render")
    {
        Some(EngineJobPhase::Generation)
    } else if message.contains("encod") {
        Some(EngineJobPhase::Encoding)
    } else if message.contains("finaliz") || message.contains("publish") {
        Some(EngineJobPhase::Finalizing)
    } else if message.contains("download") {
        Some(EngineJobPhase::Download)
    } else if message.contains("complet") || message.contains("succeed") {
        Some(EngineJobPhase::Complete)
    } else {
        None
    }
}

fn format_engine_job_transition(snapshot: &EngineJobLogSnapshot) -> String {
    let mut fields = vec![format!("status={}", snapshot.status.label())];
    if let Some(phase) = snapshot.phase {
        fields.push(format!("phase={}", phase.label()));
    }
    if let Some(progress_percent) = snapshot.progress_percent {
        fields.push(format!("progress={progress_percent}%"));
    }
    fields.join(", ")
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    recipe: Option<crate::state::EngineRecipeIdentity>,
    operation: String,
    name: String,
    #[serde(default)]
    description: Option<String>,
    workflow_kind: String,
    output: EngineOutput,
    #[serde(default)]
    inputs: Vec<EngineInput>,
    #[serde(default)]
    canvas: Option<CanvasContract>,
    #[serde(default)]
    timing: Option<ProviderTiming>,
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
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    collection: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    ordered: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    nullable: bool,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    image_dimensions: Option<crate::state::ImageDimensionsRequirement>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    paired_video_input: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    prompt_reference_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EngineChoice {
    value: String,
    #[serde(default)]
    label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EngineInputUi {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    choices: Option<Vec<Value>>,
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
    #[serde(flatten)]
    provenance: Option<crate::state::EngineExecutionProvenance>,
    id: Uuid,
    status: String,
    #[serde(default)]
    progress: Option<f64>,
    #[serde(default)]
    stage: Option<EngineJobStage>,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    artifacts: Vec<EngineArtifact>,
    #[serde(default)]
    error: Option<EngineErrorBody>,
}

fn engine_job_provider_progress(job: &EngineJob) -> Option<ProviderProgress> {
    let progress = ProviderProgress {
        overall: job.progress.map(|progress| ProviderProgressLane {
            label: "Overall".to_string(),
            progress: progress.clamp(0.0, 1.0) as f32,
        }),
        stage: job.stage.as_ref().map(|stage| ProviderProgressStage {
            label: stage.label.clone(),
            progress: stage
                .progress
                .map(|progress| progress.clamp(0.0, 1.0) as f32),
            detail: stage.detail.clone(),
        }),
    };
    (progress.overall.is_some() || progress.stage.is_some()).then_some(progress)
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
struct EngineJobStage {
    label: String,
    #[serde(default)]
    progress: Option<f64>,
    #[serde(default)]
    detail: Option<String>,
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
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};

    fn test_engine_provider(base_url: String) -> ProviderEntry {
        ProviderEntry::new(
            "Engine test",
            ProviderOutputType::Image,
            ProviderConnection::LatentSlateEngine {
                base_url,
                recipe: None,
                api_key: Some("unit-token".to_string()),
                tool_key: "unit.test".to_string(),
                operation: None,
                schema_revision: 1,
                schema_hash: "sha256:unit".to_string(),
                available: true,
                unavailable_reason: None,
            },
        )
    }

    #[test]
    fn user_recipe_catalog_tool_keeps_family_operation_for_authoring() {
        let tool: EngineTool = serde_json::from_value(json!({
            "id": "00000000-0000-4000-8000-000000000001",
            "key": "user_recipe.00000000-0000-4000-8000-000000000002",
            "operation": "ideogram4.t2i",
            "schema_revision": 1,
            "schema_hash": "sha256:test",
            "name": "Ideogram v4 t2i LWD",
            "workflow_kind": "text_to_image",
            "output": { "type": "image" },
            "inputs": [
                { "key": "prompt", "type": "text", "required": true, "label": "Prompt" },
                { "key": "background", "type": "text", "required": false, "label": "Background", "default": "" }
            ],
            "available": true
        }))
        .unwrap();
        let provider = tool_to_provider(&tool, &EngineConnectionSettings::default()).unwrap();
        assert!(matches!(
            &provider.connection,
            ProviderConnection::LatentSlateEngine {
                tool_key,
                operation: Some(operation),
                ..
            } if tool_key == "user_recipe.00000000-0000-4000-8000-000000000002"
                && operation == "ideogram4.t2i"
        ));
        assert!(crate::state::is_ideogram4_t2i(&provider));
        assert_eq!(
            crate::state::asset_lab_authoring_profile(&provider),
            crate::state::AssetLabAuthoringProfile::Regions
        );
    }

    #[test]
    fn catalog_tool_without_operation_is_rejected() {
        let tool: EngineTool = serde_json::from_value(json!({
            "id": "00000000-0000-4000-8000-000000000001",
            "key": "user_recipe.00000000-0000-4000-8000-000000000002",
            "operation": "",
            "schema_revision": 1,
            "schema_hash": "sha256:test",
            "name": "Broken recipe",
            "workflow_kind": "text_to_image",
            "output": { "type": "image" },
            "inputs": [
                { "key": "prompt", "type": "text", "required": true, "label": "Prompt" }
            ],
            "available": true
        }))
        .unwrap();
        let err = tool_to_provider(&tool, &EngineConnectionSettings::default()).unwrap_err();
        assert!(err.contains("missing operation"), "{err}");
        let missing = json!({
            "protocol_version": "1.0",
            "engine_version": "0.1.0",
            "tools": [{
                "id": "00000000-0000-4000-8000-000000000001",
                "key": "ideogram4.text_to_image",
                "schema_revision": 1,
                "schema_hash": "sha256:test",
                "name": "Ideogram",
                "workflow_kind": "text_to_image",
                "output": { "type": "image" },
                "inputs": [],
                "available": true
            }]
        });
        assert!(serde_json::from_value::<EngineCatalog>(missing).is_err());
    }

    async fn read_mock_request(stream: &mut TcpStream) -> String {
        String::from_utf8(read_mock_request_bytes(stream).await).expect("UTF-8 HTTP request")
    }

    async fn read_mock_request_bytes(stream: &mut TcpStream) -> Vec<u8> {
        let mut bytes = Vec::new();
        let header_end = loop {
            let mut chunk = [0_u8; 1024];
            let count = stream.read(&mut chunk).await.expect("read request");
            assert!(count > 0, "mock client closed before completing request");
            bytes.extend_from_slice(&chunk[..count]);
            if let Some(offset) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
                break offset + 4;
            }
        };
        let headers = String::from_utf8_lossy(&bytes[..header_end]);
        let content_length = headers
            .lines()
            .find_map(|line| line.strip_prefix("content-length: "))
            .and_then(|value| value.trim().parse::<usize>().ok())
            .unwrap_or(0);
        while bytes.len() < header_end + content_length {
            let mut chunk = [0_u8; 1024];
            let count = stream.read(&mut chunk).await.expect("read body");
            assert!(
                count > 0,
                "mock client closed before request body completed"
            );
            bytes.extend_from_slice(&chunk[..count]);
        }
        bytes
    }

    async fn write_mock_json(stream: &mut TcpStream, status: u16, body: Value) {
        let body = serde_json::to_string(&body).expect("mock JSON");
        let reason = match status {
            200 => "OK",
            401 => "Unauthorized",
            500 => "Internal Server Error",
            _ => "Mock Status",
        };
        stream
            .write_all(
                format!(
                    "HTTP/1.1 {status} {reason}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                    body.len(),
                    body
                )
                .as_bytes(),
            )
            .await
            .expect("write mock response");
    }

    fn user_recipe_catalog() -> EngineCatalog {
        serde_json::from_str(include_str!(
            "../../tests/fixtures/engine-user-catalog-d8a5979.json"
        ))
        .unwrap()
    }

    #[test]
    fn h3_reference_catalog_preserves_category_pairing_and_manual_prompt_help() {
        let tools: Vec<EngineTool> =
            serde_json::from_str(include_str!("../../tests/fixtures/catalog-h3.json")).unwrap();
        let tool = tools.iter().find(|tool| tool.key == "h3.r2v").unwrap();
        let provider = tool_to_provider(tool, &EngineConnectionSettings::default()).unwrap();
        assert_eq!(
            provider.workflow_kind,
            ProviderWorkflowKind::ReferenceToVideo
        );
        assert_eq!(
            provider.resolved_workflow_kind(),
            ProviderWorkflowKind::ReferenceToVideo
        );
        assert_eq!(
            provider
                .inputs
                .iter()
                .filter(|input| input.paired_video_input.is_some())
                .count(),
            3
        );
        assert!(provider
            .inputs
            .iter()
            .filter(|input| input.input_type == ProviderInputType::Image)
            .all(|input| input.image_dimensions.is_none()));
        assert!(provider
            .inputs
            .iter()
            .find(|input| input.name == "prompt")
            .unwrap()
            .description
            .as_ref()
            .unwrap()
            .contains("<Picture"));
        assert_eq!(
            provider
                .inputs
                .iter()
                .find(|input| input.name == "reference_video_audio_2")
                .unwrap()
                .prompt_reference_token
                .as_deref(),
            Some("<Audio {index}>")
        );
        let mut invalid = tool.clone();
        invalid
            .inputs
            .iter_mut()
            .find(|input| input.paired_video_input.is_some())
            .unwrap()
            .paired_video_input = Some("missing_video".into());
        assert!(tool_to_provider(&invalid, &EngineConnectionSettings::default()).is_err());
    }

    #[test]
    fn prompt_references_qwen_catalog_keeps_fixed_slot_notation() {
        let tool: EngineTool = serde_json::from_str(include_str!(
            "../../tests/fixtures/catalog-qwen2511-edit.json"
        ))
        .unwrap();
        let provider = tool_to_provider(&tool, &EngineConnectionSettings::default()).unwrap();
        let input = provider
            .inputs
            .iter()
            .find(|input| input.prompt_reference_token.as_deref() == Some("Picture 3"))
            .unwrap();
        let reference = crate::state::PromptReference {
            provider_id: provider.id,
            input_name: input.name.clone(),
        };
        assert_eq!(
            crate::core::prompt_references::resolve_reference(&reference, &provider, |field| field
                .name
                == input.name)
            .unwrap(),
            "Picture 3"
        );
    }

    #[test]
    fn prompt_references_klein_catalog_packs_one_to_three_references() {
        let tool: EngineTool = serde_json::from_str(include_str!(
            "../../tests/fixtures/catalog-klein9b-edit.json"
        ))
        .unwrap();
        let provider = tool_to_provider(&tool, &EngineConnectionSettings::default()).unwrap();
        let fields: Vec<_> = provider
            .inputs
            .iter()
            .filter(|input| input.prompt_reference_token.is_some())
            .collect();
        assert_eq!(fields.len(), 3);
        assert!(fields[0].required);
        assert!(!fields[1].required && !fields[2].required);
        let reference = crate::state::PromptReference {
            provider_id: provider.id,
            input_name: fields[2].name.clone(),
        };
        assert_eq!(
            crate::core::prompt_references::resolve_reference(&reference, &provider, |field| field
                .name
                != fields[1].name)
            .unwrap(),
            "image 2"
        );
        assert_eq!(
            crate::core::prompt_references::resolve_reference(&reference, &provider, |_| true)
                .unwrap(),
            "image 3"
        );
    }

    #[test]
    fn published_recipe_catalog_consumes_all_eight_operations_and_fixed_contracts() {
        use crate::core::generation::{
            effective_canvas_dimensions, predicted_output_timing, provider_request_duration,
        };
        let catalog = user_recipe_catalog();
        let providers =
            catalog_to_provider_entries(&catalog, &EngineConnectionSettings::default()).unwrap();
        assert_eq!(providers.len(), 10);
        for (tool, provider) in catalog.tools.iter().zip(&providers) {
            assert_eq!(provider.id, tool.id);
            assert!(
                matches!(&provider.connection, ProviderConnection::LatentSlateEngine { recipe, .. } if recipe == &tool.recipe && recipe.is_some())
            );
            assert_eq!(
                serde_json::from_value::<ProviderEntry>(serde_json::to_value(provider).unwrap())
                    .unwrap(),
                *provider
            );
        }
        let fixed = &providers[8];
        assert!(!fixed.inputs.iter().any(|input| matches!(
            input.role,
            Some(InputRole::Width | InputRole::Height | InputRole::DurationSeconds)
        )));
        assert_eq!(
            effective_canvas_dimensions(fixed, &HashMap::new()),
            Some((256, 256))
        );
        let duration = provider_request_duration(fixed, &HashMap::new()).unwrap();
        assert_eq!(duration, 2.0);
        let timing = predicted_output_timing(fixed, duration).unwrap();
        assert_eq!((timing.frame_count, timing.fps), (57, Some(30.0)));

        let mut mixed = fixed.clone();
        mixed.canvas.as_mut().unwrap().fixed_height = None;
        let mut height = providers[1]
            .inputs
            .iter()
            .find(|input| input.role == Some(InputRole::Height))
            .unwrap()
            .clone();
        height.default = Some(json!(320));
        mixed.inputs.push(height.clone());
        assert_eq!(
            effective_canvas_dimensions(&mixed, &HashMap::new()),
            Some((256, 320))
        );
        assert_eq!(
            effective_canvas_dimensions(&mixed, &HashMap::from([(height.name, json!(512))])),
            Some((256, 512))
        );

        let field = providers[9]
            .inputs
            .iter()
            .find(|field| field.ordered_collection)
            .unwrap();
        assert_eq!(field.default, Some(json!([0.5, 0.25])));
        assert!(
            crate::core::generation::validate_simple_input(field, &json!([0.25, 0.75])).is_none()
        );
        assert!(
            crate::core::generation::validate_simple_input(field, &json!([0.25, 1.5])).is_some()
        );
        assert!(crate::core::generation::validate_simple_input(field, &json!(0.25)).is_some());
    }

    #[test]
    fn unsupported_collection_contract_skips_the_whole_tool() {
        let original = user_recipe_catalog();
        for (kind, collection, ordered, nullable) in [
            ("image", true, true, false),
            ("number", true, false, false),
            ("number", false, true, false),
            ("number", true, true, true),
            ("text", true, true, false),
        ] {
            let mut catalog = original.clone();
            let input = catalog.tools[9]
                .inputs
                .iter_mut()
                .find(|input| input.collection)
                .unwrap();
            input.r#type = kind.to_string();
            input.collection = collection;
            input.ordered = ordered;
            input.nullable = nullable;
            assert!(
                tool_to_provider(&catalog.tools[9], &EngineConnectionSettings::default())
                    .unwrap_err()
                    .contains("unsupported contract")
            );
            let providers =
                catalog_to_provider_entries(&catalog, &EngineConnectionSettings::default())
                    .unwrap();
            assert_eq!(providers.len(), 9);
            assert!(!providers
                .iter()
                .any(|provider| provider.id == catalog.tools[9].id));
        }
    }

    #[test]
    fn asset_lab_v4_optional_nullable_media_uses_native_empty_slot_semantics() {
        for kind in ["image", "video", "audio"] {
            let input: EngineInput=serde_json::from_value(json!({"key":"reference","label":"Reference","type":kind,"required":false,"nullable":true})).unwrap();
            let field = convert_input(&input).unwrap();
            assert!(!field.required);
            assert!(field.default.is_none());
            let mut required = input.clone();
            required.required = true;
            assert!(convert_input(&required).is_err());
            let mut collection = input;
            collection.collection = true;
            collection.ordered = true;
            assert!(convert_input(&collection).is_err());
        }
    }

    #[test]
    fn wan_numeric_choices_survive_catalog_provider_and_offline_cache_round_trips() {
        use crate::core::generation::{preflight_provider_config, validate_simple_input};
        use crate::state::{GenerativeConfig, InputValue, Project};
        let tool: EngineTool = serde_json::from_str(include_str!(
            "../../tests/fixtures/engine-wan-choices-d8a5979.json"
        ))
        .unwrap();
        let mut catalog = user_recipe_catalog();
        catalog.tools = vec![tool];
        let mut cached: EngineCatalog =
            serde_json::from_str(&serde_json::to_string(&catalog).unwrap()).unwrap();
        mark_cached_catalog_unavailable(&mut cached);
        let provider =
            tool_to_provider(&cached.tools[0], &EngineConnectionSettings::default()).unwrap();
        let provider: ProviderEntry =
            serde_json::from_str(&serde_json::to_string(&provider).unwrap()).unwrap();
        assert!(!provider_is_available(&provider));
        for (key, choice) in [
            ("steps", json!(4)),
            ("split_step", json!(2)),
            ("cfg", json!(1.0)),
            ("shift", json!(5.000000000000001)),
        ] {
            let field = provider
                .inputs
                .iter()
                .find(|field| field.name == key)
                .unwrap();
            assert_eq!(
                field.ui.as_ref().unwrap().choices,
                Some(vec![choice.clone()])
            );
            assert!(validate_simple_input(field, &choice).is_none());
            assert!(validate_simple_input(field, &json!(99))
                .unwrap()
                .contains("supported choice"));
        }
        let mut config = GenerativeConfig::default();
        config
            .inputs
            .insert("steps".into(), InputValue::Literal { value: json!(5) });
        let issues =
            preflight_provider_config(&Project::new("choices"), None, None, &provider, &config);
        assert!(issues.iter().any(|issue| {
            issue
                .message
                .contains("Steps must use a supported choice (4)")
        }));
        let shift = provider
            .inputs
            .iter()
            .find(|field| field.name == "shift")
            .unwrap();
        assert!(validate_simple_input(shift, &json!(5.0)).is_some());
    }

    #[test]
    fn numeric_choice_refresh_preserves_invalid_carried_values_for_preflight() {
        use crate::state::{Asset, AssetLabNode, InputValue, Project};
        let root = std::env::temp_dir().join(format!("ls-choice-refresh-{}", Uuid::new_v4()));
        let tool: EngineTool = serde_json::from_str(include_str!(
            "../../tests/fixtures/engine-wan-choices-d8a5979.json"
        ))
        .unwrap();
        let mut next = tool_to_provider(&tool, &EngineConnectionSettings::default()).unwrap();
        let mut previous = next.clone();
        previous
            .inputs
            .iter_mut()
            .find(|input| input.name == "steps")
            .unwrap()
            .ui
            .as_mut()
            .unwrap()
            .choices = None;
        if let ProviderConnection::LatentSlateEngine {
            schema_revision,
            schema_hash,
            ..
        } = &mut next.connection
        {
            *schema_revision += 1;
            *schema_hash = "sha256:narrowed-choices".into();
        }
        let mut editor = crate::editor::EditorState::new();
        editor.project = Project::new("choice refresh");
        editor.project.project_path = Some(root.clone());
        editor.provider_entries = vec![previous];
        let id = editor.project.add_asset(Asset::new_generative_video(
            "shot",
            "generated/video/shot".into(),
            16.0,
            32,
        ));
        editor.project.update_generative_config(id, |config| {
            config.provider_id = Some(next.id);
            config
                .inputs
                .insert("steps".into(), InputValue::Literal { value: json!(5) });
            let mut node = AssetLabNode::new(Some(next.id));
            node.inputs = config.inputs.clone();
            config.lab_graph.nodes.push(node);
        });
        editor.apply_provider_refresh(vec![next.clone()], vec![], vec![], vec![]);
        let config = editor.project.generative_config(id).unwrap();
        assert_eq!(
            config.inputs["steps"],
            InputValue::Literal { value: json!(5) }
        );
        assert_eq!(
            config.lab_graph.nodes[0].inputs["steps"],
            config.inputs["steps"]
        );
        assert!(crate::core::generation::preflight_provider_config(
            &editor.project,
            Some(id),
            None,
            &next,
            config
        )
        .iter()
        .any(|issue| issue
            .message
            .contains("Steps must use a supported choice (4)")));
        editor.project.save().unwrap();
        assert_eq!(
            Project::load(&root)
                .unwrap()
                .generative_config(id)
                .unwrap()
                .inputs["steps"],
            config.inputs["steps"]
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn ordered_numeric_choices_validate_every_item_and_unsupported_choices_fail_closed() {
        let mut tool = user_recipe_catalog().tools[9].clone();
        let field = tool
            .inputs
            .iter_mut()
            .find(|input| input.key == "transformer_adapter_strengths")
            .unwrap();
        field.ui = Some(serde_json::from_value(json!({"choices":[0.25,0.75]})).unwrap());
        let provider = tool_to_provider(&tool, &EngineConnectionSettings::default()).unwrap();
        let input = provider
            .inputs
            .iter()
            .find(|input| input.name == "transformer_adapter_strengths")
            .unwrap();
        assert!(
            crate::core::generation::validate_simple_input(input, &json!([0.75, 0.25])).is_none()
        );
        for (values, item) in [
            (json!([0.5, 0.25]), "Item 1"),
            (json!([0.75, 0.5]), "Item 2"),
        ] {
            let error = crate::core::generation::validate_simple_input(input, &values).unwrap();
            assert!(error.contains(item) && error.contains("supported choice"));
        }
        for choices in [json!(["0.25"]), json!([])] {
            tool.inputs
                .iter_mut()
                .find(|input| input.key == "transformer_adapter_strengths")
                .unwrap()
                .ui
                .as_mut()
                .unwrap()
                .choices = Some(serde_json::from_value(choices).unwrap());
            assert!(
                tool_to_provider(&tool, &EngineConnectionSettings::default())
                    .unwrap_err()
                    .contains("unsupported")
            );
        }
    }

    #[test]
    fn recipe_refresh_reconciles_schema_preserves_media_and_round_trips_provenance() {
        use crate::state::{Asset, AssetLabNode, GenerationRecord, InputValue, Project};
        let root = std::env::temp_dir().join(format!("ls-recipe-refresh-{}", Uuid::new_v4()));
        let mut catalog = user_recipe_catalog();
        catalog.tools[9].available = true;
        let original =
            tool_to_provider(&catalog.tools[9], &EngineConnectionSettings::default()).unwrap();
        let provenance = crate::state::EngineExecutionProvenance {
            tool_id: original.id,
            schema_revision: catalog.tools[9].schema_revision,
            schema_hash: catalog.tools[9].schema_hash.clone(),
            recipe: catalog.tools[9].recipe.clone(),
        };
        let mut editor = crate::editor::EditorState::new();
        editor.project = Project::new("recipe refresh");
        editor.project.project_path = Some(root.clone());
        editor.provider_entries = vec![original.clone()];
        let asset_id = editor.project.add_asset(Asset::new_generative_video(
            "Shot",
            "generated/video/shot".into(),
            30.0,
            57,
        ));
        let inputs = HashMap::from([
            (
                "prompt".into(),
                InputValue::Literal {
                    value: json!("A waterfall"),
                },
            ),
            ("width".into(), InputValue::Literal { value: json!(512) }),
            ("height".into(), InputValue::Literal { value: json!(512) }),
            (
                "duration_seconds".into(),
                InputValue::Literal { value: json!(2.0) },
            ),
            (
                "transformer_adapter_strengths".into(),
                InputValue::Literal {
                    value: json!([0.75, 0.125]),
                },
            ),
        ]);
        editor.project.update_generative_config(asset_id, |config| {
            config.provider_id = Some(original.id);
            config.inputs = inputs.clone();
            config.active_version = Some("v1".into());
            let mut node = AssetLabNode::new(Some(original.id));
            node.inputs = inputs.clone();
            config.lab_graph.nodes.push(node);
            config.versions.push(GenerationRecord {
                label: String::new(),
                authoring_snapshot: None,
                version: "v1".into(),
                timestamp: chrono::Utc::now(),
                provider_id: original.id,
                inputs_snapshot: inputs.clone(),
                media_bindings_snapshot: HashMap::new(),
                resolved_media_inputs: HashMap::new(),
                lab_node_id: None,
                engine_execution: Some(provenance.clone()),
                magic_prompt: None,
            });
        });
        let before_timing =
            serde_json::to_value(editor.project.find_asset(asset_id).unwrap()).unwrap();
        let mut compatible = original.clone();
        if let ProviderConnection::LatentSlateEngine {
            recipe: Some(recipe),
            ..
        } = &mut compatible.connection
        {
            recipe.revision += 5;
            recipe.definition_hash = "sha256:new-hidden-value".into();
        }
        editor.apply_provider_refresh(vec![compatible.clone()], vec![], vec![], vec![]);
        assert_eq!(
            editor.project.generative_config(asset_id).unwrap().inputs,
            inputs
        );
        assert_eq!(
            serde_json::to_value(editor.project.find_asset(asset_id).unwrap()).unwrap(),
            before_timing
        );
        assert_eq!(editor.provider_entries[0], compatible);

        let mut changed = compatible.clone();
        if let ProviderConnection::LatentSlateEngine {
            schema_revision,
            schema_hash,
            ..
        } = &mut changed.connection
        {
            *schema_revision += 1;
            *schema_hash = "sha256:new-surface".into();
        }
        changed
            .inputs
            .retain(|field| field.role != Some(InputRole::Height));
        changed.canvas.as_mut().unwrap().fixed_height = Some(256);
        let width = changed
            .inputs
            .iter_mut()
            .find(|field| field.role == Some(InputRole::Width))
            .unwrap();
        width.name = "new_width".into();
        width.ui.as_mut().unwrap().max = Some(256.0);
        editor.apply_provider_refresh(vec![changed.clone()], vec![], vec![], vec![]);
        let config = editor.project.generative_config(asset_id).unwrap();
        assert!(!config.inputs.contains_key("width") && !config.inputs.contains_key("height"));
        assert_eq!(
            config.inputs["new_width"],
            InputValue::Literal { value: json!(512) }
        );
        assert_eq!(config.lab_graph.nodes[0].inputs, config.inputs);
        assert!(crate::core::generation::preflight_provider_config(
            &editor.project,
            Some(asset_id),
            None,
            &changed,
            config
        )
        .iter()
        .any(|issue| issue.message.contains("at most 256")));
        assert_eq!(
            serde_json::to_value(editor.project.find_asset(asset_id).unwrap()).unwrap(),
            before_timing
        );
        assert_eq!(
            config.versions[0].engine_execution,
            Some(provenance.clone())
        );
        assert_eq!(config.versions[0].inputs_snapshot, inputs);

        let config_before_disabled = config.clone();
        editor.apply_provider_refresh(vec![], vec![], vec![], vec![]);
        assert_eq!(
            editor.project.generative_config(asset_id).unwrap(),
            &config_before_disabled
        );
        assert!(editor.provider_entries.is_empty());
        editor.apply_provider_refresh(vec![changed], vec![], vec![], vec![]);
        assert_eq!(editor.provider_entries[0].id, original.id);
        editor.project.save().unwrap();
        let reopened = Project::load(&root).unwrap();
        let saved = reopened.generative_config(asset_id).unwrap();
        assert_eq!(saved.versions[0].engine_execution, Some(provenance));
        assert_eq!(
            saved.inputs["transformer_adapter_strengths"],
            inputs["transformer_adapter_strengths"]
        );
        assert_eq!(saved.versions[0].inputs_snapshot, inputs);
        let mut old_record = serde_json::to_value(&saved.versions[0]).unwrap();
        old_record
            .as_object_mut()
            .unwrap()
            .remove("engine_execution");
        assert!(serde_json::from_value::<GenerationRecord>(old_record)
            .unwrap()
            .engine_execution
            .is_none());

        mark_cached_catalog_unavailable(&mut catalog);
        let cached =
            catalog_to_provider_entries(&catalog, &EngineConnectionSettings::default()).unwrap();
        assert!(cached
            .iter()
            .all(|provider| !provider_is_available(provider)));
        assert!(matches!(
            &cached[9].connection,
            ProviderConnection::LatentSlateEngine {
                recipe: Some(_),
                ..
            }
        ));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn fixed_canvas_preflight_and_materialization_do_not_invent_hidden_inputs() {
        use crate::core::generation::{preflight_provider_config, resolve_provider_inputs};
        use crate::state::{
            Asset, GenerativeConfig, InputValue, MediaBindingSource, MediaBindingSpec, Project,
        };
        let root = std::env::temp_dir().join(format!("ls-fixed-media-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        image::RgbImage::new(256, 256)
            .save(root.join("source.png"))
            .unwrap();
        let mut project = Project::new("fixed media");
        project.project_path = Some(root.clone());
        let source_id = project.add_asset(Asset::new_image("Source", "source.png".into()));
        let mut catalog = user_recipe_catalog();
        catalog.tools[8].available = true;
        let provider =
            tool_to_provider(&catalog.tools[8], &EngineConnectionSettings::default()).unwrap();
        let image_input = provider
            .inputs
            .iter()
            .find(|field| field.input_type == ProviderInputType::Image)
            .unwrap();
        let mut config = GenerativeConfig::default();
        config.provider_id = Some(provider.id);
        config.inputs.insert(
            "prompt".into(),
            InputValue::Literal {
                value: json!("Waterfall"),
            },
        );
        config.media_bindings.insert(
            image_input.name.clone(),
            MediaBindingSpec {
                source: MediaBindingSource::ProjectAsset {
                    asset_id: source_id,
                    version: None,
                },
                ..Default::default()
            },
        );
        assert!(preflight_provider_config(&project, None, None, &provider, &config).is_empty());
        let resolved = resolve_provider_inputs(&project, None, None, &provider, &config);
        assert!(
            resolved.media_errors.is_empty(),
            "{:?}",
            resolved.media_errors
        );
        assert!(resolved.input_errors.is_empty());
        assert_eq!(
            image::image_dimensions(resolved.values[&image_input.name].as_str().unwrap()).unwrap(),
            (256, 256)
        );
        for key in ["width", "height", "duration_seconds"] {
            assert!(
                !resolved.values.contains_key(key)
                    && !resolved.snapshot.contains_key(key)
                    && !config.inputs.contains_key(key)
            );
        }
        image::RgbImage::new(512, 256)
            .save(root.join("source.png"))
            .unwrap();
        assert!(
            preflight_provider_config(&project, None, None, &provider, &config)
                .iter()
                .any(|issue| issue.message.contains("256×256 output canvas"))
        );
        assert_eq!(
            resolve_provider_inputs(&project, None, None, &provider, &config)
                .media_errors
                .len(),
            1
        );
        let mut mixed = provider.clone();
        mixed.canvas.as_mut().unwrap().fixed_height = None;
        let height = user_recipe_catalog().tools[0]
            .inputs
            .iter()
            .find(|input| input.role.as_deref() == Some("height"))
            .unwrap()
            .clone();
        mixed.inputs.push(convert_input(&height).unwrap());
        config.inputs.insert(
            height.key.clone(),
            InputValue::Literal { value: json!(320) },
        );
        image::RgbImage::new(256, 320)
            .save(root.join("source.png"))
            .unwrap();
        assert!(preflight_provider_config(&project, None, None, &mixed, &config).is_empty());
        let resolved = resolve_provider_inputs(&project, None, None, &mixed, &config);
        assert!(
            resolved.media_errors.is_empty(),
            "{:?}",
            resolved.media_errors
        );
        assert!(
            resolved.input_errors.is_empty(),
            "{:?}",
            resolved.input_errors
        );
        assert_eq!(resolved.values[&height.key], json!(320));
        assert!(!resolved.values.contains_key("width"));
        assert_eq!(
            image::image_dimensions(resolved.values[&image_input.name].as_str().unwrap()).unwrap(),
            (256, 320)
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn ordinary_submission_preserves_order_recipe_and_accepted_provenance() {
        for user_recipe in [false, true] {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let mut provider = test_engine_provider(format!("http://{address}"));
            let recipe =
                user_recipe.then(|| user_recipe_catalog().tools[9].recipe.clone().unwrap());
            if let ProviderConnection::LatentSlateEngine {
                recipe: current, ..
            } = &mut provider.connection
            {
                *current = recipe.clone();
            }
            let expected = recipe
                .clone()
                .map(|recipe| crate::state::EngineExecutionProvenance {
                    tool_id: provider.id,
                    schema_revision: 1,
                    schema_hash: "sha256:unit".into(),
                    recipe: Some(recipe),
                });
            let returned = expected.clone();
            let server = tokio::spawn(async move {
                let (mut stream, _) = listener.accept().await.unwrap();
                let request = read_mock_request(&mut stream).await;
                let body: Value =
                    serde_json::from_str(request.split_once("\r\n\r\n").unwrap().1).unwrap();
                let mut status = json!({"id":Uuid::new_v4(),"status":"succeeded","artifacts":[{"role":"primary","filename":"result.png","download_url":"/result.png"}]});
                if let Some(provenance) = returned {
                    status.as_object_mut().unwrap().extend(
                        serde_json::to_value(provenance)
                            .unwrap()
                            .as_object()
                            .unwrap()
                            .clone(),
                    );
                }
                write_mock_json(&mut stream, 200, status).await;
                let (mut stream, _) = listener.accept().await.unwrap();
                assert!(read_mock_request(&mut stream)
                    .await
                    .starts_with("GET /result.png "));
                write_mock_json(&mut stream, 200, json!("artifact bytes")).await;
                body
            });
            let inputs =
                HashMap::from([("transformer_adapter_strengths".into(), json!([0.75, 0.125]))]);
            let output = crate::providers::execute_generation(
                &provider,
                &inputs,
                ProviderOutputType::Image,
                None,
                None,
            )
            .await
            .unwrap();
            assert_eq!(output.engine_execution, expected);
            let body = server.await.unwrap();
            let mut expected_body = json!({"tool_id":provider.id,"schema_revision":1,"schema_hash":"sha256:unit","inputs":inputs});
            if let Some(recipe) = recipe {
                expected_body["recipe"] = serde_json::to_value(recipe).unwrap();
            }
            assert_eq!(body, expected_body);
        }
    }

    #[tokio::test]
    async fn stale_recipe_submission_fails_without_resubmission() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let mut provider = test_engine_provider(format!("http://{address}"));
        if let ProviderConnection::LatentSlateEngine { recipe, .. } = &mut provider.connection {
            *recipe = user_recipe_catalog().tools[9].recipe.clone();
        }
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            assert!(read_mock_request(&mut stream)
                .await
                .starts_with("POST /v1/jobs "));
            write_mock_json(
                &mut stream,
                409,
                json!({"error":{"message":"Recipe revision changed"}}),
            )
            .await;
            assert!(
                tokio::time::timeout(Duration::from_millis(250), listener.accept())
                    .await
                    .is_err()
            );
        });
        let result = crate::providers::execute_generation(
            &provider,
            &HashMap::new(),
            ProviderOutputType::Image,
            None,
            None,
        )
        .await;
        assert!(
            matches!(result, Err(ProviderExecutionError::RefreshRequired(message)) if message.contains("submit again"))
        );
        server.await.unwrap();
    }

    async fn spawn_cancel_mock(
        cancel: Arc<AtomicBool>,
        terminal_status: &'static str,
        delete_status: u16,
    ) -> (String, tokio::task::JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind mock");
        let address = listener.local_addr().expect("mock address");
        let job_id = Uuid::new_v4();
        let handle = tokio::spawn(async move {
            let expected = [
                ("POST", "/v1/jobs".to_string(), "running"),
                ("DELETE", format!("/v1/jobs/{job_id}"), "running"),
                ("GET", format!("/v1/jobs/{job_id}"), terminal_status),
            ];
            let mut trace = Vec::new();
            for (index, (method, path, status)) in expected.into_iter().enumerate() {
                let (mut stream, _) = listener.accept().await.expect("accept mock request");
                let request = read_mock_request(&mut stream).await;
                let request_line = request.lines().next().expect("request line").to_string();
                assert_eq!(request_line, format!("{method} {path} HTTP/1.1"));
                assert!(
                    request
                        .to_ascii_lowercase()
                        .contains("authorization: bearer unit-token"),
                    "request must retain Engine authentication"
                );
                trace.push(request_line);
                if index == 0 {
                    cancel.store(true, Ordering::Relaxed);
                }
                let response_status = if index == 1 { delete_status } else { 200 };
                let response_body = if index == 1 && delete_status != 200 {
                    json!({ "error": { "message": "mock delete failure" } })
                } else {
                    json!({ "id": job_id, "status": status })
                };
                write_mock_json(&mut stream, response_status, response_body).await;
            }
            trace
        });
        (format!("http://{address}"), handle)
    }

    async fn spawn_cancel_transport_failure_mock(
        cancel: Arc<AtomicBool>,
    ) -> (String, tokio::task::JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind mock");
        let address = listener.local_addr().expect("mock address");
        let job_id = Uuid::new_v4();
        let handle = tokio::spawn(async move {
            let expected = [
                ("POST", "/v1/jobs".to_string()),
                ("DELETE", format!("/v1/jobs/{job_id}")),
                ("GET", format!("/v1/jobs/{job_id}")),
            ];
            let mut trace = Vec::new();
            for (index, (method, path)) in expected.into_iter().enumerate() {
                let (mut stream, _) = listener.accept().await.expect("accept mock request");
                let request = read_mock_request(&mut stream).await;
                let request_line = request.lines().next().expect("request line").to_string();
                assert_eq!(request_line, format!("{method} {path} HTTP/1.1"));
                trace.push(request_line);
                match index {
                    0 => {
                        cancel.store(true, Ordering::Relaxed);
                        write_mock_json(
                            &mut stream,
                            200,
                            json!({ "id": job_id, "status": "running" }),
                        )
                        .await;
                    }
                    1 => drop(stream),
                    2 => {
                        write_mock_json(
                            &mut stream,
                            200,
                            json!({ "id": job_id, "status": "succeeded" }),
                        )
                        .await;
                    }
                    _ => unreachable!(),
                }
            }
            trace
        });
        (format!("http://{address}"), handle)
    }

    async fn run_test_generation(
        provider: &ProviderEntry,
        cancel: Arc<AtomicBool>,
    ) -> Result<ProviderOutput, ProviderExecutionError> {
        let base_url = match &provider.connection {
            ProviderConnection::LatentSlateEngine { base_url, .. } => base_url,
            _ => unreachable!(),
        };
        generate_output(
            provider,
            base_url,
            Some("unit-token"),
            1,
            "sha256:unit",
            true,
            None,
            &HashMap::new(),
            None,
            Some(cancel),
        )
        .await
    }

    fn test_engine_job(status: &str, progress: Option<f64>, message: Option<&str>) -> EngineJob {
        EngineJob {
            provenance: None,
            id: Uuid::nil(),
            status: status.to_string(),
            progress,
            stage: None,
            message: message.map(str::to_string),
            artifacts: Vec::new(),
            error: None,
        }
    }

    #[tokio::test]
    async fn cancellation_sends_one_authenticated_delete_then_waits_for_terminal_cancel() {
        let cancel = Arc::new(AtomicBool::new(false));
        let (base_url, server) = spawn_cancel_mock(Arc::clone(&cancel), "canceled", 200).await;
        let provider = test_engine_provider(base_url);

        let result = generate_output(
            &provider,
            match &provider.connection {
                ProviderConnection::LatentSlateEngine { base_url, .. } => base_url,
                _ => unreachable!(),
            },
            Some("unit-token"),
            1,
            "sha256:unit",
            true,
            None,
            &HashMap::new(),
            None,
            Some(cancel),
        )
        .await;

        assert!(matches!(result, Err(ProviderExecutionError::Canceled(_))));
        let trace = server.await.expect("mock server");
        assert_eq!(trace.len(), 3);
        assert_eq!(trace[0], "POST /v1/jobs HTTP/1.1");
        assert!(trace[1].starts_with("DELETE /v1/jobs/"));
        assert_eq!(trace[2].replacen("GET", "DELETE", 1), trace[1]);
    }

    #[tokio::test]
    async fn late_success_after_delete_is_canceled_without_downloading_or_publishing_output() {
        let cancel = Arc::new(AtomicBool::new(false));
        let (base_url, server) = spawn_cancel_mock(Arc::clone(&cancel), "succeeded", 200).await;
        let provider = test_engine_provider(base_url);

        let result = generate_output(
            &provider,
            match &provider.connection {
                ProviderConnection::LatentSlateEngine { base_url, .. } => base_url,
                _ => unreachable!(),
            },
            Some("unit-token"),
            1,
            "sha256:unit",
            true,
            None,
            &HashMap::new(),
            None,
            Some(cancel),
        )
        .await;

        assert!(matches!(
            result,
            Err(ProviderExecutionError::Canceled(message))
                if message.contains("completed before the cancellation request")
        ));
        let trace = server.await.expect("mock server");
        assert_eq!(
            trace.len(),
            3,
            "late success must not trigger artifact download"
        );
        assert!(trace
            .iter()
            .all(|request| !request.starts_with("GET /v1/artifacts/")));
    }

    async fn assert_failed_delete_is_preserved_on_terminal_success(delete_status: u16) {
        let cancel = Arc::new(AtomicBool::new(false));
        let (base_url, server) =
            spawn_cancel_mock(Arc::clone(&cancel), "succeeded", delete_status).await;
        let provider = test_engine_provider(base_url);

        let result = run_test_generation(&provider, cancel).await;
        let status = reqwest::StatusCode::from_u16(delete_status).expect("mock status");

        assert!(matches!(
            result,
            Err(ProviderExecutionError::Error(message))
                if message.contains(&format!("failed ({status})"))
                    && message.contains("mock delete failure")
        ));
        assert_eq!(server.await.expect("mock server").len(), 3);
    }

    #[tokio::test]
    async fn unauthorized_delete_then_terminal_success_preserves_the_delete_error() {
        assert_failed_delete_is_preserved_on_terminal_success(401).await;
    }

    #[tokio::test]
    async fn server_error_delete_then_terminal_success_preserves_the_delete_error() {
        assert_failed_delete_is_preserved_on_terminal_success(500).await;
    }

    #[tokio::test]
    async fn transport_failed_delete_then_terminal_success_preserves_transport_error() {
        let cancel = Arc::new(AtomicBool::new(false));
        let (base_url, server) = spawn_cancel_transport_failure_mock(Arc::clone(&cancel)).await;
        let provider = test_engine_provider(base_url);

        let result = run_test_generation(&provider, cancel).await;

        assert!(matches!(
            result,
            Err(ProviderExecutionError::Offline(_)) | Err(ProviderExecutionError::Error(_))
        ));
        assert_eq!(server.await.expect("mock server").len(), 3);
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
    fn engine_progress_supports_overall_only_and_optional_stage_detail() {
        let overall = engine_job_provider_progress(&test_engine_job("running", Some(0.45), None))
            .expect("overall progress");
        assert_eq!(overall.overall.expect("lane").progress, 0.45);
        assert!(overall.stage.is_none());

        let mut staged = test_engine_job("running", Some(0.45), None);
        staged.stage = Some(EngineJobStage {
            label: "Low-noise sampling".to_string(),
            progress: Some(0.5),
            detail: Some("Step 1 of 2".to_string()),
        });
        let staged = engine_job_provider_progress(&staged).expect("staged progress");
        let stage = staged.stage.expect("stage");
        assert_eq!(stage.label, "Low-noise sampling");
        assert_eq!(stage.progress, Some(0.5));
        assert_eq!(stage.detail.as_deref(), Some("Step 1 of 2"));
    }

    #[test]
    fn engine_job_log_snapshot_classifies_safe_phase_messages_without_retaining_them() {
        let snapshot = engine_job_log_snapshot(&test_engine_job(
            "running",
            Some(0.0),
            Some("Materializing source media at C:\\private\\clip.mp4"),
        ));

        assert_eq!(snapshot.phase, Some(EngineJobPhase::Materialization));
        assert_eq!(
            format_engine_job_transition(&snapshot),
            "status=running, phase=materialization, progress=0%"
        );
        assert!(!format_engine_job_transition(&snapshot).contains("clip.mp4"));

        let hostile = engine_job_log_snapshot(&test_engine_job(
            "running\nsecret=C:\\private\\token.txt",
            Some(0.0),
            Some("prompt: private scene"),
        ));
        assert_eq!(
            format_engine_job_transition(&hostile),
            "status=unknown, progress=0%"
        );

        assert_eq!(
            engine_job_phase(Some("Validating request")),
            Some(EngineJobPhase::Validation)
        );
        assert_eq!(
            engine_job_phase(Some("Starting worker")),
            Some(EngineJobPhase::WorkerStart)
        );
        assert_eq!(
            engine_job_phase(Some("Importing inputs")),
            Some(EngineJobPhase::Import)
        );
        assert_eq!(
            engine_job_phase(Some("Inspecting LTX transformer artifact")),
            Some(EngineJobPhase::Validation)
        );
        assert_eq!(
            engine_job_phase(Some("Building LTX transformer shell")),
            Some(EngineJobPhase::Preparation)
        );
    }

    #[test]
    fn engine_job_log_snapshots_dedupe_polls_and_bucket_progress() {
        let base =
            engine_job_log_snapshot(&test_engine_job("running", Some(0.201), Some("Generating")));
        let same_bucket = engine_job_log_snapshot(&test_engine_job(
            "running",
            Some(0.249),
            Some("Generating frame"),
        ));
        let next_bucket = engine_job_log_snapshot(&test_engine_job(
            "running",
            Some(0.25),
            Some("Generating frame"),
        ));
        let next_phase = engine_job_log_snapshot(&test_engine_job(
            "running",
            Some(0.25),
            Some("Encoding output"),
        ));

        assert_eq!(base, same_bucket);
        assert_ne!(base, next_bucket);
        assert_ne!(next_bucket, next_phase);
        assert_eq!(
            format_engine_job_transition(&next_phase),
            "status=running, phase=encoding, progress=25%"
        );
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
                "operation": "h3.i2v",
                "schema_revision": 2,
                "schema_hash": "sha256:test",
                "name": "First/Last Frame Video",
                "description": "Generate a shot.",
                "workflow_kind": "first_frame_last_frame_video",
                "output": { "type": "video" },
                "inputs": [
                    { "key": "prompt", "label": "Prompt", "type": "text", "required": true },
                    { "key": "start_image", "label": "First Frame", "type": "image", "required": true, "role": "start_image" },
                    { "key": "end_image", "label": "Last Frame", "type": "image", "required": false, "role": "end_image" },
                    { "key": "width", "label": "Width", "type": "integer", "required": true, "default": 960, "role": "width", "ui": { "min": 64, "step": 32 } },
                    { "key": "height", "label": "Height", "type": "integer", "required": true, "default": 544, "role": "height", "ui": { "min": 64, "step": 32 } },
                    { "key": "steps", "label": "Steps", "type": "integer", "required": true, "default": 20 }
                ],
                "canvas": { "alignment": 32, "min_side": 64, "max_pixels": 1032192, "max_aspect": 4.0 },
                "timing": {
                    "fps": { "mode": "fixed", "value": 16.0 },
                    "duration_seconds": { "min": 1.0, "max": 5.0, "step": 0.25 }
                },
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
        assert_eq!(provider.inputs[3].role, Some(InputRole::Width));
        assert_eq!(provider.inputs[3].default, Some(json!(960)));
        assert_eq!(provider.inputs[4].role, Some(InputRole::Height));
        assert_eq!(provider.inputs[4].default, Some(json!(544)));
        assert_eq!(
            provider.canvas,
            Some(CanvasContract {
                fixed_width: None,
                fixed_height: None,
                alignment: 32,
                min_side: 64,
                max_side: None,
                max_pixels: Some(1_032_192),
                max_aspect: Some(4.0),
            })
        );
        let timing = provider.timing.as_ref().expect("timing");
        assert_eq!(timing.fps.as_ref().expect("fps").mode, "fixed");
        assert_eq!(timing.fps.as_ref().expect("fps").value, Some(16.0));
        assert_eq!(
            timing.duration_seconds,
            Some(crate::state::ProviderDurationTiming {
                mode: None,
                value: None,
                min: 1.0,
                max: 5.0,
                step: 0.25,
                output_frame_counts: Vec::new(),
                frame_step: None,
                frame_offset: 0,
            })
        );
        assert!(matches!(
            provider.inputs[5].input_type,
            ProviderInputType::Integer
        ));
        assert_eq!(provider.inputs[5].default, Some(json!(20)));
        assert!(matches!(
            &provider.connection,
            ProviderConnection::LatentSlateEngine { .. }
        ));
    }

    #[test]
    fn established_engine_schema_identities_remain_usable() {
        let identities = [
            (
                "46bdb57c-3b19-5397-8949-4e20ffe757c9",
                "ltx23.text_to_video",
                2,
                "sha256:94f9397a5ff16d5101e81f62396c5c744f045799bcdbdf961b036ee8f0ac2c78",
                "text_to_video",
                "video",
            ),
            (
                "5d6e2d6f-216c-5f35-a4ec-1565d6e56ee7",
                "ltx23.image_to_video",
                3,
                "sha256:be3be547dd665155e162d51a5bea089cfcb0da66116c6e58c1766af04679bb24",
                "image_to_video",
                "video",
            ),
            (
                "1a8f9c0b-410e-56e4-90de-23bcb9d644ca",
                "ltx23.first_last_frame_to_video",
                3,
                "sha256:b58e76368b442ca723a0e2679db3b5b011870c4eeaba223704192d1190d9de1c",
                "first_frame_last_frame_video",
                "video",
            ),
            (
                "e7dcbbde-d58f-4354-ad36-b684b5c236f3",
                "flux2_klein9b.text_to_image",
                1,
                "sha256:2e94d609c2db43e883da19fb0c73faa1bef7f3459c916760079f7cedd212c6b3",
                "text_to_image",
                "image",
            ),
            (
                "a7489e73-3bb9-4bb9-888f-fa592c8f4430",
                "flux2_klein9b.two_image_to_image",
                1,
                "sha256:d756bc62e593edd29f3c2c909f3c92fd22d10cb2fb44a2b51bdd93afdb605ed8",
                "image_to_image",
                "image",
            ),
            (
                "34e57585-95a3-4bb6-b3de-fca5dd924ba6",
                "wan2214b_turbo.text_to_video",
                2,
                "sha256:4556b1e1b1ae9483ce25f2a90b45f0a3b709bff6e46b34b0b835507f81ef4f8e",
                "text_to_video",
                "video",
            ),
            (
                "aac35e26-08e7-400b-bf9b-dc389809ddd5",
                "wan2214b_turbo.image_to_video",
                2,
                "sha256:8c2c935669909fa6e010369137025cbffff321e4789b2966a31d761303d48426",
                "image_to_video",
                "video",
            ),
            (
                "d0c202bf-7dd5-4df8-b116-f7633dc94cfe",
                "wan2214b_turbo.first_last_frame_to_video",
                2,
                "sha256:9cf28f66f4a51f1631f4f527d26081bf72ba9644d453b1e6f65b34acbcf5601a",
                "first_frame_last_frame_video",
                "video",
            ),
        ];
        let oracle: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/engine-catalog-0ba2c66.json"
        ))
        .expect("frozen producer catalog");
        let catalog: EngineCatalog = serde_json::from_value(oracle.clone()).expect("catalog");
        let providers = catalog_to_provider_entries(&catalog, &EngineConnectionSettings::default())
            .expect("providers");
        assert_eq!(providers.len(), identities.len());
        for ((provider, (id, key, revision, hash, workflow, output)), tool) in providers
            .iter()
            .zip(identities.iter())
            .zip(oracle["tools"].as_array().expect("tools"))
        {
            assert_eq!(provider.id, Uuid::parse_str(id).expect("id"));
            assert_eq!(
                serde_json::to_value(provider.workflow_kind).unwrap(),
                *workflow
            );
            assert_eq!(serde_json::to_value(provider.output_type).unwrap(), *output);
            assert_eq!(provider.name, tool["name"].as_str().unwrap());
            assert_eq!(
                provider.description.as_deref(),
                tool["description"].as_str()
            );
            assert_eq!(
                serde_json::to_value(&provider.canvas).unwrap(),
                tool["canvas"]
            );
            assert_eq!(
                serde_json::to_value(&provider.timing).unwrap(),
                tool["timing"]
            );
            let inputs = tool["inputs"].as_array().expect("ordered inputs");
            assert_eq!(provider.inputs.len(), inputs.len());
            for (input, expected) in provider.inputs.iter().zip(inputs) {
                assert_eq!(input.name, expected["key"].as_str().unwrap());
                assert_eq!(input.label, expected["label"].as_str().unwrap());
                assert_eq!(
                    serde_json::to_value(&input.input_type).unwrap()["type"],
                    expected["type"]
                );
                assert_eq!(input.required, expected["required"].as_bool().unwrap());
                assert_eq!(
                    serde_json::to_value(input.image_dimensions).unwrap(),
                    expected["image_dimensions"]
                );
                assert_eq!(input.default.as_ref(), expected.get("default"));
                assert_eq!(serde_json::to_value(input.role).unwrap(), expected["role"]);
                if let Some(hints) = expected.get("ui") {
                    let actual =
                        serde_json::to_value(input.ui.as_ref().expect("UI hints")).unwrap();
                    for (key, value) in hints.as_object().unwrap() {
                        if value.is_number() {
                            assert_eq!(
                                actual[key].as_f64(),
                                value.as_f64(),
                                "{key} for {}",
                                input.name
                            );
                        } else {
                            assert_eq!(actual[key], *value, "{key} for {}", input.name);
                        }
                    }
                } else {
                    assert!(input.ui.is_none());
                }
            }
            let controls = crate::core::generation::generation_control_inputs(provider);
            assert_eq!(controls.variation.len(), 1);
            assert_eq!(controls.variation[0].name, "seed");
            assert!(controls.advanced.is_empty());
            assert_eq!(
                controls.normal.len(),
                inputs.len() - if *output == "video" { 4 } else { 3 }
            );
            if *output == "video" {
                assert_eq!(controls.timing.len(), 1);
                assert_eq!(controls.timing[0].name, "duration_seconds");
                let fps = if key.starts_with("ltx23.") {
                    30.0
                } else {
                    16.0
                };
                assert_eq!(
                    crate::core::generation::provider_fixed_fps(provider),
                    Some(fps)
                );
                assert_eq!(
                    crate::core::generation::reconcile_video_timing_for_provider(
                        5.0, 24.0, provider
                    ),
                    (
                        if key.starts_with("ltx23.") {
                            145.0 / 30.0
                        } else {
                            5.0
                        },
                        fps,
                        if key.starts_with("ltx23.") { 145 } else { 80 }
                    )
                );
            } else {
                assert!(controls.timing.is_empty());
            }
            assert!(matches!(
                &provider.connection,
                ProviderConnection::LatentSlateEngine {
                    tool_key,
                    schema_revision,
                    schema_hash,
                    available: true,
                    ..
                } if tool_key == key
                    && schema_revision == revision
                    && schema_hash == hash
            ));
        }
    }

    #[test]
    fn catalog_image_dimensions_are_checked_before_submission() {
        use crate::core::generation::{preflight_provider_config, resolve_provider_inputs};
        use crate::state::{
            Asset, GenerativeConfig, InputValue, MediaBindingSource, MediaBindingSpec, Project,
        };

        let catalog: EngineCatalog = serde_json::from_str(include_str!(
            "../../tests/fixtures/engine-catalog-0ba2c66.json"
        ))
        .unwrap();
        let providers =
            catalog_to_provider_entries(&catalog, &EngineConnectionSettings::default()).unwrap();
        let root =
            std::env::temp_dir().join(format!("latentslate-contract-audit-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let mut project = Project::new("LTX consumer audit");
        project.project_path = Some(root.clone());
        project.settings.width = 768;
        project.settings.height = 512;
        for (name, color) in [("first.png", [255_u8, 0, 0]), ("last.png", [0_u8, 0, 255])] {
            image::RgbImage::from_pixel(512, 512, image::Rgb(color))
                .save(root.join(name))
                .unwrap();
            project
                .assets
                .push(Asset::new_image(name, PathBuf::from(name)));
        }
        for provider in providers.iter() {
            for (case, explicit) in [
                ("A_matching", Some(512)),
                ("B_project_fallback", None),
                ("C_explicit_mismatch", Some(640)),
            ] {
                let mut config = GenerativeConfig {
                    provider_id: Some(provider.id),
                    ..Default::default()
                };
                config.inputs.insert(
                    "prompt".into(),
                    InputValue::Literal {
                        value: json!("A red cube turns blue"),
                    },
                );
                if let Some(width) = explicit {
                    config.inputs.insert(
                        "width".into(),
                        InputValue::Literal {
                            value: json!(width),
                        },
                    );
                    config
                        .inputs
                        .insert("height".into(), InputValue::Literal { value: json!(512) });
                }
                for (index, input) in provider
                    .inputs
                    .iter()
                    .filter(|input| input.input_type == ProviderInputType::Image)
                    .enumerate()
                {
                    config.media_bindings.insert(
                        input.name.clone(),
                        MediaBindingSpec {
                            source: MediaBindingSource::ProjectAsset {
                                asset_id: project.assets[index].id,
                                version: None,
                            },
                            ..Default::default()
                        },
                    );
                }
                let preflight = preflight_provider_config(&project, None, None, provider, &config);
                let resolved = resolve_provider_inputs(&project, None, None, provider, &config);
                let constrained = provider
                    .inputs
                    .iter()
                    .filter(|input| input.image_dimensions.is_some())
                    .count();
                let expected_errors = if explicit == Some(512) {
                    0
                } else {
                    constrained
                };
                assert_eq!(
                    preflight.len(),
                    expected_errors,
                    "{} {case}: {preflight:?}",
                    provider.name
                );
                assert_eq!(
                    resolved.media_errors.len(),
                    expected_errors,
                    "{} {case}",
                    provider.name
                );
                assert!(resolved.missing_required.is_empty());
                assert!(resolved.input_errors.is_empty());
                assert_eq!(resolved.values["width"], json!(explicit.unwrap_or(768)));
                for (index, input) in provider
                    .inputs
                    .iter()
                    .filter(|input| input.input_type == ProviderInputType::Image)
                    .enumerate()
                {
                    let path = resolved.values[&input.name].as_str().unwrap();
                    assert_eq!(image::image_dimensions(path).unwrap(), (512, 512));
                    assert_eq!(
                        std::fs::read(path).unwrap(),
                        std::fs::read(root.join(if index == 0 { "first.png" } else { "last.png" }))
                            .unwrap()
                    );
                }
            }
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn canonical_tools_preserve_inputs_through_http_and_primary_download() {
        use crate::core::generation::resolve_provider_inputs;
        use crate::state::{
            Asset, GenerativeConfig, InputValue, MediaBindingSource, MediaBindingSpec, Project,
        };
        let catalog: EngineCatalog = serde_json::from_str(include_str!(
            "../../tests/fixtures/engine-catalog-0ba2c66.json"
        ))
        .unwrap();
        let providers =
            catalog_to_provider_entries(&catalog, &EngineConnectionSettings::default()).unwrap();
        let root = std::env::temp_dir().join(format!("latentslate-http-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let mut project = Project::new("HTTP contract");
        project.project_path = Some(root.clone());
        project.settings.width = 512;
        project.settings.height = 512;
        for (name, color) in [("first.png", [255_u8, 0, 0]), ("last.png", [0_u8, 0, 255])] {
            image::RgbImage::from_pixel(512, 512, image::Rgb(color))
                .save(root.join(name))
                .unwrap();
            project.assets.push(Asset::new_image(name, name.into()));
        }
        for provider in providers {
            let mut config = GenerativeConfig::default();
            config.inputs.insert(
                "prompt".into(),
                InputValue::Literal {
                    value: json!("contract prompt"),
                },
            );
            for (index, input) in provider
                .inputs
                .iter()
                .filter(|input| input.input_type == ProviderInputType::Image)
                .enumerate()
            {
                config.media_bindings.insert(
                    input.name.clone(),
                    MediaBindingSpec {
                        source: MediaBindingSource::ProjectAsset {
                            asset_id: project.assets[index].id,
                            version: None,
                        },
                        ..Default::default()
                    },
                );
            }
            let resolved = resolve_provider_inputs(&project, None, None, &provider, &config);
            assert!(resolved.media_errors.is_empty());
            let mut expected_inputs = resolved.values.clone();
            let uploads: Vec<_> = provider
                .inputs
                .iter()
                .filter(|input| input.input_type == ProviderInputType::Image)
                .map(|input| {
                    let bytes =
                        std::fs::read(resolved.values[&input.name].as_str().unwrap()).unwrap();
                    let id = Uuid::new_v4();
                    expected_inputs
                        .insert(input.name.clone(), json!({"type":"asset", "asset_id":id}));
                    (bytes, id)
                })
                .collect();
            let ProviderConnection::LatentSlateEngine {
                schema_revision,
                schema_hash,
                ..
            } = &provider.connection
            else {
                unreachable!()
            };
            let expected = json!({"tool_id":provider.id,"schema_revision":schema_revision,"schema_hash":schema_hash,"inputs":expected_inputs});
            let extension = if provider.output_type == ProviderOutputType::Image {
                "png"
            } else {
                "mp4"
            };
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let url = format!("http://{}", listener.local_addr().unwrap());
            let server = tokio::spawn(async move {
                for (bytes, id) in uploads {
                    let (mut stream, _) = listener.accept().await.unwrap();
                    let request = read_mock_request_bytes(&mut stream).await;
                    assert!(request.starts_with(b"POST /v1/assets HTTP/1.1"));
                    assert!(request.windows(bytes.len()).any(|window| window == bytes));
                    assert!(String::from_utf8_lossy(&request)
                        .contains("authorization: Bearer unit-token"));
                    write_mock_json(&mut stream, 200, json!({"id":id})).await;
                }
                let (mut stream, _) = listener.accept().await.unwrap();
                let request = read_mock_request(&mut stream).await;
                assert!(request.starts_with("POST /v1/jobs HTTP/1.1"));
                assert_eq!(
                    serde_json::from_str::<Value>(request.split_once("\r\n\r\n").unwrap().1)
                        .unwrap(),
                    expected
                );
                let job = Uuid::new_v4();
                write_mock_json(
                    &mut stream,
                    200,
                    json!({"id":job,"status":"running","progress":0.25}),
                )
                .await;
                let (mut stream, _) = listener.accept().await.unwrap();
                assert!(read_mock_request(&mut stream)
                    .await
                    .starts_with(&format!("GET /v1/jobs/{job} HTTP/1.1")));
                write_mock_json(&mut stream,200,json!({"id":job,"status":"succeeded","progress":1.0,"artifacts":[{"role":"preview","filename":"preview.png","download_url":"/wrong"},{"role":"primary","filename":format!("result.{extension}"),"download_url":"/primary"}]})).await;
                let (mut stream, _) = listener.accept().await.unwrap();
                assert!(read_mock_request(&mut stream)
                    .await
                    .starts_with("GET /primary HTTP/1.1"));
                stream
                    .write_all(
                        b"HTTP/1.1 200 OK\r\ncontent-length: 4\r\nconnection: close\r\n\r\ndone",
                    )
                    .await
                    .unwrap();
            });
            let (tx, mut rx) = mpsc::unbounded_channel();
            let output = tokio::time::timeout(
                Duration::from_secs(10),
                generate_output(
                    &provider,
                    &url,
                    Some("unit-token"),
                    *schema_revision,
                    schema_hash,
                    true,
                    None,
                    &resolved.values,
                    Some(tx),
                    None,
                ),
            )
            .await
            .unwrap()
            .unwrap();
            server.await.unwrap();
            assert_eq!(output.bytes, b"done");
            assert_eq!(output.extension, extension);
            assert!(rx.try_recv().is_ok());
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn historical_catalog_and_optional_image_constraint_round_trip() {
        let catalog: EngineCatalog = serde_json::from_str(include_str!(
            "../../tests/fixtures/engine-catalog-7df12b4.json"
        ))
        .unwrap();
        let providers =
            catalog_to_provider_entries(&catalog, &EngineConnectionSettings::default()).unwrap();
        for provider in providers {
            for input in provider.inputs {
                assert!(input.image_dimensions.is_none());
                assert!(serde_json::to_value(&input)
                    .unwrap()
                    .get("image_dimensions")
                    .is_none());
                assert_eq!(
                    serde_json::from_value::<ProviderInputField>(
                        serde_json::to_value(&input).unwrap()
                    )
                    .unwrap(),
                    input
                );
            }
        }
        let input: crate::state::ManifestInput = serde_json::from_value(json!({"name":"image", "label":"Image", "input_type":{"type":"image"}, "required":true, "image_dimensions":"match_output_canvas", "bind":{"selector":{"class_type":"LoadImage", "input_key":"image"}}})).unwrap();
        assert_eq!(
            serde_json::to_value(input).unwrap()["image_dimensions"],
            "match_output_canvas"
        );
    }

    #[test]
    #[ignore = "requires local FFmpeg; exercises real rotated-video frame extraction"]
    fn image_constraint_checks_extracted_orientation_and_each_endpoint() {
        use crate::core::generation::{preflight_provider_config, resolve_provider_inputs};
        use crate::state::{
            Asset, GenerativeConfig, InputValue, MediaBindingSource, MediaBindingSpec, Project,
        };
        let catalog: EngineCatalog = serde_json::from_str(include_str!(
            "../../tests/fixtures/engine-catalog-0ba2c66.json"
        ))
        .unwrap();
        let mut provider = catalog_to_provider_entries(&catalog, &EngineConnectionSettings::default()).unwrap().into_iter().find(|provider| matches!(&provider.connection, ProviderConnection::LatentSlateEngine {tool_key,..} if tool_key == "ltx23.first_last_frame_to_video")).unwrap();
        let root = std::env::temp_dir().join(format!("latentslate-rotation-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("rotated.mp4"),
            include_bytes!("../../tests/fixtures/rotated-640x512.mp4"),
        )
        .unwrap();
        image::RgbImage::new(512, 640)
            .save(root.join("still.png"))
            .unwrap();
        let mut project = Project::new("rotated input");
        project.project_path = Some(root.clone());
        project.settings.width = 512;
        project.settings.height = 640;
        let mut video = Asset::new_video("Rotated", "rotated.mp4".into());
        video.duration_seconds = Some(1.0);
        project.assets.push(video);
        project
            .assets
            .push(Asset::new_image("Still", "still.png".into()));
        let mut config = GenerativeConfig::default();
        config.inputs.insert(
            "prompt".into(),
            InputValue::Literal {
                value: json!("test"),
            },
        );
        let images: Vec<_> = provider
            .inputs
            .iter()
            .filter(|input| input.input_type == ProviderInputType::Image)
            .cloned()
            .collect();
        for (index, input) in images.iter().enumerate() {
            config.media_bindings.insert(
                input.name.clone(),
                MediaBindingSpec {
                    source: MediaBindingSource::ProjectAsset {
                        asset_id: project.assets[index].id,
                        version: None,
                    },
                    sample: crate::state::MediaSample::Frame {
                        at: crate::state::MediaFramePoint::SourceStart,
                    },
                    ..Default::default()
                },
            );
        }
        let issues = preflight_provider_config(&project, None, None, &provider, &config);
        assert!(issues.is_empty(), "{issues:?}");
        let resolved = resolve_provider_inputs(&project, None, None, &provider, &config);
        assert!(
            resolved.media_errors.is_empty(),
            "{:?}",
            resolved.media_errors
        );
        assert_eq!(
            image::image_dimensions(resolved.values[&images[0].name].as_str().unwrap()).unwrap(),
            (512, 640)
        );
        image::RgbImage::new(640, 512)
            .save(root.join("still.png"))
            .unwrap();
        let issues = preflight_provider_config(&project, None, None, &provider, &config);
        assert_eq!(issues.len(), 1);
        assert!(issues[0].message.starts_with(&images[1].label));
        let resolved = resolve_provider_inputs(&project, None, None, &provider, &config);
        assert_eq!(resolved.media_errors.len(), 1);
        assert!(resolved.media_errors[0].starts_with(&images[1].label));
        let first_binding = config.media_bindings[&images[0].name].clone();
        let last_binding = config.media_bindings[&images[1].name].clone();
        config
            .media_bindings
            .insert(images[0].name.clone(), last_binding);
        config
            .media_bindings
            .insert(images[1].name.clone(), first_binding);
        let resolved = resolve_provider_inputs(&project, None, None, &provider, &config);
        assert_eq!(resolved.media_errors.len(), 1);
        assert!(resolved.media_errors[0].starts_with(&images[0].label));
        provider.connection = ProviderConnection::ComfyUi {
            base_url: "http://unused".into(),
            workflow_path: None,
            manifest: None,
        };
        for input in &mut provider.inputs {
            input.image_dimensions = None;
        }
        let issues = preflight_provider_config(&project, None, None, &provider, &config);
        assert!(issues.is_empty(), "{issues:?}");
        assert!(
            resolve_provider_inputs(&project, None, None, &provider, &config)
                .media_errors
                .is_empty()
        );
        std::fs::remove_dir_all(root).unwrap();
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
                    "operation": "flux2_klein9b.t2i",
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
                    "operation": "flux2_klein9b.two_image",
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
        assert!(entries.iter().all(|provider| provider.timing.is_none()));

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
                "operation": "h3.t2v",
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

    #[test]
    fn parses_legacy_singleton_engine_json() {
        let connections = parse_engine_connections_json(
            r#"{
                "enabled": true,
                "base_url": "http://127.0.0.1:8765/",
                "api_key": null,
                "catalog_timeout_ms": 800
            }"#,
        )
        .expect("legacy engine.json");
        assert_eq!(connections.len(), 1);
        assert_eq!(connections[0].id, default_connection_id());
        assert_eq!(connections[0].name, "LatentSlate Engine");
        assert_eq!(connections[0].base_url, "http://127.0.0.1:8765");
        assert!(connections[0].enabled);
    }

    #[test]
    fn parses_multiple_engine_connections() {
        let connections = parse_engine_connections_json(
            r#"{
                "connections": [
                    {
                        "id": "6c617465-6e74-736c-6174-650000000001",
                        "name": "LatentSlate Engine",
                        "enabled": true,
                        "base_url": "http://127.0.0.1:8765"
                    },
                    {
                        "name": "Studio Engine",
                        "enabled": false,
                        "base_url": "https://engine.example.test/"
                    }
                ]
            }"#,
        )
        .expect("multi engine.json");
        assert_eq!(connections.len(), 2);
        assert_eq!(connections[0].id, default_connection_id());
        assert_eq!(connections[1].name, "Studio Engine");
        assert_eq!(connections[1].base_url, "https://engine.example.test");
        assert!(!connections[1].enabled);
        assert_ne!(connections[0].id, connections[1].id);
    }

    #[test]
    fn empty_connections_array_is_preserved() {
        let connections =
            parse_engine_connections_json(r#"{ "connections": [] }"#).expect("empty connections");
        assert!(connections.is_empty());
    }

    #[test]
    fn names_additional_engine_connections_uniquely() {
        let existing = vec![EngineConnectionSettings::default()];
        let second = new_engine_connection(&existing);
        assert_eq!(second.name, "LatentSlate Engine 2");
        assert_ne!(second.id, existing[0].id);
    }

    #[test]
    fn default_catalog_cache_stays_on_legacy_path() {
        let path = catalog_cache_path();
        assert_eq!(
            path.file_name().and_then(|name| name.to_str()),
            Some("engine_catalog.json")
        );
        let extra = catalog_cache_path_for(Uuid::new_v4());
        assert!(extra.to_string_lossy().contains("engine_catalogs"));
    }

    #[test]
    fn catalog_failures_preserve_actionable_categories() {
        assert_eq!(
            engine_catalog_failure_kind("Engine catalog request failed (401 Unauthorized)"),
            EngineCatalogFailureKind::CredentialsRejected
        );
        assert_eq!(
            engine_catalog_failure_kind("Engine catalog request failed: connection timed out"),
            EngineCatalogFailureKind::Unreachable
        );
        assert_eq!(
            engine_catalog_failure_kind("Engine catalog response was invalid: bad JSON"),
            EngineCatalogFailureKind::InvalidResponse
        );
    }
}
