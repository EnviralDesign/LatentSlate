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
    ProviderInputType, ProviderOutputType, ProviderWorkflowKind,
};

use super::{ProviderExecutionError, ProviderOutput, ProviderProgress};

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

    let inputs = tool
        .inputs
        .iter()
        .map(convert_input)
        .collect::<Result<Vec<_>, _>>()?;
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
        if let Some(progress) = job.progress {
            if let Some(tx) = progress_tx.as_ref() {
                let _ = tx.send(ProviderProgress::overall(progress.clamp(0.0, 1.0) as f32));
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
                ))
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

    check_canceled(cancel_token.as_deref())?;
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
    Ok(ProviderOutput { bytes, extension })
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
    Err(ProviderExecutionError::Error(format!(
        "{context} failed ({status}): {message}"
    )))
}

fn provider_error_message(error: ProviderExecutionError) -> String {
    match error {
        ProviderExecutionError::Offline(message)
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
    previous.status != next.status || previous.message != next.message || progress_changed
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
    name: String,
    #[serde(default)]
    description: Option<String>,
    workflow_kind: String,
    output: EngineOutput,
    #[serde(default)]
    inputs: Vec<EngineInput>,
    #[serde(default)]
    canvas: Option<CanvasContract>,
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
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};

    fn test_engine_provider(base_url: String) -> ProviderEntry {
        ProviderEntry::new(
            "Engine test",
            ProviderOutputType::Image,
            ProviderConnection::LatentSlateEngine {
                base_url,
                api_key: Some("unit-token".to_string()),
                tool_key: "unit.test".to_string(),
                schema_revision: 1,
                schema_hash: "sha256:unit".to_string(),
                available: true,
                unavailable_reason: None,
            },
        )
    }

    async fn read_mock_request(stream: &mut TcpStream) -> String {
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
        String::from_utf8(bytes).expect("UTF-8 HTTP request")
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
            id: Uuid::nil(),
            status: status.to_string(),
            progress,
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

        assert!(matches!(
            result,
            Err(ProviderExecutionError::Error(message))
                if message.contains(&format!("failed ({delete_status})"))
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
                alignment: 32,
                min_side: 64,
                max_side: None,
                max_pixels: Some(1_032_192),
                max_aspect: Some(4.0),
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
