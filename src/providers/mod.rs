use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use futures_util::future::join_all;
use serde::Serialize;
use serde_json::Value;
use tokio::sync::mpsc;

use crate::state::{ProviderConnection, ProviderEntry, ProviderOutputType};

mod cloud;
pub mod comfyui;
pub mod latentslate_engine;
pub mod openai;
pub mod xai;

#[derive(Debug, Clone)]
pub struct ProviderOutput {
    pub bytes: Vec<u8>,
    pub extension: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProviderProgress {
    pub overall: Option<ProviderProgressLane>,
    pub stage: Option<ProviderProgressStage>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProviderProgressLane {
    pub label: String,
    pub progress: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProviderProgressStage {
    pub label: String,
    pub progress: Option<f32>,
    pub detail: Option<String>,
}

impl ProviderProgress {
    pub fn overall(value: f32) -> Self {
        Self::labeled_overall("Overall", value)
    }

    pub fn labeled_overall(label: impl Into<String>, value: f32) -> Self {
        Self {
            overall: Some(ProviderProgressLane {
                label: label.into(),
                progress: value,
            }),
            stage: None,
        }
    }

    pub fn stage(label: impl Into<String>, progress: Option<f32>, detail: Option<String>) -> Self {
        Self {
            overall: None,
            stage: Some(ProviderProgressStage {
                label: label.into(),
                progress,
                detail,
            }),
        }
    }
}

#[derive(Debug)]
pub enum ProviderExecutionError {
    Offline(String),
    Error(String),
    Canceled(String),
}

#[derive(Debug, Clone, Serialize)]
pub struct ProviderResourceReleaseTargetResult {
    pub kind: String,
    pub base_url: String,
    pub provider_ids: Vec<uuid::Uuid>,
    pub provider_names: Vec<String>,
    pub ok: bool,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response: Option<Value>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProviderResourceReleaseReport {
    pub attempted: usize,
    pub released: usize,
    pub failed: usize,
    pub unsupported_providers: usize,
    pub targets: Vec<ProviderResourceReleaseTargetResult>,
}

impl ProviderResourceReleaseReport {
    pub fn status_message(&self) -> String {
        if self.attempted == 0 {
            return "No configured providers support resource release.".to_string();
        }
        if self.failed == 0 {
            return format!(
                "Released cached provider resources for {} backend{}.",
                self.released,
                if self.released == 1 { "" } else { "s" }
            );
        }
        let first_failure = self
            .targets
            .iter()
            .find(|target| !target.ok)
            .map(|target| target.message.as_str())
            .unwrap_or("Unknown provider error.");
        if self.released == 0 {
            format!(
                "Provider resource release failed for {} backend{}: {}",
                self.failed,
                if self.failed == 1 { "" } else { "s" },
                first_failure
            )
        } else {
            format!(
                "Released resources for {} of {} backends; {} failed: {}",
                self.released, self.attempted, self.failed, first_failure
            )
        }
    }
}

#[derive(Debug, Clone)]
enum ProviderResourceReleaseConnection {
    ComfyUi {
        base_url: String,
    },
    LatentSlateEngine {
        base_url: String,
        api_key: Option<String>,
    },
}

#[derive(Debug, Clone)]
struct ProviderResourceReleaseTarget {
    kind: &'static str,
    base_url: String,
    provider_ids: Vec<uuid::Uuid>,
    provider_names: Vec<String>,
    connection: ProviderResourceReleaseConnection,
    configuration_error: Option<String>,
}

pub fn provider_resource_release_target_count(providers: &[ProviderEntry]) -> usize {
    provider_resource_release_targets(providers).0.len()
}

pub async fn release_provider_resources(
    providers: &[ProviderEntry],
) -> ProviderResourceReleaseReport {
    let (targets, unsupported_providers) = provider_resource_release_targets(providers);
    let attempted = targets.len();
    let targets = join_all(targets.into_iter().map(release_provider_resource_target)).await;
    let released = targets.iter().filter(|target| target.ok).count();
    ProviderResourceReleaseReport {
        attempted,
        released,
        failed: attempted.saturating_sub(released),
        unsupported_providers,
        targets,
    }
}

fn provider_resource_release_targets(
    providers: &[ProviderEntry],
) -> (Vec<ProviderResourceReleaseTarget>, usize) {
    let mut targets = BTreeMap::<String, ProviderResourceReleaseTarget>::new();
    let mut unsupported_providers = 0;
    for provider in providers {
        let (key, kind, base_url, connection) = match &provider.connection {
            ProviderConnection::ComfyUi { base_url, .. } => {
                let base_url = base_url.trim().trim_end_matches('/').to_string();
                (
                    provider_resource_release_target_key("comfy_ui", &base_url),
                    "comfy_ui",
                    base_url.clone(),
                    ProviderResourceReleaseConnection::ComfyUi { base_url },
                )
            }
            ProviderConnection::LatentSlateEngine {
                base_url, api_key, ..
            } => {
                let base_url = latentslate_engine::normalize_engine_base_url(base_url);
                (
                    provider_resource_release_target_key("latentslate_engine", &base_url),
                    "latentslate_engine",
                    base_url.clone(),
                    ProviderResourceReleaseConnection::LatentSlateEngine {
                        base_url,
                        api_key: api_key.clone(),
                    },
                )
            }
            _ => {
                unsupported_providers += 1;
                continue;
            }
        };
        let target = targets
            .entry(key)
            .or_insert_with(|| ProviderResourceReleaseTarget {
                kind,
                base_url,
                provider_ids: Vec::new(),
                provider_names: Vec::new(),
                connection,
                configuration_error: None,
            });
        if let (
            ProviderResourceReleaseConnection::LatentSlateEngine {
                api_key: target_key,
                ..
            },
            ProviderConnection::LatentSlateEngine { api_key, .. },
        ) = (&mut target.connection, &provider.connection)
        {
            let existing_key = target_key
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty());
            let incoming_key = api_key
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty());
            match (existing_key, incoming_key) {
                (None, Some(_)) => *target_key = api_key.clone(),
                (Some(existing), Some(incoming)) if existing != incoming => {
                    target.configuration_error = Some(format!(
                        "Conflicting credentials are configured for LatentSlate Engine backend {}.",
                        target.base_url
                    ));
                }
                _ => {}
            }
        }
        target.provider_ids.push(provider.id);
        target.provider_names.push(provider.name.clone());
    }
    (targets.into_values().collect(), unsupported_providers)
}

fn provider_resource_release_target_key(kind: &str, base_url: &str) -> String {
    let trimmed = base_url.trim().trim_end_matches('/');
    let canonical = reqwest::Url::parse(trimmed)
        .map(|url| url.to_string().trim_end_matches('/').to_string())
        .unwrap_or_else(|_| trimmed.to_string());
    format!("{kind}:{canonical}")
}

async fn release_provider_resource_target(
    target: ProviderResourceReleaseTarget,
) -> ProviderResourceReleaseTargetResult {
    let result = match target.configuration_error.as_ref() {
        Some(message) => Err(message.clone()),
        None => match &target.connection {
            ProviderResourceReleaseConnection::ComfyUi { base_url } => {
                comfyui::release_resources(base_url).await
            }
            ProviderResourceReleaseConnection::LatentSlateEngine { base_url, api_key } => {
                latentslate_engine::release_resources(base_url, api_key.as_deref()).await
            }
        },
    };
    match result {
        Ok(response) => ProviderResourceReleaseTargetResult {
            kind: target.kind.to_string(),
            base_url: target.base_url,
            provider_ids: target.provider_ids,
            provider_names: target.provider_names,
            ok: true,
            message: "Cached models and memory released.".to_string(),
            response: Some(response),
        },
        Err(message) => ProviderResourceReleaseTargetResult {
            kind: target.kind.to_string(),
            base_url: target.base_url,
            provider_ids: target.provider_ids,
            provider_names: target.provider_names,
            ok: false,
            message,
            response: None,
        },
    }
}

pub async fn test_provider_connection(
    provider: &ProviderEntry,
    live: bool,
) -> Result<Value, String> {
    match provider.connection.clone() {
        ProviderConnection::ComfyUi {
            base_url,
            workflow_path,
            manifest,
        } => {
            let workflow_path = comfyui::resolve_workflow_path(workflow_path.as_deref());
            if live {
                comfyui::check_health(&base_url).await?;
            }
            Ok(serde_json::json!({
                "provider_id": provider.id,
                "name": provider.name,
                "kind": "comfy_ui",
                "live": live,
                "ok": true,
                "base_url": base_url,
                "workflow_path": workflow_path,
                "workflow_exists": workflow_path.is_file(),
                "manifest_embedded": manifest.is_some(),
            }))
        }
        ProviderConnection::OpenAiImage {
            api_key,
            model,
            base_url,
        } => {
            test_cloud_provider(
                "openai_image",
                &provider.name,
                provider.id,
                api_key.as_deref(),
                &model,
                base_url.as_deref().unwrap_or("https://api.openai.com/v1"),
                live,
            )
            .await
        }
        ProviderConnection::XaiImage {
            api_key,
            model,
            base_url,
        } => {
            test_cloud_provider(
                "xai_image",
                &provider.name,
                provider.id,
                api_key.as_deref(),
                &model,
                base_url.as_deref().unwrap_or("https://api.x.ai/v1"),
                live,
            )
            .await
        }
        ProviderConnection::XaiVideo {
            api_key,
            model,
            base_url,
        } => {
            test_cloud_provider(
                "xai_video",
                &provider.name,
                provider.id,
                api_key.as_deref(),
                &model,
                base_url.as_deref().unwrap_or("https://api.x.ai/v1"),
                live,
            )
            .await
        }
        ProviderConnection::LatentSlateEngine {
            base_url, api_key, ..
        } => {
            latentslate_engine::test_connection(provider, &base_url, api_key.as_deref(), live).await
        }
        ProviderConnection::CustomHttp { .. } => {
            Err("Custom HTTP providers are planned but not implemented yet.".to_string())
        }
    }
}

pub async fn execute_generation(
    provider: &ProviderEntry,
    inputs: &HashMap<String, Value>,
    output_type: ProviderOutputType,
    progress_tx: Option<mpsc::UnboundedSender<ProviderProgress>>,
    cancel_token: Option<Arc<AtomicBool>>,
) -> Result<ProviderOutput, ProviderExecutionError> {
    match provider.connection.clone() {
        ProviderConnection::ComfyUi {
            base_url,
            workflow_path,
            manifest,
        } => {
            let workflow_path = comfyui::resolve_workflow_path(workflow_path.as_deref());
            if let Err(err) = comfyui::check_health(&base_url).await {
                return Err(ProviderExecutionError::Offline(err));
            }
            match comfyui::generate_output(
                &base_url,
                &workflow_path,
                inputs,
                manifest.as_ref(),
                output_type,
                progress_tx,
            )
            .await
            {
                Ok(output) => Ok(ProviderOutput {
                    bytes: output.bytes,
                    extension: output.extension,
                }),
                Err(err) => {
                    if let Err(health_err) = comfyui::check_health(&base_url).await {
                        Err(ProviderExecutionError::Offline(health_err))
                    } else {
                        Err(ProviderExecutionError::Error(err))
                    }
                }
            }
        }
        ProviderConnection::OpenAiImage {
            api_key,
            model,
            base_url,
        } => openai::generate_image(
            api_key.as_deref(),
            &model,
            base_url.as_deref(),
            inputs,
            progress_tx,
        )
        .await
        .map_err(ProviderExecutionError::Error),
        ProviderConnection::XaiImage {
            api_key,
            model,
            base_url,
        } => xai::generate_image(
            api_key.as_deref(),
            &model,
            base_url.as_deref(),
            inputs,
            progress_tx,
        )
        .await
        .map_err(ProviderExecutionError::Error),
        ProviderConnection::XaiVideo {
            api_key,
            model,
            base_url,
        } => xai::generate_video(
            api_key.as_deref(),
            &model,
            base_url.as_deref(),
            inputs,
            progress_tx,
        )
        .await
        .map_err(ProviderExecutionError::Error),
        ProviderConnection::LatentSlateEngine {
            base_url,
            api_key,
            schema_revision,
            schema_hash,
            available,
            unavailable_reason,
            ..
        } => {
            latentslate_engine::generate_output(
                provider,
                &base_url,
                api_key.as_deref(),
                schema_revision,
                &schema_hash,
                available,
                unavailable_reason.as_deref(),
                inputs,
                progress_tx,
                cancel_token,
            )
            .await
        }
        ProviderConnection::CustomHttp { .. } => Err(ProviderExecutionError::Error(
            "Custom HTTP providers are planned but not implemented yet.".to_string(),
        )),
    }
}

async fn test_cloud_provider(
    kind: &str,
    provider_name: &str,
    provider_id: uuid::Uuid,
    api_key: Option<&str>,
    model: &str,
    base_url: &str,
    live: bool,
) -> Result<Value, String> {
    let mut model_seen = None;
    let api_key_present = api_key
        .map(str::trim)
        .is_some_and(|value| !value.is_empty());
    if live {
        let api_key = api_key
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| format!("{kind} provider JSON is missing connection.api_key."))?;
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(8))
            .build()
            .map_err(|err| format!("Failed to build HTTP client: {err}"))?;
        let response = client
            .get(format!("{}/models", base_url.trim_end_matches('/')))
            .bearer_auth(api_key)
            .send()
            .await
            .map_err(|err| format!("{kind} provider test failed: {err}"))?;
        let status = response.status();
        let text = response
            .text()
            .await
            .map_err(|err| format!("{kind} provider test response read failed: {err}"))?;
        if !status.is_success() {
            return Err(format!("{kind} provider test failed ({status}): {text}"));
        }
        let payload: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
        model_seen = payload
            .get("data")
            .and_then(|value| value.as_array())
            .map(|models| {
                models.iter().any(|model_value| {
                    model_value
                        .get("id")
                        .and_then(|value| value.as_str())
                        .map(|id| id == model)
                        .unwrap_or(false)
                })
            });
    }

    Ok(serde_json::json!({
        "provider_id": provider_id,
        "name": provider_name,
        "kind": kind,
        "live": live,
        "ok": true,
        "base_url": base_url,
        "api_key_present": api_key_present,
        "model": model,
        "model_seen": model_seen,
    }))
}
