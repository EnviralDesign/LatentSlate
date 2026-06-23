use std::collections::HashMap;

use serde_json::Value;
use tokio::sync::mpsc;

use crate::state::{ProviderConnection, ProviderEntry, ProviderOutputType};

mod cloud;
pub mod comfyui;
pub mod openai;
pub mod xai;

#[derive(Debug, Clone)]
pub struct ProviderOutput {
    pub bytes: Vec<u8>,
    pub extension: String,
}

#[derive(Debug, Clone, Copy)]
pub struct ProviderProgress {
    pub overall: Option<f32>,
    pub node: Option<f32>,
}

impl ProviderProgress {
    pub fn overall(value: f32) -> Self {
        Self {
            overall: Some(value),
            node: None,
        }
    }

    pub fn node(value: f32) -> Self {
        Self {
            overall: None,
            node: Some(value),
        }
    }
}

#[derive(Debug)]
pub enum ProviderExecutionError {
    Offline(String),
    Error(String),
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
